//! `socli login devnull`, `socli auth devnull ssh` and `socli logout devnull`.

use std::path::{Path, PathBuf};

use reqwest::Method;

use super::flow::{self, authorize_url, parse_callback};
use super::{api, browser, sign, ssh};
use crate::config::{self, DevnullConfig};
use crate::net::{http, pkce, Network};
use crate::{screens, util};

const DEVICE_NAME_MAX: usize = 100;

/// `socli login devnull`: dev/null only redirects to its app's `devnull://` callback, which the user pastes.
pub fn login(device_name: Option<String>) -> Result<(), String> {
    if !util::person_present() {
        return Err(format!(
            "{} Run socli login devnull in a terminal.",
            util::NEEDS_PERSON
        ));
    }
    let path = config::dir().join(sign::KEY_FILE);
    let (key, created) = sign::load_or_create(&path)?;
    config::update(|cfg| {
        cfg.devnull.device_key = Some(sign::KEY_FILE.to_string());
        if created {
            cfg.devnull.device_approved = false;
        }
    })?;
    let fingerprint = sign::fingerprint(&key);
    let name: String = device_name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(default_device_name)
        .chars()
        .take(DEVICE_NAME_MAX)
        .collect();
    let verifier = pkce::code_verifier();
    let challenge = pkce::code_challenge(&verifier);
    let state = pkce::code_verifier();
    let url = authorize_url(&fingerprint, &name, &challenge, &state, flow::APP_CALLBACK)?;

    let key_detail = format!(
        "{fingerprint} ({} ed25519 key in {})",
        if created { "new" } else { "existing" },
        path.display()
    );
    let callback = if browser::native() {
        screens::present_login(
            Network::Devnull.title(),
            &[("Device key", &key_detail), ("Approve in the browser", "SoCLI will receive the result automatically")],
            "A macOS browser sign-in window will open. Approve SoCLI there, or cancel to return to the terminal.",
            None,
        )?;
        let callback = browser::authorize(&url)?;
        let returned =
            url::Url::parse(&callback).map_err(|_| "the browser returned an invalid callback")?;
        if !returned
            .query_pairs()
            .any(|(name, value)| name == "state" && value == state)
        {
            return Err(
                "the callback belongs to another sign-in. Retry to open a fresh sign-in window"
                    .into(),
            );
        }
        callback
    } else {
        screens::present_login(
            Network::Devnull.title(),
            &[("Device key", &key_detail), ("Approve in the browser", "open the complete link below"), ("Paste the callback", "devnull://auth/callback?code=…")],
            &format!("{url}\n\nApprove SoCLI in the browser. If the browser cannot open devnull://, copy the full callback URL from its address bar or the approval request's Location response header. On macOS, run without SOCLI_AUTH_BROWSER=manual to receive the callback automatically."),
            Some("Callback:"),
        )?;
        let _ = util::open_browser(url.as_str());
        util::prompt_line(
            "",
            "Open the sign-in in a browser yourself: socli login devnull",
        )?
    };
    let code = parse_callback(&callback, &state)?;

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
        format!("dev/null did not approve this device: {e}. run `socli login devnull` again")
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
            flow::username_from_body(&me_text).unwrap_or_default()
        }
    };
    config::update(|cfg| {
        cfg.devnull.device_key = Some(sign::KEY_FILE.to_string());
        cfg.devnull.device_approved = true;
        cfg.devnull.username = Some(username.clone());
    })?;
    screens::print_signed_in(&username, None)
}

fn default_device_name() -> String {
    let command = std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned());
    device_name(command, |key| std::env::var(key).ok())
}

/** `SoCLI on <host>`: the `hostname` output, else `HOSTNAME`, else `COMPUTERNAME`, each cut at the first `.` (macOS answers `name.local`); `SoCLI` when none is set. */
fn device_name(command: Option<String>, env: impl Fn(&str) -> Option<String>) -> String {
    command
        .into_iter()
        .chain(env("HOSTNAME"))
        .chain(env("COMPUTERNAME"))
        .map(|name| {
            name.trim()
                .split('.')
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .find(|host| !host.is_empty())
        .map_or_else(|| "SoCLI".into(), |host| format!("SoCLI on {host}"))
}

/// The public key `--key` names: the `.pub` file itself, or the `.pub` next to a private key.
fn public_key_file(path: &Path) -> Result<PathBuf, String> {
    if path.extension().is_some_and(|ext| ext == "pub") {
        return Ok(path.to_path_buf());
    }
    let mut public = path.as_os_str().to_owned();
    public.push(".pub");
    let public = PathBuf::from(public);
    if public.is_file() {
        Ok(public)
    } else {
        Err(format!(
            "no public key next to {}. Pass the .pub file",
            path.display()
        ))
    }
}

pub fn setup_ssh(key: Option<PathBuf>) -> Result<(), String> {
    let public = key.as_deref().map(public_key_file).transpose()?;
    let lines: Vec<String> = match &public {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("could not read {}: {e}", path.display()))?;
            text.lines()
                .next()
                .map(str::to_string)
                .into_iter()
                .collect()
        }
        None => {
            let agent = ssh::agent_keys();
            let files = ssh::home().map(|h| ssh::key_files(&h)).unwrap_or_default();
            agent
                .lines()
                .map(str::to_string)
                .chain(files.into_iter().map(|(_, line)| line))
                .collect()
        }
    };
    let candidates = ssh::candidates(lines.iter().map(String::as_str));
    if candidates.is_empty() {
        return Err(match public {
            Some(path) => format!(
                "{} is not an ed25519 or rsa public key. pass the .pub file to `socli auth devnull ssh --key`",
                path.display()
            ),
            None => "no ed25519 or rsa ssh key found in your ssh agent or in ~/.ssh. add one to GitHub, then run `socli auth devnull ssh`".into(),
        });
    }
    let client = http::client()?;
    let answer = api::post_text(
        &client,
        &["api", "cli", "key-lookups"],
        ssh::lookup_body(&candidates).into_bytes(),
        None,
    )
    .map_err(|e| format!("{e} then run `socli auth devnull ssh` again"))?;
    let picked = ssh::select(&answer, &candidates).ok_or(
        "dev/null answered with a key that is not yours. run `socli auth devnull ssh` again",
    )?;
    let line = picked.line.clone();
    config::update(|cfg| cfg.devnull.ssh_key = Some(line.clone()))?;
    let mut aside = "SoCLI posts and replies on dev/null with this key.".to_string();
    if let Some(public) = public {
        let id = picked.id.as_str();
        let in_agent = ssh::agent_keys()
            .lines()
            .filter_map(ssh::key_id)
            .any(|k| k == id);
        let in_ssh_dir = ssh::home()
            .map(|h| ssh::key_files(&h))
            .unwrap_or_default()
            .iter()
            .any(|(_, line)| ssh::key_id(line).as_deref() == Some(id));
        if !in_agent && !in_ssh_dir {
            aside.push_str(&format!(
                "\n\nthis key is not in ~/.ssh or the ssh agent. Run `ssh-add {}` so socli can post with it",
                public.with_extension("").display()
            ));
        }
    }
    let _ = screens::present_login(
        Network::Devnull.title(),
        &[
            (
                "Sign-in method",
                "GitHub SSH key (commits signed like `devnull commit`)",
            ),
            ("SSH key", &line),
        ],
        &aside,
        None,
    )?;
    Ok(())
}

/// The line `socli logout` prints when the device key file is still on disk.
fn retained_key_line(path: &Path) -> String {
    format!(
        "The device key stays at {}. Revoke the device at https://dev-null.sh/settings/devices",
        path.display()
    )
}

/**
Forgets the dev/null account and leaves the device key file where it is.

The returned note is for the caller to show. Printing it here lands on the reader's cursor row,
and the next frame does not paint those cells back.
*/
pub fn logout() -> Result<Option<String>, String> {
    let before = config::load()?;
    let file = before
        .devnull
        .device_key
        .clone()
        .unwrap_or_else(|| sign::KEY_FILE.to_string());
    let path = config::dir().join(file);
    config::update(|cfg| cfg.devnull = DevnullConfig::default())?;
    Ok(path.exists().then(|| retained_key_line(&path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_use_the_short_hostname_then_the_environment() {
        let none = |_: &str| None;
        assert_eq!(
            device_name(Some("mac.local\n".into()), none),
            "SoCLI on mac"
        );
        let env = |key: &str| (key == "COMPUTERNAME").then(|| "WORKPC".to_string());
        assert_eq!(device_name(Some("  \n".into()), env), "SoCLI on WORKPC");
        let both = |key: &str| Some(format!("{key}-box"));
        assert_eq!(device_name(None, both), "SoCLI on HOSTNAME-box");
        assert_eq!(device_name(None, none), "SoCLI");
    }

    #[test]
    fn the_retained_key_line_names_the_file_and_the_revoke_page() {
        let line = retained_key_line(std::path::Path::new("/tmp/devnull_ed25519"));
        assert_eq!(
            line,
            "The device key stays at /tmp/devnull_ed25519. Revoke the device at https://dev-null.sh/settings/devices"
        );
    }

    #[test]
    fn key_paths_resolve_to_a_public_key_file() {
        let dir = std::env::temp_dir().join(format!(
            "socli-devnull-keypath-{}-{}",
            std::process::id(),
            sign::nonce()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let private = dir.join("id_ed25519");
        let public = dir.join("id_ed25519.pub");
        assert_eq!(public_key_file(&public).unwrap(), public);
        assert_eq!(
            public_key_file(&private).unwrap_err(),
            format!(
                "no public key next to {}. Pass the .pub file",
                private.display()
            )
        );
        std::fs::write(&public, "ssh-ed25519 AAAA test\n").unwrap();
        assert_eq!(public_key_file(&private).unwrap(), public);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
