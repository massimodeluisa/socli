//! PKCE (RFC 7636) verifier and S256 challenge, shared by the X and dev/null sign-ins.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

pub fn code_verifier() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut bytes = [0u8; 64];
    fill_random(&mut bytes);
    bytes
        .iter()
        .map(|b| CHARSET[(*b as usize) % CHARSET.len()] as char)
        .collect()
}

pub fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

pub(crate) fn fill_random(bytes: &mut [u8]) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rand::RngCore;
        rand::thread_rng().fill_bytes(bytes);
    }
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: `bytes` is the caller's live `&mut [u8]`; `socli_random` is the `env` import `(*mut u8, usize)`, and the worker fills exactly that slice with `crypto.getRandomValues` before returning.
        unsafe { socli_random(bytes.as_mut_ptr(), bytes.len()) };
    }
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    /** `crypto.getRandomValues` for the bytes at `ptr`. */
    fn socli_random(ptr: *mut u8, len: usize);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_the_rfc_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            code_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifiers_are_long_and_unreserved() {
        let verifier = code_verifier();
        assert_eq!(verifier.len(), 64);
        assert!(verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
    }
}
