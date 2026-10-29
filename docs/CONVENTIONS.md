# Conventions

This document holds the rules for working on the SoCLI code, from the toolchain and the checks a change has to pass to the way commits and documentation are kept. Read it before you change code.

## Scope and precedence

This file and [DESIGN.md](DESIGN.md) govern the repository. This file covers code, tests, commits and documentation, and DESIGN.md covers everything a user sees. When the two overlap, DESIGN.md wins on anything visual.

## Toolchain and checks

The crate uses Rust edition 2021 and declares `rust-version = "1.89"` in `Cargo.toml`. `rust-toolchain.toml` pins the toolchain to channel `1.99.1` with the `minimal` profile and the `rustfmt` and `clippy` components, and rustup installs it on the first build.

A change is done when all four of these pass:

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

A change that touches the reader or the browser demo also passes these. The graph check must not list `crossterm`, `signal-hook`, `reqwest`, `viuer` or `ratatui-image`. The page signs a dev/null device with `ssh-key` (ed25519 only). GitHub SSH signing still needs `ssh-keygen` and stays in the terminal.

```sh
cargo check --no-default-features --target wasm32-unknown-unknown --lib
cargo tree --target wasm32-unknown-unknown --no-default-features
cargo build --release --no-default-features --target wasm32-unknown-unknown --lib
```

The page supplies `socli_millis` (`Date.now`), `socli_mono` (`performance.now`), `socli_http` (one synchronous request from the reader worker), `socli_config_read` and `socli_config_write` (the config text for this origin), `socli_random` (`crypto.getRandomValues`), and `socli_page` (this page's origin and path, with no query) when the worker instantiates the module. Calls to `https://api.x.com` from that worker go to `/api/relay/api.x.com` on the same origin. The local docs server forwards only that host. A static host does not. Cargo writes that library as `target/wasm32-unknown-unknown/release/socli.wasm`. CI runs `wasm-opt` and puts the result at `website/public/wasm/socli.wasm`. That file is not committed. The live session reads and writes that config, which can hold tokens, in the page's local storage under `socli.config`. The demo session still does not. The wasm graph has no reqwest client.

A new dependency needs its reason stated in the pull request or change description. On macOS, the build also uses `xcrun swiftc` from Xcode Command Line Tools to compile and embed the browser authentication helper in `native/auth-macos.swift`. Other targets omit this helper.

The browser library is published as `@massimodeluisa/soclib`; the native npm launcher is `soclish`. [soclib's README](../packages/web/README.md) documents the mounted and headless reader APIs. Its worker and renderer are shared with the website, and `socli_theme` updates an existing browser session's appearance without recreating that session. Changes to the JavaScript bridge also run `bun test scripts/release.test.ts scripts/soclib.test.ts scripts/install.test.ts` and the browser checks served by `scripts/serve-soclib.ts` against the packaged library. After building WebAssembly, `bun test scripts/wasm.test.ts` executes the real module without network access; the release web job runs it too. WebAssembly CI rejects compiler warnings.

## Module map

Every file in `src/` and every folder in `src/net/` has one row.

| Module | Responsibility |
| --- | --- |
| `lib.rs` | The crate root. The terminal binary and the browser demo both use it |
| `main.rs` | The terminal binary. It calls `cli::main`. A browser build prints one line and exits 2 |
| `cli.rs` | The clap command line, the dispatch to each command, JSON output, error printing and exit codes. Native builds only |
| `agent.rs` | Agent mode: which environment variables mark an AI coding agent, and `SOCLI_AGENT` |
| `chrome.rs` | The header, the sub-bar, the rules, the footer nav and the key hints, shared by the reader and `--print` |
| `clip.rs` | Copying to the system clipboard, with OSC 52 as the fallback. The browser build remembers the text for the page |
| `clock.rs` | Wall time and a monotonic instant. Native builds read the process clock. The browser build imports `socli_millis` and `socli_mono` |
| `command.rs` | The grammar of the `SoCLI>` command line, the same in the reader and the scrollback prompt |
| `config.rs` | The config file: sections, paths, modes, atomic writes, the keys `socli config` reads and writes. The browser build reads and writes the text the page stores |
| `count.rs` | Post length the way x.com counts it (links 23, emoji 2) |
| `demo.rs` | The offline fixture world with posts on all five networks |
| `drafts.rs` | Composer drafts in `<config dir>/drafts.json`. The browser demo does not keep them |
| `grid.rs` | The browser screen: a ratatui backend that records the cells that changed |
| `help.rs` | `socli help`: the full page, one page per command and the JSON form, all drawn from one table |
| `hyperlink.rs` | OSC 8 hyperlinks: when they are on, which URLs are safe, and painting link regions |
| `intent.rs` | What a composer publishes: the text, the networks, the reply or quote, and the draft it came from |
| `jsonview.rs` | A small JSON inspector that colors keys, strings and numbers |
| `key.rs` | Key and mouse events. Native builds re-export crossterm. The browser sends one byte per key |
| `layout.rs` | Threads a flat page of posts into the rails the reader draws |
| `media.rs` | Loading pictures under one policy (`media::load` and its origin), the bounded decoder, pictures for `--print` and `socli view` through the viuer library, and the image cache |
| `minimal.rs` | The minimal and plain detail levels: a network line beside each post instead of avatars and rails |
| `fixtures.rs` | Test-only fixtures: the frozen clock, the demo state, post builders, and the grapheme and control-text samples |
| `model.rs` | Posts, people, metrics and feeds as the reader sees them, and `Post::to_json` |
| `plain.rs` | The scrollback printer behind `--print` and the `SoCLI>` prompt |
| `program_status.rs` | Native OSC 7501 status reports for terminal hosts such as Rex, with provider child tasks, measured feed-load progress, prompt waits, cancellation and output policy |
| `preview.rs` | The image picker facade. Native builds use `preview_native.rs`. The browser build does not fetch |
| `preview_native.rs` | The image picker and the background loader for previews, avatars and inline images. Native builds only |
| `render.rs` | The canvas, the post, thread and notification bodies, and the painting |
| `rich.rs` | `<b>` and `<i>` to Unicode bold and italic letters |
| `screens.rs` | Screens outside the feed: help, login, profile, notices, ask and the key overlay |
| `send.rs` | Writes from the command line: posting to several networks, `send_intent` with its draft record for both composers, and the result lines every write prints |
| `settings.rs` | The Settings screen (`,`) and the network filter panel (`F`) |
| `state.rs` | Reader state and the commands that change it; demo and live mode share one model |
| `theme.rs` | Palettes, mono, network and author colors, the appearance settings, and the icon sets |
| `tui.rs` | The full-screen reader with its modes and keys |
| `unfurl.rs` | Link preview data: reads the start of a link page and its Open Graph tags, refusing non-public hosts |
| `util.rs` | Formatting, prompts, browser opening and other helpers shared by the reader and the printer |
| `watchdog.rs` | The thread that ends the process when its terminal goes away, a signal arrives or the main loop stops coming round, with the heartbeat and the restore it does first. Native builds only |
| `web.rs` | The browser session: live and demo state, input, and the wasm exports. It does not open a terminal. Live reads go through the worker's HTTP import |
| `net/mod.rs` | The provider layer: the `Provider` trait, `Caps`, `Clients`, scoped ids, `parse_target`, `NetError` |
| `net/http.rs` | The blocking HTTP client every provider shares and the error text of a failed response. Native builds use reqwest. The browser build calls `socli_http` |
| `net/pkce.rs` | PKCE verifier and S256 challenge, shared by the X and dev/null sign-ins |
| `net/x/` | X: `client.rs` (API v2), `auth.rs` (browser session and OAuth 2.0 PKCE), `cookies.rs` (Chromium cookie import), `web.rs` (the website's GraphQL), `transaction.rs` (the web client's write header), `mod.rs` (the provider) |
| `net/bluesky/` | Bluesky over XRPC: `rpc.rs`, `auth.rs` (app password sessions), `facets.rs` (rich text), `map.rs`, `mod.rs` |
| `net/devnull/` | dev/null: `api.rs`, `auth.rs` (device key, SSH key), `browser.rs` (native macOS callback capture), `sign.rs` (signed v1 API), `ssh.rs` (signed commits), `map.rs`, `mod.rs` |
| `net/threads/` | Threads on the Meta Graph API: `auth.rs` (token, OAuth, refresh), `map.rs`, `mod.rs` |
| `net/mastodon/` | Mastodon over its REST API: `auth.rs` (app registration, PKCE sign-in, token sign-in, sign-out), `html.rs` (status HTML to text), `map.rs` (JSON to posts, users and pages), `mod.rs` (the client, `Caps`, length counting, the provider), `fixtures/` (synthetic JSON for the tests) |

## Rendering rules

Renderers draw through the `Canvas` in `render.rs`, at the absolute columns of the grid in [DESIGN.md](DESIGN.md). They return `StyledLine` values and never write ANSI escape codes themselves, because painting belongs to `paint_with` and `ratatui_style` in `render.rs`.

Text from the networks can carry terminal control sequences, so it reaches the screen only through code that cleans it. `Canvas::put` and the `StyledLine` constructors pass their text through `util::clean`, and CLI text output that prints network data outside a `StyledLine` calls `util::clean` itself. Only the app adds escape sequences: OSC 8 links from `hyperlink::wrap` and color codes from `paint_with` follow cleaning, while `program_status` writes OSC 7501 reports from fixed activity descriptions. Widths are measured per grapheme cluster with `util::graphemes`, which uses the `unicode-width` measure that ratatui uses.

Every glyph in the Unicode icon set has to exist in Menlo, the default macOS terminal font. Private Use Area glyphs appear only in `theme::Icons`, and each one carries a comment with its Nerd Fonts name and codepoint, in the form `// nf-<set>-<name> U+XXXXX`, as in `// nf-md-heart U+F02D1`. A new screen starts as a mockup in `design/`, following the workflow in [DESIGN.md](DESIGN.md), before any Rust code draws it.

Widths have no floor. `render::margin` gives the side margin (2 columns from 60 columns up, 0 below), `render::inner_width` the content width that remains, and `render::narrow` whether the narrow layout of [DESIGN.md](DESIGN.md#narrow-terminals) applies. Every width, height and offset uses saturating or checked arithmetic, so no subtraction can underflow at any terminal size, and a box or a card takes `min(preferred, available)` rather than assuming a minimum. Below `render::MIN_COLS` by `render::MIN_ROWS` the reader draws only the size notice, and `--print` lays a narrower `--width` out at `MIN_COLS`.

The detail level is a rendering constraint. `plain` draws ASCII only outside post text, apart from `·`, and draws no color. `minimal` draws no boxes or icons in posts. Both are handled in `minimal.rs` and `plain.rs`, and a change to the standard renderer has to check that those two levels still hold.

## Performance

The reader's thread never decodes, resizes, encodes, hashes or copies the pixels of a picture. The download workers in `preview.rs` fetch and decode each picture, cut a profile picture to its box, and build its ratatui-image protocol. One encoder thread then resizes and encodes the picture for the size of each box that draws it, and the box keeps its loading frame until the answer comes back. A picture keeps encodings for at most two box sizes, so the same post drawn in both panes of split view settles on two encodings instead of encoding again on every frame. `Previews::poll` and the draw functions may clone an `Arc`, but never anything that holds pixels.

The loop draws a frame only when something changed: a batch of input, a finished download or encode, a link card that got its picture size or its page, a new autosave error, or a spinner that is turning. With nothing to do it still draws once every 30 seconds. Between checks it waits 120 ms while a spinner turns or an encode is out, and one second otherwise.

Input is read in batches. The loop takes whatever is already queued, up to 64 events or 8 ms of reading, and draws once after the whole batch. A run of pointer moves keeps only the last move, a run of wheel notches in one direction over one pane becomes a single step of its notch count, and a change of direction starts a new step, and a burst of resizes counts once. Keys and pastes run one at a time in the order they arrived, and a key that quits drops the rest of the batch. The terminal reports pointer movement (mode 1003) only while a list that follows the pointer is open, so a moving mouse costs nothing the rest of the time.

`SOCLI_TRACE_FRAMES=<file>` makes the reader append one line per drawn frame to that file:

```text
<ms since start> events=<n> moves_dropped=<n> scroll_net=<n> handle_ms=<ms> build_ms=<ms> draw_ms=<ms>
```

`events` counts the input read since the previous frame, `moves_dropped` the pointer moves the batching skipped, and `scroll_net` the wheel notches down minus the notches up. `handle_ms` is the time spent running presses, `build_ms` the layout work before the draw, and `draw_ms` the draw itself. A pass through the loop that draws nothing writes an `idle_wakeups=<n>` line, where `n` counts those passes since the last frame. When the variable is unset or empty, the reader opens no file and takes no timings.

## Errors

Application code returns `Result<_, String>`. The message starts in lowercase and tells the user what to run next, as in this message from `src/state.rs`:

```text
no accounts yet. `socli login bluesky` (or x, devnull, threads, mastodon), or `socli --demo`.
```

The network layer returns `NetError`, built with `thiserror`, whose variants carry the network and the text the user sees (sign-in required, unsupported on this network, an HTTP failure, a rate limit with its retry time, as in `Bluesky rate limit reached; try again in 3 minutes`). Code outside tests does not call `unwrap` or `expect`, except on an invariant that a comment documents. When a network rejects a request, SoCLI shows the rejection with its context and never hides it.

The process exits with 0 on success, 1 when a network or target failed, and 2 for a usage error or a step that needs a person at a terminal. The reader exits with 130 on `ctrl-c`. The reader, the standalone composer and the secret prompt also arm the watchdog in `watchdog.rs`, because crossterm's read can spin forever on a terminal whose other end is gone. Two seconds after the first sign of trouble (SIGHUP, SIGTERM, a main loop that has not come round for five seconds, or, in the reader and the composer, stdout no longer a terminal), the watchdog exits with 143 after SIGTERM and with 129 otherwise. After a signal on a terminal that still answers, it first restores the reader's or the composer's screen the way `ctrl-c` does; in every other case it only turns raw mode off, so nothing is written to a terminal that may be gone. A loop that is still running saves the composer's text and exits on its own before then. Leaving the reader or the composer disarms the heartbeat and terminal checks, while the signals stay watched until the process ends. When errors are JSON (`--json`, or agent mode), `main.rs` prints one line on stderr, `{"error": "…"}`, and prints nothing on stdout. Clap's own usage errors add `usage` and `tip` fields to that line. A failed write still prints its JSON array on stdout and exits 1 with no stderr line.

## Output rules for pipes and agents

Stdout that is not read by a person at a terminal carries no escape codes: no ANSI color, no OSC 8 hyperlinks, and no Nerd Font glyphs, because the icon choice falls back to Unicode icons when stdout is not a terminal or agent mode is on. `plain.rs` and `hyperlink.rs` check both conditions, and a new stdout path has to do the same. JSON is written through `say_json` in `cli.rs` and uses the field names of `Post::to_json`.

Native [terminal status reports](../README.md#osc-7501-program-status) use stderr independently of stdout. OSC 7501 reporting is automatic only when stderr is a terminal, agent mode is off and `TERM` is not `dumb`. `SOCLI_PROGRAM_STATUS=on` (or `1`) explicitly enables those escapes even in redirected stderr or agent mode; `off` (or `0`) disables them. A caller parsing stderr while reporting is enabled must separate the reports from any JSON error line.

Normal reports repeat `app=socli`, and every main report has the title `SoCLI`. Provider child tasks use `network/<slug>/<read|post|action|auth>` IDs and fixed titles such as `Bluesky read`; sign-in waits also block the matching child with `kind=auth`. Feed-load batches report main-task progress from completed provider requests, including failures, and omit progress when the batch ends. The reporter clears completed children on the next command or reader key and clears SoCLI's children on cancellation. A child `state=clear` record may omit `msg`, `title` and `app`; parsers must accept that form. Titles and messages contain no account names or request content. Reports send no probe or heartbeat. Status reporting reads no input and leaves command results unchanged. The browser reader does not emit these reports.

## Demo mode

Demo mode never touches the network. Every write changes local state only, and the status says so, for example `demo · liked here only`. In JSON, write results carry `"demo": true`. `demo.rs` holds the fixture world, and the tests and the screenshots use it too. Demo avatars are `demo://avatar/<user>` URLs. Demo mode never writes the user's config either. Every save the reader makes goes through `settings::save_for`, which in demo leaves the file alone and returns `<what> · demo, not saved`, so no reader code calls `settings::save` directly.

## Comments

A comment that restates the code does not belong in the repository. Keep a comment only when it explains a reason the code cannot show, and use `// MARK:` markers to group the sections of a file. Every module opens with a module doc: one `//!` line, or a `/*! … */` block when it needs more lines. A one-line doc comment on an item uses `///`. A doc comment longer than one line is a single `/** … */` (or `/*! … */`) block, never a stack of `///`, `//!` or `//` lines. Leave no narration and no leftover probes.

## Tests

The [0.1.4 source review](REVIEW.md) records the failure paths covered by the regression suite and the boundaries that still need live-provider verification.

Unit tests live in a `#[cfg(test)] mod tests` block next to the code they test. The integration tests in `tests/cli.rs` run the built binary through the `socli()` helper, which sets `SOCLI_CONFIG_DIR` to an empty temporary directory, `NO_COLOR=1`, `TERM_PROGRAM=vscode`, `TERM=xterm-256color`, `SSH_TTY=1` and `SOCLI_AGENT=0`, and removes `SOCLI_ICONS`, `NO_ICONS`, `SOCLI_NET`, `SOCLI_THEME`, `SOCLI_DENSITY`, `SOCLI_DETAIL`, `SOCLI_LAYOUT`, `SOCLI_MOUSE`, `SOCLI_IMAGE`, `SOCLI_TRUECOLOR`, `SOCLI_TRACE_FRAMES`, `SOCLI_PROGRAM_STATUS`, `COLORFGBG` and every network credential variable, so the output is the same on every machine. Tests use fixtures or a loopback HTTP server, never contact public networks and never read or write `~/.config/socli`. A test that needs to write uses its own directory from `own_config`. Unit tests must not trigger a settings save, because that would write the real config. Renderer tests assert the columns of elbows, stats and folds, and check that no Private Use Area character appears in Unicode mode.

## Configuration and secrets

SoCLI reads its configuration from `$SOCLI_CONFIG_DIR/config.toml`, or from `$XDG_CONFIG_HOME/socli/config.toml` when `SOCLI_CONFIG_DIR` is not set, or from `~/.config/socli/config.toml` when neither is. It writes the file with mode `0600` and its directory with mode `0700`. Every write goes through `config::update`, which holds an exclusive lock on `config.toml.lock` while it loads the file, applies the change and replaces the file atomically, so a sign-in or a setting never leaves a half-written file and two SoCLI processes never drop each other's change. Drafts follow the same rule through `drafts::update_at` and `drafts.json.lock`. An atomic write creates a new temporary file with mode `0600` beside the target, syncs it, renames it over the target and syncs the directory, and the lock files stay in place. Sign-in state lives in the network's own section (`[x]`, `[bluesky]`, `[devnull]`, `[threads]`), and the dev/null device key is a separate file in the config directory.

Settings resolve in this order, first match wins:

| Setting | Order |
| --- | --- |
| Theme, density, detail, layout, mouse | the flag, then the `SOCLI_THEME`, `SOCLI_DENSITY`, `SOCLI_DETAIL`, `SOCLI_LAYOUT` or `SOCLI_MOUSE` variable, then `[ui]` |
| Images | `--images`, then `SOCLI_IMAGE`, then `ui.images` |
| Icons | `--no-icons`, then `--icons`, then `SOCLI_ICONS`, then `NO_ICONS` and terminal detection, then `ui.icons` |
| Networks | `--net`, then `SOCLI_NET`, then `feed.networks` |
| Link page fetching | `ui.unfurl` only |
| Enter in the composer | `ui.enter` only |

### Config keys

`socli config list` shows every key, `get` reads one and `set` validates and saves one. The keys come from `config::keys` in `src/config.rs`.

| Key | Values | Default |
| --- | --- | --- |
| `feed.networks` | `all` or a comma list of `x`, `bluesky`, `devnull`, `threads`, `mastodon` | `all` |
| `feed.color_by` | `auto`, `network`, `author` | `auto` |
| `feed.replies` | `oldest`, `newest` | `oldest` |
| `default_network` | a network slug, or `none` | `none` |
| `x.color`, `bluesky.color`, `devnull.color`, `threads.color`, `mastodon.color` | `default`, `blue`, `sky`, `teal`, `green`, `sage`, `gold`, `amber`, `coral`, `rose`, `pink`, `red`, `violet`, `indigo`, or `#rrggbb` | `default`, `sky`, `amber`, `violet`, `indigo` |
| `ui.theme` | `auto`, `native`, `black`, `white` or a name from `socli config themes` (`dark` and `light` still parse) | `auto` |
| `ui.colors` | `full`, `mono` | `full` |
| `ui.density` | `compact`, `cozy`, `roomy` | `cozy` |
| `ui.detail` | `plain`, `minimal`, `medium`, `rich` (`standard` still parses as `medium`) | `rich`, or `plain` on an ESP or other low-resource terminal |
| `ui.layout` | `auto`, `single`, `split` | `auto` |
| `ui.icons` | `auto`, `nerd`, `unicode` | `auto` |
| `ui.images` | `auto`, `kitty`, `iterm`, `sixel`, `blocks`, `off` | `auto` |
| `ui.unfurl` | `on`, `off` | `on` |
| `ui.mouse` | `on`, `off` | `on` |
| `ui.enter` | `post`, `newline` | `post` |
| `colors.<name>` | `#rrggbb`, for `fg`, `dim`, `line`, `surface`, `blue`, `sky`, `teal`, `green`, `sage`, `gold`, `amber`, `coral`, `rose`, `pink`, `red`, `violet` | the theme's value |

### Environment variables

| Variable | What it does |
| --- | --- |
| `SOCLI_CONFIG_DIR` | The directory that holds `config.toml` and `drafts.json` |
| `SOCLI_CACHE_DIR` | The image cache directory; without it SoCLI uses `$XDG_CACHE_HOME/socli`, then `~/.cache/socli` |
| `SOCLI_AGENT` | `1` forces agent mode, `0` turns it off, anything else leaves detection on |
| `SOCLI_PROGRAM_STATUS` | Native OSC 7501 status on stderr: `auto` or unset uses terminal detection with agent mode off and `TERM` other than `dumb`; `on` or `1` forces reports, including pipes and agents; `off` or `0` disables them |
| `SOCLI_NET` | Networks to read or post to when `--net` is absent |
| `SOCLI_THEME` | A theme name (see `ui.theme`) when `--theme` is absent |
| `SOCLI_DENSITY` | `compact`, `cozy` or `roomy` when `--density` is absent |
| `SOCLI_DETAIL` | `plain`, `minimal`, `medium` or `rich` when `--detail` is absent (`standard` still parses as `medium`). When this and `ui.detail` are unset, a normal terminal uses `rich` and an ESP or other low-resource terminal uses `plain` |
| `SOCLI_LAYOUT` | `auto`, `single` or `split` when `--layout` is absent |
| `SOCLI_MOUSE` | `on` or `off` when `--mouse` is absent |
| `SOCLI_IMAGE` | `auto`, `kitty`, `iterm`, `sixel`, `blocks` or `off` when `--images` is absent |
| `SOCLI_ICONS` | `auto`, `nerd` or `unicode` when `--icons` is absent |
| `SOCLI_HYPERLINKS` | `1` turns OSC 8 hyperlinks on, `0` off; otherwise SoCLI detects the terminal and keeps them off in tmux, screen and pipes |
| `SOCLI_TRUECOLOR` | `1` turns 24-bit color on, `0` off; otherwise `COLORTERM`, the terminal program and `TERM` decide |
| `SOCLI_TRACE_FRAMES` | A file the reader appends one timing line to for every frame it draws (see [Performance](#performance)) |
| `NO_COLOR` | Any value turns color off |
| `NO_ICONS` | Any value picks Unicode icons over Nerd Font glyphs |
| `X_AUTH_TOKEN`, `X_CT0` | The X browser session cookies; override `[x]`. Both are required |
| `X_CLIENT_ID`, `X_CLIENT_SECRET` | The X app's OAuth credentials; override `[x]` |
| `X_ACCESS_TOKEN`, `X_REFRESH_TOKEN` | The X user tokens; override `[x]` |
| `X_USER_ID`, `X_USERNAME` | The signed-in X account; override `[x]` |
| `X_BEARER_TOKEN` | The X app-only bearer for public reads; overrides `[x]` |
| `BLUESKY_HANDLE` | The handle for `socli login bluesky` when `--handle` is absent |
| `BLUESKY_APP_PASSWORD` | The app password for `socli login bluesky`, instead of a prompt |
| `BLUESKY_SERVICE` | The server for `socli login bluesky` when `--service` is absent; the default is `bsky.social` |
| `THREADS_ACCESS_TOKEN` | The Threads access token; wins over the stored one |
| `THREADS_APP_SECRET` | The Meta app secret that exchanges a token for a long-lived one when `--app-secret` is absent |
| `MASTODON_ACCESS_TOKEN` | The Mastodon access token; wins over the stored one and signs in without a browser |
| `MASTODON_SERVER` | The Mastodon server for `socli login mastodon` when `--server` is absent, and the server a stored session talks to. Without either, `socli login mastodon` asks for the server, and an empty answer keeps `mastodon.social` |
| `AI_AGENT` | Names the agent and turns agent mode on; the other variables in `agent.rs` do the same for known agents |

Empty values are ignored. Credential overrides belong to one session. An explicit X access token selects OAuth and cannot inherit another account's refresh token or browser cookies; a complete explicit cookie pair selects browser authentication. A partial cookie pair cannot reuse a stored session. Changing the Mastodon server drops inherited credentials, and replacing a Mastodon or Threads token clears unrelated account metadata. The reader also reads terminal variables to decide what the terminal can draw: `TERM`, `TERM_PROGRAM`, `COLORTERM`, `COLORFGBG` (the light or dark background), `TMUX`, `STY`, `VTE_VERSION`, `WT_SESSION`, `KONSOLE_VERSION`, `SSH_TTY` and `SSH_CONNECTION` (clipboard choice), and `HOME`.

`socli auth status` redacts every secret: it shows `missing` when a token is not set, `set` for a token of six characters or fewer, and the last four characters for a longer one. Tokens are never printed in full and never logged, and no error text may carry a request URL that holds a token.

## Integrations

Every network lives in `src/net/<slug>/` behind the `Provider` trait in `src/net/mod.rs`, and is documented in `docs/integration/<slug>/` with five pages: `README.md`, `auth.md`, `endpoints.md`, `mapping.md` and `testing.md`. Posts carry scoped ids of the form `<slug>:<native id>` everywhere inside the app and in JSON. `Caps` gates every key, hint and command: a feature the network lacks is not drawn and not offered, and a command that needs it fails with the network's own message (for example `Threads has no likes in its API`). [integration/README.md](integration/README.md) describes how a new network plugs in.

## Commits

[Releasing SoCLI](RELEASING.md) describes version preparation, release tags, publishing destinations, and the required repository settings. Publishing runs only from release tags; branch and pull request workflows run checks.

Commit messages follow English Conventional Commits with a subject line only: imperative mood, about 50 characters, no body, no co-author lines, and one of the prefixes `feat:`, `fix:` or `docs:`. If the repository gains a `CHANGELOG.md`, each change first adds one line in the past tense under `[Unreleased]`.

## Documentation

The documents in `docs/` are prose, and every claim in them is checked against the code. Text for people follows the owner's writing rules and passes through the `technical-writing`, `humanize-writing` and `humanizer` skills, in that order. A visual change updates [DESIGN.md](DESIGN.md) in the same change, and a change to one of these rules updates this file. [INDEX.md](INDEX.md) lists every document. Each network keeps its five pages under `docs/integration/<slug>/`, and `docs/integration/README.md` links every folder.
