---
title: Install
description: Run socli with npx or bunx, install the native command, or build it from source.
---

# Install

socli runs as a native terminal binary. Use `npx socli` or `bunx socli` to run a published version without building Rust, or install the command on your `PATH`.

Package-manager installation requires a [published release](https://github.com/massimodeluisa/socli/releases). If no release is listed, [build from source](#from-git). Homebrew uses the project tap until a Core formula is accepted.

## Run with npm or Bun

```sh
npx socli
```

With Bun:

```sh
bunx socli
```

Pass arguments after the package name, for example `npx socli --demo` or `bunx socli login bluesky`. npm selects the native executable for your operating system and CPU. Use Node.js 22 or newer with npm. `bunx --bun socli` runs the launcher with Bun itself.

## Install the command

```sh
npm install --global socli
socli --version
```

A global installation makes `socli` available directly, so the other documentation examples work without an `npx` or `bunx` prefix. Update it with `npm update --global socli`.

## Homebrew

Install from the project tap:

```sh
brew tap massimodeluisa/tap
brew install socli
socli --demo
```

You can also install in one command with `brew install massimodeluisa/tap/socli`. Update the installed version with `brew update` followed by `brew upgrade socli`.

A fresh Homebrew installation needs the tap because socli is not in Homebrew Core. Core acceptance requires a stable release and evidence of public use. The [Homebrew acceptance policy](https://docs.brew.sh/Package-Acceptance-Policy) normally excludes repositories under 30 days old and applies higher popularity thresholds to self-submissions. Each socli release includes a source formula for a future Core submission.

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

With the default `auto` icon mode, socli selects Nerd Font glyphs in Ghostty and WezTerm and Unicode icons in other terminals. To use Nerd Font glyphs, activate a Nerd Font in your terminal and choose `--icons nerd`. You can install one from [nerdfonts.com](https://www.nerdfonts.com/font-downloads).

`--no-icons` switches to Unicode icons that need no special font, and `NO_ICONS` in the environment does the same. `--icons auto|nerd|unicode` picks a set.

The font and its size are terminal settings, which a terminal program cannot change. In Cursor they are `terminal.integrated.fontFamily` and `terminal.integrated.fontSize`.
