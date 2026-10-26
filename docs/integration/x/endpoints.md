# X endpoints

A browser session (`socli login x`, or `X_AUTH_TOKEN` together with `X_CT0`) talks to the same GraphQL the website uses, `https://x.com/i/api/graphql/{queryId}/{Operation}`, from `src/net/x/web.rs`. Query ids are read from the current `main.*.js` and cached in `x-ops.json` for six hours. HomeTimeline, HomeLatestTimeline and Bookmarks are no longer named in that bundle; SoCLI keeps the persisted query ids that still answer. SearchTimeline is a POST. Follow and unfollow are `POST https://x.com/i/api/1.1/friendships/create.json` and `destroy.json`. A quote is a post whose `attachment_url` is `https://x.com/i/status/{id}`.

The tables below are the official API v2 at `https://api.x.com`, used when the session is an OAuth user token or an app bearer (`src/net/x/client.rs`, `src/net/x/auth.rs`). Every official call sends the token as an `Authorization: Bearer` header. Reads use the user token when one is stored and the app bearer otherwise. Writes need the user token. A stored browser session is preferred over both.

Back to [X](README.md). Related: [auth](auth.md), [mapping](mapping.md), [testing](testing.md).

## Fields SoCLI requests

Timeline, search and lookup calls share one set of fields, so a post arrives with its author, media and referenced posts in a single response:

| Parameter | Value |
| --- | --- |
| `tweet.fields` | `created_at,public_metrics,entities,conversation_id,referenced_tweets,attachments,author_id,note_tweet,lang` |
| `expansions` | `author_id,attachments.media_keys,referenced_tweets.id,referenced_tweets.id.author_id` |
| `media.fields` | `url,preview_image_url,type,width,height,alt_text` |
| `user.fields` | `name,username,profile_image_url,verified,verified_type,description` |

The tables name them "the post fields" below. Page sizes are clamped to the range X accepts for each endpoint.

## Reads

| Method and path | Parameters SoCLI sends | Triggered by | Pagination |
| --- | --- | --- | --- |
| `GET /2/users/me` | `user.fields` | sign-in, and any call that needs your user id when it is not stored | none |
| `GET /2/users/by/username/{username}` | `user.fields` | `socli user @handle`, the author screen (`u` in the reader) | none |
| `GET /2/users/{id}/timelines/reverse_chronological` | post fields, `max_results` (1 to 100) | the `Following` feed, merged Home | `pagination_token` from `meta.next_token` |
| `GET /2/users/{id}/tweets` | post fields, `max_results` (5 to 100) | `socli user @handle`, after the lookup above | `pagination_token` |
| `GET /2/users/{id}/bookmarks` | post fields, `max_results` (1 to 100) | the `Bookmarks` feed, `socli bookmarks`, merged Saved | `pagination_token` |
| `GET /2/users/{id}/mentions` | post fields, `max_results` (5 to 100) | `socli notifications` | `pagination_token` |
| `GET /2/tweets/search/recent` | `query`, `max_results` (10 to 100), `sort_order=recency`, post fields | `socli search`, the `For you` and `Latest` feeds (query from `latest_query`), and the replies of a thread (`conversation_id:{id}`) | `next_token` from `meta.next_token` |
| `GET /2/tweets/{id}` | post fields | the root post of `socli thread` | none |

`{id}` is your own user id for the timeline, bookmarks and mentions calls, and the profile's user id for `/tweets`.

`socli thread` issues the root lookup and the conversation search together. When only one of them succeeds SoCLI shows what it has: the root alone if the search fails, the replies alone if the root fails and the search returned posts. It fails only when both fail, or when the root fails and the search is empty.

## Writes

| Method and path | Body | Triggered by |
| --- | --- | --- |
| `POST /2/tweets` | `{"text": ...}`, plus `{"reply": {"in_reply_to_tweet_id": ...}}` for a reply | `socli post`, `socli reply`, and `socli quote` (a quote is a post whose text ends with the quoted link) |
| `DELETE /2/tweets/{id}` | none | `socli delete` |
| `POST /2/users/{id}/likes` and `DELETE /2/users/{id}/likes/{tweet_id}` | `{"tweet_id": ...}` for the POST | `socli like`, `socli unlike` |
| `POST /2/users/{id}/retweets` and `DELETE /2/users/{id}/retweets/{tweet_id}` | `{"tweet_id": ...}` for the POST | `socli repost`, `socli unrepost` |
| `POST /2/users/{id}/bookmarks` and `DELETE /2/users/{id}/bookmarks/{tweet_id}` | `{"tweet_id": ...}` for the POST | `socli bookmark`, `socli unbookmark` |
| `POST /2/users/{id}/following` and `DELETE /2/users/{id}/following/{target_user_id}` | `{"target_user_id": ...}` for the POST | `socli follow`, `socli unfollow` |

SoCLI never sends `quote_tweet_id`. The create-post page says quote posting needs an Enterprise plan (checked 2026-10-20). After a post succeeds SoCLI builds the permalink `https://x.com/{username}/status/{id}` from your stored username (`i` when none is stored).

## Authentication endpoints

| Method and URL | Parameters | Triggered by |
| --- | --- | --- |
| `GET https://x.com/i/oauth2/authorize` (opened in the browser) | `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state`, `code_challenge`, `code_challenge_method=S256` | `socli login x --oauth` |
| `POST https://api.x.com/2/oauth2/token` | form: `grant_type=authorization_code`, `code`, `redirect_uri`, `code_verifier`, `client_id`; basic authentication when a client secret exists | `socli login x --oauth` |
| `POST https://api.x.com/2/oauth2/token` | form: `grant_type=refresh_token`, `refresh_token`, `client_id`; basic authentication when a client secret exists | automatic, once, after a 401 |

## Errors

Every failed call becomes an error whose first line is the best message found in the response: the `message` or `detail` of the `errors` array, else `detail`, else `title`, else "X API returned {status}" followed by the body cut to 280 characters. SoCLI then appends one hint line for these cases:

| Status | Hint |
| --- | --- |
| 401 | The token was rejected. Run `socli login x`, or check X_ACCESS_TOKEN / X_BEARER_TOKEN. |
| 402 | The developer account is out of credits. Reads are billed per post returned. |
| 403 | X refused this call. Check the app's scopes and that the token belongs to this user. |
| any failure on a like or follow | Like and follow writes were pulled from self-serve tiers (announced for 20 Apr 2026) and need Enterprise. Repost, bookmark, post, and reply are still on pay-per-use. |
| 403 on a reply | Self-serve replies are often rejected unless that author has @mentioned or quoted you. |
| 429 | "X rate limit reached; try again in 15 minutes", "in 45s" below two minutes, or "in a minute" when no header gives a wait |

SoCLI does not retry on 429. It reads the wait from `x-rate-limit-reset` (a Unix time) and falls back to `retry-after`; the other `x-rate-limit-*` headers (`x-rate-limit-limit`, `x-rate-limit-remaining`) are not read.
