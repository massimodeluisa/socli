# Code audit — socli

**Date** 2026-10-23 · **Commit** `0f11a62` plus the uncommitted working tree (77 tracked files changed, 22 untracked; +7,845 / −4,675) · **Scope** `src/`, `tests/`, `website/.vitepress/theme/`, CI, repository docs.

**Date** 2026-10-24, second pass · **Commit** `0f11a62` plus the uncommitted working tree on `main` · **Scope** the first-pass findings, security, dependencies and toolchain, CI, the module graph and the refactoring process.

This audit looks for regressions, duplicated code, monolithic files, overengineering, weak decomposition and low reuse. It does not change code. Every finding cites the current working tree; where a problem predates the uncommitted refactor, that is stated. The second pass re-checked every first-pass finding against the tree of 2026-10-24, added a security review and a review of dependencies, toolchain, CI and the module graph, and continued the numbering from AUD-24. Line numbers throughout, including those in the first-pass entries, point at that tree. [REFACTORING.md](REFACTORING.md) turns the findings into a phased plan.

## Second pass (2026-10-24)

The second pass confirms most of the first: 20 of its 23 findings are still present exactly as described, and none has been fixed. The evidence-first method, the severity table, the separation of pre-existing defects from those the diff introduced, and the section on what works well all carry over. The test count moved from 739 on 2026-10-23 to 761 on 2026-10-24 as tests were added.

The first pass also left gaps, which this pass fills. It did not trace remote data into process spawning, and it did not review how the browser reader stores credentials, whether the pages send a Content Security Policy, or which permissions CI grants. It had no dependency or toolchain review. Its remediation for AUD-05 treats the symptom, the cfg scatter, while the cause is native and browser code mixed in one crate. It described no safe process for carrying out the refactor. Its severity also mixes user impact with maintainability cost, which a separate cost-of-delay axis would keep apart.

Each correction below is also applied inside its finding entry.

| ID | What changed |
| --- | --- |
| Baseline | The wasm check now reports 71 warnings, not 66: 35 in `tui.rs`, 10 in `unfurl.rs`, 7 in `media.rs`, 6 in `clip.rs`, 4 in `config.rs`, 3 in `screens.rs`, 3 in `net/threads/auth.rs`, 2 in `net/devnull/mod.rs` and 1 in `net/devnull/api.rs` |
| Baseline | Production code has 4 `unwrap`/`expect` calls, not 5: `Session::at` and `Session::live` (`src/web.rs:59` and `:107`), Mastodon's `authorize_url` (`src/net/mastodon/auth.rs:80`) and Threads' `authorize_url` (`src/net/threads/auth.rs:112`). The count of `panic!`-family calls is confirmed at 0 |
| AUD-01 | `run` installs the panic hook after `enter()` (`src/tui.rs:1099`), so a panic between `:1082` and `:1098` is unguarded. `tui::compose` (`:1920-1928`) has the same leak |
| AUD-02 | 8,095 production lines and 159 top-level functions; `draw` is 835 lines from `fn` to its closing brace and `handle_key` is 625 |
| AUD-03 | The X lines moved: `accept_code` is at `:526`, `begin_page` at `:673` and `finish_page` at `:699`. The Threads loopback is `accept_code` at `:314` and `write_callback` at `:356` |
| AUD-06 | 17 fallback ids, not 15. A fallback id is used only when the downloaded bundle lacks the operation or the download fails, because `store()` and `load_disk()` add fallbacks with `or_insert_with`. Severity drops from Medium to Low |
| AUD-08 | `plain.rs` does not repeat the in-text rule; it renders through `screens::linked_body`. `render.rs` has 113 production functions, not 181 |
| AUD-09 | 1,842 production lines and 52 top-level functions; the login code grew (`prompt_rail`, `RailCook`) |
| AUD-12 | The filter is spelled three times, and the search arm of `plan_page` omits `signed_in` (`src/state.rs:2391`). That omission is a behavior bug and has its own entry, AUD-34 |
| AUD-15 | Also unblocked: `192.88.99.0/24`, `2002::/16` (6to4, whose embedded IPv4 address is not unwrapped), `2001::/32` (Teredo), `64:ff9b:1::/48` and `100::/64` |
| AUD-16 | `FALLBACKS` holds 17 ids (`:37-55`); `FEATURES_JSON` starts at `:57` |
| AUD-17 | `pairs_of` is already documented as query- and form-only (`src/net/http.rs:479`), so that half of the remediation is dropped |
| AUD-21 | The two bodies are byte-identical and compile in mutually exclusive builds; the native copy stays "because its tests call it as a sibling" |
| AUD-23 | The growth claim holds. The claim that a failed image is never retried is wrong, because the page's `PictureLayer` loads the images, not the worker. `PictureLayer.src` (`wasm-reader.ts:990`) is unbounded as well |
| Other observations | Splitting `tests/cli.rs` by command group would add integration-test binaries, against the one-binary practice; modules under one `tests/it/main.rs` keep a single binary |
| First-pass entries | Every line in AUD-02, AUD-05, AUD-06, AUD-08, AUD-09, AUD-10, AUD-13, AUD-14, AUD-16, AUD-21 and the other observations is re-cited by symbol against the current tree. Re-measured counts, old → new: in AUD-02, `tui.rs` 11,546 → 12,395 lines in total and its tests 3,883 → 4,300 lines; in AUD-09, `screens.rs` 1,803 → 1,842 lines and 59 → 52 functions (the second pass counts production lines and top-level functions); in AUD-14, the settings `step()` 78 → 75 lines; in the other observations, `render.rs` 181 → 113 functions. AUD-10, AUD-13 and AUD-21 changed only in their line numbers |

Six new Medium findings lead the revision, and one process item comes before all of them. Links in Bluesky and Mastodon posts reach the operating system's URL opener without a scheme check (AUD-24), and a search over all networks plans requests for networks that are not signed in (AUD-34). The native binary links two copies of crossterm, and the declared minimum Rust version is never tested (AUD-36). The Pages deploy will fail its own `ssh-key` guard on the next push (AUD-40). One call from `watchdog` into `tui` welds 19 modules into a single dependency cycle (AUD-41), and a page fetch can freeze the key loop because the fetch thread and the key handler take the same lock (AUD-42). The refactor itself starts from an uncommitted tree (AUD-38): at the time of writing, 78 tracked files differ from HEAD by +10,260/−5,004 lines, and 27 untracked entries (37 files) sit beside them, so committing that tree comes first.

## Verdict

The codebase is in better shape than its size suggests. Native builds, tests, clippy and rustfmt are all green, 29% of the Rust is test code, and production code contains only four `unwrap`/`expect` calls, all with stated invariants. The provider abstraction, the capability tables and the SSRF resolver are genuinely good design.

The weak half is the browser build. The uncommitted refactor added a wasm reader, a worker HTTP relay, page OAuth and a second OAuth implementation per network. That half compiles with **71 warnings**, has no CI job that tests or lints it, and now contains three copies of the same security-sensitive callback flow. The largest structural liability is `src/tui.rs`: 8,095 production lines and 159 top-level functions in one module.

No critical security vulnerability was found. The first pass confirmed two functional bugs, both older than the refactor: an invalid `DeleteRetweet` id in the X web fallback table (AUD-06, which the second pass lowered to Low because only the fallback path uses it) and a terminal-restore bug on the reader's error path (AUD-01). The second pass added two user-visible defects, a search that plans requests for networks without a session (AUD-34) and a key loop that a page fetch can freeze (AUD-42), and one Medium security finding, post links that reach the operating system's URL opener without a scheme check (AUD-24).

### Findings at a glance

| ID | Severity | Finding | New in this diff? |
| --- | --- | --- | --- |
| AUD-01 | High | Terminal left in raw mode / alternate screen on reader setup errors | No |
| AUD-02 | High | `src/tui.rs` is an 8,095-line god module; `draw` is 835 lines | No (grew) |
| AUD-03 | High | OAuth page flow and loopback server copied across X, Mastodon and Threads, already drifting | **Yes** |
| AUD-04 | High | Config keys enumerated in four functions; `set()` silently falls through to `ui.images` | No |
| AUD-05 | High | Wasm build emits 71 warnings; CI runs no Rust tests, clippy or fmt | **Yes** (warnings) |
| AUD-06 | Low | `DeleteRetweet` fallback query id contains a space, so un-repost fails for web sessions that fall back to it | No |
| AUD-07 | Medium | Two HTML parsers (`unfurl.rs`, `mastodon/html.rs`) with 23 identical windows | No |
| AUD-08 | Medium | Four renderers re-derive link/media presentation, including a 1,035-line TypeScript fallback | Mixed |
| AUD-09 | Medium | `screens.rs` mixes login, share, pickers, ask and help (52 top-level functions) | No |
| AUD-10 | Medium | One-shot `post`/`reply`/`quote` bypasses the draft safety used by the composer | No |
| AUD-11 | Medium | `providers` / `providers_mut` duplicate the five-network fan-out | No |
| AUD-12 | Medium | `plan_page` repeats the signed-in/capability/on filter three times | No |
| AUD-13 | Medium | Loopback callback accepts the first TCP connection, with no path filter or accept deadline | No (second copy is new) |
| AUD-14 | Medium | Adding a setting requires coordinated edits in six places | No |
| AUD-15 | Medium | SSRF allow-list misses several reserved/documentation ranges | No |
| AUD-16 | Medium | `net/x/web.rs` mixes 1,273 lines of query data with logic; no id-format test | No |
| AUD-17 | Low | Wasm HTTP shim hides serialization failure with `unwrap_or_default()` | **Yes** |
| AUD-18 | Low | `web` Cargo feature is declared but never used; `cdylib` is built for native too | **Yes** |
| AUD-19 | Low | `parse_link` computes `host` and `server` twice | No |
| AUD-20 | Low | A partial merged load overwrites the post count with the error | No |
| AUD-21 | Low | `rows_for` is copied between `preview.rs` and `preview_native.rs` | **Yes** |
| AUD-22 | Low | `COMPLETION_REPORT_1.md` is stale and contradicts the tree | No |
| AUD-23 | Low | `sentPictures` grows for the life of a browser session and marks URLs before load | **Yes** |
| AUD-24 | Medium | Post links reach the OS opener without a scheme allow-list | Not assessed |
| AUD-25 | Low | dev/null accepts a callback without `state` | Not assessed |
| AUD-26 | Low | Exit status and control flow are derived from message text | Not assessed |
| AUD-27 | Hardening | The browser reader keeps every credential in `localStorage` on the docs origin | Not assessed |
| AUD-28 | Hardening | Secrets are stored in plaintext and their structs derive `Debug` | Not assessed |
| AUD-29 | Low (hardening) | CI grants Pages permissions to the build job, pins actions by tag, and has no Dependabot or `cargo deny` | Not assessed |
| AUD-30 | Low | The native API client has no response body cap | Not assessed |
| AUD-31 | Low | `unsafe` blocks have no `SAFETY` comments, and no lint limits `unsafe` | Not assessed |
| AUD-32 | Low | Smaller missing bounds and checks (OSC 52, format characters, image cache, Threads worker, dev relay) | Not assessed |
| AUD-33 | Docs | The X web-session risk is undocumented | Not assessed |
| AUD-34 | Medium | Search plans networks that are not signed in | Not assessed |
| AUD-35 | Low | X loopback skips code validation; X and Mastodon build their authorize URL twice | Not assessed |
| AUD-36 | Medium | Two crossterm copies, dependencies behind their current releases, and an untested MSRV | Not assessed |
| AUD-37 | Low | No lint policy | Not assessed |
| AUD-38 | Process | The refactor base is uncommitted | Not applicable |
| AUD-39 | Docs | The docs index links ignored files, and several documents are stale | Not assessed |
| AUD-40 | Medium | The Pages workflow fails its own wasm-graph guard | **Yes** |
| AUD-41 | Medium | The network layer reaches into the UI, and one call welds 19 modules into a cycle | Not assessed |
| AUD-42 | Medium | A page fetch can freeze the key loop | Not assessed |
| AUD-43 | Low | Two cfg axes disagree, and a host build without `native` does not compile | Not assessed |
| AUD-44 | Low | The library exports its whole interior | Not assessed |
| AUD-45 | Low | `State` and `Ui` gather every concern | Not assessed |
| AUD-46 | Low (performance, unmeasured) | The reader allocates on every frame | Not assessed |
| AUD-47 | Low | The test portfolio has one shape | Not assessed |

Second-pass rows say "Not assessed" in the last column unless their entry compares the tree with HEAD.

## Baseline

| Check | Result |
| --- | --- |
| `cargo build` | Pass |
| `cargo test` | 690 unit + 49 integration tests pass; 0 ignored (first pass, 2026-10-23) |
| `cargo clippy --all-targets -- -D warnings` | Pass |
| `cargo fmt --check` | Pass |
| `cargo check --target wasm32-unknown-unknown --no-default-features` | **71 warnings** (66 in the first pass) |
| CI | `.github/workflows/pages.yml` builds wasm and the website only; no test, clippy or fmt job |

Rust volume on 2026-10-24: 62,631 lines in `src/` and `tests/`, of which 44,291 are production (the lines of each `src/` file above its test module) and 18,340 are tests (the test modules plus `tests/`), so tests are 29% of the Rust. The first pass reported 60,596 lines and 33% over `src/`, `tests/`, `native/` and `workers/`, with a method it did not record. Production `unwrap`/`expect`: 4, all invariant-guarded (`Session::at` at `src/web.rs:59`, `Session::live` at `src/web.rs:107`, and the `authorize_url` functions at `src/net/mastodon/auth.rs:80` and `src/net/threads/auth.rs:112`). Production `panic!`: 0.

## High severity

### AUD-01 — The reader can leave the terminal in raw mode

**Evidence** In `run`, `src/tui.rs:1081` enables raw mode, `:1089` enters the alternate screen, `:1104-1105` creates the terminal with `?`, and `leave()` is only called after the event loop at `:1305`. The panic hook at `:1099-1103` restores the terminal, but the `Err` path does not. `enter()` itself (`:2398-2401`) can fail after `EnterAlternateScreen`, and `enable_input()` (`:2417-2425`) can fail after enabling bracketed paste or focus reporting. The hook is also installed only after `enter()`, at `:1099`, so a panic between `:1082` and `:1098` runs with no restore at all. `tui::compose` (`:1920-1928`) has the same leak: it enables raw mode at `:1920`, calls `enter()` with `?`, and installs its hook only afterwards.

**Impact** Any failure between raw mode and a successful `Terminal::new` returns to the shell with raw mode and/or the alternate screen still active. The watchdog restores only when the terminal disappears or the main thread goes stale; it does not run on a normal `Err` exit. The user sees a broken prompt and has to run `reset`.

**Remediation** Arm an RAII guard immediately after `enable_raw_mode`:

```rust
struct TerminalGuard { enhanced: bool }
impl Drop for TerminalGuard {
    fn drop(&mut self) { leave(self.enhanced); }
}
```

Keep the panic hook for abort paths. The guard makes every `?` and every early return safe, including a partial `enter()`. Arm the same guard in `tui::compose`.

### AUD-02 — `src/tui.rs` is a god module

**Evidence** 12,395 lines total, 8,095 production, 159 top-level functions. The lengths below run from `fn` to the closing brace: `draw` is 835 lines (`:5516`; clippy's `too_many_lines` counts 824), `handle_key` 625 (`:2510`), `composer` 283 (`:7456`), `run` 240 (`:1068`), `composer_key` 186 (`:3255`), `activate` 153 (`:4655`), `draw_gallery` 142 (`:6793`), `click` 125 (`:4994`), `paint_previews` 117 (`:6528`), `settings_key` 100 (`:3499`). These counts were measured on 2026-10-24, at the end of the second pass. The file also owns terminal setup/teardown, the event loop, input batching, mouse hit-testing, sign-in forms, settings, the command line, ask mode and preview placement.

**Impact** Review and merge cost is high, `#[cfg(target_arch = "wasm32")]` branches are scattered through the file (35 of the 71 wasm warnings originate here), and the 4,300 lines of tests sit in the same compilation unit. A change to one mode routinely touches an 8,000-line file.

**Remediation** Split into `src/tui/` with `run.rs` (lifecycle, loop, batching), `keys.rs` (`handle_key`, `navigate_key`, `composer_key`, `settings_key`, `accounts_key`, `submit`, `activate`, `click`, `wheel`), `draw.rs` (`draw`, `paint_previews`, `overlay_rows`, `panes`), `composer.rs`, `forms.rs` (sign-in/edit), and `ask.rs`. Keep `tui.rs` as a module that re-exports. This is a mechanical move; the existing tests make it low risk. While there, break `draw` into per-region functions (`header`, `body`, `bottom`, `overlays`) so each fits on a screen.

### AUD-03 — OAuth page flow and loopback server are triplicated

**Evidence**

| Piece | X | Mastodon | Threads |
| --- | --- | --- | --- |
| `begin_page` | `src/net/x/auth.rs:673` | `src/net/mastodon/auth.rs:315` | `src/net/threads/auth.rs:371` |
| `finish_page` | `src/net/x/auth.rs:699` | `src/net/mastodon/auth.rs:355` | `src/net/threads/auth.rs:393` |
| `clear_page_oauth` | `src/net/x/auth.rs:663` | `src/net/mastodon/auth.rs:305` | `src/net/threads/auth.rs:408` |
| Loopback server | `src/net/x/auth.rs:526` (`accept_code`), `:565` (`write_http`), `:574` (`page`) | — | `src/net/threads/auth.rs:314`, `:356` |

The three `finish_page` bodies read the same pending state, clear it, trim and validate the code, compare state, exchange, then save. The copies have already drifted:

- X's `finish_page` rejects `&` in the code (`x/auth.rs:713`); Mastodon's `finish_page` does not (`mastodon/auth.rs:365`).
- Threads' `finish_page` trims a trailing `#_` (`threads/auth.rs:398`), and Threads has no PKCE verifier at all.
- Mastodon requires `client_secret`; X does not.
- X uses an `EXPIRED` constant; Mastodon and Threads inline the same sentence.
- The callback HTML and HTTP response writer are byte-identical between X and Threads.

This is new in the current diff: the page flow was added for the wasm reader by copying X's implementation.

**Impact** A fix to state handling, code validation, expiry or the callback page has to be made three times, and the drift above shows it already was not. This is the most security-sensitive code in the repository.

**Remediation** Add `src/net/oauth.rs` with:

- `Pending { state, verifier, redirect, client_id, client_secret }` and `Pending::take(section)`.
- `check_code(code, got_state, pending) -> Result<String, String>` implementing the union of today's rules (trim, reject whitespace and `&`, cap at 512, constant-time state compare).
- `Loopback::bind(port)`, `Loopback::accept()` with path filtering and a deadline, and `callback_page(message)`.
- Per-provider `exchange(code, verifier) -> Token` only.

Then each network keeps a thin `begin_page`/`finish_page` that calls the shared code.

### AUD-04 — The config key surface is duplicated four times, with a silent fall-through

**Evidence** `src/config.rs` enumerates the same 19 keys plus palette keys in `keys()` (`:504`), `get()` (`:553`), `default_of()` (`:578`) and `set()` (`:611`). `set()` validates the value in a first match (`:613-670`) and then chooses the destination field in a second match (`:671-685`). `ui.images` is handled in the first match (`:649`) but omitted from the second, relying on:

```rust
_ => &mut config.ui.images,
```

`ColorsFile` (`:186`) and `ColorOnly` (`:196-198`) duplicate every network section just to recover `color`, and the config text is parsed twice on load, in `load_at` (`:715-716`) and `load_browser` (`:782-783`).

**Impact** Adding a key to validation but forgetting the slot match silently writes the value into `ui.images` instead of failing. The compiler cannot help. Four coordinated edits per key also makes `config list` / `get` / `set` easy to diverge.

**Remediation** Replace the four matches with one table:

```rust
struct Key {
    name: &'static str,
    get: fn(&Config) -> Option<String>,
    set: fn(&mut Config, String),
    default: fn() -> Option<String>,
}
```

and look up by name. Make `set` return `Err(unknown_key(key))` on a miss instead of a wildcard. Deserialize once into a struct that holds both the typed config and the raw `[x] color` fields (for example with `#[serde(flatten)]`), removing `ColorsFile` and the second parse.

### AUD-05 — The wasm build is unlinted and emits 71 warnings

**Evidence** `cargo check --target wasm32-unknown-unknown --no-default-features` produces 71 warnings: 35 in `tui.rs`, 10 in `unfurl.rs`, 7 in `media.rs`, 6 in `clip.rs`, 4 in `config.rs`, 3 in `screens.rs`, and 6 across `threads/auth.rs` (3), `devnull/mod.rs` (2) and `devnull/api.rs` (1). They include:

- `src/tui.rs:4134` — unreachable `Ok(false)` at the end of `sign_in`, because both the wasm branch (`:4107-4113`) and the native branch return.
- `src/tui.rs:4091` — the `clients` parameter of `sign_in`, unused on wasm.
- Dead constants, statics, fields and imports across the native-only halves of `unfurl`, `media`, `clip`, `preview`.

The cause is module-level gating: `src/lib.rs` includes `clip`, `media`, `preview` and `unfurl` for every target, while large parts of each are native-only.

CI (`.github/workflows/pages.yml`) builds the wasm library and the website but never runs `cargo test`, `clippy` or `fmt`, and never passes `-D warnings` to the wasm check.

**Impact** The browser half is the least-validated half. An unused or unreachable change merges silently, and a native-only item can enter the wasm graph without anyone noticing until the bundle grows or a link fails.

**Remediation** Add a `ci.yml` with two jobs:

1. Native: `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
2. Wasm: `RUSTFLAGS="-D warnings" cargo check --no-default-features --target wasm32-unknown-unknown --lib`.

Then fix the warnings by moving native-only items behind `#[cfg(feature = "native")]`, or by splitting each mixed module into a shared core and a native half (`preview` already has that shape; `unfurl`, `media` and `clip` do not). Gating whole modules at the `lib.rs` boundary only works where nothing shared uses them. The unreachable branch should become:

```rust
#[cfg(not(target_arch = "wasm32"))]
Ok(false)
```

The second pass treats the cfg scatter as a symptom. Its cause is native and browser code mixed in one crate, which Phase 5 of [REFACTORING.md](REFACTORING.md) removes with crate boundaries. Wave 1 in the same document lists the CI jobs, including a ratchet on the 71 warnings.

## Medium severity

### AUD-07 — Two HTML parsers, already diverging

**Evidence** `src/unfurl.rs` has `tag_end` (`:429`), `attributes` (`:443`), `decode` (`:509`) and `entity` (`:534`). `src/net/mastodon/html.rs` has `tag_end` (`:115`), `attribute` (`:90`), `decode` (`:128`) and `entity` (`:153`). A production-only scan finds 23 identical 10-line windows between the two files. `decode` is byte-identical; `entity` differs only by Mastodon adding `nbsp`; `attributes` and `attribute` are two interfaces over the same tag scanner.

**Impact** HTML edge cases (quotes, self-closing tags, numeric entities, malformed markup) are fixed in one parser and not the other. Mastodon HTML is remote user content, so a divergence is user-visible: a link preview may decode differently from a status body.

**Remediation** Add `src/html.rs` with one tokenizer, one entity table (the superset) and both `attributes()` and `attribute()` as thin wrappers. Point `unfurl.rs` and `mastodon/html.rs` at it. Add a shared table of malformed inputs as tests.

### AUD-08 — Four renderers re-derive the same presentation rules

**Evidence** The repository has four output paths:

| Renderer | Size | Purpose |
| --- | --- | --- |
| `src/render.rs` | 2,759 prod lines, 113 functions | Rich TUI |
| `src/minimal.rs` | 565 prod lines | Low-detail TUI |
| `src/plain.rs` | 483 lines | One-shot CLI text |
| `website/.vitepress/theme/reader.ts` | 1,035 lines | Scripted fallback when wasm fails |

Link and media decisions are re-implemented in each. `render.rs` has `link_card` (`:1775`), `link_row` (`:1817`), `media_card` (`:1983`) and `preview` (`:2027`); `minimal.rs` has `media` (`:328`) and `link` (`:368`). `plain.rs` does not repeat the "is the link already in the text?" rule; it renders post bodies through `screens::linked_body` (`src/plain.rs:121`). `reader.ts` reimplements feed, thread and help rendering in TypeScript solely for the case where the wasm module cannot load (the module comment of `wasm-reader.ts`, `:1-5`).

**Impact** A new post field, a new link rule or a change to media labels has to be made in three Rust renderers and, if the fallback matters, a fourth TypeScript one. The TS fallback will silently diverge from the Rust reader; it already has its own `Post` shape (`reader.ts:14-26`).

**Remediation**

1. Extract presentation decisions into a small shared module: `link_to_show(post) -> Option<&Link>`, `media_label(post) -> String`, `link_is_in_text(post, link) -> bool`. Have all three Rust renderers call it. This removes the duplicated logic without forcing one drawing model on three detail levels.
2. Treat `reader.ts` as an explicit product decision. Either replace it with a static "the reader needs WebAssembly" panel, or freeze it and document that it is not feature-parity. Keeping a second renderer alive indefinitely is the most expensive form of reuse debt in the repository.

### AUD-09 — `screens.rs` has many unrelated responsibilities

**Evidence** 1,842 production lines, 52 top-level functions: login frames, prompts and results (`login_frame` `:184`, `present_login` `:278`, `prompt_rail` `:380`, `login_result` `:412`, and the `RailCook` guard at `:790`), notices (`notice_lines` `:845`), the linked body (`linked_body` and `linked_body_sized`, `:936-1014`), key help (`key_help_lines` `:1085`), the share dialog (`share_targets` to `share_lines`, `:1354-1493`), pickers (`network_rows` to `pick_lines`, `:1512-1668`) and ask screens (`ask_welcome`, `ask_answer` and `ask_prompt`, `:1679-1841`). The login part has grown since the first pass, with `prompt_rail` and `RailCook`. The module map in `docs/CONVENTIONS.md` describes it as "screens outside the feed", which is accurate but too broad to guide placement.

**Impact** Unrelated changes collide in one file, tests for the share dialog sit next to tests for login, and the file is on track to become a second `tui.rs`.

**Remediation** Split into `src/screens/login.rs`, `share.rs`, `pick.rs`, `ask.rs`, `help.rs`, `notice.rs`, keeping `screens.rs` as the shared canvas helpers and re-exports. This is a mechanical move.

### AUD-10 — One-shot publishing bypasses draft safety

**Evidence** `Publish::run` (`src/cli.rs:1660-1714`) reads the text, then calls `send::send` and `send::posted` directly. The composer path and `publish()` (`src/cli.rs:1723-1773`) call `send::send_intent` (`src/send.rs:115`), which writes the intent as a draft *before* any network call, removes it on full success, and rewrites it with the failed networks on a partial failure. `Publish::run` does none of that; it only delegates to `publish()` when no text was given and the composer opens.

**Impact** `socli post "long text"` that fails on one of several networks leaves no draft, even though the doc comment on `send_intent` (`send.rs:108-114`) says the text is kept until every network takes it. The typed text survives only in shell history. This predates the refactor (`HEAD:src/main.rs` had the same split), but the refactor was the moment to unify it.

**Remediation** Build an `Intent` in `Publish::run` and call `publish()` for both branches, or call `send_intent` directly with the same `store`. Keep the composer-specific "saved"/"discarded" outcomes in `Publish::run`.

### AUD-11 — `providers` and `providers_mut` duplicate the network fan-out

**Evidence** `providers` (`src/net/mod.rs:466-493`) and `providers_mut` (`:496-523`) are the same five-branch function, differing only in `&` versus `&mut`. Both special-case X and Threads as `Option`, and both append the test stub under `#[cfg(test)]`.

**Impact** Adding a network means editing the free function `provider` (`src/state.rs:2855`), `providers`, `providers_mut` and the capability tables. Forgetting one copy silently drops the network from merged loads or from `infos()` (`:525`), which is a behavior bug that the compiler will not catch.

**Remediation** Either:

- a `for_each_provider!` macro that expands the five branches once, or
- store the providers in an array indexed by `Network` and implement `providers`/`providers_mut` over it.

Add a test asserting that `providers().len()` equals the number of configured networks and that each `Network::ALL` appears once.

### AUD-12 — `plan_page` repeats the network filter three times

**Evidence** In `plan_page`, `src/state.rs:2317-2328` (feeds), `:2363-2371` (mentions) and `:2387-2393` (search) each spell out the filter, while the profile arm (`:2416`) relies on `self.check`. The mentions arm reads:

```rust
self.nets.iter()
    .filter(|info| info.signed_in && info.caps.mentions && self.on.contains(&info.net))
    .map(|info| info.net)
    .collect()
```

The search arm repeats it with `caps.search` but leaves out `info.signed_in` (`:2391`), a behavior bug that has its own entry, AUD-34. The feed arm repeats it with `resolve_feed`.

**Impact** A change to what "enabled" means (for example, honoring a per-feed network list, or treating a network with a broken session as off) has to be applied in three places. The copies already differ in whether they call `self.check` for the single-network case, and the search copy has already lost `signed_in`.

**Remediation** Add:

```rust
fn enabled(&self, cap: impl Fn(&Caps) -> bool) -> Vec<Network> {
    self.nets.iter()
        .filter(|info| info.signed_in && self.on.contains(&info.net) && cap(&info.caps))
        .map(|info| info.net)
        .collect()
}
```

and use it in every arm, with `self.check` only for the explicit `Scope::One` cases.

### AUD-13 — The loopback callback is fragile

**Evidence** X's `accept_code` (`src/net/x/auth.rs:526-532`) and Threads' `accept_code` (`src/net/threads/auth.rs:314-322`) both:

1. call `listener.accept()` once, with no deadline;
2. take the first connection regardless of its request path;
3. set a read timeout only after accepting.

**Impact** A stray request to `127.0.0.1:<port>` — a browser favicon request, a prefetch, another local tool probing the port — consumes the single `accept()`. The real OAuth callback then waits forever (the read timeout applies to the consumed stream, not to `accept()`). The user sees "waiting for the browser" and has to kill the command. The duplicated implementation means any fix has to be made twice.

**Remediation** Fold this into `net/oauth.rs` (AUD-03): loop over `accept()`, ignore requests whose path has neither `code` nor `error` (respond `404` and continue), and set a total deadline for the flow. A single `Loopback` type used by X and Threads removes the second copy.

### AUD-14 — Adding a setting touches six places

**Evidence** A setting lives in:

1. `Row` (`src/settings.rs:18-34`),
2. `ROWS` (`:36-56`, with a hard-coded length of 19),
3. `step()` (`:171-245`, including the config key string),
4. `lines()` (`:268-513`, a manual drawing block per row),
5. `config::keys()` (`src/config.rs:504`),
6. `config::get` / `default_of` / `set` (`:553`, `:578`, `:611`).

`lines()` is 246 lines and `step()` is 75 lines of near-identical arms. `ROWS` must also stay in sync with the `Row` variants, which the compiler does not check.

**Impact** A new setting is a six-file edit with three silent failure modes: a missing `lines` arm draws nothing, a missing `config::get` arm returns `None`, and a missing `set` slot writes `ui.images` (AUD-04). The `Row` enum makes `step` exhaustive, which is the one part that is safe.

**Remediation** Introduce a `Setting` descriptor:

```rust
struct Setting {
    row: Row,
    key: &'static str,
    section: &'static str,
    label: &'static str,
    step: fn(&mut State, i32) -> String,
}
```

and drive `ROWS`, `lines()` and the config table from one array. Keep `Row` for cursor identity and hit-testing. This also gives `config list` and Settings one source of truth for labels.

### AUD-15 — The SSRF allow-list misses reserved ranges

**Evidence** `src/unfurl.rs:348-359` (`global_v4`) blocks loopback, unspecified, private, link-local, broadcast, multicast, `100.64.0.0/10` and `0.0.0.0/8`. It does **not** block:

- `198.18.0.0/15` (benchmarking),
- `240.0.0.0/4` (reserved),
- `192.0.0.0/24` (IETF protocol assignments),
- `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24` (documentation),
- `192.88.99.0/24` (the former 6to4 relay anycast range).

`global_v6` (`:362-384`) blocks loopback, unspecified, multicast, `fe80::/10` and `fc00::/7`, and judges IPv4-mapped, IPv4-compatible and `64:ff9b::/96` addresses by their embedded IPv4 address. It does not block `fec0::/10` (deprecated site-local), `2001:db8::/32` (documentation), `2002::/16` (6to4, whose embedded IPv4 address is not unwrapped), `2001::/32` (Teredo), `64:ff9b:1::/48` (local-use NAT64) or `100::/64` (discard-only).

**Impact** Link unfurling can be pointed at addresses that are not publicly routable. None of these reach a typical internal service, so exploitability is low, but the function's contract is "public only" and a network with a permissive egress filter could route some of them. The resolver (`PublicOnly`, `:302-315`) uses the same `global()` function, so the gap is consistent rather than a bypass. This predates the refactor.

**Remediation** Add the ranges explicitly (or use `ipnet` and a small table), and add table-driven tests with one address per range. Keep the DNS resolver as the enforcement point; the pre-check in `public()` is defense in depth.

### AUD-16 — `net/x/web.rs` mixes query data with logic

**Evidence** 1,522 lines, 1,273 production. `FALLBACKS` (`:37-55`) holds 17 persisted query ids, `FEATURES_JSON` (`:57`) is a large inline JSON blob, and the GraphQL operation builders follow. The file is also where AUD-06's typo lives.

**Impact** Query ids change with X's bundle, so this file has a high churn rate. Mixing data with logic makes every id update a code diff and makes format validation easy to forget (as AUD-06 shows).

**Remediation** Move the tables to `src/net/x/web/queries.rs` or a `queries.json` included with `include_str!`. Add the format/uniqueness test from AUD-06. Keep `web.rs` for the request construction and response mapping.

## Low severity

### AUD-06 — `DeleteRetweet` has an invalid persisted query id

The second pass lowered this finding from Medium to Low, because the invalid id is only a fallback.

**Evidence** The `DeleteRetweet` entry of `FALLBACKS` (`src/net/x/web.rs:51`):

```rust
("DeleteRetweet", "Zy ZigVsNiFO6v1dEks1eWg"),
```

The id contains a space. It is one of the 17 ids in `FALLBACKS` (`:37-55`). `OpCache::seed()` (`:109-119`) loads them, and `XClient::unrepost` (`src/net/x/client.rs:338-345`) calls `web_repost(id, false)` whenever the session uses imported browser cookies (`creds.uses_web()`). A fallback id is used only when the downloaded bundle lacks the operation or the download fails: `store()` (`:132`) and `load_disk()` (`:144`) add a fallback only for a name the bundle did not supply, through `or_insert_with`.

**Impact** Un-reposting fails for X sessions backed by browser cookies whenever the session falls back to this id. The failure is a network error, not a silent no-op. No test checks the fallback table. This predates the refactor (same string in `HEAD:src/net/x/web.rs`).

**Remediation** Remove the space after verifying the id against a live bundle, and add:

```rust
#[test]
fn every_fallback_id_is_a_valid_query_id() {
    for (name, id) in FALLBACKS {
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{name}: {id}");
    }
}
```

Also assert uniqueness, since a duplicate would silently shadow an operation.

### AUD-17 — The wasm HTTP shim hides serialization failure

**Evidence** `RequestBuilder::json` (`src/net/http.rs:159-163`):

```rust
pub fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
    self.body = serde_json::to_vec(body).unwrap_or_default();
    ...
}
```

A serialization failure silently sends an empty body. `pairs_of` (`:481-497`) round-trips through `serde_json::Value` and stringifies numbers and booleans; that is acceptable for query and form pairs, and its doc comment (`:479`) already limits it to them. `MAX_RESPONSE` is 8 MB (`:45`), and each request in `exchange` starts with a 64 KB buffer (`:360`) that doubles on `usize::MAX`.

**Impact** Low. The types sent here are plain structs that do not fail to serialize, but a future `json()` call with a map containing non-string keys would produce an empty request instead of an error.

**Remediation** Make `json()` store a `Result` and surface it in `send()`, or use `expect("request bodies are serializable")` with a comment.

### AUD-18 — The `web` feature is dead

**Evidence** `Cargo.toml` declares `web = []`, but `grep -rn 'feature = "web"' src` returns nothing. The browser build is selected by `--no-default-features`, not by this feature. `crate-type = ["rlib", "cdylib"]` also makes every native build produce a cdylib that nothing loads.

**Impact** A reader cannot tell which feature actually gates the browser code. The extra crate type adds native link time for no benefit.

**Remediation** Remove `web`, or use it to gate the browser-only modules (`web.rs`, `net/devnull/auth_web.rs`) so the wasm build is expressed positively. Cargo cannot make `crate-type` target-specific; document why `cdylib` is unconditional if it stays.

### AUD-19 — `parse_link` computes the host twice

**Evidence** `parse_link` (`src/net/mod.rs:579-580`):

```rust
let host = url.host_str().unwrap_or("").to_ascii_lowercase();
let server = url.host_str().unwrap_or("").to_ascii_lowercase();
```

`host` is then normalized (`www.`/`mobile.` stripped), while `server` is used raw for Mastodon links (`:617`, `:621`).

**Impact** Trivial today. If normalization is ever extended (IDN, trailing dot, port), applying it to one binding and not the other produces inconsistent links.

**Remediation** Compute the raw host once, then derive `host` (normalized) and `server` (raw) from it, with a comment explaining why they differ.

### AUD-20 — A partial merged load loses the post count

**Evidence** `merge()` (`src/state.rs:2571-2575`) sets `self.status = format!("{n} posts")` and then calls `self.fail(first)`, which overwrites the status with the error.

**Impact** When one of several networks fails, the user sees only the error, even though `n` posts did load and are visible. The feed notice still lists the failures, so no information is lost, but the status line is less useful than it could be.

**Remediation** Set the status once, after `fail`, to `"{n} posts · {k} network(s) failed"`, or have `fail` append rather than replace when a count is present.

### AUD-21 — `rows_for` is copied between preview modules

**Evidence** `rows_for` in `src/preview.rs:238-254` is a copy of `rows_for` in `src/preview_native.rs:682-698`. The two bodies are byte-identical and compile in mutually exclusive builds, because the `preview.rs` copy sits behind `#[cfg(not(feature = "native"))]`. The comment at `preview.rs:235-236` says it is "kept here as well as in the native module so both builds share one formula", and that the native file keeps its own copy "because its tests call it as a sibling". This is new in the current diff.

**Impact** The formula (image rows from terminal cell ratio) must change in two places, and only `preview_native.rs` has the three `rows_for` tests (`:889-907`).

**Remediation** Move `rows_for` into `preview.rs` as a plain function (it has no native dependencies) and have `preview_native.rs` call it. The comment can then be deleted.

### AUD-22 — `COMPLETION_REPORT_1.md` is stale

**Evidence** The tracked report says the wasm target was not started and the suite has 671 tests. The tree now has full wasm support, `src/web.rs`, `website/.vitepress/theme/wasm-reader.ts`, and 761 tests.

**Impact** Contributors and reviewers get a false picture of the project state. It is the first document a new contributor may open.

**Remediation** Delete it, or retitle it with a date and commit range and move it under `docs/` with an explicit "historical" note. If it stays, add it to `docs/INDEX.md`.

### AUD-23 — The browser worker's `sentPictures` grows unbounded

**Evidence** `website/.vitepress/theme/reader.worker.ts:51` declares `sentPictures` as a `Set<string>`; it is cleared only in `open()` (`:291`). `pictures()` adds every `http(s)` URL before the image has loaded (`:140-147`). The worker only sends picture URLs; the page's `PictureLayer` loads the images, so the first pass's claim that a failed image is never retried does not hold. `PictureLayer.src` (`website/.vitepress/theme/wasm-reader.ts:990`), a `Map<string, string>`, is unbounded as well.

**Impact** A long-lived session that scrolls many feeds accumulates one string per unique picture URL in the worker, and the page keeps a `src` entry per picture. Both are small compared with the image data, but they are unbounded and invisible.

**Remediation** Cap the set (an LRU of a few hundred URLs) or clear it on feed change. `PictureLayer.src` needs the same bound.

## Security findings (second pass)

The second pass calibrated each security finding by its trust boundary, the principal who can act on it and the result. Only items confirmed in the source carry a severity. AUD-27 and AUD-28 are hardening advice, not vulnerabilities.

### AUD-24 — Post links reach the OS opener without a scheme allow-list (Medium)

**Evidence** A Bluesky facet whose feature type ends in `#link` supplies a `uri` to `facet_links` (`src/net/bluesky/map.rs:146-173`), and a Mastodon status supplies an `href` to `to_text` (`src/net/mastodon/html.rs:38-51`). Both are stored unchanged as `Link.expanded`. `Cmd::Open` returns the first link's `expanded` value (`src/state.rs:1562-1576`), and `apply` (`src/tui.rs:4439`), `activate` (`src/tui.rs:4736`) and `plain::dispatch` (`src/plain.rs:326`) pass a link to `util::open_browser` (`src/util.rs:506-528`). The gallery's "open original" key takes the same path: `gallery_key` returns `GalleryStep::Open` with `picture.original` (`src/tui.rs:525`), which `Gallery::new` sets from the post's media URL (`src/tui.rs:475`), and the `GalleryStep::Open` arm of `handle_key` passes it to `util::open_browser` (`src/tui.rs:2665`). It belongs to the same remote-controlled class. The other `open_browser` calls in `src/tui.rs`, in `SignInForm::submit` (`:3806`, `:3811`, `:3818`) and in `devnull_sign_in` (`:3887`), open URLs that the sign-in flows build locally, so they are not part of this finding. `util::open_browser` hands the string to `open` on macOS, to `xdg-open` on other Unix systems and to `cmd /C start "" <url>` on Windows, and no step on the way checks the scheme.

**Impact** A remote author can make "open link" launch any registered URL handler or a `file://` target. On Windows, which is a planned target, `cmd.exe` also parses `&`, `|` and `^` inside the URL. What that allows beyond truncation needs validation on Windows; at minimum, every legitimate URL that contains `&` is cut off at the first `&`.

**Remediation** Run an http/https allow-list inside `util::open_browser`, before any opener starts, by reusing `hyperlink::safe` (`src/hyperlink.rs:67`), which already accepts only `http` and `https` URLs made of printable ASCII. On Windows, replace `cmd /C start` with `ShellExecuteW`, for example through the [`opener`](https://github.com/Seeker14491/opener) crate, whose Windows backend calls `ShellExecuteW`. `opener` turns `file:` URLs into paths and opens them, which is why the allow-list has to run first. Add a Windows CI job, and add regression tests for a `file:` URL, a custom scheme, a URL that contains `&`, and the gallery path, whose remote `picture.original` the same check inside `open_browser` covers.

### AUD-25 — dev/null accepts a callback without `state` (Low)

**Evidence** `parse_callback` (`src/net/devnull/flow.rs:59`) rejects a callback only when it carries a state that differs from the expected one: `got_state.as_deref().is_some_and(|s| s != state)`. A callback with no `state` passes the check.

**Impact** PKCE limits the impact: a code obtained by an attacker will not exchange with the user's verifier, provided dev/null enforces PKCE, which needs validation.

**Remediation** Require `state` and compare it with the expected value.

### AUD-26 — Exit status is derived from message text (Low)

**Evidence** `cli::main` (`src/cli.rs:447-459`) returns silently with exit 0 when the error text contains `"Broken pipe"`, and exits 2 when it starts with `util::NEEDS_PERSON`. `NetError::Http` carries the server's own error text, which `error_text` extracts from the response body (`src/net/http.rs:511-536`).

String sentinels steer control flow in three places: `"Broken pipe"`, `NEEDS_PERSON` and `"cancelled"`. `"cancelled"` is produced in `read_prompt` (`src/util.rs:404-405`), `prompt_rail_menu` (`src/screens.rs:687`) and the native dev/null `authorize` (`src/net/devnull/browser.rs:52`), and matched in `login_attempts` (`src/tui.rs:4183`) and `x_login` (`src/cli.rs:684`). Across production code, about 175 functions return `Result<_, String>`, against about 95 that return `NetError` and about 25 that return `ApiError`. `Provider` returns `NetError`, while every provider `auth.rs` returns `String`, and about 120 `map_err` calls turn typed errors into strings.

**Impact** A server can steer the exit code that agents rely on: an error body whose text contains "Broken pipe" turns a failure into a silent exit 0, and one that starts with the `NEEDS_PERSON` sentence turns it into exit 2. Each sentinel is a string comparison that a reworded message breaks without a compiler error.

**Remediation** Introduce a typed `CliError { kind: Quiet | Failed | NeedsPerson | BrokenPipe, message }` and decide the kind where the error is created. The typed `CliError` covers all three sentinels.

### AUD-27 — The browser reader keeps every credential in `localStorage` on the docs origin (Hardening)

**Evidence** The live reader stores the whole config text, tokens included, under `socli.config` in `localStorage` (`CONFIG_KEY`, `storedConfig` and `persistConfig` in `website/.vitepress/theme/wasm-reader.ts:40`, `:198` and `:206`; on the Rust side, `load_browser`, `store_browser` and `read_browser` in `src/config.rs:776-827`). The HTML pages carry no Content Security Policy; only the worker's fallback and 404 responses send one, through its `SECURITY` headers (`workers/site/index.js:18-24`). The `head` entry at `website/.vitepress/config.ts:73` injects `themeScript` as an inline script into every page. No XSS was found.

**Impact** The concern is blast radius. Any script that runs on `socli.sh` can read the X, Bluesky, Mastodon and Threads tokens and the dev/null device key.

**Remediation** Serve the live reader from its own origin with a strict CSP (`script-src 'self' 'wasm-unsafe-eval'`, `connect-src` limited to the API hosts, and no inline script, or a hash for the one that remains), enable Trusted Types, and keep the docs on a separate origin.

### AUD-28 — Secrets at rest and in memory (Hardening)

**Evidence** Tokens and the X `auth_token` cookie, which is a full session that bypasses 2FA, live in plaintext TOML. The file is written with mode `0600` and atomically, which is correct. `Config`, `XConfig`, `BlueskyConfig`, `DevnullConfig`, `ThreadsConfig` and `MastodonConfig` derive `Debug` (`src/config.rs:25`, `:81`, `:106`, `:116`, `:129`, `:144`), and so does the X `Credentials` struct (`src/net/x/auth.rs:18-33`).

**Impact** Anyone who can read `config.toml` holds every session in it. A derived `Debug` prints every field, so formatting one of these structs with `{:?}` prints its tokens in full.

**Remediation** Wrap secrets in a `Secret` newtype with a redacted `Debug` (or use the `secrecy` crate), and store them in the OS keychain through the `keyring` crate with the file as a fallback, as [`gh` does](https://cli.github.com/manual/gh_auth_login).

### AUD-29 — CI supply chain (Low, hardening)

**Evidence** The top-level `permissions` block of `.github/workflows/pages.yml` (`:16-19`) grants `pages: write` and `id-token: write` at workflow level, so the build job, which runs `bun install` and third-party build scripts, holds them. Actions are pinned by tag (`actions/checkout@v4`, `oven-sh/setup-bun@v2`), and the "Setup Rust" step uses `dtolnay/rust-toolchain@master` (`:38`). The repository has no Dependabot configuration, no `deny.toml` and no `SECURITY.md`.

**Impact** Code that runs in the build job, including an action behind a moving tag or branch, runs with the permissions that deploy the site and request OIDC tokens. Vulnerable or duplicated dependencies are not reported.

**Remediation** Move the Pages permissions to the deploy job, pin actions by commit SHA, add Dependabot for cargo, GitHub Actions and bun, and run `cargo deny check` in CI (cargo-deny is installed locally).

### AUD-30 — The native API client has no body cap (Low)

**Evidence** The native `client()` (`src/net/http.rs:24-31`) sets a user agent, a 20-second timeout and rustls, and keeps reqwest's default redirect policy. Nothing limits the size of a response body, while the wasm half caps responses at 8 MiB (`MAX_RESPONSE`, `:45`).

**Impact** A hostile or broken Mastodon server, which the user chooses, can stream an unbounded body into memory.

**Remediation** Apply the same 8 MiB cap to native responses.

### AUD-31 — Unsafe hygiene (Low)

**Evidence** `src/web.rs` has 18 `#[no_mangle]` exports. 17 of them are `unsafe`, and each has a `# Safety` section; the eighteenth, `socli_alloc` (`src/web.rs:703`), is a safe `pub extern "C" fn`. The 19 `unsafe fn` bodies in `src/web.rs`, the 17 exports plus `text_at` and `write_text`, have no inner `unsafe {}` block. Nine `unsafe {}` blocks have no `// SAFETY:` comment: in `store_browser` (`src/config.rs:794`), `read_browser` (`:806`), the wasm `exchange` (`src/net/http.rs:363`), `fill_random` (`src/net/pkce.rs:30`), `page_url_from_host` (`src/util.rs:495`), `unix_millis` (`src/clock.rs:57`), `utc_now` (`:89`), `mono_millis` (`:104`) and `write_len` (`src/web.rs:722`). No `unsafe_code` lint is set anywhere, and edition 2021 leaves `unsafe_op_in_unsafe_fn` off. The wasm `decode` (`src/net/http.rs:419-421`) adds a `u32` body length to the read position without a check.

**Impact** A reviewer cannot see the invariant each block relies on, and nothing stops new `unsafe` from appearing outside the FFI code. On wasm32 `usize` is 32 bits, so the unchecked addition in `decode` (`:421`) can overflow before the bounds check runs.

**Remediation** Gather the wasm imports and exports into one FFI module (Phase 2b), deny `unsafe_code` everywhere else (rustls forbids it crate-wide), write a `SAFETY:` comment on each block, and use `checked_add` for the length.

### AUD-32 — Smaller items (Low)

**Evidence**

- The OSC 52 payload built by `osc52` has no size cap (`src/clip.rs:133-140`).
- `util::clean` keeps U+200B, U+FEFF and other format characters, because `dropped` removes only control and bidirectional-override characters (`src/util.rs:173-176`).
- The image cache has no bound on disk, and `materialize` writes its files with default permissions (`src/media.rs:176-187`).
- `workers/threads` processes requests from any `Origin` in `handle` (`workers/threads/index.js:33`), because `ORIGINS` (`:19-23`) only shapes the CORS headers in `cors` (`:173`), and it has no rate limit.
- The development relay (`website/.vitepress/relay.ts`) checks no `Origin`. It runs only in development and preview.

**Impact** Each item is a missing limit or check on data that arrives from a network or from another program on the machine.

**Remediation** Add the missing limit or check in each place: a payload cap for OSC 52, format characters in `util::clean`, a size bound and private permissions for the image cache, an `Origin` check and a rate limit in `workers/threads`, and an `Origin` check in the relay.

### AUD-33 — The X web-session risk is undocumented (Docs)

**Evidence** The browser-session path sends x.com's public web bearer (`WEB_BEARER`, `src/net/x/web.rs:29`), a fixed Chrome 131 user agent (`CHROME_UA`, `:30`) and a computed `x-client-transaction-id` (`src/net/x/transaction.rs`). The user docs do not say that this imitates the web client, or what risk that carries for the account.

**Impact** A person who signs in with browser cookies cannot weigh that risk before choosing the method.

**Remediation** State in the X sign-in documentation that the browser-session path presents itself as the x.com web client, and describe the account risk.

### Considered and not reported

These were examined and are not findings. They are listed so that a later review does not raise them again.

- Non-constant-time `state` comparisons. The flows are local and each state is used once.
- `sqlite3` and `security` are resolved through `PATH` (`cookie_rows`, `src/net/x/cookies.rs:352`; `safe_storage_password`, `:407`). Replacing them requires code execution at the user's level beforehand.
- The embedded web bearer. It is public and ships in x.com itself.
- Inbound Bluesky facet offsets. `body.get(start..end)` cannot panic.
- Facet slicing on the draft side. The offsets come from `char_indices`.

## Correctness, structure and process (second pass)

### AUD-34 — Search plans networks that are not signed in (Medium)

**Evidence** In `plan_page`, the `Scope::All` search arm keeps every network with `info.caps.search && self.on.contains(&info.net)` (`src/state.rs:2391`) and omits `info.signed_in`, which the feed arm (`:2320`) and the mentions arm (`:2368`) both check. The second pass found this while correcting AUD-12.

**Impact** A search across all networks plans a request for every enabled network that can search, including networks with no session.

**Remediation** Add the `signed_in` condition to the search arm, with a test that a network that is on and can search but is not signed in is left out of the plan. AUD-12's shared filter then keeps the arms from drifting again.

### AUD-35 — X loopback skips the code check, and authorize URLs are built twice (Low)

**Evidence** The native X loopback in `oauth` (`src/net/x/auth.rs:364-374`) compares the state and passes the callback code to `exchange` without the validation that `finish_page` applies (`:710-713`: trim, then reject an empty code, one longer than 512 bytes, whitespace and `&`). X and Mastodon each build their authorize URL twice: X in `oauth` (`src/net/x/auth.rs:339-347`) and `begin_page` (`:679-688`), Mastodon in `authorize_url` (`src/net/mastodon/auth.rs:78-89`) and `page_authorize_url` (`:92-110`). Mastodon's `authorize_url` calls `expect` (`src/net/mastodon/auth.rs:80`), and it is a `pub fn` that accepts any `&str`, so the invariant its message states ("normalize_server only accepts hosts that parse as a URL") is not carried by the type.

**Impact** The two X paths accept different codes, a fix to one authorize-URL builder can miss its twin, and a caller that passes a string `normalize_server` did not produce can make `authorize_url` panic.

**Remediation** Apply the shared code check from AUD-03's `net/oauth.rs` on both X paths, build each network's authorize URL in one function, and make `authorize_url` return a `Result` as `page_authorize_url` already does, or take a type that only `normalize_server` can build.

### AUD-36 — Dependencies and toolchain (Medium)

**Evidence** The native binary links two crossterm versions, 0.28.1 through ratatui 0.29 and 0.29.0 through viuer 0.11, and each copy keeps its own raw-mode state. `cargo tree -d` lists 10 crate names with two versions: base64, crossterm, getrandom, hashbrown, miniz_oxide, rustix, syn, thiserror, thiserror-impl and unicode-width. Several dependencies are behind their current releases: ratatui 0.30.2 (which unifies crossterm; the custom `Backend` in `src/grid.rs` would need an `Error` type and `clear_region`), reqwest 0.13, toml 1.x, rand 0.10 and ratatui-image. `rust-version = "1.89"` (`Cargo.toml:12`) is never tested: `rust-toolchain.toml` pins 1.99.1 and no job builds with the MSRV. Edition 2024 is available from Rust 1.85, and ripgrep has moved to it.

**Impact** Raw mode set through one crossterm copy is not recorded in the other, and the binary carries both. A change that needs a newer Rust than 1.89 merges without anyone noticing that the declared MSRV no longer holds.

**Remediation** Add an MSRV 1.89 job and `cargo deny` with `multiple-versions = "warn"` to CI (Wave 1 in [REFACTORING.md](REFACTORING.md)). Then move to ratatui 0.30 to unify crossterm, followed by reqwest 0.13, toml 1, rand 0.10 and edition 2024 (Phase 6).

### AUD-37 — No lint policy (Low)

**Evidence** The crate has no `[lints]` table, no `clippy.toml` and no crate-level lint attributes. A production-only clippy sample (see [Method](#method)) reports 102 string-indexing sites, 159 "indexing may panic" sites, 95 wildcard enum arms, 185 casting warnings, 125 public functions without an `# Errors` section and 22 functions over 100 lines.

**Impact** Nothing stops a new panicking index, a lossy cast or a wildcard arm from merging, so these counts can only grow.

**Remediation** Adopt a small `[lints]` set in which each lint has a stated reason, and hold the counts with the ratchets in [REFACTORING.md](REFACTORING.md), instead of enabling `clippy::pedantic` wholesale.

### AUD-38 — The refactor base is uncommitted (Process)

**Evidence** At the time of writing, 78 tracked files differ from HEAD by +10,260/−5,004 lines, and 27 untracked entries (37 files) sit beside them on `main`, among them `src/lib.rs`, `src/cli.rs`, `src/web.rs` and this document.

**Impact** Nothing can be bisected, and "new in this diff" has no commit to point at.

**Remediation** Commit the tree in logical commits on a branch before any refactoring starts. This is the first step of the roadmap, and it needs Massimo's go.

### AUD-39 — The docs index links ignored files (Docs)

**Evidence** `docs/INDEX.md` links `IMPROVEMENTS.md`, `design/` and `site/`, which `.gitignore` excludes (`.gitignore:6-8`), so a fresh clone shows broken links. `COMPLETION_REPORT_1.md` (AUD-22), `WEB_CLI.md` and `ROADMAP.md` crowd the repository root.

Several other documents have drifted as well. The module map in `docs/CONVENTIONS.md` (`:34-85`) has no row for `kitty_place.rs`, `onboard.rs`, `palettes.rs`, `net/caps.rs`, `devnull/auth_web.rs` or `devnull/flow.rs`, which breaks its own one-row-per-file rule. `WEB_CLI.md:7` still says nothing in the repository compiles to WebAssembly. `COMPLETION_REPORT_1.md` counts 671 tests, while the tree has 761. `workers/site/.wrangler/` and `workers/threads/.wrangler/` both exist, and neither is ignored.

**Impact** A reader who clones the repository follows links that lead nowhere, and contributors who trust the module map or the root documents get a wrong picture of the code.

**Remediation** Stop linking ignored files from the index, or track them, and reorganize the root as section 6 of [REFACTORING.md](REFACTORING.md) describes. Add the missing module-map rows, correct or archive the stale documents, ignore `.wrangler/`, and add tidy tests that check the module map and the index against the files.

### AUD-40 — The Pages workflow fails its own wasm-graph guard (Medium)

**Evidence** In the working tree, the "Check the wasm graph" step of `.github/workflows/pages.yml` (`:47`) fails the build when `ssh-key` appears in the wasm graph, while the `ssh-key` entry in `Cargo.toml` (`:60`) is a non-optional dependency and the toolchain section of `docs/CONVENTIONS.md` (`:22`) says the page signs dev/null devices with it (the forbidden list there omits it). `cargo tree --target wasm32-unknown-unknown --no-default-features --prefix none | grep -E '^ssh-key '` prints `ssh-key v0.6.7`, so the first push of this tree stops the deploy at "Check the wasm graph". HEAD has no such guard.

**Impact** The next push to `main` that touches the workflow's paths fails the Pages deploy.

**Remediation** Drop `ssh-key` from the regex so CI matches CONVENTIONS. Later, move the forbidden list into one checked place, an xtask or a tidy test over `cargo metadata`.

### AUD-41 — The network layer reaches into the UI, and one call welds 19 modules into a cycle (Medium)

**Evidence** All five provider `auth.rs` files import `crate::screens` (`present_login`, `print_signed_in`, `login_aside`, `prompt_rail`, `login_steps`). `src/net/mod.rs` imports `theme::Kind` (`:22`) for `Network::accent` (`:73`). `util` imports `watchdog`, and `watchdog` calls into `tui`. The module graph has two strongly connected components. A is {config, media, model, net, palettes, preview, theme}. B holds 19 modules: chrome, drafts, jsonview, minimal, the five providers, onboard, plain, render, screens, send, settings, state, tui, util and watchdog. B hangs on one edge, the `crate::tui::leave(..)` call in `watch_terminal` (`src/watchdog.rs:141`); without that edge, B shrinks to {minimal, render}.

**Impact** No module in B can become its own crate or be tested without the UI.

**Remediation** Cut the edges in this order:

1. `watchdog` calls a restore hook that `tui` registers (a `OnceLock<fn()>`).
2. `Network::accent` moves to `theme`.
3. Providers return their login steps as data (`LoginStep` values), and `screens` presents them.
4. `Network`, `FeedKind` and the ids move to a leaf module, so config↔net and model↔net stop being cycles.
5. `minimal` and `render`, which import each other (`src/minimal.rs:11`, `src/render.rs:12`), share a lower helper module.

### AUD-42 — A page fetch can freeze the key loop (Medium)

**Evidence** `start_page` spawns a thread that holds `Arc<Mutex<Clients>>` for the whole `state::fetch_page` call (`src/tui.rs:1674-1680`). Every key that is not a navigation key takes the same lock in the key loop of `run` (`src/tui.rs:1279`) before `handle_key_busy` starts. `start_preview` does the same (`:1708-1714`).

**Impact** While a page loads, the next such key blocks the loop until the page arrives, and the busy footer does not show.

**Remediation** The minimal fix uses `try_lock` on the main thread: when a job holds the lock, show the loading footer and keep the key pending until the job ends. The lasting fix (Phase 6) gives `Clients` to one network thread that serves a request channel. That is the Tokio tutorial's pattern of a manager task that owns the client, built on `std::thread` and `mpsc` with no async runtime.

### AUD-43 — Two cfg axes disagree, and one build combination does not compile (Low)

**Evidence** `cargo check --lib --no-default-features` on the host fails with 13 errors. `src/net/http.rs:14` gates the reqwest module `imp` on `not(target_arch = "wasm32")`, but `reqwest` and `rand` are optional dependencies behind the `native` feature; `fill_random` (`src/net/pkce.rs:25`) and `generate` and `load_or_create` (`src/net/devnull/sign.rs:60` and `:124`) have the same mismatch. Across `src/`, tests included, the literal string `cfg(feature = "native")` appears 130 times, `cfg(not(feature = "native"))` 50 times, `cfg(target_arch = "wasm32")` 24 times and `cfg(not(target_arch = "wasm32"))` 22 times, and neighbouring functions use different axes (`sign_in` at `src/tui.rs:4107` against `login_net` at `:4194`). The `web = []` feature is never referenced (AUD-18).

**Impact** A host build without `native` does not compile, so code behind `not(feature = "native")` is checked only on wasm32, and a reader cannot tell which axis governs a given item.

**Remediation** Use the feature for capability and `target_arch` only inside the http, clock and storage shims, and run `cargo hack check --each-feature` in CI, as Tokio does. Phase 5 removes most cfgs.

### AUD-44 — The library exports its whole interior (Low)

**Evidence** `src/lib.rs` declares 39 `pub mod`. Production code has about 700 `pub fn`, about 100 `pub struct` and about 45 `pub enum`. The only outside caller is `main` (`src/main.rs:5`, `socli::cli::main()`), and the wasm exports are `#[no_mangle]`. About half of the public items are documented, and there are no doctests.

**Impact** Because everything is public, `dead_code` never fires, and the API is accidental.

**Remediation** Make the modules `pub(crate)` and re-export `cli::main`. `unreachable_pub` and `dead_code` then show what is unused. Confirm with the wasm build that the exports survive.

### AUD-45 — `State` and `Ui` gather every concern (Low)

**Evidence** `State` has 57 fields (`src/state.rs:122-210`) and one 2,533-line impl with 109 methods (`:259-2791`). `Ui` (`src/tui.rs:986-1023`) has 21 fields, 15 of them interior-mutable (6 `Cell` and 9 `RefCell`).

**Impact** Every concern of the reader shares one struct and one impl, so unrelated changes meet in the same code, and the interior mutability in `Ui` hides which code writes which field.

**Remediation** Split owned sub-structs per concern (navigation, appearance, paging, status, settings view, compose) and move their methods with them, splitting borrows by field instead of relying on `RefCell`.

Data mixed with logic is not a finding, but it is input to Phase 4 alongside these sub-structs. `palettes.rs` `NAMED` takes 954 of the file's 970 lines, about 530 of the 807 production lines of `help.rs` are tables, `kitty_place.rs` `DIACRITICS` is 299 lines, `demo.rs` `world()` is a 376-line function, and `src/net/x/web.rs` keeps `FALLBACKS`, `FEATURES_JSON`, the bearer and the user agent next to its parsers.

### AUD-46 — Allocation on every frame (Low, performance, unmeasured)

**Evidence** `posts_for` clones each shown `Post` (`src/state.rs:1278`), `last = done.buffer.clone()` in `run` copies the ratatui buffer for every drawn frame (`src/tui.rs:1189`), and `state.theme.clone()` in `handle_key_busy` runs for every key that is not a navigation key (`src/tui.rs:1771`). No benchmark exists.

**Impact** The cost is unmeasured, so whether these copies matter is unknown.

**Remediation** Measure first, with the existing `Trace` plus a benchmark that draws the demo world, then borrow posts and swap buffers if the numbers justify it.

### AUD-47 — The test portfolio has one shape (Low)

**Evidence** The suite has 761 example tests and 20 JSON fixture files, loaded by 24 `include_str!` calls, and no property, snapshot, fuzz, benchmark or doc tests. These modules have no tests: `net/caps.rs` (292 lines of capability tables that decide what the UI offers), `clock`, `intent`, `jsonview`, `palettes`, `demo`, `devnull/auth_web` and `devnull/browser`. The eight `node:test` worker tests (four in `workers/site/pages.test.mjs`, four in `workers/threads/deletion.test.mjs`) have no `scripts` entry, and CI does not run them. `website/` has no `tsconfig.json`, linter or tests. The test helper `temp()` is written three times (`src/cli.rs:1948`, `src/drafts.rs:305`, `src/config.rs:913`).

**Impact** Example tests check the cases someone thought of. Parsers of remote input, the frames the reader draws and the capability tables have no test that explores beyond those cases or pins their output.

**Remediation** Build the safety net of Phase 1 in [REFACTORING.md](REFACTORING.md): frame snapshots of the demo world, property tests and fuzz targets for the parsers of remote input.

## Other observations (not ranked)

These are worth knowing but did not justify a separate severity:

- `src/cli.rs` `dispatch` (146 lines, `:1340`) and `src/command.rs` `parse` (157 lines, `:125`) are large matches. They are readable today; a table-driven dispatch would pay off only when the command surface grows again.
- `src/render.rs` (2,759 production lines, 113 functions) is the next split candidate after `tui.rs` and `screens.rs`. Natural seams are post bodies, profile, thread and cards; `rich_link_card` (138 lines) and `profile` (130) are the first functions to break up.
- `src/demo.rs` `world` (376 lines, `:104`) builds the demo fixtures inline. It is compile-time checked, which is a real benefit, but a 376-line function is hard to review; a small builder API would keep the safety and improve the diff.
- `tests/cli.rs` is 1,513 lines in one file. It is well organized and every test asserts something, but it will become a merge hotspot. Splitting it into more `tests/*.rs` files would add integration-test binaries, against the one-binary practice in matklad's [Delete Cargo Integration Tests](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html); when it next grows, split it into modules by command group (`feed`, `write`, `config`, `auth`) under one `tests/it/main.rs`.
- `src/net/caps.rs` repeats the same table shape five times. That is data, not logic duplication; it is the right trade-off and should stay.
- `src/net/x/web.rs` `FEATURES_JSON` is a large inline JSON literal (`:57`). It is stable compared with the query ids, so moving it is optional; see AUD-16 for the ids.

## Regression assessment of the current diff

The uncommitted work is a large but coherent refactor: `main.rs` becomes an 11-line shim over `cli.rs`; the reader moves into `lib.rs`; `web.rs`, `preview_native.rs`, `key.rs`, `grid.rs`, `net/caps.rs`, `net/http.rs` (wasm half), page OAuth, the worker relay and the website docs are new.

What the evidence supports:

- **No functional regression found in covered native paths.** The 739 tests pass on 2026-10-23, clippy and fmt are clean, and the split preserved the public CLI surface (the 49 integration tests exercise it).
- **The refactor introduced duplication, not behavior change**: page OAuth copies (AUD-03), `rows_for` (AUD-21), the wasm warnings (AUD-05), and the dead `web` feature (AUD-18).
- **The uncovered surface is the browser path**: the worker relay, page OAuth, the loopback callback and `reader.ts`. The wasm build is not linted, and `reader.ts` has no tests. That is where a future regression is most likely to land unnoticed.
- **Two user-visible bugs predate the refactor** and should not be blamed on it: terminal restore (AUD-01) and `DeleteRetweet` (AUD-06). The same is true for the draft inconsistency (AUD-10), the SSRF ranges (AUD-15) and the merge status (AUD-20).

## What is working well

- **The provider boundary is real.** `Provider`, `Caps`, `Draft`, `FeedKind` and `NetError` (`src/net/mod.rs`) keep five networks behind one interface without leaking transport details into the reader. The capability tables (`src/net/caps.rs`) are data, not branching logic.
- **Errors are typed where it matters.** `NetError` distinguishes sign-in, unsupported, HTTP, rate limit, parse and other; the app layer uses `Result<_, String>` deliberately and consistently. The second pass qualifies this: AUD-26 shows three places where string errors now decide control flow, the exit code among them.
- **Production code is panic-free.** Four `unwrap`/`expect` calls, each guarded by an invariant, although AUD-35 shows that no type enforces the invariant behind `mastodon/auth.rs:80`. Tests carry the rest.
- **The test suite is substantial and honest.** 761 tests on 2026-10-24 (739 at the first pass), 29% of the Rust, isolated config directories, no `#[ignore]`, no assertion-free tests. The pagination, merge, OAuth-state and key-codec tests are the right tests.
- **SSRF defense is layered.** `public()` plus the `PublicOnly` DNS resolver (`src/unfurl.rs:302`) means a rebinding answer cannot reach a private address even if the pre-check is bypassed. The range gaps in AUD-15 are a completeness issue, not a hole in the architecture.
- **Data and logic are separated where it was deliberate.** `src/palettes.rs` (970 lines of palette data) and `src/net/caps.rs` show the pattern the config and settings code should follow.
- **`src/net/devnull/flow.rs` is the right extraction.** Shared authorize/callback parsing lives in `flow.rs`, while `auth.rs` and `auth_web.rs` hold the native and browser front ends. That is the model for AUD-03.
- **Documentation is unusually thorough.** `docs/CONVENTIONS.md`, `docs/DESIGN.md` and the per-network pages describe the intended architecture; the audit could judge against them rather than guess.

## Suggested order of work

[REFACTORING.md](REFACTORING.md) now holds the order of work. Phase 0 commits the tree and adds CI, Phase 1 builds a safety net of snapshots, property tests and fuzz targets, Phase 2 makes small test-first fixes, Phase 2b cuts the module cycles, Phase 3 removes duplication behind tests, Phase 4 splits the large files, Phase 5 moves to a workspace, and Phase 6 handles types, dependencies, storage and concurrency. Its Wave 1 lists the steps that can run as soon as Massimo says go. The table maps each finding to the phase that handles it; a finding that two phases share names both, and "Project organization" refers to the last section of that document, which no phase owns.

| ID | Phase |
| --- | --- |
| AUD-01 | Phase 2 (Wave 1) |
| AUD-02 | Phase 4 |
| AUD-03 | Phase 3 |
| AUD-04 | Phase 1 (round-trip test), Phase 3 |
| AUD-05 | Phase 0 (Wave 1), Phase 5 |
| AUD-06 | Phase 2 |
| AUD-07 | Phase 3 |
| AUD-08 | Phase 3 |
| AUD-09 | Phase 4 |
| AUD-10 | Phase 2 |
| AUD-11 | Phase 3 |
| AUD-12 | Phase 3 |
| AUD-13 | Phase 3 |
| AUD-14 | Phase 3 |
| AUD-15 | Phase 2 |
| AUD-16 | Phase 4 |
| AUD-17 | Phase 2 |
| AUD-18 | Phase 2 (the `web` feature, with AUD-43), Phase 5 (`cdylib`) |
| AUD-19 | Phase 3 |
| AUD-20 | Phase 2 |
| AUD-21 | Phase 3 |
| AUD-22 | Project organization |
| AUD-23 | Phase 2 |
| AUD-24 | Phase 2 (Wave 1) |
| AUD-25 | Phase 2 |
| AUD-26 | Phase 2 |
| AUD-27 | Phase 6 |
| AUD-28 | Phase 6 |
| AUD-29 | Phase 0 (Wave 1) |
| AUD-30 | Phase 2 |
| AUD-31 | Phase 0 (Wave 1: the 9 `SAFETY:` comments, with `undocumented_unsafe_blocks`), Phase 2 (`checked_add`), Phase 2b (one FFI module, then `unsafe_code` denied), Phase 5 (`forbid(unsafe_code)` in every crate but `web`), Phase 6 (`unsafe_op_in_unsafe_fn` with edition 2024) |
| AUD-32 | Phase 2 |
| AUD-33 | Phase 0 (Wave 1) |
| AUD-34 | Phase 2 (Wave 1) |
| AUD-35 | Phase 2, Phase 3 |
| AUD-36 | Phase 0 (Wave 1), Phase 6 |
| AUD-37 | Phase 0 (Wave 1), with its thresholds in the ratchets |
| AUD-38 | Phase 0 (Wave 1) |
| AUD-39 | Phase 0 (Wave 1: ignore `.wrangler/`), Project organization |
| AUD-40 | Phase 0 (Wave 1) |
| AUD-41 | Phase 2b |
| AUD-42 | Phase 2 (Wave 1), Phase 6 |
| AUD-43 | Phase 2, Phase 5 |
| AUD-44 | Phase 2b |
| AUD-45 | Phase 4 |
| AUD-46 | Phase 1 (benchmark), Phase 6 (change if the measurement justifies it) |
| AUD-47 | Phase 1 |

## Method

- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- `cargo check --target wasm32-unknown-unknown --no-default-features`, with warning counts grouped by file.
- Production-only function-length scan (brace counting, test modules excluded) over `src/**/*.rs`.
- Production-only duplicate-window scan: 10-line normalized windows, cross-file matches only, with `#[cfg(test)]` regions excluded.
- Manual reads of `tui.rs`, `state.rs`, `render.rs`, `screens.rs`, `cli.rs`, `config.rs`, `settings.rs`, `unfurl.rs`, `net/mod.rs`, `net/http.rs`, `net/x/{auth,web,client}.rs`, `net/{mastodon,threads,bluesky}/*`, `preview*.rs`, `web.rs`, `send.rs`, `draft`/`command`/`key` modules, `tests/cli.rs`, the wasm worker/reader, CI and the docs.
- `git show HEAD:` comparisons to separate pre-existing defects from refactor-introduced ones.

### Second pass

The second pass ran read-only on 2026-10-24. Every cargo command used a scratch `CARGO_TARGET_DIR` with `--offline --locked`, so nothing in the repository changed, and every `file:line` in this document was found again by grepping its symbol or quoted text in that day's tree. Line numbers refer to the working tree on 2026-10-24 and will drift as the code changes; the symbol named next to each line is the stable anchor. These commands reproduce the numbers.

The wasm check prints "generated 71 warnings":

```sh
cargo check --lib --no-default-features --target wasm32-unknown-unknown
```

The same check on the host, without the `native` feature, fails with 13 errors (AUD-43):

```sh
cargo check --lib --no-default-features
```

The production clippy sample runs on the library only, on the host, with tests excluded:

```sh
cargo clippy --lib -- -A clippy::all -W clippy::unwrap_used -W clippy::expect_used -W clippy::panic -W clippy::string_slice -W clippy::indexing_slicing -W clippy::cast_possible_truncation -W clippy::cast_sign_loss -W clippy::cast_possible_wrap -W clippy::too_many_lines -W clippy::missing_errors_doc -W clippy::needless_pass_by_value -W clippy::large_enum_variant -W clippy::large_stack_arrays -W clippy::redundant_clone -W clippy::match_same_arms -W clippy::struct_excessive_bools -W clippy::fn_params_excessive_bools -W clippy::undocumented_unsafe_blocks -W clippy::multiple_unsafe_ops_per_block -W clippy::print_stdout -W clippy::dbg_macro -W clippy::todo -W clippy::wildcard_enum_match_arm
```

| Warning | Count |
| --- | --- |
| Indexing may panic | 159 |
| Missing `# Errors` section | 125 |
| String indexing | 102 |
| Wildcard enum arm | 95 |
| Casting (all three cast lints) | 185 |
| Slicing may panic | 28 |
| `needless_pass_by_value` | 32 |
| `match_same_arms` | 11 |
| Redundant clone | 9 |
| Struct with more than 3 bools | 7 |
| `too_many_lines` | 22 |
| `expect` | 4 (`build.rs` is excluded from the production count) |

The 9 `unsafe` blocks without a `SAFETY:` comment were counted from the source. All of them are gated to wasm32, so a host clippy run cannot see them.

The fixture counts in AUD-47 come from two commands, which print 20 and 24:

```sh
git ls-files 'src/**/*.json' | wc -l
grep -rn 'include_str!' src | grep -c '\.json'
```

The test count in the second pass, used in AUD-22, AUD-39 and AUD-47, comes from this command, which listed 761 tests on 2026-10-24:

```sh
cargo test --offline --locked -- --list | grep -c ': test$'
```

The Rust volume in the Baseline comes from two commands, which print 62,631 and 44,291. The second counts only the lines of each `src/` file above its test module:

```sh
cat $(find src tests -name '*.rs') | wc -l
for f in $(find src -name '*.rs'); do awk 'p ~ /^#\[cfg\(test\)\]$/ && /^(pub )?mod / {print NR-2; f=1; exit} {p=$0} END {if (!f) print NR}' "$f"; done | paste -sd+ - | bc
```

The state of the working tree in AUD-38 and in the second-pass summary comes from three commands, which printed 78 files changed with +10,260/−5,004 lines, 27 untracked entries and 37 untracked files:

```sh
git diff --shortstat HEAD
git status --porcelain | grep -c '^??'
git ls-files --others --exclude-standard | wc -l
```

The cfg counts in AUD-43 come from this loop, which prints 130, 50, 24 and 22:

```sh
for s in 'cfg(feature = "native")' 'cfg(not(feature = "native"))' 'cfg(target_arch = "wasm32")' 'cfg(not(target_arch = "wasm32"))'; do grep -rhoF "$s" src | wc -l; done
```

The dependency counts include `socli` itself. The first command gives 229 packages for the native build; adding `--target wasm32-unknown-unknown --no-default-features` to the `cargo tree` call gives 137 for wasm:

```sh
cargo tree --prefix none -e normal | sed 's/ (\*)//' | sort -u | wc -l
```

This command lists the 10 crate names that appear in two versions (base64, crossterm, getrandom, hashbrown, miniz_oxide, rustix, syn, thiserror, thiserror-impl, unicode-width):

```sh
cargo tree -d --depth 0 --prefix none -e normal
```

The wasm-graph guard of AUD-40 can be reproduced with this command, which prints `ssh-key v0.6.7`:

```sh
cargo tree --target wasm32-unknown-unknown --no-default-features --prefix none | grep -E '^ssh-key '
```

The longest function, `draw`, is 835 lines from `fn` to its closing brace, or 824 by clippy's `too_many_lines` count. Function lengths elsewhere in this document use the first count unless they say otherwise; "functions over 100 lines" uses clippy's.

The module graph, its strongly connected components and the visibility counts in AUD-44 came from a throwaway lexer script that blanked comments and strings, stripped `#[cfg(test)]` items and collected `crate::` and `super::` paths. The same script produced the error-type counts in AUD-26 (about 175 functions returning `Result<_, String>`, about 95 returning `NetError`, about 25 returning `ApiError`, and about 120 `map_err` calls) and the documentation share in AUD-44, about half. It counted production code only, with `#[cfg(test)]` items stripped, so a plain grep over `src/` counts more, because it includes test code. The script was not kept. The counts are rounded because the script was not kept, and a line-start grep gives slightly different values. The edge that holds the 19-module component together can be checked with:

```sh
grep -n 'crate::tui' src/watchdog.rs
```
