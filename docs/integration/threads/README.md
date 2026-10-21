# Threads

socli reaches Threads through Meta's Threads API (`https://graph.threads.com/v1.0`). The API is narrower than the app: it has no home timeline, no likes, no follows and no view of other people's profiles, so socli on Threads is a place to read your own threads, mentions, conversations and keyword search, and to post, reply, quote, repost and delete. This page lists what works, which limits the code and the official documentation state, what is not built, and where the other four pages for this network live.

Part of the [integration overview](../README.md). The other pages: [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md), [testing](testing.md).

## Capabilities

The Threads client exists only when socli holds an access token, and the code distinguishes no further sign-in state. `threads::caps` (`src/net/threads/mod.rs`) returns one fixed set:

| Capability | Threads (token stored) |
| --- | --- |
| Feeds | `Your threads` (feed kind `mine`) |
| Primary feed (merged Home) | `Your threads` |
| Merged Discover | none |
| Merged Saved | none |
| Mentions | yes |
| Search | yes |
| Thread | yes |
| Profile | your own account only |
| Post and reply | yes |
| Quote | yes (native when the quoted post is on Threads, otherwise your text plus the link) |
| Like | no |
| Repost | yes |
| Undo repost | no |
| Bookmark | no |
| Follow | no |
| Delete | yes |
| Counts (replies, reposts, likes, quotes) | no, socli does not request them |
| Views | no |
| Length limit | 500 characters |

Unsupported actions fail before any request with a message of the form "Threads has no {feature} in its API" (for example "Threads has no likes in its API").

## Limits

The official pages state these limits (checked 2026-10-20) and the code works inside them:

- A token is valid for one hour when it is short-lived and for 60 days when it is long-lived. A long-lived token can be refreshed once it is at least 24 hours old and before it expires, and socli does this on its own when it knows the expiry. A token pasted without the app secret has no known expiry and is never refreshed automatically. See [auth](auth.md).
- Keyword search is capped at 2,200 queries in a rolling 24 hours per user, shared across apps. Queries that return nothing do not count. Until Meta approves the `threads_keyword_search` permission for the app, the search covers only posts owned by the signed-in user, and public posts become searchable after approval. socli sends `search_type=RECENT`, which returns the newest posts first, while Meta's default is `TOP`.
- Until Meta approves `threads_content_publish` for advanced access, an app can post only on behalf of the Threads testers of that app.
- The profile endpoint serves only the app-scoped user, so socli can show your profile but not anyone else's. (The official profile page also describes a separate `GET /profile_lookup` endpoint for public profiles. socli does not call it.)
- A repost is made with `POST /{id}/repost`. The API has no call to undo it, so `unrepost` is unsupported.
- The API gives no way to turn a post link into a post id. A link on `threads.net` or `threads.com` is therefore refused, for example by `socli thread <link>`, with "Threads post links cannot be resolved through the API; use the id from `socli --json`".

Limits checked by socli before it sends a post:

- Empty text is rejected: "a threads post needs text. write something and post again".
- More than 500 characters is rejected: "Threads posts hold 500 characters at most and this one has {used}. shorten it and post again". Emoji count by their UTF-8 length, so the count can exceed the number of characters you see (see [mapping](mapping.md)).
- More than five distinct links is rejected: "Threads allows at most 5 links in a post. Remove {n}".

Rate limits: when Meta answers HTTP 429, or an error code of 4, 17, 32 or 613, socli reports "Threads rate limit reached; try again in {n}s" ("in {n} minutes" from 120 seconds up, and "in a minute" when no wait is given). The client does not retry. Code 190 (an invalid or expired token) is reported as "sign in first: `socli login threads`".

## Status

Working today: your threads feed, mentions, keyword search (`search_type=RECENT`), a post by id, conversations (up to five pages of 100 replies), your profile, posting, replying, native quotes, reposting, deleting, token paste-in, the browser sign-in with a pasted redirect URL, and automatic token refresh.

Not built, or not possible through the API: a home or discover timeline, likes, bookmarks, follows, undoing a repost, other people's profiles, counts and views, and post links as input.

## Official sources

Read on 2026-10-20. Every address below starts with `https://developers.facebook.com/documentation/threads`.

- [Threads API overview](https://developers.facebook.com/documentation/threads)
- [Get access tokens and permissions](https://developers.facebook.com/documentation/threads/get-started/get-access-tokens-and-permissions)
- [Long-lived tokens](https://developers.facebook.com/documentation/threads/get-started/long-lived-tokens)
- [Create posts](https://developers.facebook.com/documentation/threads/create-posts)
- [Keyword search](https://developers.facebook.com/documentation/threads/keyword-search)
- [Mentions](https://developers.facebook.com/documentation/threads/threads-mentions)
- [Profiles](https://developers.facebook.com/documentation/threads/threads-profiles)
- [Retrieve posts](https://developers.facebook.com/documentation/threads/retrieve-and-discover-posts/retrieve-posts)
- [Reposts](https://developers.facebook.com/documentation/threads/posts/reposts)
- [Replies and conversations](https://developers.facebook.com/documentation/threads/retrieve-and-manage-replies/replies-and-conversations)
