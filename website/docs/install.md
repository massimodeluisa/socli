---
title: Install
description: Run SoCLI with npx or bunx, install the native command, or build it from source.
---

# Install

SoCLI runs as a native terminal binary. Use `npx soclish` or `bunx soclish` to run a published version without building Rust, or install the command on your `PATH`.

Use [Ghostty](https://ghostty.org/docs/install/binary) instead of Terminal.app. Install it from the [official installation page](https://ghostty.org/docs/install/binary). Ghostty draws profile pictures and post images. Terminal.app cannot, and SoCLI paints half blocks there. [Images](images.html) explains why.

Package-manager installation requires a [published release](https://github.com/massimodeluisa/socli/releases). If no release is listed, [build from source](#from-git). Homebrew uses the project tap until a Core formula is accepted.

## Run with npm or Bun

```sh
npx soclish
```

With Bun:

```sh
bunx soclish
```

Pass arguments after the package name, for example `npx soclish --demo` or `bunx soclish login bluesky`. The launcher selects the native executable for your operating system and CPU from the six binaries bundled in the package. Use Node.js 22 or newer with npm. `bunx --bun soclish` runs the launcher with Bun itself.

## Install the command

```sh
npm install --global soclish
soclish --version
```

A global installation puts `soclish` on your PATH. `socli` is the same program, so the other examples work without an `npx` or `bunx` prefix. Update it with `npm update --global soclish`.

## Homebrew

Install from the project tap:

```sh
brew tap massimodeluisa/tap
brew install socli
socli --demo
```

You can also install in one command with `brew install massimodeluisa/tap/socli`. Update the installed version with `brew update` followed by `brew upgrade socli`.

A fresh Homebrew installation needs the tap because SoCLI is not in Homebrew Core. Core acceptance requires a stable release and evidence of public use. The [Homebrew acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy) normally excludes repositories under 30 days old and applies higher popularity thresholds to self-submissions. Each SoCLI release includes a source formula for a future Core submission.

## macOS installer

Each release also has a disk image: `socli-<version>-aarch64-apple-darwin.dmg` for Apple Silicon and `socli-<version>-x86_64-apple-darwin.dmg` for Intel. Open the image and run `socli.pkg`. The package installs `socli` into `/usr/local/bin`. It is not notarized, so macOS asks for confirmation before it opens.

## Linux

```sh
curl -fsSL https://socli.sh/install.sh | sh
```

The script reads `/etc/os-release`, downloads the matching file from the latest [release](https://github.com/massimodeluisa/socli/releases), and checks `SHA256SUMS` before it installs. Debian and Ubuntu get the `.deb`. Fedora and other RPM systems get the `.rpm`. Alpine gets the `.apk`. That apk is not in the Alpine archive and is not signed, so the script passes `--allow-untrusted` after the checksum matches. Arch, Omarchy, and any other distribution get the musl binary in `/usr/local/bin`, because there is no pacman package. The script then prints `socli` and `socli --demo`.

`curl -fsSL https://socli.sh/install.sh | SOCLI_VERSION=0.1.2 sh` installs that version instead of the latest. Run the script again to install a newer release.

## Linux packages

The same release includes `.deb`, `.rpm`, and `.apk` files for ARMv7, ARM64, x86, and x64. Download the file that matches the machine from the [release](https://github.com/massimodeluisa/socli/releases) and install that file with `apt`, `dnf`, or `apk`. The package name is `socli`, and the executable is `/usr/bin/socli`.

A later release does not appear in `apt upgrade`, `dnf upgrade`, or `apk upgrade`. Those commands follow the archives already configured on the machine. SoCLI is not in the Debian, Fedora, or Alpine archives, and a release tag cannot add it there.

## From git

```sh
cargo install --git https://github.com/massimodeluisa/socli
```

Cargo puts `socli` on your cargo bin path, and `socli --version` prints the installed version.

## From a checkout

```sh
git clone https://github.com/massimodeluisa/socli
cd socli
cargo build --release
```

The binary is `target/release/socli`. Copy it to a directory on your `PATH` to call it as `socli`.

`rust-toolchain.toml` pins the compiler to channel `1.99.1`, and rustup installs it on the first build. `cargo run -- --demo` opens the demo feed from the checkout.

## Shell completions

```sh
socli completions zsh > ~/.zfunc/_socli
```

The argument is a shell name: `bash`, `zsh`, `fish`, `elvish` or `powershell`.

## Icons

With the default `auto` icon mode, SoCLI selects Nerd Font glyphs in Ghostty and WezTerm and Unicode icons in other terminals. To use Nerd Font glyphs, activate a Nerd Font in your terminal and choose `--icons nerd`. You can install one from [nerdfonts.com](https://www.nerdfonts.com/font-downloads).

`--no-icons` switches to Unicode icons that need no special font, and `NO_ICONS` in the environment does the same. `--icons auto|nerd|unicode` picks a set.

The font and its size are terminal settings, which a terminal program cannot change. In Cursor they are `terminal.integrated.fontFamily` and `terminal.integrated.fontSize`.
