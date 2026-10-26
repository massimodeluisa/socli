# Bluesky: signing in

SoCLI signs in to Bluesky with an app password. It posts the handle and the app password to `com.atproto.server.createSession`, keeps the two JWTs the server returns, and refreshes them when they expire. Reading public posts, threads and profiles needs no sign-in. The rest of this folder: [overview](README.md), [endpoints](endpoints.md), [mapping](mapping.md), [testing](testing.md).

## Create an app password

Make one at https://bsky.app/settings/app-passwords. SoCLI never asks for the account password, and a 401 from `createSession` produces `Bluesky rejected the handle or app password. Make one at https://bsky.app/settings/app-passwords`.

## Sign in

Two forms do the same thing:

```
socli login bluesky [--handle <handle>] [--service <url>] [--password-stdin]
socli auth bluesky login [--handle <handle>] [--service <url>] [--password-stdin]
```

| Flag | Effect |
| --- | --- |
| `--handle <handle>` | The account handle. A leading `@` is dropped. |
| `--service <url>` | The server to sign in on. A trailing slash is dropped. |
| `--password-stdin` | Reads one line from standard input as the app password and never prompts. Requires `--handle`; without it SoCLI exits with a usage error. |

Where each value comes from, first match wins:

| Value | Order |
| --- | --- |
| Handle | `--handle`, then `BLUESKY_HANDLE`, then a visible prompt |
| App password | `--password-stdin`, then `BLUESKY_APP_PASSWORD`, then a hidden prompt (it prints one `•` per character, and ctrl-c cancels) |
| Service | `--service`, then `BLUESKY_SERVICE`, then `https://bsky.social` |

Steps SoCLI takes:

1. It resolves the three values above and rejects an empty handle or password.
2. It posts `{identifier, password}` to `{service}/xrpc/com.atproto.server.createSession`.
3. It reads the PDS from the `didDoc` in the answer (the service entry whose id ends in `#atproto_pds`). If the document has none, it keeps the service it signed in on.
4. It writes the session to `config.toml` and reports the account.

## What is stored

The session goes into the `[bluesky]` section of `config.toml`:

| Key | Content |
| --- | --- |
| `service` | The PDS base URL. Defaults to `https://bsky.social` when missing. |
| `handle` | The account handle. |
| `did` | The account DID, used as the `repo` of every write. |
| `access_jwt` | Bearer token sent with each request. |
| `refresh_jwt` | Token used to get a new pair. |

The file is `$SOCLI_CONFIG_DIR/config.toml` when that variable is set, else `$XDG_CONFIG_HOME/socli/config.toml`, else `~/.config/socli/config.toml`. SoCLI writes it with mode 0600 in a directory with mode 0700, through an atomic replace. `socli auth status` shows the account (`@handle` or `not signed in`), the session redacted (`missing`, `set`, or `set (…abcd)` for tokens longer than six characters) and the service.

## Environment variables

| Variable | Used for |
| --- | --- |
| `BLUESKY_HANDLE` | Handle when `--handle` is absent |
| `BLUESKY_APP_PASSWORD` | App password when `--password-stdin` is absent |
| `BLUESKY_SERVICE` | Service when `--service` is absent |

Blank values are ignored.

## Token lifetime and refresh

When a request returns 400 or 401 and the JSON `error` is `ExpiredToken` or `InvalidToken`, SoCLI posts to `{service}/xrpc/com.atproto.server.refreshSession` with the refresh JWT as bearer, saves the new access and refresh tokens, and repeats the original request once. If the refresh fails, the command stops with `sign in first: socli login bluesky`. The code does not know how long a token lives, and the pages in the source list below do not state it, so this document gives no figure.

## Sign out

```
socli logout bluesky
socli auth bluesky logout
socli logout all
```

SoCLI calls `com.atproto.server.deleteSession` with the refresh JWT (a failure there is ignored) and then clears the `[bluesky]` section. SoCLI does not delete the app password itself. Delete it in the Bluesky settings when you no longer need it.

## Agents and scripts

Bluesky sign-in is not gated behind a browser, so an agent can run it, but only without prompts. Agent mode is on when `SOCLI_AGENT=1` is set, or when SoCLI detects an AI agent in the environment (`AI_AGENT` or an agent's own variable); `SOCLI_AGENT=0` turns it off. When it is on and a value would need a prompt, SoCLI exits with the hint `Pass the handle and read the app password from stdin: socli login bluesky --handle <handle> --password-stdin`. A script feeds the password on standard input:

```
printf '%s\n' "$APP_PASSWORD" | socli login bluesky --handle <handle> --password-stdin
```

## Why not OAuth

Bluesky documents OAuth for clients. An OAuth client needs a client metadata file hosted on a domain, which SoCLI does not have yet, so OAuth support comes later. Sources: https://docs.bsky.app/docs/advanced-guides/oauth-client and https://atproto.com/specs/oauth.

Back to [network overview](../README.md).
