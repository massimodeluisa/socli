# Bluesky: testing

Bluesky is checked in two layers. The offline layer runs the built-in demo data and the unit tests over recorded API responses. The live layer needs a Bluesky account and an app password, and only a person can run it. Related pages: [overview](README.md), [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md).

## Manual steps with the demo

The demo reads a bundled feed and sends nothing. Use a throwaway config and cache directory so that nothing lands in your real configuration:

```
D=$(mktemp -d)
export SOCLI_CONFIG_DIR=$D SOCLI_CACHE_DIR=$D/cache NO_COLOR=1
socli feed --demo --print --no-prompt --width 100 --no-icons --net bluesky
```

The first line should read `socli › bluesky › following`, and the tab row `[1 Following]  2 Discover  3 Bookmarks`. The demo authors have handles such as `@ada.bsky.social`.

| Check | Command | Expect |
| --- | --- | --- |
| Profile | `socli user @ada.bsky.social --demo --net bluesky --print --width 100 --no-icons` | A profile card, then that author's posts |
| Thread | `socli thread bluesky:p8 --demo --print --no-prompt --width 90 --no-icons` | The post with its replies |
| Search | `socli search sparkline --demo --net bluesky --print --width 90 --no-icons` | A results page headed `Search  sparkline` |
| Mentions | `socli notifications --demo --net bluesky --print --width 90 --no-icons` | A `Mentions` tab |
| JSON | `socli feed --demo --json --net bluesky` | Posts with `network` set to `bluesky` and `permalink` under `https://bsky.app/profile/` |
| Length | `socli post "hi" --demo --dry-run --net bluesky --json` | `length` 2 and `limit` 300 |
| Bold | `socli post "<b>hi</b>" --demo --dry-run --net bluesky --json` | `text` shows bold letters, `length` 2 |
| Raw | `socli post "<b>hi</b>" --demo --dry-run --raw --net bluesky --json` | `text` is `<b>hi</b>`, `length` 9 |

In the demo JSON, avatars appear as `demo://avatar/<user>` URLs. Settings changed in the reader and the `d` and `D` keys save to the real configuration even in demo mode, so do not touch them while testing.

## Manual steps with an account

These steps talk to the real service. Run them in a terminal and on an account you accept to post from.

1. Create an app password at https://bsky.app/settings/app-passwords.
2. Sign in: `socli login bluesky --handle <handle>`, and type the password at the hidden prompt. Check with `socli auth status`, which should show `@<handle>` and a redacted session.
3. Read: `socli feed --net bluesky --print`, then `socli feed discover --net bluesky --print`, `socli bookmarks --net bluesky --print`, `socli notifications --net bluesky --print`.
4. Search: `socli search <words> --net bluesky --print`. Signed out, the same command asks you to sign in.
5. Dry run a write without sending: `socli post "test" --net bluesky --dry-run`. It checks sign-in, capabilities and length.
6. Write: `socli post "<text>" --net bluesky`, then `socli like <id|url> --net bluesky`, `socli repost`, `socli bookmark`, `socli follow <@handle> --net bluesky` and `socli delete <id|url> --yes` on your own test post.
7. Long post: a text over 300 graphemes must fail with `Bluesky allows 300 characters; this post has N` and send nothing.
8. Refresh: after the access token expires, any command should succeed again without asking for a password (the refresh path in [auth](auth.md)).
9. Pipe sign-in: `printf '%s\n' "$APP_PASSWORD" | socli login bluesky --handle <handle> --password-stdin`.
10. Sign out: `socli logout bluesky`, and check that `socli auth status` reports `not signed in`.

## Unit tests and fixtures

Run them with `cargo test`. The fixtures are recorded API responses in `src/net/bluesky/fixtures/` and are loaded with `include_str!` in `src/net/bluesky/map.rs`.

| Fixture | Loaded by test |
| --- | --- |
| `author_feed.json` | `author_feed_maps_posts_quotes_reposts_and_reply_context` |
| `post_thread.json` | `thread_flattens_replies_under_the_root_post`, `rename_keeps_the_requested_id_on_the_page` |
| `profile.json` | `profile_maps_the_user` |
| `search.json` | `search_results_become_a_page` |

Tests with inline JSON in `map.rs` cover viewer state and counts (`viewer_state_and_counts_map_onto_the_post`), link cards and video (`facet_links_external_cards_and_video_map_onto_the_post`, `external_card_thumb_becomes_the_link_image`, `a_facet_link_and_its_card_merge_into_one_link`, `a_card_thumb_that_is_not_http_is_dropped`), thread ancestors (`thread_walks_parents_to_the_root_and_skips_gone_or_blocked_items`), bookmarks (`bookmarks_skip_items_that_are_not_posts`), notifications (`notifications_hydrate_in_batches_of_25_and_keep_order`, `hydrate_drops_uris_the_api_no_longer_returns`) and AT URIs (`at_uris_split_into_authority_collection_and_rkey`).

The other modules test their own parts:

| File | Tests |
| --- | --- |
| `src/net/bluesky/facets.rs` | `mentions_need_a_dot_and_a_word_start`, `links_lose_trailing_punctuation`, `tags_skip_digits_and_stop_at_punctuation`, `tags_over_sixty_four_graphemes_are_ignored`, `offsets_are_utf8_bytes_after_emoji`, `unresolved_mentions_get_no_facet`, `the_limit_counts_graphemes_not_bytes` |
| `src/net/bluesky/rpc.rs` | `expired_and_invalid_tokens_trigger_a_refresh`, `other_failures_do_not_trigger_a_refresh`, `answers_become_json_or_typed_errors` |
| `src/net/bluesky/auth.rs` | `the_pds_comes_from_the_did_document`, `the_service_and_handle_are_normalised_before_sign_in`, `a_session_falls_back_to_the_sign_in_service` |
| `src/net/bluesky/mod.rs` | `caps_follow_the_session_and_public_columns`, `a_session_needs_a_handle_a_did_and_a_token`, `a_reply_to_a_root_post_uses_it_as_root_and_parent`, `a_reply_to_a_reply_keeps_the_thread_root`, `the_post_record_carries_facets_reply_and_quote_only_when_present`, `the_timestamp_has_milliseconds_and_a_z`, `an_over_long_post_fails_before_any_request`, `a_public_client_asks_for_sign_in`, `ids_from_another_network_are_refused` |
| `src/net/mod.rs` | `targets_accept_ids_links_and_a_fallback_network`, `targets_explain_what_they_cannot_resolve`, `composer_counts_graphemes_only_where_the_network_does`, `profile_urls_follow_each_network`, `text_len_counts_the_way_each_network_does`, `scoped_ids_round_trip` |

`tests/cli.rs` runs the binary against the demo data, including the Bluesky scope header and tabs (`one_network_scope_shows_its_tabs_and_no_tags`).

## What needs an account

No test calls the live service. These paths are therefore checked only by hand: `createSession` and the PDS lookup against a real `didDoc`, token refresh against a real expiry, the write calls (`createRecord`, `deleteRecord`, bookmarks), rate-limit headers, and the signed-in feeds.

Back to [network overview](../README.md).
