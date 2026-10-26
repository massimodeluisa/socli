//! What each network can do, and how it counts characters. No client and no network.

#[cfg(feature = "native")]
use std::sync::atomic::{AtomicUsize, Ordering};

use unicode_segmentation::UnicodeSegmentation;

use super::{Caps, FeedKind, NetError};

pub const NO_TRENDING: &str =
    "X has no trending feed in SoCLI yet. Try `socli search <words> --net x`";

pub const MASTODON_DEFAULT_SERVER: &str = "mastodon.social";
const MASTODON_DEFAULT_MAX: usize = 500;
/// Each link counts as 23 characters, whatever its length.
const MASTODON_URL_LENGTH: usize = 23;

/// The signed-in server's limit. `Network::caps` has no config, so the client publishes it here.
#[cfg(feature = "native")]
static MASTODON_MAX: AtomicUsize = AtomicUsize::new(MASTODON_DEFAULT_MAX);

pub fn mastodon_limit() -> usize {
    #[cfg(feature = "native")]
    {
        MASTODON_MAX.load(Ordering::Relaxed)
    }
    #[cfg(not(feature = "native"))]
    MASTODON_DEFAULT_MAX
}

pub fn set_mastodon_limit(max: usize) {
    #[cfg(feature = "native")]
    MASTODON_MAX.store(max, Ordering::Relaxed);
    #[cfg(not(feature = "native"))]
    let _ = max;
}

pub fn x(user: bool) -> Caps {
    if !user {
        return Caps {
            search: true,
            views: true,
            ..Caps::public(280)
        };
    }
    Caps {
        feeds: vec![
            (FeedKind::ForYou, "For you"),
            (FeedKind::Following, "Following"),
            (FeedKind::Bookmarks, "Bookmarks"),
        ],
        primary: Some(FeedKind::Following),
        mentions: true,
        search: true,
        like: true,
        repost: true,
        unrepost: true,
        quote: true,
        bookmark: true,
        follow: true,
        delete: true,
        thread: true,
        user: true,
        post: true,
        counts: true,
        views: true,
        like_verb: "like",
        max_chars: 280,
        count_graphemes: false,
    }
}

pub fn bluesky(session: bool) -> Caps {
    if !session {
        return Caps {
            count_graphemes: true,
            ..Caps::public(300)
        };
    }
    Caps {
        feeds: vec![
            (FeedKind::Following, "Following"),
            (FeedKind::Discover, "Discover"),
            (FeedKind::Bookmarks, "Bookmarks"),
        ],
        primary: Some(FeedKind::Following),
        mentions: true,
        search: true,
        like: true,
        repost: true,
        unrepost: true,
        quote: true,
        bookmark: true,
        follow: true,
        delete: true,
        thread: true,
        user: true,
        post: true,
        counts: true,
        views: false,
        like_verb: "like",
        max_chars: 300,
        count_graphemes: true,
    }
}

pub fn devnull_public() -> Caps {
    Caps::public(4000)
}

pub fn devnull_ssh() -> Caps {
    Caps {
        post: true,
        ..Caps::public(4000)
    }
}

pub fn devnull_device() -> Caps {
    Caps {
        feeds: vec![
            (FeedKind::Following, "Following"),
            (FeedKind::Latest, "Latest"),
            (FeedKind::Trending, "Trending"),
            (FeedKind::Bookmarks, "Starred"),
        ],
        primary: Some(FeedKind::Following),
        mentions: true,
        search: false,
        like: true,
        repost: false,
        unrepost: true,
        quote: true,
        bookmark: false,
        follow: true,
        delete: true,
        thread: true,
        user: true,
        post: true,
        counts: true,
        views: false,
        like_verb: "star",
        max_chars: 4000,
        count_graphemes: false,
    }
}

pub fn threads() -> Caps {
    Caps {
        feeds: vec![(FeedKind::Mine, "Your threads")],
        primary: Some(FeedKind::Mine),
        mentions: true,
        search: true,
        like: false,
        repost: true,
        unrepost: false,
        quote: true,
        bookmark: false,
        follow: false,
        delete: true,
        thread: true,
        user: true,
        post: true,
        counts: false,
        views: false,
        like_verb: "like",
        max_chars: 500,
        count_graphemes: false,
    }
}

/// Threads counts emoji and symbol characters as their UTF-8 bytes against the 500 limit.
pub fn threads_text_len(text: &str) -> usize {
    text.chars()
        .map(|c| match c {
            '\u{1F000}'..='\u{1FAFF}'
            | '\u{2300}'..='\u{23FF}'
            | '\u{2600}'..='\u{27BF}'
            | '\u{2B00}'..='\u{2BFF}'
            | '\u{FE0F}'
            | '\u{200D}' => c.len_utf8(),
            _ => 1,
        })
        .sum()
}

const THREADS_MAX_LINKS: usize = 5;

/// Unique http and https URLs in the text. Threads counts each as a link.
pub(crate) fn threads_link_count(text: &str) -> usize {
    let mut seen: Vec<&str> = Vec::new();
    for word in text.split_whitespace() {
        if (word.starts_with("http://") || word.starts_with("https://")) && !seen.contains(&word) {
            seen.push(word);
        }
    }
    seen.len()
}

/// Rejects empty text, text over the 500 limit and more than 5 links before any request.
pub fn threads_check_text(text: &str) -> Result<(), NetError> {
    if text.trim().is_empty() {
        return Err(NetError::Other(
            "a threads post needs text. write something and post again".into(),
        ));
    }
    let max = threads().max_chars;
    let used = threads_text_len(text);
    if used > max {
        return Err(NetError::Other(format!(
            "Threads posts hold {max} characters at most and this one has {used}. shorten it and post again"
        )));
    }
    let links = threads_link_count(text);
    if links > THREADS_MAX_LINKS {
        return Err(NetError::Other(format!(
            "Threads allows at most {THREADS_MAX_LINKS} links in a post. Remove {}",
            links - THREADS_MAX_LINKS
        )));
    }
    Ok(())
}

pub fn mastodon(session: bool) -> Caps {
    mastodon_with(session, mastodon_limit())
}

pub fn mastodon_with(session: bool, max_chars: usize) -> Caps {
    if !session {
        return Caps {
            count_graphemes: true,
            ..Caps::public(max_chars)
        };
    }
    Caps {
        feeds: vec![
            (FeedKind::Following, "Home"),
            (FeedKind::Trending, "Trending"),
            (FeedKind::Bookmarks, "Bookmarks"),
        ],
        primary: Some(FeedKind::Following),
        mentions: true,
        search: true,
        like: true,
        repost: true,
        unrepost: true,
        quote: true,
        bookmark: true,
        follow: true,
        delete: true,
        thread: true,
        user: true,
        post: true,
        counts: true,
        views: false,
        like_verb: "favorite",
        max_chars,
        count_graphemes: true,
    }
}

/// Characters `text` uses on Mastodon: graphemes, each link as 23, a mention as `@user`.
pub fn mastodon_text_len(text: &str) -> usize {
    text.split_inclusive(char::is_whitespace)
        .map(|piece| {
            let (token, space) = match piece.chars().last() {
                Some(last) if last.is_whitespace() => (&piece[..piece.len() - last.len_utf8()], 1),
                _ => (piece, 0),
            };
            mastodon_token_len(token) + space
        })
        .sum()
}

fn mastodon_token_len(token: &str) -> usize {
    let graphemes = |part: &str| part.graphemes(true).count();
    if token.starts_with("http://") || token.starts_with("https://") {
        return MASTODON_URL_LENGTH;
    }
    match token.strip_prefix('@').and_then(|rest| rest.find('@')) {
        Some(at) => graphemes(&token[..1 + at]),
        None => graphemes(token),
    }
}

/// The web profile of `handle`: on its own server when it names one, else on mastodon.social.
pub fn mastodon_profile_url(handle: &str) -> String {
    let handle = handle.strip_prefix('@').unwrap_or(handle);
    match handle.split_once('@') {
        Some((user, host)) => format!("https://{host}/@{user}"),
        None => format!("https://{MASTODON_DEFAULT_SERVER}/@{handle}"),
    }
}
