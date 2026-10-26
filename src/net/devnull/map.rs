//! Commits, users and pages as the API returns them, and how they become SoCLI posts.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::model::{Link, Media, MediaKind, Metrics, Page, Post, User};
use crate::net::{scoped, Network};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Commit {
    pub id: String,
    pub body: Option<String>,
    pub parent_id: Option<String>,
    pub html_url: Option<String>,
    pub created_at: Option<String>,
    pub deleted_at: Option<String>,
    pub stars_count: Option<u64>,
    pub replies_count: Option<u64>,
    pub link_preview: Option<LinkPreview>,
    pub images: Option<Vec<Image>>,
    pub author: Option<Author>,
    pub starred: Option<bool>,
    pub parent: Option<Box<Commit>>,
    pub quoted_commit: Option<Box<Commit>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Author {
    pub id: String,
    pub username: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct LinkPreview {
    pub url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub site_name: Option<String>,
    pub embed: Option<LinkEmbed>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct LinkEmbed {
    pub poster: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Image {
    pub id: Option<Value>,
    pub url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub description: Option<String>,
}

/// A JSON resource the server wraps in `data`.
#[derive(Debug, Deserialize)]
pub struct One<T> {
    pub data: T,
}

/// A signed `GET commits/{id}`: the commit, and the commits it replies to, root first.
#[derive(Debug, Deserialize)]
pub struct Shown {
    pub data: Commit,
    #[serde(default)]
    pub ancestors: Vec<Commit>,
}

/// A cursor-paginated list: `data` plus `meta.next_cursor`.
#[derive(Debug, Deserialize)]
pub struct List<T> {
    #[serde(default = "Vec::new")]
    pub data: Vec<T>,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub next_cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Notification {
    pub commit: Option<Commit>,
}

pub fn user(author: &Author) -> User {
    User {
        id: scoped(Network::Devnull, &author.id),
        name: non_empty(author.name.as_deref())
            .unwrap_or(&author.username)
            .to_string(),
        username: author.username.clone(),
        avatar_url: author.avatar_url.clone(),
        verified: false,
        business: false,
        description: non_empty(author.bio.as_deref()).map(str::to_string),
    }
}

/** A page of commits in API order, plus the parents a signed response expands. A commit's conversation is its topmost ancestor in the page, that ancestor's parent when it is outside the page, or the commit itself. */
pub fn page(items: Vec<Commit>, next: Option<String>) -> Page {
    let mut all: Vec<Commit> = Vec::new();
    let mut ids = Vec::new();
    for mut commit in items {
        let parent = commit.parent.take();
        if let Some(post_id) = mappable(&commit) {
            if !ids.contains(&post_id) {
                ids.push(post_id);
            }
        }
        push_unique(&mut all, commit);
        if let Some(parent) = parent {
            push_unique(&mut all, *parent);
        }
    }
    let parents: HashMap<&str, Option<&str>> = all
        .iter()
        .map(|c| (c.id.as_str(), c.parent_id.as_deref()))
        .collect();
    let posts = all
        .iter()
        .filter_map(|c| post(c, conversation(&c.id, &parents)))
        .collect();
    Page { ids, posts, next }
}

fn push_unique(all: &mut Vec<Commit>, commit: Commit) {
    if !commit.id.is_empty() && !all.iter().any(|c| c.id == commit.id) {
        all.push(commit);
    }
}

fn mappable(commit: &Commit) -> Option<String> {
    (!commit.id.is_empty()).then(|| scoped(Network::Devnull, &commit.id))
}

/// Only signed requests get a deleted commit: its id, parent_id, deleted_at and replies_count.
fn deleted(commit: &Commit) -> bool {
    commit.deleted_at.is_some() || commit.author.is_none()
}

fn conversation(id: &str, parents: &HashMap<&str, Option<&str>>) -> String {
    let mut current = id;
    for _ in 0..=parents.len() {
        match parents.get(current).copied().flatten() {
            None => break,
            Some(parent) if parents.contains_key(parent) => current = parent,
            Some(parent) => return scoped(Network::Devnull, parent),
        }
    }
    scoped(Network::Devnull, current)
}

/** One commit as a post. A deleted commit keeps its id and parent links so its replies still hang under it. */
pub fn post(commit: &Commit, conversation_id: String) -> Option<Post> {
    mappable(commit)?;
    if deleted(commit) {
        return Some(tombstone(commit, conversation_id, "This post was deleted."));
    }
    let author = commit.author.as_ref().map(user)?;
    let quoted = commit.quoted_commit.as_deref().and_then(|quoted| {
        let conversation = quoted
            .parent_id
            .as_deref()
            .unwrap_or(&quoted.id)
            .to_string();
        let conversation = scoped(Network::Devnull, &conversation);
        if deleted(quoted) {
            mappable(quoted)?;
            return Some(Box::new(tombstone(
                quoted,
                conversation,
                "The quoted post was deleted.",
            )));
        }
        post(quoted, conversation).map(Box::new)
    });
    Some(Post {
        id: scoped(Network::Devnull, &commit.id),
        net: Network::Devnull,
        url: commit.html_url.clone(),
        cid: None,
        author,
        text: commit.body.clone().unwrap_or_default(),
        created_at: commit.created_at.as_deref().and_then(timestamp),
        conversation_id,
        in_reply_to: commit
            .parent_id
            .as_deref()
            .map(|id| scoped(Network::Devnull, id)),
        quoted,
        reposted_by: None,
        metrics: Metrics {
            replies: commit.replies_count.unwrap_or(0),
            likes: commit.stars_count.unwrap_or(0),
            ..Metrics::default()
        },
        links: commit.link_preview.iter().filter_map(link).collect(),
        media: commit.images.iter().flatten().filter_map(media).collect(),
        liked: commit.starred.unwrap_or(false),
        bookmarked: false,
        reposted: false,
    })
}

fn tombstone(commit: &Commit, conversation_id: String, text: &str) -> Post {
    Post {
        id: scoped(Network::Devnull, &commit.id),
        net: Network::Devnull,
        url: None,
        cid: None,
        author: User {
            id: String::new(),
            name: "deleted".into(),
            username: "deleted".into(),
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        },
        text: text.into(),
        created_at: None,
        conversation_id,
        in_reply_to: commit
            .parent_id
            .as_deref()
            .map(|id| scoped(Network::Devnull, id)),
        quoted: None,
        reposted_by: None,
        metrics: Metrics {
            replies: commit.replies_count.unwrap_or(0),
            ..Metrics::default()
        },
        links: Vec::new(),
        media: Vec::new(),
        liked: false,
        bookmarked: false,
        reposted: false,
    }
}

fn link(preview: &LinkPreview) -> Option<Link> {
    let url = non_empty(preview.url.as_deref())?;
    Some(Link {
        short: url.to_string(),
        expanded: url.to_string(),
        display: display(url),
        title: non_empty(preview.title.as_deref()).map(str::to_string),
        description: non_empty(preview.description.as_deref()).map(str::to_string),
        image: web_url(preview.image_url.as_deref())
            .or_else(|| web_url(preview.embed.as_ref().and_then(|e| e.poster.as_deref())))
            .map(str::to_string),
        site: non_empty(preview.site_name.as_deref()).map(str::to_string),
        ..Default::default()
    })
}

fn web_url(value: Option<&str>) -> Option<&str> {
    non_empty(value).filter(|u| u.starts_with("https://") || u.starts_with("http://"))
}

/// Host and path, without the scheme, a leading `www.` or a trailing `/`.
fn display(url: &str) -> String {
    let Some(parsed) = url::Url::parse(url).ok().filter(|u| u.host_str().is_some()) else {
        return url.to_string();
    };
    let host = parsed.host_str().unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(host);
    format!("{host}{}", parsed.path().trim_end_matches('/'))
}

fn media(image: &Image) -> Option<Media> {
    let url = non_empty(image.url.as_deref())?;
    let key = match &image.id {
        Some(Value::String(id)) => id.clone(),
        Some(Value::Number(id)) => id.to_string(),
        _ => url.to_string(),
    };
    Some(Media {
        key,
        kind: MediaKind::Photo,
        url: Some(url.to_string()),
        preview_url: None,
        alt: non_empty(image.description.as_deref()).map(str::to_string),
        width: image.width,
        height: image.height,
    })
}

fn timestamp(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn non_empty(text: Option<&str>) -> Option<&str> {
    text.filter(|t| !t.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC: &str = include_str!("fixtures/pub-commits.json");

    fn commit(id: &str, parent: Option<&str>) -> Commit {
        Commit {
            id: id.into(),
            parent_id: parent.map(str::to_string),
            body: Some(format!("body {id}")),
            author: Some(Author {
                id: "u1".into(),
                username: "nuno".into(),
                ..Author::default()
            }),
            ..Commit::default()
        }
    }

    #[test]
    fn public_commits_parse_into_devnull_posts() {
        let list: List<Commit> = serde_json::from_str(PUBLIC).unwrap();
        let next = list.meta.next_cursor.clone();
        let page = page(list.data, next);
        assert_eq!(page.ids.len(), 20);
        assert_eq!(page.posts.len(), 20);
        assert!(page.next.as_deref().is_some_and(|c| c.starts_with("eyJ")));

        let first = &page.posts[0];
        assert_eq!(first.id, "devnull:01a157e5-4099-72e0-bbf2-4591a526171c");
        assert_eq!(page.ids[0], first.id);
        assert_eq!(first.net, Network::Devnull);
        assert_eq!(
            first.url.as_deref(),
            Some("https://dev-null.sh/@nunomaduro/commits/01a157e5-4099-72e0-bbf2-4591a526171c")
        );
        assert!(first.text.starts_with("ok devs, assemble"));
        assert_eq!(first.conversation_id, first.id);
        assert_eq!(first.in_reply_to, None);
        assert_eq!(first.metrics.likes, 93);
        assert_eq!(first.metrics.replies, 30);
        assert_eq!(first.metrics.reposts + first.metrics.views, 0);
        assert_eq!(
            first.created_at.map(|t| t.to_rfc3339()).as_deref(),
            Some("2026-10-20T06:00:04+00:00")
        );
        assert_eq!(
            first.author.id,
            "devnull:01a13fa8-6cd4-70ef-b8e5-f46a3ca70ca8"
        );
        assert_eq!(first.author.username, "nunomaduro");
        assert_eq!(first.author.name, "nunomaduro");
        assert!(first.author.avatar_url.is_some());
        assert!(!first.liked && !first.bookmarked);
        assert_eq!(first.links.len(), 1);
        assert_eq!(
            first.links[0].expanded,
            "https://www.producthunt.com/products/dev-null"
        );
        assert_eq!(first.links[0].short, first.links[0].expanded);
        assert_eq!(first.links[0].display, "producthunt.com/products/dev-null");
        assert_eq!(first.links[0].title, None);

        let with_image = page
            .posts
            .iter()
            .find(|p| p.id.ends_with("01a15031-c667-7017-bb74-e9e48f19dc53"))
            .unwrap();
        assert_eq!(with_image.media.len(), 1);
        assert_eq!(with_image.media[0].width, Some(1600));
        assert_eq!(with_image.media[0].height, Some(900));
        assert_eq!(with_image.media[0].kind, MediaKind::Photo);
        assert_eq!(
            with_image.media[0].url.as_deref(),
            Some("https://dev-null.sh/commit-images/01a15031-c9c1-7161-b9e5-69ad80ba5bad")
        );
        let titled = page
            .posts
            .iter()
            .flat_map(|p| &p.links)
            .find(|l| l.expanded == "https://dev-null.sh/brand#wallpapers")
            .unwrap();
        assert_eq!(titled.title.as_deref(), Some("Brand - /dev/null"));
        assert_eq!(titled.display, "dev-null.sh/brand");
        assert!(titled.description.is_some());
    }

    #[test]
    fn link_previews_carry_their_image_and_site() {
        let list: List<Commit> = serde_json::from_str(PUBLIC).unwrap();
        let next = list.meta.next_cursor.clone();
        let page = page(list.data, next);

        let wallpapers = page
            .posts
            .iter()
            .flat_map(|p| p.links.iter())
            .find(|l| l.expanded == "https://dev-null.sh/brand#wallpapers")
            .expect("link wallpapers");
        assert_eq!(
            wallpapers.image.as_deref(),
            Some("https://dev-null.sh/og.png")
        );
        assert_eq!(wallpapers.site.as_deref(), Some("/dev/null"));

        let youtube = page
            .posts
            .iter()
            .flat_map(|p| p.links.iter())
            .find(|l| l.expanded == "https://www.youtube.com/live/uHhhNffD40U?si=UzZ9ElC7QJHrCwr7")
            .expect("link youtube");
        assert_eq!(
            youtube.image.as_deref(),
            Some("https://i.ytimg.com/vi/uHhhNffD40U/hqdefault.jpg")
        );
        assert_eq!(youtube.site, None);

        let product_hunt = page
            .posts
            .iter()
            .flat_map(|p| p.links.iter())
            .find(|l| l.expanded == "https://www.producthunt.com/products/dev-null")
            .expect("link product hunt");
        assert_eq!(product_hunt.image, None);
        assert_eq!(product_hunt.site, None);

        let x_post = page
            .posts
            .iter()
            .flat_map(|p| p.links.iter())
            .find(|l| l.expanded == "https://x.com/enunomaduro/status/2112284321101062544?s=20")
            .expect("link x post");
        assert_eq!(
            x_post.image.as_deref(),
            Some("https://pbs.twimg.com/card_img/2111049729815564288/J3FFvNR4?format=webp&name=medium")
        );
        assert_eq!(x_post.site.as_deref(), Some("X (formerly Twitter)"));
    }

    #[test]
    fn conversations_follow_parents_inside_the_page() {
        let page = page(
            vec![
                commit("c", Some("b")),
                commit("b", Some("a")),
                commit("a", None),
                commit("x", Some("outside")),
                commit("y", Some("x")),
            ],
            None,
        );
        let conversation = |id: &str| {
            page.posts
                .iter()
                .find(|p| p.id == scoped(Network::Devnull, id))
                .map(|p| p.conversation_id.clone())
                .unwrap()
        };
        assert_eq!(conversation("a"), "devnull:a");
        assert_eq!(conversation("b"), "devnull:a");
        assert_eq!(conversation("c"), "devnull:a");
        assert_eq!(conversation("x"), "devnull:outside");
        assert_eq!(conversation("y"), "devnull:outside");
        let c = page.posts.iter().find(|p| p.id == "devnull:c").unwrap();
        assert_eq!(c.in_reply_to.as_deref(), Some("devnull:b"));
    }

    #[test]
    fn signed_commits_carry_stars_parents_and_quotes() {
        let json = r#"{"data":[{
            "id":"r1","body":"reply","parent_id":"p1","starred":true,
            "stars_count":2,"replies_count":0,
            "author":{"id":"u2","username":"ada","name":""},
            "parent":{"id":"p1","body":"root","parent_id":null,"author":{"id":"u1","username":"nuno"}},
            "quoted_commit":{"id":"q1","body":"quoted","parent_id":null,"author":{"id":"u3","username":"kal"}}
        }],"meta":{"next_cursor":null}}"#;
        let list: List<Commit> = serde_json::from_str(json).unwrap();
        let page = page(list.data, list.meta.next_cursor);
        assert_eq!(page.ids, vec!["devnull:r1"]);
        assert_eq!(page.posts.len(), 2);
        let reply = &page.posts[0];
        assert!(reply.liked && !reply.bookmarked);
        assert_eq!(reply.author.name, "ada");
        assert_eq!(reply.conversation_id, "devnull:p1");
        let quoted = reply.quoted.as_deref().unwrap();
        assert_eq!(quoted.id, "devnull:q1");
        assert_eq!(quoted.author.username, "kal");
        assert_eq!(page.posts[1].id, "devnull:p1");
        assert_eq!(page.posts[1].conversation_id, "devnull:p1");
    }

    #[test]
    fn deleted_commits_become_placeholders_that_keep_their_replies() {
        let json = r#"{"data":{"id":"c3","body":"reply","parent_id":"c2","author":{"id":"u1","username":"nuno"},
            "quoted_commit":{"id":"q1","parent_id":null,"deleted_at":"2026-10-20T06:00:00Z"}},
            "ancestors":[
                {"id":"c1","body":"root","parent_id":null,"author":{"id":"u1","username":"nuno"}},
                {"id":"c2","parent_id":"c1","deleted_at":"2026-10-20T06:00:00Z","replies_count":3}
            ]}"#;
        let shown: Shown = serde_json::from_str(json).unwrap();
        let mut chain = shown.ancestors;
        chain.push(shown.data);
        let page = page(chain, None);
        assert_eq!(page.ids, vec!["devnull:c1", "devnull:c2", "devnull:c3"]);
        let gone = &page.posts[1];
        assert_eq!(gone.text, "This post was deleted.");
        assert_eq!(gone.author.id, "");
        assert_eq!(gone.author.username, "deleted");
        assert_eq!(gone.author.name, "deleted");
        assert_eq!(gone.url, None);
        assert_eq!(gone.created_at, None);
        assert_eq!(gone.metrics.replies, 3);
        assert_eq!(gone.in_reply_to.as_deref(), Some("devnull:c1"));
        assert_eq!(gone.conversation_id, "devnull:c1");
        assert!(!gone.liked && !gone.bookmarked && !gone.reposted);
        let reply = &page.posts[2];
        assert_eq!(reply.in_reply_to.as_deref(), Some("devnull:c2"));
        assert_eq!(reply.conversation_id, "devnull:c1");
        let quoted = reply.quoted.as_deref().unwrap();
        assert_eq!(quoted.id, "devnull:q1");
        assert_eq!(quoted.text, "The quoted post was deleted.");
        assert_eq!(quoted.author.username, "deleted");
    }

    #[test]
    fn link_display_drops_the_scheme_www_and_trailing_slash() {
        assert_eq!(display("https://www.example.com/"), "example.com");
        assert_eq!(display("http://example.com/a/b/?q=1#x"), "example.com/a/b");
        assert_eq!(display("not a url"), "not a url");
    }
}
