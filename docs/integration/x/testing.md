# Testing X

The X client has two kinds of checks. The demo mode and the dry run exercise SoCLI's screens, ids, quote handling and length counter without any account. The unit tests cover parsing, cookie decryption, token handling and counting with inline JSON, and they never call X. Anything that needs X's servers is marked below.

Back to [X](README.md). Related: [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md).

## Without an account

Run these from the repository root after `cargo build`. The first line makes a throwaway configuration and cache directory so nothing touches `~/.config/socli`:

```
D=$(mktemp -d)
export SOCLI_AGENT=0 SOCLI_CONFIG_DIR=$D SOCLI_CACHE_DIR=$D/cache NO_COLOR=1
```

The X demo feed shows the three tabs of a signed-in account (`1 For you  2 Following  3 Bookmarks`):

```
./target/debug/socli --demo feed --net x --print --no-prompt --width 100 --no-icons
./target/debug/socli --demo thread x:p1 --print --no-prompt --width 100
./target/debug/socli --demo feed --net x --json
```

The JSON output holds scoped ids such as `x:l3` and `x:p1`. Demo avatar URLs read `demo://avatar/<user>`.

The trending feed is not built for X, and the command says so:

```
./target/debug/socli --demo feed trending --net x --print --no-prompt --width 100
```

It prints "SoCLI: X has no trending feed in SoCLI yet. Try `socli search <words> --net x`".

Write commands accept `--dry-run`, which checks the length and the target without sending anything:

```
./target/debug/socli --demo post "hello <b>bold</b>" --net x --dry-run
./target/debug/socli --demo post "hello <b>bold</b>" --net x --dry-run --raw
./target/debug/socli --demo quote x:p1 "this" --net x --dry-run
./target/debug/socli --demo like x:p1 --dry-run
```

The first command reports `would post · 14/280`: the bold letters of "bold" count two each. The `--raw` run reports 17, because it counts the text as typed, tags included. The quote reports 29 of 280, which is the four letters of "this", a blank line, and the 23 points a link counts.

Look up the exact flags of the sign-in commands:

```
./target/debug/socli help login
./target/debug/socli help auth
./target/debug/socli help logout
```

Remove the directory when you are done: `rm -r "$D"`.

## With an account

The default sign-in uses a browser that is already signed in at https://x.com. It does not spend official API credits.

1. Sign in: `SOCLI_AGENT=0 socli login x`. SoCLI prints the account and the browser profile it read.
2. Check what was stored: `socli auth status` (the browser session shows `missing`, `set` or only its last four characters) and `socli whoami`.
3. Read: `socli feed following --net x --json`, `socli feed --net x --json` for For you, `socli feed latest --net x --json`, `socli bookmarks --net x --json`, `socli notifications --net x --json`, `socli search rust --net x --json`, `socli user @handle --net x --json`, `socli thread <post link> --json`.
4. On a post you can undo: `socli bookmark <id> --net x` then `socli unbookmark <id> --net x`, and `socli like <id> --net x` then `socli unlike <id> --net x` when the post is not already liked. Do not unfollow someone to test follow. A follow of an account you already follow comes back as already following and leaves the state as it was.
5. Sign out: `socli logout x`. `socli auth status` then shows no browser session. A stored client id stays.

The official API path uses a developer account and real credits. Reads are billed per post returned and writes per request (see [pricing](https://docs.x.com/x-api/getting-started/pricing), checked 2026-10-20), so keep pages small.

1. Create an app in the developer console, enable OAuth 2.0, and add the callback `http://127.0.0.1:8741/callback` (see [auth](auth.md)).
2. Sign in: `socli login x --oauth --client-id <id>`. A browser opens. Approve the app. SoCLI prints the signed-in account.
3. Check what was stored: `socli auth status` (the user token, app bearer and client id each show `missing`, `set` or only their last four characters) and `socli whoami`.
4. Read the same commands as above. For you on this path is the `latest_query` search.
5. Write on a post you own, then undo each step: `socli post "test" --net x`, `socli like <id> --net x` then `socli unlike <id> --net x`, `socli bookmark <id> --net x` then `socli unbookmark <id> --net x`, `socli repost <id> --net x` then `socli unrepost <id> --net x`, and `socli delete <id> --net x` for the test post. Like and follow calls may fail with the Enterprise hint described on the [overview page](README.md).
6. Quote: `socli quote <id> "text" --net x` posts your text plus the link of the quoted post.
7. Token refresh: after the access token expires (two hours without `offline.access`), run any read again. SoCLI refreshes once and carries on. This cannot be checked without waiting for the expiry.
8. Sign out: `socli logout x`. `socli auth status` then shows no user token, and the client id stays.

Agent behavior: with `SOCLI_AGENT=1`, `socli login x` must exit with code 2 and the message "signing in to X needs a person at a browser. Run socli login x in a terminal".

## Unit tests

All of these run with `cargo test`. The X client keeps its test responses inline in the source, so there is no `fixtures` directory for X.

| File | Tests | What they check |
| --- | --- | --- |
| `src/net/x/client.rs` | `link_images_pick_the_widest_web_image`, `x_links_carry_the_widest_preview_image` | link card image selection from `entities.urls` |
| `src/net/x/client.rs` | `parses_a_thread_with_media_and_a_parent` | posts, users, media, replies and parent links from one response |
| `src/net/x/client.rs` | `post_bodies_never_carry_a_quote_id` | the `POST /2/tweets` body holds `text` and `reply` only |
| `src/net/x/client.rs` | `like_forbidden_mentions_the_self_serve_removal` | the hint on a refused like |
| `src/net/x/client.rs` | `rate_limits_wait_for_the_reset_time` | the wait from `x-rate-limit-reset`, with `retry-after` as the fallback |
| `src/net/mod.rs` | `rate_limit_waits_read_in_seconds_or_minutes` | the wording of the wait for every network |
| `src/net/x/mod.rs` | `a_quote_draft_fails_before_any_request` | an official-API quote stops before any HTTP call |
| `src/net/x/auth.rs` | `env_wins_over_the_file`, `env_overlays_a_bearer_without_a_file`, `a_browser_session_counts_as_signed_in` | `X_*` variables override stored credentials, and a cookie pair counts as signed in |
| `src/net/x/cookies.rs` | `v10_drops_the_binary_prefix_and_returns_the_cookie`, `v10_keeps_a_cookie_that_is_already_text`, `twid_reads_a_percent_encoded_user_id` | Chromium `v10` cookie decryption and the `twid` user id |
| `src/net/x/web.rs` | `ops_regex_reads_the_bundle_shape`, `timeline_keeps_order_skips_promoted_and_reads_the_cursor`, `a_retweet_keeps_the_original_id_and_names_the_reposter` | query ids from the web bundle, timeline order, and retweets |
| `src/net/pkce.rs` | `pkce_matches_the_rfc_vector`, `verifiers_are_long_and_unreserved` | the PKCE challenge against the RFC example, verifier length and alphabet |
| `src/count.rs` | `ascii`, `unicode_bold_letters_count_two`, `accents_count_one`, `cjk_counts_two`, `urls_count_23`, `emoji_graphemes_count_two`, `curly_quotes_count_one`, `whitespace_counts_one` | X length weighting |
| `src/net/mod.rs` | `targets_accept_ids_links_and_a_fallback_network`, `text_len_counts_the_way_each_network_does` | `x.com` links and the X length |
| `src/rich.rs` | `bold`, `italic_leaves_digits`, `nested_is_bold_italic_with_bold_digits` and the other conversion tests | the `<b>` and `<i>` conversion |
| `tests/cli.rs` | `x_has_no_trending_feed` | the trending message |

## What needs an account

The authorize and token exchange against `x.com` and `api.x.com`, the exact fields X returns today, rate-limit and credit errors (402 and 429), `note_tweet` long posts, the refresh after expiry, and the self-serve refusals on likes, follows and replies. The unit tests show that SoCLI parses the response shapes written into them, not that X still sends those shapes today.
