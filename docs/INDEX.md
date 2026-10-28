# Documentation index

This page lists every document in the SoCLI repository, a thread-first terminal client for X, Bluesky, dev/null, Threads and Mastodon, and says when to read each one. Paths are relative to this file.

| Document | What it covers | When to read it |
| --- | --- | --- |
| [README.md](../README.md) | What SoCLI is, installing and building, signing in, the reader keys, one-shot commands, detail levels, images and icons, configuration and limits | To use SoCLI or to set it up |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | How to report an issue, open a pull request, and run the checks | Before opening an issue or a pull request |
| [Current TUI refinement](../design/improvements/REFINEMENT.md) | The original SoCLI visual language, refined in an [interactive preview](../design/improvements/index.html), with an original/refined comparison | To review current visual changes while preserving the existing TUI |
| [DESIGN.md](DESIGN.md) | The visual system: color tokens, the grid, the detail levels, the anatomy of every screen, the icon sets, hyperlinks and images | Before changing anything the user sees |
| [CONVENTIONS.md](CONVENTIONS.md) | The toolchain, the module map, rendering rules, errors, tests, configuration and environment variables, commits and documentation | Before changing code |
| [RELEASING.md](RELEASING.md) | Release tags, versions, the changelog, platform packages, npm, Homebrew, Pages, and publishing setup | Before preparing or troubleshooting a release |
| [REVIEW.md](REVIEW.md) | Source review coverage, corrected failure behavior, verification and remaining boundaries for 0.1.3 | To assess the reliability changes and their limits |
| [packages/cli/README.md](../packages/cli/README.md) | The native npm launcher, `npx`, `bunx`, and the six bundled executables | To install or package the terminal client through npm |
| [packages/web/README.md](../packages/web/README.md) | soclib's mounted and headless reader APIs, host callbacks, assets, and browser checks | To embed the reader in a browser application |
| [agents.md](agents.md) | How an AI agent or a script drives SoCLI: agent detection, JSON shapes, scoped ids, errors, exit codes and sign-in | Before calling SoCLI from a script or an agent |
| [agents/socli/SKILL.md](agents/socli/SKILL.md) | A Claude Code skill that teaches an agent the SoCLI commands and the rules for writing safely | To install the skill into `~/.claude/skills/socli/` |
| [integration/README.md](integration/README.md) | The network layer: the capability matrix, scoped ids and target formats, the merged timeline, network colors, and how to add a network | Before changing or adding a network |
| [integration/x/README.md](integration/x/README.md) | X: capabilities, limits and cost, status, and links to its auth, endpoints, mapping and testing pages | Before working on X |
| [integration/bluesky/README.md](integration/bluesky/README.md) | Bluesky: capabilities, limits, status, and links to its auth, endpoints, mapping and testing pages | Before working on Bluesky |
| [integration/devnull/README.md](integration/devnull/README.md) | dev/null: capabilities, access levels, status, and links to its auth, endpoints, mapping and testing pages | Before working on dev/null |
| [integration/threads/README.md](integration/threads/README.md) | Threads: capabilities, limits, status, and links to its auth, endpoints, mapping and testing pages | Before working on Threads |
| [integration/mastodon/README.md](integration/mastodon/README.md) | Mastodon: capabilities, the per-server limit, status, and links to its auth, endpoints, mapping and testing pages | Before working on Mastodon |
| [integration/tieba/README.md](integration/tieba/README.md) | Why Baidu Tieba is not integrated and what an integration would need | Before reopening the Tieba question |
| [integration/reddit/README.md](integration/reddit/README.md) | Why Reddit is not integrated and what an integration would need | Before reopening the Reddit question |
| [design/index.html](../design/index.html) | The gallery of mockup screens, with a switch between the dark and light themes | To see how a screen is meant to look |
| [design/screenshots/](../design/screenshots/) | The rendered PNG of every mockup, one folder per theme (`dark`, `light`, `mono`) | To compare one screen of the reader with its mockup |
| [website/](../website/) | The VitePress source of the website, built by `.github/workflows/pages.yml` and deployed by the release workflow | Before changing the website |
| [website/docs/index.md](../website/docs/index.md) | The public quick start and documentation index | To find user guides and references |
| [website/docs/commands.md](../website/docs/commands.md) | Direct CLI commands, arguments, options, stdin, output modes and the interactive prompt | To run SoCLI from a shell or script |
| [website/docs/keys.md](../website/docs/keys.md) | Reader shortcuts and composer, dialog and Settings controls; the reader table is also included on the homepage | To look up a key or update the shared shortcut reference |
| [website/docs/web.md](../website/docs/web.md) | The WebAssembly reader, what the page can sign in to, and what stays in the terminal | To understand the browser version |

- [Profile redesign](../design/profile.html) compares the new profile with the previous fixture, while [native recordings](../design/profile-evidence/index.html) show four terminal sizes in all four detail levels.
- [Installed CLI and source comparison](../design/improvements/COMPARISON.md) records the current evidence and interaction recommendations.
- [Interactive design study](../design/improvements/evolution.html) preserves the existing post anatomy and explores the welcome screen, composer and palette.

- [Post action comparison](../design/action-spacing.html) shows the corrected spacing, equal action targets and repost glyph in Nerd Font and Unicode modes.
