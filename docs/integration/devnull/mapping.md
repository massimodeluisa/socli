# dev/null: mapping

SoCLI turns each dev/null commit into the network-neutral types in `src/model.rs`. The mapper is `src/net/devnull/map.rs`. Related pages: [overview](README.md), [auth](auth.md), [endpoints](endpoints.md), [testing](testing.md).

## Ids

A commit id is `devnull:<commit id>` and a user id is `devnull:<author.id>`. A bare id with `--net devnull` is scoped to dev/null. An id from another network is refused with `<id> belongs to <network>, not dev/null` (for example `x:p1 belongs to X, not dev/null`). `Network` accepts `devnull`, `dev/null` and `dev-null` as names. Short commit hashes (the seven-character form the site shows) are not resolved.

## Fields

| SoCLI | dev/null source |
| --- | --- |
| `Post.id` | `devnull:` plus `id` |
| `Post.text` | `body` |
| `Post.url` and `permalink` | `html_url` |
| `Post.created_at` | `created_at` |
| `Post.in_reply_to` | scoped `parent_id` |
| `Post.liked` | `starred` |
| `Post.bookmarked` | always false (dev/null has no bookmarks) |
| `Metrics.replies` | `replies_count` |
| `Metrics.likes` | `stars_count` |
| `User.name` | `author.name`, else `author.username` |
| `User.username` | `author.username` |
| `User.avatar`, `description` | `author.avatar_url`, `author.bio` |
| `User.verified` | always false |

Reposts, quotes, bookmarks and views have no count on dev/null and map to 0. `starred` appears only in signed answers, so unsigned reads show it as false. The mapper does not read `is_saved`, which upstream removed. Posts show reply, an empty repost column and the star count, with no bookmark cell.

### Conversations

`Post.conversation_id` is the topmost ancestor found within the same page. When the topmost ancestor's own parent lies outside the page, that parent id is used. A post with no parent is its own conversation. A quoted post takes its `parent_id` or its own id.

### Deleted commits

A commit with `deleted_at` set, or one that has no author, becomes a placeholder post with the text `This post was deleted.` and the author `deleted`. Its parent links are kept, so replies stay attached. A deleted quoted commit shows `The quoted post was deleted.`.

### Link cards and media

`link_preview` (`url`, `title`, `description`, `image_url`, `site_name`, `embed.poster`) becomes the link card. The image is `image_url`, else `embed.poster`, and only `http` or `https` URLs are kept. The display form is the host without `www.` plus the path without a trailing slash, and the site name is kept.

`images` become photos. The key is the image id (string or number), else the URL, and the alt text is `description`. Dev/null media in the code is photos only.

### Quotes

`quoted_commit` becomes the quoted post. When SoCLI quotes, it uses the native quote (`quoted_commit_id`) only if the quoted post is on dev/null and the access is an approved device. Otherwise it appends the link after a blank line. An SSH-key login can post and reply but not quote.

## Accepted URLs

`net::parse_target` accepts `https://dev-null.sh/@<user>/commits/<id>`, where the host is `dev-null.sh` and the id is the segment after `commits`. The result is `devnull:<id>`. Profile links are built by `Network::profile_url` as `https://dev-null.sh/@{handle}`.

## Rich text

dev/null commits are plain text, so SoCLI sends no facets, mentions or tags. The `<b>`, `<strong>`, `<i>` and `<em>` conversion (nesting included) to Unicode sans-serif bold, italic and bold-italic letters applies to dev/null as it does everywhere (`src/rich.rs`), unless you pass `--raw`, which sends the text as typed. The converted letters are different characters from the plain ones: screen readers may spell them out and a search for the plain word does not match them.

## Length

`max_chars` is 4000, counted in characters (`count_graphemes` is false), so each converted bold letter counts as one character. The composer and `socli post --dry-run` use the same count.

Back to [network overview](../README.md).
