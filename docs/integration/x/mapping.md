# X data mapping

socli keeps one model for every network (`src/model.rs`), and the X client (`src/net/x/client.rs`) fills it from the API v2 response. A response carries the posts in `data` and the objects they refer to in `includes` (users, media, referenced posts). socli joins them while it parses, so a `Post` arrives with its author, media, links and quoted post already attached.

Back to [X](README.md). Related: [auth](auth.md), [endpoints](endpoints.md), [testing](testing.md).

## Identifiers and links

socli writes every id as `<slug>:<native id>`, so an X post is `x:1849213377`. `net::parse_target` turns user input into such an id:

| Input | Result |
| --- | --- |
| `x:1849213377` | used as is |
| `https://x.com/<user>/status/<digits>` | `x:<digits>`. `twitter.com` works too, and a leading `www.` or `mobile.` on the host is ignored. Path segments after the id are ignored. |
| `1849213377` with `--net x`, with the X scope selected, or with `default_network = "x"` | `x:1849213377` |
| `1849213377` with no network | an error: "which network? add --net x\|bluesky\|devnull\|threads, or use a full id like bluesky:at://…" |

The post permalink is `https://x.com/<username>/status/<id>`, with `i` in place of the username when the author is unknown. `Network::profile_url` builds `https://x.com/<handle>` for a profile.

## Fields

| socli | X API source |
| --- | --- |
| `Post.id` | `data.id`, scoped as `x:<id>` |
| `Post.text` | `note_tweet.text` when present (long posts over 280 characters), else `text`. Each `t.co` short link is replaced by its `expanded_url`. |
| `Post.author` | the `includes.users` entry for `author_id`. With no match the author's name is "unknown". |
| `Post.created_at` | `created_at` |
| `Post.conversation_id` | `conversation_id`, defaulting to the post's own id |
| `Post.in_reply_to` | the `replied_to` entry of `referenced_tweets` |
| `Post.quoted` | the `quoted` entry of `referenced_tweets`, when that post is in `includes.tweets`. Otherwise it stays empty. |
| `Post.reposted_by` | the author's username, when the post is a `retweeted` reference. The post shown is then the original, and its own id is kept. |
| `User.name`, `username`, `avatar_url`, `verified`, `description` | `name`, `username`, `profile_image_url`, `verified`, `description` |
| `User.business` | true when `verified_type` is `business`; the rich level then draws the profile picture as a rounded square |
| `Post.liked`, `bookmarked`, `reposted` | always false. The read endpoints do not say whether you did these. |

`Metrics` come from `public_metrics`:

| `Metrics` | X field |
| --- | --- |
| `replies` | `reply_count` |
| `reposts` | `retweet_count` |
| `likes` | `like_count` |
| `quotes` | `quote_count` |
| `bookmarks` | `bookmark_count` |
| `views` | `impression_count` |

The link card (`Link`) is built from `entities.urls`: `short` (the `t.co` link), `expanded`, `display` (`display_url`), `title`, `description`, and `image` with its width and height. When an entry has several `images`, socli picks the widest http(s) one, or the first when none gives a width. The `site` name stays empty for X.

`Media` comes from the `includes.media` entries named in `attachments.media_keys`: `photo` becomes a photo, `video` a video, `animated_gif` a gif, and any other type is shown as generic media. `url`, `preview_image_url`, `alt_text`, `width` and `height` carry over.

## Quotes and replies when posting

A reply sends `reply.in_reply_to_tweet_id` with the native id. A quote never uses X's quote field. `socli quote <id> "text"` posts your text, a blank line and the quoted post's permalink. When the quoted post's author is not loaded socli cannot build the link, and `socli quote` fails before any request with "can't quote this post on X: its author is unknown. Open it and press s to copy the link". The same happens in the composer.

## Rich text

socli converts `<b>`, `<strong>`, `<i>` and `<em>` (any letter case, nesting allowed) into Unicode sans-serif bold, italic and bold-italic letters before it sends a post on any network, X included. Digits are made bold but italic leaves them unchanged. Pass `--raw` to send the text exactly as typed. X's own bold exists only in its composer for Premium accounts, and the post body socli sends has no formatting field, so the Unicode letters carry the emphasis. Screen readers spell these letters one by one, search does not match them, and each one counts as two characters on X.

The post body socli sends holds only the text (and the reply target for a reply), so no mention, hashtag or link entity is attached by socli. Links are counted at a fixed length, described next.

## Length

The limit is 280 (`Caps.max_chars`), and the composer counts with `count::x_len`, which follows the way x.com weights text:

- A word that starts with `http://` or `https://` counts as 23, whatever its length.
- An emoji grapheme counts as 2, including multi-part emoji such as a family (one grapheme, 2).
- Any other character outside the light ranges (U+0000 to U+10FF, U+2000 to U+200D, U+2010 to U+201F, U+2032 to U+2037) counts as 2.
- Everything else counts as 1.

The count works on characters and graphemes as listed, not on bytes, so `count_graphemes` is false in `Caps`. Bold Unicode letters sit outside the light ranges, so a bolded five-letter word counts as 10 (`unicode_bold_letters_count_two` in `src/count.rs`). The tests `text_len_counts_the_way_each_network_does` in `src/net/mod.rs` pin three values: a short sentence with a long URL counts 26, a family emoji counts 2, and `𝗛i` counts 3.
