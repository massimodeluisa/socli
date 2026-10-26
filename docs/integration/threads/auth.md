# Threads sign-in

`socli login threads` opens Threads so you can approve SoCLI's app. The authorize redirect is `https://socli.sh/`, which is the callback saved on the Threads app. socli.sh sends the browser on to `http://127.0.0.1:8742/threads/callback`, then the terminal posts the code to `https://socli.sh/threads/token`. That exchange holds the Threads app secret and returns a 60-day user token, or the one-hour token when the 60-day swap fails. SoCLI checks the token against `GET /me` and stores it. The page uses its own address as the redirect. The full-screen reader uses `https://socli.sh/web/`.

SoCLI's Threads app id is compiled in. Setting `SOCLI_THREADS_APP_ID` when SoCLI is compiled replaces it. An empty value makes the build say `this build has no SoCLI Threads app` and does not ask you to create an app. The app is published. Each permission is ready for testing. A public account that Meta has not cleared is told to submit for review or be a tester. Public-profile grants last 90 days, and a refresh extends them. A private profile has to approve again.

Two personal methods stay: pasting a token, and a browser authorization where you paste back the address you land on. Those use your own app id and secret. The secret never goes in the SoCLI binary.

Back to [Threads](README.md) or the [integration overview](../README.md). Related: [endpoints](endpoints.md), [mapping](mapping.md), [testing](testing.md).

## Token lifetimes

Meta's documentation says a Threads user access token is short-lived by default and valid for one hour, and that a short-lived token can be exchanged for a long-lived token valid for 60 days. A long-lived token can be refreshed when it is at least 24 hours old and has not expired, and a refresh makes it valid for 60 days again. A token that is not refreshed within 60 days expires (checked 2026-10-20). The long-lived exchange needs the app secret. For SoCLI's app that secret stays on the exchange. For a token you paste yourself, pass `--app-secret` or set `THREADS_APP_SECRET`.

## The SoCLI app

The Meta app uses the Threads use case. The exchange and the client use the Threads app id and the Threads app secret. Meta also issues a Facebook pair on the same app. That pair is not the one SoCLI sends. The Threads app has these redirects saved:

```
https://socli.sh/
https://socli.sh/web/
```

Meta rejects an `http://` redirect, including `http://127.0.0.1`. The worker allowlist also contains those local addresses and `https://web.socli.sh/web/`, which does not resolve. The uninstall callback is `https://socli.sh/threads/deauthorize`. The delete callback is `https://socli.sh/threads/delete`. `https://socli.sh/threads/deletion` answers the same delete request.

The Worker source is `workers/threads/index.js`. `THREADS_APP_SECRET` is a Worker secret. The public app id is compiled into SoCLI. The script allowlists the redirects above, posts the code to `https://graph.threads.com/oauth/access_token`, swaps the short-lived token for a long-lived one, and returns `access_token`, `expires_in` and `user_id`. A failed swap still returns the one-hour token. A failed code exchange returns Meta's message, code, and subcode. It does not return the secret. Deploy is manual.

Reading needs `threads_basic`. Publishing also needs `threads_content_publish`. The default scope list is `threads_basic,threads_content_publish,threads_read_replies,threads_manage_replies,threads_manage_mentions,threads_keyword_search,threads_delete`. Meta's access-token page is [Get Access Tokens](https://developers.facebook.com/documentation/threads/get-started/get-access-tokens-and-permissions).

## Paste an access token

```
socli login threads --token <token>
socli login threads --token <token> --app-secret <secret>
socli auth threads login --token <token>
```

`socli auth threads login` takes the same flags. The flags:

| Flag | Meaning |
| --- | --- |
| `--token <token>` | A token you already have. Without it SoCLI reads `THREADS_ACCESS_TOKEN`, and without that it opens the Threads approval. |
| `--app-secret <secret>` | Your Meta app's secret. Without it SoCLI reads `THREADS_APP_SECRET`. With a secret, SoCLI swaps the token for a long-lived one (`GET https://graph.threads.com/access_token` with `grant_type=th_exchange_token`). If the swap fails it prints "could not swap for a long-lived token, keeping the one you gave" and keeps your token. |

After the token is settled, SoCLI calls `GET /me` to check it. A rejected token stops the sign-in. Otherwise SoCLI stores the account and the token and prints "token valid until YYYY-MM-DD", or, when the lifetime is unknown, "token lifetime unknown; add --app-secret to swap it for a 60-day token that renews itself".

Without an app secret SoCLI has no expiry date from Meta, so it stores the token with no expiry and never refreshes it on its own. A short-lived token pasted without a secret still expires after an hour, so give the secret, or use a token that is already long-lived. With the secret, SoCLI exchanges the token for a 60-day token, which it refreshes once the token is over a day old and in its last 7 days. When the exchange fails, SoCLI keeps the token you gave, with an unknown lifetime. `socli auth threads oauth` always ends in the 60-day token.

## Browser authorization

```
socli auth threads oauth --app-id <id> --app-secret <secret> --redirect-uri <url> [--scopes <list>]
```

| Flag | Meaning |
| --- | --- |
| `--app-id <id>` | The Threads app id from your Meta app dashboard. |
| `--app-secret <secret>` | The app secret. |
| `--redirect-uri <url>` | A redirect URI registered in the app. |
| `--scopes <list>` | Comma-separated permissions. Default: `threads_basic,threads_content_publish,threads_read_replies,threads_manage_replies,threads_manage_mentions,threads_keyword_search,threads_delete`. |

SoCLI prints the authorize URL (`https://threads.com/oauth/authorize` with `client_id`, `redirect_uri`, `scope`, `response_type=code` and a random `state`), tries to open it in your browser, and asks you to paste the address your browser lands on. SoCLI runs no local listener, so the redirect URI may point anywhere you control, and the page it lands on may even fail to load. From the pasted address SoCLI removes a trailing `#_`, checks that `state` matches, and reads the `code` (or reports that you denied access). It then exchanges the code (`POST https://graph.threads.com/oauth/access_token`), swaps the short-lived token for a long-lived one, and finishes like the paste-in sign-in. If the swap fails, the short-lived token is not kept and SoCLI asks you to run the command again, because that token lasts only an hour.

## What is stored

The `[threads]` section of `config.toml` (`$SOCLI_CONFIG_DIR/config.toml`, else `$XDG_CONFIG_HOME/socli/config.toml`, else `~/.config/socli/config.toml`; mode 0600 in a 0700 directory). There is no key file.

| Key | Content |
| --- | --- |
| `user_id`, `username` | Your account. |
| `access_token` | The token. |
| `issued_at`, `expires_at` | Unix times of the sign-in or last refresh and of the expected expiry. |

## Environment variables

| Variable | Use |
| --- | --- |
| `THREADS_ACCESS_TOKEN` | The token, used by `socli login threads` when no `--token` is given, and used by every command instead of the token in the file. A token that comes from the environment is never refreshed. |
| `THREADS_APP_SECRET` | The app secret for the long-lived swap, when no `--app-secret` is given. |

## Refresh

SoCLI refreshes on its own each time it loads the stored token, when the token is more than a day old and expires within the next seven days (`GET https://graph.threads.com/refresh_access_token` with `grant_type=th_refresh_token`). A token stored with no known expiry is never refreshed automatically. A failed automatic refresh keeps the token you have. An expired token is not refreshed. Meta rejects it with error code 190, which SoCLI reports as "sign in first: `socli login threads`".

To refresh by hand:

```
socli auth threads refresh
```

It always asks Meta to refresh the stored token, whatever its recorded expiry, and a long-lived token over a day old is refreshed: SoCLI records the new 60-day expiry and prints "Refreshed the Threads token, valid until YYYY-MM-DD". Meta accepts a refresh only for a token that is at least a day old and not yet expired. When it refuses, SoCLI reports the error followed by "tokens refresh once they are a day old and before they expire, otherwise run `socli login threads`".

## Status and sign-out

```
socli auth status
socli logout threads
socli auth threads logout
```

`socli auth status` shows the account and the token as `set` or its last four characters. Sign-out resets the whole `[threads]` section, so the next sign-in starts from nothing.

## Agents and scripts

`socli login threads --token <token>` and `socli auth threads refresh` need no person, so an agent can use them. Every prompt refuses when nobody can answer it: stdin and stderr must both be terminals and agent mode must be off. The refusal is a usage error with exit code 2 and a message that starts "this step needs a person at a terminal.". So `socli login threads` with no `--token` and no `THREADS_ACCESS_TOKEN` fails in agent mode instead of waiting. Pass the token as an argument. `socli auth threads oauth` opens a browser and asks for a pasted address, so agent mode (`SOCLI_AGENT=1`, or an agent SoCLI recognizes from its environment) refuses it with a usage error and exit code 2: "signing in to Threads needs a person at a browser. Run socli auth threads oauth in a terminal".
