# Mastodon: testing

Mastodon is checked in two layers. The offline layer runs the built-in demo data and the unit tests over synthetic API responses. The live layer needs a Mastodon account, and only a person can run it. The client has not been run against a live account yet, so the live layer is a checklist and not a record. Related pages: [overview](README.md), [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md).

## Manual steps with the demo

The demo reads bundled posts and sends nothing. Use a throwaway config and cache directory so that nothing lands in your real configuration:

```
D=$(mktemp -d)
export SOCLI_CONFIG_DIR=$D SOCLI_CACHE_DIR=$D/cache NO_COLOR=1
socli feed --demo --print --no-prompt --width 100 --no-icons --net mastodon
```

The first line should read `SoCLI › mastodon › home`, and the tab row `[1 Home]  2 Trending  3 Bookmarks`. The demo post is by Ines Duarte (`@ines@mastodon.social`), with one reply by Theo Lang (`@theo@fosstodon.org`). The first lines of the output:

```
  SoCLI › mastodon › home                                                                     demo

  All  ○ X  ○ Bluesky  ○ dev/null  ○ Threads  [● Mastodon]  │  [1 Home]  2 Trending  3 Bookmarks
```

| Check | Command | Expect |
| --- | --- | --- |
| Profile | `socli user @ines@mastodon.social --demo --net mastodon --print --width 90 --no-icons` | A profile card, then that author's posts |
| Thread | `socli thread mastodon:p11 --demo --print --no-prompt --width 90 --no-icons` | The post with Theo Lang's reply |
| Search | `socli search full --demo --net mastodon --print --width 90 --no-icons` | A results page headed `Search  full` |
| Mentions | `socli notifications --demo --net mastodon --print --width 90 --no-icons` | A `Mentions` tab |
| Favorite | `socli like mastodon:p11 --demo` | `✓ Mastodon  demo · favorited here only` |
| JSON | `socli feed --demo --json --net mastodon` | Posts with `network` set to `mastodon` and `permalink` such as `https://mastodon.social/@ines/p11` |
| Length | `socli post "hi" --demo --dry-run --net mastodon --json` | `length` 2 and `limit` 500 |
| Bold | `socli post "<b>hi</b>" --demo --dry-run --net mastodon --json` | `text` shows bold letters, `length` 2 |
| Raw | `socli post --raw "<b>hi</b>" --demo --dry-run --net mastodon --json` | `text` is `<b>hi</b>`, `length` 9 |

The thread command prints the post and its reply:

```
   ╭───╮  Ines Duarte @ines@mastodon.social
   │ I │  Boosts show who shared a post above it, the way the web app does.
   ╰─┬─╯  @theo@fosstodon.org was right about full handles.
     ╎    ↩ 1       ⇄ 3        ♡ 12       ☆
     ╎
     ╎   ╭───╮  Theo Lang @theo@fosstodon.org · 32m
     ╰╌╌╌┤ T │  Full handles look long, but they tell you which server answers.
         ╰───╯
```

The `--dry-run` JSON for the first length check:

```
{
  "network": "mastodon",
  "ok": true,
  "demo": true,
  "dry_run": true,
  "text": "hi",
  "length": 2,
  "limit": 500
}
```

Settings changed in the reader and the `d` and `D` keys save to the real configuration even in demo mode, so do not touch them while testing. Remove the directory afterwards with `rm -rf "$D"`.

## Manual steps with an account

These steps talk to the real service. Run them in a terminal and on an account you accept to post from.

1. Sign in with the browser path: `socli login mastodon --server <host>`, open the address, approve SoCLI and paste the code. Or create a token in Preferences > Development with the scopes `read write` and run `socli login mastodon --server <host> --token <token>`. Check with `socli auth status`, which should show `@<user>@<host>`, a redacted token, the server and `registered` or `none` for the app.
2. Read: `socli feed --net mastodon --print`, then `socli feed discover --net mastodon --print` (Trending), `socli bookmarks --net mastodon --print` and `socli notifications --net mastodon --print`.
3. Open a thread from the feed, then a link from another server: `socli thread https://<other-server>/@<user>/<id> --net mastodon --print`. It resolves through search and needs the session.
4. Search: `socli search <words> --net mastodon --print`, and `socli search "#tag" --net mastodon --print` for the hashtag timeline. Signed out, the same commands ask you to sign in.
5. Dry run a write without sending: `socli post "test" --net mastodon --dry-run`. It checks sign-in, capabilities and length.
6. Reply with a dry run first: `socli reply <id|url> "test" --net mastodon --dry-run`, and check that the text starts with the author's handle. Then send it without `--dry-run`.
7. Write: `socli post "<text>" --net mastodon`, then `socli like <id|url> --net mastodon`, `socli repost`, `socli bookmark`, `socli follow <@user@host> --net mastodon` and `socli delete <id|url> --yes` on your own test post.
8. Quote: `socli quote <id|url> "<text>" --net mastodon --dry-run`, on a server that runs Mastodon 4.5 or newer and on an older one if you have access to both. Expect a native quote on the first and the link appended on the second.
9. Long post: a text over the server's limit must fail with `Mastodon allows N characters on <host>; this post has M` and send nothing.
10. Rate limit: the 429 path is easiest to check against a test server with a low limit. Expect `Mastodon rate limit reached; try again in …`.
11. Revoked token: revoke the app in your server's settings, then run any command. It should answer `<host> no longer accepts the saved token. run `socli login mastodon` again`.
12. Sign out: `socli logout mastodon`, and check that `socli auth status` reports `not signed in`. With the browser path, the server also drops the token.

## Unit tests and fixtures

Run them with `cargo test`. The fixtures are synthetic API responses in `src/net/mastodon/fixtures/` and are loaded with `include_str!` in `src/net/mastodon/map.rs`.

| Fixture | Loaded by test |
| --- | --- |
| `home.json` | `a_home_page_maps_links_cards_media_boosts_polls_and_quotes` |
| `status.json`, `context.json` | `a_thread_runs_from_the_root_through_the_replies`, `feed_conversations_climb_to_the_topmost_parent_in_the_page` |
| `account.json` | `profiles_notifications_and_search_map` |
| `notifications.json` | `profiles_notifications_and_search_map` |
| `search.json` | `profiles_notifications_and_search_map` |

`instance.json` sits in the folder and no test loads it.

The other modules test their own parts:

| File | Tests |
| --- | --- |
| `src/net/mastodon/html.rs` | `paragraphs_breaks_entities_and_links`, `mentions_get_their_full_handle_and_hashtags_stay_text`, `the_quote_fallback_paragraph_is_dropped`, `lists_become_bullets` |
| `src/net/mastodon/auth.rs` | `server_names_are_normalized`, `the_authorize_url_asks_for_a_code_with_pkce` |
| `src/net/mastodon/mod.rs` | `length_counts_links_as_23_and_mentions_without_their_server`, `profile_links_go_to_the_handle_server`, `link_headers_become_cursors`, `rate_limits_wait_until_the_reset_time`, `caps_follow_the_session`, `a_public_client_asks_for_sign_in`, `ids_from_another_network_are_refused`, `the_section_and_the_environment_pick_the_server_and_token` |
| `src/net/mod.rs` | `mastodon_links_keep_their_url_as_the_id`, which also checks the `masto` alias, the slug and the title |

`tests/cli.rs` runs the binary against the demo data. `x_has_no_trending_feed` also expects the merged Trending feed to hold only dev/null and Mastodon posts,.

## What needs an account

No test calls a live server. These paths are therefore checked only by hand: app registration and the token exchange against a real server, `verify_credentials` and the instance limit, the write calls (`statuses`, favourite, reblog, bookmark, follow, delete), native quotes on a 4.5 server, the signed-in feeds, search, rate-limit headers and the revoke call on sign-out.

Back to [network overview](../README.md).
