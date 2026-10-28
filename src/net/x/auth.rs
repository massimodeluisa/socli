/*!
X sign-in: OAuth 2.0 PKCE for a user, plus an app-only bearer for public reads. Tokens live in
the `[x]` section of the config file. `X_*` environment variables win, so a shell can try a
token without writing it down.
*/

use std::io::{Read, Write};
use std::net::TcpListener;

use crate::config::{self, XConfig};
use crate::net::pkce::{code_challenge, code_verifier};

pub const DEFAULT_SCOPES: &str =
    "tweet.read tweet.write users.read like.write follows.write bookmark.read bookmark.write offline.access";

pub const DEFAULT_LATEST_QUERY: &str = "lang:en -is:retweet";

#[derive(Clone, Debug, Default)]
pub struct Credentials {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub bearer_token: Option<String>,
    pub latest_query: Option<String>,
    /// Browser session cookie. Together with `ct0` this is the default sign-in.
    pub auth_token: Option<String>,
    /// CSRF cookie paired with `auth_token`.
    pub ct0: Option<String>,
}

impl Credentials {
    /// The `[x]` section with the `X_*` environment variables laid over it.
    pub fn from_config(section: &XConfig) -> Self {
        let mut creds = Self::from_section(section);
        overlay(&mut creds, |key| std::env::var(key).ok());
        creds
    }

    fn from_section(section: &XConfig) -> Self {
        Credentials {
            client_id: section.client_id.clone(),
            client_secret: section.client_secret.clone(),
            access_token: section.access_token.clone(),
            refresh_token: section.refresh_token.clone(),
            expires_at: section.expires_at,
            user_id: section.user_id.clone(),
            username: section.username.clone(),
            bearer_token: section.bearer_token.clone(),
            latest_query: section.latest_query.clone(),
            auth_token: section.auth_token.clone(),
            ct0: section.ct0.clone(),
        }
    }

    /// The x.com browser session. It can read the real home timelines and write likes.
    pub fn uses_web(&self) -> bool {
        nonempty(&self.auth_token) && nonempty(&self.ct0)
    }

    pub fn has_user(&self) -> bool {
        nonempty(&self.access_token) || self.uses_web()
    }

    pub fn has_bearer(&self) -> bool {
        self.bearer_token.as_deref().is_some_and(|t| !t.is_empty())
    }

    pub fn has_any_read(&self) -> bool {
        self.has_user() || self.has_bearer()
    }

    pub fn latest_query(&self) -> &str {
        self.latest_query
            .as_deref()
            .filter(|q| !q.trim().is_empty())
            .unwrap_or(DEFAULT_LATEST_QUERY)
    }
}

fn overlay(creds: &mut Credentials, env: impl Fn(&str) -> Option<String>) {
    let value = |key: &str| {
        env(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let fields: [(&str, &mut Option<String>); 3] = [
        ("X_CLIENT_ID", &mut creds.client_id),
        ("X_CLIENT_SECRET", &mut creds.client_secret),
        ("X_BEARER_TOKEN", &mut creds.bearer_token),
    ];
    for (key, field) in fields {
        if let Some(v) = value(key) {
            *field = Some(v);
        }
    }
    let access = value("X_ACCESS_TOKEN");
    let refresh = value("X_REFRESH_TOKEN");
    let auth = value("X_AUTH_TOKEN");
    let ct0 = value("X_CT0");
    let browser = auth.is_some() && ct0.is_some();
    if browser || (access.is_none() && (auth.is_some() || ct0.is_some())) {
        if creds.auth_token != auth || creds.ct0 != ct0 {
            creds.user_id = None;
            creds.username = None;
        }
        creds.auth_token = auth;
        creds.ct0 = ct0;
        creds.access_token = None;
        creds.refresh_token = None;
        creds.expires_at = None;
    } else if let Some(access) = access {
        // An environment token is one session, not a replacement access token for a stored account.
        if creds.access_token.as_deref() != Some(access.as_str()) {
            creds.refresh_token = None;
            creds.expires_at = None;
            creds.user_id = None;
            creds.username = None;
        } else if creds.uses_web() {
            creds.user_id = None;
            creds.username = None;
        }
        creds.access_token = Some(access);
        creds.auth_token = None;
        creds.ct0 = None;
        if let Some(refresh) = refresh {
            creds.refresh_token = Some(refresh);
        }
    } else if let Some(refresh) = refresh {
        creds.refresh_token = Some(refresh);
    }
    if let Some(user_id) = value("X_USER_ID") {
        creds.user_id = Some(user_id);
    }
    if let Some(username) = value("X_USERNAME") {
        creds.username = Some(username);
    }
}

/// The `x` rows of `socli auth status`, secrets redacted.
pub fn status(section: &XConfig) -> Vec<(&'static str, String)> {
    let creds = Credentials::from_config(section);
    let account = match creds.username.as_deref().filter(|s| !s.is_empty()) {
        Some(name) if creds.has_user() => format!("@{name}"),
        _ if creds.has_user() => "signed in".into(),
        _ if creds.has_bearer() => "not signed in · app bearer for public reads".into(),
        _ => "not signed in".into(),
    };
    vec![
        ("account", account),
        (
            "browser session",
            config::redact(creds.auth_token.as_deref()),
        ),
        ("user token", config::redact(creds.access_token.as_deref())),
        ("app bearer", config::redact(creds.bearer_token.as_deref())),
        ("client id", config::redact(creds.client_id.as_deref())),
    ]
}

fn nonempty(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !text.is_empty())
}

pub fn logout() -> Result<(), String> {
    config::update(|cfg| {
        cfg.x.access_token = None;
        cfg.x.refresh_token = None;
        cfg.x.expires_at = None;
        cfg.x.user_id = None;
        cfg.x.username = None;
        cfg.x.bearer_token = None;
        cfg.x.auth_token = None;
        cfg.x.ct0 = None;
        cfg.x.oauth_state = None;
        cfg.x.oauth_verifier = None;
        cfg.x.oauth_redirect = None;
    })?;
    Ok(())
}

pub fn save_bearer(token: String) -> Result<(), String> {
    config::update(|cfg| cfg.x.bearer_token = Some(token))?;
    Ok(())
}

pub fn save_token(
    access_token: String,
    user_id: Option<String>,
    username: Option<String>,
    refresh_token: Option<String>,
) -> Result<(), String> {
    config::update(|cfg| {
        cfg.x.access_token = Some(access_token);
        cfg.x.user_id = user_id;
        cfg.x.username = username;
        cfg.x.refresh_token = refresh_token;
        cfg.x.expires_at = None;
        cfg.x.auth_token = None;
        cfg.x.ct0 = None;
    })?;
    Ok(())
}

const READING: &str = "Reading that session now. macOS may ask to use the keychain.";
const OTHER_ACCOUNT: &str = "A different account on x.com";

struct AccountRow<'a> {
    browser: &'a str,
    profile: &'a str,
    dir: &'a str,
}

/// Asks which X account to use, then imports that browser session.
///
/// Each signed-in browser profile is a row. The last row opens x.com so a second account can be
/// approved. `--browser` and `--profile` skip the question when they match one session.
pub fn login_browser(browser: Option<&str>, profile: Option<&str>) -> Result<(), String> {
    let _ = crate::screens::present_login(
        crate::net::Network::X.title(),
        &[
            ("How to sign in", "Pick the account socli should use"),
            ("Browsers", "Chrome, Brave, Edge, Arc or Chromium"),
        ],
        "",
        None,
    )?;
    let found = super::cookies::find_sessions(browser, profile)?;
    let pinned = browser.is_some() && profile.is_some();
    let session = if pinned && found.len() == 1 {
        crate::screens::login_aside(READING)?;
        super::cookies::read_session(&found[0])?
    } else if !crate::util::person_present() {
        if found.len() == 1 {
            crate::screens::login_aside(READING)?;
        }
        super::cookies::import(browser, profile)?
    } else {
        if found.is_empty() {
            let where_ = match browser {
                Some(name) => format!(" in {name}"),
                None => " in Chrome, Brave, Edge, Arc or Chromium".to_string(),
            };
            crate::screens::login_aside(&format!("No X session{where_} yet."))?;
        }
        let rows: Vec<AccountRow<'_>> = found
            .iter()
            .map(|session| AccountRow {
                browser: session.browser_label(),
                profile: session.display_name(),
                dir: session.profile_dir(),
            })
            .collect();
        let labels = account_menu(&rows);
        let index = crate::screens::prompt_rail("Account", &labels)?;
        if index == found.len() {
            return oauth(None, None, None, 8741, false);
        }
        crate::screens::login_aside(READING)?;
        super::cookies::read_session(&found[index])?
    };
    let browser_name = session.browser.clone();
    let profile_name = session.profile.clone();
    let creds = Credentials {
        auth_token: Some(session.auth_token.clone()),
        ct0: Some(session.ct0.clone()),
        ..Credentials::default()
    };
    let client = super::XClient::new(creds)
        .map_err(|err| format!("reading the profile failed: {err}. run `socli login x` again."))?;
    let user = accept_browser_session(session.auth_token, session.ct0, &client)?;
    crate::screens::print_signed_in(
        &user.username,
        Some(&format!("{browser_name} · {profile_name}")),
    )?;
    Ok(())
}

/// Browser profiles first, then the row that signs in as someone else on x.com.
fn account_menu(found: &[AccountRow<'_>]) -> Vec<String> {
    let base: Vec<String> = found
        .iter()
        .map(|row| format!("{} · {}", row.browser, row.profile))
        .collect();
    let mut labels = Vec::with_capacity(base.len() + 1);
    for (index, row) in found.iter().enumerate() {
        let label = &base[index];
        let unique = base.iter().filter(|other| *other == label).count() == 1;
        if unique {
            labels.push(label.clone());
        } else {
            labels.push(format!("{label} ({})", row.dir));
        }
    }
    labels.push(OTHER_ACCOUNT.to_string());
    labels
}

/**
Resolves the account from the in-memory browser session, then writes the cookies and that
account together. A failed profile read leaves the config file untouched.
*/
pub(crate) fn accept_browser_session(
    auth_token: String,
    ct0: String,
    client: &super::XClient,
) -> Result<crate::model::User, String> {
    let user = client
        .me()
        .map_err(|err| format!("reading the profile failed: {err}. run `socli login x` again."))?;
    config::update(|cfg| {
        cfg.x.auth_token = Some(auth_token);
        cfg.x.ct0 = Some(ct0);
        cfg.x.user_id = Some(user.id.clone());
        cfg.x.username = Some(user.username.clone());
    })?;
    Ok(user)
}

/** Pasted browser cookies. The profile is read before anything is written. */
pub fn login_session(auth_token: String, ct0: String) -> Result<(), String> {
    let creds = Credentials {
        auth_token: Some(auth_token.clone()),
        ct0: Some(ct0.clone()),
        ..Credentials::default()
    };
    let client = super::XClient::new(creds)
        .map_err(|err| format!("reading the profile failed: {err}. run `socli login x` again."))?;
    let user = accept_browser_session(auth_token, ct0, &client)?;
    crate::screens::print_signed_in(&user.username, None)?;
    Ok(())
}

pub fn login(
    client_id: Option<String>,
    client_secret: Option<String>,
    scopes: Option<String>,
    port: u16,
) -> Result<(), String> {
    oauth(client_id, client_secret, scopes, port, true)
}

fn oauth(
    client_id: Option<String>,
    client_secret: Option<String>,
    scopes: Option<String>,
    port: u16,
    fresh: bool,
) -> Result<(), String> {
    let mut creds = Credentials::from_config(&config::load()?.x);
    if let Some(id) = client_id {
        creds.client_id = Some(id);
    }
    if let Some(secret) = client_secret {
        creds.client_secret = Some(secret);
    }
    let client_id = creds
        .client_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .or_else(|| compiled_client_id().map(str::to_string))
        .ok_or_else(|| NO_APP.to_string())?;
    let scopes = scopes.unwrap_or_else(|| DEFAULT_SCOPES.to_string());
    let TerminalAuth {
        redirect,
        verifier,
        state,
        url,
    } = terminal_request(&client_id, creds.client_secret.as_deref(), &scopes, port)?;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
        format!("could not listen on 127.0.0.1:{port} ({e}). the callback URL registered on the app has to be this exact port.")
    })?;
    let steps = [
        ("Sign-in method", "Browser (OAuth 2.0 with PKCE)"),
        ("Waiting for the browser", "the address is on the right"),
    ];
    let aside = format!("{url}\n\nwaiting for the callback on {redirect}");
    if fresh {
        let _ =
            crate::screens::present_login(crate::net::Network::X.title(), &steps, &aside, None)?;
    } else {
        crate::screens::login_steps(&steps, &aside)?;
    }
    let _ = crate::util::open_browser(url.as_str());
    let (code, got_state) = accept_code(&listener)?;
    if got_state != state {
        return Err("oauth state did not match. try `socli login x` again.".into());
    }
    let token = exchange(
        &client_id,
        creds.client_secret.as_deref(),
        &redirect,
        &verifier,
        &code,
    )?;
    creds.access_token = Some(token.access_token.clone());
    creds.refresh_token = token.refresh_token.clone();
    creds.expires_at = token.expires_in.map(|secs| now_secs().saturating_add(secs));
    creds.client_id = Some(client_id);
    // The profile read has to be this token. Leftover cookies would answer for the old account.
    creds.auth_token = None;
    creds.ct0 = None;
    creds.username = None;
    creds.user_id = None;
    let client = super::XClient::new(creds.clone())
        .map_err(|err| format!("reading the profile failed: {err}. run `socli login x` again."))?;
    let user = client
        .me()
        .map_err(|err| format!("reading the profile failed: {err}. run `socli login x` again."))?;
    let username = user.username.clone();
    accept_oauth_session(&creds, Ok(user))?;
    crate::screens::print_signed_in(&username, None)?;
    Ok(())
}

/**
Writes an OAuth session only after the profile read has named the account. `Err` leaves the
file as it was. A successful write drops any browser cookies so later calls use this account.
*/
pub(crate) fn accept_oauth_session(
    creds: &Credentials,
    user: Result<crate::model::User, String>,
) -> Result<(), String> {
    let user = user?;
    config::update(|cfg| {
        cfg.x.client_id = creds.client_id.clone();
        cfg.x.client_secret = creds.client_secret.clone();
        cfg.x.access_token = creds.access_token.clone();
        cfg.x.refresh_token = creds.refresh_token.clone();
        cfg.x.expires_at = creds.expires_at;
        cfg.x.user_id = Some(user.id.clone());
        cfg.x.username = Some(user.username.clone());
        cfg.x.auth_token = None;
        cfg.x.ct0 = None;
    })?;
    Ok(())
}

struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

fn exchange(
    client_id: &str,
    client_secret: Option<&str>,
    redirect: &str,
    verifier: &str,
    code: &str,
) -> Result<TokenResponse, String> {
    let http = crate::net::http::client()?;
    let params = authorization_code_params(client_id, redirect, verifier, code);
    let mut req = http.post(token_endpoint());
    if let Some(secret) = client_secret {
        req = req.basic_auth(client_id, Some(secret));
    }
    let response = req
        .form(&params)
        .send()
        .map_err(|e| format!("token request: {}", e.without_url()))?;
    let value = read_token_response(response, "token exchange")?;
    let access_token = value
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| "X token response had no access_token".to_string())?
        .to_string();
    Ok(TokenResponse {
        access_token,
        refresh_token: value
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        expires_in: value.get("expires_in").and_then(|v| v.as_i64()),
    })
}

/// Trades the refresh token for a new access token and saves both.
pub fn refresh(creds: &mut Credentials) -> Result<(), String> {
    let previous = creds.clone();
    let refresh_token = creds
        .refresh_token
        .clone()
        .ok_or_else(|| "no refresh token stored".to_string())?;
    let client_id = creds
        .client_id
        .clone()
        .ok_or_else(|| "no client id stored; cannot refresh. run `socli login x`".to_string())?;
    let http = crate::net::http::client()?;
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.as_str()),
        ("client_id", client_id.as_str()),
    ];
    let mut req = http.post(token_endpoint());
    if let Some(secret) = creds.client_secret.as_deref() {
        req = req.basic_auth(&client_id, Some(secret));
    }
    let response = req
        .form(&params)
        .send()
        .map_err(|e| e.without_url().to_string())?;
    let value = read_token_response(response, "token refresh")?;
    creds.access_token = Some(
        value
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| "refresh response had no access_token".to_string())?
            .to_string(),
    );
    if let Some(next) = value
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|token| !token.trim().is_empty())
    {
        creds.refresh_token = Some(next.to_string());
    }
    creds.expires_at = value
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .map(|secs| now_secs().saturating_add(secs));
    config::update(|cfg| {
        store_refreshed_session(&mut cfg.x, &previous, creds);
    })?;
    Ok(())
}

fn read_token_response(
    response: crate::net::http::Response,
    operation: &str,
) -> Result<serde_json::Value, String> {
    let status = response.status();
    let text = crate::net::http::read_text(response)?;
    if !status.is_success() {
        return Err(token_failure(operation, status.as_u16(), &text));
    }
    serde_json::from_str(&text)
        .map_err(|_| "X returned an unreadable token response. run `socli login x` again".into())
}

fn token_failure(operation: &str, status: u16, text: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(text).ok();
    // OAuth error descriptions and unknown fields can echo submitted secrets.
    let code = value
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(|value| value.as_str())
        .filter(|code| {
            matches!(
                *code,
                "invalid_request"
                    | "invalid_client"
                    | "invalid_grant"
                    | "unauthorized_client"
                    | "unsupported_grant_type"
                    | "invalid_scope"
                    | "access_denied"
                    | "server_error"
                    | "temporarily_unavailable"
            )
        });
    let next = if status == 429 || status >= 500 {
        "try again later"
    } else {
        "run `socli login x` again"
    };
    match code {
        Some(code) => format!("{operation} failed ({status}, {code}). {next}"),
        None => format!("{operation} failed ({status}). {next}"),
    }
}

fn store_refreshed_session(section: &mut XConfig, previous: &Credentials, refreshed: &Credentials) {
    // A refresh must not replace a later login, or save credentials supplied only by the shell.
    if section.access_token == previous.access_token
        && section.refresh_token == previous.refresh_token
        && section.client_id == previous.client_id
    {
        section.access_token = refreshed.access_token.clone();
        section.refresh_token = refreshed.refresh_token.clone();
        section.expires_at = refreshed.expires_at;
    }
}

fn accept_code(listener: &TcpListener) -> Result<(String, String), String> {
    let mut stream = wait_for_callback(listener, std::time::Duration::from_secs(180))?;
    stream
        .set_nonblocking(false)
        .map_err(|error| format!("reading the browser callback: {error}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|error| format!("reading the browser callback: {error}"))?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|error| format!("answering the browser callback: {error}"))?;
    let mut buf = [0u8; 8192];
    let mut n = 0;
    let started = std::time::Instant::now();
    while !buf[..n].windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let remaining = std::time::Duration::from_secs(5).saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err("the browser callback timed out. run `socli login x` again".into());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| format!("reading the browser callback: {error}"))?;
        if n == buf.len() {
            return Err("the browser callback was too large. run `socli login x` again".into());
        }
        let read = stream.read(&mut buf[n..]).map_err(|_| {
            "could not read the browser callback. run `socli login x` again".to_string()
        })?;
        if read == 0 {
            return Err("the browser callback was cut off. run `socli login x` again".into());
        }
        n += read;
    }
    let req = String::from_utf8_lossy(&buf[..n]);
    let first = req.lines().next().unwrap_or("");
    let path = first.split_whitespace().nth(1).unwrap_or("");
    let fake = url::Url::parse(&format!("http://127.0.0.1{path}")).map_err(|e| e.to_string())?;
    let error = fake
        .query_pairs()
        .find(|(k, _)| k == "error")
        .map(|(_, v)| v.to_string());
    if let Some(error) = error {
        let body = page("X did not grant access. You can close this tab.");
        let _ = write_http(&mut stream, &body);
        return Err(if error == "access_denied" {
            "X sign-in was cancelled. run `socli login x` to try again".into()
        } else {
            "X did not grant access. run `socli login x` again".into()
        });
    }
    let code = fake
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| "callback had no code".to_string())?;
    let state = fake
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .unwrap_or_default();
    let _ = write_http(
        &mut stream,
        &page("The browser callback arrived. Return to the terminal to finish signing in."),
    );
    Ok((code, state))
}

fn wait_for_callback(
    listener: &TcpListener,
    timeout: std::time::Duration,
) -> Result<std::net::TcpStream, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("waiting for the browser: {error}"))?;
    let start = std::time::Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("waiting for the browser: {error}")),
        }
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err("X sign-in timed out. run `socli login x` again".into());
        }
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(100)));
    }
}

fn write_http(stream: &mut impl Write, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn page(message: &str) -> String {
    format!(
        "<!doctype html><meta charset=utf-8><title>SoCLI</title>\
<body style=\"background:#000;color:#eee;font-family:ui-monospace,monospace;padding:3rem\">\
<p>{message}</p></body>"
    )
}

fn now_secs() -> i64 {
    crate::clock::unix_secs() as i64
}

const EXPIRED: &str = "this sign-in has expired. start it again.";

/// Callback saved on SoCLI's X app. The site sends an `x.` state on to the loopback.
pub const PUBLIC_REDIRECT: &str = "https://socli.sh/";

fn terminal_redirect(client_id: &str, client_secret: Option<&str>, port: u16) -> String {
    let id = client_id.trim();
    let secret = client_secret
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if port == 8741 && secret.is_none() && compiled_client_id() == Some(id) {
        PUBLIC_REDIRECT.to_string()
    } else {
        format!("http://127.0.0.1:{port}/callback")
    }
}

fn terminal_state(redirect: &str) -> String {
    let nonce = code_verifier();
    if redirect == PUBLIC_REDIRECT {
        format!("x.{nonce}")
    } else {
        nonce
    }
}

struct TerminalAuth {
    redirect: String,
    verifier: String,
    state: String,
    url: String,
}

fn terminal_request(
    client_id: &str,
    client_secret: Option<&str>,
    scopes: &str,
    port: u16,
) -> Result<TerminalAuth, String> {
    let redirect = terminal_redirect(client_id, client_secret, port);
    let verifier = code_verifier();
    let state = terminal_state(&redirect);
    let url = authorize_url(
        client_id,
        &redirect,
        scopes,
        &state,
        &code_challenge(&verifier),
    )?;
    Ok(TerminalAuth {
        redirect,
        verifier,
        state,
        url,
    })
}

fn authorize_url(
    client_id: &str,
    redirect: &str,
    scopes: &str,
    state: &str,
    challenge: &str,
) -> Result<String, String> {
    let mut url =
        url::Url::parse("https://x.com/i/oauth2/authorize").map_err(|err| err.to_string())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("scope", scopes)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.into())
}

/// True when socli.sh would send this callback on to a terminal loopback.
fn terminal_callback_state(state: &str) -> bool {
    let Some(rest) = state
        .strip_prefix("x.")
        .or_else(|| state.strip_prefix("th."))
    else {
        return false;
    };
    (20..=80).contains(&rest.len())
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~'))
}

/// Drops a state socli.sh would 302 to the terminal. The page has to stay on this origin.
fn page_state(mut state: String) -> String {
    while terminal_callback_state(&state) {
        state = code_verifier();
    }
    state
}

const TOKEN_ENDPOINT: &str = "https://api.x.com/2/oauth2/token";

#[cfg(test)]
thread_local! {
    static TOKEN_ENDPOINT_OVERRIDE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

fn token_endpoint() -> String {
    #[cfg(test)]
    {
        if let Some(url) = TOKEN_ENDPOINT_OVERRIDE.with(|slot| slot.borrow().clone()) {
            return url;
        }
    }
    TOKEN_ENDPOINT.to_string()
}

fn authorization_code_params<'a>(
    client_id: &'a str,
    redirect: &'a str,
    verifier: &'a str,
    code: &'a str,
) -> [(&'a str, &'a str); 5] {
    [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect),
        ("code_verifier", verifier),
        ("client_id", client_id),
    ]
}

pub const NO_APP: &str = "this build has no SoCLI X app";

/// SoCLI's public OAuth client. The id is not a secret.
const DEFAULT_CLIENT_ID: &str = "RExTUlVaU1BXWHVla1VLOFR2WWs6MTpjaQ";

/// The public client id. `SOCLI_X_CLIENT_ID` replaces it at compile time.
/// An empty value is a build with no app, so the missing-id path stays testable.
pub fn compiled_client_id() -> Option<&'static str> {
    resolve_client_id(option_env!("SOCLI_X_CLIENT_ID"))
}

fn resolve_client_id(from_env: Option<&'static str>) -> Option<&'static str> {
    match from_env.map(str::trim) {
        Some("") => None,
        Some(id) => Some(id),
        None => Some(DEFAULT_CLIENT_ID),
    }
}

/// The page's approval screen. Ghostty still imports the browser session and does not use this.
pub fn page_copy() -> (Vec<(String, String)>, String) {
    let aside = if compiled_client_id().is_some() {
        "Enter opens X. Escape closes this.".to_string()
    } else {
        format!("{NO_APP}. Escape closes this.")
    };
    (
        vec![("Sign-in method".into(), "Approve SoCLI on X".into())],
        aside,
    )
}

fn public_client_id(raw: &str) -> Result<String, String> {
    let id = raw.trim();
    if id.is_empty() {
        return Err(
            "Client id is required. Create a public app at console.x.com and set the callback to this page."
                .into(),
        );
    }
    let ok = id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(id.to_string())
    } else {
        Err("that client id is not a public app id from console.x.com.".into())
    }
}

fn clear_page_oauth() -> Result<(), String> {
    config::update(|cfg| {
        cfg.x.oauth_state = None;
        cfg.x.oauth_verifier = None;
        cfg.x.oauth_redirect = None;
    })?;
    Ok(())
}

/// Starts a public PKCE sign-in and returns the X authorize URL. The verifier stays in the config.
pub fn begin_page(client_id: &str, redirect: &str) -> Result<String, String> {
    let client_id = public_client_id(client_id)?;
    let redirect = crate::util::page_callback(redirect)?;
    let verifier = code_verifier();
    let challenge = code_challenge(&verifier);
    let state = page_state(code_verifier());
    let url = authorize_url(&client_id, &redirect, DEFAULT_SCOPES, &state, &challenge)?;
    config::update(|cfg| {
        cfg.x.client_id = Some(client_id);
        cfg.x.oauth_state = Some(state);
        cfg.x.oauth_verifier = Some(verifier);
        cfg.x.oauth_redirect = Some(redirect);
    })?;
    Ok(url)
}

/// Trades the code from this page for a user token. A mismatched state never calls X.
pub fn finish_page(code: &str, got_state: &str) -> Result<(), String> {
    let saved = config::load()?.x;
    let pending = saved.oauth_state.clone().unwrap_or_default();
    let verifier = saved.oauth_verifier.clone().unwrap_or_default();
    let redirect = saved.oauth_redirect.clone().unwrap_or_default();
    let client_id = saved
        .client_id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_default();
    clear_page_oauth()?;
    let code = code.trim();
    let code_ok = !code.is_empty()
        && code.len() <= 512
        && code.chars().all(|c| !c.is_whitespace() && c != '&');
    if !code_ok
        || pending.is_empty()
        || pending != got_state
        || verifier.is_empty()
        || redirect.is_empty()
        || client_id.is_empty()
    {
        return Err(EXPIRED.into());
    }
    let token = exchange(&client_id, None, &redirect, &verifier, code)?;
    let mut creds = Credentials::from_section(&config::load()?.x);
    creds.client_id = Some(client_id);
    creds.client_secret = None;
    creds.access_token = Some(token.access_token);
    creds.refresh_token = token.refresh_token;
    creds.expires_at = token.expires_in.map(|secs| now_secs().saturating_add(secs));
    creds.auth_token = None;
    creds.ct0 = None;
    creds.username = None;
    creds.user_id = None;
    let client = super::XClient::new(creds.clone())
        .map_err(|err| format!("reading the profile failed: {err}. start sign-in again."))?;
    let user = client
        .me()
        .map_err(|err| format!("reading the profile failed: {err}. start sign-in again."))?;
    let username = user.username.clone();
    accept_oauth_session(&creds, Ok(user))?;
    crate::screens::print_signed_in(&username, None)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_abandoned_browser_sign_in_has_a_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let error = wait_for_callback(&listener, std::time::Duration::ZERO).unwrap_err();
        assert!(error.contains("timed out"));
        assert!(error.contains("socli login x"));
    }

    #[test]
    fn a_fragmented_browser_callback_is_read_completely() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let sender = std::thread::spawn(move || {
            let mut stream = std::net::TcpStream::connect(addr).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            stream.write_all(b"GET /callback?code=approved").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            stream
                .write_all(b"&state=nonce HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.contains("finish signing in"));
        });
        let (code, state) = accept_code(&listener).unwrap();
        assert_eq!(code, "approved");
        assert_eq!(state, "nonce");
        sender.join().unwrap();
    }

    #[test]
    fn token_rejection_diagnostics_never_echo_response_secrets() {
        for body in [
            r#"{"error":"invalid_grant","error_description":"secret-refresh-token","access_token":"secret-access-token"}"#,
            r#"{"error":"secret-refresh-token"}"#,
            "<html>secret-refresh-token</html>",
        ] {
            let error = token_failure("token refresh", 400, body);
            assert!(error.contains("400"));
            assert!(!error.contains("secret-"));
        }
        assert!(
            token_failure("token refresh", 400, r#"{"error":"invalid_grant"}"#)
                .contains("invalid_grant")
        );
        assert!(token_failure("token refresh", 503, "").contains("try again later"));
    }

    #[test]
    fn default_scopes_include_the_documented_like_and_follow_permissions() {
        let scopes: Vec<_> = DEFAULT_SCOPES.split_whitespace().collect();
        assert!(scopes.contains(&"like.write"));
        assert!(scopes.contains(&"follows.write"));
    }

    #[test]
    fn refresh_only_replaces_the_session_that_was_refreshed() {
        let previous = Credentials {
            access_token: Some("old-access".into()),
            refresh_token: Some("old-refresh".into()),
            client_id: Some("client".into()),
            ..Credentials::default()
        };
        let refreshed = Credentials {
            access_token: Some("new-access".into()),
            refresh_token: Some("new-refresh".into()),
            expires_at: Some(900),
            ..previous.clone()
        };
        let mut stored = XConfig {
            access_token: previous.access_token.clone(),
            refresh_token: previous.refresh_token.clone(),
            client_id: previous.client_id.clone(),
            ..XConfig::default()
        };
        store_refreshed_session(&mut stored, &previous, &refreshed);
        assert_eq!(stored.access_token, refreshed.access_token);
        assert_eq!(stored.refresh_token, refreshed.refresh_token);
        assert_eq!(stored.expires_at, Some(900));

        stored.access_token = Some("another-account".into());
        store_refreshed_session(&mut stored, &previous, &refreshed);
        assert_eq!(stored.access_token.as_deref(), Some("another-account"));

        let mut empty = XConfig::default();
        store_refreshed_session(&mut empty, &previous, &refreshed);
        assert!(empty.access_token.is_none());
        assert!(empty.refresh_token.is_none());
    }

    #[test]
    fn importing_a_token_replaces_the_previous_account_and_auth_method() {
        with_config(|| {
            config::update(|cfg| {
                cfg.x.auth_token = Some("old-cookie".into());
                cfg.x.ct0 = Some("old-csrf".into());
                cfg.x.user_id = Some("old-user".into());
                cfg.x.username = Some("old-name".into());
                cfg.x.refresh_token = Some("old-refresh".into());
                cfg.x.expires_at = Some(1);
            })
            .unwrap();
            save_token("imported-token".into(), None, None, None).unwrap();
            let saved = config::load().unwrap().x;
            assert_eq!(saved.access_token.as_deref(), Some("imported-token"));
            assert!(saved.auth_token.is_none());
            assert!(saved.ct0.is_none());
            assert!(saved.user_id.is_none());
            assert!(saved.username.is_none());
            assert!(saved.refresh_token.is_none());
            assert!(saved.expires_at.is_none());
        });
    }

    #[test]
    fn two_x_accounts_are_listed_before_a_different_one() {
        let labels = account_menu(&[
            AccountRow {
                browser: "Brave",
                profile: "Personal",
                dir: "Default",
            },
            AccountRow {
                browser: "Chrome",
                profile: "Aziendale",
                dir: "Profile 1",
            },
        ]);
        assert_eq!(
            labels,
            [
                "Brave · Personal",
                "Chrome · Aziendale",
                "A different account on x.com",
            ]
        );
    }

    #[test]
    fn identical_profile_names_keep_the_directory() {
        let labels = account_menu(&[
            AccountRow {
                browser: "Brave",
                profile: "Personal",
                dir: "Default",
            },
            AccountRow {
                browser: "Brave",
                profile: "Personal",
                dir: "Profile 2",
            },
        ]);
        assert_eq!(
            labels,
            [
                "Brave · Personal (Default)",
                "Brave · Personal (Profile 2)",
                "A different account on x.com",
            ]
        );
    }

    /// Holds the process config dir for one test. Unit tests share this process.
    fn with_config(body: impl FnOnce()) {
        let _guard = crate::config::lock_config_env();
        let dir = std::env::temp_dir().join(format!(
            "socli-x-login-{}-{}",
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

    /// `named` home pages say `bea` until a profile query, then `ada`.
    fn serve_x(named: bool) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = asked.clone();
        std::thread::spawn(move || {
            for _ in 0..16 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .ok();
                let mut buf = vec![0u8; 65536];
                let n = std::io::Read::read(&mut stream, &mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req
                    .lines()
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/");
                let body = if path.contains("UserByScreenName") {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    let name = if path.contains("bea") { "bea" } else { "ada" };
                    format!(
                        r#"{{"data":{{"user":{{"result":{{"__typename":"User","rest_id":"2","legacy":{{"screen_name":"{name}","name":"Bea"}}}}}}}}}}"#
                    )
                } else if !named {
                    "<html></html>".to_string()
                } else if flag.load(std::sync::atomic::Ordering::SeqCst) {
                    r#"{"screen_name":"ada"}"#.to_string()
                } else {
                    r#"{"screen_name":"bea"}"#.to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = std::io::Write::write_all(&mut stream, resp.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn env_overlays_a_bearer_without_a_file() {
        let mut creds = Credentials::from_section(&XConfig::default());
        overlay(&mut creds, |key| {
            (key == "X_BEARER_TOKEN").then(|| "abc123456789".to_string())
        });
        assert!(creds.has_bearer());
        assert!(!creds.has_user());
        assert_eq!(creds.latest_query(), DEFAULT_LATEST_QUERY);
    }

    #[test]
    fn env_wins_over_the_file() {
        let section = XConfig {
            access_token: Some("from-file".into()),
            username: Some("mina".into()),
            ..XConfig::default()
        };
        let mut creds = Credentials::from_section(&section);
        overlay(&mut creds, |key| {
            (key == "X_ACCESS_TOKEN").then(|| " from-env ".to_string())
        });
        assert_eq!(creds.access_token.as_deref(), Some("from-env"));
        assert!(creds.username.is_none());
    }

    fn stored_oauth() -> Credentials {
        Credentials {
            access_token: Some("stored-access".into()),
            refresh_token: Some("stored-refresh".into()),
            expires_at: Some(900),
            user_id: Some("stored-user".into()),
            username: Some("stored-name".into()),
            ..Credentials::default()
        }
    }

    #[test]
    fn a_changed_environment_access_token_cannot_refresh_as_the_stored_account() {
        let mut creds = stored_oauth();
        creds.auth_token = Some("stored-cookie".into());
        creds.ct0 = Some("stored-csrf".into());
        overlay(&mut creds, |key| {
            (key == "X_ACCESS_TOKEN").then(|| "environment-access".into())
        });
        assert!(!creds.uses_web());
        assert!(creds.refresh_token.is_none());
        assert!(creds.expires_at.is_none());
        assert!(creds.user_id.is_none());
        assert!(creds.username.is_none());
        let mut client = super::super::XClient::new(creds).unwrap();
        let calls = std::cell::Cell::new(0);
        let result: Result<(), crate::net::NetError> = client.call(|_| {
            calls.set(calls.get() + 1);
            Err(super::super::client::ApiError::Failed {
                status: 401,
                detail: "expired environment token".into(),
                hint: String::new(),
            })
        });
        assert!(matches!(
            result,
            Err(crate::net::NetError::Http { status: 401, .. })
        ));
        assert_eq!(calls.get(), 1);
        assert_eq!(
            client.credentials().access_token.as_deref(),
            Some("environment-access")
        );
    }

    #[test]
    fn an_environment_oauth_bundle_uses_only_its_explicit_account_metadata() {
        let mut creds = stored_oauth();
        overlay(&mut creds, |key| match key {
            "X_ACCESS_TOKEN" => Some("environment-access".into()),
            "X_REFRESH_TOKEN" => Some("environment-refresh".into()),
            "X_USER_ID" => Some("environment-user".into()),
            "X_USERNAME" => Some("environment-name".into()),
            _ => None,
        });
        assert_eq!(creds.refresh_token.as_deref(), Some("environment-refresh"));
        assert_eq!(creds.user_id.as_deref(), Some("environment-user"));
        assert_eq!(creds.username.as_deref(), Some("environment-name"));
        assert!(creds.expires_at.is_none());
    }

    #[test]
    fn the_same_environment_oauth_token_keeps_its_matching_stored_metadata() {
        let mut creds = stored_oauth();
        overlay(&mut creds, |key| {
            (key == "X_ACCESS_TOKEN").then(|| "stored-access".into())
        });
        assert_eq!(creds.refresh_token.as_deref(), Some("stored-refresh"));
        assert_eq!(creds.expires_at, Some(900));
        assert_eq!(creds.user_id.as_deref(), Some("stored-user"));
        assert_eq!(creds.username.as_deref(), Some("stored-name"));
    }

    #[test]
    fn an_environment_browser_bundle_takes_precedence_and_drops_stale_oauth_identity() {
        let mut creds = stored_oauth();
        creds.auth_token = Some("stored-cookie".into());
        creds.ct0 = Some("stored-csrf".into());
        overlay(&mut creds, |key| match key {
            "X_AUTH_TOKEN" => Some("environment-cookie".into()),
            "X_CT0" => Some("environment-csrf".into()),
            "X_ACCESS_TOKEN" => Some("environment-access".into()),
            "X_REFRESH_TOKEN" => Some("environment-refresh".into()),
            _ => None,
        });
        assert!(creds.uses_web());
        assert_eq!(creds.auth_token.as_deref(), Some("environment-cookie"));
        assert!(creds.access_token.is_none());
        assert!(creds.refresh_token.is_none());
        assert!(creds.expires_at.is_none());
        assert!(creds.user_id.is_none());
        assert!(creds.username.is_none());
    }

    #[test]
    fn an_incomplete_environment_browser_bundle_cannot_select_another_stored_session() {
        for key in ["X_AUTH_TOKEN", "X_CT0"] {
            let mut creds = stored_oauth();
            creds.auth_token = Some("stored-cookie".into());
            creds.ct0 = Some("stored-csrf".into());
            overlay(&mut creds, |name| {
                (name == key).then(|| "environment-cookie-part".into())
            });
            assert!(!creds.uses_web());
            assert!(!creds.has_user());
            assert!(creds.refresh_token.is_none());
            assert!(creds.user_id.is_none());
            assert!(creds.username.is_none());
        }
    }

    #[test]
    fn the_socli_x_client_is_compiled_and_an_empty_override_removes_it() {
        assert_eq!(
            compiled_client_id(),
            Some("RExTUlVaU1BXWHVla1VLOFR2WWs6MTpjaQ")
        );
        assert!(public_client_id(compiled_client_id().unwrap()).is_ok());
        assert_eq!(resolve_client_id(Some("")), None);
        assert_eq!(resolve_client_id(Some("  ")), None);
        assert_eq!(resolve_client_id(Some("app-id")), Some("app-id"));
        assert_eq!(resolve_client_id(None), compiled_client_id());
        let id = compiled_client_id().unwrap();
        assert_eq!(terminal_redirect(id, None, 8741), PUBLIC_REDIRECT);
        let state = terminal_state(PUBLIC_REDIRECT);
        let suffix = state.strip_prefix("x.").expect("public state");
        assert_eq!(suffix.len(), 64);
        assert_eq!(
            terminal_redirect(id, None, 8742),
            "http://127.0.0.1:8742/callback"
        );
        assert_eq!(
            terminal_redirect("other-app", None, 8741),
            "http://127.0.0.1:8741/callback"
        );
        assert_eq!(
            terminal_redirect(id, Some("secret"), 8741),
            "http://127.0.0.1:8741/callback"
        );
        let plain = terminal_state("http://127.0.0.1:8741/callback");
        assert_eq!(plain.len(), 64);
    }

    #[test]
    fn the_page_authorize_url_is_pkce_and_a_wrong_state_does_not_call_x() {
        with_config(|| {
            assert!(begin_page("  ", "http://127.0.0.1:5174/").is_err());
            assert!(begin_page("bad id", "http://127.0.0.1:5174/").is_err());
            assert!(begin_page("app-id", "http://localhost:5174/").is_err());
            assert!(config::load().unwrap().x.oauth_verifier.is_none());
            let url = begin_page("app-id", "http://127.0.0.1:5174/").unwrap();
            let parsed = url::Url::parse(&url).unwrap();
            assert_eq!(parsed.host_str(), Some("x.com"));
            assert_eq!(parsed.path(), "/i/oauth2/authorize");
            let pairs: Vec<(String, String)> = parsed.query_pairs().into_owned().collect();
            let value = |key: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, item)| item.clone())
            };
            assert_eq!(value("client_id").as_deref(), Some("app-id"));
            assert_eq!(value("code_challenge_method").as_deref(), Some("S256"));
            assert_eq!(
                value("redirect_uri").as_deref(),
                Some("http://127.0.0.1:5174/")
            );
            assert_eq!(value("scope").as_deref(), Some(DEFAULT_SCOPES));
            let saved = config::load().unwrap().x;
            let verifier = saved.oauth_verifier.unwrap();
            let state = value("state").unwrap();
            assert_eq!(code_challenge(&verifier), value("code_challenge").unwrap());
            assert_ne!(code_challenge(&state), value("code_challenge").unwrap());
            assert_eq!(saved.oauth_state.as_deref(), Some(state.as_str()));
            assert_ne!(state, verifier);
            assert!(!state.starts_with("x."));
            assert!(!state.starts_with("th."));
            assert!(!terminal_callback_state(&state));
            assert_eq!(
                saved.oauth_redirect.as_deref(),
                Some("http://127.0.0.1:5174/")
            );
            assert_eq!(saved.client_id.as_deref(), Some("app-id"));
            let (token_url, hits) = serve_token();
            let _endpoint = RestoreTokenEndpoint::set(token_url);
            let err = finish_page("code", &verifier).unwrap_err();
            assert_eq!(err, EXPIRED);
            assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
            let err = finish_page("code", "wrong").unwrap_err();
            assert_eq!(err, EXPIRED);
            assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
            let saved = config::load().unwrap().x;
            assert!(saved.oauth_verifier.is_none());
            assert!(saved.oauth_state.is_none());
            assert!(saved.oauth_redirect.is_none());
            assert_eq!(saved.client_id.as_deref(), Some("app-id"));
        });
    }

    #[test]
    fn a_browser_login_stores_the_account_the_home_page_names() {
        with_config(|| {
            config::update(|cfg| {
                cfg.x.username = Some("ada".into());
                cfg.x.user_id = Some("1".into());
                cfg.x.auth_token = Some("old-token".into());
                cfg.x.ct0 = Some("old-ct0".into());
            })
            .unwrap();
            let origin = serve_x(true);
            let client = super::super::XClient::for_test(
                Credentials {
                    username: Some("ada".into()),
                    auth_token: Some("new-token".into()),
                    ct0: Some("new-ct0".into()),
                    ..Credentials::default()
                },
                origin,
            )
            .unwrap();
            let user =
                accept_browser_session("new-token".into(), "new-ct0".into(), &client).unwrap();
            assert_eq!(user.username, "bea");
            let saved = config::load().unwrap();
            assert_eq!(saved.x.username.as_deref(), Some("bea"));
            assert_eq!(saved.x.user_id.as_deref(), Some("2"));
            assert_eq!(saved.x.auth_token.as_deref(), Some("new-token"));
            assert_eq!(saved.x.ct0.as_deref(), Some("new-ct0"));
            assert_eq!(client.me().unwrap().username, "bea");
        });
    }

    #[test]
    fn a_home_page_without_an_account_leaves_the_saved_session() {
        with_config(|| {
            config::update(|cfg| {
                cfg.x.username = Some("ada".into());
                cfg.x.auth_token = Some("old-token".into());
                cfg.x.ct0 = Some("old-ct0".into());
            })
            .unwrap();
            let before = std::fs::read(config::path()).unwrap();
            let client = super::super::XClient::for_test(
                Credentials {
                    username: Some("ada".into()),
                    auth_token: Some("new-token".into()),
                    ct0: Some("new-ct0".into()),
                    ..Credentials::default()
                },
                serve_x(false),
            )
            .unwrap();
            let err =
                accept_browser_session("new-token".into(), "new-ct0".into(), &client).unwrap_err();
            assert!(err.contains("reading the profile failed"), "{err}");
            assert_eq!(std::fs::read(config::path()).unwrap(), before);
        });
    }

    #[test]
    fn a_failed_oauth_profile_leaves_the_previous_file() {
        with_config(|| {
            config::update(|cfg| {
                cfg.x.username = Some("ada".into());
                cfg.x.auth_token = Some("old-token".into());
                cfg.x.ct0 = Some("old-ct0".into());
            })
            .unwrap();
            let before = std::fs::read(config::path()).unwrap();
            let creds = Credentials {
                client_id: Some("cid".into()),
                access_token: Some("new-access".into()),
                refresh_token: Some("new-refresh".into()),
                ..Credentials::default()
            };
            let err = accept_oauth_session(&creds, Err("nope".into())).unwrap_err();
            assert_eq!(err, "nope");
            assert_eq!(std::fs::read(config::path()).unwrap(), before);
            accept_oauth_session(
                &creds,
                Ok(crate::model::User {
                    id: "9".into(),
                    name: "Bea".into(),
                    username: "bea".into(),
                    avatar_url: None,
                    verified: false,
                    business: false,
                    description: None,
                }),
            )
            .unwrap();
            let saved = config::load().unwrap();
            assert_eq!(saved.x.username.as_deref(), Some("bea"));
            assert_eq!(saved.x.user_id.as_deref(), Some("9"));
            assert_eq!(saved.x.access_token.as_deref(), Some("new-access"));
            assert_eq!(saved.x.refresh_token.as_deref(), Some("new-refresh"));
            assert_eq!(saved.x.client_id.as_deref(), Some("cid"));
            assert!(saved.x.auth_token.is_none());
            assert!(saved.x.ct0.is_none());
        });
    }

    #[test]
    fn a_browser_session_counts_as_signed_in() {
        let mut creds = Credentials::default();
        overlay(&mut creds, |key| match key {
            "X_AUTH_TOKEN" => Some("token-value-1234".into()),
            "X_CT0" => Some("csrf-value-5678".into()),
            _ => None,
        });
        assert!(creds.uses_web());
        assert!(creds.has_user());
        assert!(creds.has_any_read());
    }

    fn pairs_of(url: &str) -> Vec<(String, String)> {
        url::Url::parse(url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }

    fn query_value<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
        pairs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn the_compiled_terminal_url_uses_socli_sh_and_a_second_verifier() {
        let id = compiled_client_id().unwrap();
        let pending = terminal_request(id, None, DEFAULT_SCOPES, 8741).unwrap();
        assert_eq!(pending.redirect, PUBLIC_REDIRECT);
        assert_eq!(pending.redirect, "https://socli.sh/");
        let pairs = pairs_of(&pending.url);
        let parsed = url::Url::parse(&pending.url).unwrap();
        assert_eq!(parsed.host_str(), Some("x.com"));
        assert_eq!(parsed.path(), "/i/oauth2/authorize");
        assert_eq!(query_value(&pairs, "response_type"), Some("code"));
        assert_eq!(query_value(&pairs, "client_id"), Some(id));
        assert_eq!(query_value(&pairs, "redirect_uri"), Some(PUBLIC_REDIRECT));
        assert_ne!(
            query_value(&pairs, "redirect_uri"),
            Some("http://127.0.0.1:8741/callback")
        );
        assert_eq!(query_value(&pairs, "scope"), Some(DEFAULT_SCOPES));
        assert_eq!(query_value(&pairs, "code_challenge_method"), Some("S256"));
        assert_eq!(query_value(&pairs, "state"), Some(pending.state.as_str()));
        let suffix = pending.state.strip_prefix("x.").expect("public state");
        assert_eq!(suffix.len(), 64);
        assert_ne!(suffix, pending.verifier);
        assert_ne!(pending.state, pending.verifier);
        assert!(terminal_callback_state(&pending.state));
        let challenge = code_challenge(&pending.verifier);
        let state_challenge = code_challenge(&pending.state);
        let suffix_challenge = code_challenge(suffix);
        assert_eq!(
            query_value(&pairs, "code_challenge"),
            Some(challenge.as_str())
        );
        assert_ne!(
            query_value(&pairs, "code_challenge"),
            Some(state_challenge.as_str())
        );
        assert_ne!(
            query_value(&pairs, "code_challenge"),
            Some(suffix_challenge.as_str())
        );
        let form = authorization_code_params(id, &pending.redirect, &pending.verifier, "auth-code");
        assert_eq!(form[2], ("redirect_uri", PUBLIC_REDIRECT));
        assert_eq!(form[3], ("code_verifier", pending.verifier.as_str()));
        assert_ne!(form[3].1, suffix);
        assert!(!form[2].1.contains("127.0.0.1"));
    }

    #[test]
    fn a_personal_client_secret_or_port_keeps_the_loopback() {
        let id = compiled_client_id().unwrap();
        let cases: [(&str, Option<&str>, u16, &str); 3] = [
            ("other-app", None, 8741, "http://127.0.0.1:8741/callback"),
            (id, Some("secret"), 8741, "http://127.0.0.1:8741/callback"),
            (id, None, 8742, "http://127.0.0.1:8742/callback"),
        ];
        for (client_id, secret, port, redirect) in cases {
            let pending = terminal_request(client_id, secret, DEFAULT_SCOPES, port).unwrap();
            assert_eq!(pending.redirect, redirect);
            assert_eq!(pending.state.len(), 64);
            assert_ne!(pending.state, pending.verifier);
            let pairs = pairs_of(&pending.url);
            assert_eq!(query_value(&pairs, "redirect_uri"), Some(redirect));
            assert_eq!(query_value(&pairs, "state"), Some(pending.state.as_str()));
            let challenge = code_challenge(&pending.verifier);
            assert_eq!(
                query_value(&pairs, "code_challenge"),
                Some(challenge.as_str())
            );
        }
        let blank = terminal_request(id, Some("  "), DEFAULT_SCOPES, 8741).unwrap();
        assert_eq!(blank.redirect, PUBLIC_REDIRECT);
        assert!(blank.state.starts_with("x."));
        assert_ne!(
            blank.state.strip_prefix("x.").unwrap(),
            blank.verifier.as_str()
        );
    }

    #[test]
    fn a_page_state_the_worker_would_steal_is_replaced() {
        let stolen = format!("x.{}", "a".repeat(64));
        assert!(terminal_callback_state(&stolen));
        let state = page_state(stolen.clone());
        assert_ne!(state, stolen);
        assert_eq!(state.len(), 64);
        assert!(!state.starts_with("x."));
        assert!(!state.starts_with("th."));
        assert!(!terminal_callback_state(&state));
        let threads = format!("th.{}", "b".repeat(20));
        assert!(terminal_callback_state(&threads));
        assert!(!terminal_callback_state(&page_state(threads)));
        let plain = "n".repeat(64);
        assert!(!terminal_callback_state(&plain));
        assert_eq!(page_state(plain.clone()), plain);
        assert!(!terminal_callback_state(&format!("x.{}", "a".repeat(19))));
    }

    #[test]
    fn the_socli_sh_page_state_is_not_prefixed_and_a_mismatch_stays_local() {
        with_config(|| {
            let (token_url, hits) = serve_token();
            let _endpoint = RestoreTokenEndpoint::set(token_url);
            let id = compiled_client_id().unwrap();
            let url = begin_page(id, PUBLIC_REDIRECT).unwrap();
            let pairs = pairs_of(&url);
            let state = query_value(&pairs, "state").unwrap().to_string();
            assert_eq!(query_value(&pairs, "redirect_uri"), Some(PUBLIC_REDIRECT));
            assert_eq!(query_value(&pairs, "client_id"), Some(id));
            assert_eq!(query_value(&pairs, "scope"), Some(DEFAULT_SCOPES));
            assert_eq!(query_value(&pairs, "code_challenge_method"), Some("S256"));
            assert!(!state.starts_with("x."));
            assert!(!state.starts_with("th."));
            assert!(!terminal_callback_state(&state));
            let saved = config::load().unwrap().x;
            let verifier = saved.oauth_verifier.unwrap();
            assert_eq!(saved.oauth_state.as_deref(), Some(state.as_str()));
            assert_eq!(saved.oauth_redirect.as_deref(), Some(PUBLIC_REDIRECT));
            assert_eq!(saved.client_id.as_deref(), Some(id));
            assert_ne!(state, verifier);
            let challenge = code_challenge(&verifier);
            assert_eq!(
                query_value(&pairs, "code_challenge"),
                Some(challenge.as_str())
            );
            let prefixed = format!("x.{state}");
            let err = finish_page("code", &prefixed).unwrap_err();
            assert_eq!(err, EXPIRED);
            assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
            assert!(config::load().unwrap().x.access_token.is_none());
        });
    }

    fn serve_token() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let flag = hits.clone();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            flag.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .ok();
            let mut buf = [0u8; 4096];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let body = "no";
            let resp = format!(
                "HTTP/1.1 500 ERR\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = std::io::Write::write_all(&mut stream, resp.as_bytes());
        });
        (format!("http://{addr}/2/oauth2/token"), hits)
    }

    struct RestoreTokenEndpoint;

    impl RestoreTokenEndpoint {
        fn set(url: String) -> Self {
            TOKEN_ENDPOINT_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(url));
            Self
        }
    }

    impl Drop for RestoreTokenEndpoint {
        fn drop(&mut self) {
            TOKEN_ENDPOINT_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
        }
    }
}
