# Mastodon: mapping

socli turns each Mastodon status and account into the network-neutral types in `src/model.rs` (`Post`, `User`, `Metrics`, link card, `Media`, quoted post). The mapper is `src/net/mastodon/map.rs`, and `src/net/mastodon/html.rs` turns the HTML of a status into text. Related pages: [overview](README.md), [auth](auth.md), [endpoints](endpoints.md), [testing](testing.md).

## Ids

A socli id is `<slug>:<native>`. A Mastodon status id belongs to the server that issued it, so the native part takes one of two forms:

| Form | Example | Used for |
| --- | --- | --- |
| The status id | `mastodon:115000000000000001` | Posts the client's own server returned. Always digits |
| The post link | `mastodon:https://fosstodon.org/@theo/115000000000000010` | A link you gave socli, kept as written until it is used |

A link whose host is the client's own server, and whose last path segment is digits, becomes the local id without a request. A link from another server is resolved through `GET /api/v2/search` with `resolve=true`, which needs a token. Signed out, socli answers ``sign in to open posts from <host>: `socli login mastodon` ``. When the thread was opened through a link, the page keeps the id that was asked for, so the scoped id you passed is the one you see again. A user id is `mastodon:<account id>`. A bare id with `--net mastodon` is scoped to Mastodon, and an id from another network is refused with `<id> belongs to <network>, not Mastodon`. `Network` accepts `mastodon` and the alias `masto`.

## Fields

| socli | Mastodon source |
| --- | --- |
| `Post.id` | `mastodon:` plus the status `id` |
| `Post.text` | `content` (HTML) turned into text, plus the content warning and poll lines below |
| `Post.created_at` | `created_at` |
| `Post.url` and `permalink` | `url`, else `uri` |
| `Post.conversation_id` | see [conversations](#conversations) |
| `Post.in_reply_to` | `in_reply_to_id`, scoped |
| `Post.reposted_by` | the full handle of the account that boosted the post |
| `Post.liked`, `reposted`, `bookmarked` | `favourited`, `reblogged`, `bookmarked` |
| `User.name` | `display_name`, else `username` |
| `User.username` | the full handle, `user@server` |
| `User.avatar`, `description` | `avatar_static` (else `avatar`) when it is an `http` or `https` URL; `note` turned into text |
| `User.verified` | always false |

Metrics map as `replies_count` to replies, `reblogs_count` to reposts, `favourites_count` to likes and `quotes_count` to quotes. Bookmarks and views are always 0, because the API gives none. In the Bookmarks feed every post is marked bookmarked.

### Full handles

Mastodon gives a local author as `ines` and a remote one as `theo@fosstodon.org`. socli always writes the full form: it adds the client's server to a local name, so the same author reads `ines@mastodon.social` everywhere. Mentions in the text use the `mentions` array of the status to print `@theo@fosstodon.org`. Hashtags stay text.

### Boosts

A feed item with a `reblog` object is shown as the inner post, and `reposted_by` carries the booster's full handle. A page lists a post once, even when several items point at the same status.

### Content warnings and polls

A content warning (`spoiler_text`) does not hide the post. socli puts `CW: <warning>` first, then a blank line and the text, or just the `CW:` line when the text is empty. A poll adds one line per option below the text, as `○ <option> · <votes>`, or `○ <option>` when the server hides the count. socli shows polls and does not vote.

### Quotes

A status carries a quote when `quote.state` is `accepted` and `quote.quoted_status` is an object. socli then makes that status the quoted post, mapped by the same rules, and removes the quote's own traces from the text: the link to the quoted post is dropped from the links, and a last paragraph that is only that link is cut. Servers add a fallback paragraph (`quote-inline`, usually starting with `RE:`) for clients that do not know quotes, and socli always drops that paragraph. A quote in any other state (pending, rejected, revoked) shows no quoted post.

### Links and cards

The links of a post are the `href` of every anchor in the HTML that is not a mention or a hashtag, in order and without repeats. The short and expanded form are the address, and the display form is the address without scheme and trailing slash. The `card` of the status fills the link with the same address: title, description (when not empty), preview image (when it is an `http` or `https` URL, with its width and height when positive) and site (`provider_name`). A card whose address matches no link becomes an extra link. Link previews render only in the full-screen reader.

### Media

Each attachment becomes one `Media` item: `image` is a photo, `gifv` a gif, `video` a video, and every other type (audio, for example) is listed as other. The fields are `id` as key, `url`, `preview_url`, `description` as alt text, and the width and height from `meta.original`.

### Conversations

In a feed, a profile, search results, notifications and bookmarks, socli groups replies by climbing `in_reply_to` through the posts of the same page. The conversation id is the topmost parent found in the page. When that post's own parent is not in the page, the id is that missing parent, so replies to the same missing post still group together. A thread page lists the ancestors from the root down, then the focused post, then the replies in the order the API returns them, and gives all of them the root's id. A thread opened through `GET /api/v1/statuses/{id}/context` is shown whole, and it stops at what the server returns.

### Notifications

`socli notifications` asks for mentions only and shows the status of each notification. A notification without a status is skipped.

## Accepted URLs

`net::parse_target` accepts, for Mastodon:

| Input | Result |
| --- | --- |
| A scoped id | used as is |
| `https://<server>/@<user>/<digits>` | `mastodon:https://<server>/@<user>/<digits>` |
| `https://<server>/@<user>@<other>/<digits>` | `mastodon:https://<server>/@<user>@<other>/<digits>` (the link as seen on your server) |
| `https://<server>/users/<user>/statuses/<digits>` | `mastodon:https://<server>/@<user>/<digits>` |
| A bare id with `--net mastodon` | the scoped id |

Any host except `x.com`, `twitter.com`, `bsky.app`, `dev-null.sh` and Threads is read as a possible Mastodon server. A query string or fragment is dropped, and `www.` or `mobile.` is removed only for the known hosts. A path of any other shape fails with `socli cannot read <input>. use a post id, or a link from x.com, bsky.app, dev-null.sh or a Mastodon server`.

Profile links come from `profile_url` in `src/net/mastodon/mod.rs`. `user@host` gives `https://<host>/@user`, a handle without a server gives `https://mastodon.social/@<handle>`, and a leading `@` is dropped.

## Replies and quotes

When socli posts, it builds a JSON body with `status` and adds three fields only when they apply.

| Field | Rule |
| --- | --- |
| `in_reply_to_id` | The `id` of the parent, read from the server. |
| `visibility` | Copied from the parent when it is `unlisted`, `private` or `direct`. A public parent leaves the field out, and the server applies your account's default. |
| `quoted_status_id` | The `id` of the quoted post, sent when the server's answer carries `quote_approval`. |

Mastodon delivers a reply to another server only to the accounts the reply mentions, so socli puts `@<parent author> ` in front of a reply that lacks it. The check ignores case and is skipped when the parent is your own post. A reply to a remote author names the full handle, as in `@theo@fosstodon.org`.

A quote depends on the server. When the quoted status has no `quote_approval` field, the server predates quotes (they arrived in Mastodon 4.5, according to the official statuses page), and socli appends `\n\n<link>` to your text, using `url` and then `uri`. When `quote_approval.current_user` is `denied`, socli stops with `the author of that post does not allow quotes. share its link instead`. Otherwise it sends `quoted_status_id`. A quote of a post on another network is sent as your text followed by that post's link.

## Bold and italic

socli converts `<b>`, `<strong>`, `<i>` and `<em>` (nesting included) into Unicode sans-serif bold, italic and bold-italic letters, and bold digits, by default on every network (`src/rich.rs`). Unknown tags stay as typed. `--raw` sends the text as typed, with no conversion. The Mastodon API takes plain text, so the converted letters are separate characters: screen readers may spell them out, and a search for the plain word does not match them. Each converted letter is one grapheme, so it counts as one character against the limit.

## Length

`max_chars` is the server's limit (500 when the server does not say), read at sign-in and stored. Mastodon counts graphemes, signed in or not, so an emoji sequence counts as one. socli counts the text in whitespace-separated pieces. A piece that starts with `http://` or `https://` counts as 23 characters whatever its length, and a piece of the form `@user@domain` counts as `@user`. Every other piece counts its graphemes, and each whitespace character counts as 1. A reply is counted after socli adds the mention. A post longer than the limit fails before any request with `Mastodon allows N characters on <server>; this post has M`. The composer shows the same count.

For example, `<b>hi</b> https://example.org/a/very/long/path/that/goes/on/and/on @theo@fosstodon.org` counts 32 in a dry run: 2 for the converted `hi`, 23 for the link, 5 for `@theo`, and 2 for the spaces. With `--raw` the tags stay in the text and `<b>hi</b>` alone counts 9.

Back to [network overview](../README.md).
