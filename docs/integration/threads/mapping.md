# Threads data mapping

The Threads API describes every post as a "media object", and `src/net/threads/map.rs` turns each one into SoCLI's shared `Post` (`src/model.rs`). The mapping is lean: Threads gives SoCLI an author name, text, a permalink, media and a few relations, and it gives no counts, so the counts of a Threads post stay at zero.

Back to [Threads](README.md). Related: [auth](auth.md), [endpoints](endpoints.md), [testing](testing.md).

## Identifiers and links

SoCLI writes every id as `<slug>:<native id>`, so a Threads post is `threads:<id>`. A bare id is also accepted wherever the command already names the network (`--net threads`, the Threads scope, or `default_network`), and a scoped id of another network is refused with "{id} belongs to {Network}, not Threads" (for example "bluesky:at://… belongs to Bluesky, not Threads").

`net::parse_target` does not accept Threads post links. A link on `threads.net` or `threads.com` fails with "Threads post links cannot be resolved through the API; use the id from `socli --json`". Take the id from `socli feed mine --net threads --json` or from the output of a command that listed the post.

The permalink of a post is the `permalink` field Meta returns (for example `https://www.threads.com/@<user>/post/<shortcode>`). `Network::profile_url` builds `https://www.threads.com/@<handle>`, dropping a leading `@` from the handle.

## Fields

| SoCLI | Threads source |
| --- | --- |
| `Post.id` | `id`, scoped as `threads:<id>` |
| `Post.url` | `permalink` |
| `Post.author` | `username` for both the display name and the handle. The user id is `owner` when the response lists it, else the username. Once SoCLI knows your account, posts by your username are re-pointed at your user id. |
| `Post.text` | `text` |
| `Post.created_at` | `timestamp`, in Meta's `+0000` offset form or RFC 3339 |
| `Post.conversation_id` | `root_post`, else the post's own id |
| `Post.in_reply_to` | `replied_to` |
| `Post.quoted` | `quoted_post`. Meta may return only an id, so SoCLI keeps a stub and fetches the full post with `GET /{id}` (at most ten fetches per page). A failed fetch leaves the stub. |
| `Post.reposted_by` | the reposter's username. A post with media type `REPOST_FACADE` is replaced by the original named in `reposted_post`, and `reposted_by` is set. |
| `Post.metrics` | all zero. SoCLI does not request counts. |
| `Post.liked`, `bookmarked`, `reposted` | always false |
| `Post.links` | one link card from `link_attachment_url`, with `display` as the address without scheme, `www.` and trailing slash. It has no title, description or image. |

The profile (`GET /me`) maps `id` (scoped), `username`, `name` (falling back to the username), `threads_profile_picture_url` as the avatar, `threads_biography` as the description and `is_verified`.

`Media` comes from the post itself. An `IMAGE` becomes a photo and a `VIDEO` a video, with `media_url` as the file, `thumbnail_url` as the preview and `alt_text` as the alt text. `AUDIO` is carried as generic media. A carousel (`CAROUSEL_ALBUM` or `CAROUSEL`) becomes the media of its `children`. Width and height are not provided, so SoCLI leaves them empty.

## Quotes and replies when posting

A reply sends `reply_to_id` and a quote sends `quote_post_id`, both with the native id. Quoting is native only when the quoted post is on Threads. When the quoted post comes from another network, `socli quote` posts your text followed by that post's link instead. A reply to a post on another network is refused, because the id belongs to a different network.

## Rich text

SoCLI converts `<b>`, `<strong>`, `<i>` and `<em>` into Unicode sans-serif bold, italic and bold-italic letters before it sends a post on any network, Threads included. Pass `--raw` to send the text as typed. The Threads post body SoCLI sends (`media_type=TEXT` and `text`) has no formatting field, so the Unicode letters carry the emphasis. Screen readers spell these letters one by one and search does not match them. For length, each styled letter is a character outside the emoji ranges below, so it counts once.

## Length

The limit is 500 characters (`Caps.max_chars`), and `threads::text_len` counts the way the API does:

- Most characters count as 1, including accented letters.
- Characters in these ranges count as their length in UTF-8 bytes: U+1F000 to U+1FAFF, U+2300 to U+23FF, U+2600 to U+27BF, U+2B00 to U+2BFF, U+FE0F and U+200D. A flag counts 8, and a family emoji built from several people joined with U+200D counts the bytes of every part.
- `count_graphemes` is false, so the counter works on characters, not grapheme clusters.

Before sending, SoCLI rejects empty text, text over 500, and more than five distinct `http://` or `https://` links (a link repeated in the text counts once). The tests `text_len_counts_emoji_as_bytes`, `text_len_sums_a_zwj_family`, `text_len_counts_a_flag`, `five_links_are_allowed`, `six_links_are_rejected` and `repeated_links_count_once` in `src/net/threads/mod.rs` pin these rules.
