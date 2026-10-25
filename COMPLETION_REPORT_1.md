# socli — Project Status Evaluation

**Evaluated:** 22 October 2026
**Repository:** `/Users/massimodeluisa/Git/PersonalProjects/xcli` (directory `xcli`, package `socli`)
**Commit evaluated:** `4b56b43` *"feat: add socli with a real X browser session"* (2026-10-21 20:47 +0200)
**Working tree:** dirty — 27 modified files + 1 untracked file (`src/onboard.rs`)

---

## 1. Executive summary

socli is a **feature-complete, working, well-tested Rust TUI** for five social networks, and it is **not yet released**. The codebase is healthy: it builds in debug and release, clippy is clean with zero warnings, rustfmt reports zero diffs, and all **671 tests pass**. The VitePress website builds. Documentation is unusually thorough for a 0.1.0 project.

The blocking issues are **process**, not code:

| # | Blocker | Severity |
|---|---|---|
| 1 | ~2,900 lines of finished, working, **uncommitted** work on `main` with no branch | **Critical** |
| 2 | **No Rust CI** — only a Pages workflow exists; build/test/lint are manual | **High** |
| 3 | **No release process** — no tags, no release workflow, no CHANGELOG, v0.1.0 never published | **High** |
| 4 | `website/public/` is **gitignored** but referenced by `.vitepress/config.ts` → clean-clone Pages deploy ships without favicon/OG image | **High** |
| 5 | 18 doc links point into **gitignored** paths (`design/`, `IMPROVEMENTS.md`, `site/`) → broken for anyone who clones | **Medium** |
| 6 | `src/tui.rs` is a 9,809-line monolith (20% of the crate) | **Medium** |

Feature-wise the crate is well ahead of its own improvement plan: **11 of the 20 prompts in `IMPROVEMENTS.md` are verifiably implemented**, including every P1 correctness item. The two large remaining milestones (WASM browser target, Raspberry Pi ARM lite build) are explicitly scoped as separate, later work and have **not** been started.

---

## 2. Project identity

| Field | Value |
|---|---|
| Package / binary | `socli` |
| Version | `0.1.0` |
| License | MIT (`LICENSE`, `THIRD-PARTY-NOTICES.md` for the 27 borrowed palettes) |
| Edition / MSRV | 2021 / `rust-version = "1.89"` |
| Toolchain | pinned `1.99.1` via `rust-toolchain.toml` (minimal + rustfmt + clippy) |
| Description | Thread-first terminal client for X, Bluesky, dev/null, Threads and Mastodon |
| Homepage / repository | `https://socli.sh` · `https://github.com/massimodeluisa/socli` (declared; see §7.6) |
| Configured remote | `origin → https://origin.cursor.com/git/massimodeluisa/xcli` (no GitHub remote) |

The checkout directory is still `xcli` while the crate was renamed to `socli` in commit `f4206ca`. `IMPROVEMENTS.md` acknowledges this explicitly ("The package and executable are named `socli`; the checkout directory is still `xcli`").

---

## 3. Build, test and lint health (all verified locally)

| Check | Command | Result |
|---|---|---|
| Debug build | `cargo build` | ✅ pass (3.4 s incremental) |
| Release build | `cargo build --release` | ✅ pass (47.5 s) |
| Lint | `cargo clippy --all-targets` | ✅ pass, **0 warnings** |
| Format | `cargo fmt --check` | ✅ pass, **0 diffs** |
| Unit tests | `cargo test` (in-crate) | ✅ **623 passed**, 0 failed, 0 ignored |
| Integration tests | `cargo test` (`tests/cli.rs`) | ✅ **48 passed**, 0 failed, 0 ignored |
| **Total** | | ✅ **671 tests, 0 failures** |
| Website build | `bun run docs:build` | ✅ pass (after `bun install`) |

**Runtime characteristics**

- Release binary: **8.8 MB** (LTO thin, `codegen-units = 1`).
- Cold start: **~15 ms** per `socli --demo --print` invocation (0.077 s for 5 runs).
- No-account exit code is `1`; demo runs exit `0`. Consistent with `docs/agents.md`.

**Smoke verification performed**

- `socli --demo --print --width 100 --no-icons` renders the merged feed, network chips, feed tabs, thread rails, link cards and stat rows correctly.
- `socli --help` in agent mode emits the machine-readable command table (29 commands).
- `socli auth status --json` emits correct per-network signed-in flags.
- The **new signed-out welcome screen** was driven in a real 100×30 pty (custom `pty`/`TIOCSWINSZ` harness): it renders the `socli > login` breadcrumb, the So_ wordmark, animated ASCII water-ripple frames, per-network character-raster logos, and the six choices (Explore the demo, X, Bluesky, dev/null, Threads, Mastodon) with `↑↓ choose / enter connect` hints. **The uncommitted feature is confirmed working, not half-built.**

> Note: the website build failed on first attempt with `Cannot find package 'rollup'`. This was a stale `website/node_modules` in the working copy, not a repo defect — `bun install --frozen-lockfile` resolved it and the build then succeeded.

---

## 4. Codebase scale and structure

- **60 Rust files**, **50,283 lines** in `src/`.
- **163 tracked files** total (84 `src`, 35 `website`, 35 `docs`, plus root files).

**Largest modules**

| File | Lines | Inline tests |
|---|---:|---:|
| `src/tui.rs` | 9,809 | 113 |
| `src/render.rs` | 3,845 | 49 |
| `src/state.rs` | 3,125 | 24 |
| `src/main.rs` | 2,096 | — |
| `src/theme.rs` | 1,657 | 26 |
| `src/screens.rs` | 1,651 | 13 |
| `src/net/x/web.rs` | 1,465 | — |
| `src/net/x/client.rs` | 1,104 | 7 |
| `src/chrome.rs` | 1,083 | 14 |
| `src/preview.rs` | 1,027 | 18 |

Test density is concentrated where the complexity is: `tui.rs` 113 tests, `render.rs` 49, `unfurl.rs` 30, `theme.rs` 26, `state.rs` 24, `hyperlink.rs` 24.

**Public surface**: 29 CLI commands (`feed`, `thread`, `search`, `user`, `notifications`, `bookmarks`, `ask`, `post`, `reply`, `quote`, `like`/`unlike`, `repost`/`unrepost`, `bookmark`/`unbookmark`, `share`, `follow`/`unfollow`, `delete`, `view`, `login`, `logout`, `whoami`, `auth`, `config`, `drafts`, `completions`, `help`), 31 themes, 4 detail levels, 3 density levels, 3 layouts, 5 networks.

---

## 5. Uncommitted work — the critical finding

```
 27 files changed, 2,199 insertions(+), 451 deletions(-)   (tracked files)
 ?? src/onboard.rs                                        (703 lines, untracked)
```

Total uncommitted delta ≈ **2,900 insertions**. This is not scratch work — it is a coherent, tested, documented feature set:

| Area | What changed |
|---|---|
| `src/onboard.rs` (new) | Signed-out welcome screen: animated radial water ripples, per-network character-raster logos, random glitch bands, ASCII fallback in plain mode, compact menu down to the 24×8 minimum |
| `src/state.rs` | `SettingsPage` enum (Main/Accounts), `gate_row`, `page_gate`, `show_page_card`, `enter_demo()`, `adopt_accounts()`, `temporary_status()` |
| `src/tui.rs` | Welcome-screen key handling (`j/k`, Home/End, Enter), Accounts page, in-reader sign-in/out with terminal suspend/resume, auto-paging loading card, `should_draw()` idle-redraw gate |
| `src/screens.rs` | New `So_` masthead mark, redesigned sign-in frame with gutter/rails/aside column |
| `src/render.rs`, `src/chrome.rs`, `src/layout.rs` | Medium level draws to the edges at every width; new geometry |
| `src/settings.rs` | Accounts row/page wiring |
| `src/theme.rs` | `standard` → **`medium`** rename (old name still parses) |
| `src/main.rs` | The reader opens the welcome gate instead of erroring when signed out and on a terminal |
| `src/net/*/auth.rs` (all 5) | Refactored sign-in flows for the in-reader path |
| `docs/DESIGN.md`, `docs/CONVENTIONS.md`, `README.md`, `website/**` | Documentation updated **in step with** the code |
| `tests/cli.rs` | Integration expectations updated |

**Assessment:** the work is production-shaped and verified. The risk is purely that it lives only on disk. A single `git checkout .`, `git stash drop`, or a fresh clone loses the entire feature.

---

## 6. Feature completeness by network

Source: `docs/integration/<network>/README.md` (status sections) plus code inspection.

| Network | Auth | Reads | Writes | Known gaps |
|---|---|---|---|---|
| **X** | Browser-session import (Chrome/Brave/Edge/Arc/Chromium) **or** OAuth 2.0 + PKCE on `127.0.0.1:8741` | For you, Following, Latest, Bookmarks, mentions, search, profiles, threads, counts | post, reply, quote, delete, like, repost, bookmark, follow | No trending feed; no native quotes on the official API; no paging beyond the requested count; no filtered stream / full-archive search |
| **Bluesky** | App password (stdin-capable) | Full capability table, unit-tested against recorded responses | post, reply, quote, like, repost, follow | OAuth not built; search blocked for signed-out clients; no image upload; no DMs/lists; text-only posts |
| **dev/null** | Device key approval (pasted `devnull://` callback) **or** GitHub SSH key | Public reads, 3 log feeds, notifications, threads, profiles, stars, follows | post, reply, delete, follow | Quoting needs the approved device; no bookmarks (upstream removed them on 2026-10-20, upstream commit `230c004a`); short commit hashes not resolved |
| **Threads** | Token paste-in **or** browser flow with pasted redirect; auto token refresh | Threads feed, mentions, keyword search, post by id, conversations (5×100 replies), own profile | post, reply, native quote, repost, delete | No home/discover timeline, no likes, no bookmarks, no follows, no undo-repost, no other profiles, no counts/views, no post links as input |
| **Mastodon** | Browser PKCE (app registered on the server) **or** manual token; per-server post limit | Threads and profiles without an account; feeds with one | post, reply, quote, delete, like, repost, bookmark, follow | **Never tested against a live account by the owner**; no media upload, no poll voting, no content warning or visibility on new posts; no lists/scheduled posts/edits/filters |
| **Reddit** | — | — | — | Deliberately not integrated; documented in `docs/integration/reddit/README.md` |
| **Tieba** | — | — | — | Deliberately not integrated; documented in `docs/integration/tieba/README.md` |

---

## 7. Gaps, risks and findings

### 7.1 Critical — uncommitted work on `main`
No branch, no stash-safe point, ~2,900 lines of finished feature code plus its documentation. **Fix immediately.**

### 7.2 High — no Rust CI
`.github/` contains exactly one workflow, `pages.yml`, which builds the website. There is **no** workflow that runs `cargo build`, `cargo test`, `cargo clippy` or `cargo fmt`. For a 50k-line binary crate with 671 tests, this is the single largest process gap — regressions can only be caught manually.

### 7.3 High — no release path
- `git tag -l` → empty. No version has ever been published.
- No release workflow, no `cargo-dist`/`cargo-release` config, no CHANGELOG.
- `Cargo.toml` still says `0.1.0`; `src/screens.rs` embeds `env!("CARGO_PKG_VERSION")`.
- The 8.8 MB release artifact exists only as a local build.

### 7.4 High — `website/public/` is gitignored but required by the site
Root `.gitignore` line 9 ignores `/website/public/`, but `.vitepress/config.ts` references `favicon.ico`, `favicon.svg`, `favicon-32.png`, `favicon-16.png`, `og.png` and `icon-512.png` from it. `git ls-files website/public` → **0 files**. A clean-clone CI Pages deploy therefore publishes a site with **no favicon and no social preview image**. The website build still succeeds locally only because the ignored files happen to exist on disk.

### 7.5 Medium — documentation links into gitignored trees
`design/`, `IMPROVEMENTS.md` and `site/` are gitignored, yet **18 links** in `docs/INDEX.md`, `docs/DESIGN.md` and `README.md` point into them. Every one of these is broken for a fresh clone. (`docs/INDEX.md` itself calls `site/` "no longer deployed and kept until it is removed" — a stale artifact.)

### 7.6 Medium — repository and install-path mismatch
`README.md` tells users `git clone https://github.com/massimodeluisa/socli && cd socli`, and `Cargo.toml` declares that repository/homepage. The only configured remote is `origin.cursor.com/git/massimodeluisa/xcli`, and the local directory is `xcli`. **The published install instructions do not match the actual remote**, and the checkout directory contradicts the crate name.

### 7.7 Medium — `src/tui.rs` monolith
9,809 lines = **~20% of the crate**, mixing input handling, composer, panes, key routing, animation, worker threads and 113 tests. `render.rs` (3,845) is the same pattern. This is the main maintainability risk as the feature set grows.

### 7.8 Medium — live-path verification gaps
Mastodon has never been exercised against a live account. The X OAuth path needs a developer app of the owner's own. Threads needs a Meta app. The Bluesky OAuth path is deliberately absent. These are documented honestly in `docs/integration/*/README.md`, but they cap confidence in the write paths.

### 7.9 Low — `standard` → `medium` rename
`ui.detail` / `SOCLI_DETAIL` now report `medium`. The old value still parses, and `config set ui.detail standard` normalises to `medium`, so it is backwards compatible — but it is a user-visible rename worth calling out in release notes.

### 7.10 Low — stale operator instruction
`IMPROVEMENTS.md` states "Do not run typechecking under the current operator rule." That rule belonged to the review session; the crate now builds, lints, formats and tests cleanly, so the instruction is obsolete and should be removed to avoid confusing future contributors.

### 7.11 Low — two documented feature plans not started
`docs/plans/direct-messages.md` and `docs/plans/profile-editing.md` are written design documents with explicit "must hold before it ships" gates. Neither has code. This is correctly sequenced, but they are the next logical scope after the current release.

---

## 8. Implementation status of the 20 improvement prompts

`IMPROVEMENTS.md` (reviewed 2026-10-21) defines a delivery order: **P1 first** (06–08, 10–11, 14), then layout/resource limits (02, 09, 12–13), then hierarchy and command discovery (01, 03–05, 15–17), then the platform milestones (18–19), with the fixture suite (20) built alongside each fix.

| # | Prompt | Priority | Status | Evidence |
|---|---|---|---|---|
| 01 | Refine post focus and section hierarchy | P2 | Design only | Interactive preview `design/improvements/index.html` |
| 02 | Render the real terminal width | P2 | ✅ **Done** | Medium draws to the edges at all widths; narrow rules re-documented in `docs/DESIGN.md` |
| 03 | Refine split reading, keep post anatomy | P2 | Partly | Split view exists; compact-split refinement is design-only |
| 04 | Make detail changes quiet and predictable | P2 | ✅ **Done** | Session-only level change, `saved` status, placement preserved |
| 05 | Make the command surface searchable | P2 | Partly | `:` / `ctrl-k` command line exists; palette is not yet the searchable Ctrl/Cmd+K surface |
| 06 | Accept paste and edit complete graphemes | **P1** | ✅ **Done** | `Event::Paste` handled in both loops; `Input` uses `unicode-segmentation` grapheme boundaries; one paste = one undo step; `clean_paste()` strips escape-like bytes |
| 07 | Publish exactly the intent shown in the composer | **P1** | ✅ **Done** | New `src/intent.rs` (`Intent`: body, targets, reply, quote, draft id); composer returns the `Intent`; `send_intent()` consumes exactly it |
| 08 | Durable draft and per-network send outcome | **P1** | ✅ **Done** | `src/send.rs` persists intent before sending, records per-target success/failure/unknown, drafts marked `may have posted` |
| 09 | Keep reading while requests run | P2 | ✅ **Done** | Scoped worker thread, 150 ms `loading…`, keys dropped while busy, `ctrl-c` quits |
| 10 | Share grapheme layout, isolate terminal controls | **P1** | ✅ **Done** | Shared `unicode-width` / `unicode-segmentation` layout helpers; control chars isolated from display text |
| 11 | One policy for media fetches and decoding | **P1** | ✅ **Done** | `src/media.rs` refuses private/loopback/link-local/local-file sources **before any I/O**, checks every redirect hop, 8 MiB (unfurl) / 20 MiB (typed) caps |
| 12 | Bound preview memory, queues, disk retention | P2 | Partly | Preview/download caps present; explicit queue-depth and on-disk retention budgets not implemented |
| 13 | Stop rebuilding unchanged screens while idle | P2 | ✅ **Done** | `should_draw(dirty, moving, since_frame)` gates repaints; idle refresh interval |
| 14 | Serialize complete configuration and draft transactions | **P1** | ✅ **Done** | Locked read-modify-write with atomic replace; concurrency tests in `config.rs` and `drafts.rs` |
| 15 | Describe local search honestly | P2 | ✅ **Done** | `docs/DESIGN.md`: "a local search over the posts already loaded" |
| 16 | Keep machine output a stable contract | P2 | ✅ **Done** | `--json` everywhere, agent auto-detection (`src/agent.rs`), one JSON line per error, exit codes in `docs/agents.md` |
| 17 | Capability and empty states guide the next action | P2 | Partly | Per-network capability tables documented; reader empty states are still mostly plain notices |
| 18 | Shared Rust core for a browser demo (WASM) | P2 | ❌ **Not started** | No `wasm-bindgen`, no `crate-type = ["cdylib"]`, no feature gates, no Ratzilla spike |
| 19 | Explicit Raspberry build and resource contract | P2 | ❌ **Not started** | No ARM cross profile, no measured cold start / idle CPU / peak RSS, no named board |
| 20 | Review scenes as a regression fixture suite | P2 | Partly | 671 unit + integration tests and `src/fixtures.rs`; no golden-file screen-fixture suite for the review scenes |

**Tally: 11 fully done, 5 partial, 2 not started, 2 design-only.** Every P1 correctness/trust-boundary item is complete.

---

## 9. Documentation status

**Strong.** The documentation is a genuine asset of this project.

| Document | Coverage |
|---|---|
| `README.md` | Install, sign-in per network, reader keys, one-shot commands, detail levels, split view, narrow terminals, images/icons, themes, config, AI agents, limits |
| `docs/INDEX.md` | Index of every document with a "when to read it" column |
| `docs/DESIGN.md` | Visual system: color tokens, grid, detail levels, screen anatomy, icon sets, hyperlinks, images, animations, sign-in screens |
| `docs/CONVENTIONS.md` | Toolchain, module map, rendering rules, errors, tests, config keys, env vars, commit and doc rules |
| `docs/agents.md` | Agent detection, JSON shapes, scoped ids, exit codes, sign-in limits |
| `docs/agents/socli/SKILL.md` | Installable Claude Code skill |
| `docs/integration/<5 networks>/{README,auth,endpoints,mapping,testing}.md` | Per-network capabilities, limits, endpoints, field mapping, test procedure |
| `docs/integration/{reddit,tieba}/README.md` | Why those networks are not integrated |
| `docs/plans/{direct-messages,profile-editing}.md` | Scoped plans for the next two features |
| `website/` | VitePress site (10 doc pages + `llms.txt` / `llms-full.txt` generation) |

Uncommitted doc edits were made **in step with** the code (the new welcome screen, accounts page, `medium` level and paging card are all documented in `docs/DESIGN.md`). The only documentation defects are the broken links of §7.5.

---

## 10. Recommended next steps

**Immediate (this session)**

1. **Commit the work.** `git switch -c feature/welcome-accounts`, `git add -A` (including `src/onboard.rs`), commit as a single coherent change. Nothing else matters until this is safe.
2. **Un-ignore `website/public/`** and commit the brand/favicon/OG assets, or relocate them to a tracked directory and update `.vitepress/config.ts`. Verify with a clean-clone build.

**Before any release**

3. **Add Rust CI** — `.github/workflows/ci.yml`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`, on `ubuntu-latest` and `macos-latest`, toolchain `1.99.1` (or `rust-toolchain.toml`).
4. **Set up releases** — `cargo-dist` or `cargo-release`, a tag-triggered workflow, a CHANGELOG, and the first `v0.1.0` tag. Confirm the GitHub remote in §7.6 first, and make `README.md` clone instructions match reality.
5. **Resolve the gitignored-document links** — either commit `design/` and `IMPROVEMENTS.md` (they are referenced from 18 places and are substantial review artifacts), or rewrite those links to point at the VitePress site. Delete the stale `site/` tree.

**Next development cycle**

6. **Close prompt 17** (capability and empty states that guide the next action) and **prompt 20** (golden screen fixtures for the review scenes) — both are small and raise user-visible quality.
7. **Split `src/tui.rs`** into `input/`, `composer/`, `panes/`, `keys/`, `animate/` modules, with the 113 inline tests moved into `mod tests` files. Target: no module above ~2,000 lines.
8. **Live-verify Mastodon** and write down exactly which X-OAuth and Threads flows require the owner's own developer apps, so the docs state the real constraints.
9. **Prompts 18 and 19** (WASM core, ARM lite build) as separately scoped milestones — extract platform boundaries first, per `IMPROVEMENTS.md`'s own guidance, and do not let them block the 0.1.0 release.

---

## 11. Verdict

| Dimension | Rating | Note |
|---|---|---|
| Build health | ✅ Green | debug + release, clippy 0 warnings, fmt 0 diffs |
| Test health | ✅ Green | 671 tests, 0 failures, good density on the complex modules |
| Feature breadth | ✅ Green | 5 networks, 29 commands, full TUI + print + JSON surfaces, themes, images, agent mode |
| Design & docs | ✅ Green | unusually complete; docs updated in step with code |
| Release readiness | 🟡 Amber | builds and tests, but no tag, no release automation, no CHANGELOG |
| Automation | 🔴 Red | **zero Rust CI** on a 50k-line crate |
| Version control hygiene | 🔴 Red | ~2,900 lines of finished work uncommitted on `main` |
| Repo hygiene | 🔴 Red | ignored `website/public/` breaks the deploy; 18 doc links break on clone |
| Architecture | 🟡 Amber | correct layering overall, but a 9.8k-line `tui.rs` and 3.8k-line `render.rs` |
| Scope control | 🟡 Amber | the two platform milestones (18/19) are unstarted and unbounded in cost |

**Bottom line.** socli is a strong, near-release 0.1.0. The engineering is ahead of the plan — every P1 correctness item in the review is implemented and tested — and the remaining work on the roadmap is scope expansion (browser target, Raspberry), not defect repair. The project's biggest current risk is not code quality but **operational**: a large uncommitted feature set, no CI, no release automation, and gitignore rules that break both the deployed site and the documentation for anyone who clones. Items 1–5 in §10 are a single focused day of work and would move the project from "not released" to "releasable".

---
*Report generated 22 October 2026 from the working tree at `4b56b43` plus uncommitted changes. All commands in §3 were executed locally; the pty verification in §3 used a synthetic 100×30 terminal.*
