# Mastodon: endpoints

Every call goes to `https://<server>` plus the path in the tables below. The server is the stored one (`MASTODON_SERVER` wins when set) and `mastodon.social` when nothing names another. When a token exists it is sent as a bearer token, and a signed-out client sends none. Requests time out after 20 seconds and send the user agent `socli/<version>`. The client code is in `src/net/mastodon/mod.rs` and the sign-in calls are in `src/net/mastodon/auth.rs`. Related pages: [overview](README.md), [auth](auth.md), [mapping](mapping.md), [testing](testing.md).

## Reads

All reads use GET. `limit` is clamped to 1 through 40. Every paginated call adds the cursor from the previous answer (see [pagination](#pagination)). Calls marked "token" are refused by SoCLI before any request when no token exists (``sign in first: `socli login mastodon` ``).

| Path | Parameters sent | Triggered by | Pagination |
| --- | --- | --- | --- |
| `/api/v1/timelines/home` | `limit`, cursor | `socli feed` on Home; Home tab. Token | `max_id` |
| `/api/v1/trends/statuses` | `limit`, cursor | `socli feed trending` or `discover`; Discover tab. Token | `offset` |
| `/api/v1/bookmarks` | `limit`, cursor | `socli bookmarks`; Saved tab. Token. Every post is marked bookmarked | `max_id` |
| `/api/v1/notifications` | `types[]=mention`, `limit`, cursor | `socli notifications`. Token | `max_id` |
| `/api/v1/statuses/{id}` | none | `socli thread` and opening a post (the focused post); reading the parent of a reply and the post of a quote before sending. Works signed out | none |
| `/api/v1/statuses/{id}/context` | none | `socli thread` and opening a post (ancestors and replies). Works signed out | none |
| `/api/v1/accounts/lookup` | `acct` | `socli user`; `socli follow` and `unfollow`. Works signed out | none |
| `/api/v1/accounts/{id}/statuses` | `exclude_replies=true`, `limit`, cursor | `socli user`. Works signed out | `max_id` |
| `/api/v1/timelines/tag/{word}` | `limit`, cursor | `socli search "#tag"` when the query is one `#word`. Token | `max_id` |
| `/api/v2/search` | `q`, `type=statuses`, `limit`, `offset` | `socli search <words>` for any other query. Token | `offset`, only when a full page came back |
| `/api/v2/search` | `q` (a post link), `type=statuses`, `resolve=true`, `limit=1` | Turning a post link from another server into a local id. Token | none |
| `/api/v2/search` | `q=@handle`, `type=accounts`, `resolve=true`, `limit=1` | Looking up an account that `/api/v1/accounts/lookup` answered 404 for, when signed in | none |
| `/api/v1/accounts/verify_credentials` | none | `socli login` (verification) and the provider's `me` call. Token | none |
| `/api/v2/instance` | none, no bearer token | `socli login`, to read `configuration.statuses.max_characters` | none |

Reading the focused post and its context are two requests. A link whose host is the client's own server and whose last path segment is digits is used as the id directly, without a search.

## Writes

All writes need a token. The write calls use POST, except delete.

| Method and path | Body | Triggered by |
| --- | --- | --- |
| `POST /api/v1/statuses` | JSON: `status`, and when present `in_reply_to_id`, `visibility`, `quoted_status_id` | `socli post`, `reply`, `quote` |
| `DELETE /api/v1/statuses/{id}` | none | `socli delete` |
| `POST /api/v1/statuses/{id}/favourite` and `/unfavourite` | none | `socli like` and `unlike` |
| `POST /api/v1/statuses/{id}/reblog` and `/unreblog` | none | `socli repost` and `unrepost` |
| `POST /api/v1/statuses/{id}/bookmark` and `/unbookmark` | none | `socli bookmark` and `unbookmark` |
| `POST /api/v1/accounts/{id}/follow` and `/unfollow` | none | `socli follow` and `unfollow` |

`POST /api/v1/statuses` sends an `Idempotency-Key` header with a random string, so the server can drop a duplicate of the same submission (the official statuses page says it keeps those keys for up to one hour). A reply first reads the parent through `GET /api/v1/statuses/{id}` to take its `id`, its `visibility` and its author. A quote reads the quoted post the same way, to find out whether the server supports quotes and whether the author allows them. How those answers change the body is in [mapping](mapping.md#replies-and-quotes).

Favorite, boost and bookmark send the requested state without reading the post first.

## Sign-in

| Method and path | Body | Triggered by |
| --- | --- | --- |
| `POST /api/v1/apps` | form: `client_name=SoCLI`, `redirect_uris=urn:ietf:wg:oauth:2.0:oob`, `scopes=read write` | The browser path, when the saved section holds no app for that server |
| `GET /oauth/authorize` | query: `response_type=code`, `client_id`, `redirect_uri`, `scope`, `code_challenge`, `code_challenge_method=S256` | Opened in the browser by the browser path. SoCLI does not request it |
| `POST /oauth/token` | form: `grant_type=authorization_code`, `code`, `client_id`, `client_secret`, `redirect_uri`, `code_verifier`, `scope` | The browser path, after you paste the code |
| `POST /oauth/revoke` | form: `client_id`, `client_secret`, `token` | `socli logout mastodon`, when the section holds a server, token and app |

## Pagination

Mastodon lists carry a `Link` header with `rel="next"` and `rel="prev"` URLs, as the official pagination guide describes (read 2026-10-20). SoCLI reads only the `next` URL and keeps one query pair from it as the cursor: `max_id=<id>` for timelines, bookmarks, notifications and account statuses, and `offset=<n>` for trends. The next request adds that pair. A bare number is read as a `max_id`, and any other key is ignored. Search does not use the header. It sends `offset` itself and offers a next page only when the answer filled the page.

## Errors handled

| Condition | Result |
| --- | --- |
| 401 while a token is saved | `<server> no longer accepts the saved token. run `socli login mastodon` again` |
| 429 | `Mastodon rate limit reached; try again in N minutes` (120 seconds or more), `try again in Ns`, or `try again in a minute`. The wait is the time to `x-ratelimit-reset` (an ISO 8601 time, at least 1 second), else `Retry-After` in seconds |
| Other non-2xx | HTTP error carrying the message of the body: the `errors` list, or the first of `message`, `error_description`, `detail`, `title`, `error`; `Mastodon returned <status>` when the body has none |
| 404 on `accounts/lookup`, signed out | `Mastodon has no @<handle>` |
| 404 on `accounts/lookup`, signed in | A second try through account search, then `Mastodon has no @<handle>` |
| 2xx with an empty body | Treated as `null` |
| 2xx with a body that is not JSON | `could not read the Mastodon response: <detail>` |
| Network failure | `could not reach <server> (<detail>). check your connection and try again` |
| Post link not found by search | `Mastodon could not find <link>` |
| Post link from another server, signed out | ``sign in to open posts from <host>: `socli login mastodon` `` |
| Text that is neither a status id nor a link | `<id> is not a Mastodon post id or link` |
| Id from another network | `<id> belongs to <network>, not Mastodon` (for example `x:1846 belongs to X, not Mastodon`) |
| Feed kind Mastodon lacks | `Mastodon has no <kind> feed in its API` |
| Feed, search, notifications or write without a token | ``sign in first: `socli login mastodon` `` |

The sign-in calls report their own errors: `<server> did not register socli: <message>`, `<server> did not accept the code: <message>. run `socli login mastodon` again`, and `<server> did not accept the token: <message>`.

Back to [network overview](../README.md).
