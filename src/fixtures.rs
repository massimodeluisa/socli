//! Shared test data: a frozen clock, the demo state, malformed reply graphs, grapheme text and terminal controls.

use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::config::Config;
use crate::model::{Metrics, Post, User};
use crate::net::{Clients, Network};
use crate::state::State;
use crate::theme::Detail;

/// Text that needs grapheme cells: a decomposed accent, accents, ZWJ emoji, a flag, an emoji presentation selector and CJK.
pub const GRAPHEME_TEXTS: [&str; 8] = [
    "e\u{301}",
    "José",
    "café",
    "👩‍💻",
    "👨‍👩‍👧",
    "🇮🇹",
    "❤\u{FE0F}",
    "日本語",
];

/// Terminal control payloads, each around words that must survive cleaning.
pub const CONTROL_TEXTS: [&str; 8] = [
    "\x1b[31mred\x1b[0m",
    "\x1b]8;;https://evil.example\x07click\x1b]8;;\x07",
    "\x1b]0;title\x07",
    "\x07",
    "a\rb",
    "\u{009b}2J",
    "\u{202e}abc",
    "\x1bP+q\x1b\\",
];

pub fn frozen_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0)
        .single()
        .expect("a valid fixed time")
}

/// The demo state at `detail`, with its clock and its demo world frozen at [`frozen_now`].
pub fn demo_state(detail: Detail) -> State {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.frozen_now = Some(frozen_now());
    state.install_demo();
    state.detail = detail;
    state.retheme();
    state
}

/// An X post `id` answering `parent`, `minutes` before [`frozen_now`].
pub fn post(id: &str, parent: Option<&str>, minutes: i64) -> Post {
    Post {
        id: id.into(),
        net: Network::X,
        url: Some(format!("https://x.com/ada/status/{id}")),
        cid: None,
        author: User {
            id: "x:u1".into(),
            name: "Ada".into(),
            username: "ada".into(),
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        },
        text: format!("text of {id}"),
        created_at: Some(frozen_now() - Duration::minutes(minutes)),
        conversation_id: "x:c1".into(),
        in_reply_to: parent.map(str::to_string),
        quoted: None,
        reposted_by: None,
        metrics: Metrics::default(),
        links: Vec::new(),
        media: Vec::new(),
        liked: false,
        bookmarked: false,
        reposted: false,
    }
}

/// A answers B and B answers A, so neither has a parent outside the cycle.
pub fn cycle() -> Vec<Post> {
    vec![post("x:a", Some("x:b"), 2), post("x:b", Some("x:a"), 1)]
}

pub fn self_parent() -> Vec<Post> {
    vec![post("x:root", None, 3), post("x:self", Some("x:self"), 1)]
}

/// A reply whose parent is not on the page.
pub fn orphan() -> Vec<Post> {
    vec![post("x:root", None, 3), post("x:orphan", Some("x:gone"), 1)]
}

/// The same id twice, with a reply under it.
pub fn duplicates() -> Vec<Post> {
    vec![
        post("x:dup", None, 3),
        post("x:dup", None, 2),
        post("x:reply", Some("x:dup"), 1),
    ]
}

/// `n` posts, each answering the one before, oldest first.
pub fn deep_chain(n: usize) -> Vec<Post> {
    (0..n)
        .map(|i| {
            let parent = i.checked_sub(1).map(|p| format!("x:p{p}"));
            let minutes = i64::try_from(n - i).unwrap_or(i64::MAX);
            post(&format!("x:p{i}"), parent.as_deref(), minutes)
        })
        .collect()
}

/// One post per [`GRAPHEME_TEXTS`] entry, with a link right after the text.
pub fn graphemes() -> Vec<Post> {
    texts(&GRAPHEME_TEXTS, "g")
}

/// One post per [`CONTROL_TEXTS`] entry.
pub fn controls() -> Vec<Post> {
    texts(&CONTROL_TEXTS, "c")
}

fn texts(texts: &[&str], prefix: &str) -> Vec<Post> {
    texts
        .iter()
        .enumerate()
        .map(|(i, text)| Post {
            text: format!("{text} https://example.com/{prefix}{i}"),
            ..post(&format!("x:{prefix}{i}"), None, 10 + i as i64)
        })
        .collect()
}
