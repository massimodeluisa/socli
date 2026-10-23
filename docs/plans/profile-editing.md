# Plan: your profile and profile editing

The reader used to have a Profile section, reached with `[` and `]`, with `:profile`, `:menu` and `:whoami`, or by clicking the account name in the top bar. It showed a card with "You", the handle of the account in scope and the words "signed in", and nothing else. That section is gone. Other people's profiles are unchanged: `u` opens the profile of the selected post's author, and `:user @handle` opens any account's profile, which on one of your own posts or with your own handle already shows your profile. `socli whoami` still lists the accounts the process will use.

This plan covers two things that replace the old card: a real view of your own profile, and editing it on the networks whose APIs allow that. The API facts were read on 2026-10-21 from each network's documentation and are cited next to each one.

## Your own profile

Your profile comes back as the ordinary profile screen, the one `u` opens, filled with the signed-in account. `:user` with no handle opens it for the network in scope, and in a merged scope it first asks which network, with the same picker `A` uses. Clicking the account name in the top bar does the same. Every network already implements the profile read that this needs (`fn user` in each `src/net/<network>/mod.rs`), Threads included, since its API serves only your own profile. The view therefore needs no new API call, only the entry points and a way to resolve "my account on this network" to a handle.

## What each network lets you edit

| Network | Editable | API | Sign-in change |
| --- | --- | --- | --- |
| Bluesky | display name, description, pronouns, website, avatar, banner | the `app.bsky.actor.profile` record | none |
| Mastodon | display name, bio, avatar, header, their descriptions, profile fields, account flags | `PATCH /api/v1/accounts/update_credentials` | none |
| dev/null | a resync from GitHub | `PUT v1/me/github` | none |
| X | nothing | none in API v2 | not applicable |
| Threads | nothing | none | not applicable |

### Bluesky

A Bluesky profile is a record in the account's own repository: collection `app.bsky.actor.profile`, record key `self`. Its lexicon in `@atproto/api` gives these fields and limits:

| Field | Limit |
| --- | --- |
| `displayName` | 64 graphemes, 640 bytes |
| `description` | 256 graphemes, 2560 bytes |
| `pronouns` | 20 graphemes, 200 bytes |
| `website` | a URI |
| `avatar`, `banner` | PNG or JPEG, at most 1,000,000 bytes |
| `pinnedPost` | a reference to one of your posts |

The record also holds fields socli does not edit, such as `labels`, `links`, `joinedViaStarterPack` and `createdAt`, so an edit reads the whole record with `com.atproto.repo.getRecord`, changes only the edited fields and writes it back with `com.atproto.repo.putRecord`, passing the CID it read as `swapRecord`. If the profile changed elsewhere in between, the write fails instead of overwriting that change. A new avatar or banner goes up first with `com.atproto.repo.uploadBlob`, and the record write follows straight away, because the server deletes a blob that no record references within a short time. socli already writes records with the app password it holds (`createRecord` for posts, likes and reposts, see `docs/integration/bluesky/endpoints.md`), so editing needs no new sign-in.

### Mastodon

Mastodon edits the profile with `PATCH /api/v1/accounts/update_credentials`, a multipart form that needs `write:accounts` ([accounts](https://docs.joinmastodon.org/methods/accounts/#update_credentials)). socli signs in with `read write`, which covers it. The form takes `display_name`, `note` (the bio), `avatar` and `header` images, `avatar_description` and `header_description` (from 4.6.1), the flags `locked`, `bot`, `discoverable`, `hide_collections` and `indexable`, profile fields as `fields_attributes` with a `name` and a `value` each, and posting defaults under `source[...]`. By default a server allows 4 profile fields of 255 characters per name and per value.

The documentation asks clients to read the plain text first from the `source` of `GET /api/v1/accounts/verify_credentials`, let the user edit that, and send it back, because the server turns it into HTML. socli already calls `verify_credentials` at sign-in. From Mastodon 4.6.0 the instance configuration reports the account limits (`max_display_name_length`, `max_note_length`, `max_profile_fields`, `profile_field_name_limit`, `profile_field_value_limit`), and socli counts against those. On an older server it uses the documented defaults for fields and leaves the other limits to the server's own refusal.

### dev/null

A dev/null profile is mirrored from GitHub. The API routes file defines no route that sets a name, bio or avatar, and its one profile route, `PUT v1/me/github`, carries the comment that the profile is "mirrored from GitHub again, as Sync from GitHub does on the web". socli can offer that resync as a single action. What the route changes has to be confirmed against a test account before it ships.

### X and Threads

The Users section of the [X API v2 reference](https://docs.x.com/x-api/llms.txt) covers lookups, search, timelines, follows, mutes, blocks, bookmarks, likes, reposts and lists, and has no endpoint that changes the signed-in account's name, bio, picture or banner. The [Threads profile endpoints](https://developers.facebook.com/documentation/threads/reference), `GET /me` and `GET /profile_lookup`, only read. socli edits neither, and the edit key tells you so in one line on those networks.

## In the reader

In your own profile, `e` opens the edit form. `e` has no binding in the reader today. The form shows one field per row with the value read from the network. Text fields open in the composer's editor, whose counter uses the network's unit, so graphemes on Bluesky and characters on Mastodon. An image field takes a local path, and socli checks the file type and size before uploading anything.

Saving shows the fields that changed, each with its old and new value, and sends only after you confirm. A Bluesky swap failure reads that the profile changed elsewhere and asks you to reload it and edit again. In the demo an edit changes the screen only, and the status row ends in `demo, not saved`, as it does for every other change there.

The form's anatomy goes into `docs/DESIGN.md` and a mockup into `design/` before any code.

## On the command line

`socli profile` prints your profile on the network `--net` names, or on each signed-in network without it, and `--json` prints each profile as one JSON object. `socli profile set` changes it:

```sh
socli profile set --name "Ada" --bio "Writes compilers." --net bluesky
socli profile set --avatar ./me.png --net mastodon
socli profile set --field "Site=https://example.com" --net mastodon
socli profile sync --net devnull
```

`--dry-run` prints the request without sending it, a flag a network does not support is refused before any request, and writes follow the same agent-mode rules as `socli post` (see `docs/agents.md`).

## Before it ships

1. Your own profile opens from `:user`, from the top bar and from your own posts, on every signed-in network, Threads included.
2. On Bluesky and Mastodon, an edit of each field round-trips against a real account, leaves every field it did not touch unchanged, and shows the server's refusal as a readable message.
3. On Bluesky, an edit made in another client between the read and the write fails with the swap message and overwrites nothing.
4. The dev/null resync is either confirmed against a test account or left out.
5. README, `docs/agents.md`, `docs/DESIGN.md`, the integration pages and the website's Networks page describe only what shipped.

## Questions for later

- Mastodon profile fields can be reordered and some servers verify links in them. A first version could only add, change and remove fields.
- Whether `pinnedPost` on Bluesky belongs in the form or on a post's own actions.
