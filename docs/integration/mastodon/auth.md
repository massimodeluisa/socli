# Mastodon: signing in

socli signs in to Mastodon on the server where your account lives, and it offers two ways. The browser path registers socli as an app on that server, sends you to its authorization page, and exchanges the code you paste for a token with PKCE. The token path skips the browser: you create an access token yourself in your server's settings and hand it to socli. Reading public threads and profiles needs no sign-in. The rest of this folder: [overview](README.md), [endpoints](endpoints.md), [mapping](mapping.md), [testing](testing.md).

## Sign in

Two forms do the same thing:

```
socli login mastodon [--server <host>] [--token <token>]
socli auth mastodon login [--server <host>] [--token <token>]
```

| Flag | Effect |
| --- | --- |
| `--server <host>` | Your server, such as `fosstodon.org`. A leading `https://` or `http://` and any path are dropped, the host is lowercased, and a trailing dot is removed. |
| `--token <token>` | An access token. With it (or with `MASTODON_ACCESS_TOKEN`) socli never opens a browser. |

Where each value comes from, first match wins:

| Value | Order |
| --- | --- |
| Server | `--server`, then `MASTODON_SERVER`, then the prompt below |
| Token | `--token`, then `MASTODON_ACCESS_TOKEN`, then the browser path |

When neither names a server and a person is at a terminal, socli asks `  Server (mastodon.social): ` before it prints the sign-in screen. An empty answer keeps `mastodon.social`. An answer that is not a server name prints the error and asks again. Without a person (agent mode, or no terminal on stdin or stderr) socli does not ask and uses `mastodon.social`. This holds for both the browser path and the token path.

socli accepts a server name only when it is a host with a dot and without spaces or an `@`. Anything else stops with `<input> is not a server name. use one like mastodon.social`. The server is the host of your account, so an account `ines@fosstodon.org` signs in with `--server fosstodon.org`.

### The browser path

Run `socli login mastodon --server <host>` with no token (without `--server`, socli asks for the server first). Steps socli takes:

1. It prints the server and the method, then looks at the saved `[mastodon]` section. When it already holds an app (`client_id` and `client_secret`) registered on the same server, socli reuses it. Otherwise it posts `client_name=socli`, `redirect_uris=urn:ietf:wg:oauth:2.0:oob` and `scopes=read write` to `https://<server>/api/v1/apps`, and saves the returned `client_id` and `client_secret` at once. The registration happens once per server.
2. It creates a PKCE code verifier and builds `https://<server>/oauth/authorize` with `response_type=code`, the `client_id`, the same out-of-band redirect, the scopes `read write`, and `code_challenge_method=S256`.
3. It prints the address (`open this url if the browser does not:`) and tries to open it in the browser. Sign in on your server if it asks, and approve socli.
4. The server shows an authorization code instead of redirecting. Paste it at the `  Authorization code: ` prompt. An empty entry stops with `no code was entered. run `socli login mastodon` again`.
5. socli posts the code with the verifier, the app credentials and the same redirect and scope to `https://<server>/oauth/token`. A refusal reads `<server> did not accept the code: <message>. run `socli login mastodon` again`.
6. It verifies the token with `GET /api/v1/accounts/verify_credentials`, reads the server's post length limit from `GET /api/v2/instance`, writes the section and prints `Signed in as @<user>@<server>`.

Mastodon added PKCE support to its authorization endpoint in version 4.3.0, according to the official OAuth page (read 2026-10-20). That page also documents that `S256` is the only method the server supports.

### The token path

Create an application on your server under Preferences > Development, give it the scopes `read write`, and copy its access token. Then run:

```
socli login mastodon --server <host> --token <token>
```

or set the variables and run `socli login mastodon`:

```
export MASTODON_SERVER=<host> MASTODON_ACCESS_TOKEN=<token>
```

socli prints the server and the method, then runs the same verification as the browser path (steps 6 above), without registering an app. The prompt of the browser path points to this option as `socli login mastodon --token <token>`.

A token on the command line shows up in your shell history and in the process list. The variable keeps it out of both.

## What is stored

The `[mastodon]` section of `config.toml`, from `MastodonConfig` in `src/config.rs`:

| Key | Content |
| --- | --- |
| `server` | The server's host name, never a URL. |
| `client_id`, `client_secret` | The app socli registered on that server. Present after the browser path. The token path keeps them only when they belong to the same server. |
| `access_token` | Bearer token sent with each request. |
| `user_id` | The account id on that server, used to tell your own posts from replies to others. |
| `username` | The full handle, `user@server`. |
| `max_chars` | The server's post length limit read at sign-in, 500 when the server did not answer. |

The file is `$SOCLI_CONFIG_DIR/config.toml` when that variable is set, else `$XDG_CONFIG_HOME/socli/config.toml`, else `~/.config/socli/config.toml`. socli writes it with mode 0600 in a directory with mode 0700, through an atomic replace.

`socli auth status` shows four rows for Mastodon:

| Row | Content |
| --- | --- |
| `account` | `@user@server`, or `signed in` when a token exists without a stored handle, or `not signed in` |
| `token` | The token redacted: `missing`, `set`, or `set (…abcd)` for tokens longer than six characters |
| `server` | The server in use |
| `app` | `registered` when the section holds an app, else `none` |

With `--json`, a signed-in Mastodon entry reports `method` as `oauth` when an app is registered and `token` otherwise, with the server as `detail`.

## Environment variables

| Variable | Used for |
| --- | --- |
| `MASTODON_SERVER` | The server at sign-in when `--server` is absent, and the server of every request while it is set |
| `MASTODON_ACCESS_TOKEN` | The token at sign-in when `--token` is absent, and the token of every request while it is set |

Blank values are ignored. Both variables also override the stored values at run time. Keep them as a pair, because a token only works on the server that issued it, and `socli login` stores the two together. A token set only in the environment authorizes reads, favorites, boosts, bookmarks and follows. `post` also needs the handle that `socli login` stores in `username`, so without it posting answers ``sign in first: `socli login mastodon` ``.

## Token lifetime

socli has no refresh step and stores no expiry for Mastodon. The official pages in the source list (read 2026-10-20) do not state a token lifetime, so this document gives no figure. When a server stops accepting the saved token, any request answers `<server> no longer accepts the saved token. run `socli login mastodon` again`.

## Sign out

```
socli logout mastodon
socli auth mastodon logout
socli logout all
```

When the section holds a server, a token and a registered app, socli posts the token to `https://<server>/oauth/revoke` so the server stops accepting it, and ignores a failure there. Then it clears the whole `[mastodon]` section, including the registered app, so the next browser sign-in registers a new one. A token you created by hand in Preferences carries no app credentials in socli, so socli only forgets it. Delete or revoke it in your server's settings when you no longer need it.

## Agents and scripts

The token path runs without a person. Agent mode is on when `SOCLI_AGENT=1` is set, or when socli detects an AI agent in the environment; `SOCLI_AGENT=0` turns it off. In agent mode, `socli login mastodon --token <token>` and `socli login mastodon` with `MASTODON_ACCESS_TOKEN` set both work. The browser path is refused with exit 2:

```
{"error":"signing in to Mastodon needs a person at a browser. Run socli login mastodon in a terminal"}
```

A person signs in once at a terminal, and the agent then uses the stored session. The check runs before any request, so the refusal sends nothing to the server.

Back to [network overview](../README.md).
