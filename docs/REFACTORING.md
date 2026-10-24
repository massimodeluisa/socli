# Refactoring roadmap for socli

This roadmap is the prescription that goes with [AUDIT.md](AUDIT.md). The audit diagnoses the code and numbers each finding, and this document orders the work that resolves those findings, so that socli reaches a layered shape without a step that breaks the reader, the browser build or the CLI. It builds on the audit's second pass, made on 2026-10-24 against the uncommitted tree on `main` (HEAD `0f11a62`), and it refers to findings by their AUD numbers. None of the work has been carried out yet. Wave 1, the first set of steps, waits for Massimo's go, because its first step commits the working tree.

The document opens with the principles every step follows and the practices socli borrows from reference Rust projects. It then walks through the phases in order, each with entry criteria, steps and an exit gate, and describes Wave 1 in enough detail to execute. The last three sections describe the layout the phases lead to, the ratchets that keep the gains, and the organization of the repository's files.

## Purpose and principles

The refactor follows one rule: make the change easy, then make the easy change. Most findings in the audit are expensive to fix in today's shape, because the same logic lives in several places (AUD-03, AUD-07, AUD-11) or one module holds many concerns (AUD-02, AUD-09, AUD-45). The early phases therefore add tests and cut dependency cycles, and the later phases move and merge code once those tests can show that nothing changed.

Three more rules keep each step reviewable. A commit carries one concern. A commit that moves code changes no behavior, and a commit that changes behavior moves no code, so a reviewer can read a move as a move. Every step leaves the gate green.

The gate is the set of checks that [CONVENTIONS.md](CONVENTIONS.md) uses to define a finished change, the four native checks followed by the three wasm checks:

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo check --no-default-features --target wasm32-unknown-unknown --lib
cargo tree --target wasm32-unknown-unknown --no-default-features
cargo build --release --no-default-features --target wasm32-unknown-unknown --lib
```

The `cargo tree` check passes when the wasm graph lists none of the crates that CONVENTIONS keeps out of the browser build. Once Phase 1 has added frame snapshots, a step that moves code must also leave every snapshot unchanged.

## What the reference projects do, and what socli takes from each

Five projects, or groups of projects, served as references. The table summarizes what socli takes from each, and the paragraphs after it give the detail and the sources.

| Project | What socli takes |
| --- | --- |
| ripgrep and BurntSushi | Focused crates, one definition that drives every surface of a setting, a strict CI, and `unwrap` reserved for invariants |
| Tokio | Per-feature checks, an MSRV job, and one owner of a client that the rest of the program sends requests to |
| rust-analyzer and matklad | An architecture document whose invariants tidy tests enforce, a flat workspace, xtask, and one integration-test binary |
| Serde and dtolnay | Discipline over dependencies and build time, and `thiserror` for library errors |
| rustls | `unsafe` confined to FFI, fuzzed parsers, secrets kept out of logs, and APIs in which the safe path is the only path |

[ripgrep](https://github.com/BurntSushi/ripgrep) splits its code into focused, reusable crates under `crates/`, such as `searcher`, `printer` and `ignore`, which is the model for the target layout below. It defines its flags once, in `crates/core/flags/defs.rs`, and that one definition drives argument parsing, the help text, the man page and the shell completions. socli applies the same idea to its settings: one `Setting` or `Key` descriptor drives `config list`, `config get` and `config set`, the Settings screen and the docs, which removes the parallel lists behind AUD-04 and AUD-14. ripgrep's [CI](https://github.com/BurntSushi/ripgrep/blob/master/.github/workflows/ci.yml) sets `permissions: contents: read`, tests a pinned toolchain alongside stable, beta and nightly, builds cross targets and a wasm target, runs rustdoc with `RUSTDOCFLAGS=-D warnings` and compiles its fuzz targets, and the `ci.yml` of Wave 1 starts from that shape. For errors, BurntSushi's [essay on `unwrap`](https://burntsushi.net/unwrap/) reserves `unwrap` for invariants, and ripgrep pairs concrete error types in its libraries with `anyhow`-style context at the binary edge. socli adopts both rules through AUD-26, whose typed `CliError` replaces the string sentinels.

[Tokio's CI](https://github.com/tokio-rs/tokio/blob/master/.github/workflows/ci.yml) checks each feature on its own with [cargo-hack](https://github.com/taiki-e/cargo-hack), builds with the declared MSRV, resolves dependencies with `-Z minimal-versions`, runs miri and nextest, and ends with a gate job. socli takes all six. `cargo hack check --each-feature` covers the `native` feature on and off, and it would have caught the host build that does not compile without `native` (AUD-43). Miri applies to the pure parts of socli, those with no I/O. The [channels chapter](https://tokio.rs/tokio/tutorial/channels) of the Tokio tutorial gives one task ownership of a client and has every other task send it requests over a channel. That is the model for the lasting fix of AUD-42, which socli builds on `std::thread` and `mpsc` without adopting an async runtime.

matklad's essay on [ARCHITECTURE.md](https://matklad.github.io/2021/02/06/ARCHITECTURE.md.html) describes a short document with a code map and a list of architecture invariants, many of them written as absences; in socli's terms, one such invariant is that `net` never imports a UI module. [rust-analyzer's architecture document](https://rust-analyzer.github.io/book/contributing/architecture.html) follows that form, and the project enforces such invariants with ordinary `#[test]` functions, called tidy tests. [Large Rust Workspaces](https://matklad.github.io/2021/08/22/large-rust-workspaces.html) recommends a flat `crates/` directory under a virtual manifest, with internal crates at `version = "0.0.0"`, and [cargo-xtask](https://github.com/matklad/cargo-xtask) keeps project automation in a Rust crate inside the workspace. [Delete Cargo Integration Tests](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html) argues for a single integration-test binary. socli adopts all of these, along with data-driven snapshot tests and unit tests kept in their own files through `#[cfg(test)] mod tests;`.

From Serde and dtolnay, socli takes discipline over dependencies and build time: `cargo build --timings` to see where the build spends its time, and a ban on duplicate crates. Libraries define their errors with [thiserror](https://github.com/dtolnay/thiserror). If type-state APIs appear, `trybuild` tests can check that misuse fails to compile.

[rustls](https://github.com/rustls/rustls/blob/main/rustls/src/lib.rs) declares `#![forbid(unsafe_code, unused_must_use)]` and `#![warn(missing_docs, clippy::exhaustive_enums, clippy::exhaustive_structs)]` at the top of its library. socli takes four rules from it. `unsafe_code` is denied everywhere outside one FFI module (Phase 2b), and forbidden outright in every crate but `web` once the workspace exists (Phase 5). Every parser of remote input gets a fuzz target, secrets never appear in logs, and an API is shaped so that the safe path is the only path. For socli, that last rule means a `PublicUrl` type that only `unfurl::public` can build, and an `open_browser` that takes a checked URL.

## The process, phase by phase

The phases run in order. A phase starts when its entry criteria hold, every step in it names the findings it closes, and the phase ends when its exit gate passes. Wave 1, in the next section, pulls the first steps forward from Phase 0 and Phase 2.

### Phase 0: baseline

Phase 0 puts the tree under version control and continuous integration, so that every later step has a commit to point at and a machine that runs the gate.

Entry criteria: Massimo has given the go to commit the working tree.

Steps:

1. Commit the current tree in logical commits on a branch (AUD-38), and add `workers/*/.wrangler/` to `.gitignore` in the same series (AUD-39).
2. Before the first push, fix the `ssh-key` guard in `pages.yml` so that the push deploys (AUD-40).
3. Add `ci.yml` with fmt, clippy with `-D warnings`, tests on macOS, Linux and Windows, the wasm check with a warning ratchet that later becomes `-D warnings`, `cargo deny` and an MSRV check (AUD-05, AUD-36). The `[lints]` table and a `clippy.toml` enter with it: a small set in which each lint has a stated reason, starting with `clippy::undocumented_unsafe_blocks = "deny"`, and a `too-many-lines-threshold` set to the current maximum (AUD-37, AUD-31). The wasm job runs clippy for wasm32 as well as the wasm check, so the lint sees the wasm-gated blocks, and the 9 `SAFETY:` comments of AUD-31 land in the same step. `unsafe_code` is denied at the exit of Phase 2b, and `unsafe_op_in_unsafe_fn` arrives with edition 2024 in Phase 6.
4. Move the Pages permissions to the deploy job, and add Dependabot (AUD-29).
5. State in the X sign-in documentation that the browser-session path presents itself as the x.com web client, and describe the account risk (AUD-33). This step changes no code.

Exit gate: the gate passes in the new CI on all three operating systems, and the Pages workflow passes its wasm-graph check.

### Phase 1: safety net

Phase 1 adds the tests that let the later phases move code safely. They pin what the reader draws and explore inputs that the example tests miss (AUD-47).

Entry criteria: the CI from Phase 0 runs the gate on every push.

Steps:

1. Snapshot the frames of the demo world at the four detail levels and three widths, drawn with ratatui's `TestBackend` and stored with [insta](https://insta.rs/).
2. Add [proptest](https://github.com/proptest-rs/proptest) property tests: `util::clean` never emits ESC or a C1 control, and properties for `hyperlink::safe`, the key codec, the command grammar and scoped ids. A get/set round trip over every key in `config::keys()` catches the wildcard slot of AUD-04.
3. Add [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets for Mastodon HTML, Open Graph parsing in `unfurl` and the network mappers.
4. Add a benchmark that runs `draw` over the demo world, so the per-frame allocations of AUD-46 are measured before anything changes.

Exit gate: the gate passes with the snapshots and the property tests in `cargo test`, and the fuzz targets compile.

### Phase 2: fixes

Phase 2 fixes the confirmed defects. Each fix is small, independent of the others and test-first: its regression test fails on the tree before the fix and passes after it.

Entry criteria: Phase 0 is complete. Each fix carries its own regression test, and Wave 1 takes four of these fixes, and the 9 `SAFETY:` comments of AUD-31, ahead of Phase 1.

Steps, one commit per finding:

| Finding | Fix |
| --- | --- |
| AUD-01 | A terminal guard armed right after raw mode is enabled, in `run` and in `tui::compose` |
| AUD-24 | The http/https allow-list inside `util::open_browser`, and `ShellExecuteW` on Windows |
| AUD-25 | A required `state` in the dev/null callback |
| AUD-26 | A typed `CliError` that replaces all three string sentinels |
| AUD-34 | The `signed_in` condition in the search arm of `plan_page` |
| AUD-42 | `try_lock` on the main thread, with the loading footer while a job holds the lock |
| AUD-06 | The corrected `DeleteRetweet` id, with the fallback-table test |
| AUD-15 | The missing reserved ranges, as a range table |
| AUD-30 | The 8 MiB response cap on the native client |
| AUD-35 | The code check on the X loopback, one authorize-URL builder per network, and a Mastodon `authorize_url` that cannot panic |
| AUD-43 | One cfg axis per concern |
| AUD-18 | The `web` feature removed, or used to gate the browser-only modules, together with AUD-43 |
| AUD-10 | One-shot posts reach `send_intent`, directly or through `publish()`, so they keep the composer's draft safety |
| AUD-17 | `RequestBuilder::json` surfaces a serialization failure instead of sending an empty body |
| AUD-20 | A status line that keeps the post count next to the failure |
| AUD-23 | A bound on `sentPictures` in the worker and on `PictureLayer.src` in the page |
| AUD-31 | `checked_add` in the wasm `decode`. The 9 `SAFETY:` comments already landed in Wave 1, step (c) |
| AUD-32 | The missing limits and checks: the OSC 52 cap, format characters in `util::clean`, the image cache bound and permissions, and the `Origin` checks and rate limit in the workers |

Exit gate: the gate passes, and every fix has a regression test that failed before it.

### Phase 2b: cut the cycles

Phase 2b breaks the 19-module cycle of AUD-41, narrows the public surface of the library (AUD-44) and gathers the wasm FFI into one module (AUD-31), so that the target layout has boundaries to follow. Each step is one commit and ends with a tidy test that asserts the removed edge stays removed.

Entry criteria: the snapshots from Phase 1 run in CI, because most of these steps move code.

Steps:

1. `watchdog` calls a restore hook that `tui` registers, held in a `OnceLock<fn()>`.
2. `Network::accent` moves to `theme`.
3. Providers return their login steps as `LoginStep` data, and `screens` presents them.
4. `Network`, `FeedKind` and the ids move to a leaf module, which ends the config↔net and model↔net cycles.
5. `minimal` and `render` share a lower helper module instead of importing each other.
6. The modules become `pub(crate)` and `cli::main` is re-exported, so `unreachable_pub` and `dead_code` show what is unused; the wasm build confirms that the exports survive (AUD-44).
7. The clock's platform source leaves `clock`, so that its time arithmetic can sit in `core`. The wasm import, with the three wasm32-gated `unsafe` blocks of `src/clock.rs`, moves into the FFI module of `web` (today `src/web.rs`), and the native source uses `std::time`. Each binary registers its source once at startup in a `OnceLock` that holds the two source functions, the wall clock and the monotonic clock (`socli_millis` and `socli_mono` today, both `fn() -> f64`), the same registration pattern as the restore hook of step 1. The `socli` binary's `main` registers the `std::time`-based sources before anything reads the clock, and `web` registers its two imports in its initialising export, `socli_new`.
8. The remaining wasm `extern "C"` imports move, each with its unsafe call, into one FFI module. Step 7 has already moved the clock's imports; the other four import blocks sit in `src/util.rs:491` (`socli_page`), `src/config.rs:824` (`socli_config_read`, `socli_config_write`), `src/net/pkce.rs:36` (`socli_random`) and `src/net/http.rs:457` (`socli_http`), beside `mod ffi` in `src/web.rs` (`:390`). `unsafe_code` is then denied crate-wide, with `#[allow(unsafe_code, reason = "…")]` only on that FFI module (AUD-31).

Exit gate: the gate passes with every snapshot unchanged, a tidy test guards each removed edge, the largest strongly connected module group has one module, and `unsafe_code` is denied everywhere except the FFI module.

### Phase 3: remove duplication behind tests

Phase 3 merges the copies that the audit found, one at a time and each behind tests.

Entry criteria: Phase 2b is complete, and the tests from Phase 1 and Phase 2 cover the code being merged.

Steps:

| Step | Findings |
| --- | --- |
| `net/oauth.rs` holds the page flow, the loopback server and the code check | AUD-03, AUD-13, AUD-35 |
| `html.rs` holds one HTML tokenizer and one entity table | AUD-07 |
| One setting descriptor drives the config keys and the Settings screen | AUD-04, AUD-14 |
| The providers live in one array | AUD-11 |
| `enabled()` replaces the repeated network filter | AUD-12 |
| One `rows_for` serves both builds | AUD-21 |
| Shared presentation helpers serve the three Rust renderers | AUD-08 |
| A product decision settles the future of `reader.ts` | AUD-08 |
| `parse_link` computes the raw host once | AUD-19 |

Exit gate: the gate passes with every snapshot unchanged, and the tests that covered each former copy pass against the shared code.

### Phase 4: mechanical splits

Phase 4 breaks the large files into modules. Every step is a pure move, which snapshot equality confirms.

Entry criteria: Phase 3 is complete, and the snapshots from Phase 1 pass.

Steps:

1. `src/tui.rs` becomes `src/tui/` (AUD-02).
2. `src/screens.rs` becomes `src/screens/` (AUD-09).
3. `src/render.rs` splits along the seams that the audit lists under its other observations.
4. The X query tables move to `src/net/x/web/queries.rs` (AUD-16).
5. The data tables move to their own files, such as `palettes/named.rs` and `help/tables.rs`, as ripgrep keeps its flag definitions in `defs.rs` apart from the parsing.
6. `State` gains owned sub-structs per concern (AUD-45).

Exit gate: the gate passes with every snapshot unchanged, and each commit is a pure move.

### Phase 5: workspace

Phase 5 turns the directories of the target layout into crates under a [virtual manifest](https://doc.rust-lang.org/cargo/reference/workspaces.html). This is stage B of the layout. Crate boundaries replace the `native` feature, which removes the cfg scatter behind AUD-05.

Entry criteria: the tidy tests for every planned crate boundary have held since stage A.

Steps:

1. Add the virtual manifest and a flat `crates/` directory with one crate per layer.
2. Build the `cdylib` only in the `web` crate (AUD-18).
3. Replace the `native` feature with crate boundaries (AUD-05, AUD-43).
4. Add an xtask for the wasm build, `wasm-opt` and a size budget.
5. Declare `#![forbid(unsafe_code)]` at the root of every crate except `web`, whose `unsafe` stays inside its FFI module (AUD-31).

Exit gate: the gate passes for the workspace, and the tidy tests hold across the crate boundaries.

### Phase 6: types, dependencies, storage and concurrency

Phase 6 makes the changes that cut across crates: new types, dependency upgrades, credential storage and the concurrency model.

Entry criteria: Phase 5 is complete.

Steps:

1. Add the `Secret`, `ScopedId` and `PublicUrl` newtypes; `Secret` carries the redacted `Debug` of AUD-28.
2. Move to edition 2024, ratatui 0.30 (which unifies crossterm), reqwest 0.13, toml 1 and rand 0.10 (AUD-36). Edition 2024 turns `unsafe_op_in_unsafe_fn` on by default: `cargo fix --edition` adds inner blocks to the 19 `unsafe fn` bodies in `src/web.rs` (17 exports plus `text_at` and `write_text`), and each new block gets its `SAFETY:` comment in the same step (AUD-31).
3. Store secrets in the OS keychain, as [gh](https://cli.github.com/manual/gh_auth_login) does, with the file as a fallback (AUD-28).
4. Serve the reader from its own origin with a Content Security Policy (AUD-27).
5. Give `Clients` to one network thread (AUD-42), so that `state` emits requests instead of calling providers inline, as `write_post` (`src/state.rs:2582`), `toggle_repost`, `toggle_flag`, `follow_user` and `delete_post` (`:2773`) do today.
6. Borrow posts in `posts_for` and swap frame buffers in `run`, if the benchmark from Phase 1 shows that the copies matter (AUD-46).

Exit gate: the gate passes, and `cargo tree -d` no longer lists crossterm.

## Wave 1, ready to execute

Wave 1 is the first set of steps, taken from Phase 0 and Phase 2 and written so that it can run as soon as Massimo says go. This document only describes the steps; it creates none of the files they name. Step (a) comes first, and step (b) must land before the first push of the committed tree, because that push would otherwise fail the Pages deploy.

| Step | Work | Acceptance check |
| --- | --- | --- |
| (a) | Commit the current working tree in logical commits on a branch, with the subject-only Conventional Commit messages that CONVENTIONS requires (AUD-38). The series also adds `workers/*/.wrangler/` to `.gitignore` (AUD-39), so that `git status --porcelain` prints nothing afterwards. This step needs Massimo's explicit go | `git status --porcelain` prints nothing on the branch, `main` still points at `0f11a62`, and the four native checks pass at the head of the branch |
| (b) | Remove `ssh-key` from the regex of the "Check the wasm graph" step in `.github/workflows/pages.yml` (`:47`), so the guard matches the forbidden list in CONVENTIONS (AUD-40) | The guard's pipeline prints nothing when run locally, while `cargo tree --target wasm32-unknown-unknown --no-default-features --prefix none \| grep -E '^ssh-key '` still prints `ssh-key v0.6.7`, and the Pages workflow passes "Check the wasm graph" on the first push |
| (c) | Add `.github/workflows/ci.yml` with six jobs: fmt; clippy with `-D warnings`; tests on macOS, Linux and Windows; a wasm job; a check with Rust 1.89, the declared MSRV; and `cargo deny check`. The Windows test job runs with `continue-on-error: true` until its first green run, then becomes blocking. If the MSRV job fails on 1.89, the same commit raises `rust-version` to the lowest toolchain that builds, so the job is not left red. The wasm job runs the wasm `cargo check` and `cargo clippy --lib --no-default-features --target wasm32-unknown-unknown`, so that clippy sees the wasm-gated blocks, and it holds a warning ratchet: 71 rustc warnings today, re-measured when Phase 0 lands, with any other warning on the wasm target counted toward the same ratchet. The new workflow sets `permissions: contents: read` at workflow level and pins every action by commit SHA. In the same step, `pages.yml` moves its `pages: write` and `id-token: write` permissions to the deploy job, `Cargo.toml` gains a `[lints]` table with `clippy::undocumented_unsafe_blocks = "deny"`, which fails the wasm job regardless of the ratchet, and a new `clippy.toml` sets `too-many-lines-threshold` to the current maximum, so that `too_many_lines` fires on no function on the first day and the ratchets lower it step by step. Because the lint denies an `unsafe` block without a comment, the comment part of AUD-31 lands in this step as well: a `SAFETY:` comment on each of the 9 blocks that have none. `unsafe_code` waits for the exit of Phase 2b, and `unsafe_op_in_unsafe_fn` for edition 2024 in Phase 6 (AUD-05, AUD-29, AUD-31, AUD-36, AUD-37) | Every blocking job passes on the branch. On the host, `cargo clippy --all-targets -- -D warnings` passes. On wasm32, the clippy run reports zero `undocumented_unsafe_blocks`, and the warning count does not exceed the baseline re-measured when Phase 0 lands. `too-many-lines-threshold` equals the current maximum, every `uses:` line in both workflows names a full commit SHA, and `pages.yml` grants `pages: write` and `id-token: write` only to the deploy job |
| (d) | Add `.github/dependabot.yml` with updates for cargo, GitHub Actions and bun in `website/` (AUD-29) | The file declares one update entry for each of the three ecosystems, and GitHub reports no configuration error for it after the push |
| (e) | Add `deny.toml` with advisory and license checks, and duplicate detection set to `multiple-versions = "warn"` (AUD-29, AUD-36) | `cargo deny check` passes locally and in CI, and it reports the 10 duplicated crate names as warnings |
| (f) | Fix AUD-24, AUD-34, AUD-01 and AUD-42, each in its own commit with a regression test, as the next table describes | Each regression test fails on the tree before its fix and passes after it, and the gate passes after every commit |
| (g) | Add a paragraph to `docs/integration/x/auth.md` and to `website/docs/login.md`. It says that the browser-session path imitates x.com's web client, with the public web bearer, a fixed Chrome user agent and a computed `x-client-transaction-id`, and that X may treat this as automation, which puts the account at risk. It names `socli login x --oauth` as the path X sanctions. The step changes no code (AUD-33) | Both pages contain the paragraph, and `cargo test` is unaffected |

The four fixes of step (f) are the Phase 2 fixes for these findings, and each regression test covers the behavior its finding describes.

| Finding | Fix | Regression test |
| --- | --- | --- |
| AUD-24 | The http/https allow-list, through `hyperlink::safe`, runs inside `util::open_browser` before any opener; on Windows, `ShellExecuteW` replaces `cmd /C start` | `open_browser` refuses a `file:` URL and a custom scheme, a URL that contains `&` reaches the opener whole, and the gallery's "open original" path refuses a remote `picture.original` with a custom scheme |
| AUD-34 | The search arm of `plan_page` checks `signed_in` | A network that is on and can search, but is not signed in, is left out of the search plan |
| AUD-01 | A terminal guard is armed right after raw mode is enabled, in `run` and in `tui::compose` | An error between enabling raw mode and creating the terminal leaves the terminal restored, in both functions |
| AUD-42 | The main thread takes the `Clients` lock with `try_lock` | A key pressed while a job holds the lock does not block the loop, and the loading footer shows |

## Target layout

The target layout follows from the module graph in AUD-41. Its layers point one way: each layer depends only on layers above it in the table, and `term` and `web` are siblings that do not depend on each other.

| Layer | Holds | Rules |
| --- | --- | --- |
| `core` | The model, `Network` and `FeedKind`, ids, the clock's time arithmetic, the key codec, the command grammar and intent | No I/O, and `forbid(unsafe_code)` |
| `config` | The config, drafts and `Secret` | The theme and image-mode enums move from here to `core` |
| `net` | The providers, the http shim, pkce, oauth and unfurl | Providers return `LoginStep` data |
| `reader` | State, render, layout, minimal, screens, chrome, settings, onboard, theme, palettes, hyperlink and the sanitizer | Turns state into lines, with no terminal I/O |
| `term` | The tui loop, the grid backend, previews, kitty placement, the watchdog, OSC 52 and the media cache | Native only |
| `web` | The FFI exports and the browser shims | The only `cdylib`, wasm only |
| `socli` (binary) | cli, plain, send, help and agent output | Depends on the layers above |
| `xtask` | The wasm build, `wasm-opt`, the size budget, tidy and generated docs | |

`core` keeps the clock's time arithmetic, and the clock's platform source moves out of it. The wasm import, which today sits in `src/clock.rs` behind three wasm32-gated `unsafe` blocks, moves into the FFI module of the `web` crate, and the native source uses `std::time`. Each binary registers its source once at startup in a `OnceLock` that holds the two source functions, the wall clock and the monotonic clock (`socli_millis` and `socli_mono` today, both `fn() -> f64`), the same registration pattern as the watchdog's restore hook (AUD-41). The `socli` binary's `main` registers the `std::time`-based sources before anything reads the clock, and `web` registers its two imports in its initialising export, `socli_new`. `core` therefore contains no cfg and keeps `forbid(unsafe_code)`. Step 7 of Phase 2b makes this move.

The layout arrives in two stages. In stage A, the layers become directories inside the one crate, during Phases 2b to 4. In stage B, which is Phase 5, a directory becomes a crate only after its tidy test has held.

The invariants are written as absences, in matklad's style, and a tidy test enforces each one:

- `net` never names a module of `reader` or `term`.
- `core` contains no cfg.
- Only the FFI module in `web` contains `unsafe`, and every other crate forbids `unsafe_code` once the workspace exists.
- Only `term` links crossterm.

## Ratchets and metrics

A ratchet records today's value of a metric and lets it move only toward its target. The last five rows are the lint counts of AUD-37, which the `[lints]` table from Phase 0 starts to hold. The baselines below were measured on 2026-10-24, with the commands in the Method section of [AUDIT.md](AUDIT.md#method). These figures are second-pass measurements, and each ratchet's baseline is re-measured on the commit that lands Phase 0, before any threshold is set. Where no number is given, the target is any value below the baseline.

| Metric | Baseline | Target |
| --- | --- | --- |
| Largest production file | 8,095 lines (`src/tui.rs`) | Lower only |
| Longest function | 835 lines (`draw`, from `fn` to its closing brace) | Lower only |
| Functions over 100 lines (clippy `too_many_lines`) | 22 | Lower only |
| Largest strongly connected module group | 19 modules | 1 |
| `pub mod` in `src/lib.rs` | 39 | 1 |
| Wasm warnings | 71 | 0 |
| Production `unwrap` and `expect` | 4 | Lower only |
| `unsafe` blocks without a `SAFETY:` comment | 9 | 0, reached in Wave 1 |
| `unsafe fn` bodies without inner `unsafe` blocks | 19 | 0 (Phase 6, edition 2024) |
| Distinct packages on normal edges | 229 native, 137 wasm | Lower only |
| Duplicated crate names | 10 | Lower only |
| Indexing that may panic (clippy `indexing_slicing`) | 159 | Lower only |
| String indexing (clippy `string_slice`) | 102 | Lower only |
| Wildcard enum arms (clippy `wildcard_enum_match_arm`) | 95 | Lower only |
| Casting warnings (the three cast lints) | 185 | Lower only |
| Public functions without an `# Errors` section | 125 | Lower only |

Two mechanisms hold the ratchets. Thresholds in `clippy.toml`, described in the [Clippy lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html), are lowered step by step as the code improves, and a tidy test holds an allowlist that may only shrink. The first such threshold is `too-many-lines-threshold`, which Wave 1 sets to the current maximum so that `too_many_lines` fires on no function; each step that shortens the longest function lowers it.

## Project organization

The repository root keeps `README.md`, `LICENSE`, `THIRD-PARTY-NOTICES.md` and the Cargo files. It gains a `CHANGELOG.md` and a `SECURITY.md`. Once `CHANGELOG.md` exists, the commit rule in CONVENTIONS applies to it, and each change first adds one past-tense line under `[Unreleased]`. An `ARCHITECTURE.md` with the code map and the invariants of the target layout completes the set. It sits at the repository root next to `README.md`, as matklad recommends, and [INDEX.md](INDEX.md) lists it once it exists. These three files are roadmap items, and none of them exists yet.

The other documents at the root move into `docs/`. `WEB_CLI.md` becomes `docs/web-architecture.md`, `ROADMAP.md` moves under `docs/plans/`, and `COMPLETION_REPORT_1.md` is deleted or archived (AUD-22). [INDEX.md](INDEX.md) stops linking ignored files, or those files become tracked (AUD-39). Tidy tests then check that the module map in CONVENTIONS has a row for every file and that INDEX lists every document.
