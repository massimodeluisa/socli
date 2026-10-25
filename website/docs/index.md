---
title: Quick start
description: What socli is, and how to install it, open the demo, sign in and post.
---

# Quick start

socli reads X, Bluesky, dev/null, Threads and Mastodon in one full-screen terminal reader. Signed in to more than one network, it merges their home timelines into a single list of posts, and `--net` narrows the reader to one network or a comma list. `socli --demo` opens the same reader with an offline feed, before any account exists.

## Install

```sh
npx socli
```

Or run `bunx socli`. These commands require a [published release](https://github.com/massimodeluisa/socli/releases); if no release is listed, [build from source](install.html#from-git).

To use the bare `socli` commands in the examples below, install the command globally:

```sh
npm install --global socli
```

The [Install](install.html) page covers Homebrew availability, source builds, shell completions and icons.

## Open the demo

```sh
socli --demo
```

The demo opens an offline feed with posts from all five networks. It never reaches a network, and the header reads `demo`. Likes, replies, reposts and bookmarks in the demo change the screen and are never sent.

The demo never writes your `config.toml` either. A change in Settings, the theme picker, the network filter or the detail picker applies for the session, and the status row shows it with `demo, not saved`.

`j` and `k` move through the feed, `⏎` opens a thread, and `?` lists the keys. The [Reader](reader.html) page describes the screen, and [Keys](keys.html) covers every keyboard control.

## Sign in

```sh
socli login bluesky
socli auth status
```

Each network has its own sign-in command, and `socli auth status` shows what is configured without printing secrets. The [Sign in](login.html) page has the command and the flags for every network.

## Post

```sh
socli post "hello from the terminal" --net bluesky
```

Without `--net` or `SOCLI_NET`, `socli post` uses the network set by `socli auth default`, or the only signed-in network that can post.

## Use direct CLI commands

Read from the shell with a command, its arguments and the options you need:

```sh
socli feed saved --net bluesky --json
socli search "rust tui" --net bluesky --print --no-prompt
socli post "hello from the terminal" --net bluesky --dry-run
```

`--json` returns a JSON value and exits. `--print --no-prompt` prints terminal text once. Without either mode, read commands open the TUI at a terminal. The [CLI command reference](commands.html) lists all commands, their arguments and options, stdin input and exit codes.

## Documentation

| Page | What it covers |
| --- | --- |
| [Install](install.html) | Installation, builds, shell completions and fonts. |
| [Sign in](login.html) | Account setup and authentication options for each network. |
| [Reader](reader.html) | Feeds, threads, split view, profiles, composition and appearance. |
| [Keys](keys.html) | Shortcuts for the reader, composer, dialogs and Settings. |
| [CLI commands](commands.html) | Shell commands, arguments, options and the interactive prompt. |
| [Images](images.html) | Image viewing and terminal graphics modes. |
| [Web implementation](web.html) | The WebAssembly reader, what the page can sign in to, and what stays in the terminal. |
| [Networks](networks.html) | Supported feeds and actions, access requirements and limitations. |
| [Configuration](config.html) | Settings, defaults, file locations and environment overrides. |
| [Agents](agents.html) | Agent detection, JSON output, errors and exit codes. |
