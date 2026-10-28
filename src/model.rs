//! Posts, people, and feeds as the reader sees them.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::net::{FeedKind, Network};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub name: String,
    pub username: String,
    pub avatar_url: Option<String>,
    pub verified: bool,
    /// An X Verified Organization (`verified_type` business); the rich level draws its picture as a rounded square.
    pub business: bool,
    pub description: Option<String>,
}

impl User {
    pub fn badge(&self) -> char {
        self.name
            .chars()
            .find(|c| c.is_ascii_alphanumeric())
            .or_else(|| self.username.chars().find(|c| c.is_ascii_alphanumeric()))
            .unwrap_or('?')
            .to_ascii_uppercase()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Photo,
    Video,
    Gif,
    Other,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Photo => "image",
            MediaKind::Video => "video",
            MediaKind::Gif => "gif",
            MediaKind::Other => "media",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    pub key: String,
    pub kind: MediaKind,
    pub url: Option<String>,
    pub preview_url: Option<String>,
    pub alt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl Media {
    /// Still image the viewer can paint. Video files are not decoded here.
    pub fn view_url(&self) -> Option<&str> {
        match self.kind {
            MediaKind::Photo | MediaKind::Gif => {
                self.url.as_deref().or(self.preview_url.as_deref())
            }
            MediaKind::Video | MediaKind::Other => {
                self.preview_url.as_deref().or(self.url.as_deref())
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub short: String,
    pub expanded: String,
    pub display: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The page's preview image (og:image or the network's card thumbnail).
    pub image: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    /// The site name the page gives (og:site_name), shown instead of the domain when present.
    pub site: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metrics {
    pub replies: u64,
    pub reposts: u64,
    pub likes: u64,
    pub quotes: u64,
    pub bookmarks: u64,
    pub views: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Post {
    /// Scoped: `<slug>:<native id>`.
    pub id: String,
    pub net: Network,
    /// The permalink on the network's website.
    pub url: Option<String>,
    /// Bluesky content id; None elsewhere.
    pub cid: Option<String>,
    pub author: User,
    pub text: String,
    pub created_at: Option<DateTime<Utc>>,
    pub conversation_id: String,
    pub in_reply_to: Option<String>,
    pub quoted: Option<Box<Post>>,
    pub reposted_by: Option<String>,
    pub metrics: Metrics,
    pub links: Vec<Link>,
    pub media: Vec<Media>,
    pub liked: bool,
    pub bookmarked: bool,
    pub reposted: bool,
}

impl Post {
    pub fn permalink(&self) -> String {
        self.url.clone().unwrap_or_default()
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "id": self.id,
            "network": self.net.slug(),
            "url": self.url,
            "permalink": self.url,
            "conversation_id": self.conversation_id,
            "in_reply_to": self.in_reply_to,
            "reposted_by": self.reposted_by,
            "created_at": self.created_at.map(|t| t.to_rfc3339()),
            "text": self.text,
            "author": {
                "id": self.author.id,
                "name": self.author.name,
                "username": self.author.username,
                "verified": self.author.verified,
                "avatar": self.author.avatar_url,
                "description": self.author.description,
            },
            "metrics": {
                "replies": self.metrics.replies,
                "reposts": self.metrics.reposts,
                "likes": self.metrics.likes,
                "quotes": self.metrics.quotes,
                "bookmarks": self.metrics.bookmarks,
                "views": self.metrics.views,
            },
            "liked": self.liked,
            "bookmarked": self.bookmarked,
            "reposted": self.reposted,
            "links": self.links.iter().map(|l| json!({
                "short": l.short,
                "expanded": l.expanded,
                "display": l.display,
                "image": l.image,
                "site": l.site,
            })).collect::<Vec<_>>(),
            "media": self.media.iter().map(|m| json!({
                "key": m.key,
                "type": m.kind.label(),
                "url": m.url,
                "preview": m.preview_url,
                "alt": m.alt,
                "width": m.width,
                "height": m.height,
            })).collect::<Vec<_>>(),
            "quoted": self.quoted.as_ref().map(|q| q.to_json()),
        });
        if let (Some(cid), Some(map)) = (&self.cid, value.as_object_mut()) {
            map.insert("cid".into(), json!(cid));
        }
        value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// Timeline items, in the order the API returned them.
    pub ids: Vec<String>,
    /// Those items plus expanded parents, quotes, and repost originals.
    pub posts: Vec<Post>,
    pub next: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Home,
    Notifications,
    Search,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Home => "Home",
            Section::Notifications => "Notifications",
            Section::Search => "Search",
        }
    }

    pub fn cycle(self, dir: i32) -> Self {
        let all = [Section::Home, Section::Notifications, Section::Search];
        let i = all.iter().position(|s| *s == self).unwrap_or(0) as i32;
        let n = all.len() as i32;
        all[((i + dir.rem_euclid(n)) % n) as usize]
    }
}

/// Which networks the reader shows: all of them merged, or one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    All,
    One(Network),
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::All => "all",
            Scope::One(net) => net.label(),
        }
    }

    pub fn includes(self, net: Network) -> bool {
        match self {
            Scope::All => true,
            Scope::One(one) => one == net,
        }
    }
}

/// What the reader is currently listing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FeedId {
    /// The All timeline: every home network's primary feed.
    Merged,
    /** One feed kind from every signed-in network that has it (`socli feed saved` in All scope). */
    Kind(FeedKind),
    Feed(Network, FeedKind),
    Mentions(Scope),
    Search(Scope, String),
    /// One account's posts; the demo lists a handle from every network in All scope.
    User(Scope, String),
    Thread(String),
}

impl FeedId {
    /// Feeds that merge every network that is on, so they follow the on-set.
    pub fn merged(&self) -> bool {
        matches!(
            self,
            FeedId::Merged
                | FeedId::Kind(_)
                | FeedId::Mentions(Scope::All)
                | FeedId::Search(Scope::All, _)
                | FeedId::User(Scope::All, _)
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct Feed {
    pub ids: Vec<String>,
    /// The next-page cursor of every network that has one.
    pub next: BTreeMap<Network, String>,
    /**
    Ids added by the latest fetch. Empty means the whole list is that page, which is the demo and
    any feed that has not been fetched yet. The reader prefetches from the 10th of these.
    */
    pub page_ids: Vec<String>,
    pub notice: Option<String>,
    pub ready: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_cycle_accepts_the_full_direction_range() {
        for section in [Section::Home, Section::Notifications, Section::Search] {
            for direction in [i32::MIN, -3, -1, 0, 1, 3, i32::MAX] {
                assert_eq!(
                    section.cycle(direction),
                    section.cycle(direction.rem_euclid(3))
                );
            }
        }
    }

    #[test]
    fn post_json_lists_link_images() {
        let post = Post {
            id: "x:1".into(),
            net: Network::X,
            url: None,
            cid: None,
            author: User {
                id: "u1".into(),
                name: "Example".into(),
                username: "example".into(),
                avatar_url: None,
                verified: false,
                business: false,
                description: None,
            },
            text: "hello".into(),
            created_at: None,
            conversation_id: "c1".into(),
            in_reply_to: None,
            quoted: None,
            reposted_by: None,
            metrics: Metrics::default(),
            links: vec![Link {
                short: "https://t.co/a".into(),
                expanded: "https://example.com/a".into(),
                display: "example.com/a".into(),
                image: Some("https://example.com/og.png".into()),
                site: Some("Example".into()),
                ..Default::default()
            }],
            media: Vec::new(),
            liked: false,
            bookmarked: false,
            reposted: false,
        };
        let json = post.to_json();
        assert_eq!(json["links"][0]["image"], "https://example.com/og.png");
        assert_eq!(json["links"][0]["site"], "Example");
    }
}
