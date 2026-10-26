# Testing Threads

Every call to the Threads API needs a token for a Meta app, so what SoCLI can check without an account is limited. The demo mode shows the screens and the limits, the dry run shows what a post would do, and the unit tests run against JSON files stored under `src/net/threads/fixtures/`. The OAuth exchanges, the container flow and the permission rules are checked only with a real account.

Back to [Threads](README.md). Related: [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md).

## Without an account

Run these from the repository root after `cargo build`. The first two lines make a throwaway configuration and cache directory so nothing touches `~/.config/socli`:

```
D=$(mktemp -d)
export SOCLI_AGENT=0 SOCLI_CONFIG_DIR=$D SOCLI_CACHE_DIR=$D/cache NO_COLOR=1
```

The Threads demo feed has one tab, `1 Your threads`:

```
./target/debug/socli --demo feed --net threads --print --no-prompt --width 100 --no-icons
./target/debug/socli --demo thread threads:l4 --print --no-prompt --width 100
./target/debug/socli --demo feed --net threads --json
```

The JSON output holds scoped ids such as `threads:l4` and `threads:p10`, permalinks such as `https://www.threads.com/@kal/post/l4`, and `demo://avatar/<user>` avatars. The posts carry no counts, because Threads provides none.

The dry run checks length and capability without sending:

```
./target/debug/socli --demo post "hello <b>bold</b>" --net threads --dry-run
./target/debug/socli --demo like threads:l4 --dry-run
./target/debug/socli --demo repost threads:l4 --dry-run
./target/debug/socli --demo unrepost threads:l4 --dry-run
./target/debug/socli --demo quote x:p1 "hi" --net threads --dry-run
```

The post reports `would post · 10/500`. The like fails with "Threads has no likes in its API", the repost succeeds, the undo fails with "Threads has no repost undo in its API", and the quote of an X post reports 32 of 500 because the text and the X link are posted as plain text.

A Threads post link is refused (the unit test is `targets_explain_what_they_cannot_resolve` in `src/net/mod.rs`):

```
./target/debug/socli --demo thread https://www.threads.com/@kal/post/l4 --print
```

Remove the directory when you are done: `rm -r "$D"`.

## With an account

These steps need a Meta app with the Threads use case and a Threads account that can use it. See [auth](auth.md) for the token. Posts you publish are real posts, so delete the test ones.

1. Sign in with a token: `socli login threads --token <token> --app-secret <secret>`. SoCLI prints the account and "token valid until YYYY-MM-DD". Without `--app-secret` it prints "token lifetime unknown; add --app-secret to swap it for a 60-day token that renews itself" instead. A wrong token fails at the `GET /me` check.
2. Check what was stored: `socli auth status` (the token shows `set` or its last four characters) and `socli whoami`.
3. Read: `socli feed mine --net threads --print`, `socli notifications --net threads --print`, `socli search rust --net threads --print`, `socli thread <id> --print`, and `socli user @<your handle> --net threads --print`. A search before Meta approves `threads_keyword_search` shows only your own posts. `socli user @<someone else> --net threads` fails with "threads has no profiles of other accounts in its API".
4. Write and clean up: `socli post "test" --net threads`, then `socli reply <id> "test" --net threads`, then `socli repost <other id> --net threads`, then `socli delete <id> --net threads` for each test post. SoCLI cannot undo the repost, so pick a post you do not mind reposting.
5. Quote: `socli quote <id> "text" --net threads` posts a native quote.
6. Limits: a post of 501 characters, an empty post and a post with six distinct links must each fail before any request (`--dry-run` shows the length without sending).
7. Refresh: `socli auth threads refresh` works once the token is at least a day old.
8. Browser sign-in (needs a person): `socli auth threads oauth --app-id <id> --app-secret <secret> --redirect-uri <url>`, then paste the address you land on.
9. Sign out: `socli logout threads`.

Agent behavior: with `SOCLI_AGENT=1`, `socli auth threads oauth` must exit with code 2 and the message "signing in to Threads needs a person at a browser. Run socli auth threads oauth in a terminal".

## Unit tests and fixtures

All of these run with `cargo test`. The fixtures are Meta-shaped responses in `src/net/threads/fixtures/`.

| Fixture | Loaded by (in `src/net/threads/map.rs`) | What it covers |
| --- | --- | --- |
| `media_page1.json` | `media_list_maps_ids_urls_and_text`, `carousel_children_become_media_and_link_is_kept`, `image_post_keeps_alt_text`, `quote_target_is_a_stub_until_fetched` | a first page with a carousel, an image with alt text, a link attachment and a quote target |
| `media_page_last.json` | `media_list_pages_on_next_and_ends_without_it`, `a_full_page_without_next_still_offers_a_cursor`, `repost_facade_becomes_the_original_marked_reposted_by` | the last page, the cursor rule and a repost |
| `conversation.json` | `conversation_rebuilds_reply_links`, `claim_own_points_the_account_posts_at_the_user_id` | reply links and own-account ids |
| `mentions.json` | `mentions_map_with_a_timestamp` | mentions |
| `error_token.json` | `expired_token_asks_to_sign_in` | error code 190 |
| `error_rate.json` | `rate_limit_codes_map_to_rate_limited` | rate-limit codes |
| `error_permission.json` | `permission_errors_say_what_to_run` | permission errors |
| `error_oauth.json` | `oauth_endpoint_errors_use_error_message` | OAuth endpoint errors |

Tests in `map.rs` that use no fixture: `timestamps_parse_meta_offsets_and_rfc3339`, `profile_maps_fields`, `http_429_keeps_retry_after`, `other_codes_keep_message_and_code`, `non_json_failures_name_the_status`, `decode_rejects_a_non_json_success`, `container_status_maps`.

Tests in `src/net/threads/auth.rs`: `redirect_strips_the_trailing_marker`, `redirect_strips_the_marker_when_state_comes_last`, `redirect_rejects_a_wrong_state`, `redirect_reports_a_denied_authorization`, `redirect_rejects_text_that_is_not_a_url`, `redirect_without_a_code_is_rejected`, `authorize_url_carries_scopes_and_state`, `refresh_waits_until_the_token_is_a_day_old`, `refresh_waits_until_the_last_week`, `refresh_runs_inside_the_window`, `refresh_skips_an_expired_token`, `refresh_skips_a_token_with_unknown_dates`, `default_scopes_include_delete`, `scoped_ids_round_trip`.

Tests in `src/net/threads/mod.rs`: `caps_match_the_threads_feature_set`, `five_hundred_characters_are_allowed`, `five_hundred_and_one_characters_are_rejected`, `empty_text_is_rejected`, `native_ids_accept_bare_and_scoped_threads_ids`, `unsupported_actions_name_the_missing_feature`, `over_long_posts_fail_before_any_request`, `status_redacts_the_token`, `text_len_counts_ascii_and_accents_per_char`, `text_len_counts_emoji_as_bytes`, `text_len_sums_a_zwj_family`, `text_len_counts_a_flag`, `emoji_bytes_count_toward_the_limit`, `five_links_are_allowed`, `six_links_are_rejected`, `repeated_links_count_once`.

Shared tests that touch Threads: `src/net/mod.rs` has `targets_explain_what_they_cannot_resolve` and `text_len_counts_the_way_each_network_does`, and `src/rich.rs` holds the `<b>` and `<i>` conversion tests.

## What needs an account

The OAuth and token calls, the container status flow before publish, the effect of Meta's app review (search scope, publishing for non-testers), the real response of `GET /me/mentions` and the conversation endpoint, rate-limit codes in production and token refresh after a day. The fixtures show that SoCLI parses the shapes written into them, not that Meta still returns those shapes.
