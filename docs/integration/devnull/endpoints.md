# dev/null: endpoints

The origin is `https://dev-null.sh`. socli calls the v1 API under `/api/v1/` and two CLI endpoints under `/api/cli/`. Requests time out after 20 seconds and send the user agent `socli/<version>`. The code is in `src/net/devnull/api.rs`, `sign.rs`, `ssh.rs`, `auth.rs` and `mod.rs`. Related pages: [overview](README.md), [auth](auth.md), [mapping](mapping.md), [testing](testing.md).

## Conventions

JSON requests send `Accept: application/json`. With an approved device, each v1 request carries the `X-Devnull-Timestamp`, `X-Devnull-Nonce` and `X-SSH-Signature` headers described in [auth](auth.md); without one, v1 reads go out unsigned and only public data comes back. List answers have the shape `{data, meta.next_cursor}` and single answers `{data}`. Path segments are percent-encoded.

Pagination passes `meta.next_cursor` back as the `cursor` query parameter.

## v1 reads

| Method and path | Parameters | Triggered by | Needs |
| --- | --- | --- | --- |
| `GET log/following` | `cursor` | `socli feed`; Home tab | device |
| `GET log/latest` | `cursor` | `socli feed latest` | device |
| `GET log/trending` | `cursor` | `socli feed trending`; Discover tab | device |
| `GET starred` | `cursor` (20 per page, ordered by when you starred) | `socli feed saved`, `socli bookmarks`, the Starred tab; the merged Saved tab | device |
| `GET notifications` | `type=replies,mentions,quotes`, `cursor` | `socli notifications` (marks them read) | device |
| `GET commits/{id}` | none | `socli thread`; opening a post | public. A device answer includes `ancestors`. |
| `GET commits/{id}/replies` | none | thread replies | device |
| `GET users/{username}` | none | `socli user` | public |
| `GET users/{username}/commits` | `cursor` | `socli user` | public |
| `GET me` | none | device login, when the approval answer carries no username | device |

Without a device, a thread is built by fetching each parent in turn with `GET commits/{id}`, up to 20 ancestors, and stopping at the first HTTP error.

## v1 writes

| Method and path | Body | Triggered by |
| --- | --- | --- |
| `POST commits` | `{body, parent_id?, quoted_commit_id?}` | `socli post`, `reply`, `quote` |
| `DELETE commits/{id}` | none | `socli delete` |
| `POST commits/{id}/star` | none | `socli like` |
| `DELETE commits/{id}/star` | none | turning off a star |
| `POST users/{username}/follow` | none | `socli follow` |
| `DELETE users/{username}/follow` | none | turning off a follow |
| `POST devices` | `{code, code_verifier}` | `socli login devnull` |

`POST devices` is signed with the device key that is being approved.

## CLI endpoints (SSH key)

| Method and path | Body | Triggered by |
| --- | --- | --- |
| `POST /api/cli/key-lookups` | plain text, one public key per line | `socli auth devnull ssh` |
| `POST /api/cli/commits` | `text/plain; charset=utf-8` payload `timestamp\nnonce\nreply_to\nmessage`, header `X-SSH-Signature` | `socli post` and `reply` with an SSH key |

The commit answer is parsed for a link of the form `https://dev-null.sh/@<user>/commits/<id>`. A link on any other host is refused.

## Errors handled

| Condition | Result |
| --- | --- |
| Body `code` is `device_not_approved` | `sign in first: socli login devnull` |
| Body `code` is `rate_limited`, or HTTP 429 | `dev/null rate limit reached; try again in N minutes` (120 seconds or more, rounded up), `try again in Ns` (less), or `try again in a minute` (no `Retry-After`) |
| Body `code` is `invalid_signature` | The server `message`, for example `Signature expired. Check that your clock is right.` |
| Other error with a `message` field | That message |
| Other error with a plain-text body (not starting with `<`) | The text |
| Anything else | `dev/null returned <status>` |
| Action outside the stored access | `sign in first: socli login devnull` (quotes with only an SSH key end here) |
| Repost or bookmark | `dev/null has no reposts in its API` or `dev/null has no bookmarks in its API`, exit status 1, no request sent |
| `ssh-keygen` missing | `ssh-keygen is required for dev/null posting with your GitHub key` |

## Upstream routes

The routes file of the upstream repository (https://raw.githubusercontent.com/nunomaduro/dev-null.sh/production/routes/api.php, read 2026-10-20) defines `GET starred`, `POST` and `DELETE commits/{commit}/star`, `POST commits`, `DELETE commits/{commit}` and `GET commits/{commit}/replies`, and no `saved` or `bookmark` route. Upstream removed those on 2026-10-20 (commit 230c004a). socli no longer calls them.

Back to [network overview](../README.md).
