//! The dev/null authorize URL and the pasted `devnull://` callback. The provider only redirects to that scheme.

use url::Url;

pub const AUTHORIZE: &str = "https://dev-null.sh/mobile/authorize";
pub const APP_CALLBACK: &str = "devnull://auth/callback";

pub fn authorize_url(
    fingerprint: &str,
    device_name: &str,
    challenge: &str,
    state: &str,
    redirect_uri: &str,
) -> Result<Url, String> {
    let mut url = Url::parse(AUTHORIZE).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("fingerprint", fingerprint)
        .append_pair("device_name", device_name)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("redirect_uri", redirect_uri);
    Ok(url)
}

/** `{code, code_verifier}` for `POST /api/v1/devices`. */
pub fn approval_body(code: &str, verifier: &str) -> serde_json::Value {
    serde_json::json!({
        "code": code,
        "code_verifier": verifier,
    })
}

/**
The account name on an approval or `/me` body.
Missing, blank and JSON null are absent, so the caller can ask `/me`.
A field that is not a string does not reject the rest of the body.
*/
pub fn username_from_body(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let data = match value.get("data") {
        Some(data) if data.is_object() => data,
        _ => &value,
    };
    data.get("username")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|username| !username.is_empty())
        .map(str::to_string)
}

/** The code in a pasted callback: the whole `devnull://auth/callback?code=…&state=…` address (or any text holding it), or the bare code. A line break that continues the same `state` stays part of the address. A `state` that is present has to match. */
pub fn parse_callback(input: &str, state: &str) -> Result<String, String> {
    let input = input.trim().trim_matches(|c| c == '"' || c == '\'');
    let address = collect_devnull(input, state);
    if !address.contains('?') && !address.contains('=') {
        return match address.as_str() {
            "" => Err("no callback pasted. run `socli login devnull` again".into()),
            code if code.chars().any(char::is_whitespace) || code.contains("://") => Err(
                "that is not a dev/null callback. paste the devnull://auth/callback?code=… address"
                    .into(),
            ),
            code => Ok(code.to_string()),
        };
    }
    let query = query_of(&address);
    if let Some(error) = query.error {
        return Err(format!(
            "dev/null did not approve the device ({error}). run `socli login devnull` again"
        ));
    }
    if query.state.as_deref().is_some_and(|got| got != state) {
        return Err(
            "the callback belongs to another sign-in. run `socli login devnull` again".into(),
        );
    }
    query.code.filter(|code| !code.is_empty()).ok_or_else(|| {
        "the callback has no code. paste the whole devnull://auth/callback?code=… address".into()
    })
}

struct Query {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn query_of(address: &str) -> Query {
    let query = address.split_once('?').map_or(address, |(_, query)| query);
    let query = query.split('#').next().unwrap_or(query);
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" if error.is_none() => error = Some(value.into_owned()),
            _ => {}
        }
    }
    Query { code, state, error }
}

fn collect_devnull(input: &str, expected: &str) -> String {
    let Some(start) = input.find("devnull://") else {
        return input.to_string();
    };
    let mut lines = input[start..].lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    let token = first.split_whitespace().next().unwrap_or(first);
    let mut url = trim_wrapper(token).to_string();
    for line in lines.take(8) {
        let line = line.trim();
        if !should_extend(&url, line, expected) {
            break;
        }
        url.push_str(line);
    }
    url
}

fn trim_wrapper(token: &str) -> &str {
    token.trim_end_matches(['>', '"', '\'', ')', ']', ','])
}

fn should_extend(url: &str, line: &str, expected: &str) -> bool {
    if !is_continuation(line) {
        return false;
    }
    match query_of(url).state {
        Some(got) => expected.starts_with(&got) && got.len() < expected.len(),
        None => line.chars().any(|c| matches!(c, '&' | '=' | '%' | '?')),
    }
}

fn is_continuation(line: &str) -> bool {
    !line.is_empty()
        && !line.contains("://")
        && line.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    '-' | '.' | '_' | '~' | '/' | '?' | '&' | '=' | '%' | '+' | '#'
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authorize_url_carries_every_parameter() {
        let url = authorize_url(
            "SHA256:hPEdU4FVZ1lfjzbPyBhqTWG2qxUu3ZbCnD8mZw9t2Vk",
            "SoCLI on mac",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            "af0ifjsldkj",
            APP_CALLBACK,
        )
        .unwrap();
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(url.host_str(), Some("dev-null.sh"));
        assert_eq!(url.path(), "/mobile/authorize");
        let raw = url.as_str();
        assert!(raw.starts_with("https://dev-null.sh/mobile/authorize?"));
        assert!(raw.contains("fingerprint=SHA256%3AhPEdU4FVZ1lfjzbPyBhqTWG2qxUu3ZbCnD8mZw9t2Vk"));
        assert!(raw.contains("redirect_uri=devnull%3A%2F%2Fauth%2Fcallback"));
        assert!(!raw.contains("redirect_uri=https"));
        assert_eq!(
            pairs,
            [
                (
                    "fingerprint",
                    "SHA256:hPEdU4FVZ1lfjzbPyBhqTWG2qxUu3ZbCnD8mZw9t2Vk"
                ),
                ("device_name", "SoCLI on mac"),
                (
                    "code_challenge",
                    "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
                ),
                ("code_challenge_method", "S256"),
                ("state", "af0ifjsldkj"),
                ("redirect_uri", "devnull://auth/callback"),
            ]
            .map(|(k, v)| (k.to_string(), v.to_string()))
        );
    }

    #[test]
    fn callbacks_parse_from_the_full_address_or_a_bare_code() {
        assert_eq!(
            parse_callback("devnull://auth/callback?code=abc123&state=s1", "s1").unwrap(),
            "abc123"
        );
        assert_eq!(
            parse_callback(
                "  Location: devnull://auth/callback?code=abc%2B1&state=s1\n",
                "s1"
            )
            .unwrap(),
            "abc+1"
        );
        assert_eq!(parse_callback("abc123", "s1").unwrap(), "abc123");
        assert_eq!(
            parse_callback("/callback?code=abc&state=s1", "s1").unwrap(),
            "abc"
        );
        assert_eq!(
            parse_callback("devnull://auth/callback?code=abc", "s1").unwrap(),
            "abc"
        );
    }

    #[test]
    fn callbacks_with_another_state_an_error_or_no_code_are_refused() {
        assert!(
            parse_callback("devnull://auth/callback?code=abc&state=other", "s1")
                .unwrap_err()
                .contains("another sign-in")
        );
        assert!(
            parse_callback("devnull://auth/callback?error=access_denied&state=s1", "s1")
                .unwrap_err()
                .contains("access_denied")
        );
        assert!(parse_callback("devnull://auth/callback?state=s1", "s1").is_err());
        assert!(parse_callback("", "s1").is_err());
        assert!(parse_callback("two words", "s1").is_err());
    }

    #[test]
    fn a_callback_held_in_other_text_or_split_on_a_line_break_keeps_its_code() {
        assert_eq!(
            parse_callback(
                "Location: devnull://auth/callback?code=abc%2B1&state=s1\ncontent-type: text/html",
                "s1"
            )
            .as_deref(),
            Ok("abc+1")
        );
        assert_eq!(
            parse_callback("<devnull://auth/callback?code=abc123&state=s1>", "s1").as_deref(),
            Ok("abc123")
        );
        assert_eq!(
            parse_callback(
                "see \"devnull://auth/callback?code=abc123&state=s1\" now",
                "s1"
            )
            .as_deref(),
            Ok("abc123")
        );
        assert_eq!(
            parse_callback(
                "devnull://auth/callback?code=abc123&state=abcdefghij\nKLMNOPQRST",
                "abcdefghijKLMNOPQRST"
            )
            .as_deref(),
            Ok("abc123")
        );
        assert_eq!(
            parse_callback("devnull://auth/callback?code=abc123&state=s1\nThanks", "s1").as_deref(),
            Ok("abc123")
        );
        assert_eq!(
            parse_callback("devnull://auth/callback?code=abc123\nThanks", "s1").as_deref(),
            Ok("abc123")
        );
        assert!(matches!(
            parse_callback(
                "devnull://auth/callback?code=abc&state=other\nKLMNOPQRST",
                "s1"
            )
            .as_deref(),
            Err(err) if err.contains("another sign-in")
        ));
    }

    #[test]
    fn the_approval_body_sends_the_code_and_the_verifier() {
        let body = approval_body("abc123", "verifier-value");
        assert_eq!(body["code"], "abc123");
        assert_eq!(body["code_verifier"], "verifier-value");
        assert_eq!(body.as_object().map(|fields| fields.len()), Some(2));
    }

    #[test]
    fn a_username_in_the_approval_body_is_kept() {
        assert_eq!(
            username_from_body(r#"{"data":{"id":1,"username":"nuno"}}"#).as_deref(),
            Some("nuno")
        );
        assert_eq!(
            username_from_body(r#"{"username":" nuno "}"#).as_deref(),
            Some("nuno")
        );
        assert_eq!(username_from_body(r#"{"data":{"username":null}}"#), None);
        assert_eq!(username_from_body(r#"{"data":{"username":""}}"#), None);
        assert_eq!(username_from_body("not json"), None);
    }

    #[test]
    fn the_authorize_url_carries_a_fixture_fingerprint() -> Result<(), String> {
        let public = ssh_key::PublicKey::from_openssh(include_str!("fixtures/ssh/ed25519.pub"))
            .map_err(|err| err.to_string())?;
        let fingerprint = public.fingerprint(ssh_key::HashAlg::Sha256).to_string();
        let url = authorize_url(&fingerprint, "SoCLI", "challenge", "state", APP_CALLBACK)?;
        let carried = url
            .query_pairs()
            .find(|(key, _)| key == "fingerprint")
            .map(|(_, value)| value.into_owned());
        assert_eq!(carried.as_deref(), Some(fingerprint.as_str()));
        assert!(url.as_str().contains("fingerprint=SHA256%3A"));
        assert!(url
            .as_str()
            .contains("redirect_uri=devnull%3A%2F%2Fauth%2Fcallback"));
        Ok(())
    }
}
