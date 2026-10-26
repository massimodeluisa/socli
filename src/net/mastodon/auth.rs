/*!
Mastodon sign-in on the person's own server: socli registers itself as an app there (once per
server), the browser shows a code after approval, and the code is exchanged with PKCE for a token
that does not expire. A token pasted from Preferences > Development works too. Docs:
https://docs.joinmastodon.org/client/token/ and https://docs.joinmastodon.org/methods/oauth/
*/

use crate::net::http::{Client, Error, Response};
use serde_json::Value;
use url::Url;

use super::DEFAULT_SERVER;
use crate::config::{self, MastodonConfig};
use crate::net::pkce::{code_challenge, code_verifier};
use crate::net::{http, Network};
use crate::screens;
use crate::util;

const REDIRECT: &str = "urn:ietf:wg:oauth:2.0:oob";
const SCOPES: &str = "read write";
const FALLBACK_MAX_CHARS: usize = 500;

/** A server name from what the person typed: a bare host, or a URL whose host is kept. */
pub fn normalize_server(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let bare = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = bare
        .split('/')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let valid = host.contains('.')
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
        && Url::parse(&format!("https://{host}/")).is_ok();
    if valid {
        Ok(host)
    } else {
        Err(format!(
            "{input} is not a server name. use one like mastodon.social"
        ))
    }
}

/** The server for an answer to the prompt: empty keeps the default, anything else must be a server name. */
fn chosen_server(answer: &str) -> Result<String, String> {
    if answer.trim().is_empty() {
        return Ok(DEFAULT_SERVER.to_string());
    }
    normalize_server(answer)
}

/// Asks for the server until the answer is empty or valid; without a person it is the default.
fn ask_server() -> Result<String, String> {
    if !util::person_present() {
        return Ok(DEFAULT_SERVER.to_string());
    }
    loop {
        screens::present_login(
            Network::Mastodon.title(),
            &[("Sign-in method", "Approve SoCLI on your Mastodon server")],
            "Enter your server, or press Enter to use mastodon.social.",
            Some(&format!("Server ({DEFAULT_SERVER}):")),
        )?;
        let answer = util::prompt_line("", "")?;
        match chosen_server(&answer) {
            Ok(server) => return Ok(server),
            Err(message) => eprintln!("{message}"),
        }
    }
}

pub fn authorize_url(server: &str, client_id: &str, challenge: &str) -> String {
    let mut url = Url::parse(&format!("https://{server}/oauth/authorize"))
        .expect("normalize_server only accepts hosts that parse as a URL");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", REDIRECT)
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    url.into()
}

/// The page callback, not the terminal's `oob` code. `state` comes back on the redirect.
pub fn page_authorize_url(
    server: &str,
    client_id: &str,
    challenge: &str,
    redirect: &str,
    state: &str,
) -> Result<String, String> {
    let mut url = Url::parse(&format!("https://{server}/oauth/authorize"))
        .map_err(|_| format!("{server} is not a server name. use one like mastodon.social"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("scope", SCOPES)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.into())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env(key: &str) -> Option<String> {
    non_empty(std::env::var(key).ok())
}

fn unreachable(server: &str, error: Error) -> String {
    format!(
        "could not reach {server} ({}). check your connection and try again",
        error.without_url()
    )
}

/// The JSON body of a successful answer, or the server's own message of a failed one.
fn read(server: &str, response: Response) -> Result<Value, String> {
    let status = response.status();
    let body = response.text().map_err(|e| unreachable(server, e))?;
    if status.is_success() {
        return serde_json::from_str(&body)
            .map_err(|e| format!("could not read the answer of {server}: {e}"));
    }
    Err(http::error_text(&body).unwrap_or_else(|| format!("Mastodon returned {}", status.as_u16())))
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().filter(|v| !v.is_empty())
}

pub fn login(server: Option<String>, token: Option<String>) -> Result<(), String> {
    let server = match non_empty(server).or_else(|| env("MASTODON_SERVER")) {
        Some(given) => normalize_server(&given)?,
        None => ask_server()?,
    };
    let token = non_empty(token).or_else(|| env("MASTODON_ACCESS_TOKEN"));
    if let Some(token) = token {
        let _ = screens::present_login(
            Network::Mastodon.title(),
            &[
                ("Server", &server),
                (
                    "Sign-in method",
                    "Access token from Preferences > Development",
                ),
                ("Token", "••••"),
            ],
            "",
            None,
        )?;
        return finish(server, token, None);
    }
    let hint = "Sign in without a browser: socli login mastodon --token <token>";
    if !util::person_present() {
        return Err(format!("{} {hint}", util::NEEDS_PERSON));
    }
    let http = http::client().map_err(|e| e.to_string())?;
    let (client_id, client_secret) = app(&http, &server)?;
    let verifier = code_verifier();
    let url = authorize_url(&server, &client_id, &code_challenge(&verifier));
    let _ = screens::present_login(
        Network::Mastodon.title(),
        &[
            ("Server", &server),
            ("Sign-in method", "Approve SoCLI in your browser"),
            ("Code", "paste it below"),
        ],
        &format!("{url}\n\nApprove SoCLI in the browser, then paste the code it shows."),
        Some("Authorization code:"),
    )?;
    let _ = util::open_browser(&url);
    let code = util::prompt_line("", hint)?;
    let code = code.trim();
    if code.is_empty() {
        return Err("no code was entered. run `socli login mastodon` again".into());
    }
    let response = http
        .post(format!("https://{server}/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", REDIRECT),
            ("code_verifier", verifier.as_str()),
            ("scope", SCOPES),
        ])
        .send()
        .map_err(|e| unreachable(&server, e))?;
    let denied = |message: String| {
        format!("{server} did not accept the code: {message}. run `socli login mastodon` again")
    };
    let body = read(&server, response).map_err(denied)?;
    let access_token = text(&body, "access_token")
        .ok_or_else(|| denied("no token came back".into()))?
        .to_string();
    finish(server, access_token, Some((client_id, client_secret)))
}

/// The app SoCLI registered on `server`: the saved one, else a new one that is saved at once.
fn app(http: &Client, server: &str) -> Result<(String, String), String> {
    let saved = config::load()?.mastodon;
    if let (Some(id), Some(secret)) = (&saved.client_id, &saved.client_secret) {
        if saved.server.as_deref() == Some(server) {
            return Ok((id.clone(), secret.clone()));
        }
    }
    let response = http
        .post(format!("https://{server}/api/v1/apps"))
        .form(&[
            ("client_name", "SoCLI"),
            ("redirect_uris", REDIRECT),
            ("scopes", SCOPES),
        ])
        .send()
        .map_err(|e| unreachable(server, e))?;
    let body =
        read(server, response).map_err(|m| format!("{server} did not register socli: {m}"))?;
    let (Some(id), Some(secret)) = (text(&body, "client_id"), text(&body, "client_secret")) else {
        return Err(format!(
            "{server} did not register socli: no app credentials came back"
        ));
    };
    let (id, secret) = (id.to_string(), secret.to_string());
    config::update(|cfg| {
        cfg.mastodon = MastodonConfig {
            server: Some(server.to_string()),
            client_id: Some(id.clone()),
            client_secret: Some(secret.clone()),
            ..MastodonConfig::default()
        }
    })?;
    Ok((id, secret))
}

fn finish(server: String, token: String, app: Option<(String, String)>) -> Result<(), String> {
    let http = http::client().map_err(|e| e.to_string())?;
    let response = http
        .get(format!(
            "https://{server}/api/v1/accounts/verify_credentials"
        ))
        .bearer_auth(&token)
        .send()
        .map_err(|e| unreachable(&server, e))?;
    let account =
        read(&server, response).map_err(|m| format!("{server} did not accept the token: {m}"))?;
    let acct = text(&account, "acct")
        .ok_or_else(|| format!("{server} did not accept the token: the profile has no handle"))?;
    let username = if acct.contains('@') {
        acct.to_string()
    } else {
        format!("{acct}@{server}")
    };
    let user_id = text(&account, "id").map(str::to_string);
    let max_chars = http
        .get(format!("https://{server}/api/v2/instance"))
        .send()
        .ok()
        .and_then(|response| read(&server, response).ok())
        .and_then(|instance| {
            instance
                .pointer("/configuration/statuses/max_characters")
                .and_then(Value::as_u64)
        })
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(FALLBACK_MAX_CHARS);
    config::update(|cfg| {
        let (client_id, client_secret) = match app {
            Some((id, secret)) => (Some(id), Some(secret)),
            None if cfg.mastodon.server.as_deref() == Some(server.as_str()) => (
                cfg.mastodon.client_id.clone(),
                cfg.mastodon.client_secret.clone(),
            ),
            None => (None, None),
        };
        cfg.mastodon = MastodonConfig {
            server: Some(server.clone()),
            client_id,
            client_secret,
            access_token: Some(token),
            user_id,
            username: Some(username.clone()),
            max_chars: Some(max_chars),
            oauth_state: None,
            oauth_verifier: None,
            oauth_redirect: None,
        };
    })?;
    screens::print_signed_in(&username, None)
}

fn clear_page_oauth() -> Result<(), String> {
    config::update(|cfg| {
        cfg.mastodon.oauth_state = None;
        cfg.mastodon.oauth_verifier = None;
        cfg.mastodon.oauth_redirect = None;
    })?;
    Ok(())
}

/// Registers an app whose callback is this page, then returns the authorize URL.
pub fn begin_page(server: &str, redirect: &str) -> Result<String, String> {
    let redirect = crate::util::page_callback(redirect)?;
    let server = chosen_server(server)?;
    let http = http::client().map_err(|e| e.to_string())?;
    let response = http
        .post(format!("https://{server}/api/v1/apps"))
        .form(&[
            ("client_name", "SoCLI"),
            ("redirect_uris", redirect.as_str()),
            ("scopes", SCOPES),
        ])
        .send()
        .map_err(|e| unreachable(&server, e))?;
    let body =
        read(&server, response).map_err(|m| format!("{server} did not register socli: {m}"))?;
    let (Some(id), Some(secret)) = (text(&body, "client_id"), text(&body, "client_secret")) else {
        return Err(format!(
            "{server} did not register socli: no app credentials came back"
        ));
    };
    let (id, secret) = (id.to_string(), secret.to_string());
    let verifier = code_verifier();
    let state = code_verifier();
    let url = page_authorize_url(&server, &id, &code_challenge(&verifier), &redirect, &state)?;
    config::update(|cfg| {
        cfg.mastodon.server = Some(server);
        cfg.mastodon.client_id = Some(id);
        cfg.mastodon.client_secret = Some(secret);
        cfg.mastodon.oauth_state = Some(state);
        cfg.mastodon.oauth_verifier = Some(verifier);
        cfg.mastodon.oauth_redirect = Some(redirect);
    })?;
    Ok(url)
}

/// Trades the code from this page for a token. A mismatched state never calls the server.
pub fn finish_page(code: &str, got_state: &str) -> Result<(), String> {
    let saved = config::load()?.mastodon;
    let pending = saved.oauth_state.clone().unwrap_or_default();
    if pending.is_empty() || got_state.is_empty() || pending != got_state {
        clear_page_oauth()?;
        return Err("this sign-in has expired. start it again.".into());
    }
    let verifier = saved.oauth_verifier.clone().unwrap_or_default();
    let redirect = saved.oauth_redirect.clone().unwrap_or_default();
    let server = saved.server.clone().unwrap_or_default();
    let client_id = saved.client_id.clone().unwrap_or_default();
    let client_secret = saved.client_secret.clone().unwrap_or_default();
    clear_page_oauth()?;
    let code = code.trim();
    let code_ok = !code.is_empty() && code.len() <= 512 && code.chars().all(|c| !c.is_whitespace());
    if !code_ok
        || verifier.is_empty()
        || redirect.is_empty()
        || server.is_empty()
        || client_id.is_empty()
        || client_secret.is_empty()
    {
        return Err("this sign-in has expired. start it again.".into());
    }
    let http = http::client().map_err(|e| e.to_string())?;
    let response = http
        .post(format!("https://{server}/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("code_verifier", verifier.as_str()),
            ("scope", SCOPES),
        ])
        .send()
        .map_err(|e| unreachable(&server, e))?;
    let denied = |message: String| {
        format!("{server} did not accept the code: {message}. start sign-in again")
    };
    let body = read(&server, response).map_err(denied)?;
    let access_token = text(&body, "access_token")
        .ok_or_else(|| denied("no token came back".into()))?
        .to_string();
    finish(server, access_token, Some((client_id, client_secret)))
}

pub fn logout() -> Result<(), String> {
    let saved = config::load()?.mastodon;
    if let (Some(server), Some(token), Some(id), Some(secret)) = (
        &saved.server,
        &saved.access_token,
        &saved.client_id,
        &saved.client_secret,
    ) {
        if let Ok(client) = http::client() {
            let _ = client
                .post(format!("https://{server}/oauth/revoke"))
                .form(&[
                    ("client_id", id.as_str()),
                    ("client_secret", secret.as_str()),
                    ("token", token.as_str()),
                ])
                .send();
        }
    }
    config::update(|cfg| cfg.mastodon = MastodonConfig::default()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_answer_picks_the_server() {
        assert_eq!(DEFAULT_SERVER, "mastodon.social");
        assert_eq!(chosen_server("").unwrap(), DEFAULT_SERVER);
        assert_eq!(chosen_server("   ").unwrap(), DEFAULT_SERVER);
        assert_eq!(
            chosen_server("https://Fosstodon.org/about").unwrap(),
            "fosstodon.org"
        );
        assert_eq!(
            chosen_server("not a host").unwrap_err(),
            "not a host is not a server name. use one like mastodon.social"
        );
    }

    #[test]
    fn server_names_are_normalized() {
        assert_eq!(
            normalize_server("mastodon.social").unwrap(),
            "mastodon.social"
        );
        assert_eq!(
            normalize_server(" https://Fosstodon.org/about ").unwrap(),
            "fosstodon.org"
        );
        assert!(normalize_server("ines@mastodon.social").is_err());
        assert!(normalize_server("localhost").is_err());
        assert!(normalize_server("mastodon.social?x=1").is_err());
        assert!(normalize_server("[mastodon.social").is_err());
        assert_eq!(
            normalize_server("mastodon.social.").unwrap(),
            "mastodon.social"
        );
        assert_eq!(
            normalize_server("not a host").unwrap_err(),
            "not a host is not a server name. use one like mastodon.social"
        );
    }

    #[test]
    fn the_authorize_url_asks_for_a_code_with_pkce() {
        let url = url::Url::parse(&authorize_url("mastodon.social", "cid", "chal")).unwrap();
        assert_eq!(url.host_str(), Some("mastodon.social"));
        assert_eq!(url.path(), "/oauth/authorize");
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        for (key, value) in [
            ("response_type", "code"),
            ("client_id", "cid"),
            ("redirect_uri", "urn:ietf:wg:oauth:2.0:oob"),
            ("scope", "read write"),
            ("code_challenge", "chal"),
            ("code_challenge_method", "S256"),
        ] {
            assert!(
                pairs.contains(&(key.to_string(), value.to_string())),
                "{key}"
            );
        }
    }

    #[test]
    fn the_page_authorize_url_returns_to_the_page() {
        let url = page_authorize_url(
            "fosstodon.org",
            "cid",
            "chal",
            "http://127.0.0.1:5174/web/",
            "state1",
        )
        .unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert_eq!(parsed.host_str(), Some("fosstodon.org"));
        assert_eq!(parsed.path(), "/oauth/authorize");
        let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        for (key, value) in [
            ("response_type", "code"),
            ("client_id", "cid"),
            ("redirect_uri", "http://127.0.0.1:5174/web/"),
            ("scope", "read write"),
            ("state", "state1"),
            ("code_challenge", "chal"),
            ("code_challenge_method", "S256"),
        ] {
            assert!(
                pairs.contains(&(key.to_string(), value.to_string())),
                "{key}"
            );
        }
        assert!(!pairs.iter().any(|(_, value)| value.contains("oob")));
        let home = page_authorize_url(
            DEFAULT_SERVER,
            "cid",
            "chal",
            "http://127.0.0.1:5174/",
            "state1",
        )
        .unwrap();
        let home = url::Url::parse(&home).unwrap();
        assert_eq!(home.scheme(), "https");
        assert_eq!(home.host_str(), Some("mastodon.social"));
        assert_eq!(home.path(), "/oauth/authorize");
    }

    #[test]
    fn a_page_sign_in_rejects_a_query_callback_before_any_request() {
        let err = begin_page("mastodon.social", "http://127.0.0.1:5174/?x=1").unwrap_err();
        assert!(err.contains("no address"), "{err}");
        let err = begin_page("", "http://127.0.0.1:5174/?x=1").unwrap_err();
        assert!(err.contains("no address"), "{err}");
        let err = begin_page("   ", "http://127.0.0.1:5174/?x=1").unwrap_err();
        assert!(err.contains("no address"), "{err}");
    }

    #[test]
    fn a_wrong_or_empty_page_state_never_calls_the_server() {
        with_config(|| {
            let expired = "this sign-in has expired. start it again.";
            assert_eq!(finish_page("abc", "").unwrap_err(), expired);
            assert_eq!(finish_page("abc", "other-state").unwrap_err(), expired);
            remember_page_login();
            assert_eq!(finish_page("abc", "").unwrap_err(), expired);
            let saved = config::load().unwrap().mastodon;
            assert!(saved.oauth_state.is_none());
            assert!(saved.oauth_verifier.is_none());
            assert!(saved.oauth_redirect.is_none());
            assert_eq!(saved.client_id.as_deref(), Some("cid"));
            remember_page_login();
            assert_eq!(finish_page("abc", "other-state").unwrap_err(), expired);
            let saved = config::load().unwrap().mastodon;
            assert!(saved.oauth_state.is_none());
            assert!(saved.oauth_verifier.is_none());
            assert!(saved.oauth_redirect.is_none());
            assert_eq!(saved.server.as_deref(), Some(DEFAULT_SERVER));
            assert_eq!(saved.client_id.as_deref(), Some("cid"));
        });
    }

    fn remember_page_login() {
        config::update(|cfg| {
            cfg.mastodon.server = Some(DEFAULT_SERVER.to_string());
            cfg.mastodon.client_id = Some("cid".to_string());
            cfg.mastodon.client_secret = Some("sec".to_string());
            cfg.mastodon.oauth_state = Some("pending-state".to_string());
            cfg.mastodon.oauth_verifier = Some("verifier-value".to_string());
            cfg.mastodon.oauth_redirect = Some("http://127.0.0.1:5174/".to_string());
        })
        .unwrap();
    }

    /// Holds the process config dir for one test. Unit tests share this process.
    fn with_config(body: impl FnOnce()) {
        let _guard = crate::config::lock_config_env();
        let dir = std::env::temp_dir().join(format!(
            "socli-mastodon-login-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var_os("SOCLI_CONFIG_DIR");
        std::env::set_var("SOCLI_CONFIG_DIR", &dir);
        // Declared after the directory so a panic restores the variable before the directory goes.
        let _remove = RemoveDir(dir);
        let _restore = Restore(previous);
        body();
    }

    struct RemoveDir(std::path::PathBuf);

    impl Drop for RemoveDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Restore(Option<std::ffi::OsString>);

    impl Drop for Restore {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("SOCLI_CONFIG_DIR", value),
                None => std::env::remove_var("SOCLI_CONFIG_DIR"),
            }
        }
    }
}
