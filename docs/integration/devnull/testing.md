# dev/null: testing

dev/null is checked in two layers. The offline layer uses the built-in demo data and the unit tests over a recorded commit list and recorded SSH signatures. The live layer needs a dev/null account, a GitHub SSH key or a device approval, and a person at a browser. Related pages: [overview](README.md), [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md).

## Manual steps with the demo

The demo reads a bundled feed and sends nothing. Use a throwaway config and cache directory:

```
D=$(mktemp -d)
export SOCLI_CONFIG_DIR=$D SOCLI_CACHE_DIR=$D/cache NO_COLOR=1
socli feed --demo --print --no-prompt --width 100 --no-icons --net devnull
```

The first line should read `socli › dev/null › following`, and the tab row `[1 Following]  2 Latest  3 Trending  4 Starred`. The demo author is `@nuno`.

| Check | Command | Expect |
| --- | --- | --- |
| Starred | `socli feed saved --demo --net devnull --print --width 80 --no-icons` | The crumb `socli › dev/null › starred` and tab 4 (Starred) selected |
| Bookmark | `socli bookmark devnull:p3 --demo` | `✗ dev/null  dev/null has no bookmarks in its API` on the error output, exit status 1 |
| Thread | `socli thread devnull:p3 --demo --print --width 80 --no-icons` | The post and its replies |
| JSON | `socli feed --demo --json --net devnull` | `network` is `devnull` and `permalink` is under `https://dev-null.sh/@` |
| Star | `socli like p3 --demo --net devnull --json` | `ok: true` and a `url` under `https://dev-null.sh/@` |
| Italic | `socli post "<i>hi</i> there" --demo --dry-run --net devnull --json` | `text` shows italic letters and `limit` is 4000 |
| Trending alias | `socli feed discover --demo --net devnull --print --width 80 --no-icons` | The Trending feed |

In the demo JSON, avatars appear as `demo://avatar/<user>` URLs. Settings changed in the reader and the `d` and `D` keys save to the real configuration even in demo mode, so do not touch them while testing.

## Manual steps with an account

These steps talk to the real service.

1. Public read, no sign-in: `socli user @<username> --net devnull --print`, then `socli thread <https://dev-null.sh/@user/commits/id> --print`.
2. SSH login: run `socli auth devnull ssh`, then `socli auth status` to see the redacted key. Post with `socli post "<text>" --net devnull` and confirm the printed link opens the commit.
3. Device login: `socli login devnull`, open the printed URL, approve the device, copy the `devnull://auth/callback?...` address into the `Callback: ` prompt. `socli auth status` should show `approved (devnull_ed25519)`.
4. Signed reads: `socli feed --net devnull --print`, `socli feed latest --net devnull --print`, `socli feed trending --net devnull --print`, `socli notifications --net devnull --print`, `socli feed saved --net devnull --print` (your starred commits). Listing notifications marks them read.
5. Writes: `socli like <id|url> --net devnull` (a star), `socli follow <@user> --net devnull`, `socli delete <id|url> --yes --net devnull` on your own test commit. Preview any of them first with `--dry-run`.
6. Clock check: shift the machine clock by several minutes and run a signed read; it should fail with `Signature expired. Check that your clock is right.`
7. Sign out: `socli logout devnull`, then revoke the device at https://dev-null.sh/settings/devices.

Posting with the SSH key needs `ssh-keygen`; without it the post fails with `ssh-keygen is required for dev/null posting with your GitHub key`.

## Unit tests and fixtures

Run them with `cargo test`.

| Fixture | Loaded by test |
| --- | --- |
| `src/net/devnull/fixtures/pub-commits.json` | `public_commits_parse_into_devnull_posts`, `link_previews_carry_their_image_and_site` (in `map.rs`) |
| `src/net/devnull/fixtures/ssh/ed25519.pub`, `ed25519.sig`, `message`, `rsa.pub`, `rsa.sig` | `upstream_fixture_signatures_parse_and_the_ed25519_one_verifies` (in `sign.rs`) |

| File | Tests |
| --- | --- |
| `src/net/devnull/map.rs` | `conversations_follow_parents_inside_the_page`, `signed_commits_carry_stars_saves_parents_and_quotes`, `deleted_commits_become_placeholders_that_keep_their_replies`, `link_display_drops_the_scheme_www_and_trailing_slash` |
| `src/net/devnull/sign.rs` | `canonical_message_joins_the_five_parts_with_newlines`, `canonical_message_without_a_body_ends_with_the_separator`, `nonces_are_32_lowercase_hex_characters`, `signatures_are_one_line_of_base64_that_parses_back`, `ssh_keygen_verifies_the_signature_over_a_canonical_message`, `a_new_key_is_written_once_with_mode_0600_and_reused_after` |
| `src/net/devnull/ssh.rs` | `key_ids_drop_the_comment_and_other_key_types`, `candidates_are_deduplicated_and_capped`, `the_lookup_answer_must_name_one_of_the_candidates`, `payload_bytes_for_a_top_level_commit_and_a_reply`, `the_commit_answer_gives_the_link_id_and_user`, `ssh_keygen_signs_the_payload_for_the_commit_namespace` |
| `src/net/devnull/auth.rs` | `the_authorize_url_carries_every_parameter`, `callbacks_parse_from_the_full_address_or_a_bare_code`, `callbacks_with_another_state_an_error_or_no_code_are_refused`, `device_names_use_the_short_hostname_then_the_environment`, `key_paths_resolve_to_a_public_key_file` |
| `src/net/devnull/api.rs` | `the_signed_target_is_the_path_and_query_as_sent`, `error_codes_map_to_next_steps`, `plain_text_errors_keep_their_message_and_html_does_not` |
| `src/net/devnull/mod.rs` | `access_follows_the_stored_keys`, `caps_follow_the_access_state`, `actions_outside_the_access_ask_to_sign_in_and_missing_ones_are_unsupported`, `ids_from_other_networks_are_refused` |

`tests/cli.rs` runs the binary against the demo data, including a dev/null star and the trending feed.

## What needs an account

No test calls the live service. By hand only: the device approval and its callback, real signature acceptance by the server (the fixtures prove that socli's signatures verify with `ssh-keygen`, not that the server accepts them), the GitHub key lookup, commit posting through `/api/cli/commits`, the signed feeds and the rate-limit answers.

Back to [network overview](../README.md).
