# dev/null

dev/null (https://dev-null.sh) is a social network for developers whose posts are called commits. socli reads it in three ways, depending on what is stored in the `[devnull]` section of `config.toml`: public pages with no credentials, the signed v1 API of an approved device, and commits signed with a GitHub SSH key. The client lives in `src/net/devnull/`. The other pages in this folder: [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md) and [testing](testing.md). The overview of all networks is in [../README.md](../README.md).

## Capabilities

The code distinguishes three access states (`Access` in `src/net/devnull/mod.rs`). A device that is approved wins over an SSH key. A device key that is stored but not yet approved counts as public unless an SSH key is also stored.

| Capability | Public | SSH key | Approved device |
| --- | --- | --- | --- |
| Feeds | none | none | Following, Latest, Trending, Starred |
| Merged Home feed | none | none | Following |
| Mentions (`socli notifications`) | no | no | yes (replies, mentions, quotes) |
| Search | no | no | no |
| Thread | yes | yes | yes (replies need the device) |
| Profile (`socli user`) | yes | yes | yes |
| Post and reply | no | yes | yes |
| Quote | no | no | yes |
| Like (called star) | no | no | yes |
| Repost | no | no | no |
| Bookmark | no | no | no |
| Follow | no | no | yes |
| Delete | no | no | yes |
| Counts (replies, stars) | yes | yes | yes |
| Views | no | no | no |
| Length limit | 4000 | 4000 | 4000 (counted in characters) |

Discover is an alias of Trending on dev/null, so the merged Discover tab shows Trending. In the merged view the Saved tab shows your starred commits from dev/null. Reposts, bookmarks and search are not in the dev/null API. socli answers `dev/null has no reposts in its API` and `dev/null has no bookmarks in its API` (exit status 1, without calling the network), and reports search as unsupported. An action outside the stored access asks for a sign-in: `sign in first: socli login devnull`.

## Limits

dev/null answers an exhausted rate limit with the code `rate_limited` or HTTP 429, and socli turns either into `dev/null rate limit reached; try again in N minutes` (from 120 seconds up, rounded up), `try again in Ns` below that, or `try again in a minute` when the answer has no `Retry-After` header. The signed API rejects a request whose timestamp is too far from the server clock (`Signature expired. Check that your clock is right.`), so the machine clock has to be correct. Posts are limited to 4000 characters. The terms page (read 2026-10-20) states no request quota, so this page gives none.

The v1 feed endpoints return 20 commits per page, and socli ignores its own `--count` for dev/null.

## Status

- Public reads, device sign-in, SSH-key posting, the three log feeds, notifications, threads, profiles, stars, follows and deletes have code and unit tests. The live paths need an account (see [testing](testing.md)).
- The sign-in page of dev/null only redirects to the mobile app's `devnull://` scheme, so socli asks you to paste the callback address ([auth](auth.md)).
- Listing notifications marks them as read on the server.
- A short commit hash is not resolved. Use the full id or the link.
- dev/null has no bookmarks. On 2026-10-20 the upstream project removed `GET /api/v1/saved`, `PUT` and `DELETE /api/v1/commits/{id}/bookmark` and the `is_saved` commit field (upstream commit 230c004a, "feat: consolidate saved and bookmarks into one (#811)"). The private list is now `GET /api/v1/starred`, which socli reads as the fourth device tab, Starred. The upstream routes file (production branch, read 2026-10-20) lists `starred` and no `saved` or `bookmark` route.
- Quoting needs the approved device. With only an SSH key, `socli quote` fails with `sign in first: socli login devnull`, because the SSH key covers posts and replies only.
- Link previews render only in the full-screen reader, not in `--print` output.

## Official sources

Read on 2026-10-20:

- https://dev-null.sh
- https://dev-null.sh/terms
- https://github.com/nunomaduro/dev-null.sh (the API routes and the device approval flow)

Back to [network overview](../README.md).
