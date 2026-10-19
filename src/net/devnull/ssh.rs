/*!
Commits signed with the user's GitHub SSH key, the way the official `devnull commit` script
signs them: the key lookup, the signed payload, `ssh-keygen -Y sign`, and the CLI endpoint.
*/

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use reqwest::blocking::Client;

use super::{api, sign};
use crate::net::NetError;

/// The SSHSIG namespace of a commit without images.
pub const COMMIT_NAMESPACE: &str = "devnull-commit";

/// The server reads at most this many candidate keys.
const MAX_KEYS: usize = 20;

pub const NO_KEYGEN: &str = "ssh-keygen is required for dev/null posting with your GitHub key";

/// `<type> <base64>` without the comment, for the two key types the server accepts.
pub fn key_id(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    let kind = parts.next()?;
    let blob = parts.next()?;
    matches!(kind, "ssh-ed25519" | "ssh-rsa").then(|| format!("{kind} {blob}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// The public key line as found, comment included.
    pub line: String,
    pub id: String,
}

/** Agent keys first, then key files, deduplicated by key and capped at what the server reads. */
pub fn candidates<'a>(lines: impl IntoIterator<Item = &'a str>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for line in lines {
        let line = line.trim();
        let Some(id) = key_id(line) else { continue };
        if out.len() < MAX_KEYS && !out.iter().any(|c| c.id == id) {
            out.push(Candidate {
                line: line.to_string(),
                id,
            });
        }
    }
    out
}

/// The key-lookup body: one `<type> <base64>` per line.
pub fn lookup_body(candidates: &[Candidate]) -> String {
    candidates.iter().map(|c| format!("{}\n", c.id)).collect()
}

/// The candidate the server named on its first line; nothing else is ever trusted.
pub fn select<'a>(response: &str, candidates: &'a [Candidate]) -> Option<&'a Candidate> {
    let id = key_id(response.lines().next()?)?;
    candidates.iter().find(|c| c.id == id)
}

/// The signed payload: `timestamp\nnonce\nreply_to\nmessage`, with no trailing newline.
pub fn payload(timestamp: u64, nonce: &str, reply_to: &str, message: &str) -> Vec<u8> {
    format!("{timestamp}\n{nonce}\n{reply_to}\n{message}").into_bytes()
}

/// A reply target goes on its own payload line, so only the characters the script allows pass.
pub fn valid_reply_to(reply_to: &str) -> bool {
    reply_to
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || ":/._@-".contains(c))
}

/** The `https://dev-null.sh/@<user>/commits/<id>` link in the endpoint's `Committed: <url>` answer, with its id and username. Any other host is refused. */
pub fn committed(response: &str) -> Option<(String, String, String)> {
    let url = response
        .split_whitespace()
        .find(|word| word.starts_with("https://"))?;
    let parsed = url::Url::parse(url).ok()?;
    if parsed.host_str() != Some("dev-null.sh") {
        return None;
    }
    let parts: Vec<&str> = parsed.path_segments()?.filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [user, "commits", id] => {
            let user = user.strip_prefix('@').filter(|u| !u.is_empty())?;
            Some((url.to_string(), id.to_string(), user.to_string()))
        }
        _ => None,
    }
}

/** Posts through `/api/cli/commits` signed with the saved GitHub key, as `devnull commit` does, and returns the plain-text answer. */
pub fn commit(
    http: &Client,
    key_line: &str,
    reply_to: &str,
    message: &str,
) -> Result<String, NetError> {
    let id = key_id(key_line).ok_or_else(|| {
        NetError::Other(
            "the saved dev/null ssh key is not ed25519 or rsa. run `socli auth devnull ssh`".into(),
        )
    })?;
    if !valid_reply_to(reply_to) {
        return Err(NetError::Other(format!(
            "{reply_to} is not a dev/null commit id"
        )));
    }
    let payload = payload(api::now(), &sign::nonce(), reply_to, message);
    let signature = sign_with_local_key(&id, &payload)?;
    api::post_text(http, &["api", "cli", "commits"], payload, Some(&signature))
}

/// Through the agent when it holds the key, otherwise with the private file next to the matching `~/.ssh/*.pub`.
fn sign_with_local_key(id: &str, payload: &[u8]) -> Result<String, NetError> {
    if agent_keys().lines().filter_map(key_id).any(|k| k == id) {
        let path = std::env::temp_dir().join(format!(
            "socli-devnull-{}-{}.pub",
            std::process::id(),
            sign::nonce()
        ));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options
            .open(&path)
            .and_then(|mut file| writeln!(file, "{id}"))
            .map_err(|e| NetError::Other(format!("could not write {}: {e}", path.display())))?;
        let signed = keygen_sign(&path, payload);
        let _ = fs::remove_file(&path);
        return signed;
    }
    let private = home()
        .map(|h| key_files(&h))
        .unwrap_or_default()
        .into_iter()
        .find(|(_, line)| key_id(line).as_deref() == Some(id))
        .map(|(public, _)| public.with_extension(""));
    match private {
        Some(path) => keygen_sign(&path, payload),
        None => Err(NetError::Other(format!(
            "could not find the private key of {id}. load it with `ssh-add`, or run `socli auth devnull ssh`"
        ))),
    }
}

// MARK: local keys

/// Public keys in the SSH agent, one line each; empty when there is no agent.
pub fn agent_keys() -> String {
    Command::new("ssh-add")
        .arg("-L")
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

/// `~/.ssh/*.pub` files that sit next to their private key, sorted by name, with their first line.
pub fn key_files(home: &Path) -> Vec<(PathBuf, String)> {
    let Ok(entries) = fs::read_dir(home.join(".ssh")) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "pub"))
        .filter(|p| p.with_extension("").is_file())
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let line = text.lines().next()?.trim().to_string();
            Some((path, line))
        })
        .collect()
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/** Signs `payload` with `ssh-keygen -Y sign -n devnull-commit -f <key>` on stdin, and returns the base64 signature on one line. */
pub fn keygen_sign(key: &Path, payload: &[u8]) -> Result<String, NetError> {
    let mut child = Command::new("ssh-keygen")
        .args(["-Y", "sign", "-n", COMMIT_NAMESPACE, "-f"])
        .arg(key)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => NetError::Other(NO_KEYGEN.into()),
            _ => NetError::Other(format!("could not run ssh-keygen: {e}")),
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(payload).map_err(|e| {
            NetError::Other(format!("could not hand the commit to ssh-keygen: {e}"))
        })?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| NetError::Other(format!("ssh-keygen did not finish: {e}")))?;
    if !output.status.success() {
        let reason = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("no reason given")
            .to_string();
        return Err(NetError::Other(format!(
            "ssh-keygen could not sign with {} ({reason}). load the key with `ssh-add`, or run `socli auth devnull ssh`",
            key.display()
        )));
    }
    let signature = sign::strip_armor(&String::from_utf8_lossy(&output.stdout));
    if signature.is_empty()
        || !signature
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c))
    {
        return Err(NetError::Other(
            "ssh-keygen produced an unexpected signature".into(),
        ));
    }
    Ok(signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOnHvGTG8nArIhSYWCgg7gT1E2PIsaG3zzjURXGy/57C";

    #[test]
    fn key_ids_drop_the_comment_and_other_key_types() {
        assert_eq!(key_id(&format!("{ED} me@laptop")).as_deref(), Some(ED));
        assert_eq!(key_id("ecdsa-sha2-nistp256 AAAA me"), None);
        assert_eq!(key_id("ssh-rsa"), None);
    }

    #[test]
    fn candidates_are_deduplicated_and_capped() {
        let agent = format!("{ED} agent\nssh-rsa AAAAB3rsa agent\necdsa-sha2-nistp256 AAAA x\n");
        let file = format!("{ED} file");
        let lines = agent.lines().chain([file.as_str()]);
        let list = candidates(lines);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].line, format!("{ED} agent"));
        assert_eq!(list[1].id, "ssh-rsa AAAAB3rsa");
        assert_eq!(lookup_body(&list), format!("{ED}\nssh-rsa AAAAB3rsa\n"));

        let many: Vec<String> = (0..30).map(|i| format!("ssh-ed25519 KEY{i} c")).collect();
        assert_eq!(candidates(many.iter().map(String::as_str)).len(), 20);
    }

    #[test]
    fn the_lookup_answer_must_name_one_of_the_candidates() {
        let list = candidates([format!("{ED} me@laptop").as_str(), "ssh-rsa AAAAB3rsa work"]);
        let picked = select("ssh-rsa AAAAB3rsa\n", &list).unwrap();
        assert_eq!(picked.line, "ssh-rsa AAAAB3rsa work");
        assert_eq!(
            select(&format!("{ED}\n"), &list).unwrap().line,
            format!("{ED} me@laptop")
        );
        assert_eq!(select("ssh-ed25519 SOMEONEELSE\n", &list), None);
        assert_eq!(select("", &list), None);
    }

    #[test]
    fn payload_bytes_for_a_top_level_commit_and_a_reply() {
        assert_eq!(
            payload(
                1760534400,
                "3f9c2a7e5b1d4c8f9a0b2c3d4e5f6a7b",
                "",
                "Hello world!"
            ),
            b"1760534400\n3f9c2a7e5b1d4c8f9a0b2c3d4e5f6a7b\n\nHello world!"
        );
        assert_eq!(
            payload(
                1760534400,
                "3f9c2a7e5b1d4c8f9a0b2c3d4e5f6a7b",
                "01a157e5-4099-72e0-bbf2-4591a526171c",
                "Agreed!\nTwo lines."
            ),
            b"1760534400\n3f9c2a7e5b1d4c8f9a0b2c3d4e5f6a7b\n01a157e5-4099-72e0-bbf2-4591a526171c\nAgreed!\nTwo lines."
        );
        assert!(valid_reply_to("01a157e5-4099-72e0-bbf2-4591a526171c"));
        assert!(valid_reply_to("https://dev-null.sh/@nuno/commits/0be0130"));
        assert!(!valid_reply_to("abc\nevil"));
    }

    #[test]
    fn the_commit_answer_gives_the_link_id_and_user() {
        let answer = "Committed: https://dev-null.sh/@nunomaduro/commits/01a157e5-4099-72e0-bbf2-4591a526171c (pending review, only you can see it until it is approved)\n";
        let (url, id, user) = committed(answer).unwrap();
        assert_eq!(
            url,
            "https://dev-null.sh/@nunomaduro/commits/01a157e5-4099-72e0-bbf2-4591a526171c"
        );
        assert_eq!(id, "01a157e5-4099-72e0-bbf2-4591a526171c");
        assert_eq!(user, "nunomaduro");
        assert_eq!(committed("Something else\n"), None);
        assert_eq!(
            committed("Committed: https://evil.example/@nuno/commits/01a1\n"),
            None
        );
        assert_eq!(
            committed("Committed: https://dev-null.sh/@/commits/01a1\n"),
            None
        );
        assert_eq!(
            committed("Committed: https://dev-null.sh/nuno/commits/01a1\n"),
            None
        );
    }

    #[test]
    fn ssh_keygen_signs_the_payload_for_the_commit_namespace() {
        if Command::new("ssh-keygen").arg("-?").output().is_err() {
            eprintln!("ssh-keygen is not installed; skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "socli-devnull-keygen-{}-{}",
            std::process::id(),
            sign::nonce()
        ));
        fs::create_dir_all(&dir).unwrap();
        let key = ssh_key::PrivateKey::random(&mut rand::rngs::OsRng, ssh_key::Algorithm::Ed25519)
            .unwrap();
        let path = dir.join("id_ed25519");
        let pem = key.to_openssh(ssh_key::LineEnding::LF).unwrap();
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options
            .open(&path)
            .unwrap()
            .write_all(pem.as_bytes())
            .unwrap();

        let message = payload(1760534400, &sign::nonce(), "", "Hello world!");
        let signature = keygen_sign(&path, &message).unwrap();
        let _ = fs::remove_dir_all(&dir);
        let parsed = ssh_key::SshSig::from_pem(sign::armor(&signature)).unwrap();
        assert_eq!(parsed.namespace(), COMMIT_NAMESPACE);
        key.public_key()
            .verify(COMMIT_NAMESPACE, &message, &parsed)
            .unwrap();
    }
}
