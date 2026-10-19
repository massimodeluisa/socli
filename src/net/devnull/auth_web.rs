//! Page sign-in for dev/null. The provider only redirects to `devnull://`, so the page opens the
//! approval and the person pastes that callback, the same path as a terminal that cannot catch it.

use std::path::PathBuf;

use super::flow::{self, authorize_url, parse_callback};
use super::{api, sign};
use crate::config::{self, DevnullConfig};
use crate::net::http::Method;
use crate::net::{http, pkce};

pub struct Prepared {
    pub url: String,
    pub fingerprint: String,
    pub state: String,
    pub verifier: String,
}

/// Creates or loads the page's device key and returns the approval address.
pub fn prepare() -> Result<Prepared, String> {
    let saved = config::load()?.devnull;
    let (key, created) = match saved.device_secret.as_deref().filter(|pem| !pem.is_empty()) {
        Some(pem) => (sign::from_openssh(pem)?, false),
        None => (sign::generate()?, true),
    };
    let pem = sign::to_openssh(&key)?;
    let fingerprint = sign::fingerprint(&key);
    config::update(|cfg| {
        cfg.devnull.device_secret = Some(pem);
        cfg.devnull.device_key = Some(sign::KEY_FILE.to_string());
        if created {
            cfg.devnull.device_approved = false;
            cfg.devnull.username = None;
        }
    })?;
    let verifier = pkce::code_verifier();
    let challenge = pkce::code_challenge(&verifier);
    let state = pkce::code_verifier();
    let url = authorize_url(
        &fingerprint,
        "socli on this page",
        &challenge,
        &state,
        flow::APP_CALLBACK,
    )?;
    Ok(Prepared {
        url: url.to_string(),
        fingerprint,
        state,
        verifier,
    })
}

/// Trades a pasted `devnull://` callback for an approved device.
pub fn finish(callback: &str, state: &str, verifier: &str) -> Result<(), String> {
    let code = parse_callback(callback, state)?;
    let saved = config::load()?.devnull;
    let pem = saved
        .device_secret
        .filter(|pem| !pem.is_empty())
        .ok_or_else(|| "the device key is missing. open dev/null sign-in again".to_string())?;
    let key = sign::from_openssh(&pem)?;
    let client = http::client()?;
    let body = flow::approval_body(&code, &verifier);
    let response = api::request(
        &client,
        Some(&key),
        Method::POST,
        api::url(&["api", "v1", "devices"], &[])?,
        Some(&body),
    )
    .map_err(|e| {
        format!("dev/null did not approve this device: {e}. open dev/null sign-in again")
    })?;
    let text = api::text(response)?;
    let username = match flow::username_from_body(&text) {
        Some(username) => username,
        None => {
            let me = api::request(
                &client,
                Some(&key),
                Method::GET,
                api::url(&["api", "v1", "me"], &[])?,
                None,
            )?;
            let me_text = api::text(me)?;
            flow::username_from_body(&me_text).ok_or_else(|| {
                "dev/null approved the device without a username. open sign-in again".to_string()
            })?
        }
    };
    config::update(|cfg| {
        cfg.devnull.device_key = Some(sign::KEY_FILE.to_string());
        cfg.devnull.device_approved = true;
        cfg.devnull.username = Some(username);
    })?;
    Ok(())
}

pub fn login(_device_name: Option<String>) -> Result<(), String> {
    Err("open dev/null on the page and paste the devnull:// callback.".into())
}

pub fn logout() -> Result<Option<String>, String> {
    config::update(|cfg| cfg.devnull = DevnullConfig::default())?;
    Ok(None)
}

pub fn setup_ssh(_key: Option<PathBuf>) -> Result<(), String> {
    Err("GitHub SSH signing stays in the terminal. run `socli auth devnull ssh`.".into())
}
