//! The signed v1 API: the canonical message, the nonce, the device key and its SSHSIG.

#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, SshSig};

/// The SSHSIG namespace of requests signed by an approved device.
pub const API_NAMESPACE: &str = "devnull-api";

/// File name of the device key inside the config directory.
pub const KEY_FILE: &str = "devnull_ed25519";

/** The bytes the server verifies: timestamp, nonce, method, path with its query, and the raw body, joined by `\n`. */
pub fn canonical(timestamp: u64, nonce: &str, method: &str, target: &str, body: &[u8]) -> Vec<u8> {
    let mut message = format!("{timestamp}\n{nonce}\n{method}\n{target}\n").into_bytes();
    message.extend_from_slice(body);
    message
}

/// 32 random lowercase hex characters.
pub fn nonce() -> String {
    let mut bytes = [0u8; 16];
    crate::net::pkce::fill_random(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// An SSHSIG over `message`: base64 on one line, without the armor lines.
pub fn signature(key: &PrivateKey, namespace: &str, message: &[u8]) -> Result<String, String> {
    let sig = SshSig::sign(key, namespace, HashAlg::Sha512, message)
        .map_err(|e| format!("could not sign the dev/null request: {e}"))?;
    let pem = sig
        .to_pem(LineEnding::LF)
        .map_err(|e| format!("could not encode the dev/null signature: {e}"))?;
    Ok(strip_armor(&pem))
}

/// Drops the `-----BEGIN/END SSH SIGNATURE-----` lines and joins the base64 on one line.
pub fn strip_armor(pem: &str) -> String {
    pem.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("-----"))
        .collect()
}

pub fn fingerprint(key: &PrivateKey) -> String {
    key.public_key().fingerprint(HashAlg::Sha256).to_string()
}

/// A new ed25519 device key. The terminal writes it to disk; the page keeps the OpenSSH text.
#[cfg(any(test, not(feature = "native")))]
pub fn generate() -> Result<PrivateKey, String> {
    #[cfg(target_arch = "wasm32")]
    let mut rng = PageRng;
    #[cfg(not(target_arch = "wasm32"))]
    let mut rng = rand::rngs::OsRng;
    PrivateKey::random(&mut rng, Algorithm::Ed25519)
        .map_err(|e| format!("could not make a device key: {e}"))
}

#[cfg(any(test, not(feature = "native")))]
pub fn from_openssh(pem: &str) -> Result<PrivateKey, String> {
    PrivateKey::from_openssh(pem)
        .map_err(|e| format!("could not read the dev/null device key: {e}"))
}

#[cfg(any(test, not(feature = "native")))]
pub fn to_openssh(key: &PrivateKey) -> Result<String, String> {
    key.to_openssh(LineEnding::LF)
        .map(|pem| pem.to_string())
        .map_err(|e| format!("could not encode the device key: {e}"))
}

#[cfg(target_arch = "wasm32")]
struct PageRng;

#[cfg(target_arch = "wasm32")]
impl ssh_key::rand_core::RngCore for PageRng {
    fn next_u32(&mut self) -> u32 {
        let mut buf = [0u8; 4];
        self.fill_bytes(&mut buf);
        u32::from_le_bytes(buf)
    }

    fn next_u64(&mut self) -> u64 {
        let mut buf = [0u8; 8];
        self.fill_bytes(&mut buf);
        u64::from_le_bytes(buf)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        crate::net::pkce::fill_random(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), ssh_key::rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
impl ssh_key::rand_core::CryptoRng for PageRng {}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_key(path: &Path) -> Result<PrivateKey, String> {
    PrivateKey::read_openssh_file(path).map_err(|e| {
        format!(
            "could not read the dev/null device key at {}: {e}. run `socli login devnull`",
            path.display()
        )
    })
}

/** The key at `path`, or a new ed25519 key written there with mode 0600. An existing file is never overwritten. The flag says whether the key is new. */
#[cfg(not(target_arch = "wasm32"))]
pub fn load_or_create(path: &Path) -> Result<(PrivateKey, bool), String> {
    if path.exists() {
        return read_key(path).map(|key| (key, false));
    }
    let key = PrivateKey::random(&mut rand::rngs::OsRng, Algorithm::Ed25519)
        .map_err(|e| format!("could not make a device key: {e}"))?;
    let pem = key
        .to_openssh(LineEnding::LF)
        .map_err(|e| format!("could not encode the device key: {e}"))?;
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(path)
        .and_then(|mut file| file.write_all(pem.as_bytes()))
        .map_err(|e| format!("could not write the device key to {}: {e}", path.display()))?;
    Ok((key, true))
}

#[cfg(not(target_arch = "wasm32"))]
fn create_private_dir(dir: &Path) -> Result<(), String> {
    if dir.as_os_str().is_empty() || dir.is_dir() {
        return Ok(());
    }
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(dir)
        .map_err(|e| format!("creating {}: {e}", dir.display()))
}

/// Wraps a one-line signature back into the 70-column armor `SshSig::from_pem` requires.
#[cfg(test)]
pub fn armor(signature: &str) -> String {
    let body: Vec<&str> = signature
        .as_bytes()
        .chunks(70)
        .map(|chunk| std::str::from_utf8(chunk).unwrap())
        .collect();
    format!(
        "-----BEGIN SSH SIGNATURE-----\n{}\n-----END SSH SIGNATURE-----\n",
        body.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    #[test]
    fn a_generated_key_round_trips_through_openssh_text() {
        let key = generate().unwrap();
        let pem = to_openssh(&key).unwrap();
        let back = from_openssh(&pem).unwrap();
        assert_eq!(fingerprint(&key), fingerprint(&back));
        let message = b"device";
        let signed = signature(&key, API_NAMESPACE, message).unwrap();
        assert!(!signed.is_empty());
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "socli-devnull-{name}-{}-{}",
            std::process::id(),
            nonce()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn canonical_message_joins_the_five_parts_with_newlines() {
        let message = canonical(
            1760534400,
            "3f9c2a7e5b1d4c8f9a0b2c3d4e5f6a7b",
            "POST",
            "/api/v1/commits",
            br#"{"body":"Hello from my phone"}"#,
        );
        assert_eq!(
            String::from_utf8(message).unwrap(),
            "1760534400\n3f9c2a7e5b1d4c8f9a0b2c3d4e5f6a7b\nPOST\n/api/v1/commits\n{\"body\":\"Hello from my phone\"}"
        );
    }

    #[test]
    fn canonical_message_without_a_body_ends_with_the_separator() {
        let message = canonical(1, "00", "GET", "/api/v1/log/latest?cursor=abc", b"");
        assert_eq!(message, b"1\n00\nGET\n/api/v1/log/latest?cursor=abc\n");
    }

    #[test]
    fn nonces_are_32_lowercase_hex_characters() {
        let nonce = nonce();
        assert_eq!(nonce.len(), 32);
        assert!(nonce
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn signatures_are_one_line_of_base64_that_parses_back() {
        let key = PrivateKey::random(&mut rand::rngs::OsRng, Algorithm::Ed25519).unwrap();
        let sig = signature(&key, API_NAMESPACE, b"hello").unwrap();
        assert!(!sig.contains('\n') && !sig.contains('-'));
        let parsed = SshSig::from_pem(armor(&sig)).unwrap();
        assert_eq!(parsed.namespace(), API_NAMESPACE);
        assert_eq!(parsed.hash_alg(), HashAlg::Sha512);
        key.public_key()
            .verify(API_NAMESPACE, b"hello", &parsed)
            .unwrap();
    }

    #[test]
    fn ssh_keygen_verifies_the_signature_over_a_canonical_message() {
        if Command::new("ssh-keygen").arg("-?").output().is_err() {
            eprintln!("ssh-keygen is not installed; skipping");
            return;
        }
        let dir = temp_dir("verify");
        let key = PrivateKey::random(&mut rand::rngs::OsRng, Algorithm::Ed25519).unwrap();
        let message = canonical(
            1760534400,
            &nonce(),
            "POST",
            "/api/v1/commits",
            br#"{"body":"hi"}"#,
        );
        let sig = signature(&key, API_NAMESPACE, &message).unwrap();
        let armored =
            format!("-----BEGIN SSH SIGNATURE-----\n{sig}\n-----END SSH SIGNATURE-----\n");
        let public = key.public_key().to_openssh().unwrap();
        fs::write(
            dir.join("allowed_signers"),
            format!("device@socli {public}\n"),
        )
        .unwrap();
        fs::write(dir.join("message.sig"), armored).unwrap();
        fs::write(dir.join("message"), &message).unwrap();
        let output = Command::new("sh")
            .current_dir(&dir)
            .args([
                "-c",
                "ssh-keygen -Y verify -f allowed_signers -I device@socli -n devnull-api -s message.sig < message",
            ])
            .output()
            .unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn upstream_fixture_signatures_parse_and_the_ed25519_one_verifies() {
        let message = include_bytes!("fixtures/ssh/message");
        let ed25519 = SshSig::from_pem(include_str!("fixtures/ssh/ed25519.sig")).unwrap();
        assert_eq!(ed25519.namespace(), "devnull-commit");
        let public =
            ssh_key::PublicKey::from_openssh(include_str!("fixtures/ssh/ed25519.pub")).unwrap();
        public.verify("devnull-commit", message, &ed25519).unwrap();
        assert!(public
            .verify("devnull-commit", b"It was not DNS.", &ed25519)
            .is_err());
        assert!(public.verify("git", message, &ed25519).is_err());

        let rsa = SshSig::from_pem(include_str!("fixtures/ssh/rsa.sig")).unwrap();
        assert_eq!(rsa.namespace(), "devnull-commit");
        let rsa_public =
            ssh_key::PublicKey::from_openssh(include_str!("fixtures/ssh/rsa.pub")).unwrap();
        assert_eq!(rsa.public_key(), rsa_public.key_data());
    }

    #[test]
    fn a_new_key_is_written_once_with_mode_0600_and_reused_after() {
        let dir = temp_dir("key");
        let path = dir.join("nested").join(KEY_FILE);
        let (first, created) = load_or_create(&path).unwrap();
        assert!(created);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let (again, created) = load_or_create(&path).unwrap();
        assert!(!created);
        assert_eq!(fingerprint(&first), fingerprint(&again));
        assert!(fingerprint(&first).starts_with("SHA256:"));
        assert_eq!(fingerprint(&first).len(), "SHA256:".len() + 43);
        let _ = fs::remove_dir_all(&dir);
    }
}
