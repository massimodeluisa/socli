# Mastodon

socli reads and writes Mastodon through its REST API, on one server at a time: the server of the account you sign in with, or mastodon.social while you are signed out. Without an account it opens public threads and profiles. With an access token it adds Home, Trending, Bookmarks, mentions, search and every write action. Mastodon is a federation of independent servers, so the account, the post length limit and the post ids all belong to the server you choose. The client lives in `src/net/mastodon/`, and the other pages in this folder describe its parts: [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md) and [testing](testing.md). The overview of all networks is in [../README.md](../README.md).

## Capabilities

The capability set (`caps_with` in `src/net/mastodon/mod.rs`) depends on whether a token is available. A token counts as available when `MASTODON_ACCESS_TOKEN` is set or the `[mastodon]` section of `config.toml` holds an `access_token`.

| Capability | Signed out | Signed in |
| --- | --- | --- |
| Feeds | none | Home, Trending, Bookmarks |
| Merged Home feed | none | Home |
| Mentions (`socli notifications`) | no | yes (mentions only) |
| Search | no | yes |
| Thread | yes | yes |
| Profile (`socli user`) | yes | yes |
| Post and reply | no | yes |
| Quote | no | yes |
| Like | no | yes (named favorite) |
| Repost | no | yes, with undo (Mastodon calls it a boost) |
| Bookmark | no | yes |
| Follow | no | yes |
| Delete | no | yes |
| Counts (replies, reposts, likes, quotes) | yes | yes |
| Views and bookmark counts | no | no |
| Length limit | 500 (graphemes) | the server's limit, 500 by default (graphemes) |

Mastodon calls a like a favorite, so the `l` hint reads `favorite` when a Mastodon post is selected and the status line says `favorited`. A boost shows as a repost, and the booster's handle appears above the post.

Signed out, the client reads from `mastodon.social`, unless `MASTODON_SERVER` or a saved `server` names another host. Asking for a feed, a search or a write returns ``sign in first: `socli login mastodon` ``. Asking for a feed Mastodon does not have (for example `latest`) returns `Mastodon has no <kind> feed in its API`. A thread link from another server needs a session, because socli resolves it to a local id through search: signed out it answers ``sign in to open posts from <host>: `socli login mastodon` ``.

In the merged view, Mastodon supplies Home to Home, Trending to Discover and Bookmarks to Saved. `socli feed discover` opens Trending.

Quoting is native only on servers that run Mastodon 4.5 or newer and only when the quoted author's policy allows it. On an older server socli appends the quoted post's link to your text after a blank line. When the author denies quotes, socli stops with `the author of that post does not allow quotes. share its link instead`.

## Limits

**Length.** The limit belongs to the server. At sign-in socli reads `configuration.statuses.max_characters` from `/api/v2/instance`, stores it as `max_chars` in `[mastodon]`, and uses 500 when the server does not answer. mastodon.social reported 500 on 2026-10-20. socli counts graphemes, counts every link as 23 characters, and counts a mention such as `@theo@fosstodon.org` as `@theo`. The same server answer carries `characters_reserved_per_url` (also 23 on mastodon.social), but socli keeps 23 as a constant and does not read it. A post over the limit fails before any request with `Mastodon allows N characters on <server>; this post has M`. The details are in [mapping](mapping.md#length).

**Rate limits.** The official rate-limit page (read 2026-10-20) states these defaults:

| Scope | Limit |
| --- | --- |
| Any endpoint, per account | 300 per 5 minutes |
| Any endpoint, per IP | 300 per 5 minutes |
| Deleting statuses (`DELETE /api/v1/statuses/:id` or `POST /api/v1/statuses/:id/unreblog`), per account | 30 per 30 minutes |
| Uploading media, per account | 30 per 30 minutes |

The page carries a last-updated date of November 20, 2022, and the numbers are the server's defaults, so another server may set its own. A live request to mastodon.social on 2026-10-20 returned `x-ratelimit-limit: 300` and an ISO 8601 `x-ratelimit-reset`. When a request returns 429, socli turns it into `Mastodon rate limit reached; try again in …`, taking the wait from `x-ratelimit-reset`, then from `Retry-After`, and writing `try again in a minute` when neither is present. The wait reads `Ns` below 120 seconds and `N minutes` (rounded up) from 120 seconds up. socli does not count requests itself. Page sizes are clamped to 1 through 40 posts per request, and requests time out after 20 seconds.

**One account per server.** The `[mastodon]` section holds one account on one server. Signing in again replaces it, and `socli logout mastodon` clears it.

**Reads without an account.** The maintainer checked mastodon.social on 2026-10-20 without a token: `GET /api/v1/timelines/home` answers 401, and `GET /api/v1/timelines/public` answers 422 because that server keeps its live feeds closed to signed-out readers. Trending, hashtag timelines, account lookup, a single status, its context and an account's statuses answer 200, and status search returns no statuses (account search works). socli offers none of the feeds without a token, and reads only threads and profiles.

**Search.** Search needs a token. A query that is one `#tag` goes to the hashtag timeline. Any other query uses status search. The official search page says status search depends on an ElasticSearch backend on the server and on an authenticated request, so a server without that backend returns no posts. Each account also chooses whether its public posts are searchable by anyone (the `source[indexable]` setting of the Account entity), so posts from accounts that keep it off do not show up for other people.

## Status

The client is built and covered by unit tests over synthetic fixtures and by the demo (see [testing](testing.md)). It has not been tested against a live account by the owner yet, so the signed-in calls (sign-in, feeds, writes, rate-limit headers) are checked only by the checks listed in the testing page.

- Sign-in uses the browser path (an app registered on your server, PKCE, a pasted code) or an access token you create yourself. Both are described in [auth](auth.md).
- Posts carry text only. socli does not upload media, attach a poll, set a content warning or choose a visibility for a new post. A reply keeps the visibility of an unlisted, private or direct parent.
- Polls, content warnings and cards display. Polls cannot be voted on.
- Link previews render only in the full-screen reader, not in `--print` output.
- Lists, direct conversations, scheduled posts, edits and filters have no code.

## Official sources

Read on 2026-10-20:

- https://docs.joinmastodon.org/client/token/
- https://docs.joinmastodon.org/methods/apps/
- https://docs.joinmastodon.org/methods/oauth/
- https://docs.joinmastodon.org/methods/statuses/
- https://docs.joinmastodon.org/methods/timelines/
- https://docs.joinmastodon.org/methods/trends/
- https://docs.joinmastodon.org/methods/bookmarks/
- https://docs.joinmastodon.org/methods/notifications/
- https://docs.joinmastodon.org/methods/search/
- https://docs.joinmastodon.org/methods/accounts/
- https://docs.joinmastodon.org/methods/instance/
- https://docs.joinmastodon.org/api/guidelines/ (pagination)
- https://docs.joinmastodon.org/api/rate-limits/
- https://docs.joinmastodon.org/entities/Account/
- https://docs.joinmastodon.org/entities/Status/

Back to [network overview](../README.md).
