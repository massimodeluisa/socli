# X

`socli login x` reads and writes X through the website's GraphQL, using the browser session already signed in on this machine. `socli login x --oauth` uses the X API v2 (`https://api.x.com`) instead: OAuth 2.0, a stored access token and refresh token, and a refresh when that API answers 401. This page lists what socli can do on X in each sign-in state, which limits the code handles, what is not built, and where the other four pages for this network live.

Part of the [integration overview](../README.md). The other pages: [auth](auth.md), [endpoints](endpoints.md), [mapping](mapping.md), [testing](testing.md).

## Capabilities

The client exists when socli holds a browser session, a user token, or an app bearer token. The code distinguishes a signed-in account from a bearer in `x::caps` (`src/net/x/mod.rs`). With none of the three, the network is not loaded at all and `socli` tells you to run `socli login x`.

| Capability | Browser session (`socli login x`) | User token (`--oauth`) | App bearer token only |
| --- | --- | --- | --- |
| Feeds | For you, Following, Bookmarks | For you, Following, Bookmarks | none |
| Primary feed (merged Home) | Following | Following | none |
| Mentions | yes | yes | no |
| Search | yes | yes | yes |
| Thread | yes | yes | yes |
| Profile | yes | yes | yes |
| Post and reply | yes | yes | no |
| Quote | yes, as a quote | text plus the post link | no |
| Like and unlike | yes | yes (see limits) | no |
| Repost and undo | yes | yes | no |
| Bookmark and remove | yes | yes | no |
| Follow and unfollow | yes | yes (see limits) | no |
| Delete | yes | yes | no |
| Counts (replies, reposts, likes, quotes, bookmarks) | yes | yes | yes |
| Views | yes | yes | yes |
| Length limit | 280 | 280 | 280 |

For you and Following on a browser session are the website's Home and Latest timelines. On an official token, For you is not X's ranked feed, because that API does not give apps the ranked feed. socli runs a recent public search instead, using the `latest_query` key in the `[x]` section (default `lang:en -is:retweet`). The same query is the Latest tab on both sessions. Trending is not built: it returns "X has no trending feed in socli yet. Try `socli search <words> --net x`".

## Limits

Reads are billed per post returned. The reader asks each signed-in network for at least 20 posts and prefetches one next page when the selection reaches the 10th post of the current page. It does not walk the whole timeline. A shell read asks for `--count` posts (15 by default) and does not fetch another page on its own. The split view shows only the replies already loaded. The X pricing page lists reads per resource, writes per request, and says prices can change (checked 2026-10-20). When the account is out of credits, X answers 402 and socli shows "The developer account is out of credits. Reads are billed per post returned."

Rate limits are per endpoint and per 15-minute window. The official table gives, for example, 300 requests per 15 minutes per user for recent search, 180 for the reverse-chronological timeline and for bookmarks, 100 per 15 minutes per user for `POST /2/tweets`, and 50 per 15 minutes for deleting a post (checked 2026-10-20). socli does not track these windows. When X answers 429, socli reads the `x-rate-limit-reset` header (a Unix time, documented on the official rate-limit page) and falls back to `retry-after`. The error reads "X rate limit reached; try again in 15 minutes" (minutes from 120 seconds up, rounded up; seconds below that, as in "try again in 45s"), and "try again in a minute" when neither header is present.

Other limits that surface as errors:

- Quoting through the official API (`quote_tweet_id`) needs an Enterprise plan according to the create-post page, so that path never sends it. A quote on an official token is your text followed by the link of the quoted post. If the quoted post's author is unknown, socli stops before any request with "can't quote this post on X: its author is unknown. Open it and press s to copy the link". A browser session sends the quote as `attachment_url`.
- Like and follow writes on the official API were pulled from self-serve tiers (announced for 20 Apr 2026) and need Enterprise, so a 403 on those calls carries that explanation. A browser session uses the website's like and follow calls instead.
- Replies from self-serve apps are often rejected unless the author has mentioned or quoted you; the 403 on a reply says so.
- Sign-in needs a browser once. Agent mode (`SOCLI_AGENT=1`) refuses `socli login x` and `socli auth x login` up front with exit code 2; pasting tokens with `socli auth x token` or `socli auth x bearer` works without one. See [auth](auth.md).

## Status

Working today, on a browser session: For you, Following, Latest, Bookmarks, mentions, search, profiles, threads and counts; posting, replying, quoting, deleting, liking, reposting, bookmarking and following. On an official token: the same reads, with For you as search; posting, replying, quoting by link, deleting, reposting and bookmarking; likes and follows subject to the Enterprise limit above; token refresh on 401; the app bearer for public reads.

Not built: a trending feed, native quotes on the official API, paging beyond what you ask for, and any use of the filtered stream or the full-archive search.

## Official sources

Read on 2026-10-20.

- [X API introduction](https://docs.x.com/x-api/introduction)
- [OAuth 2.0 authorization code flow with PKCE](https://docs.x.com/fundamentals/authentication/oauth-2-0/authorization-code)
- [OAuth 2.0 user access token](https://docs.x.com/fundamentals/authentication/oauth-2-0/user-access-token)
- [Create post](https://docs.x.com/x-api/posts/create-post)
- [Data dictionary](https://docs.x.com/x-api/fundamentals/data-dictionary)
- [Rate limits](https://docs.x.com/x-api/fundamentals/rate-limits)
- [Pricing](https://docs.x.com/x-api/getting-started/pricing)
