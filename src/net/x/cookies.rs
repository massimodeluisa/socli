//! Import an X session from a local Chromium-family browser.
//!
//! macOS stores cookie values as `v10` plus AES-128-CBC (PBKDF2-HMAC-SHA1, salt `saltysalt`,
//! 1003 rounds, the Safe Storage password). Current Chrome builds then prepend 32 bytes that
//! are not the cookie. The plaintext starts at the first printable byte after that prefix.

use std::path::{Path, PathBuf};
use std::process::Command;

use aes::Aes128;
use cbc::Decryptor;
use cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;

type Aes128CbcDec = Decryptor<Aes128>;

const SALT: &[u8] = b"saltysalt";
const ROUNDS: u32 = 1003;
const IV: [u8; 16] = [0x20; 16];
const PREFIX_LEN: usize = 32;

struct KnownBrowser {
    slug: &'static str,
    label: &'static str,
    /// Path under the home directory.
    root: &'static str,
    service: &'static str,
    account: &'static str,
}

const BROWSERS: &[KnownBrowser] = &[
    KnownBrowser {
        slug: "brave",
        label: "Brave",
        root: "Library/Application Support/BraveSoftware/Brave-Browser",
        service: "Brave Safe Storage",
        account: "Brave",
    },
    KnownBrowser {
        slug: "chrome",
        label: "Chrome",
        root: "Library/Application Support/Google/Chrome",
        service: "Chrome Safe Storage",
        account: "Chrome",
    },
    KnownBrowser {
        slug: "chromium",
        label: "Chromium",
        root: "Library/Application Support/Chromium",
        service: "Chromium Safe Storage",
        account: "Chromium",
    },
    KnownBrowser {
        slug: "edge",
        label: "Edge",
        root: "Library/Application Support/Microsoft Edge",
        service: "Microsoft Edge Safe Storage",
        account: "Microsoft Edge",
    },
    KnownBrowser {
        slug: "arc",
        label: "Arc",
        root: "Library/Application Support/Arc/User Data",
        service: "Arc Safe Storage",
        account: "Arc",
    },
];

pub struct BrowserSession {
    pub browser: String,
    pub profile: String,
    pub auth_token: String,
    pub ct0: String,
    /// A user id some cookie rows carry. Sign-in does not trust it. The home page names the account.
    #[allow(dead_code)]
    pub user_id: Option<String>,
}

struct Candidate {
    browser: &'static KnownBrowser,
    profile_dir: String,
    display_name: String,
    db: PathBuf,
}

/// One Chromium profile that already holds an X session. The cookies stay encrypted until
/// [`read_session`].
pub(crate) struct FoundSession {
    candidate: Candidate,
}

impl FoundSession {
    pub(crate) fn browser_label(&self) -> &str {
        self.candidate.browser.label
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.candidate.display_name
    }

    pub(crate) fn profile_dir(&self) -> &str {
        &self.candidate.profile_dir
    }
}

/// Profiles that hold an X `auth_token`. An empty list means none were readable. A profile macOS
/// will not let us read is an error, not an empty list.
pub(crate) fn find_sessions(
    browser: Option<&str>,
    profile: Option<&str>,
) -> Result<Vec<FoundSession>, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| {
        "no HOME set, so socli cannot find a browser profile. set X_AUTH_TOKEN and X_CT0, or run `socli login x --oauth`.".to_string()
    })?;
    let mut found = Vec::new();
    let mut blocked = None;
    for known in BROWSERS {
        if let Some(want) = browser {
            if !same_name(want, known.slug) && !same_name(want, known.label) {
                continue;
            }
        }
        let root = PathBuf::from(&home).join(known.root);
        match browser_present(&root) {
            Ok(false) => continue,
            Err(err) => {
                blocked.get_or_insert_with(|| read_error(known.label, &err));
                continue;
            }
            Ok(true) => {}
        }
        let (candidates, child_block) = match profiles(known, &root) {
            Ok(pair) => pair,
            Err(err) => {
                blocked.get_or_insert(err);
                continue;
            }
        };
        if let Some(err) = child_block {
            blocked.get_or_insert(err);
        }
        for candidate in candidates {
            if let Some(want) = profile {
                let dir_match = same_name(want, &candidate.profile_dir);
                let name_match = same_name(want, &candidate.display_name);
                if !dir_match && !name_match {
                    continue;
                }
            }
            match has_session(&candidate.db) {
                Ok(true) => found.push(candidate),
                Ok(false) => {}
                Err(err) => {
                    blocked.get_or_insert(err);
                }
            }
        }
    }
    if found.is_empty() {
        if let Some(err) = blocked {
            return Err(err);
        }
    }
    found.sort_by(|left, right| {
        left.browser
            .label
            .cmp(right.browser.label)
            .then(left.display_name.cmp(&right.display_name))
            .then(left.profile_dir.cmp(&right.profile_dir))
    });
    Ok(found
        .into_iter()
        .map(|candidate| FoundSession { candidate })
        .collect())
}

/// The signed-in browser session matching `browser` and `profile`.
///
/// One match is used as-is. Several matches are an error: `socli login x` asks which account.
pub fn import(browser: Option<&str>, profile: Option<&str>) -> Result<BrowserSession, String> {
    let mut found = find_sessions(browser, profile)?;
    if found.is_empty() {
        return Err(none_found(browser, profile));
    }
    if found.len() > 1 {
        return Err(
            "several X sessions are signed in. run `socli login x` and pick one, or pass --browser and --profile."
                .into(),
        );
    }
    read_session(&found.remove(0))
}

/// Decrypts one profile's X cookies. The keychain prompt happens here, not while listing.
pub(crate) fn read_session(chosen: &FoundSession) -> Result<BrowserSession, String> {
    let chosen = &chosen.candidate;
    let password = safe_storage_password(chosen.browser)?;
    let mut key = [0u8; 16];
    pbkdf2_hmac::<Sha1>(password.as_bytes(), SALT, ROUNDS, &mut key);
    let rows = cookie_rows(&chosen.db)?;
    let mut auth_token = None;
    let mut ct0 = None;
    let mut user_id = None;
    for (name, blob) in rows {
        let value = decrypt_v10(&blob, &key).map_err(|_| {
            format!(
                "could not decrypt the X cookies in {} ({}). sign in at https://x.com in that browser, or set X_AUTH_TOKEN and X_CT0.",
                chosen.browser.label, chosen.display_name
            )
        })?;
        match name.as_str() {
            "auth_token" => auth_token = Some(value),
            "ct0" => ct0 = Some(value),
            "twid" => user_id = user_id_from_twid(&value),
            _ => {}
        }
    }
    let auth_token = auth_token.filter(|v| cookie_ok(v)).ok_or_else(|| {
        format!(
            "no X session in {} ({}). sign in at https://x.com in that browser and run `socli login x` again.",
            chosen.browser.label, chosen.display_name
        )
    })?;
    let ct0 = ct0.filter(|v| cookie_ok(v)).ok_or_else(|| {
        format!(
            "{} ({}) has an X session without a csrf cookie. reload https://x.com and run `socli login x` again.",
            chosen.browser.label, chosen.display_name
        )
    })?;
    Ok(BrowserSession {
        browser: chosen.browser.label.to_string(),
        profile: chosen.display_name.clone(),
        auth_token,
        ct0,
        user_id,
    })
}

fn none_found(browser: Option<&str>, profile: Option<&str>) -> String {
    let where_ = match (browser, profile) {
        (Some(browser), Some(profile)) => format!(" in {browser} profile {profile}"),
        (Some(browser), None) => format!(" in {browser}"),
        _ => String::new(),
    };
    format!(
        "no X session{where_} in Chrome, Brave, Edge, Arc or Chromium.\n\
sign in at https://x.com in one of those browsers, then run `socli login x` again.\n\
or set X_AUTH_TOKEN and X_CT0.\n\
or run `socli login x --oauth` with a developer app."
    )
}

/**
`Path::is_dir` reports a folder macOS will not let us read as missing. Sign-in then said
there was no X session.
*/
fn browser_present(root: &Path) -> Result<bool, std::io::Error> {
    match std::fs::metadata(root) {
        Ok(meta) => Ok(meta.is_dir()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

fn read_error(label: &str, err: &std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        format!(
            "could not read {label} ({err}). allow the terminal under Privacy & Security > Full Disk Access, then run `socli login x` again."
        )
    } else {
        format!("could not read {label} ({err}). run `socli login x` again.")
    }
}

/// Profiles that have a cookie database, plus a read error if one profile could not be opened.
fn profiles(
    browser: &'static KnownBrowser,
    root: &Path,
) -> Result<(Vec<Candidate>, Option<String>), String> {
    let mut out = Vec::new();
    let mut blocked = None;
    let entries = std::fs::read_dir(root).map_err(|err| read_error(browser.label, &err))?;
    for entry in entries.flatten() {
        let path = entry.path();
        match browser_present(&path) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(err) => {
                blocked.get_or_insert_with(|| read_error(browser.label, &err));
                continue;
            }
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let db = match cookie_db(&path) {
            Ok(Some(db)) => db,
            Ok(None) => continue,
            Err(err) => {
                blocked.get_or_insert_with(|| read_error(browser.label, &err));
                continue;
            }
        };
        out.push(Candidate {
            browser,
            profile_dir: name.to_string(),
            display_name: profile_name(&path).unwrap_or_else(|| name.to_string()),
            db,
        });
    }
    Ok((out, blocked))
}

fn cookie_db(profile: &Path) -> Result<Option<PathBuf>, std::io::Error> {
    for candidate in [
        profile.join("Cookies"),
        profile.join("Network").join("Cookies"),
    ] {
        match std::fs::metadata(&candidate) {
            Ok(meta) if meta.is_file() => return Ok(Some(candidate)),
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(None)
}

fn profile_name(profile: &Path) -> Option<String> {
    let text = std::fs::read_to_string(profile.join("Preferences")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .pointer("/profile/name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn has_session(db: &Path) -> Result<bool, String> {
    Ok(cookie_rows(db)?
        .iter()
        .any(|(name, _)| name == "auth_token"))
}

fn cookie_rows(db: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let uri = sqlite_uri(db);
    let output = Command::new("sqlite3")
        .arg(&uri)
        .arg(
            "SELECT name, hex(encrypted_value) FROM cookies \
             WHERE host_key IN ('.x.com', 'x.com') AND name IN ('auth_token','ct0','twid');",
        )
        .output()
        .map_err(|_| {
            "could not read the browser cookie database (sqlite3 is missing). set X_AUTH_TOKEN and X_CT0, or run `socli login x --oauth`.".to_string()
        })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim();
        let detail: String = detail.chars().take(160).collect();
        let detail = if detail.is_empty() {
            String::new()
        } else {
            format!(" ({detail})")
        };
        return Err(format!(
            "could not read the browser cookie database{detail}. quit the browser if it is locked, then run `socli login x` again."
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut rows = Vec::new();
    for line in text.lines() {
        let Some((name, hex_value)) = line.split_once('|') else {
            continue;
        };
        let blob = decode_hex(hex_value).ok_or_else(|| {
            "the browser cookie database returned a value socli could not read. run `socli login x` again.".to_string()
        })?;
        rows.push((name.to_string(), blob));
    }
    Ok(rows)
}

fn sqlite_uri(path: &Path) -> String {
    let mut encoded = String::new();
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'_' | b'-' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    format!("file:{encoded}?mode=ro&immutable=1")
}

fn safe_storage_password(browser: &KnownBrowser) -> Result<String, String> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-w",
            "-s",
            browser.service,
            "-a",
            browser.account,
        ])
        .output()
        .map_err(|_| {
            format!(
                "could not read the {} keychain item. set X_AUTH_TOKEN and X_CT0, or run `socli login x --oauth`.",
                browser.label
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "the {} safe storage password is not in the keychain. set X_AUTH_TOKEN and X_CT0, or run `socli login x --oauth`.",
            browser.label
        ));
    }
    let password = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if password.is_empty() {
        return Err(format!(
            "the {} safe storage password was empty. set X_AUTH_TOKEN and X_CT0.",
            browser.label
        ));
    }
    Ok(password)
}

/// Decrypt a Chromium `v10` cookie blob. The 32-byte prefix is dropped when the plaintext
/// does not start with a printable character.
pub fn decrypt_v10(blob: &[u8], key: &[u8; 16]) -> Result<String, ()> {
    let payload = blob.strip_prefix(b"v10").ok_or(())?;
    let plain = Aes128CbcDec::new(&(*key).into(), &IV.into())
        .decrypt_padded_vec_mut::<Pkcs7>(payload)
        .map_err(|_| ())?;
    let text = if plain.first().is_some_and(|byte| !(32..=126).contains(byte)) {
        plain.get(PREFIX_LEN..).unwrap_or_default()
    } else {
        plain.as_slice()
    };
    let value = std::str::from_utf8(text).map_err(|_| ())?.to_string();
    if value.is_empty() {
        return Err(());
    }
    Ok(value)
}

/// `twid` is `u=<id>` or the same value with `=` percent-encoded.
pub fn user_id_from_twid(twid: &str) -> Option<String> {
    let decoded = twid.replace("%3D", "=").replace("%3d", "=");
    let rest = decoded.strip_prefix("u=")?;
    (!rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())).then(|| rest.to_string())
}

fn cookie_ok(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn same_name(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let hi = hex_nibble(bytes[index])?;
        let lo = hex_nibble(bytes[index + 1])?;
        out.push((hi << 4) | lo);
        index += 2;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::Encryptor;
    use cipher::BlockEncryptMut;

    type Aes128CbcEnc = Encryptor<Aes128>;

    fn seal(key: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        let mut blob = b"v10".to_vec();
        blob.extend(
            Aes128CbcEnc::new(&(*key).into(), &IV.into()).encrypt_padded_vec_mut::<Pkcs7>(plain),
        );
        blob
    }

    #[test]
    fn v10_drops_the_binary_prefix_and_returns_the_cookie() {
        let key = [7u8; 16];
        let mut plain = vec![0xc0, b'y', b'&'];
        plain.resize(PREFIX_LEN, 0xab);
        plain.extend(b"auth-token-value");
        let blob = seal(&key, &plain);
        assert_eq!(decrypt_v10(&blob, &key).as_deref(), Ok("auth-token-value"));
    }

    #[test]
    fn v10_keeps_a_cookie_that_is_already_text() {
        let key = [3u8; 16];
        let blob = seal(&key, b"plain-cookie");
        assert_eq!(decrypt_v10(&blob, &key).as_deref(), Ok("plain-cookie"));
    }

    #[test]
    fn a_blocked_browser_is_not_reported_as_no_session() {
        // os error 1 is EPERM on Unix and "Incorrect function" on Windows. The branch keys off the kind.
        let err = read_error(
            "Brave",
            &std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );
        assert!(err.contains("Full Disk Access"), "{err}");
        assert!(!err.contains("no X session"), "{err}");
        let missing = browser_present(std::path::Path::new("/no/such/socli-browser-root")).unwrap();
        assert!(!missing);
    }

    #[test]
    fn twid_reads_a_percent_encoded_user_id() {
        assert_eq!(
            user_id_from_twid("u%3D123456789").as_deref(),
            Some("123456789")
        );
        assert_eq!(user_id_from_twid("u=42").as_deref(), Some("42"));
        assert_eq!(user_id_from_twid("guest"), None);
    }
}
