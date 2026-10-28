# X sign-in

`socli login x` imports the X session that is already signed in inside Chrome, Brave, Edge, Arc or Chromium, and stores the `auth_token` and `ct0` cookies. Later runs reuse those cookies and do not open a browser. On macOS the cookie values are read from the browser's local database and decrypted with the Safe Storage password in the login keychain.

`socli login x --oauth` is the other path: OAuth 2.0 with PKCE against a developer app you create at console.x.com. Passing `--client-id` or `--client-secret` selects that path as well. Two paste-in alternatives exist for scripts and agents: a user token you already hold, and an app-only bearer token for public reads. `X_AUTH_TOKEN` and `X_CT0` supply a browser session without reading a cookie database.

The page cannot read those cookies. It opens `https://x.com/i/oauth2/authorize` for SoCLI's public client, and the person approves it. Setting `SOCLI_X_CLIENT_ID` when SoCLI is compiled replaces that client. An empty value makes the page say `this build has no SoCLI X app` and does not ask the person to create one. That page session is the official API. For you is a search, and likes and follows can be refused. The callbacks registered for that public client are `https://socli.sh/`, `https://socli.sh/web/`, `http://127.0.0.1:5174/`, and `http://127.0.0.1:5174/web/`. The full-screen reader on socli.sh returns to `https://socli.sh/web/`. The terminal row "A different account on x.com" uses `https://socli.sh/`. socli.sh sends the browser on to `http://127.0.0.1:8741/callback`, which is only the local listener. A personal app passed with `--client-id` or `--port` still uses `http://127.0.0.1:<port>/callback`. The id is read when SoCLI is compiled, not at run time.

Back to [X](README.md) or the [integration overview](../README.md). Related: [endpoints](endpoints.md), [mapping](mapping.md), [testing](testing.md).

## Sign in from a browser that is already signed in

Sign in at https://x.com in Chrome, Brave, Edge, Arc or Chromium, then run:

```
socli login x
socli login x --browser brave --profile "Profile 7"
```

On a terminal, SoCLI asks which profile to use even when only one matches. Pass `--browser` and `--profile` together to skip that question when they match one session. Without a terminal, one match is used as-is, and several matches still need those two flags. SoCLI reads the cookies, asks the signed-in home page which account they belong to, and stores the cookies together with that account. A home page that does not name an account leaves the previous config file unchanged.

This browser session imitates x.com's web client. Requests go out with the public web bearer `WEB_BEARER`, the fixed Chrome user agent `CHROME_UA`, and a computed `x-client-transaction-id`. X may treat that as automation, and the account is at risk. `socli login x --oauth` is the path X sanctions.

| Flag | Meaning |
| --- | --- |
| `--browser <name>` | `chrome`, `brave`, `edge`, `arc` or `chromium`. The default is every one of those that has an X session. |
| `--profile <name>` | The profile directory or its display name. With `--browser`, one match skips the account question. |

This path does not use a developer app. The official API's For you feed, likes and follows are not what this session calls. See [endpoints](endpoints.md).

## Sign in with a developer app

Create an app in the developer console (`console.x.com`) with OAuth 2.0 enabled and this callback URL, which SoCLI listens on:

```
http://127.0.0.1:8741/callback
```

Choose a public client with PKCE, or a confidential client and pass its secret with `--client-secret`. Then run:

```
socli login x --oauth --client-id <id>
socli login x --client-id <id> --client-secret <secret>
socli auth x login --client-id <id>
```

`socli auth x login` takes the same flags as `socli login x`. The OAuth flags:

| Flag | Meaning |
| --- | --- |
| `--oauth` | Use OAuth even when a client id is already stored. A stored client id alone does not select this path. |
| `--client-id <id>` | The app's OAuth client id. Falls back to `X_CLIENT_ID`, then the `client_id` stored in `config.toml`, then SoCLI's compiled client. Passing it selects OAuth. |
| `--client-secret <secret>` | The app's client secret, for confidential clients. Sent as HTTP basic authentication at the token endpoint. Passing it selects OAuth. |
| `--scopes <list>` | The OAuth scopes to ask for. Default: `tweet.read tweet.write users.read like.write follows.write bookmark.read bookmark.write offline.access`. |
| `--port <n>` | The local callback port. Default `8741`. The callback URL in the developer console must use the same port. |

With no `--client-id` and no stored client id, `--oauth` and "A different account on x.com" use SoCLI's compiled client. SoCLI then does the following:

1. Builds the authorize URL on `https://x.com/i/oauth2/authorize` with `response_type=code`, the client id, the redirect URI, the scopes, a random `state` and the PKCE challenge, and opens it in the browser.
2. Waits up to 180 seconds on `127.0.0.1:<port>` for the redirect, checks that `state` matches, and takes the `code`. Each connected callback has a five-second read limit and an 8 KiB request limit.
3. Exchanges the code at `POST https://api.x.com/2/oauth2/token` with the PKCE verifier.
4. Calls `GET /2/users/me` with the new token. When that names an account, one write stores the client id, the secret, the tokens, the expiry, and that account, and clears any browser cookies. A failed profile read leaves the previous file unchanged.

Sign in again with `socli login x --oauth` if your existing OAuth session lacks `like.write` or `follows.write`. Refreshing a token does not grant new permissions.

The scope `offline.access` is what makes X issue a refresh token. The official documentation says an access token without it stays valid for two hours (checked 2026-10-20), so removing it from `--scopes` means signing in again after that time.

## Paste a token you already have

```
socli auth x token --access-token <token> [--user-id <id>] [--username <name>] [--refresh-token <token>]
```

This stores a user access token without any browser. `--user-id` and `--username` are optional but speed up later calls. Without `--refresh-token` (and a stored client id) SoCLI cannot renew the token when X rejects it.

## App-only bearer token

```
socli auth x bearer <token>
```

The bearer token is for public reads: search, threads, profiles, counts and views. It cannot post, like, repost, bookmark or follow. When a browser session or a user token is stored, SoCLI prefers the browser session for every call and otherwise uses the user token. With only a bearer, it uses that for reads and refuses writes with "sign in first: `socli login x`".

## What is stored

Everything lives in the `[x]` section of `config.toml`: `$SOCLI_CONFIG_DIR/config.toml` if that variable is set, else `$XDG_CONFIG_HOME/socli/config.toml`, else `~/.config/socli/config.toml`. The file is written atomically with mode 0600 in a 0700 directory. There is no separate key file for X. Query ids for the web client are cached beside it in `x-ops.json`, which holds no cookies.

| Key | Content |
| --- | --- |
| `auth_token`, `ct0` | The browser session. Both are required. |
| `client_id`, `client_secret` | The app credentials, kept after sign-out. |
| `access_token`, `refresh_token` | The user token pair from OAuth or `socli auth x token`. |
| `expires_at` | Unix time when the access token expires (now plus `expires_in`). |
| `user_id`, `username` | Your account. |
| `bearer_token` | The app-only bearer. |
| `latest_query` | The search behind the `Latest` tab, and behind `For you` when the session is an official token. Default `lang:en -is:retweet`. Edit the file to change it; no command or environment variable sets it. |

## Environment variables

Environment values override the file, are trimmed, and are ignored when empty.

| Variable | Overrides |
| --- | --- |
| `X_AUTH_TOKEN` | `auth_token` |
| `X_CT0` | `ct0` |
| `X_CLIENT_ID` | `client_id` |
| `X_CLIENT_SECRET` | `client_secret` |
| `X_ACCESS_TOKEN` | `access_token` |
| `X_REFRESH_TOKEN` | `refresh_token` |
| `X_USER_ID` | `user_id` |
| `X_USERNAME` | `username` |
| `X_BEARER_TOKEN` | `bearer_token` |

A client exists as soon as both cookie variables are set, or `X_ACCESS_TOKEN` or `X_BEARER_TOKEN` is set, so a job can read X without a browser database.

## Token lifetime and refresh

A browser session has no refresh call. When X rejects the cookies, run `socli login x` again.

When the official API answers 401 and a refresh token is stored, SoCLI refreshes once and retries the call. The refresh is `POST https://api.x.com/2/oauth2/token` with `grant_type=refresh_token`, the refresh token and the client id (basic authentication when a client secret exists). It needs a stored client id and refresh token. The new tokens are saved to `config.toml`. Without a stored client id the refresh stops with "no client id stored; cannot refresh. run `socli login x`", and a refresh that X rejects is reported as "refresh failed (<status>)" followed by X's answer. A 401 with no refresh token stored is reported as "The token was rejected. Run `socli login x`, or check X_ACCESS_TOKEN / X_BEARER_TOKEN."

## Status and sign-out

```
socli auth status
socli logout x
socli auth x logout
```

`socli auth status` shows the account, then the browser session, the user token, the app bearer and the client id, each as `missing`, `set` or only its last four characters. Sign-out clears the cookies, the access token, the refresh token, the expiry, the user id, the username and the bearer token. It keeps the client id and secret.

## Agents and scripts

`socli login x` and `socli auth x login` need a person, so they refuse to run when agent mode is active (`SOCLI_AGENT=1`, or SoCLI recognizes an agent from its environment variables; `SOCLI_AGENT=0` turns the mode off). The refusal is a usage error with exit code 2: "signing in to X needs a person at a browser. Run socli login x in a terminal". Any other prompt refuses the same way when nobody can answer it (stdin and stderr must both be terminals and agent mode must be off), with exit code 2 and a message that starts "this step needs a person at a terminal.". After a person has signed in, an agent reuses the stored cookies or token. Without a person, `X_AUTH_TOKEN` and `X_CT0`, or `socli auth x token` and `socli auth x bearer`, provide credentials. JSON output of a sign-in command has the form `{"ok": true, "network": "x", ...}`; `auth` subcommands print `{"ok": true, "network": "x", "action": "token"}` and similar.
