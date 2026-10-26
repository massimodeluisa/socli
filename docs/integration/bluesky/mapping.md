# Bluesky: mapping

SoCLI turns each Bluesky view object into the network-neutral types in `src/model.rs` (`Post`, `User`, `Metrics`, link card, `Media`, quoted post). The mapper is `src/net/bluesky/map.rs`, rich text is `src/net/bluesky/facets.rs`. Related pages: [overview](README.md), [auth](auth.md), [endpoints](endpoints.md), [testing](testing.md).

## Ids

A SoCLI id is `<slug>:<native>`. For Bluesky the native part is the AT URI of the post, so a post id looks like `bluesky:at://did:plc:abc/app.bsky.feed.post/3m`. A user id is `bluesky:<did>`. A bare id with `--net bluesky` is scoped to Bluesky. An id that belongs to another network is refused with `<id> belongs to <network>, not Bluesky` (for example `x:p1 belongs to X, not Bluesky`). `Network` accepts `bluesky` and `bsky` as names.

## Fields

| SoCLI | Bluesky source |
| --- | --- |
| `Post.id` | `bluesky:` plus the post `uri` |
| `Post.cid` | post `cid` (only Bluesky fills it; replies, quotes and bookmarks need it) |
| `Post.text` | `record.text` |
| `Post.created_at` | `record.createdAt`, else `indexedAt` |
| `Post.url` and `permalink` | `https://bsky.app/profile/{handle}/post/{rkey}` |
| `Post.conversation_id` | scoped `record.reply.root.uri`, else the post's own id |
| `Post.in_reply_to` | scoped `record.reply.parent.uri` |
| `Post.reposted_by` | handle of the reposter when a feed item carries a repost reason |
| `Post.liked`, `reposted` | `viewer.like` and `viewer.repost` are non-null |
| `Post.bookmarked` | `viewer.bookmarked` |
| `User.name` | `displayName`, else `handle` |
| `User.username` | `handle` |
| `User.verified` | `verification.verifiedStatus == "valid"` |
| `User.avatar`, `description` | `avatar`, `description` |

Metrics map as `replyCount` to replies, `repostCount` to reposts, `likeCount` to likes, `quoteCount` to quotes and `bookmarkCount` to bookmarks. Views are always 0, because the API gives none.

### Link cards and media

Links come from two places that the mapper merges: the `app.bsky.richtext.facet#link` features of the record (short form is the text span, expanded form is the URI, display form is the URI without scheme and trailing slash) and the `app.bsky.embed.external#view` card (title, description, thumbnail). When both describe the same URL they become one link. A thumbnail is kept only when it is an `http` or `https` URL.

Images come from `app.bsky.embed.images#view` (full-size URL, thumbnail as preview, alt text, aspect ratio, key `{rkey}:{index}`). Video comes from `app.bsky.embed.video#view` (playlist URL, thumbnail, kind video). `recordWithMedia` embeds contribute their media the same way.

### Quotes, replies and threads

A quoted post comes from `app.bsky.embed.record#view` or the record half of `recordWithMedia#view`. Items that are gone or blocked are dropped. The quoted post is mapped without its own quote, so nested quotes are not followed.

Feed pages also include the root and parent of replies as context posts. A thread page lists ancestors from the root down, then the target, then replies depth first. A thread opened through a handle link keeps the id that was asked for. Bookmarks skip items that are not found or blocked. Notifications hydrate their distinct post URIs in batches of 25, keep the notification order and drop posts the API no longer returns.

## Accepted URLs

`net::parse_target` accepts, for Bluesky:

| Input | Result |
| --- | --- |
| A scoped id | used as is |
| `https://bsky.app/profile/<actor>/post/<rkey>` (host may be `www.` or `mobile.` prefixed) | `bluesky:at://<actor>/app.bsky.feed.post/<rkey>`; a handle in `<actor>` is resolved to a DID before the call |
| A bare id with `--net bluesky` | the scoped id |
| Any other `bsky.app` path | error: `SoCLI cannot read <input>. use a post id, or a link from x.com, bsky.app or dev-null.sh` |

Profile links are built by `Network::profile_url` as `https://bsky.app/profile/{handle}`, with a leading `@` dropped.

## Rich text

When SoCLI posts, it builds facets from the text. Offsets are UTF-8 byte offsets, as the record format requires.

| Facet | Rule |
| --- | --- |
| Mention (`app.bsky.richtext.facet#mention`) | `@handle` at the start of the text or after whitespace or `(`. The handle needs a dot, uses ASCII letters, digits, `.` and `-`, and loses trailing dots. It is resolved through `resolveHandle`; a handle that does not resolve gets no facet. |
| Link (`app.bsky.richtext.facet#link`) | `http://` or `https://` URLs, not directly after an alphanumeric character. Trailing `. , ; : ! ? ) ] ' "` are trimmed. |
| Tag (`app.bsky.richtext.facet#tag`) | `#tag` at a word start, at most 64 graphemes, not all digits, ending at whitespace or punctuation other than `_`. |

A reply record carries `root` (the parent's own root when the parent is itself a reply, otherwise the parent) and `parent`. A quote is an `app.bsky.embed.record` embed with the quoted post's `uri` and `cid`. The record always has `$type`, `text` and `createdAt` (RFC 3339 with milliseconds and a `Z`), and the other fields appear only when present. When the quoted post is on another network, SoCLI appends its link after a blank line instead of using an embed.

## Bold and italic

SoCLI converts `<b>`, `<strong>`, `<i>` and `<em>` (nesting included) into Unicode sans-serif bold, italic and bold-italic letters, and bold digits, by default on every network (`src/rich.rs`). Unknown tags stay as typed. `--raw` sends the text as typed, with no conversion. Bluesky has no formatting in its text, so the converted letters are separate characters: screen readers may spell them out, and a search for the plain word does not match them. Each converted letter is one grapheme, so it counts as one character against the Bluesky limit.

## Length

`max_chars` is 300. Bluesky counts graphemes, signed in or not, so an emoji sequence counts as one. A post longer than 300 graphemes fails before any request with `Bluesky allows 300 characters; this post has N`. The composer shows the same count.

Back to [network overview](../README.md).
