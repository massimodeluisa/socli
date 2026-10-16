use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=native/auth-macos.swift");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo sets target architecture");
    let arch = if arch == "aarch64" { "arm64" } else { &arch };
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-O",
            "-target",
            &format!("{arch}-apple-macosx12.0"),
        ])
        .arg("native/auth-macos.swift")
        .arg("-o")
        .arg(out.join("socli-auth"))
        .status()
        .expect("Building macOS browser authentication needs Xcode Command Line Tools");
    assert!(
        status.success(),
        "Could not build macOS browser authentication"
    );
}
