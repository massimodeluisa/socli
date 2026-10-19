/*!
Threads sign-in. The default opens Threads' approval window for the socli app and trades the
code at the socli.sh exchange, which holds the app secret. A pasted token, and OAuth with your
own app id and secret, stay available. A pasted token keeps the lifetime Meta gave it unless
you also pass the secret. Docs:
https://developers.facebook.com/documentation/threads/get-started/get-access-tokens-and-permissions
https://developers.facebook.com/documentation/threads/get-started/long-lived-tokens
*/

use crate::net::http::{Client, RequestBuilder};
use serde_json::Value;
use url::Url;

use super::{map, send, ThreadsClient};
use crate::config::{self, ThreadsConfig};
use crate::net::pkce::code_verifier;
use crate::net::{http, split, Network};
use crate::screens;
use crate::util;

const GRAPH: &str = "https://graph.threads.com";
const AUTHORIZE: &str = "https://threads.com/oauth/authorize";
const DAY: i64 = 86_400;
const LONG_LIVED: i64 = 60 * DAY;
const REFRESH_WINDOW: i64 = 7 * DAY;

pub const DEFAULT_SCOPES: &str = "threads_basic,threads_content_publish,threads_read_replies,threads_manage_replies,threads_manage_mentions,threads_keyword_search,threads_delete";

/// Fixed loopback the terminal listens on. Meta will not accept it as a redirect.
pub const LOOPBACK_PORT: u16 = 8742;
pub const LOOPBACK_REDIRECT: &str = "http://127.0.0.1:8742/threads/callback";

/// Callback saved on the Threads app. The site sends a `th.` state on to the loopback.
pub const PUBLIC_REDIRECT: &str = "https://socli.sh/";

/// The exchange holds the Threads app secret. The client sends the code and the redirect only.
pub const EXCHANGE_URL: &str = "https://socli.sh/threads/token";

pub const NO_APP: &str = "this build has no socli Threads app";

/// SoCLI's public Threads app id. The id is not a secret.
const DEFAULT_APP_ID: &str = "2168336333756240";

/// The public Threads app id. `SOCLI_THREADS_APP_ID` replaces it at compile time.
/// An empty value is a build with no app, so the missing-id path stays testable.
pub fn compiled_app_id() -> Option<&'static str> {
    resolve_app_id(option_env!("SOCLI_THREADS_APP_ID"))
}

fn resolve_app_id(from_env: Option<&'static str>) -> Option<&'static str> {
    match from_env.map(str::trim) {
        Some("") => None,
        Some(id) => Some(id),
        None => Some(DEFAULT_APP_ID),
    }
}

fn now() -> i64 {
    crate::clock::unix_secs() as i64
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env(key: &str) -> Option<String> {
    non_empty(std::env::var(key).ok())
}

/** A token is refreshable once it is over a day old; it is worth refreshing in its last week. */
pub fn needs_refresh(issued_at: Option<i64>, expires_at: Option<i64>, now: i64) -> bool {
    match (issued_at, expires_at) {
        (Some(issued), Some(expires)) => {
            now - issued > DAY && expires > now && expires - now <= REFRESH_WINDOW
        }
        _ => false,
    }
}

/** The OAuth `code` from a pasted redirect URL: `#_` stripped, errors and a wrong `state` rejected. */
pub fn parse_redirect(input: &str, state: &str) -> Result<String, String> {
    let url = Url::parse(input.trim()).map_err(|_| {
        "that is not a url. paste the full address the browser was sent to, then try again"
            .to_string()
    })?;
    let param = |key: &str| {
        url.query_pairs()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.into_owned())
    };
    if let Some(error) = param("error") {
        let reason = param("error_description").unwrap_or(error);
        return Err(format!(
            "Threads did not authorize the app: {reason}. run `socli auth threads oauth` to try again"
        ));
    }
    if param("state").as_deref() != Some(state) {
        return Err(
            "the state in that url does not match this sign-in. run `socli auth threads oauth` again and paste the new redirect"
                .into(),
        );
    }
    let code = param("code").ok_or_else(|| {
        "that url has no code. paste the address after you approve the app".to_string()
    })?;
    Ok(authorization_code(&code))
}

/** The authorization code with Meta's trailing `#_` removed. A real trailing `_` stays. */
fn authorization_code(raw: &str) -> String {
    raw.trim().trim_end_matches("#_").to_string()
}

pub fn authorize_url(app_id: &str, redirect_uri: &str, scopes: &str, state: &str) -> String {
    let mut url = Url::parse(AUTHORIZE).expect("authorize url is a constant");
    url.query_pairs_mut()
        .append_pair("client_id", app_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", scopes)
        .append_pair("response_type", "code")
        .append_pair("state", state);
    url.into()
}

struct Issued {
    access_token: String,
    expires_in: Option<i64>,
}

fn token_from(body: &Value) -> Result<Issued, String> {
    let access_token = map::text_of(body, "access_token")
        .ok_or("Threads sent no token. try again")?
        .to_string();
    Ok(Issued {
        access_token,
        expires_in: body.get("expires_in").and_then(Value::as_i64),
    })
}

fn call(builder: RequestBuilder) -> Result<Value, String> {
    send(builder).map_err(|e| e.to_string())
}

fn exchange_long_lived(http: &Client, token: &str, secret: &str) -> Result<Issued, String> {
    let body = call(http.get(format!("{GRAPH}/access_token")).query(&[
        ("grant_type", "th_exchange_token"),
        ("client_secret", secret),
        ("access_token", token),
    ]))?;
    token_from(&body)
}

fn refresh_token(http: &Client, token: &str) -> Result<Issued, String> {
    let body = call(
        http.get(format!("{GRAPH}/refresh_access_token"))
            .query(&[("grant_type", "th_refresh_token"), ("access_token", token)]),
    )?;
    token_from(&body)
}

/** Refreshes a stored token inside its window and saves it. A failure keeps the token you have. */
pub fn auto_refresh(http: &Client, section: &ThreadsConfig, stored: String) -> String {
    if !needs_refresh(section.issued_at, section.expires_at, now()) {
        return stored;
    }
    match refresh_token(http, &stored) {
        Ok(issued) => {
            save_refreshed(&issued);
            issued.access_token
        }
        Err(_) => stored,
    }
}

fn save_refreshed(issued: &Issued) {
    let at = now();
    let _ = config::update(|cfg| {
        cfg.threads.access_token = Some(issued.access_token.clone());
        cfg.threads.issued_at = Some(at);
        cfg.threads.expires_at = Some(at + issued.expires_in.unwrap_or(LONG_LIVED));
    });
}

fn date(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/** Checks the token against `/me`, stores it, and prints the signed-in screen. */
fn finish(http: Client, issued: Issued) -> Result<(), String> {
    let client = ThreadsClient::with_token(http, issued.access_token.clone());
    let user = match client.profile() {
        Ok(user) => user,
        Err(crate::net::NetError::SignInRequired(_)) => {
            return Err("Threads rejected the access token. Paste the token from your Meta app and run `socli login threads` again".into());
        }
        Err(err) => return Err(err.to_string()),
    };
    let user_id = split(&user.id).map(|(_, native)| native.to_string());
    let at = now();
    let expires_at = issued.expires_in.map(|secs| at + secs);
    config::update(|cfg| {
        cfg.threads.user_id = user_id;
        cfg.threads.username = Some(user.username.clone());
        cfg.threads.access_token = Some(issued.access_token.clone());
        cfg.threads.issued_at = Some(at);
        cfg.threads.expires_at = expires_at;
        cfg.threads.oauth_state = None;
        cfg.threads.oauth_redirect = None;
    })?;
    let detail = match expires_at {
        Some(expires_at) => format!("token valid until {}", date(expires_at)),
        None => "token lifetime unknown; add --app-secret to swap it for a 60-day token that renews itself".to_string(),
    };
    screens::print_signed_in(&user.username, Some(&detail))
}

/// The page's approval screen. With no baked-in app id it says so, and it does not teach a personal app.
pub fn page_copy() -> (Vec<(String, String)>, String) {
    let aside = if compiled_app_id().is_some() {
        "Enter opens Threads. Escape closes this.".to_string()
    } else {
        format!("{NO_APP}. Escape closes this.")
    };
    (
        vec![("Sign-in method".into(), "Approve socli on Threads".into())],
        aside,
    )
}

pub fn login(token: Option<String>, app_secret: Option<String>) -> Result<(), String> {
    let http = http::client().map_err(|e| e.to_string())?;
    let hint = "Pass the token: socli login threads --token <token>";
    if let Some(token) = non_empty(token).or_else(|| env("THREADS_ACCESS_TOKEN")) {
        let _ = screens::present_login(
            Network::Threads.title(),
            &[
                ("Sign-in method", "Access token from your Meta app"),
                ("Token", "••••"),
            ],
            "",
            None,
        )?;
        let issued = match non_empty(app_secret).or_else(|| env("THREADS_APP_SECRET")) {
            Some(secret) => exchange_long_lived(&http, &token, &secret).unwrap_or_else(|_| {
                let _ =
                    util::say("could not swap for a long-lived token, keeping the one you gave");
                Issued {
                    access_token: token.clone(),
                    expires_in: None,
                }
            }),
            None => Issued {
                access_token: token,
                expires_in: None,
            },
        };
        return finish(http, issued);
    }
    if !util::person_present() {
        return Err(format!("{} {hint}", util::NEEDS_PERSON));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = http;
        return Err(format!("{NO_APP}. {hint}"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        login_shared(http)
    }
}

/** `th.` plus a random state. The site bounces that prefix to the loopback. Threads has no PKCE. */
#[cfg(any(test, not(target_arch = "wasm32")))]
fn terminal_login(app_id: &str) -> (String, String) {
    let state = format!("th.{}", code_verifier());
    let url = authorize_url(app_id, PUBLIC_REDIRECT, DEFAULT_SCOPES, &state);
    (url, state)
}

/// Opens Threads and catches the loopback redirect, then asks socli.sh to exchange the code.
#[cfg(not(target_arch = "wasm32"))]
fn login_shared(http: Client) -> Result<(), String> {
    use std::net::TcpListener;

    let Some(app_id) = compiled_app_id() else {
        let _ = screens::present_login(
            Network::Threads.title(),
            &[("Sign-in method", "Approve socli on Threads")],
            NO_APP,
            None,
        );
        return Err(format!(
            "{NO_APP}. Pass the token: socli login threads --token <token>"
        ));
    };
    let (url, state) = terminal_login(app_id);
    let listener = TcpListener::bind(("127.0.0.1", LOOPBACK_PORT)).map_err(|err| {
        format!(
            "could not listen for {LOOPBACK_REDIRECT} ({err}). the Threads callback uses port {LOOPBACK_PORT}."
        )
    })?;
    let _ = screens::present_login(
        Network::Threads.title(),
        &[
            ("Sign-in method", "Approve socli on Threads"),
            ("Waiting for the browser", "the address is on the right"),
        ],
        &format!("{url}\n\nwaiting for the callback on {PUBLIC_REDIRECT}"),
        None,
    )?;
    let _ = util::open_browser(&url);
    let (code, got_state) = accept_code(&listener)?;
    if got_state != state {
        return Err("oauth state did not match. try `socli login threads` again.".into());
    }
    let issued = exchange_code(&http, &code, PUBLIC_REDIRECT)?;
    finish(http, issued)
}

#[cfg(not(target_arch = "wasm32"))]
fn accept_code(listener: &std::net::TcpListener) -> Result<(String, String), String> {
    use std::io::Read;

    let (mut stream, _) = listener
        .accept()
        .map_err(|err| format!("waiting for the browser: {err}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(180)))
        .ok();
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).map_err(|err| err.to_string())?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .trim_end_matches("#_");
    let fake = Url::parse(&format!("http://127.0.0.1{path}")).map_err(|err| err.to_string())?;
    let param = |key: &str| {
        fake.query_pairs()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.into_owned())
    };
    if let Some(error) = param("error") {
        let _ = write_callback(
            &mut stream,
            "Threads did not grant access. You can close this tab.",
        );
        return Err(format!("authorization failed: {error}"));
    }
    let code =
        authorization_code(&param("code").ok_or_else(|| "callback had no code".to_string())?);
    if code.is_empty() {
        return Err("callback had no code".into());
    }
    let state = param("state").unwrap_or_default();
    let _ = write_callback(
        &mut stream,
        "signed in. close this tab and go back to the terminal.",
    );
    Ok((code, state))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_callback(stream: &mut impl std::io::Write, message: &str) -> std::io::Result<()> {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>socli</title>\
<body style=\"background:#000;color:#eee;font-family:ui-monospace,monospace;padding:3rem\">\
<p>{message}</p></body>"
    );
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// Starts the page approval and returns the Threads authorize URL. The state stays in the config.
pub fn begin_page(redirect: &str) -> Result<String, String> {
    let app_id = compiled_app_id().ok_or_else(|| NO_APP.to_string())?;
    begin_with(app_id, redirect)
}

/// Same as [`begin_page`] with an explicit app id, so a test can build the URL without a baked-in id.
pub fn begin_with(app_id: &str, redirect: &str) -> Result<String, String> {
    let redirect = crate::util::page_callback(redirect)?;
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return Err(NO_APP.into());
    }
    let state = code_verifier();
    let url = authorize_url(app_id, &redirect, DEFAULT_SCOPES, &state);
    config::update(|cfg| {
        cfg.threads.oauth_state = Some(state);
        cfg.threads.oauth_redirect = Some(redirect);
    })?;
    Ok(url)
}

/// Trades the page's code at the socli.sh exchange. A mismatched state never calls it.
pub fn finish_page(code: &str, got_state: &str) -> Result<(), String> {
    let saved = config::load()?.threads;
    let pending = saved.oauth_state.clone().unwrap_or_default();
    let redirect = saved.oauth_redirect.clone().unwrap_or_default();
    clear_page_oauth()?;
    let code = authorization_code(code);
    let code_ok = !code.is_empty() && code.len() <= 512 && code.chars().all(|c| !c.is_whitespace());
    if !code_ok || pending.is_empty() || pending != got_state || redirect.is_empty() {
        return Err("this sign-in has expired. start it again.".into());
    }
    let http = http::client().map_err(|e| e.to_string())?;
    let issued = exchange_code(&http, &code, &redirect)?;
    finish(http, issued)
}

fn clear_page_oauth() -> Result<(), String> {
    config::update(|cfg| {
        cfg.threads.oauth_state = None;
        cfg.threads.oauth_redirect = None;
    })?;
    Ok(())
}

fn exchange_code(http: &Client, code: &str, redirect: &str) -> Result<Issued, String> {
    let body = call(http.post(EXCHANGE_URL).json(&serde_json::json!({
        "code": code,
        "redirect_uri": redirect,
    })))?;
    if let Some(message) = body.get("error").and_then(Value::as_str) {
        let message = message.trim();
        if !message.is_empty() {
            return Err(message.to_string());
        }
    }
    token_from(&body)
}

pub fn oauth(
    app_id: String,
    app_secret: String,
    redirect_uri: String,
    scopes: Option<String>,
) -> Result<(), String> {
    let (app_id, app_secret, redirect_uri) = (
        app_id.trim().to_string(),
        app_secret.trim().to_string(),
        redirect_uri.trim().to_string(),
    );
    if app_id.is_empty() || app_secret.is_empty() || redirect_uri.is_empty() {
        return Err("the app id, app secret and redirect uri cannot be empty. check the values from your Meta app dashboard and run `socli auth threads oauth` again".into());
    }
    let scopes = non_empty(scopes).unwrap_or_else(|| DEFAULT_SCOPES.to_string());
    let http = http::client().map_err(|e| e.to_string())?;
    let state = code_verifier();
    let url = authorize_url(&app_id, &redirect_uri, &scopes, &state);
    let _ = screens::present_login(
        Network::Threads.title(),
        &[
            ("Sign-in method", "OAuth in your browser"),
            ("Redirect", "paste the address you land on"),
        ],
        &format!("{url}\n\nPaste the address the browser lands on."),
        Some("Redirect url:"),
    )?;
    let _ = util::open_browser(&url);
    let pasted = util::prompt_line(
        "",
        "Open the sign-in in a browser yourself: socli auth threads oauth",
    )?;
    let code = parse_redirect(&pasted, &state)?;
    let body = call(http.post(format!("{GRAPH}/oauth/access_token")).form(&[
        ("client_id", app_id.as_str()),
        ("client_secret", app_secret.as_str()),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri.as_str()),
        ("code", code.as_str()),
    ]))?;
    let short = token_from(&body)?;
    let issued = exchange_long_lived(&http, &short.access_token, &app_secret).map_err(|e| {
        format!("{e}. the short-lived token lasts an hour, so run `socli auth threads oauth` again")
    })?;
    finish(http, issued)
}

pub fn refresh() -> Result<(), String> {
    let cfg = config::load()?;
    let token = non_empty(cfg.threads.access_token.clone())
        .ok_or("no stored Threads token. run `socli login threads` first")?;
    let http = http::client().map_err(|e| e.to_string())?;
    let issued = refresh_token(&http, &token).map_err(|e| {
        format!("{e}. tokens refresh once they are a day old and before they expire, otherwise run `socli login threads`")
    })?;
    save_refreshed(&issued);
    let until = date(now() + issued.expires_in.unwrap_or(LONG_LIVED));
    util::say(format!("Refreshed the Threads token, valid until {until}"))
}

pub fn logout() -> Result<(), String> {
    config::update(|cfg| cfg.threads = ThreadsConfig::default()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::scoped;

    const NOW: i64 = 10_000_000;

    fn with_config(body: impl FnOnce() + std::panic::UnwindSafe) {
        let _guard = crate::config::lock_config_env();
        let dir = std::env::temp_dir().join(format!(
            "socli-threads-login-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var_os("SOCLI_CONFIG_DIR");
        std::env::set_var("SOCLI_CONFIG_DIR", &dir);
        let result = std::panic::catch_unwind(body);
        match previous {
            Some(value) => std::env::set_var("SOCLI_CONFIG_DIR", value),
            None => std::env::remove_var("SOCLI_CONFIG_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    fn redirect_strips_the_trailing_marker() {
        let url = "https://example.com/cb?code=AQBxyz&state=abc#_";
        assert_eq!(parse_redirect(url, "abc").as_deref(), Ok("AQBxyz"));
    }

    #[test]
    fn redirect_strips_the_marker_when_state_comes_last() {
        let url = "https://example.com/cb?state=abc&code=AQBxyz#_";
        assert_eq!(parse_redirect(url, "abc").as_deref(), Ok("AQBxyz"));
    }

    #[test]
    fn redirect_strips_a_suffix_glued_to_the_code() {
        let url = "https://example.com/cb?code=AQB_xyz%23_&state=abc";
        assert_eq!(parse_redirect(url, "abc").as_deref(), Ok("AQB_xyz"));
    }

    #[test]
    fn authorization_code_strips_the_meta_suffix_only() {
        assert_eq!(authorization_code("  AQB_xyz#_"), "AQB_xyz");
        assert_eq!(authorization_code("AQB_xyz#_#_"), "AQB_xyz");
        assert_eq!(authorization_code("AQB_xyz_"), "AQB_xyz_");
        assert_eq!(authorization_code("AQB_xyz"), "AQB_xyz");
    }

    #[test]
    fn redirect_rejects_a_wrong_state() {
        let err = parse_redirect("https://example.com/cb?code=AQB&state=zzz#_", "abc").unwrap_err();
        assert!(err.contains("state"));
        assert!(err.contains("socli auth threads oauth"));
    }

    #[test]
    fn redirect_reports_a_denied_authorization() {
        let url = "https://example.com/cb?error=access_denied&error_reason=user_denied&error_description=The+user+denied+your+request&state=abc#_";
        let err = parse_redirect(url, "abc").unwrap_err();
        assert!(err.contains("The user denied your request"));
    }

    #[test]
    fn redirect_rejects_text_that_is_not_a_url() {
        assert!(parse_redirect("AQBxyz", "abc").is_err());
    }

    #[test]
    fn redirect_without_a_code_is_rejected() {
        assert!(parse_redirect("https://example.com/cb?state=abc", "abc").is_err());
    }

    #[test]
    fn authorize_url_carries_scopes_and_state() {
        let url = authorize_url(
            "123",
            "https://example.com/cb",
            "threads_basic,threads_delete",
            "s1",
        );
        assert!(url.starts_with("https://threads.com/oauth/authorize?"));
        assert!(url.contains("client_id=123"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=s1"));
        assert!(url.contains("scope=threads_basic%2Cthreads_delete"));
    }

    #[test]
    fn authorize_url_keeps_the_given_redirect() {
        let redirect = "https://example.com/threads/done";
        let state = "page-state";
        let url = authorize_url("123", redirect, DEFAULT_SCOPES, state);
        let parsed = Url::parse(&url).unwrap();
        let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        let value = |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, item)| item.clone())
        };
        assert_eq!(value("redirect_uri").as_deref(), Some(redirect));
        assert_eq!(value("state").as_deref(), Some(state));
        assert!(value("code_challenge").is_none());
        assert!(value("code_verifier").is_none());
        assert!(value("code_challenge_method").is_none());
        assert!(!url.contains("127.0.0.1"), "{url}");
    }

    #[test]
    fn refresh_waits_until_the_token_is_a_day_old() {
        let issued = NOW - DAY + 60;
        assert!(!needs_refresh(Some(issued), Some(NOW + DAY), NOW));
    }

    #[test]
    fn refresh_waits_until_the_last_week() {
        let issued = NOW - 30 * DAY;
        assert!(!needs_refresh(Some(issued), Some(NOW + 30 * DAY), NOW));
    }

    #[test]
    fn refresh_runs_inside_the_window() {
        let issued = NOW - 54 * DAY;
        assert!(needs_refresh(Some(issued), Some(NOW + 6 * DAY), NOW));
        assert!(needs_refresh(Some(issued), Some(NOW + 7 * DAY), NOW));
    }

    #[test]
    fn refresh_skips_an_expired_token() {
        let issued = NOW - 61 * DAY;
        assert!(!needs_refresh(Some(issued), Some(NOW - 1), NOW));
    }

    #[test]
    fn refresh_skips_a_token_with_unknown_dates() {
        assert!(!needs_refresh(None, Some(NOW + DAY), NOW));
        assert!(!needs_refresh(Some(NOW - 2 * DAY), None, NOW));
    }

    #[test]
    fn the_page_authorize_url_uses_the_app_id_and_a_wrong_state_does_not_call_threads() {
        let _guard = crate::config::lock_config_env();
        let dir = std::env::temp_dir().join(format!(
            "socli-threads-login-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var_os("SOCLI_CONFIG_DIR");
        std::env::set_var("SOCLI_CONFIG_DIR", &dir);
        let result = std::panic::catch_unwind(|| {
            let shared = begin_page("http://127.0.0.1:5174/").unwrap();
            assert!(shared.contains("client_id=2168336333756240"), "{shared}");
            assert_eq!(compiled_app_id(), Some("2168336333756240"));
            assert_eq!(resolve_app_id(Some("")), None);
            assert_eq!(resolve_app_id(Some("  ")), None);
            assert_eq!(resolve_app_id(Some("99")), Some("99"));
            assert!(begin_with("", "http://127.0.0.1:5174/").is_err());
            assert!(begin_with("123", "http://localhost:5174/").is_err());
            let url = begin_with("123", "http://127.0.0.1:5174/web/").unwrap();
            let parsed = Url::parse(&url).unwrap();
            assert_eq!(parsed.host_str(), Some("threads.com"));
            assert_eq!(parsed.path(), "/oauth/authorize");
            let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
            let value = |key: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, item)| item.clone())
            };
            assert_eq!(value("client_id").as_deref(), Some("123"));
            assert_eq!(
                value("redirect_uri").as_deref(),
                Some("http://127.0.0.1:5174/web/")
            );
            assert_eq!(value("scope").as_deref(), Some(DEFAULT_SCOPES));
            let state = value("state").unwrap();
            assert!(!state.starts_with("th."), "{state}");
            assert_eq!(state.len(), 64);
            assert!(!state.contains('='));
            assert!(pairs.iter().all(|(name, _)| {
                name != "code_challenge"
                    && name != "code_verifier"
                    && name != "code_challenge_method"
            }));
            let saved = config::load().unwrap().threads;
            assert_eq!(saved.oauth_state.as_deref(), Some(state.as_str()));
            assert_eq!(
                saved.oauth_redirect.as_deref(),
                Some("http://127.0.0.1:5174/web/")
            );
            let err = finish_page("AQBxyz#_", "wrong").unwrap_err();
            assert_eq!(err, "this sign-in has expired. start it again.");
            assert!(config::load().unwrap().threads.oauth_state.is_none());
        });
        match previous {
            Some(value) => std::env::set_var("SOCLI_CONFIG_DIR", value),
            None => std::env::remove_var("SOCLI_CONFIG_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    #[test]
    fn the_terminal_redirect_is_the_https_callback_meta_allows() {
        assert_eq!(LOOPBACK_PORT, 8742);
        assert_eq!(LOOPBACK_REDIRECT, "http://127.0.0.1:8742/threads/callback");
        assert_eq!(PUBLIC_REDIRECT, "https://socli.sh/");
        let url = authorize_url(
            "123",
            PUBLIC_REDIRECT,
            DEFAULT_SCOPES,
            "th.abcdefghijklmnopqrstuvwxyz",
        );
        assert!(
            url.contains("redirect_uri=https%3A%2F%2Fsocli.sh%2F"),
            "{url}"
        );
        assert!(url.contains("state=th.abcdefghijklmnopqrstuvwxyz"), "{url}");
        assert!(!url.contains("127.0.0.1"), "{url}");
        assert!(url.starts_with("https://threads.com/oauth/authorize?"));
    }

    #[test]
    fn terminal_state_starts_with_th_and_is_not_pkce() {
        let (url, state) = terminal_login("123");
        assert!(state.starts_with("th."), "{state}");
        assert_eq!(state.len(), "th.".len() + 64);
        let suffix = &state["th.".len()..];
        assert_eq!(suffix.len(), 64);
        assert!(!suffix.contains('='));
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.host_str(), Some("threads.com"));
        assert_eq!(parsed.path(), "/oauth/authorize");
        let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
        let value = |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, item)| item.clone())
        };
        assert_eq!(value("redirect_uri").as_deref(), Some(PUBLIC_REDIRECT));
        assert_eq!(value("state").as_deref(), Some(state.as_str()));
        assert!(pairs.iter().all(|(_, item)| item != suffix));
        assert!(value("code_challenge").is_none());
        assert!(value("code_verifier").is_none());
        assert!(value("code_challenge_method").is_none());
        assert!(!url.contains("127.0.0.1"), "{url}");
        assert!(url.starts_with("https://threads.com/oauth/authorize?"));
    }

    #[test]
    fn page_state_has_no_th_prefix() {
        with_config(|| {
            let redirect = "https://example.com/threads/done";
            let url = begin_with("123", redirect).unwrap();
            let parsed = Url::parse(&url).unwrap();
            let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
            let value = |key: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, item)| item.clone())
            };
            assert_eq!(value("redirect_uri").as_deref(), Some(redirect));
            let state = value("state").unwrap();
            assert!(!state.starts_with("th."), "{state}");
            assert_eq!(state.len(), 64);
            assert!(!state.contains('='));
            assert!(value("code_challenge").is_none());
            assert!(value("code_verifier").is_none());
            assert!(value("code_challenge_method").is_none());
            let saved = config::load().unwrap().threads;
            assert_eq!(saved.oauth_state.as_deref(), Some(state.as_str()));
            assert_eq!(saved.oauth_redirect.as_deref(), Some(redirect));
            let err = finish_page("AQBxyz#_", "th.not-the-page-state").unwrap_err();
            assert_eq!(err, "this sign-in has expired. start it again.");
            assert!(config::load().unwrap().threads.oauth_state.is_none());
        });
    }

    #[test]
    fn default_scopes_include_delete() {
        assert!(DEFAULT_SCOPES.split(',').any(|s| s == "threads_delete"));
        assert!(DEFAULT_SCOPES.split(',').any(|s| s == "threads_basic"));
    }

    #[test]
    fn scoped_ids_round_trip() {
        let id = scoped(Network::Threads, "1789");
        assert_eq!(split(&id), Some((Network::Threads, "1789")));
    }
}
