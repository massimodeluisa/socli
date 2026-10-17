//! Bluesky JSON views turned into the reader's `Post`, `User` and `Page`.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::{Link, Media, MediaKind, Metrics, Page, Post, User};
use crate::net::{scoped, NetError, Network};

/// `getPosts` accepts at most this many uris per request.
pub const HYDRATE_BATCH: usize = 25;

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn time(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text(value, key)?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// The record key: the last segment of an `at://` uri.
pub fn rkey(uri: &str) -> &str {
    uri.rsplit('/').next().unwrap_or(uri)
}

/// Splits `at://<authority>/<collection>/<rkey>`.
pub fn parse_uri(uri: &str) -> Option<(&str, &str, &str)> {
    let mut parts = uri.strip_prefix("at://")?.splitn(3, '/');
    Some((parts.next()?, parts.next()?, parts.next()?))
}

pub fn post_url(handle: &str, uri: &str) -> String {
    format!("https://bsky.app/profile/{handle}/post/{}", rkey(uri))
}

// MARK: users

/// A `profileView` family object: an author, or the result of `getProfile`.
pub fn user(view: &Value) -> Option<User> {
    let did = text(view, "did")?;
    let handle = text(view, "handle")?;
    let name = text(view, "displayName")
        .filter(|n| !n.trim().is_empty())
        .unwrap_or(handle);
    Some(User {
        id: scoped(Network::Bluesky, did),
        name: name.to_string(),
        username: handle.to_string(),
        avatar_url: text(view, "avatar").map(str::to_string),
        verified: view
            .pointer("/verification/verifiedStatus")
            .and_then(Value::as_str)
            == Some("valid"),
        business: false,
        description: text(view, "description")
            .filter(|d| !d.trim().is_empty())
            .map(str::to_string),
    })
}

// MARK: posts

/// A `postView`; None for notFound, blocked and detached items.
pub fn post(view: &Value) -> Option<Post> {
    build(view, view.get("record")?, view.get("embed"), true)
}

fn build(view: &Value, record: &Value, embed: Option<&Value>, quotes: bool) -> Option<Post> {
    let uri = text(view, "uri")?;
    let cid = text(view, "cid")?;
    let author = user(view.get("author")?)?;
    let id = scoped(Network::Bluesky, uri);
    let reply = record.get("reply");
    let conversation_id = reply
        .and_then(|r| r.pointer("/root/uri"))
        .and_then(Value::as_str)
        .map_or_else(|| id.clone(), |root| scoped(Network::Bluesky, root));
    let in_reply_to = reply
        .and_then(|r| r.pointer("/parent/uri"))
        .and_then(Value::as_str)
        .map(|parent| scoped(Network::Bluesky, parent));
    let body = text(record, "text").unwrap_or_default();
    let mut links = facet_links(record, body);
    let mut media = Vec::new();
    if let Some(embed) = embed {
        collect_embed(embed, rkey(uri), &mut media, &mut links);
    }
    let viewer = view.get("viewer");
    let flag = |key: &str| {
        viewer
            .and_then(|v| v.get(key))
            .is_some_and(|v| !v.is_null())
    };
    Some(Post {
        id,
        net: Network::Bluesky,
        url: Some(post_url(&author.username, uri)),
        cid: Some(cid.to_string()),
        text: body.to_string(),
        created_at: time(record, "createdAt").or_else(|| time(view, "indexedAt")),
        conversation_id,
        in_reply_to,
        quoted: embed.filter(|_| quotes).and_then(quoted).map(Box::new),
        reposted_by: None,
        metrics: Metrics {
            replies: count(view, "replyCount"),
            reposts: count(view, "repostCount"),
            likes: count(view, "likeCount"),
            quotes: count(view, "quoteCount"),
            bookmarks: count(view, "bookmarkCount"),
            views: 0,
        },
        links,
        media,
        liked: flag("like"),
        reposted: flag("repost"),
        bookmarked: viewer
            .and_then(|v| v.get("bookmarked"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        author,
    })
}

/// The post inside `record#view` or `recordWithMedia#view`; None for gone or blocked records.
fn quoted(embed: &Value) -> Option<Post> {
    let inner = if embed.get("media").is_some() {
        embed.get("record")?.get("record")?
    } else {
        embed.get("record")?
    };
    let value = inner.get("value")?;
    build(
        inner,
        value,
        inner.get("embeds").and_then(|e| e.get(0)),
        false,
    )
}

fn facet_links(record: &Value, body: &str) -> Vec<Link> {
    let Some(facets) = record.get("facets").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut links = Vec::new();
    for facet in facets {
        let uri = facet
            .get("features")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|f| text(f, "$type").is_some_and(|t| t.ends_with("#link")))
            .find_map(|f| text(f, "uri"));
        let Some(uri) = uri else { continue };
        let span = |key: &str| {
            facet
                .pointer(&format!("/index/{key}"))
                .and_then(Value::as_u64)
                .map(|n| n as usize)
        };
        let short = span("byteStart")
            .zip(span("byteEnd"))
            .and_then(|(start, end)| body.get(start..end))
            .unwrap_or(uri);
        push_link(&mut links, short, uri, None, None);
    }
    links
}

fn push_link(
    links: &mut Vec<Link>,
    short: &str,
    uri: &str,
    title: Option<&str>,
    description: Option<&str>,
) {
    if links.iter().any(|l| l.expanded == uri) {
        return;
    }
    let display = uri
        .split_once("://")
        .map_or(uri, |(_, rest)| rest)
        .trim_end_matches('/');
    links.push(Link {
        short: short.to_string(),
        expanded: uri.to_string(),
        display: display.to_string(),
        title: title.filter(|t| !t.is_empty()).map(str::to_string),
        description: description.filter(|d| !d.is_empty()).map(str::to_string),
        ..Default::default()
    });
}

/// Fills the card fields a link is still missing, so a facet link and its card stay one link.
fn fill_card(
    links: &mut [Link],
    uri: &str,
    title: Option<&str>,
    description: Option<&str>,
    thumb: Option<&str>,
) {
    let Some(link) = links.iter_mut().find(|l| l.expanded == uri) else {
        return;
    };
    if link.title.is_none() {
        link.title = title.filter(|t| !t.is_empty()).map(str::to_string);
    }
    if link.description.is_none() {
        link.description = description.filter(|d| !d.is_empty()).map(str::to_string);
    }
    if link.image.is_none() {
        link.image = thumb
            .filter(|t| t.starts_with("https://") || t.starts_with("http://"))
            .map(str::to_string);
    }
}

fn collect_embed(embed: &Value, key: &str, media: &mut Vec<Media>, links: &mut Vec<Link>) {
    let embed = embed.get("media").unwrap_or(embed);
    let kind = text(embed, "$type").unwrap_or_default();
    if kind.ends_with("images#view") {
        for (i, image) in embed
            .get("images")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            media.push(media_item(
                format!("{key}:{i}"),
                MediaKind::Photo,
                text(image, "fullsize"),
                text(image, "thumb"),
                image,
            ));
        }
    } else if kind.ends_with("video#view") {
        media.push(media_item(
            format!("{key}:0"),
            MediaKind::Video,
            text(embed, "playlist"),
            text(embed, "thumbnail"),
            embed,
        ));
    } else if kind.ends_with("external#view") {
        if let Some(external) = embed.get("external") {
            if let Some(uri) = text(external, "uri") {
                push_link(
                    links,
                    uri,
                    uri,
                    text(external, "title"),
                    text(external, "description"),
                );
                fill_card(
                    links,
                    uri,
                    text(external, "title"),
                    text(external, "description"),
                    text(external, "thumb"),
                );
            }
        }
    }
}

fn media_item(
    key: String,
    kind: MediaKind,
    url: Option<&str>,
    preview: Option<&str>,
    source: &Value,
) -> Media {
    let dimension = |name: &str| {
        source
            .pointer(&format!("/aspectRatio/{name}"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    Media {
        key,
        kind,
        url: url.map(str::to_string),
        preview_url: preview.map(str::to_string),
        alt: text(source, "alt")
            .filter(|a| !a.is_empty())
            .map(str::to_string),
        width: dimension("width"),
        height: dimension("height"),
    }
}

// MARK: pages

fn push_unique(posts: &mut Vec<Post>, post: Post) {
    if !posts.iter().any(|p| p.id == post.id) {
        posts.push(post);
    }
}

/// FeedViewPost items in feed order, plus reply parents and roots so the trees render.
pub fn feed_page(items: &[Value], next: Option<String>) -> Page {
    let mut ids = Vec::new();
    let mut posts = Vec::new();
    for item in items {
        let Some(mut entry) = item.get("post").and_then(post) else {
            continue;
        };
        let reposted = item
            .pointer("/reason/$type")
            .and_then(Value::as_str)
            .is_some_and(|t| t.ends_with("reasonRepost"));
        if reposted {
            entry.reposted_by = item
                .pointer("/reason/by/handle")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        ids.push(entry.id.clone());
        push_unique(&mut posts, entry);
    }
    // Context goes in after every feed item, so a repost is not shadowed by its plain copy.
    for item in items {
        for key in ["root", "parent"] {
            if let Some(related) = item.pointer(&format!("/reply/{key}")).and_then(post) {
                push_unique(&mut posts, related);
            }
        }
    }
    Page { ids, posts, next }
}

/// A flat list of `postView`s: search results, or hydrated notifications.
pub fn posts_page(views: &[Value], next: Option<String>) -> Page {
    let posts: Vec<Post> = views.iter().filter_map(post).collect();
    Page {
        ids: posts.iter().map(|p| p.id.clone()).collect(),
        posts,
        next,
    }
}

/// Bookmark items hold a postView, or a notFound or blocked placeholder that is skipped.
pub fn bookmarks_page(items: &[Value], next: Option<String>) -> Page {
    let views: Vec<Value> = items
        .iter()
        .filter_map(|b| b.get("item").cloned())
        .collect();
    let mut page = posts_page(&views, next);
    for post in &mut page.posts {
        post.bookmarked = true;
    }
    page
}

/** Ancestors from the root down, the post itself, then its replies depth first. */
pub fn thread_page(thread: &Value) -> Option<Page> {
    let target = post(thread.get("post")?)?;
    let mut ancestors = Vec::new();
    let mut node = thread;
    while let Some(parent) = node.get("parent").filter(|p| p.get("post").is_some()) {
        if let Some(entry) = parent.get("post").and_then(post) {
            ancestors.push(entry);
        }
        node = parent;
    }
    ancestors.reverse();
    ancestors.push(target);
    flatten_replies(thread, &mut ancestors);
    Some(Page {
        ids: ancestors.iter().map(|p| p.id.clone()).collect(),
        posts: ancestors,
        next: None,
    })
}

fn flatten_replies(node: &Value, out: &mut Vec<Post>) {
    for reply in node
        .get("replies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(entry) = reply.get("post").and_then(post) {
            out.push(entry);
            flatten_replies(reply, out);
        }
    }
}

/// Gives the post at `from` the id `to` everywhere in the page, so a post asked for by a handle link stays findable.
pub fn rename(page: &mut Page, from: &str, to: &str) {
    let swap = |id: &mut String| {
        if id == from {
            *id = to.to_string();
        }
    };
    page.ids.iter_mut().for_each(swap);
    for post in &mut page.posts {
        swap(&mut post.id);
        swap(&mut post.conversation_id);
        if let Some(parent) = post.in_reply_to.as_mut() {
            swap(parent);
        }
    }
}

// MARK: notifications

/// The distinct post uris of a `listNotifications` response, in notification order.
pub fn notification_uris(response: &Value) -> Vec<String> {
    let mut uris: Vec<String> = Vec::new();
    for notification in response
        .get("notifications")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(uri) = text(notification, "uri") {
            if !uris.iter().any(|u| u == uri) {
                uris.push(uri.to_string());
            }
        }
    }
    uris
}

/**
Fetches the posts behind `uris` in batches of at most `HYDRATE_BATCH`, and returns the
`postView`s in the order of `uris`. Uris the API no longer returns are dropped.
*/
pub fn hydrate(
    uris: &[String],
    mut fetch: impl FnMut(&[String]) -> Result<Vec<Value>, NetError>,
) -> Result<Vec<Value>, NetError> {
    let mut found: Vec<Value> = Vec::new();
    for batch in uris.chunks(HYDRATE_BATCH) {
        found.extend(fetch(batch)?);
    }
    let mut ordered = Vec::with_capacity(uris.len());
    for uri in uris {
        if let Some(i) = found
            .iter()
            .position(|v| text(v, "uri") == Some(uri.as_str()))
        {
            ordered.push(found.swap_remove(i));
        }
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const AUTHOR_FEED: &str = include_str!("fixtures/author_feed.json");
    const POST_THREAD: &str = include_str!("fixtures/post_thread.json");
    const PROFILE: &str = include_str!("fixtures/profile.json");
    const SEARCH: &str = include_str!("fixtures/search.json");

    fn fixture(raw: &str) -> Value {
        serde_json::from_str(raw).expect("fixture is valid json")
    }

    fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
        value
            .get(key)
            .and_then(Value::as_array)
            .expect("fixture array")
    }

    #[test]
    fn author_feed_maps_posts_quotes_reposts_and_reply_context() {
        let feed = fixture(AUTHOR_FEED);
        let page = feed_page(
            array(&feed, "feed"),
            feed["cursor"].as_str().map(str::to_string),
        );
        assert_eq!(page.ids.len(), 5);
        assert_eq!(page.next, feed["cursor"].as_str().map(str::to_string));
        let first = page
            .posts
            .iter()
            .find(|p| p.id == page.ids[0])
            .expect("first");
        assert_eq!(first.net, Network::Bluesky);
        assert_eq!(first.author.username, "bsky.app");
        assert_eq!(first.author.name, "Bluesky");
        assert_eq!(first.conversation_id, first.id);
        assert_eq!(first.in_reply_to, None);
        assert_eq!(
            first.url.as_deref(),
            Some("https://bsky.app/profile/bsky.app/post/3myaytlknus25")
        );
        let quoted = first.quoted.as_ref().expect("quoted post");
        assert_eq!(quoted.author.username, "crumblsucks.wyattswickedgoods.com");
        assert!(quoted.text.starts_with("Happy None Pizza"));
        assert_eq!(quoted.media.len(), 1);
        assert_eq!(quoted.media[0].kind, MediaKind::Photo);
        assert_eq!(
            quoted.media[0].alt.as_deref(),
            Some("None Pizza, Left Beef")
        );
        assert_eq!(quoted.media[0].width, Some(250));
        assert!(quoted.quoted.is_none());

        let reposts: Vec<&Post> = page
            .posts
            .iter()
            .filter(|p| p.reposted_by.is_some())
            .collect();
        assert_eq!(reposts.len(), 2);

        let reply = page
            .posts
            .iter()
            .find(|p| p.id == page.ids[1])
            .expect("reply");
        let parent = reply.in_reply_to.as_ref().expect("reply parent");
        assert!(page.posts.iter().any(|p| &p.id == parent));
        assert!(page.posts.iter().any(|p| p.id == reply.conversation_id));
        assert_ne!(reply.conversation_id, reply.id);
        assert!(page.posts.len() > page.ids.len());
    }

    #[test]
    fn thread_flattens_replies_under_the_root_post() {
        let thread = fixture(POST_THREAD);
        let page = thread_page(&thread["thread"]).expect("thread page");
        fn nodes(node: &Value) -> usize {
            node["replies"]
                .as_array()
                .map_or(0, |r| r.iter().map(|n| 1 + nodes(n)).sum())
        }
        assert_eq!(page.posts.len(), 1 + nodes(&thread["thread"]));
        assert_eq!(page.ids.len(), page.posts.len());
        let root = &page.posts[0];
        assert_eq!(root.id, page.ids[0]);
        assert!(page
            .posts
            .iter()
            .skip(1)
            .all(|p| p.conversation_id == root.id));
        assert!(page.posts[1].in_reply_to.is_some());
    }

    #[test]
    fn thread_walks_parents_to_the_root_and_skips_gone_or_blocked_items() {
        let view = |n: u32| {
            json!({
                "uri": format!("at://did:plc:a/app.bsky.feed.post/{n}"),
                "cid": format!("cid{n}"),
                "author": {"did": "did:plc:a", "handle": "a.test"},
                "record": {"text": format!("post {n}"), "createdAt": "2026-01-01T00:00:00.000Z"},
                "indexedAt": "2026-01-01T00:00:00.000Z",
            })
        };
        let thread = json!({
            "$type": "app.bsky.feed.defs#threadViewPost",
            "post": view(3),
            "parent": {
                "post": view(2),
                "parent": {"post": view(1)},
            },
            "replies": [
                {"$type": "app.bsky.feed.defs#notFoundPost", "uri": "at://x/y/z", "notFound": true},
                {"$type": "app.bsky.feed.defs#blockedPost", "uri": "at://x/y/b", "blocked": true,
                 "author": {"did": "did:plc:b"}},
                {"post": view(4), "replies": [{"post": view(5), "replies": []}]},
            ],
        });
        let page = thread_page(&thread).expect("page");
        let texts: Vec<&str> = page.posts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["post 1", "post 2", "post 3", "post 4", "post 5"]
        );
        let blocked_parent = json!({"post": view(3), "parent": {"uri": "u", "blocked": true}});
        let page = thread_page(&blocked_parent).expect("page");
        assert_eq!(page.posts.len(), 1);
    }

    #[test]
    fn rename_keeps_the_requested_id_on_the_page() {
        let thread = fixture(POST_THREAD);
        let mut page = thread_page(&thread["thread"]).expect("thread page");
        let root = page.ids[0].clone();
        rename(
            &mut page,
            &root,
            "bluesky:at://bsky.app/app.bsky.feed.post/x",
        );
        assert_eq!(page.ids[0], "bluesky:at://bsky.app/app.bsky.feed.post/x");
        assert_eq!(page.posts[0].id, page.ids[0]);
        assert!(page
            .posts
            .iter()
            .skip(1)
            .all(|p| p.conversation_id == page.ids[0]));
        assert!(page
            .posts
            .iter()
            .any(|p| p.in_reply_to.as_deref() == Some(&page.ids[0])));
    }

    #[test]
    fn profile_maps_the_user() {
        let user = user(&fixture(PROFILE)).expect("user");
        assert_eq!(user.id, "bluesky:did:plc:z72i7hdynmk6r22z27h6tvur");
        assert_eq!(user.username, "bsky.app");
        assert_eq!(user.name, "Bluesky");
        assert!(user.avatar_url.is_some());
        assert!(user.description.is_some());
        assert!(!user.verified);
        let verified = json!({"did": "did:plc:a", "handle": "a.test",
            "verification": {"verifiedStatus": "valid"}});
        assert!(super::user(&verified).expect("user").verified);
        let nameless = json!({"did": "did:plc:a", "handle": "a.test", "displayName": " "});
        assert_eq!(super::user(&nameless).expect("user").name, "a.test");
    }

    #[test]
    fn search_results_become_a_page() {
        let search = fixture(SEARCH);
        let page = posts_page(array(&search, "posts"), Some("3".into()));
        assert_eq!(page.posts.len(), 3);
        assert_eq!(page.ids.len(), 3);
        assert_eq!(page.next.as_deref(), Some("3"));
    }

    #[test]
    fn viewer_state_and_counts_map_onto_the_post() {
        let view = json!({
            "uri": "at://did:plc:a/app.bsky.feed.post/3k",
            "cid": "cid",
            "author": {"did": "did:plc:a", "handle": "a.test"},
            "record": {"text": "hi", "createdAt": "not a date"},
            "indexedAt": "2026-02-03T04:05:06.000Z",
            "replyCount": 1, "repostCount": 2, "likeCount": 3, "quoteCount": 4, "bookmarkCount": 5,
            "viewer": {"like": "at://did:plc:me/app.bsky.feed.like/1", "bookmarked": true},
        });
        let post = post(&view).expect("post");
        assert!(post.liked && post.bookmarked && !post.reposted);
        assert_eq!(
            (
                post.metrics.replies,
                post.metrics.reposts,
                post.metrics.likes
            ),
            (1, 2, 3)
        );
        assert_eq!((post.metrics.quotes, post.metrics.bookmarks), (4, 5));
        assert_eq!(
            post.created_at.map(|t| t.to_rfc3339()),
            Some("2026-02-03T04:05:06+00:00".to_string())
        );
    }

    #[test]
    fn facet_links_external_cards_and_video_map_onto_the_post() {
        let text = "see example.com/a… now";
        let start = text.find("example").expect("start");
        let end = text.find(" now").expect("end");
        let view = json!({
            "uri": "at://did:plc:a/app.bsky.feed.post/3k",
            "cid": "cid",
            "author": {"did": "did:plc:a", "handle": "a.test"},
            "record": {
                "text": text,
                "createdAt": "2026-01-01T00:00:00.000Z",
                "facets": [{
                    "index": {"byteStart": start, "byteEnd": end},
                    "features": [{"$type": "app.bsky.richtext.facet#link", "uri": "https://example.com/a-long-path"}],
                }],
            },
            "embed": {
                "$type": "app.bsky.embed.recordWithMedia#view",
                "record": {"record": {"$type": "app.bsky.embed.record#viewNotFound", "uri": "x"}},
                "media": {
                    "$type": "app.bsky.embed.external#view",
                    "external": {"uri": "https://card.test/", "title": "Card", "description": "About"},
                },
            },
        });
        let post = post(&view).expect("post");
        assert_eq!(post.links.len(), 2);
        assert_eq!(post.links[0].short, "example.com/a…");
        assert_eq!(post.links[0].display, "example.com/a-long-path");
        assert_eq!(post.links[1].title.as_deref(), Some("Card"));
        assert_eq!(post.links[1].display, "card.test");
        assert!(post.quoted.is_none());

        let video = json!({
            "uri": "at://did:plc:a/app.bsky.feed.post/3v",
            "cid": "cid",
            "author": {"did": "did:plc:a", "handle": "a.test"},
            "record": {"text": "", "createdAt": "2026-01-01T00:00:00.000Z"},
            "embed": {"$type": "app.bsky.embed.video#view", "playlist": "https://v/p.m3u8",
                      "thumbnail": "https://v/t.jpg", "alt": "clip"},
        });
        let post = super::post(&video).expect("post");
        assert_eq!(post.media[0].kind, MediaKind::Video);
        assert_eq!(post.media[0].url.as_deref(), Some("https://v/p.m3u8"));
        assert_eq!(
            post.media[0].preview_url.as_deref(),
            Some("https://v/t.jpg")
        );
    }

    #[test]
    fn external_card_thumb_becomes_the_link_image() {
        let view = json!({
            "uri": "at://did:plc:a/app.bsky.feed.post/3c",
            "cid": "cid",
            "author": {"did": "did:plc:a", "handle": "a.test"},
            "record": {"text": "", "createdAt": "2026-01-01T00:00:00.000Z"},
            "embed": {
                "$type": "app.bsky.embed.external#view",
                "external": {"uri": "https://card.test/a", "title": "Card", "description": "About",
                             "thumb": "https://cdn.bsky.app/img/feed_thumbnail/plain/did:plc:a/bafk@jpeg"},
            },
        });
        let post = post(&view).expect("post");
        assert_eq!(post.links.len(), 1);
        assert_eq!(
            post.links[0].image.as_deref(),
            Some("https://cdn.bsky.app/img/feed_thumbnail/plain/did:plc:a/bafk@jpeg")
        );
        assert_eq!(post.links[0].title.as_deref(), Some("Card"));
        assert_eq!(post.links[0].site, None);
        assert_eq!(post.links[0].image_width, None);
    }

    #[test]
    fn a_facet_link_and_its_card_merge_into_one_link() {
        let text = "see card.test/a";
        let start = text.find("card.test/a").expect("start");
        let end = start + "card.test/a".len();
        let view = json!({
            "uri": "at://did:plc:a/app.bsky.feed.post/3m",
            "cid": "cid",
            "author": {"did": "did:plc:a", "handle": "a.test"},
            "record": {
                "text": text,
                "createdAt": "2026-01-01T00:00:00.000Z",
                "facets": [{
                    "index": {"byteStart": start, "byteEnd": end},
                    "features": [{"$type": "app.bsky.richtext.facet#link", "uri": "https://card.test/a"}],
                }],
            },
            "embed": {
                "$type": "app.bsky.embed.external#view",
                "external": {"uri": "https://card.test/a", "title": "Card", "description": "About",
                             "thumb": "https://cdn.bsky.app/t.jpg"},
            },
        });
        let post = post(&view).expect("post");
        assert_eq!(post.links.len(), 1);
        assert_eq!(post.links[0].title.as_deref(), Some("Card"));
        assert_eq!(post.links[0].description.as_deref(), Some("About"));
        assert_eq!(
            post.links[0].image.as_deref(),
            Some("https://cdn.bsky.app/t.jpg")
        );
    }

    #[test]
    fn a_card_thumb_that_is_not_http_is_dropped() {
        let view = json!({
            "uri": "at://did:plc:a/app.bsky.feed.post/3j",
            "cid": "cid",
            "author": {"did": "did:plc:a", "handle": "a.test"},
            "record": {"text": "", "createdAt": "2026-01-01T00:00:00.000Z"},
            "embed": {
                "$type": "app.bsky.embed.external#view",
                "external": {"uri": "https://card.test/a", "title": "Card", "description": "About",
                             "thumb": "javascript:alert(1)"},
            },
        });
        let post = post(&view).expect("post");
        assert_eq!(post.links.len(), 1);
        assert_eq!(post.links[0].image, None);
    }

    #[test]
    fn bookmarks_skip_items_that_are_not_posts() {
        let post_view = json!({
            "uri": "at://did:plc:a/app.bsky.feed.post/3k",
            "cid": "cid",
            "author": {"did": "did:plc:a", "handle": "a.test"},
            "record": {"text": "kept"},
            "indexedAt": "2026-01-01T00:00:00.000Z",
        });
        let items = vec![
            json!({"subject": {"uri": "u1", "cid": "c"}, "item": post_view}),
            json!({"subject": {"uri": "u2", "cid": "c"},
                   "item": {"$type": "app.bsky.feed.defs#notFoundPost", "uri": "u2", "notFound": true}}),
            json!({"subject": {"uri": "u3", "cid": "c"},
                   "item": {"$type": "app.bsky.feed.defs#blockedPost", "uri": "u3", "blocked": true,
                            "author": {"did": "did:plc:b"}}}),
        ];
        let page = bookmarks_page(&items, None);
        assert_eq!(page.ids.len(), 1);
        assert!(page.posts[0].bookmarked);
    }

    #[test]
    fn notifications_hydrate_in_batches_of_25_and_keep_order() {
        let notifications: Vec<Value> = (0..60)
            .map(|n| json!({"uri": format!("at://did:plc:a/app.bsky.feed.post/{n:03}"), "reason": "reply"}))
            .chain([json!({"uri": "at://did:plc:a/app.bsky.feed.post/000", "reason": "mention"})])
            .collect();
        let uris = notification_uris(&json!({"notifications": notifications}));
        assert_eq!(uris.len(), 60);
        let mut sizes = Vec::new();
        let hydrated = hydrate(&uris, |batch| {
            sizes.push(batch.len());
            Ok(batch.iter().rev().map(|u| json!({"uri": u})).collect())
        })
        .expect("hydrate");
        assert_eq!(sizes, vec![25, 25, 10]);
        let order: Vec<&str> = hydrated
            .iter()
            .map(|v| v["uri"].as_str().unwrap_or(""))
            .collect();
        let expected: Vec<&str> = uris.iter().map(String::as_str).collect();
        assert_eq!(order, expected);
    }

    #[test]
    fn hydrate_drops_uris_the_api_no_longer_returns() {
        let uris = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let hydrated = hydrate(&uris, |_| {
            Ok(vec![json!({"uri": "c"}), json!({"uri": "a"})])
        })
        .expect("ok");
        let order: Vec<&str> = hydrated
            .iter()
            .map(|v| v["uri"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(order, vec!["a", "c"]);
    }

    #[test]
    fn at_uris_split_into_authority_collection_and_rkey() {
        assert_eq!(
            parse_uri("at://did:plc:a/app.bsky.feed.post/3k"),
            Some(("did:plc:a", "app.bsky.feed.post", "3k"))
        );
        assert_eq!(parse_uri("did:plc:a"), None);
        assert_eq!(rkey("at://did:plc:a/app.bsky.feed.post/3k"), "3k");
    }
}
