---
title: Install
description: How to install socli from git or from a checkout, and how to get shell completions and icons.
---

# Install

socli is a Rust program, and it runs as a terminal binary. You install it with cargo, or you build it from a checkout.

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
