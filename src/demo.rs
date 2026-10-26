//! Offline feeds on all five networks, so the reader is usable before any account exists.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};

use crate::model::{FeedId, Link, Media, MediaKind, Metrics, Post, User};
use crate::net::{scoped, FeedKind, Network};

pub struct DemoWorld {
    pub posts: HashMap<String, Post>,
    home: Vec<String>,
    mentions: Vec<String>,
    bookmarks: Vec<String>,
}

impl DemoWorld {
    #[cfg(test)]
    pub fn posts_for_feed(&self, id: &FeedId) -> Vec<Post> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in self.feed_ids(id) {
            push_with_ancestors(&id, &self.posts, &mut seen, &mut out);
        }
        out
    }

    /// What a demo feed lists. Every network shares the home, mention and bookmark lists.
    pub fn feed_ids(&self, id: &FeedId) -> Vec<String> {
        let on = |ids: &[String], keep: &dyn Fn(Network) -> bool| -> Vec<String> {
            ids.iter()
                .filter(|id| self.posts.get(*id).is_some_and(|p| keep(p.net)))
                .cloned()
                .collect()
        };
        match id {
            FeedId::Merged => self.home.clone(),
            FeedId::Kind(FeedKind::Bookmarks) => self.bookmarks.clone(),
            FeedId::Kind(kind) => on(&self.home, &|net| {
                net.caps().resolve_feed(net, *kind).is_some()
            }),
            FeedId::Feed(net, FeedKind::Bookmarks) => on(&self.bookmarks, &|n| n == *net),
            FeedId::Feed(net, _) => on(&self.home, &|n| n == *net),
            FeedId::Mentions(scope) => on(&self.mentions, &|n| scope.includes(n)),
            FeedId::Search(..) | FeedId::User(..) | FeedId::Thread(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
fn push_with_ancestors(
    id: &str,
    posts: &HashMap<String, Post>,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<Post>,
) {
    let Some(post) = posts.get(id) else {
        return;
    };
    if let Some(parent) = &post.in_reply_to {
        if posts.contains_key(parent) {
            push_with_ancestors(parent, posts, seen, out);
        }
    }
    if seen.insert(id.to_string()) {
        out.push(post.clone());
    }
}

/// The signed-in handle the demo pretends to have on each network.
pub fn you(net: Network) -> User {
    let username = match net {
        Network::Bluesky => "you.bsky.social",
        Network::Mastodon => "you@mastodon.social",
        _ => "you",
    };
    User {
        id: scoped(net, "u0"),
        name: "You".into(),
        username: username.into(),
        avatar_url: Some(format!("demo://avatar/{username}")),
        verified: false,
        business: false,
        description: Some("The account this demo pretends you are.".into()),
    }
}

/// A plausible permalink on the network's real website.
pub fn permalink(net: Network, username: &str, native: &str) -> String {
    match net {
        Network::X => format!("https://x.com/{username}/status/{native}"),
        Network::Bluesky => format!("https://bsky.app/profile/{username}/post/{native}"),
        Network::Devnull => format!("https://dev-null.sh/@{username}/commits/{native}"),
        Network::Threads => format!("https://www.threads.com/@{username}/post/{native}"),
        Network::Mastodon => {
            let (user, host) = username
                .split_once('@')
                .unwrap_or((username, "mastodon.social"));
            format!("https://{host}/@{user}/{native}")
        }
    }
}

pub fn world(now: DateTime<Utc>) -> DemoWorld {
    use Network::{Bluesky, Devnull, Mastodon, Threads, X};
    let mina = user(X, "u1", "Mina Cho", "mina", "Draws threads for a living.");
    let ines = user(
        Mastodon,
        "u20",
        "Ines Duarte",
        "ines@mastodon.social",
        "Terminal tools and small type. Lisbon.",
    );
    let theo = user(Mastodon, "u21", "Theo Lang", "theo@fosstodon.org", "");
    let nuno = user(Devnull, "u2", "Nuno Park", "nuno", "Shipping the reader.");
    let priya = user(Devnull, "u3", "Priya Shah", "priya", "");
    let juri = user(Devnull, "u4", "Juri Holm", "juri", "");
    let diego = user(Devnull, "u5", "Diego Martins", "diego", "");
    let sam = user(Devnull, "u6", "Sam Idris", "sam", "");
    let lea = user(Devnull, "u7", "Lea Ortega", "lea", "");
    let ada = user(
        Bluesky,
        "u8",
        "Ada Okonkwo",
        "ada.bsky.social",
        "Charts, but only the honest kind.",
    );
    let mina_sky = user(Bluesky, "u1", "Mina Cho", "mina.bsky.social", "");
    let river = user(
        X, "u9", "River Hale", "river",
        "Exploring terminal graphics and small interfaces.\nMaking the most of every character cell.",
    );
    let moss = User {
        business: true,
        ..user(X, "u10", "Moss Adler", "moss", "")
    };
    let kal = user(Threads, "u11", "Kal Nguyen", "kal", "");

    let mut posts = HashMap::new();
    let mut add = |post: Post| {
        posts.insert(post.id.clone(), post);
    };

    add(post(
        "p1",
        &mina,
        "Step 1: draw the thread in the terminal.\nStep 2: leave the scrollback alone.\n\n… @nuno if you know what I mean",
        now - Duration::hours(3),
        "c1",
        None,
        Metrics {
            replies: 6,
            reposts: 4,
            likes: 48,
            quotes: 1,
            bookmarks: 7,
            views: 2140,
        },
        vec![],
        vec![],
    ));
    add(post(
        "p2",
        &nuno,
        "ok devs, assemble. the reader is up.\n\nnotes: https://example.com/socli\n\nmake some noise.",
        now - Duration::minutes(50),
        "c9",
        None,
        Metrics {
            replies: 5,
            reposts: 0,
            likes: 86,
            quotes: 0,
            bookmarks: 3,
            views: 0,
        },
        vec![Link {
            short: "https://example.com/socli".into(),
            expanded: "https://example.com/socli".into(),
            display: "example.com/socli".into(),
            title: Some("SoCLI".into()),
            description: Some(
                "A thread-first terminal client for X, Bluesky, dev/null, Threads and Mastodon"
                    .into(),
            ),
            image: Some("demo://og-wide".into()),
            image_width: Some(1200),
            image_height: Some(630),
            ..Default::default()
        }],
        vec![],
    ));
    /* The text above already contains the expanded URL; links are kept so a
    live-shaped post still round-trips through the same model. */
    add(reply(
        "p3",
        &priya,
        "Hype",
        now - Duration::minutes(40),
        "c9",
        "p2",
        4,
    ));
    add(reply(
        "p4",
        &juri,
        "The intro thread is the whole product.",
        now - Duration::minutes(28),
        "c9",
        "p2",
        9,
    ));
    add(reply(
        "p5",
        &diego,
        "Those were the fastest 18 hours of the build.",
        now - Duration::minutes(20),
        "c9",
        "p2",
        15,
    ));
    add(reply(
        "p6",
        &sam,
        "Image next. Kitty, iTerm and sixel terminals draw it; the rest get half blocks.",
        now - Duration::minutes(12),
        "c9",
        "p2",
        6,
    ));
    add(reply(
        "p7",
        &lea,
        "The rails are the interface.",
        now - Duration::minutes(7),
        "c9",
        "p2",
        3,
    ));
    add(Post {
        media: vec![Media {
            key: "demo-spark".into(),
            kind: MediaKind::Photo,
            url: Some("demo://spark".into()),
            preview_url: None,
            alt: Some("A rising sparkline on a black field.".into()),
            width: Some(360),
            height: Some(180),
        }],
        ..post(
            "p8",
            &ada,
            "shipped a sparkline so the reader has something to look at.\n\npress v.",
            now - Duration::hours(5),
            "c2",
            None,
            Metrics {
                replies: 1,
                reposts: 2,
                likes: 21,
                quotes: 0,
                bookmarks: 4,
                views: 0,
            },
            vec![],
            vec![],
        )
    });
    add(reply(
        "p9",
        &mina_sky,
        "v opens it full screen. without image support, the same library paints half blocks.",
        now - Duration::hours(4),
        "c2",
        "p8",
        2,
    ));
    add(post(
        "p10",
        &kal,
        "Reading this in a terminal on purpose. notes: https://example.org/terminal-notes",
        now - Duration::hours(8),
        "c3",
        None,
        Metrics::default(),
        vec![Link {
            short: "https://example.org/terminal-notes".into(),
            expanded: "https://example.org/terminal-notes".into(),
            display: "example.org/terminal-notes".into(),
            title: Some("Terminal notes".into()),
            description: Some("Why a feed reads better as threads in eighty columns.".into()),
            image: Some("demo://og-square".into()),
            image_width: Some(150),
            image_height: Some(150),
            site: None,
        }],
        vec![],
    ));

    let river_media = vec![
        Media {
            key: "demo-wave".into(),
            kind: MediaKind::Photo,
            url: Some("demo://wave".into()),
            preview_url: None,
            alt: Some("Sparkline drawn with half blocks in an 80-column terminal".into()),
            width: Some(384),
            height: Some(296),
        },
        Media {
            key: "demo-spark".into(),
            kind: MediaKind::Photo,
            url: Some("demo://spark".into()),
            preview_url: None,
            alt: Some("A rising sparkline on a black field.".into()),
            width: Some(360),
            height: Some(180),
        },
    ];
    let mut river_post = post(
        "l1",
        &river,
        "Half-block graphics still look like magic in an 80-column terminal.",
        now - Duration::hours(2),
        "c4",
        None,
        Metrics {
            replies: 2,
            reposts: 6,
            likes: 40,
            quotes: 1,
            bookmarks: 5,
            views: 3900,
        },
        vec![],
        vec![],
    );
    river_post.media = river_media.clone();
    add(river_post.clone());
    let original_url = river_post.url.clone();
    let mut quote = post(
        "l2",
        &moss,
        "This is the whole argument for drawing the feed instead of dumping JSON.",
        now - Duration::minutes(70),
        "c5",
        None,
        Metrics {
            replies: 0,
            reposts: 1,
            likes: 8,
            quotes: 0,
            bookmarks: 1,
            views: 870,
        },
        vec![],
        vec![],
    );
    quote.quoted = Some(Box::new(river_post));
    add(quote);
    let mut repost = post(
        "l3",
        &river,
        "Half-block graphics still look like magic in an 80-column terminal.",
        now - Duration::minutes(30),
        "c4",
        None,
        Metrics {
            replies: 2,
            reposts: 7,
            likes: 40,
            quotes: 1,
            bookmarks: 5,
            views: 3950,
        },
        vec![],
        vec![],
    );
    repost.url = original_url;
    repost.reposted_by = Some("moss".into());
    repost.media = river_media;
    add(repost);
    add(post(
        "l4",
        &kal,
        "Rate limits are why this client does not page until you ask.",
        now - Duration::minutes(15),
        "c6",
        None,
        Metrics::default(),
        vec![],
        vec![],
    ));

    add(post(
        "m1",
        &nuno,
        "@you the prompt at the bottom is the whole navigation model.",
        now - Duration::minutes(25),
        "c7",
        None,
        Metrics {
            likes: 2,
            ..Metrics::default()
        },
        vec![],
        vec![],
    ));
    add(post(
        "m2",
        &ada,
        "@you.bsky.social press v on the sparkline when you want the picture, not before.",
        now - Duration::hours(1),
        "c8",
        None,
        Metrics {
            likes: 1,
            ..Metrics::default()
        },
        vec![],
        vec![],
    ));

    add(post(
        "p11",
        &ines,
        "Boosts show who shared a post above it, the way the web app does. @theo@fosstodon.org was right about full handles.",
        now - Duration::minutes(40),
        "p11",
        None,
        Metrics {
            replies: 1,
            reposts: 3,
            likes: 12,
            quotes: 0,
            bookmarks: 2,
            views: 0,
        },
        vec![],
        vec![],
    ));
    add(reply(
        "r11",
        &theo,
        "Full handles look long, but they tell you which server answers.",
        now - Duration::minutes(32),
        "p11",
        "p11",
        2,
    ));

    let ids = |list: &[(Network, &str)]| -> Vec<String> {
        list.iter().map(|(net, id)| scoped(*net, id)).collect()
    };
    DemoWorld {
        posts,
        home: ids(&[
            (X, "p1"),
            (Devnull, "p2"),
            (Devnull, "p3"),
            (Devnull, "p4"),
            (Devnull, "p5"),
            (Devnull, "p6"),
            (Devnull, "p7"),
            (Bluesky, "p8"),
            (Bluesky, "p9"),
            (Threads, "p10"),
            (X, "l1"),
            (X, "l2"),
            (X, "l3"),
            (Threads, "l4"),
            (Devnull, "m1"),
            (Bluesky, "m2"),
            (Mastodon, "p11"),
        ]),
        mentions: ids(&[(Devnull, "m1"), (Bluesky, "m2")]),
        bookmarks: ids(&[(X, "p1"), (Bluesky, "p8"), (Devnull, "p2")]),
    }
}

fn user(net: Network, id: &str, name: &str, username: &str, description: &str) -> User {
    User {
        id: scoped(net, id),
        name: name.into(),
        username: username.into(),
        avatar_url: Some(format!("demo://avatar/{username}")),
        verified: matches!(username, "nuno" | "ada.bsky.social"),
        business: false,
        description: if description.is_empty() {
            None
        } else {
            Some(description.into())
        },
    }
}

fn reply(
    id: &str,
    author: &User,
    text: &str,
    at: DateTime<Utc>,
    conversation: &str,
    parent: &str,
    likes: u64,
) -> Post {
    post(
        id,
        author,
        text,
        at,
        conversation,
        Some(parent),
        Metrics {
            replies: 0,
            reposts: 0,
            likes,
            quotes: 0,
            bookmarks: 0,
            views: 120 + likes * 40,
        },
        vec![],
        vec![],
    )
}

/// A demo post on the author's network; `id`, `conversation` and `parent` are native.
#[allow(clippy::too_many_arguments)]
fn post(
    id: &str,
    author: &User,
    text: &str,
    at: DateTime<Utc>,
    conversation: &str,
    parent: Option<&str>,
    metrics: Metrics,
    links: Vec<Link>,
    media: Vec<Media>,
) -> Post {
    let net = crate::net::split(&author.id).map_or(Network::X, |(net, _)| net);
    let caps = net.caps();
    Post {
        id: scoped(net, id),
        net,
        url: Some(permalink(net, &author.username, id)),
        cid: (net == Network::Bluesky).then(|| format!("bafyreidemo{id}")),
        author: author.clone(),
        text: text.into(),
        created_at: Some(at),
        conversation_id: scoped(net, conversation),
        in_reply_to: parent.map(|p| scoped(net, p)),
        quoted: None,
        reposted_by: None,
        metrics: Metrics {
            views: if caps.views { metrics.views } else { 0 },
            bookmarks: if caps.bookmark { metrics.bookmarks } else { 0 },
            ..metrics
        },
        links,
        media,
        liked: false,
        bookmarked: false,
        reposted: false,
    }
}
