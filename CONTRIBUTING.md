# Contributing

SoCLI is the name on the screen. The command, the crate, and this repository stay `socli`. The license is [MIT](LICENSE).

GitHub shows this file when you open an issue or a pull request. The code rules are in [docs/CONVENTIONS.md](docs/CONVENTIONS.md). Anything a person sees also follows [docs/DESIGN.md](docs/DESIGN.md).

## Issues

Search the open issues first. One issue is one problem.

Use the bug report when something failed, and the change request when you want different behavior. Say which command or screen you mean. For a failure, include `socli --version`, the operating system, what you expected, and the lines SoCLI printed. A change request says what you want and why, before a large patch.

Do not paste access tokens, app secrets, OAuth codes, or `~/.config/socli/config.toml`. If a sign-in report needs proof of what is stored, paste `socli auth status`. That command redacts secrets.

A report that fits neither form can be a blank issue.

If the report would have to include a live token or secret, leave the secret out and describe the path and the effect.

## Pull requests

Open the pull request against `main`. One pull request is one change. The description says what changed, and links the issue when there is one.

Commit subjects are English Conventional Commits: imperative, about 50 characters, no body, no co-author line, and one of `feat:`, `fix:`, or `docs:`. The pull request description can be longer than that subject.

Add one past-tense line under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md). If you add a dependency, the description says why. CI runs `cargo deny check` on new crates.

A pull request runs the checks below. It does not publish. Publishing runs only from a release tag. [docs/RELEASING.md](docs/RELEASING.md) describes that. Do not push a tag.

Leave files you did not need to change as they are. Tests stay offline: they do not call a network, and they do not read `~/.config/socli`.

A pull request is a contribution under the same MIT license.

## Checks

`rust-toolchain.toml` pins Rust 1.99.1 with rustfmt and clippy. `Cargo.toml` sets the minimum at Rust 1.89, and CI checks that version too.

```sh
git clone https://github.com/massimodeluisa/socli
cd socli
cargo test --locked
```

`cargo run -- --demo` opens the offline reader. `cargo run -- --demo --print` draws that feed into the scrollback. Demo mode does not sign in and does not write your config.

These are the checks a pull request runs:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Tests run on macOS, Ubuntu, and Windows. Clippy denies warnings.

A change to the reader or the browser demo also keeps native crates out of the wasm graph. The tree must not list `crossterm`, `signal-hook`, `reqwest`, `viuer`, or `ratatui-image`. The target is `wasm32-unknown-unknown` (`rustup target add wasm32-unknown-unknown`).

```sh
cargo check --locked --lib --no-default-features --target wasm32-unknown-unknown
cargo tree --locked --target wasm32-unknown-unknown --no-default-features --prefix none
```

A change to the JavaScript bridge also runs:

```sh
bun test scripts/release.test.ts scripts/soclib.test.ts scripts/install.test.ts
```

The native npm launcher is `soclish`. The browser library is `@massimodeluisa/soclib`.
