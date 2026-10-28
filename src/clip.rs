//! Copies text to the clipboard with the system tool (pbcopy, clip.exe, wl-copy, xclip, xsel), or with OSC 52 over SSH and as the last resort.

#[cfg(feature = "native")]
use std::io::Write;
#[cfg(feature = "native")]
use std::process::{Command, Stdio};

use std::cell::RefCell;

thread_local! {
    static COPIED: RefCell<Option<String>> = const { RefCell::new(None) };
    static OPENED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The text the browser demo copied. The page writes it to `navigator.clipboard`.
pub fn take_copied() -> Option<String> {
    COPIED.with(|slot| slot.borrow_mut().take())
}

/// Remembers a URL the browser demo asked to open.
pub fn note_open(url: &str) {
    OPENED.with(|slot| *slot.borrow_mut() = Some(url.to_string()));
}

/// The URL the browser demo asked to open, if one is waiting.
pub fn take_open() -> Option<String> {
    OPENED.with(|slot| slot.borrow_mut().take())
}

#[cfg(feature = "native")]
use base64::engine::general_purpose::STANDARD;
#[cfg(feature = "native")]
use base64::Engine;

/// Where a copy ended up.
pub enum Via {
    System,
    Terminal,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[cfg(any(test, feature = "native"))]
enum Method {
    Pbcopy,
    ClipExe,
    WlCopy,
    Xclip,
    Xsel,
    Osc52,
}

/// Copies `text` to the clipboard, falling back to the terminal's OSC 52.
pub fn copy(text: &str) -> Result<Via, String> {
    #[cfg(not(feature = "native"))]
    {
        COPIED.with(|slot| *slot.borrow_mut() = Some(text.to_string()));
        Ok(Via::System)
    }
    #[cfg(feature = "native")]
    {
        let env = |key: &str| std::env::var(key).ok().filter(|value| !value.is_empty());
        for method in order(&env, std::env::consts::OS) {
            match method {
                Method::Osc52 => {
                    let tmux = env("TMUX").is_some();
                    return write_terminal(&osc52(text, tmux)).map(|()| Via::Terminal);
                }
                program => {
                    if let Some((name, args)) = command(program) {
                        if pipe(name, args, text) {
                            return Ok(Via::System);
                        }
                    }
                }
            }
        }
        Err("couldn't copy: no clipboard available".to_string())
    }
}

#[cfg(any(test, feature = "native"))]
fn order(env: &dyn Fn(&str) -> Option<String>, os: &str) -> Vec<Method> {
    let set = |key: &str| env(key).is_some_and(|value| !value.is_empty());
    if set("SSH_TTY") || set("SSH_CONNECTION") {
        return vec![Method::Osc52];
    }
    let mut methods = Vec::new();
    match os {
        "macos" => methods.push(Method::Pbcopy),
        "windows" => methods.push(Method::ClipExe),
        _ => {
            if set("WAYLAND_DISPLAY") {
                methods.push(Method::WlCopy);
            }
            if set("DISPLAY") {
                methods.push(Method::Xclip);
                methods.push(Method::Xsel);
            }
        }
    }
    methods.push(Method::Osc52);
    methods
}

#[cfg(any(test, feature = "native"))]
fn command(method: Method) -> Option<(&'static str, &'static [&'static str])> {
    match method {
        Method::Pbcopy => Some(("pbcopy", &[])),
        Method::ClipExe => Some(("clip.exe", &[])),
        Method::WlCopy => Some(("wl-copy", &[])),
        Method::Xclip => Some(("xclip", &["-selection", "clipboard"])),
        Method::Xsel => Some(("xsel", &["--clipboard", "--input"])),
        Method::Osc52 => None,
    }
}

/** Runs `name` with `text` on stdin; false when it is missing or exits non-zero. */
#[cfg(feature = "native")]
fn pipe(name: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(name)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            let _ = child.wait();
            return false;
        }
    }
    child.wait().is_ok_and(|status| status.success())
}

#[cfg(feature = "native")]
fn osc52(text: &str, tmux: bool) -> String {
    let sequence = format!("\x1b]52;c;{}\x07", STANDARD.encode(text.as_bytes()));
    if tmux {
        format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
    } else {
        sequence
    }
}

/**
OSC 52 goes to stdout when it is the terminal, else to stderr when that is, so a piped or agent run
never gets escape bytes in its output and a test run never reaches the owner's terminal.
*/
#[cfg(feature = "native")]
fn write_terminal(sequence: &str) -> Result<(), String> {
    use std::io::IsTerminal;
    let fail = |error: std::io::Error| format!("couldn't copy: {error}");
    if std::io::stdout().is_terminal() {
        let mut out = std::io::stdout().lock();
        return out
            .write_all(sequence.as_bytes())
            .and_then(|()| out.flush())
            .map_err(fail);
    }
    if std::io::stderr().is_terminal() {
        let mut err = std::io::stderr().lock();
        return err
            .write_all(sequence.as_bytes())
            .and_then(|()| err.flush())
            .map_err(fail);
    }
    Err("couldn't copy: no terminal to hold the link".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        move |key| {
            owned
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn osc52_utf8() {
        assert_eq!(osc52("é", false), "\x1b]52;c;w6k=\x07");
    }

    #[test]
    fn osc52_tmux_doubles_escapes() {
        assert_eq!(osc52("hi", true), "\x1bPtmux;\x1b\x1b]52;c;aGk=\x07\x1b\\");
    }

    #[test]
    fn order_macos() {
        let env = env_of(&[("DISPLAY", ":0")]);
        assert_eq!(order(&env, "macos"), [Method::Pbcopy, Method::Osc52]);
    }

    #[test]
    fn order_windows() {
        assert_eq!(
            order(&env_of(&[]), "windows"),
            [Method::ClipExe, Method::Osc52]
        );
    }

    #[test]
    fn order_linux_wayland() {
        let env = env_of(&[("WAYLAND_DISPLAY", "wayland-0"), ("DISPLAY", ":0")]);
        assert_eq!(
            order(&env, "linux"),
            [Method::WlCopy, Method::Xclip, Method::Xsel, Method::Osc52]
        );
    }

    #[test]
    fn order_linux_x11() {
        let env = env_of(&[("DISPLAY", ":0")]);
        assert_eq!(
            order(&env, "linux"),
            [Method::Xclip, Method::Xsel, Method::Osc52]
        );
    }

    #[test]
    fn order_linux_headless() {
        assert_eq!(order(&env_of(&[]), "linux"), [Method::Osc52]);
    }

    #[test]
    fn order_ssh_wins() {
        for key in ["SSH_TTY", "SSH_CONNECTION"] {
            let env = env_of(&[(key, "x"), ("DISPLAY", ":0")]);
            assert_eq!(order(&env, "macos"), [Method::Osc52]);
        }
    }

    #[test]
    fn missing_program_is_not_a_success() {
        assert!(!pipe("socli-no-such-clipboard", &[], "x"));
    }
}
