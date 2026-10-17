//! Rich-text facets for a draft: mentions, links and tags with UTF-8 byte offsets.

use serde_json::{json, Value};
use unicode_segmentation::UnicodeSegmentation;

use crate::net::NetError;

pub const MAX_GRAPHEMES: usize = 300;
const MAX_TAG_GRAPHEMES: usize = 64;
const LINK_TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '\'', '"'];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Mention(String),
    Link(String),
    Tag(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

/// Fails before any request when the text is over Bluesky's grapheme limit.
pub fn check_length(text: &str) -> Result<(), NetError> {
    let used = text.graphemes(true).count();
    if used > MAX_GRAPHEMES {
        return Err(NetError::Other(format!(
            "Bluesky allows {MAX_GRAPHEMES} characters; this post has {used}"
        )));
    }
    Ok(())
}

/// Finds mentions, links and tags, in text order, with byte offsets into `text`.
pub fn detect(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut previous: Option<char> = None;
    let mut skip_until = 0;
    for (index, c) in text.char_indices() {
        let at_start = previous.is_none_or(|p| p.is_whitespace() || p == '(');
        previous = Some(c);
        if index < skip_until {
            continue;
        }
        let rest = &text[index..];
        let found = match c {
            '@' if at_start => mention(rest),
            'h' if previous_allows_link(text, index) => link(rest),
            '#' if at_start => tag(rest),
            _ => None,
        };
        if let Some((len, kind)) = found {
            skip_until = index + len;
            spans.push(Span {
                start: index,
                end: index + len,
                kind,
            });
        }
    }
    spans
}

fn previous_allows_link(text: &str, index: usize) -> bool {
    text[..index]
        .chars()
        .next_back()
        .is_none_or(|p| !p.is_alphanumeric())
}

fn mention(rest: &str) -> Option<(usize, Kind)> {
    let body = &rest[1..];
    let len = body
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'))
        .unwrap_or(body.len());
    let handle = body[..len].trim_end_matches('.');
    if handle.contains('.') && !handle.starts_with('.') {
        Some((1 + handle.len(), Kind::Mention(handle.to_string())))
    } else {
        None
    }
}

fn link(rest: &str) -> Option<(usize, Kind)> {
    if !(rest.starts_with("http://") || rest.starts_with("https://")) {
        return None;
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(LINK_TRAILING);
    let scheme = if url.starts_with("https://") { 8 } else { 7 };
    if url.len() > scheme {
        Some((url.len(), Kind::Link(url.to_string())))
    } else {
        None
    }
}

fn tag(rest: &str) -> Option<(usize, Kind)> {
    let body = &rest[1..];
    let len = body.find(|c: char| !tag_char(c)).unwrap_or(body.len());
    let name = &body[..len];
    let graphemes = name.graphemes(true).count();
    let all_digits = name.chars().all(|c| c.is_ascii_digit());
    if graphemes == 0 || graphemes > MAX_TAG_GRAPHEMES || all_digits {
        None
    } else {
        Some((1 + len, Kind::Tag(name.to_string())))
    }
}

fn tag_char(c: char) -> bool {
    !(c.is_whitespace()
        || (c.is_ascii_punctuation() && c != '_')
        || ('\u{2000}'..='\u{206f}').contains(&c)
        || ('\u{3000}'..='\u{303f}').contains(&c))
}

/**
Builds the `facets` array of an `app.bsky.feed.post` record. `resolve` turns a handle into a
DID; a mention it cannot resolve gets no facet. Returns an empty vector when nothing applies.
*/
pub fn build(text: &str, mut resolve: impl FnMut(&str) -> Option<String>) -> Vec<Value> {
    detect(text)
        .into_iter()
        .filter_map(|span| {
            let feature = match span.kind {
                Kind::Mention(handle) => json!({
                    "$type": "app.bsky.richtext.facet#mention",
                    "did": resolve(&handle)?,
                }),
                Kind::Link(uri) => json!({
                    "$type": "app.bsky.richtext.facet#link",
                    "uri": uri,
                }),
                Kind::Tag(tag) => json!({
                    "$type": "app.bsky.richtext.facet#tag",
                    "tag": tag,
                }),
            };
            Some(json!({
                "index": {"byteStart": span.start, "byteEnd": span.end},
                "features": [feature],
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(text: &str, span: &Span) -> String {
        text[span.start..span.end].to_string()
    }

    #[test]
    fn mentions_need_a_dot_and_a_word_start() {
        let text = "hi @ada.bsky.social, (@mina.dev.) and @nodot or mail@x.io";
        let spans = detect(text);
        let found: Vec<(String, Kind)> = spans
            .iter()
            .map(|s| (slice(text, s), s.kind.clone()))
            .collect();
        assert_eq!(
            found,
            vec![
                (
                    "@ada.bsky.social".to_string(),
                    Kind::Mention("ada.bsky.social".into())
                ),
                ("@mina.dev".to_string(), Kind::Mention("mina.dev".into())),
            ]
        );
    }

    #[test]
    fn links_lose_trailing_punctuation() {
        let text = "read https://example.com/a?b=1, then (http://x.org/p). Not ftp://nope";
        let found: Vec<String> = detect(text).iter().map(|s| slice(text, s)).collect();
        assert_eq!(found, vec!["https://example.com/a?b=1", "http://x.org/p"]);
    }

    #[test]
    fn tags_skip_digits_and_stop_at_punctuation() {
        let text = "#rust, #2026 #_x #日本語! a#no";
        let found: Vec<(String, Kind)> = detect(text)
            .iter()
            .map(|s| (slice(text, s), s.kind.clone()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("#rust".to_string(), Kind::Tag("rust".into())),
                ("#_x".to_string(), Kind::Tag("_x".into())),
                ("#日本語".to_string(), Kind::Tag("日本語".into())),
            ]
        );
    }

    #[test]
    fn tags_over_sixty_four_graphemes_are_ignored() {
        let text = format!("#{}", "a".repeat(65));
        assert!(detect(&text).is_empty());
        let text = format!("#{}", "a".repeat(64));
        assert_eq!(detect(&text).len(), 1);
    }

    #[test]
    fn offsets_are_utf8_bytes_after_emoji() {
        let text = "🎉👩‍👩‍👧 @ada.bsky.social https://x.com/é #tag";
        let facets = build(text, |_| Some("did:plc:ada".into()));
        let ranges: Vec<(usize, usize)> = facets
            .iter()
            .map(|f| {
                (
                    f["index"]["byteStart"].as_u64().unwrap_or(0) as usize,
                    f["index"]["byteEnd"].as_u64().unwrap_or(0) as usize,
                )
            })
            .collect();
        let slices: Vec<&str> = ranges.iter().map(|&(s, e)| &text[s..e]).collect();
        assert_eq!(slices, vec!["@ada.bsky.social", "https://x.com/é", "#tag"]);
        assert_eq!(ranges[0].0, "🎉👩‍👩‍👧 ".len());
        assert_eq!(ranges[0].0, 23);
        assert_eq!(
            facets[0]["features"][0]["$type"],
            "app.bsky.richtext.facet#mention"
        );
        assert_eq!(facets[0]["features"][0]["did"], "did:plc:ada");
        assert_eq!(facets[1]["features"][0]["uri"], "https://x.com/é");
        assert_eq!(facets[2]["features"][0]["tag"], "tag");
    }

    #[test]
    fn unresolved_mentions_get_no_facet() {
        let facets = build("hi @ghost.example.com #ok", |_| None);
        assert_eq!(facets.len(), 1);
        assert_eq!(facets[0]["features"][0]["tag"], "ok");
    }

    #[test]
    fn the_limit_counts_graphemes_not_bytes() {
        let family = "👩‍👩‍👧";
        assert!(check_length(&family.repeat(300)).is_ok());
        let err = check_length(&family.repeat(301)).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Bluesky allows 300 characters; this post has 301"
        );
    }
}
