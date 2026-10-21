# Bluesky: endpoints

Every call goes to `{base}/xrpc/{nsid}`. The base is the stored PDS (default `https://bsky.social`) when a session exists, with the access JWT as bearer token, and `https://public.api.bsky.app` when signed out. Requests time out after 20 seconds and send the user agent `socli/<version>`. The code is in `src/net/bluesky/rpc.rs` and `src/net/bluesky/mod.rs`. Related pages: [overview](README.md), [auth](auth.md), [mapping](mapping.md), [testing](testing.md).

## Reads

All reads use GET. `limit` is clamped to 1 through 100, and every paginated call passes the `cursor` from the previous answer.

| NSID | Parameters sent | Triggered by | Pagination |
| --- | --- | --- | --- |
| `app.bsky.feed.getTimeline` | `limit`, `cursor` | `socli feed` on Following; Home tab | `cursor` |
| `app.bsky.feed.getFeed` | `feed` (the Discover generator), `limit`, `cursor` | `socli feed discover`; Discover tab | `cursor` |
| `app.bsky.bookmark.getBookmarks` | `limit`, `cursor` | `socli bookmarks`; Saved tab | `cursor` |
| `app.bsky.notification.listNotifications` | `reasons` (`mention`, `reply`, `quote`, repeated), `limit`, `cursor` | `socli notifications` | `cursor` |
| `app.bsky.feed.getPosts` | `uris` (repeated, 25 per request) | Hydrating notifications; reading viewer state before like and repost | none |
| `app.bsky.feed.getPostThread` | `uri`, `depth=10`, `parentHeight=80` | `socli thread`; opening a post. Works signed out | none |
| `app.bsky.feed.getAuthorFeed` | `actor`, `filter=posts_and_author_threads`, `limit`, `cursor` | `socli user`. Works signed out | `cursor` |
| `app.bsky.feed.searchPosts` | `q`, `sort=latest`, `limit`, `cursor` | `socli search`. Needs a session | `cursor` |
| `app.bsky.actor.getProfile` | `actor` | `socli user`; the follow check | none |
| `com.atproto.identity.resolveHandle` | `handle` | Turning a handle into a DID for links, mentions and deletes | none |

## Writes

All writes use POST with a JSON body and need a session.

| NSID | Body | Triggered by |
| --- | --- | --- |
| `com.atproto.repo.createRecord` | `repo` (own DID), `collection`, `record` | `socli post`, `reply`, `quote` (`app.bsky.feed.post`); `socli like` (`app.bsky.feed.like`); `socli repost` (`app.bsky.feed.repost`); `socli follow` (`app.bsky.graph.follow`) |
| `com.atproto.repo.deleteRecord` | `repo`, `collection`, `rkey` | `socli delete`; turning off a like, repost or follow |
| `app.bsky.bookmark.createBookmark` | strong reference (`uri`, `cid`) | `socli bookmark` |
| `app.bsky.bookmark.deleteBookmark` | `{uri}` | Removing a bookmark |

Like and repost first read the post through `getPosts`: when the viewer record already exists the call deletes it, and when the requested state is already in place the call does nothing. Follow reads `getProfile` for the DID and `viewer.following` before it creates or deletes the record. Delete accepts only posts whose repo is the signed-in DID or handle and whose collection is `app.bsky.feed.post`; anything else fails with `Bluesky can only delete your own posts`.

## Session

| NSID | Method | Body | Triggered by |
| --- | --- | --- | --- |
| `com.atproto.server.createSession` | POST | `{identifier, password}` | `socli login bluesky` |
| `com.atproto.server.refreshSession` | POST | none, refresh JWT as bearer | An expired token on any call |
| `com.atproto.server.deleteSession` | POST | none, refresh JWT as bearer | `socli logout bluesky` |

## Errors handled

| Condition | Result |
| --- | --- |
| 400 or 401 with `error` of `ExpiredToken` or `InvalidToken` | One refresh through `refreshSession`, then one retry. If that fails: `sign in first: socli login bluesky` |
| 401 on `createSession` | `Bluesky rejected the handle or app password.` with the app-password link |
| 429 | `Bluesky rate limit reached; try again in N minutes` (120 seconds or more), `try again in Ns`, or `try again in a minute` without `Retry-After` |
| Other non-2xx | HTTP error carrying the `message` text of the body |
| 2xx with an empty body | Treated as `null` |
| Feed, search or post without a session | `sign in first: socli login bluesky` |
| Id from another network | `<id> belongs to <network>, not Bluesky` (for example `x:p1 belongs to X, not Bluesky`) |

Back to [network overview](../README.md).
