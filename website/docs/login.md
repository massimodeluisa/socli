---
title: Sign in
description: How to sign in to X, Bluesky, dev/null, Threads and Mastodon, and how to check the accounts SoCLI uses.
---

# Sign in

Each network has its own sign-in command. `socli auth status` shows what is configured without printing secrets, and `socli whoami` lists the accounts this process will use.

## X

`socli login x` imports the session already signed in inside Chrome, Brave, Edge, Arc or Chromium and stores the cookies in the `[x]` section of `config.toml`. On a terminal, SoCLI asks which profile to use even when only one matches. `--browser` and `--profile` together skip that question when they match one session.

Importing that session imitates the x.com web client. The calls use the public web bearer `WEB_BEARER`, a fixed Chrome user agent `CHROME_UA`, and a computed `x-client-transaction-id`. X may treat those calls as automation, so the account is at risk. The path X sanctions is `socli login x --oauth`.

| Flag | What it does |
| --- | --- |
| `--browser <name>` | `chrome`, `brave`, `edge`, `arc` or `chromium`. |
| `--profile <name>` | The profile directory or its display name. |
| `--oauth` | Use OAuth 2.0 with a developer app instead of the browser session. |
| `--client-id <id>` | The app's OAuth client id. Falls back to `X_CLIENT_ID` and then to the `client_id` stored in `config.toml`. Passing it selects OAuth. |
| `--client-secret <secret>` | The app's client secret, for confidential clients. Passing it selects OAuth. |
| `--scopes <list>` | The OAuth scopes to ask for. |
| `--port <n>` | The local callback port. Default `8741`. |

SoCLI's X app is compiled in. "A different account on x.com" and the page both approve that app. The terminal returns to `https://socli.sh/`, and socli.sh sends the browser on to `http://127.0.0.1:8741/callback`. The reader on socli.sh returns to `https://socli.sh/` from the homepage and to `https://socli.sh/web/` from the full-screen page. A reader on this machine uses `http://127.0.0.1:5174/` or `http://127.0.0.1:5174/web/`. `--client-id` uses a different app, which needs `http://127.0.0.1:8741/callback` saved on that app.

The default scopes are `tweet.read tweet.write users.read bookmark.read bookmark.write offline.access`. The scope `offline.access` is what makes X issue a refresh token.

The environment variables are `X_AUTH_TOKEN`, `X_CT0`, `X_CLIENT_ID`, `X_CLIENT_SECRET`, `X_ACCESS_TOKEN`, `X_REFRESH_TOKEN`, `X_USER_ID`, `X_USERNAME` and `X_BEARER_TOKEN`. Both cookie variables together are a browser session. An app-only bearer token is enough for public reads, and `socli auth x bearer <token>` stores one.

## Bluesky

`socli login bluesky` asks for your handle and an app password created at [bsky.app/settings/app-passwords](https://bsky.app/settings/app-passwords). SoCLI never asks for the account password.

| Flag | What it does |
| --- | --- |
| `--handle <handle>` | The account handle. A leading `@` is dropped. |
| `--password-stdin` | Reads one line from standard input as the app password and never prompts. It requires `--handle`. |
| `--service <url>` | The server to sign in on, when it is not `bsky.social`. |

The handle comes from `--handle`, then `BLUESKY_HANDLE`, then a prompt. The app password comes from `--password-stdin`, then `BLUESKY_APP_PASSWORD`, then a hidden prompt, and the service comes from `--service`, then `BLUESKY_SERVICE`, then `https://bsky.social`. The environment variables are `BLUESKY_HANDLE`, `BLUESKY_APP_PASSWORD` and `BLUESKY_SERVICE`.

## dev/null

`socli login devnull` creates a device key and opens browser approval. On macOS, the browser sign-in window returns the result to SoCLI automatically. On other platforms, or with `SOCLI_AUTH_BROWSER=manual`, paste the `devnull://` callback address back into the terminal.

| Flag | What it does |
| --- | --- |
| `--device-name <name>` | The label the device shows in your dev/null settings. |

The default device name is `SoCLI on <short hostname>`, and names are limited to 100 characters. No variable selects a dev/null credential: SoCLI reads `HOSTNAME` or `COMPUTERNAME` for the default device name, and `HOME` to find `~/.ssh`.

`socli auth devnull ssh` picks the GitHub SSH key that signs your commits, which lets you post without an approved device. Use `--key <path>` to choose a key explicitly.

## Threads

`socli login threads` opens Threads and asks you to approve SoCLI. The terminal returns to `https://socli.sh/`, and socli.sh sends the browser on to `http://127.0.0.1:8742/threads/callback`. The code goes to `https://socli.sh/threads/token`, which returns a 60-day token. The app secret stays on that exchange. The full-screen reader returns to `https://socli.sh/web/`. Threads does not accept an `http://` redirect, so a reader on this machine cannot finish that approval. SoCLI's Threads app is compiled in. An empty `SOCLI_THREADS_APP_ID` at compile time is a build that says it has no app and does not ask you to create one. The Threads app is published.

`socli login threads --token <token>` still stores a token you already have. SoCLI checks it against `GET /me`. With `--app-secret`, SoCLI swaps it for a long-lived token and refreshes that token before it lapses. Without the secret, SoCLI stores the token with no expiry and never refreshes it, so a short-lived token stops working after an hour.

| Flag | What it does |
| --- | --- |
| `--token <token>` | A token you already have. Without it, and without `THREADS_ACCESS_TOKEN`, SoCLI opens the Threads approval. |
| `--app-secret <secret>` | Your own Meta app's secret, for a pasted token. With it SoCLI swaps that token for a long-lived one. |

The environment variables are `THREADS_ACCESS_TOKEN` and `THREADS_APP_SECRET`. `SOCLI_THREADS_APP_ID` replaces the compiled SoCLI app id, and it is read when SoCLI is compiled, not at run time. `socli auth threads oauth --app-id <id> --app-secret <secret> --redirect-uri <url>` is the personal-app flow and asks you to paste the address you land on. `--scopes <list>` overrides the requested scopes, and `socli auth threads refresh` refreshes a long-lived token.

## Mastodon

`socli login mastodon` asks for your server, registers SoCLI as an app on it, opens the authorization page, and asks you to paste the code back. An empty answer keeps `mastodon.social`.

| Flag | What it does |
| --- | --- |
| `--server <host>` | Your server, such as `fosstodon.org`. Without it SoCLI reads `MASTODON_SERVER` or asks. |
| `--token <token>` | An access token. With it SoCLI never opens a browser. |

The environment variables are `MASTODON_SERVER` and `MASTODON_ACCESS_TOKEN`. `socli login mastodon --token <token>` stores a token you created in Preferences > Development with the scopes `read write`, and it needs no browser.

## Accounts

`socli auth status` prints the config path, the default network and one entry per network, and it redacts every secret. `socli whoami` lists the accounts this process will use.

`socli auth default <network>` sets the network that `socli post` uses when neither `--net` nor `SOCLI_NET` is given. `socli logout <network>` signs out of one network, and `socli logout all` signs out of every network. The [CLI reference](commands.html#accounts-and-sign-in) also lists token commands and the `socli auth <network> login` aliases.

## Privacy

Sign-in stores tokens on your machine, or in the site's local storage when you use the reader on the web. The Threads approval code is the request that is sent to socli.sh. The [privacy policy](../privacy.html) says what is stored, [data deletion](../data-deletion.html) says how to remove Threads data from the app, and the [terms](../terms.html) say what socli.sh does not keep.
