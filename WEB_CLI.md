# WEB_CLI — the browser build of socli

Plan for compiling the socli reader to WebAssembly and running it inside the website's demo, so that the landing page shows the compiled CLI instead of a second, hand-written implementation of it.

This is the implementation plan for improvement 18 in [IMPROVEMENTS.md](IMPROVEMENTS.md), which asks for "a shared Rust core for an interactive browser demo" and records that "no WASM build claimed" is the current evidence status. [COMPLETION_REPORT_1.md](COMPLETION_REPORT_1.md:208) lists the same prompt as not started.

Every number in this document is a target or a budget, not a measurement. Nothing in this repository compiles to WebAssembly yet.

## The goal

The landing page runs the browser build. [website/index.md](website/index.md) still promises that `socli --demo` "opens an offline feed with posts from all five networks before any account exists", that "the status row says demo", that "likes, replies, reposts and bookmarks change the screen and are never sent", and that "settings you change in the demo are not saved". Opening the demo from the welcome screen must keep every one of those promises, because the code that keeps them is the code that runs.

Three things follow from that:

1. The browser runs the real reader. The same state machine, the same rails, the same badges, the same detail levels, the same keys.
2. The browser runs the live reader on this page. Bluesky, Mastodon, Threads and X sign in and read here. X and Threads open an approval page when this build includes socli's app id. A build without that id says so. X on the page uses the official API, so For you is a search, and likes and follows can be refused. The local docs server forwards only `api.x.com` for the X token exchange, and a static host such as GitHub Pages does not. Threads sends the approval code to `https://socli.sh/threads/token`, which holds the app secret. The Threads app is published. dev/null signs in on the page: it opens the approval on dev-null.sh, and you paste the `devnull://` callback. GitHub SSH signing stays in the terminal. `socli login x` in a terminal still imports the browser session. Config, tokens included, is stored in the origin's local storage. Rich mode draws profile pictures and post images on the page, including a signed-in timeline, by loading each image from its URL. `ui.images` set to `off` keeps the image card. kitty, iTerm2 and sixel stay in the terminal. The published site is static.
3. The demo data has one source. The posts come from [src/demo.rs](src/demo.rs), through the same reducer the native binary uses. The copy in [website/.vitepress/theme/reader.ts](website/.vitepress/theme/reader.ts) is the fallback when the wasm file does not load.

The welcome screen can still open the offline demo. Posting works on the networks the page can reach.

## What is true today

### The reader's drawing is already portable

The reader's content is not written as ANSI strings and it is not built from a terminal library's widget set. It is built as a grid of styled spans, and that grid is handed to a backend at the very last moment.

- [src/render.rs:417](src/render.rs:417) holds `Canvas`, a cell grid with `links`, `targets` and `previews` collected in canvas coordinates while it is filled.
- [src/render.rs:199](src/render.rs:199) holds `View`, the result: lines, link regions, image slots, pressable targets.
- [src/render.rs:137](src/render.rs:137) holds `Span` and `StyledLine`, a `(Kind, text)` pair. `Kind` is semantic ([src/theme.rs:8](src/theme.rs:8)): `Fg`, `Dim`, `Bold`, the named accents, `Link`, and `Net(network)`.
- [src/screens.rs:508](src/screens.rs:508) holds `linked_body`, the one function that produces the whole feed body. The reader and the `--print` path call the same one.
- [src/tui.rs:5883](src/tui.rs:5883) holds `to_line_in`, the adapter from `StyledLine` to a ratatui `Line`.
- [src/tui.rs:4449](src/tui.rs:4449) holds `draw`, the composition of chrome, body, split panes and overlays into a ratatui `Frame`.

The reader draws with `Paragraph`, `Block` and `Clear` and nothing else. There is no `List`, no `Table`, no `Gauge`. That is why a browser backend is an addition and not a rewrite: the drawing code is already a function from state to cells, and the cells are already a backend's job.

The tests prove the point. [src/tui.rs:7384](src/tui.rs:7384) and [src/tui.rs:7406](src/tui.rs:7406) build a `ratatui::backend::TestBackend` and draw the real reader into it. A browser backend is the same thing with a different destination.

### The edges are not portable

Everything around the drawing is bound to a terminal or to a native process.

| Edge | Where | Why it blocks a browser |
| --- | --- | --- |
| `crossterm` 0.28 | [Cargo.toml:26](Cargo.toml:26), 44 lines of [src/tui.rs](src/tui.rs) | crossterm 0.28 has only `event/sys/unix.rs` and `event/sys/windows.rs`. There is no wasm32 target, so it does not compile for `wasm32-unknown-unknown`. |
| `signal-hook` 0.3 | [Cargo.toml:37](Cargo.toml:37), [src/tui.rs:999](src/tui.rs:999), [src/watchdog.rs:113](src/watchdog.rs:113) | pulls `libc`, which has no wasm32 support. A tab has no SIGTERM. |
| `reqwest` with `blocking` | [Cargo.toml:32](Cargo.toml:32), [src/net/http.rs:5](src/net/http.rs:5) | the blocking client needs `tokio`, which the blocking feature pulls in unconditionally. |
| `viuer`, `ratatui-image` | [Cargo.toml:31](Cargo.toml:31), [Cargo.toml:45](Cargo.toml:45), [src/media.rs:10](src/media.rs:10), [src/preview.rs:13](src/preview.rs:13) | kitty, iTerm2 and sixel are terminal graphics protocols. A browser has a canvas and no protocol. `ratatui-image` also pulls crossterm. |
| `ssh-key` | [Cargo.toml](Cargo.toml), [src/net/devnull/sign.rs](src/net/devnull/sign.rs) | The page signs a dev/null device with ed25519. GitHub SSH signing still calls `ssh-keygen` and stays in the terminal. |
| Threads | [src/tui.rs:1406](src/tui.rs:1406), [src/preview.rs:252](src/preview.rs:252), [src/state.rs:2519](src/state.rs:2519), [src/unfurl.rs:204](src/unfurl.rs:204) | `std::thread` compiles for wasm and panics at run time. |
| The filesystem | [src/config.rs](src/config.rs), [src/drafts.rs](src/drafts.rs), [src/media.rs](src/media.rs), [src/tui.rs:891](src/tui.rs:891) | `std::fs` compiles and fails. The browser has `localStorage`. |
| The process | [src/util.rs:415](src/util.rs:415) opens a browser, [src/clip.rs:4](src/clip.rs:4) copies to the clipboard, [src/tui.rs:1470](src/tui.rs:1470) exits with 130 | a page has `window.open` and `navigator.clipboard`, and no exit code. |

Two things soften this. `ratatui` itself is fine: its `crossterm` feature is optional, and [its `Cargo.toml` for 0.29](https://github.com/ratatui/ratatui/blob/v0.29.0/Cargo.toml) lists `default = ["crossterm", "underline-color"]`, so `default-features = false` gives a ratatui with `Buffer`, `Cell`, `Style`, `Line`, `Span`, `Frame` and no terminal. And [build.rs](build.rs:5) already returns early for every target that is not macOS, so the Swift authentication helper never enters a browser build.

### The website's demo is a second implementation

The demo on the landing page is a hand-written TypeScript reader, and it has drifted from the Rust one.

| | Website | Rust |
| --- | --- | --- |
| Posts | 13, hardcoded in [reader.ts:102](website/.vitepress/theme/reader.ts:102) | 18 in [src/demo.rs:144](src/demo.rs:144), 17 of them in the home feed ([src/demo.rs:455](src/demo.rs:455)) |
| Home feed | 5 roots, `HOME = ['p1','p2','p8','p11','p10']` ([reader.ts:83](website/.vitepress/theme/reader.ts:83)) | the demo world's own 17 ids |
| Keys wired | `? j k g G Enter Esc l` ([reader.ts:933](website/.vitepress/theme/reader.ts:933)) | the list in [website/parts/keys.md](website/parts/keys.md) |
| Split view | none; opening a thread replaces the feed | [src/tui.rs:86](src/tui.rs:86) splits at 140 columns, 100 with `--layout split` |
| Mentions, bookmarks, quotes, reposts | none | all present in the demo world |
| Badge colours | invented per post | a network colour or a colour from the handle |
| Ages | preformatted strings (`'3h'`) | `DateTime<Utc>` against an injected clock |

The site also carries two older copies: `site/reader.js` (84 columns, four networks) and the design prototype under `design/`. The demo post copy in `site/` is already one revision behind the Rust text, which is the drift this plan removes at the source.

### The build has no wasm target

- [Cargo.toml](Cargo.toml) has one `[[bin]]`, no `[lib]`, no `crate-type`, no `[features]`, no `wasm-bindgen`.
- [src/main.rs:3-36](src/main.rs:3) declares 33 modules, all compiled unconditionally.
- [.github/workflows/pages.yml:6](.github/workflows/pages.yml:6) builds the site on changes to `website/**` only. A Rust change does not reach the site, so the site cannot ship a Rust artifact yet.
- `website/public/` is gitignored, and that directory is where VitePress takes static assets from. The site's own fonts and icons are missing from a fresh checkout, which [COMPLETION_REPORT_1.md:159](COMPLETION_REPORT_1.md:159) already flags. The wasm artifact needs a home that survives a checkout.

## The route

### Chosen: the real reader, drawn by Rust, painted by the site

Rust keeps drawing the reader exactly as it does today, into a ratatui `Buffer`. A small backend, new to the project, keeps that buffer and hands the changed cells to JavaScript. The site renders those cells with its own markup and its own palette, inside the terminal surface it already reserves.

```
browser keydown ──▶ bytes ──▶ Session::key ──▶ App (state machine)
                                         │
                                         ▼
site DOM ◀── changed cells ── Session::frame ◀── App (draw)
```

The parts that stay Rust: the state machine, the reducer ([src/command.rs:7](src/command.rs:7)), the layout ([src/layout.rs](src/layout.rs)), the painters ([src/render.rs](src/render.rs), [src/chrome.rs](src/chrome.rs), [src/screens.rs](src/screens.rs)), the themes ([src/theme.rs](src/theme.rs)), and the demo world ([src/demo.rs](src/demo.rs)).

The parts that move to the site: the pixels, the key decoding, the viewport size, and the focus ring.

### Alternatives, and why not

| Route | What it gives | What it costs |
| --- | --- | --- |
| **Ratzilla** ([ratatui/ratzilla](https://github.com/ratatui/ratzilla)) | A ready browser backend: `DomBackend`, `CanvasBackend`, a WebGL2 backend, key and mouse handlers with grid coordinates. | The version pair has to be right: Ratzilla 0.2.0 is the last release that accepts ratatui `^0.29`, and 0.3.x needs ratatui `^0.30`. This project is on 0.29 ([Cargo.toml:30](Cargo.toml:30)). Ratzilla also owns the DOM, so the site would lose its palette and its markup, and it wants `trunk` in the build. Worth a measured spike in milestone 0, not worth committing to. |
| **A JSON bridge** — wasm core, the existing agent-mode JSON, the site keeps its TypeScript reader | Cheap: the JSON contract is already stable and documented ([website/docs/agents.md](website/docs/agents.md), [src/model.rs:131](src/model.rs:131)). Kills the data drift immediately. | The browser still shows a JavaScript re-implementation of the reader, which is the thing this plan is meant to end. Useful as milestone 1's by-product, not as the answer. |
| **WASI and a browser terminal** (`wasm32-wasi` plus an xterm.js) | The real binary, unmodified, in a real terminal emulator. | Crossterm still does not compile, so the interactive reader is out of reach; what remains is the `--print` scrollback, which is not the reader the site advertises. It also adds a WASI runtime to the page. |
| **A server PTY** | The native binary, exactly. | A running server, session state, account exposure on a public page. [IMPROVEMENTS.md:57](IMPROVEMENTS.md:57) rules this out: "Do not introduce an open proxy to obtain parity." |

## Target architecture

### One crate, two feature sets

The crate stays one crate. What changes is that the library becomes a library, and that the native-only edges become optional.

```toml
[lib]
name = "socli"
path = "src/lib.rs"
crate-type = ["rlib", "cdylib"]

[[bin]]
name = "socli"
path = "src/main.rs"

[features]
default = ["native"]
native = ["dep:crossterm", "dep:signal-hook", "dep:reqwest/blocking", "dep:viuer",
          "dep:ratatui-image", "dep:ssh-key", "ratatui/crossterm", "ratatui/underline-color"]
web = []
```

`src/main.rs` keeps clap, the command dispatch, the JSON output and the exit codes; it becomes a thin binary over `socli::`. The modules that the reader needs become library modules; the modules that only the native binary needs (sign-in, sending, media, drafts on disk) stay behind the `native` feature.

The rule for the split is narrow: **the reader must compile with `native` off.** That means the reader cannot reach a provider, a file, a thread, a process or a terminal.

Two consequences worth naming:

- `net/mod.rs` keeps its types (`Network`, `FeedKind`, `Clients`, scoped ids). The providers and `net/http.rs` move behind `native`. `State::apply` keeps its signature, and the browser never calls it with a client that can fetch.
- `theme.rs` returns `ratatui::style::Color` today ([src/theme.rs:5](src/theme.rs:5), [src/theme.rs:786](src/theme.rs:786)). That type is ratatui core, not crossterm, so it can stay. The browser backend resolves it to RGB through the same palette the terminal uses.

### The seams

Five seams, each with a native side and a browser side: keys and mouse, the terminal, the clock, storage, and the network and media edges.

#### Keys and mouse

`crossterm::event::KeyCode` is not only in the input layer. It is in the model layer: [src/render.rs:268](src/render.rs:268) defines `Act::Key(KeyCode)` and `Act::Input(KeyCode, KeyModifiers)`, and [src/chrome.rs:3](src/chrome.rs:3), [src/screens.rs:851](src/screens.rs:851) and [src/screens.rs:1541](src/screens.rs:1541) use it too. Replacing it outright would touch every match arm in a 9,883-line file.

The cheap way out is a facade with the same names.

```rust
// src/key.rs
//! Key and mouse events, one shape for the terminal and for the browser.

// The names the code already uses, so the reader's match arms do not change.
pub use Code as KeyCode;
pub use Modifiers as KeyModifiers;
pub use Key as KeyEvent;

/// The codes the reader uses: Char, Enter, Esc, Tab, BackTab, Backspace, Up,
/// Down, Left, Right, Home, End, PageUp, PageDown, Delete, F, Null, Modifier,
/// and the eight modifier keys.
pub enum Code { /* … */ }
```

The facade exports the names the code already uses, so `KeyCode::Char('j')`, `KeyCode::Enter` and `KeyModifiers::CONTROL` keep compiling unchanged in [src/tui.rs](src/tui.rs), [src/render.rs](src/render.rs), [src/chrome.rs](src/chrome.rs) and [src/screens.rs](src/screens.rs).

Native builds re-export crossterm's types from the facade and translate at the edge. Browser builds define them. The browser sends one byte per key, with the code and the modifiers packed into it; the session decodes it into the same `KeyEvent` the terminal produces.

#### The terminal

The escape sequences and the mode changes move behind a second facade:

| Native | Browser |
| --- | --- |
| `enter` / `leave` alternate screen ([src/tui.rs:2013](src/tui.rs:2013)) | nothing: the page owns the scroll |
| raw mode, bracketed paste, focus change, keyboard enhancement flags ([src/tui.rs:2018](src/tui.rs:2018)) | nothing; the browser reports paste and focus on its own |
| mouse capture escapes written by hand ([src/tui.rs:838](src/tui.rs:838)) | nothing; the browser reports clicks and wheel |
| `terminal::size()` ([src/tui.rs:1099](src/tui.rs:1099), [src/util.rs:207](src/util.rs:207)) | the size the session was told |
| the `/dev/tty` OSC 11 background query ([src/tui.rs:1967](src/tui.rs:1967)) | `prefers-color-scheme` |
| `signal_hook` and the watchdog ([src/watchdog.rs:113](src/watchdog.rs:113)) | nothing |
| `std::thread::scope` around a key's work ([src/tui.rs:1406](src/tui.rs:1406)) | the work runs inline |

The last row is the one that changes behaviour rather than plumbing. On the terminal, a key that hits the network runs on its own thread so the screen stays alive. In the browser the same work runs inline, inside the reader worker. A synchronous request blocks that worker until it returns. The page keeps painting the last frame.

#### Clock, storage, network, media

| Concern | Native | Browser |
| --- | --- | --- |
| Clock | `Utc::now`, and `Local` for the reader's clock ([src/state.rs:350](src/state.rs:350)) | `socli_millis` (`Date.now`) and `socli_mono` (`performance.now`). The demo session still freezes its clock |
| Storage | `config.toml`, `drafts.json`, the image cache | `socli.config` in the origin's local storage. Drafts are not kept. The demo session still does not save |
| Network | `reqwest::blocking` through [src/net/http.rs](src/net/http.rs) | `socli_http` in the reader worker. The wasm graph has no reqwest |
| Media | `viuer` and `ratatui-image` | none in this plan: the browser demo runs with images off |
| Clipboard | `pbcopy` and friends ([src/clip.rs](src/clip.rs)) | `navigator.clipboard` |
| Open a link | `open` and `xdg-open` ([src/util.rs:415](src/util.rs:415)) | `window.open` |

### The frame transport

The browser backend is a `ratatui::backend::Backend`. In ratatui 0.29 the trait wants `draw`, `hide_cursor`, `show_cursor`, `get_cursor_position`, `set_cursor_position` and `size`, and it provides `flush`, `clear`, `clear_region` and `append_lines` with defaults. The backend keeps a `Buffer`, applies the cells ratatui hands it, and records what changed.

Ratatui already diffs: `Terminal::draw` hands the backend only the cells that changed since the last frame. So the transport is a diff, not a full frame, and a keystroke that moves the selection sends a few dozen cells rather than a whole screen.

The cell payload is fixed width: one `char`, one foreground RGB, one background RGB, one modifier byte. The site paints each row as spans, the way [reader.ts:349](website/.vitepress/theme/reader.ts:349) already paints its rows, so the change to the site is a change of source, not a change of look.

The link regions and the pressable targets come out of the same `Canvas` ([src/render.rs:417](src/render.rs:417)) that draws the pixels, in canvas coordinates, so the site can make a URL clickable and a name pressable without re-deriving anything.

### The session API

The wasm side exposes one session type. The surface is small on purpose.

```rust
// src/web.rs
//! The browser session: the reader, a viewport, and a way in.

pub struct Session {
    app: App,
    backend: Grid,
    clock: Instant,
}

impl Session {
    pub fn new(cols: u16, rows: u16) -> Session;
    pub fn key(&mut self, bytes: &[u8]);          // one key, one byte of code plus modifiers
    pub fn paste(&mut self, text: &str);
    pub fn pointer(&mut self, kind: u8, col: u16, row: u16, button: u8);
    pub fn resize(&mut self, cols: u16, rows: u16);
    pub fn frame(&mut self) -> &[u8];             // the cells that changed since the last frame
    pub fn text(&self) -> String;                 // the same screen as plain text, for screen readers
}
```

The wasm exports are a thin `extern "C"` layer over it. The site needs six calls and a byte buffer.

The session starts in demo mode, with the demo world installed through the same path the native binary uses: [src/state.rs:341](src/state.rs:341) calls `install_demo`, and [src/state.rs:354](src/state.rs:354) fills the feeds from `demo::world(now)`.

### The site

The demo keeps its place on the landing page. What changes is where the pixels come from.

- [website/.vitepress/theme/components/DemoReader.vue](website/.vitepress/theme/components/DemoReader.vue) keeps the terminal chrome, the about panel, the touch buttons and the live region. It gains a canvas or a grid of spans that the session paints.
- The terminal surface is already reserved: [site.css:292](website/.vitepress/theme/styles/site.css:292) sizes the frame to `min(100%, calc(92ch + 28px))`, [site.css:393](website/.vitepress/theme/styles/site.css:393) reserves `92ch`, and [site.css:378](website/.vitepress/theme/styles/site.css:378) gives the screen its height. The grid drops into that space without a layout change.
- The palette maps across: the reader's eleven accents ([site.css:463](website/.vitepress/theme/styles/site.css:463)) are the same eleven the Rust themes name. The Rust side sends RGB, so the browser shows the theme socli would have drawn, and the site's own chrome keeps its CSS variables.
- The keys come from `keydown` on the screen element, the same element that already takes focus ([DemoReader.vue:46](website/.vitepress/theme/components/DemoReader.vue:46)). Paste comes from the `paste` event. The wheel becomes a scroll event. The viewport size comes from a `ResizeObserver`.
- The screen-reader text comes from `Session::text()`, so the live region says what the screen says.
- If the wasm does not load, the site keeps the current TypeScript reader. The fallback is not removed until the wasm has been on the site for a release.

## Milestones

Each milestone ends with something that can be checked. The order matters: the seams come before the browser, and the browser comes before the site.

### Milestone 0 — the spike, one day

Prove the three facts the plan rests on, and record the numbers.

- `rustup target add wasm32-unknown-unknown`, then compile a crate with `ratatui = { version = "0.29", default-features = false }` and a `Backend` that captures a `Buffer`. This proves ratatui core compiles and runs without crossterm.
- Compile the same spike with Ratzilla 0.2.0 alongside it, to see what it costs in bytes and in control.
- Record: the compressed wasm size of ratatui core alone, of ratatui plus the socli render layer, and the JS glue.

This milestone ends with a decision on paper, not in the code: own backend or Ratzilla, and the size budget for milestone 3.

### Milestone 1 — the crate becomes a library, two to three days

- Move the module declarations from [src/main.rs:3](src/main.rs:3) into `src/lib.rs`; the binary becomes a thin wrapper.
- Add the features and the optional dependencies. The native build keeps its behavior, byte for byte where it matters, and `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` all pass ([docs/CONVENTIONS.md:13](docs/CONVENTIONS.md:13)).
- The demo world's JSON is emitted at build time from Rust, so the site can serve real data to its current reader even before any wasm exists.

This milestone is worth doing on its own: it removes the drift between the site's posts and the binary's, with no browser involved.

### Milestone 2 — the seams, three to five days

- The key and mouse facade ([src/key.rs](src/key.rs)), with crossterm re-exported on the native side.
- The terminal facade: enter, leave, size, input modes, the background query, the watchdog.
- The reader's loop split into a reducer that takes events and returns a frame, and two runners: the native one and the browser one.
- The reader compiles with `native` off. That is the acceptance test: `cargo build --no-default-features --target wasm32-unknown-unknown` succeeds.

### Milestone 3 — the browser session, three to four days

- `src/web.rs`, the session, and the wasm exports.
- The key decoder, the paste path, the pointer path, the resize path.
- The frame diff and the plain-text mirror.
- A CI check that the wasm graph is clean: `cargo tree --target wasm32-unknown-unknown` lists no `crossterm`, `signal-hook`, `reqwest`, `viuer` or `ratatui-image`. `ssh-key` (ed25519 only) signs a dev/null device on the page.

### Milestone 4 — the site, two to three days

- The demo paints the session's cells; the keys, the touch buttons, the wheel and the resize are wired.
- The about panel and the live region describe the live reader, and they say what the page cannot do (X cookies, dev/null device signing, images).
- The fallback stays until the wasm has survived a release.
- The copy in [website/index.md](website/index.md) and [website/docs/reader.md](website/docs/reader.md) is corrected to say what the browser demo does and does not do.

### Milestone 5 — parity and budgets, two days

- The same fixture-driven state-transition tests run against the native reader and against the session. The fixtures are already shared: [src/fixtures.rs:42](src/fixtures.rs:42) holds `demo_state`, and [src/fixtures.rs:35](src/fixtures.rs:35) holds the frozen clock.
- A scripted key sequence (`j`, `j`, `k`, `enter`, `esc`, `d`, `?`) produces the same screen in both. The native side is captured through `TestBackend`, the browser side through the session, and the two are compared cell by cell.
- The compressed size of wasm plus JS is recorded in the release notes, against the budget set in milestone 0.

### Milestone 6 — the documentation, one day

- [docs/CONVENTIONS.md](docs/CONVENTIONS.md) gets the new module rows and the rules that the browser build follows.
- The README and the site say that the browser demo is the compiled binary.

## The browser demo contract

What the demo on the landing page must do, and what it must not do.

| | Behavior |
| --- | --- |
| Feed | signed-in timelines when this origin has an account. With none, the welcome screen. The last row still opens the offline demo: 18 posts, 17 of them in the home feed |
| Network | Bluesky, Mastodon, Threads, and public dev/null reads, through the worker. The wasm graph has no reqwest |
| Account | sign-in inside the reader for Bluesky, Mastodon, Threads and X. X uses a public client id. The local docs server forwards only `api.x.com`. The X cookie import and dev/null device signing stay in the terminal |
| Status row | the live status, or `demo` after the welcome row opens the demo |
| Writes | a signed-in network receives them. The demo still only changes the screen |
| Settings | a live change is written to the page config. The demo still says it is not saved |
| Keys | the list in [website/parts/keys.md](website/parts/keys.md), with the ones that need a terminal or a network refused with the same words the terminal uses |
| Split view | at 100 columns and above, the way the terminal does it. The landing page's stage is 92 columns wide, so the split appears on a wide screen and not on a phone. The page does not label the layout. Buttons above the reader set a mobile, tablet, or desktop window, and full screen opens `/web/` |
| Icons | Unicode. The site carries Geist Mono and no Nerd Font ([site.css:1](website/.vitepress/theme/styles/site.css:1)), so the browser uses the icon set that needs no special font. A Nerd Font webfont is a later, optional cost |
| Images | Rich mode draws profile pictures and post images on the page, including a signed-in timeline, by loading each image from its URL. `ui.images` set to `off` keeps the image card. The page does not speak kitty, iTerm2 or sixel |
| Threads | the reader worker. A page fetch runs there and the page paints again when it returns |
| Tokens | the page config in local storage on this origin, key `socli.config`. It can hold tokens. The terminal keeps its own file |

The honesty rule from [docs/CONVENTIONS.md:116](docs/CONVENTIONS.md:116) applies here too: in demo mode every write changes local state only, and the status says so.

## Tests

| Test | Where | What it proves |
| --- | --- | --- |
| The native suite, unchanged | `cargo test` | the refactor changed no behavior |
| The wasm build | `cargo build --no-default-features --target wasm32-unknown-unknown` | the reader compiles without a terminal, a filesystem or a network |
| The graph check | `cargo tree --target wasm32-unknown-unknown` | no native dependency entered the wasm graph |
| The shared fixtures | the session tests | the browser and the terminal run the same posts through the same reducer |
| Key parity | a scripted sequence against both backends | `j`, `k`, `enter`, `esc`, `d`, `?` and the search and compose paths do the same thing in both |
| The frame | the session's diff against a `TestBackend` capture | the browser sees the cells the terminal would draw |
| The site | a manual pass and a screenshot | the landing page shows the reader, the keys work, and the fallback still works with the wasm blocked |

## Build and CI

The artifact is built by Cargo and served by the site. The site does not gain a bundler.

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --no-default-features --target wasm32-unknown-unknown --lib
wasm-opt -Oz target/wasm32-unknown-unknown/release/libsocli.wasm -o website/public/wasm/socli.wasm
```

The library is built as an `rlib` and a `cdylib` at the same time, so Cargo writes `libsocli.wasm` next to `libsocli.rlib`. The site takes the wasm file; the rlib stays in the target directory.

Two choices to make in milestone 0:

- **wasm-bindgen or raw exports.** wasm-bindgen is the comfortable one, and it needs a version-pinned `wasm-bindgen-cli` in CI. Raw `extern "C"` exports over the wasm's linear memory need no tool at all, and the transport is bytes in both directions, so the glue stays small. The plan recommends raw exports unless milestone 0 shows the glue getting ugly.
- **Where the artifact lives.** `website/public/` is gitignored, and the site already needs it for its fonts and icons. Either that rule changes, or the artifact is built in CI and copied into the site before VitePress runs. The second is smaller and keeps a binary out of git.

The Pages workflow changes in three places:

- the path filter grows `src/**`, `Cargo.toml` and `rust-toolchain.toml`, so a Rust change reaches the site;
- the job installs the Rust toolchain and the wasm target and builds the artifact;
- the artifact is written into `website/public/wasm/` before `bun run docs:build`, so VitePress copies it into `dist`.

The base path is already handled: [config.ts:8](website/.vitepress/config.ts:8) reads `SITE_BASE`, so the asset URL has to be built from the same variable rather than hardcoded to `/`.

## Risks

| Risk | How it shows | What the plan does about it |
| --- | --- | --- |
| The refactor changes the reader | a native regression | the native suite is the gate at every milestone; the reader's loop is split before anything is moved |
| The wasm graph grows a native dependency | the browser build stops compiling, or worse, compiles and panics | the graph check in CI, and `native` off as the build gate |
| The browser shows a different reader than the terminal | the site drifts again, in a new direction | one code path: the session runs the same `draw`, and the parity test compares cells |
| The size budget is blown | a slow landing page | the budget is set from milestone 0's measurements, and CI reports the compressed size |
| The site is slow to load the wasm | a blank demo for a moment | the current reader stays as the fallback; the wasm loads after the first paint |
| Nerd Font glyphs are missing on the web | tofu boxes | the page bundles the Nerd Font symbols Ghostty uses. `ui.icons` set to `unicode` keeps the Unicode set |
| Split view never appears | the site claims something it does not show | the split needs 100 columns and the stage is 92; the demo states which layout it is in |
| A token reaches the browser | a public page holding an account | the config, tokens included, stays in this origin's local storage. The local docs server forwards only `api.x.com`. It does not forward pictures or any other host. X session cookies still cannot be attached |
| The demo drifts from the binary again | the site's posts differ from the binary's | the fixture JSON is generated by Rust, and the posts live in one place ([src/demo.rs](src/demo.rs)) |

## Out of scope

- **X session cookies and GitHub SSH signing in the browser.** The page cannot attach a Cookie header, so `socli login x` still imports that cookie in a terminal. Page sign-in for X is OAuth with a public client, and only where this origin forwards `api.x.com`. dev/null device approval runs on the page: the person pastes the `devnull://` callback, and the page signs the device request with ed25519. GitHub SSH signing still needs `ssh-keygen` and stays in the terminal.
- **A server PTY or an open proxy.** The published site is static. The local docs server forwards only `api.x.com`. It does not forward pictures or any other host.
- **Images in the browser.** Half-block graphics need pixels; the demo world uses `demo://` placeholders. A later milestone can carry real pictures for the demo posts.
- **The Raspberry build.** That is improvement 19, and [IMPROVEMENTS.md:509](IMPROVEMENTS.md:509) keeps the two milestones apart.
- **The `site/` and `design/` prototypes.** They are gitignored and stale. The wasm build replaces the first; the second is a design reference.

## Open decisions

Three calls belong to the owner.

1. **Own backend or Ratzilla.** The plan recommends the own backend, because the site owns its palette and its markup, and because Ratzilla's ratatui-0.29-compatible release is two versions old. Milestone 0 decides with numbers.
2. **The artifact in git or in CI.** The plan recommends CI, so no binary is committed and the site's fonts and icons problem is not made worse.
3. **The width of the demo.** At 92 columns the reader never splits. Either the landing page's stage grows to 140 columns on wide screens so the split view can be shown, or the demo says it is showing the single layout.

## Evidence index

| Claim | Where |
| --- | --- |
| The reader draws into a ratatui frame, and the tests already use `TestBackend` | [src/tui.rs:1107](src/tui.rs:1107), [src/tui.rs:4449](src/tui.rs:4449), [src/tui.rs:7384](src/tui.rs:7384) |
| The content model is styled spans, not ANSI | [src/render.rs:137](src/render.rs:137), [src/render.rs:417](src/render.rs:417), [src/render.rs:2530](src/render.rs:2530) |
| The same body feeds the reader and `--print` | [src/screens.rs:508](src/screens.rs:508), [src/plain.rs:121](src/plain.rs:121) |
| crossterm has no wasm32 target | crossterm 0.28.1 source: `event/sys/` holds only `unix.rs` and `windows.rs` |
| ratatui's crossterm feature is optional | ratatui 0.29.0 `Cargo.toml`: `default = ["crossterm", "underline-color"]` |
| Ratzilla 0.2.0 takes ratatui 0.29; 0.3.x takes 0.30 | crates.io dependency list for `ratzilla` |
| The event loop reads crossterm's global queue | [src/tui.rs:1232](src/tui.rs:1232), [src/tui.rs:1297](src/tui.rs:1297) |
| The split thresholds | [src/tui.rs:86](src/tui.rs:86), [src/tui.rs:88](src/tui.rs:88) |
| The demo world and its feeds | [src/demo.rs:104](src/demo.rs:104), [src/demo.rs:455](src/demo.rs:455), [src/state.rs:354](src/state.rs:354) |
| The shared fixtures and the frozen clock | [src/fixtures.rs:35](src/fixtures.rs:35), [src/fixtures.rs:42](src/fixtures.rs:42) |
| The website's demo is a second implementation | [reader.ts:102](website/.vitepress/theme/reader.ts:102), [reader.ts:83](website/.vitepress/theme/reader.ts:83), [reader.ts:933](website/.vitepress/theme/reader.ts:933) |
| The site reserves 92 columns and its own palette | [site.css:292](website/.vitepress/theme/styles/site.css:292), [site.css:393](website/.vitepress/theme/styles/site.css:393), [site.css:463](website/.vitepress/theme/styles/site.css:463) |
| The site builds from `website/**` only | [.github/workflows/pages.yml:6](.github/workflows/pages.yml:6) |
| The direction and the rules | [IMPROVEMENTS.md:441](IMPROVEMENTS.md:441), [IMPROVEMENTS.md:49](IMPROVEMENTS.md:49), [docs/CONVENTIONS.md:24](docs/CONVENTIONS.md:24) |
