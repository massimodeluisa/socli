#[cfg(target_os = "macos")]
use std::io::Write;

pub fn native() -> bool {
    cfg!(target_os = "macos") && std::env::var("SOCLI_AUTH_BROWSER").as_deref() != Ok("manual")
}

#[cfg(target_os = "macos")]
pub fn authorize(url: &url::Url) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    let binary = include_bytes!(concat!(env!("OUT_DIR"), "/socli-auth"));
    let digest = format!("{:x}", Sha256::digest(binary));
    let contents = crate::config::dir()
        .join("browser-auth")
        .join(format!("socli-auth-{}.app", &digest[..16]))
        .join("Contents");
    let dir = contents.join("MacOS");
    let executable = dir.join("socli-auth");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|err| format!("could not prepare browser sign-in: {err}"))?;
    let plist = contents.join("Info.plist");
    if !plist.exists() {
        std::fs::write(&plist, br#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>sh.socli.browser-auth</string><key>CFBundleExecutable</key><string>socli-auth</string><key>CFBundleName</key><string>SoCLI</string><key>CFBundlePackageType</key><string>APPL</string><key>LSUIElement</key><true/></dict></plist>"#)
            .map_err(|err| format!("could not prepare browser sign-in: {err}"))?;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&executable)
    {
        Ok(mut file) => file.write_all(binary).map_err(|err| err.to_string())?,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            let installed = std::fs::read(&executable).map_err(|err| err.to_string())?;
            if Sha256::digest(&installed) != Sha256::digest(binary) {
                return Err("the browser sign-in helper does not match this socli build".into());
            }
        }
        Err(err) => return Err(format!("could not prepare browser sign-in: {err}")),
    }
    let result = {
        let _waiting = crate::program_status::waiting(crate::program_status::Kind::Auth);
        std::process::Command::new(&executable)
            .arg(url.as_str())
            .output()
            .map_err(|err| format!("could not open browser sign-in: {err}"))?
    };
    if !result.status.success() {
        return Err(if result.status.code() == Some(130) {
            crate::program_status::cancel();
            "cancelled".into()
        } else {
            "the browser could not complete sign-in. Retry, or set SOCLI_AUTH_BROWSER=manual for the pasted callback procedure".into()
        });
    }
    let callback = String::from_utf8(result.stdout)
        .map_err(|_| "the browser returned an unreadable callback")?;
    let callback = callback.trim();
    let parsed =
        url::Url::parse(callback).map_err(|_| "the browser returned an invalid callback")?;
    if parsed.scheme() != "devnull"
        || parsed.host_str() != Some("auth")
        || parsed.path() != "/callback"
    {
        return Err("the browser returned an unexpected callback".into());
    }
    Ok(callback.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn authorize(_url: &url::Url) -> Result<String, String> {
    Err("native browser sign-in is available on macOS".into())
}
