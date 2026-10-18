# Bluesky

socli reads and writes Bluesky through the AT Protocol HTTP API (XRPC). Without an account it reads public posts, threads and profiles from the public AppView. With an app password it adds the Following timeline, Discover, bookmarks, mentions, search and every write action. The client lives in `src/net/bluesky/`, and the other pages in this folder describe its parts: [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md) and [testing](testing.md). The overview of all networks is in [../README.md](../README.md).

## Capabilities

The capability set (`Caps` in `src/net/bluesky/mod.rs`) depends on whether a session is stored. A session counts as stored when `handle`, `did` and a non-empty `access_jwt` are all present in the `[bluesky]` section of `config.toml`.

| Capability | Signed out | Signed in (app password) |
| --- | --- | --- |
| Feeds | none | Following, Discover, Bookmarks |
| Merged Home feed | none | Following |
| Mentions (`socli notifications`) | no | yes (mentions, replies, quotes) |
| Search | no | yes |
| Thread | yes | yes |
| Profile (`socli user`) | yes | yes |
| Post and reply | no | yes |
| Quote | no | yes |
| Like | no | yes |
| Repost | no | yes |
| Bookmark | no | yes |
| Follow | no | yes |
| Delete | no | yes (own posts only) |
| Counts (replies, reposts, likes, quotes, bookmarks) | yes | yes |
| Views | no | no |
| Length limit | 300 (graphemes) | 300 (graphemes) |

Signed out, the client talks to `https://public.api.bsky.app`. Asking it for a feed, a search or a post returns `sign in first: socli login bluesky`. Asking for a feed Bluesky does not have (for example Trending) returns `Bluesky has no <kind> feed in its API`.

In the merged view, Bluesky supplies Following to Home, Discover to Discover and Bookmarks to Saved. Discover is the `whats-hot` generator `at://did:plc:z72i7hdynmk6r22z27h6tvur/app.bsky.feed.generator/whats-hot`.

## Limits

The code handles one limit itself: an HTTP 429 answer becomes `Bluesky rate limit reached; try again in N minutes` (from 120 seconds up, rounded up) or `try again in Ns` below that, using the `Retry-After` header, and `try again in a minute` when the header is absent. Page sizes are clamped to 1 through 100 posts per request. Requests time out after 20 seconds.

The official rate-limit page (read 2026-10-20) states these limits for the hosted PDS:

| Scope | Limit |
| --- | --- |
| All requests, per IP | 3000 per 5 minutes |
| `com.atproto.server.createSession`, per account | 30 per 5 minutes and 300 per day |
| Record writes, per account | 5,000 points per hour and 35,000 per day (create 3, update 2, delete 1) |

The page says the public AppView endpoints have generous limits, and that every number may change. socli does not count requests itself.

## Status

Every row of the capability table has code behind it and unit tests over recorded responses. The calls against the live service need an account, so they are checked by hand (see [testing](testing.md)).

- Sign-in uses an app password. OAuth needs a client metadata file hosted on a domain, so it is not built yet.
- Search is blocked for signed-out clients by the AppView, so socli asks for a sign-in.
- Link previews render only in the full-screen reader, not in `--print` output.
- Direct messages, lists, feeds other than Following and Discover, and image uploads have no code. Posts carry text only.

## Official sources

Read on 2026-10-20:

- https://docs.bsky.app/docs/get-started
- https://docs.bsky.app/docs/advanced-guides/api-directory
- https://docs.bsky.app/docs/advanced-guides/rate-limits (redirects to the Bluesky Protocol Services site)
- https://docs.bsky.app/docs/advanced-guides/posts (same redirect)
- https://docs.bsky.app/docs/api/com-atproto-server-create-session
- https://docs.bsky.app/docs/api/app-bsky-feed-get-timeline
- https://docs.bsky.app/docs/advanced-guides/oauth-client
- https://atproto.com/specs/oauth

Back to [network overview](../README.md).
