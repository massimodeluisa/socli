//! Status HTML to plain text: paragraphs, line breaks, full links and full mention handles.

/** Plain text of a status's HTML, and the links it holds (not mentions or hashtags), in order. */
pub fn to_text(html: &str, mentions: &[(String, String)]) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut links: Vec<String> = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(open) = rest.find('<') else {
            out.push_str(&decode(rest));
            break;
        };
        out.push_str(&decode(&rest[..open]));
        rest = &rest[open..];
        let end = tag_end(&rest[1..]);
        let tag = &rest[1..1 + end];
        rest = rest.get(end + 2..).unwrap_or("");
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let class = attribute(tag, "class").unwrap_or_default();
        let has_class = |wanted: &str| class.split_whitespace().any(|c| c == wanted);
        match (closing, name.as_str()) {
            (false, "p") if has_class("quote-inline") => rest = skip_past(rest, "</p>"),
            (false, "br") => out.push('\n'),
            (false, "p" | "blockquote" | "pre") => blank_line(&mut out),
            (false, "li") => {
                end_line(&mut out);
                out.push_str("• ");
            }
            (false, "ul" | "ol") => blank_line(&mut out),
            (true, "ul" | "ol") => end_line(&mut out),
            (false, "a") => {
                let Some(href) = attribute(tag, "href") else {
                    continue;
                };
                let handle = mentions
                    .iter()
                    .find(|(url, _)| *url == href)
                    .filter(|_| !has_class("hashtag"))
                    .map(|(_, handle)| handle);
                if let Some(handle) = handle {
                    out.push('@');
                    out.push_str(handle);
                    rest = skip_past(rest, "</a>");
                } else if !has_class("mention") && !has_class("hashtag") && !links.contains(&href) {
                    links.push(href);
                }
            }
            _ => {}
        }
    }
    (tidy(&out), links)
}

fn blank_line(out: &mut String) {
    if out.is_empty() || out.ends_with("\n\n") {
        return;
    }
    out.push_str(if out.ends_with('\n') { "\n" } else { "\n\n" });
}

fn end_line(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn tidy(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').map(str::trim_end).collect();
    let mut joined = lines.join("\n");
    while joined.contains("\n\n\n") {
        joined = joined.replace("\n\n\n", "\n\n");
    }
    joined.trim().to_string()
}

/// What follows the first `marker` (ASCII case-insensitive) in `rest`, or nothing.
fn skip_past<'a>(rest: &'a str, marker: &str) -> &'a str {
    rest.to_ascii_lowercase()
        .find(marker)
        .map_or("", |at| &rest[at + marker.len()..])
}

/// The entity-decoded value of attribute `name` in the text of one tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(name) {
        let start = from + at;
        from = start + name.len();
        let spaced = start > 0 && lower.as_bytes()[start - 1].is_ascii_whitespace();
        let after = tag[from..].trim_start();
        let Some(value) = after.strip_prefix('=').filter(|_| spaced) else {
            continue;
        };
        let value = value.trim_start();
        let raw = match value.chars().next()? {
            quote @ ('"' | '\'') => {
                let inner = &value[1..];
                &inner[..inner.find(quote)?]
            }
            _ => value.split_whitespace().next()?,
        };
        return Some(decode(raw));
    }
    None
}

/// Returns the byte index of the `>` that closes a tag, ignoring any inside quotes.
fn tag_end(rest: &str) -> usize {
    let mut quote = None;
    for (index, c) in rest.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, '>') => return index,
            _ => {}
        }
    }
    rest.len()
}

fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest
            .find(';')
            .filter(|&end| end <= 10)
            .and_then(|end| entity(&rest[1..end]).map(|c| (c, end)));
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "nbsp" => Some(' '),
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_breaks_entities_and_links() {
        let html = r#"<p>One &amp; two<br />three</p><p><a href="https://example.org/a/b" rel="nofollow"><span class="invisible">https://</span><span class="ellipsis">example.org/a</span><span class="invisible">/b</span></a></p>"#;
        let (text, links) = to_text(html, &[]);
        assert_eq!(text, "One & two\nthree\n\nhttps://example.org/a/b");
        assert_eq!(links, vec!["https://example.org/a/b".to_string()]);
    }

    #[test]
    fn mentions_get_their_full_handle_and_hashtags_stay_text() {
        let html = r#"<p><span class="h-card"><a href="https://fosstodon.org/@theo" class="u-url mention">@<span>theo</span></a></span> hi <a href="https://mastodon.social/tags/cli" class="mention hashtag" rel="tag">#<span>CLI</span></a></p>"#;
        let mentions = vec![(
            "https://fosstodon.org/@theo".to_string(),
            "theo@fosstodon.org".to_string(),
        )];
        let (text, links) = to_text(html, &mentions);
        assert_eq!(text, "@theo@fosstodon.org hi #CLI");
        assert!(links.is_empty());
    }

    #[test]
    fn the_quote_fallback_paragraph_is_dropped() {
        let html = r#"<p class="quote-inline">RE: <a href="https://a.example/@b/1">a.example/@b/1</a></p><p>Mine.</p>"#;
        assert_eq!(to_text(html, &[]).0, "Mine.");
    }

    #[test]
    fn lists_become_bullets() {
        assert_eq!(
            to_text("<p>Steps</p><ul><li>one</li><li>two</li></ul>", &[]).0,
            "Steps\n\n• one\n• two"
        );
    }
}
