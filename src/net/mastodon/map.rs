//! Mastodon JSON to socli's posts, users and pages.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::html;
use crate::model::{Link, Media, MediaKind, Metrics, Page, Post, User};
use crate::net::{scoped, Network};

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn web_url(value: &Value, key: &str) -> Option<String> {
    text(value, key)
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .map(str::to_string)
}

fn full_acct(acct: &str, server: &str) -> String {
    if acct.contains('@') {
        acct.to_string()
    } else {
        format!("{acct}@{server}")
    }
}

pub fn user(account: &Value, server: &str) -> Option<User> {
    let id = text(account, "id")?;
    let acct = text(account, "acct")?;
    let username = text(account, "username").unwrap_or(acct);
    let name = text(account, "display_name")
        .filter(|n| !n.is_empty())
        .unwrap_or(username);
    let description = text(account, "note")
        .map(|note| html::to_text(note, &[]).0)
        .filter(|note| !note.is_empty());
    Some(User {
        id: scoped(Network::Mastodon, id),
        name: name.to_string(),
        username: full_acct(acct, server),
        avatar_url: web_url(account, "avatar_static").or_else(|| web_url(account, "avatar")),
        verified: false,
        business: false,
        description,
    })
}

fn display(url: &str) -> String {
    let bare = url.split_once("://").map_or(url, |(_, rest)| rest);
    bare.trim_end_matches('/').to_string()
}

fn fill_from_card(link: &mut Link, card: &Value) {
    link.title = text(card, "title").map(str::to_string);
    link.description = text(card, "description")
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    link.image = web_url(card, "image");
    let size = |key: &str| {
        card.get(key)
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .and_then(|n| u32::try_from(n).ok())
    };
    link.image_width = size("width");
    link.image_height = size("height");
    link.site = text(card, "provider_name")
        .filter(|s| !s.is_empty())
        .map(str::to_string);
}

fn links_of(hrefs: &[String], card: Option<&Value>) -> Vec<Link> {
    let mut links: Vec<Link> = hrefs
        .iter()
        .map(|href| Link {
            short: href.clone(),
            expanded: href.clone(),
            display: display(href),
            ..Link::default()
        })
        .collect();
    let Some(card) = card.filter(|c| c.is_object()) else {
        return links;
    };
    let Some(card_url) = text(card, "url").filter(|u| !u.is_empty()) else {
        return links;
    };
    match links.iter_mut().find(|l| l.expanded == card_url) {
        Some(link) => fill_from_card(link, card),
        None => {
            let mut link = Link {
                short: card_url.to_string(),
                expanded: card_url.to_string(),
                display: display(card_url),
                ..Link::default()
            };
            fill_from_card(&mut link, card);
            links.push(link);
        }
    }
    links
}

fn media_of(attachment: &Value) -> Media {
    let kind = match text(attachment, "type") {
        Some("image") => MediaKind::Photo,
        Some("gifv") => MediaKind::Gif,
        Some("video") => MediaKind::Video,
        _ => MediaKind::Other,
    };
    let size = |key: &str| {
        attachment
            .pointer(&format!("/meta/original/{key}"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    Media {
        key: text(attachment, "id").unwrap_or_default().to_string(),
        kind,
        url: text(attachment, "url").map(str::to_string),
        preview_url: text(attachment, "preview_url").map(str::to_string),
        alt: text(attachment, "description")
            .filter(|d| !d.is_empty())
            .map(str::to_string),
        width: size("width"),
        height: size("height"),
    }
}

fn poll_lines(poll: &Value) -> Vec<String> {
    poll.get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|option| {
            let title = text(option, "title").unwrap_or_default();
            match option.get("votes_count").and_then(Value::as_u64) {
                Some(votes) => format!("○ {title} · {votes}"),
                None => format!("○ {title}"),
            }
        })
        .collect()
}

pub fn post(status: &Value, server: &str) -> Option<Post> {
    if let Some(inner) = status.get("reblog").filter(|r| r.is_object()) {
        let mut post = post(inner, server)?;
        let booster = status.pointer("/account/acct").and_then(Value::as_str);
        post.reposted_by = booster.map(|acct| full_acct(acct, server));
        return Some(post);
    }
    let id = scoped(Network::Mastodon, text(status, "id")?);
    let author = user(status.get("account")?, server)?;
    let mentions: Vec<(String, String)> = status
        .get("mentions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some((
                text(m, "url")?.to_string(),
                full_acct(text(m, "acct")?, server),
            ))
        })
        .collect();
    let (mut body, mut hrefs) =
        html::to_text(text(status, "content").unwrap_or_default(), &mentions);

    let quoted_status = status
        .get("quote")
        .filter(|q| text(q, "state") == Some("accepted"))
        .and_then(|q| q.get("quoted_status"))
        .filter(|q| q.is_object());
    if let Some(quoted) = quoted_status {
        let own: Vec<&str> = ["url", "uri"]
            .iter()
            .filter_map(|k| text(quoted, k))
            .collect();
        hrefs.retain(|href| !own.contains(&href.as_str()));
        let (head, last) = body.rsplit_once("\n\n").unwrap_or(("", body.as_str()));
        if own.contains(&last) {
            body = head.trim_end().to_string();
        }
    }
    if let Some(spoiler) = text(status, "spoiler_text").filter(|s| !s.is_empty()) {
        body = if body.is_empty() {
            format!("CW: {spoiler}")
        } else {
            format!("CW: {spoiler}\n\n{body}")
        };
    }
    if let Some(poll) = status.get("poll").filter(|p| p.is_object()) {
        let lines = poll_lines(poll).join("\n");
        if !lines.is_empty() {
            body = if body.is_empty() {
                lines
            } else {
                format!("{body}\n\n{lines}")
            };
        }
    }

    Some(Post {
        net: Network::Mastodon,
        url: text(status, "url")
            .or_else(|| text(status, "uri"))
            .map(str::to_string),
        cid: None,
        author,
        text: body,
        created_at: text(status, "created_at")
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc)),
        conversation_id: id.clone(),
        in_reply_to: text(status, "in_reply_to_id").map(|p| scoped(Network::Mastodon, p)),
        quoted: quoted_status.and_then(|q| post(q, server)).map(Box::new),
        reposted_by: None,
        metrics: Metrics {
            replies: count(status, "replies_count"),
            reposts: count(status, "reblogs_count"),
            likes: count(status, "favourites_count"),
            quotes: count(status, "quotes_count"),
            bookmarks: 0,
            views: 0,
        },
        links: links_of(&hrefs, status.get("card")),
        media: status
            .get("media_attachments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(media_of)
            .collect(),
        liked: flag(status, "favourited"),
        bookmarked: flag(status, "bookmarked"),
        reposted: flag(status, "reblogged"),
        id,
    })
}

pub fn statuses_page(statuses: &[Value], next: Option<String>, server: &str) -> Page {
    let mapped: Vec<Post> = statuses.iter().filter_map(|s| post(s, server)).collect();
    let ids = mapped.iter().map(|p| p.id.clone()).collect();
    let mut posts: Vec<Post> = Vec::new();
    for post in mapped {
        if !posts.iter().any(|p| p.id == post.id) {
            posts.push(post);
        }
    }
    let parents: HashMap<String, Option<String>> = posts
        .iter()
        .map(|p| (p.id.clone(), p.in_reply_to.clone()))
        .collect();
    for post in &mut posts {
        let mut top = post.id.clone();
        for _ in 0..=parents.len() {
            match parents.get(&top).and_then(Option::as_ref) {
                Some(parent) if parents.contains_key(parent) => top = parent.clone(),
                _ => break,
            }
        }
        post.conversation_id = match parents.get(&top).and_then(Option::as_ref) {
            Some(outside) => outside.clone(),
            None => top,
        };
    }
    Page { ids, posts, next }
}

pub fn notifications_page(notifications: &[Value], next: Option<String>, server: &str) -> Page {
    let statuses: Vec<Value> = notifications
        .iter()
        .filter_map(|n| n.get("status").filter(|s| s.is_object()).cloned())
        .collect();
    statuses_page(&statuses, next, server)
}

pub fn thread_page(status: &Value, context: &Value, server: &str) -> Option<Page> {
    let list = |key: &str| -> Vec<Post> {
        context
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| post(s, server))
            .collect()
    };
    let focus = post(status, server)?;
    let mut posts = list("ancestors");
    posts.push(focus);
    posts.extend(list("descendants"));
    let root = posts.first()?.id.clone();
    for post in &mut posts {
        post.conversation_id = root.clone();
    }
    Some(Page {
        ids: posts.iter().map(|p| p.id.clone()).collect(),
        posts,
        next: None,
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: &str = "mastodon.social";

    #[test]
    fn a_home_page_maps_links_cards_media_boosts_polls_and_quotes() {
        let home: Vec<Value> = serde_json::from_str(include_str!("fixtures/home.json")).unwrap();
        let page = statuses_page(&home, Some("max_id=1".into()), SERVER);
        assert_eq!(
            page.ids,
            vec![
                "mastodon:115000000000000001",
                "mastodon:115000000000000002",
                "mastodon:115000000000000005"
            ]
        );
        let first = &page.posts[0];
        assert_eq!(first.author.username, "ines@mastodon.social");
        assert_eq!(first.author.name, "Ines Duarte");
        assert_eq!(
            first.text,
            "Reading the fediverse from a terminal feels right. @theo@fosstodon.org wrote up how boosts & quotes travel between servers:\n\nhttps://blog.example.org/2026/10/terminal-clients-and-the-fediverse/\n\n#Mastodon #CLI"
        );
        assert_eq!(
            first.url.as_deref(),
            Some("https://mastodon.social/@ines/115000000000000001")
        );
        assert_eq!(first.metrics.replies, 3);
        assert_eq!(first.metrics.reposts, 7);
        assert_eq!(first.metrics.likes, 21);
        assert_eq!(first.metrics.quotes, 1);
        assert!(first.liked && !first.bookmarked);
        assert_eq!(first.links.len(), 1);
        let link = &first.links[0];
        assert_eq!(
            link.display,
            "blog.example.org/2026/10/terminal-clients-and-the-fediverse"
        );
        assert_eq!(
            link.title.as_deref(),
            Some("Terminal clients and the fediverse")
        );
        assert_eq!(link.site.as_deref(), Some("Example Blog"));
        assert_eq!(
            link.image.as_deref(),
            Some("https://files.example/cache/preview_cards/images/a1.webp")
        );
        assert_eq!(
            (link.image_width, link.image_height),
            (Some(1200), Some(630))
        );
        assert_eq!(first.media.len(), 1);
        assert_eq!(first.media[0].kind, MediaKind::Photo);
        assert_eq!(
            first.media[0].alt.as_deref(),
            Some("A terminal window showing a thread with three replies")
        );
        assert_eq!(first.media[0].width, Some(1200));

        let boost = &page.posts[1];
        assert_eq!(boost.reposted_by.as_deref(), Some("maren@mastodon.social"));
        assert_eq!(boost.author.username, "theo@fosstodon.org");
        assert_eq!(
            boost.text,
            "CW: poll about feeds\n\nWhich feed do you open first?\n\n○ Home · 11\n○ Trending · 4"
        );

        let quote = &page.posts[2];
        assert_eq!(quote.text, "Finally, one client for all of them.");
        assert!(quote.links.is_empty());
        assert!(quote.bookmarked);
        let quoted = quote.quoted.as_ref().expect("the accepted quote");
        assert_eq!(quoted.author.username, "socli_news@hachyderm.io");
        assert_eq!(
            quoted.text,
            "socli now reads Mastodon.\nBoosts, quotes and polls included."
        );
        assert_eq!(page.next.as_deref(), Some("max_id=1"));
    }

    #[test]
    fn a_thread_runs_from_the_root_through_the_replies() {
        let status: Value = serde_json::from_str(include_str!("fixtures/status.json")).unwrap();
        let context: Value = serde_json::from_str(include_str!("fixtures/context.json")).unwrap();
        let page = thread_page(&status, &context, SERVER).unwrap();
        assert_eq!(
            page.ids,
            vec![
                "mastodon:115000000000000010",
                "mastodon:115000000000000011",
                "mastodon:115000000000000012"
            ]
        );
        assert!(page
            .posts
            .iter()
            .all(|p| p.conversation_id == "mastodon:115000000000000010"));
        assert_eq!(
            page.posts[1].in_reply_to.as_deref(),
            Some("mastodon:115000000000000010")
        );
        assert_eq!(
            page.posts[1].text,
            "@theo@fosstodon.org the reply in the middle."
        );
        assert_eq!(page.posts[2].author.name, "maren");
    }

    #[test]
    fn feed_conversations_climb_to_the_topmost_parent_in_the_page() {
        let context: Value = serde_json::from_str(include_str!("fixtures/context.json")).unwrap();
        let status: Value = serde_json::from_str(include_str!("fixtures/status.json")).unwrap();
        let list = vec![status, context["descendants"][0].clone()];
        let page = statuses_page(&list, None, SERVER);
        assert!(page
            .posts
            .iter()
            .all(|p| p.conversation_id == "mastodon:115000000000000010"));
    }

    #[test]
    fn profiles_notifications_and_search_map() {
        let account: Value = serde_json::from_str(include_str!("fixtures/account.json")).unwrap();
        let user = user(&account, SERVER).unwrap();
        assert_eq!(user.id, "mastodon:109001");
        assert_eq!(user.username, "ines@mastodon.social");
        assert_eq!(
            user.description.as_deref(),
            Some("Terminal tools & small type. Lisbon.")
        );
        assert_eq!(
            user.avatar_url.as_deref(),
            Some("https://files.example/avatars/109001.png")
        );
        let notifications: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/notifications.json")).unwrap();
        let page = notifications_page(&notifications, None, SERVER);
        assert_eq!(
            page.ids,
            vec!["mastodon:115000000000000011", "mastodon:115000000000000001"]
        );
        let search: Value = serde_json::from_str(include_str!("fixtures/search.json")).unwrap();
        let found = statuses_page(search["statuses"].as_array().unwrap(), None, SERVER);
        assert_eq!(found.ids, vec!["mastodon:115000000000000004"]);
    }
}
