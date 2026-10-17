//! `socli login bluesky` and `socli logout bluesky`: app-password sessions.

use serde_json::{json, Value};

use super::rpc::{self, Call};
use super::{Session, DEFAULT_SERVICE};
use crate::config::{self, BlueskyConfig};
use crate::net::{http, NetError, Network};
use crate::{screens, util};

const APP_PASSWORDS: &str = "https://bsky.app/settings/app-passwords";
const NO_PROMPT: &str =
    "Pass the username and read the app password from stdin: socli login bluesky --handle <username> --password-stdin";

fn password_missing() -> String {
    format!(
        "bluesky needs an app password. Make one at {APP_PASSWORDS}, then run `socli login bluesky`"
    )
}

fn env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The PDS that holds the account: the `#atproto_pds` entry of the DID document.
pub fn pds_endpoint(did_doc: &Value) -> Option<String> {
    did_doc
        .get("service")?
        .as_array()?
        .iter()
        .find(|s| {
            s.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.ends_with("#atproto_pds"))
        })
        .and_then(|s| s.get("serviceEndpoint")?.as_str())
        .map(|url| url.trim_end_matches('/').to_string())
}

/// Reads a `createSession` response; `signed_in_at` is the fallback when the DID document has no PDS.
pub fn session_from(body: &Value, signed_in_at: &str) -> Option<Session> {
    let text = |key: &str| body.get(key)?.as_str().map(str::to_string);
    Some(Session {
        handle: text("handle")?,
        did: text("did")?,
        access_jwt: text("accessJwt")?,
        refresh_jwt: text("refreshJwt"),
        service: body
            .get("didDoc")
            .and_then(pds_endpoint)
            .unwrap_or_else(|| signed_in_at.to_string()),
    })
}

fn create_session(service: &str, handle: &str, password: &str) -> Result<Session, String> {
    let client = http::client()?;
    let url = format!("{service}/xrpc/com.atproto.server.createSession");
    let body = json!({"identifier": handle, "password": password});
    let raw = rpc::send(&client, &url, &Call::Post(Some(&body)), None)?;
    let value = rpc::finish(raw).map_err(|err| match err {
        NetError::Http { status: 401, .. } => {
            format!("Bluesky rejected the username or app password. Make one at {APP_PASSWORDS}")
        }
        other => other.to_string(),
    })?;
    session_from(&value, service)
        .ok_or_else(|| "bluesky answered without a session. run `socli login bluesky` again".into())
}

/// Flag value, then `BLUESKY_SERVICE`, then the default host, without a trailing slash.
fn resolve_service(flag: Option<String>) -> String {
    pick_service(flag, env("BLUESKY_SERVICE"))
}

fn pick_service(flag: Option<String>, from_env: Option<String>) -> String {
    flag.map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or(from_env)
        .unwrap_or_else(|| DEFAULT_SERVICE.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn clean_handle(handle: &str) -> Result<String, String> {
    let handle = handle.trim().trim_start_matches('@').to_string();
    if handle.is_empty() {
        return Err(
            "bluesky needs a username. run `socli login bluesky --handle you.bsky.social`".into(),
        );
    }
    Ok(handle)
}

/// `socli login bluesky`: username from `--handle`, `BLUESKY_HANDLE`, or a prompt; app password from `BLUESKY_APP_PASSWORD` or a hidden prompt.
pub fn login(handle: Option<String>, service: Option<String>) -> Result<(), String> {
    let given = handle
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .or_else(|| env("BLUESKY_HANDLE"));
    let password = env("BLUESKY_APP_PASSWORD");
    if let (Some(handle), Some(password)) = (given.clone(), password.clone()) {
        return login_with_password(handle, service, password);
    }
    if !util::person_present() {
        return Err(format!("{} {NO_PROMPT}", util::NEEDS_PERSON));
    }
    let asked_handle = given.is_none();
    let known = given.clone().unwrap_or_default();
    let method = "App password (bsky.app/settings/app-passwords)";
    let steps: Vec<(&str, &str)> = if asked_handle {
        vec![("Sign-in method", method), ("App password", "••••")]
    } else {
        vec![
            ("Sign-in method", method),
            ("Username", known.as_str()),
            ("App password", "••••"),
        ]
    };
    let prompt = if asked_handle {
        "Username:"
    } else {
        "App password:"
    };
    let column = screens::present_login(Network::Bluesky.title(), &steps, "", Some(prompt))?;
    let handle = match given {
        Some(handle) => handle,
        None => util::prompt_line("", NO_PROMPT)?,
    };
    let password = match password {
        Some(password) => password,
        None if asked_handle => {
            util::prompt_secret(&format!("{}App password: ", " ".repeat(column)), NO_PROMPT)?
        }
        None => util::prompt_secret("", NO_PROMPT)?,
    };
    open_session(handle, service, password)
}

/// Signs in without prompting: the same session, saved config and output as `login`.
pub fn login_with_password(
    handle: String,
    service: Option<String>,
    password: String,
) -> Result<(), String> {
    let handle = clean_handle(&handle)?;
    if password.is_empty() {
        return Err(password_missing());
    }
    let _ = screens::present_login(
        Network::Bluesky.title(),
        &[
            (
                "Sign-in method",
                "App password (bsky.app/settings/app-passwords)",
            ),
            ("Username", &handle),
            ("App password", "••••"),
        ],
        "",
        None,
    );
    open_session(handle, service, password)
}

fn open_session(handle: String, service: Option<String>, password: String) -> Result<(), String> {
    let service = resolve_service(service);
    let handle = clean_handle(&handle)?;
    if password.is_empty() {
        return Err(password_missing());
    }
    let session = create_session(&service, &handle, &password)?;
    let saved = session.clone();
    config::update(|cfg| {
        cfg.bluesky = BlueskyConfig {
            service: Some(saved.service),
            handle: Some(saved.handle),
            did: Some(saved.did),
            access_jwt: Some(saved.access_jwt),
            refresh_jwt: saved.refresh_jwt,
        };
    })?;
    screens::print_signed_in(&session.handle, None)
}

/// Revokes the session on the server when it can, then clears `[bluesky]`.
pub fn logout() -> Result<(), String> {
    let section = config::load()?.bluesky;
    if let Some(refresh) = section.refresh_jwt.as_deref().filter(|t| !t.is_empty()) {
        let service = section.service.as_deref().unwrap_or(DEFAULT_SERVICE);
        let url = format!(
            "{}/xrpc/com.atproto.server.deleteSession",
            service.trim_end_matches('/')
        );
        if let Ok(client) = http::client() {
            let _ = rpc::send(&client, &url, &Call::Post(None), Some(refresh));
        }
    }
    config::update(|cfg| cfg.bluesky = BlueskyConfig::default())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pds_comes_from_the_did_document() {
        let doc = json!({
            "id": "did:plc:abc",
            "service": [
                {"id": "#other", "type": "Other", "serviceEndpoint": "https://other.example"},
                {"id": "#atproto_pds", "type": "AtprotoPersonalDataServer",
                 "serviceEndpoint": "https://morel.us-east.host.bsky.network/"},
            ],
        });
        assert_eq!(
            pds_endpoint(&doc).as_deref(),
            Some("https://morel.us-east.host.bsky.network")
        );
        assert_eq!(pds_endpoint(&json!({"service": []})), None);
        assert_eq!(pds_endpoint(&json!({})), None);
    }

    #[test]
    fn the_service_and_handle_are_normalised_before_sign_in() {
        assert_eq!(
            pick_service(
                Some(" https://pds.test/ ".into()),
                Some("https://env.test".into())
            ),
            "https://pds.test"
        );
        assert_eq!(
            pick_service(Some(" ".into()), Some("https://env.test/".into())),
            "https://env.test"
        );
        assert_eq!(
            pick_service(None, None),
            DEFAULT_SERVICE.trim_end_matches('/')
        );
        assert_eq!(clean_handle(" @ada.test ").as_deref(), Ok("ada.test"));
        assert_eq!(
            clean_handle(" @ ").err().as_deref(),
            Some("bluesky needs a username. run `socli login bluesky --handle you.bsky.social`")
        );
    }

    #[test]
    fn an_empty_password_is_refused_before_any_request() {
        let err = match login_with_password(" ada.test ".into(), None, String::new()) {
            Ok(()) => panic!("an empty app password must not sign in"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            "bluesky needs an app password. Make one at https://bsky.app/settings/app-passwords, then run `socli login bluesky`"
        );
    }

    #[test]
    fn a_session_falls_back_to_the_sign_in_service() {
        let body = json!({
            "handle": "ada.test", "did": "did:plc:abc",
            "accessJwt": "access", "refreshJwt": "refresh",
            "didDoc": {"service": [{"id": "did:plc:abc#atproto_pds", "serviceEndpoint": "https://pds.test"}]},
        });
        let session = session_from(&body, "https://bsky.social").expect("session");
        assert_eq!(session.service, "https://pds.test");
        assert_eq!(session.refresh_jwt.as_deref(), Some("refresh"));
        let bare = json!({"handle": "ada.test", "did": "did:plc:abc", "accessJwt": "a"});
        let session = session_from(&bare, "https://bsky.social").expect("session");
        assert_eq!(session.service, "https://bsky.social");
        assert_eq!(session.refresh_jwt, None);
        assert!(session_from(&json!({"handle": "x"}), "https://bsky.social").is_none());
    }
}
