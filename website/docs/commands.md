---
title: CLI commands
description: Run SoCLI from the shell with direct commands, positional arguments, options, stdin and JSON output.
---

# CLI commands

Run `socli <command> [arguments] [options]` from your shell to read feeds, publish posts or manage accounts. Options that take a value use `--option value`, such as `--net bluesky` or `--count 10`. The image option uses an equals sign: `--images=blocks`.

```sh
socli feed saved --net bluesky --json
socli search "rust tui" --net bluesky --print --no-prompt
socli post "hello from the terminal" --net x,bluesky
```

Read commands open the full-screen TUI when both input and output are terminals. Add `--json` to get one JSON value and exit, or `--print --no-prompt` to print terminal text and exit. With redirected input or output, reads print once without opening the TUI. Writes with supplied text and account/configuration commands return after the operation; sign-in can still require a browser or a prompt.

To try a read and a write check without an account:

```sh
socli feed --demo --json
socli post "hello from the terminal" --demo --net x,bluesky --dry-run --json
```

## Output modes

| Invocation | Behavior |
| --- | --- |
| `socli` or a read command at a terminal | Open the TUI. Without an account, bare `socli` opens the sign-in screen. |
| `socli --print` | Print the feed, then open the `SoCLI>` prompt when both input and output are terminals. |
| `socli --print --no-prompt` | Print the feed once and exit. |
| `socli --json` | Print the feed as a JSON array and exit. |
| `socli feed --json` | Print a read command's posts as a JSON array and exit. |
| `socli post "hello" --net bluesky --json` | Publish, print per-network results as a JSON array and exit. |

Coding agents get JSON by default and never open prompts. `--print` selects text output for them; `--json` takes precedence when both are present. [Agents](agents.html) documents detection, JSON fields and errors.

## Networks and targets

`--net` accepts `x`, `bluesky`, `devnull`, `threads`, `mastodon`, a comma-separated list or `all`. Aliases are `twitter`, `bsky`, `dev/null`, `dev-null` and `masto`. Naming `threads`, including in a comma list, answers Waiting for Meta approval. `--net all` leaves Threads out. That hold is outside `--demo`. Global options can appear before or after a command.

For reads, scope comes from `--net`, then `SOCLI_NET`, then `feed.networks` in the configuration, then the available signed-in networks. Public search, profile and thread reads can use an explicitly named network without a session where its API allows it; [Networks](networks.html) lists the restrictions.

For `post`, `--net` or `SOCLI_NET` chooses the destinations. Without either, SoCLI uses the default set by `socli auth default`, or the only signed-in network that can post. With several possible destinations and no default, the command asks you to specify `--net`. `--net all` posts to every signed-in network that can publish. The reader's `feed.networks` filter does not choose posting destinations.

A post target is the `id` from a JSON read, in the form `<network>:<native-id>`. Supported post URLs from X, Bluesky, dev/null and Mastodon also work. Threads requires the JSON `id` because its API cannot resolve post URLs. For a bare native id, specify one network with `--net`, or configure a default. `view` requires a scoped id or post URL when targeting a post.

In the examples below, set `$post_id` to an actual post's JSON `id` and `$handle` to the account handle you want to read or follow.

## Read

| Command | Arguments and behavior |
| --- | --- |
| `socli feed [name]` | Open the home feed, or choose `following`, `latest`, `trending`, `discover`, `saved`, `mine` or `notifications`. `foryou` is an alias for `latest`. Availability depends on the network. |
| `socli search <words...>` | Search the networks in scope that support search. Multiple words can be quoted or passed separately. |
| `socli user <handle>` | Read an account's profile and posts. Use `--net` to specify the account's network. |
| `socli thread <id-or-url>` | Open one conversation. |
| `socli notifications` | Read mentions. This also marks notifications as read on dev/null. |
| `socli bookmarks` | Read saved posts, including starred commits on dev/null. |
| `socli ask` | Open Ask in the TUI for text matching against posts already loaded. It does not call an AI model. |

```sh
socli feed following --net bluesky --count 10 --json
socli user "$handle" --net mastodon --print --no-prompt
socli thread "$post_id" --json
socli notifications --net bluesky --json
socli bookmarks --json
```

`--count` defaults to 15 posts requested per network. Networks can enforce their own page size. The reader asks each signed-in network for at least 20, and starts the next page when the selection reaches the 10th post of the current page; `m` loads one page the same way. A shell read prints the current page. `socli ask` is interactive: text passed after it is currently ignored, and `--json` or `--print` returns the feed rather than an answer.

## Write

| Command | Arguments and behavior |
| --- | --- |
| `socli post [text...]` | Publish to the selected destinations. |
| `socli reply <id-or-url> [text...]` | Reply on the target post's network. |
| `socli quote <id-or-url> [text...]` | Quote on the target's network, or on destinations chosen with `--net`. Other networks receive your text and the post link where native quoting is unavailable. |
| `socli like <id-or-url>` / `socli unlike <id-or-url>` | Like or undo a like (favorite on Mastodon, star on dev/null). |
| `socli repost <id-or-url>` / `socli unrepost <id-or-url>` | Repost or undo a repost where supported. |
| `socli bookmark <id-or-url>` / `socli unbookmark <id-or-url>` | Save a post or remove the bookmark. |
| `socli follow <handle>` / `socli unfollow <handle>` | Follow or unfollow on the network chosen with `--net`, the sole signed-in network or the configured default. |
| `socli delete <id-or-url>` | Delete your post. At a terminal it asks for confirmation; `--yes` skips that question and is required for JSON or noninteractive deletion. |

```sh
socli post "hello from the terminal" --net bluesky --dry-run --json
socli reply "$post_id" "agreed"
socli quote "$post_id" "worth reading" --net x,bluesky
socli like "$post_id"
socli follow "$handle" --net bluesky
```

`--dry-run` checks sign-in, supported actions and text length for writes without sending them. `--json` reports each destination separately, including errors, and a failed destination makes the command exit with code 1. [Networks](networks.html) lists which actions each network supports.

### Text, stdin and the composer

Pass text as a quoted argument or use `-` to read standard input. Outside agent mode, omitting text also reads redirected input. At a terminal, omitting text opens the composer for `post`, `reply` or `quote`.

```sh
printf '%s\n' 'hello from a pipe' | socli post - --net bluesky
socli post - --net bluesky < post.txt
socli post --net bluesky
```

Posts contain text only; there is no attachment option. `<b>`, `<strong>`, `<i>` and `<em>` become Unicode bold and italic letters before sending. These letters affect accessibility and character limits; add `--raw` to preserve the text as typed. `--raw` belongs to `post`, `reply` and `quote`, so place it after one of those command names.

For text that starts with a dash, place `--` before the text so the shell command treats it as an argument rather than an option.

### Links and images

| Command | Options and behavior |
| --- | --- |
| `socli share <id-or-url> [--profile]` | Copy the post link, or the author's profile link with `--profile`. In a pipe or agent mode it prints the link without changing the clipboard. `--json` reports the URL and whether it was copied. |
| `socli view <target>` | Show images from a scoped post id, supported post URL, image URL, local image file or `demo`. `--json` lists image URLs instead of drawing them. |

```sh
socli share "$post_id" --profile --json
socli view demo
socli view "$post_id" --json
```

[Images](images.html) covers graphics modes and terminal support.

## Accounts and sign-in

| Command | Arguments and behavior |
| --- | --- |
| `socli login <network> [options]` | Sign in to one network. Each network takes its own options below. |
| `socli logout <network-or-all>` | Sign out of one network, or every network with `all`. |
| `socli whoami` | List the accounts this process will use. |
| `socli auth status` | Show configuration and sign-in status with secrets redacted. |
| `socli auth default <network>` | Save the default used when posting without `--net` or `SOCLI_NET`. |

### Login options

| Command | Options |
| --- | --- |
| `socli login x` | `--browser <name>`, `--profile <name>` for browser sessions; `--oauth`, `--client-id <id>`, `--client-secret <secret>`, `--scopes <list>`, `--port <n>` for OAuth (port defaults to `8741`). |
| `socli login bluesky` | `--handle <handle>`, `--service <url>`, `--password-stdin` (requires `--handle`). |
| `socli login devnull` | `--device-name <name>`. |
| `socli login threads` | Answers Waiting for Meta approval. `--token` and `--app-secret` are not used while that hold is on. |
| `socli login mastodon` | `--server <host>`, `--token <token>`. |

The [Sign in](login.html) page explains prerequisites, prompts, environment variables and browser flows.

### Advanced authentication

`socli auth <network> login` accepts the same options as `socli login <network>`, and `socli auth <network> logout` signs that network out. The other authentication commands are:

| Command | Arguments and options |
| --- | --- |
| `socli auth x token` | Required `--access-token <token>`; optional `--user-id <id>`, `--username <name>`, `--refresh-token <token>`. Store an existing user token. |
| `socli auth x bearer <token>` | Store an app-only bearer for public reads. |
| `socli auth devnull ssh` | Optional `--key <path>`. Select the GitHub SSH key used to sign posts. |
| `socli auth threads oauth` | Answers Waiting for Meta approval. Does not open a browser while that hold is on. |
| `socli auth threads refresh` | Answers Waiting for Meta approval. |

## Configuration and drafts

| Command | Arguments and behavior |
| --- | --- |
| `socli config [list]` | List every setting with its value or default. |
| `socli config get <key>` | Read a setting. |
| `socli config set <key> <value...>` | Validate and save a setting. |
| `socli config path` | Print the configuration file path. |
| `socli config themes` | List supported theme names and variants. |
| `socli drafts` | List saved drafts and their numbers. |
| `socli drafts rm <number>` | Remove a saved draft using its number in the list. |

```sh
socli config get ui.detail
socli config set ui.theme nord
socli config set feed.networks x,bluesky
socli drafts --json
```

[Configuration](config.html) lists valid keys and values. Resume a draft with `ctrl-o` in the composer; the CLI has no draft-send or draft-resume subcommand.

## Global flags

These options are accepted before or after the command. They apply to the relevant read, rendering or write operation; they do not turn account/configuration commands into demo operations.

| Flag | Values and behavior |
| --- | --- |
| `--json` | Print JSON. Read results are arrays of posts; writes are arrays of per-network results. |
| `--print` | Use scrollback text for reads instead of the TUI. |
| `--no-prompt` | Suppress the `SoCLI>` prompt after printing. |
| `--demo` | Use the offline feed and simulate post actions. |
| `--net <list>` | Select networks; see [Networks and targets](#networks-and-targets). |
| `--count <n>` | Request this many posts per network; default `15`. The argument is an integer from `0` to `255`; providers can impose other limits. |
| `--width <n>` | Set the total wrap width for printed output; default is the detected terminal width, or `80` when unavailable. Widths below `24` render at `24`. |
| `--dry-run` | Check a write without sending it. |
| `--theme <name>` | Select `auto`, `native`, `black`, `white` or a name from `socli config themes`. |
| `--density <name>` | Select `compact`, `cozy` or `roomy`. |
| `--detail <level>` | Select `plain`, `minimal`, `medium` or `rich`; `standard` is an alias for `medium`. |
| `--layout <name>` | Select `auto`, `single` or `split`. |
| `--mouse <on\|off>` | Enable or disable mouse handling in the TUI. |
| `--mono` | Use monochrome rendering. |
| `--images[=<mode>]` | Select `auto`, `kitty`, `iterm`, `sixel`, `blocks` or `off`. Bare `--images` means `auto`; a value requires `=`, as in `--images=off`. |
| `--icons <name>` | Select `auto`, `nerd` or `unicode`. Cannot be combined with `--no-icons`. |
| `--no-icons` | Use Unicode icons without a Nerd Font. |
| `-h`, `--help` | Show full help, or the top-level command's help when a command is named. |

`--raw`, `share --profile`, `delete --yes` and the login/authentication options are command-specific, as listed in their sections above.

## Help and completions

```sh
socli help
socli help post
socli post --help
socli help --json
socli completions zsh
```

`socli --version` (or `socli -V`) prints the installed version. `socli help` lists the CLI, `socli help <command>` shows usage and examples, and `socli help --json` returns the command table. `socli completions <shell>` prints a completion script for `bash`, `zsh`, `fish`, `elvish` or `powershell`; [Install](install.html#shell-completions) shows how to save it.

## In-reader commands

The TUI command palette (`:` or `ctrl-k`) and the scrollback `SoCLI>` prompt share an interactive grammar. Enter commands without the `socli` prefix. Use `net bluesky` to change scope, `following` to pick a feed and post numbers or the selection for actions. Shell options such as `--net`, `--json` and `--raw` are not parsed here, and quotes are treated as text rather than shell delimiters.

```text
net bluesky
following
search rust tui
reply 2 agreed
```

| Commands | Behavior |
| --- | --- |
| `next`, `prev`, `expand`, `back` | Move the selection, open a thread or return to the last view. |
| `more`, `refresh` | Load another page or reload the current page. |
| `home`, `notifications`, `search [words]` | Switch section or run a search. |
| `net <network-or-all>` | Show one network or all signed-in networks. |
| `following`, `foryou`, `latest`, `bookmarks`, `trending`, `discover`, `saved`, `mine` | Choose a feed tab. |
| `user <handle>`, `thread <id-or-url>` | Open a profile or conversation. |
| `post <text>`, `reply [number] <text>`, `quote [number] <text>` | Publish using the current scope or selected post. |
| `like [number]`, `unlike [number]`, `repost [number]`, `unrepost [number]`, `bookmark [number]`, `unbookmark [number]`, `delete [number]` | Act on a numbered post, or the current selection when the number is omitted. |
| `follow <handle>`, `unfollow <handle>` | Follow or unfollow an account. |
| `open [number]`, `view [number]`, `avatar [number]`, `json [number]`, `share [number]` | Open the post's link or images, view its avatar, inspect it or copy its link. |
| `filter [words]`, `clear` | Filter posts already loaded, or clear that filter. |
| `theme [name]` | Open the theme picker in the TUI, or apply a named theme. |
| `help`, `quit` | Show interactive help or leave SoCLI. |

Interactive post numbers start at 1. They are positions in the current list, not the scoped ids used by the shell CLI. `help` in the TUI opens the key reference; `help` in the scrollback prompt lists interactive commands. The scrollback prompt requires `theme <name>` and applies it only to the running session. In the TUI, `:theme <name>` also saves the choice. [Keys](keys.html) covers the keyboard controls.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success (including a closed output pipe). |
| `1` | An operation or a network failed. |
| `2` | A usage error, or an operation that needs a person at a terminal. |

JSON errors normally go to stderr as one object with an `error` field. A failed write reports its per-network result array on stdout and exits with `1`. [Agents](agents.html#errors-and-exit-codes) documents the error shapes and terminal interruption codes.
