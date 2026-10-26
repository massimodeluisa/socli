---
title: Agents
description: How SoCLI behaves when an AI coding agent or a script drives it.
---

# Agents

An AI coding agent or a script drives SoCLI through one-shot commands that print JSON and exit, and never through the full-screen reader. SoCLI turns that behavior on by itself when it recognizes an agent, and `--json` turns it on for any caller. This page describes how SoCLI detects an agent, what changes in agent mode, the JSON each command prints, how to chain one command's output into the next, how errors and exit codes work, and how an agent can sign in. Every sample below was captured from a `--demo --json` run, so the ids and URLs are fixtures and nothing was sent to a network.

This site also serves `llms.txt` and `llms-full.txt`, which hold every docs page as plain Markdown.

## Agent detection

`src/agent.rs` decides whether a process is run by an agent. It checks, in this order:

1. `AI_AGENT`, with any non-empty value. The value becomes the agent's name.
2. One variable per known agent, in the order of the table below.
3. The file `/opt/.devin`, which marks Devin.

| Agent | Variables |
| --- | --- |
| `cursor-cli` | `CURSOR_AGENT`, or `CURSOR_EXTENSION_HOST_ROLE=agent-exec` |
| `gemini` | `GEMINI_CLI` |
| `codex` | `CODEX_SANDBOX`, `CODEX_CI`, `CODEX_THREAD_ID` |
| `antigravity` | `ANTIGRAVITY_AGENT` |
| `augment-cli` | `AUGMENT_AGENT` |
| `opencode` | `OPENCODE_CLIENT` |
| `claude` | `CLAUDECODE`, `CLAUDE_CODE` |
| `github-copilot` | `COPILOT_MODEL`, `COPILOT_ALLOW_ALL`, `COPILOT_GITHUB_TOKEN` |

`CURSOR_TRACE_ID` and `REPL_ID` are deliberately not on the list, because the terminals a person opens in Cursor and Replit carry them too, and treating those people as agents would take their reader away.

`SOCLI_AGENT=1` forces agent mode and `SOCLI_AGENT=0` turns it off, whatever the other variables say. Any other value, or no value, leaves detection on. A script that wants the same behavior without being detected can pass `--json` on every call.

## What changes in agent mode

Agent mode makes SoCLI safe to run with nobody at the terminal:

- Output is JSON by default. `--print` asks for the text form instead, and `--print --no-prompt` leaves out the `SoCLI>` prompt.
- SoCLI never opens the reader, never prompts and never reads a key. A command that would ask a question fails with a usage error instead.
- Text for `post`, `reply` and `quote` has to be given as an argument, or as `-` to read standard input. An empty command line does not read standard input in agent mode.
- `delete` needs `--yes`, because there is nobody to answer `y/N`.
- Errors print as one JSON line on stderr (see [Errors and exit codes](#errors-and-exit-codes)).
- Output carries no color, no OSC 8 hyperlinks and no Nerd Font glyphs, and `--print` uses Unicode icons.
- The sign-ins that need a browser refuse to run (see [Signing in](#signing-in)).

Without a command, `SOCLI_AGENT=1 socli` prints the home feed as JSON, which is the same as `socli feed --json`.

## One-shot commands

`socli help` lists every command, `socli help <command>` shows the flags and examples of one, and `socli help --json` prints the whole table as JSON, so an agent can read the current surface instead of this page. The commands an agent uses most:

| Command | What it does |
| --- | --- |
| `socli feed [following\|latest\|trending\|discover\|saved\|mine\|notifications]` | The home timeline, or one named feed on the networks that have it |
| `socli thread <id>` | A post with its replies |
| `socli search <query>` | Recent public search on the networks that can search |
| `socli user <handle>` | Public posts from one account |
| `socli notifications`, `socli bookmarks` | Mentions, and saved posts |
| `socli post <text>`, `reply <id> <text>`, `quote <id> <text>` | Publish, reply, quote |
| `socli like`, `repost`, `bookmark`, `follow <id or handle>` and their `un…` forms | Toggle one state |
| `socli delete <id> --yes` | Delete one of your posts |
| `socli whoami`, `socli auth status` | The accounts in use and what is configured |
| `socli config list\|get\|set\|path\|themes`, `socli drafts [rm <n>]` | Settings and saved drafts |

`--net` takes `x`, `bluesky`, `devnull`, `threads`, `mastodon` (or `masto`), a comma list or `all`. Without it, `SOCLI_NET`, then `feed.networks`, then every network applies, and `post` uses the network set by `socli auth default`. A reply goes only to the parent post's network. `--count <n>` sets how many posts each network is asked for (15 by default; the demo ignores it and always returns its whole feed), and each X post returned is billed, so an agent should not page on X without a reason.

## Scoped ids

Every post id in JSON has the form `<network>:<native id>`, such as `x:1849213377`, `bluesky:at://…`, `devnull:p2` or `mastodon:115000000000000001`. Pass that string back to `thread`, `reply`, `quote`, `like`, `repost`, `bookmark` and `delete`. A bare native id needs `--net` to say which network it belongs to, and a post link is accepted in place of an id. To chain commands, read `id` from one command's JSON and pass it to the next:

```sh
ID=$(socli --json feed --net bluesky | jq -r '.[0].id')
socli --json thread "$ID"
socli --json reply "$ID" "agreed" --dry-run
```

The ids in the first command's output are scoped already, so the second and third commands need no `--net`.

## JSON shapes

### Posts

Reads (`feed`, `thread`, `search`, `user`, `notifications`, `bookmarks`) print one array of posts, built by `Post::to_json` in `src/model.rs`. This is one post from `socli --demo --json feed`:

```json
{
  "author": {
    "avatar": "demo://avatar/nuno",
    "description": "Shipping the reader.",
    "id": "devnull:u2",
    "name": "Nuno Park",
    "username": "nuno",
    "verified": true
  },
  "bookmarked": false,
  "conversation_id": "devnull:c9",
  "created_at": "2026-10-20T18:37:47.585019+00:00",
  "id": "devnull:p2",
  "in_reply_to": null,
  "liked": false,
  "links": [
    {
      "display": "example.com/socli",
      "expanded": "https://example.com/socli",
      "image": "demo://og-wide",
      "short": "https://example.com/socli",
      "site": null
    }
  ],
  "media": [],
  "metrics": {
    "bookmarks": 0,
    "likes": 86,
    "quotes": 0,
    "replies": 5,
    "reposts": 0,
    "views": 0
  },
  "network": "devnull",
  "permalink": "https://dev-null.sh/@nuno/commits/p2",
  "quoted": null,
  "reposted": false,
  "reposted_by": null,
  "text": "ok devs, assemble. the reader is up.\n\nnotes: https://example.com/socli\n\nmake some noise.",
  "url": "https://dev-null.sh/@nuno/commits/p2"
}
```

The fields mean the following.

| Field | Meaning |
| --- | --- |
| `id`, `network` | The scoped id and the network slug |
| `url`, `permalink` | The post's link on its network (both hold the same value) |
| `conversation_id`, `in_reply_to` | The thread the post belongs to, and its parent's scoped id or `null` |
| `reposted_by` | The handle that reposted it into your timeline, or `null` |
| `created_at` | An RFC 3339 time, or `null` |
| `text` | The post text |
| `author` | `id`, `name`, `username`, `verified`, `avatar` (a URL, or `demo://avatar/<user>` in the demo) and `description` |
| `metrics` | `replies`, `reposts`, `likes`, `quotes`, `bookmarks` and `views` as numbers; a network without a count reports `0` |
| `liked`, `bookmarked`, `reposted` | Whether the signed-in account did it |
| `links` | `short`, `expanded`, `display`, and `image` and `site` when the network or the link page gave a card |
| `media` | `key`, `type`, `url`, `preview`, `alt`, `width` and `height` per picture |
| `quoted` | The quoted post as the same object, or `null` |
| `cid` | Bluesky only: the record's content id, omitted on other networks |

Key order is alphabetical and is not part of the contract.

### Write results

`post`, `reply`, `quote`, `like`, `repost`, `bookmark`, `follow`, `delete` and their undo forms print one array with one object per network that was addressed:

```json
[
  {
    "network": "bluesky",
    "ok": true,
    "id": "bluesky:local-17",
    "url": "https://bsky.app/profile/you.bsky.social/post/local-17",
    "demo": true,
    "text": "ship it"
  }
]
```

`network` and `ok` are always present. `id` and `url` appear when the network returned them, `handle` names the account of a follow or unfollow, `text` is the text a demo or dry-run post would send, and `demo` is `true` when nothing was sent because of `--demo`. When a network refuses, its object has `"ok": false` and an `error` line, and the process exits 1:

```json
[
  {
    "network": "threads",
    "ok": false,
    "error": "Threads has no likes in its API"
  }
]
```

### Dry runs

`--dry-run` checks a write against the sign-in, the network's capabilities and its length limit without sending anything. A passing post reports the length counted the network's way and the limit, and a failing one explains the failure:

```json
[
  {
    "network": "x",
    "ok": true,
    "demo": true,
    "dry_run": true,
    "text": "ship it",
    "length": 7,
    "limit": 280
  }
]
```

```json
[
  {
    "network": "bluesky",
    "ok": false,
    "dry_run": true,
    "error": "too long for Bluesky: 301/300"
  }
]
```

### Accounts and configuration

`socli whoami` prints one object per network. `handle` and `url` are present only for a signed-in network:

```json
[
  { "network": "x", "signed_in": false },
  { "network": "bluesky", "signed_in": false },
  { "network": "devnull", "signed_in": false },
  { "network": "threads", "signed_in": false },
  { "network": "mastodon", "signed_in": false }
]
```

`socli auth status` prints the config path, the default network (`null` when unset), the image mode and one entry per network. A signed-in network adds `handle`, `method` (`oauth`, `bearer`, `app password`, `device key`, `ssh key` or `token`) and a `detail` string such as the Bluesky or Mastodon server. Secrets never appear.

```json
{
  "config": "/path/to/config.toml",
  "default_network": null,
  "images": "auto",
  "networks": [
    { "network": "x", "signed_in": false },
    { "network": "bluesky", "signed_in": false },
    { "network": "devnull", "signed_in": false },
    { "network": "threads", "signed_in": false },
    { "network": "mastodon", "signed_in": false }
  ]
}
```

`socli config list` prints one object with every key and its value (or default) plus `path`, `config get <key>` prints `{"key", "value", "default"}` where `default` says whether the value is the default, and `config set <key> <value>` prints `{"key", "ok", "value"}` with the value as saved:

```json
{
  "default": true,
  "key": "ui.theme",
  "value": "auto"
}
```

`socli config themes` prints `{"themes": [{"name", "description", "variants"}]}` for every value `ui.theme` accepts, where `variants` is `["dark"]`, `["dark", "light"]` or empty for `auto`, `native`, `black` and `white`.

`socli drafts` prints an array of drafts, empty when there are none (`[]`). Each draft has `index` (the number `socli drafts rm <index>` takes), `id`, `targets`, `text`, `created_at`, `updated_at`, and `reply_to` or `quote` when the draft is a reply or a quote. `drafts rm` prints `{"ok": true, "removed": <index>, "id": "…"}`. The demo run has no drafts, so these two shapes come from `src/main.rs` and not from a captured sample.

Sign-in and sign-out commands print one object per account: `socli login <net>` prints `{"ok", "network", "handle", "url"}` (`handle` and `url` when known), and `socli logout <net|all>` prints an array of `{"network", "ok", "error"}` with `error` only on a failure. Those two shapes also come from `src/main.rs`, because running them would change a real configuration.

### The help table

`socli help --json` prints one object with `name`, `summary`, `version`, `commands`, `global_flags`, `environment` and `exit_codes`. Each entry of `commands` has `name`, `summary`, `usage`, `flags` (each with `flag`, `value` and `description`) and `examples`. `exit_codes` maps the three codes to their meaning:

```json
{
  "0": "success",
  "1": "a network or target failed",
  "2": "a usage error, or a step that needs a person at a terminal"
}
```

## Errors and exit codes

When errors are JSON (agent mode, or `--json`), a failure prints exactly one line on stderr and nothing on stdout:

```json
{"error":"delete needs --yes when socli runs without a terminal"}
```

Usage errors from the command line parser add `usage`, and `tip` when the parser has one:

```json
{"error":"unrecognized subcommand 'frobnicate'","usage":"socli [OPTIONS] [COMMAND]"}
```

Outside JSON mode the same message prints as `socli: <message>`. A failed write is the exception: it prints its result array on stdout and exits 1 without an extra stderr line. A closed pipe exits 0.

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | A network or target failed: sign-in missing, a rate limit (`<Network> rate limit reached; try again in N minutes`, `in Ns` below two minutes, `in a minute` when the network sent no reset time), an unsupported feature, a post that does not exist |
| 2 | A usage error, or a step that needs a person at a terminal (a browser sign-in, a prompt, `delete` without `--yes`, `socli help` for an unknown command) |

Messages are lowercase and say what to run next, for example `which network? add --net x|bluesky|devnull|threads|mastodon, or use a full id like bluesky:at://…`, so an agent can show the message to the person or act on it.

## Writing safely

Run every write with `--dry-run` first. The dry run reports the length and the limit on each network and fails for a network that is not signed in or lacks the feature, without sending anything. Show the person the text and the networks, then run the real command after an explicit confirmation. `post`, `reply`, `quote`, `delete` and `follow` are public or destructive, so an agent asks before each one. `delete --yes` skips SoCLI's own question and nothing else asks in its place, so the confirmation belongs to the agent and the person.

`<b>`, `<i>`, `<strong>` and `<em>` in post text become Unicode bold and italic letters. Screen readers spell those letters one by one, search does not match them, and on X each counts as two characters, so use `--raw` to send the text as typed.

`socli share` copies a link only when a person runs it in a terminal. In a pipe or in agent mode it copies nothing and prints the link, as `{"url": "…", "copied": false}` in JSON mode (and in agent mode) and as the bare link otherwise, with nothing on stderr and exit 0. It adds nothing for an agent, which reads `url` or `permalink` from the post JSON. `socli ask` reads posts that are already loaded and does not call a model.

Keep tokens out of command lines and logs. Prefer the environment variables below or standard input, and read `socli auth status`, which redacts every secret.

## Signing in

An agent can sign in to three networks without a person:

| Network | Command |
| --- | --- |
| Bluesky | `printf '%s\n' "$APP_PASSWORD" \| socli login bluesky --handle you.bsky.social --password-stdin`, or set `BLUESKY_HANDLE` and `BLUESKY_APP_PASSWORD` |
| Threads | `socli login threads --token <token>`, or set `THREADS_ACCESS_TOKEN` |
| Mastodon | `socli login mastodon --token <token> --server <host>`, or set `MASTODON_ACCESS_TOKEN` and `MASTODON_SERVER` |

X and dev/null need a person at a browser, and agent mode refuses their sign-ins with exit 2:

```json
{"error":"signing in to X needs a person at a browser. Run socli login x in a terminal"}
```

`socli login devnull` and `socli auth threads oauth` are refused the same way, and so is `socli login mastodon` without a token (a token comes from `--token` or `MASTODON_ACCESS_TOKEN`):

```json
{"error":"signing in to Mastodon needs a person at a browser. Run socli login mastodon in a terminal"}
```

Create the Mastodon token in the server's Preferences > Development with the scopes `read write`. [integration/mastodon/auth.md](https://github.com/massimodeluisa/socli/blob/main/docs/integration/mastodon/auth.md) lists both Mastodon sign-in paths. A person signs in once at a terminal, and the agent then uses the stored session. X also reads `X_ACCESS_TOKEN`, `X_BEARER_TOKEN` and the other `X_*` variables from the environment (listed in [CONVENTIONS.md](https://github.com/massimodeluisa/socli/blob/main/docs/CONVENTIONS.md#environment-variables)), and `socli auth x token --access-token <token>` stores a user token the person already has. The per-network pages in `docs/integration/<network>/auth.md` give every method.

## Installing the skill

`docs/agents/socli/SKILL.md` is a ready-to-install Claude Code skill that teaches an agent these rules. Copy the folder `docs/agents/socli/` to `~/.claude/skills/socli/`, and Claude Code lists the `socli` skill in its next session.
