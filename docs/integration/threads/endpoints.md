# Threads endpoints

The product currently refuses live Threads with `Waiting for Meta approval` and does not call these endpoints. The tables below are the calls for when that hold is removed.

SoCLI calls the Threads API at `https://graph.threads.com/v1.0`, plus three token endpoints on `https://graph.threads.com`. The code is in `src/net/threads/mod.rs` and `src/net/threads/auth.rs`. The token goes in an `access_token` query or form parameter on every call, and SoCLI keeps the request URL out of every error message so the token never shows up in output.

Back to [Threads](README.md). Related: [auth](auth.md), [mapping](mapping.md), [testing](testing.md).

## Field lists

Each read asks for an explicit list of fields. The three post lists differ in what their endpoint accepts:

| Name | Used by | Fields |
| --- | --- | --- |
| own posts | `GET /me/threads` | `id,media_product_type,media_type,media_url,permalink,owner,username,text,timestamp,shortcode,thumbnail_url,children,is_quote_post,quoted_post,reposted_post,alt_text,link_attachment_url,has_replies,root_post,replied_to,is_reply` |
| public posts | `GET /me/mentions`, `GET /keyword_search`, `GET /{id}` | the own list without `owner` |
| conversation | `GET /{root_id}/conversation` | the public list without `reposted_post`, `alt_text` and `link_attachment_url` |
| profile | `GET /me` | `id,username,name,threads_profile_picture_url,threads_biography,is_verified` |

## Reads

| Method and path | Parameters SoCLI sends | Triggered by | Pagination |
| --- | --- | --- | --- |
| `GET /me` | profile fields | sign-in check, and `socli user` with no handle, `me` or your own handle | none |
| `GET /me/threads` | own post fields, `limit` (1 to 100) | the `Your threads` feed (`socli feed mine --net threads`), merged Home, and your profile screen | `after`, from `paging.cursors.after` |
| `GET /me/mentions` | public post fields, `limit` (1 to 100) | `socli notifications` | `after` |
| `GET /keyword_search` | `q`, `search_type=RECENT`, public post fields, `limit` (1 to 100) | `socli search` | `after` |
| `GET /{id}` | public post fields | the target post of `socli thread`, its root post when that differs, and the fetch of a repost original or quoted post that arrived as a bare id | none |
| `GET /{root_id}/conversation` | conversation fields, `reverse=false`, `limit=100` | the replies of `socli thread`, skipped when the root says `has_replies` is false | `after`, up to five pages |

A cursor is offered as the next page only when the response has `paging.next` or the page came back full.

When a list contains reposts or quotes that arrive as a bare id, SoCLI fetches up to ten of them with `GET /{id}` after the list call. A failed fetch leaves the bare entry in place. `socli thread` returns one flat list with no next page and drops duplicate ids.

## Writes

A post is published in two calls, as Meta's create-post flow requires: a container, then a publish.

| Method and path | Body or parameters | Triggered by |
| --- | --- | --- |
| `POST /me/threads` | form: `media_type=TEXT`, `text`, optionally `reply_to_id` and `quote_post_id` | `socli post`, `socli reply`, `socli quote` (step 1, creates the container) |
| `GET /{creation_id}` | `fields=status,error_message` | the same commands, polled up to five times, two seconds apart, until the container is ready |
| `POST /me/threads_publish` | form: `creation_id` | the same commands (step 2, publishes) |
| `GET /{id}` | `fields=permalink,text,timestamp,username` | the same commands, to read the permalink of the new post |
| `DELETE /{id}` | none | `socli delete` |
| `POST /{id}/repost` | none | `socli repost` |

If the status poll itself fails, SoCLI carries on to publish. A status of `ERROR` or `EXPIRED` stops the post with "Threads could not prepare the post: {reason}. check the text and try again". If the follow-up `GET /{id}` fails, the posted text is used in place of the fetched fields. A delete whose response says `success: false` is reported as "threads did not delete the post. check that it is yours and try again".

The client never calls an endpoint for likes, bookmarks, follows or undoing a repost; those return an unsupported error before any request.

## Token endpoints

| Method and URL | Parameters | Triggered by |
| --- | --- | --- |
| `GET https://graph.threads.com/access_token` | `grant_type=th_exchange_token`, `client_secret`, `access_token` | `socli login threads` with an app secret, and the last step of `socli auth threads oauth` |
| `GET https://graph.threads.com/refresh_access_token` | `grant_type=th_refresh_token`, `access_token` | automatic refresh, and `socli auth threads refresh` |
| `POST https://graph.threads.com/oauth/access_token` | form: `client_id`, `client_secret`, `grant_type=authorization_code`, `redirect_uri`, `code` | `socli auth threads oauth` |
| `https://threads.com/oauth/authorize` (opened in a browser) | `client_id`, `redirect_uri`, `scope`, `response_type=code`, `state` | `socli auth threads oauth` |

## Errors

`map::error_from_body` turns a failed response into one message:

| Response | Result |
| --- | --- |
| HTTP 429 | "Threads rate limit reached; try again in {n}s" ("in {n} minutes" from 120 seconds up), using the `Retry-After` header when there is one, else "in a minute" |
| Meta `error.code` 190 | "sign in first: `socli login threads`" |
| `error.code` 4, 17, 32 or 613 | the same rate-limit message |
| `error.code` 10, or 200 to 299 | "threads: {message} (code {code}). grant the missing permission, then run `socli login threads` again" |
| any other `error.code` | "threads: {message} (code {code})" |
| an OAuth endpoint body with `error_message` | "threads: {text}" |
| anything else | "Threads returned {status}" |

A successful response that is not JSON is reported as a parse error. A connection failure is reported as "could not reach threads: {reason}. check your connection and try again". SoCLI does not retry any call.
