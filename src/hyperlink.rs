//! OSC 8 hyperlinks: whether the terminal supports them, and the cells that open names and links.

use std::io::IsTerminal;

use ratatui::buffer::Buffer;
use ratatui::style::Style;
use ratatui::text::Span;
use url::Url;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const PROGRAMS: [&str; 7] = [
    "vscode",
    "iTerm.app",
    "WezTerm",
    "ghostty",
    "Hyper",
    "Tabby",
    "rio",
];
const TERMS: [&str; 5] = ["kitty", "ghostty", "wezterm", "foot", "alacritty"];

/// A run of cells on one row that links to `url`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub url: String,
}

/// Whether OSC 8 hyperlinks should be emitted for this process.
pub fn enabled() -> bool {
    let env = |key: &str| std::env::var(key).ok().filter(|value| !value.is_empty());
    enabled_from(
        &env,
        std::io::stdout().is_terminal() && !crate::agent::active(),
    )
}

/// Applies the hyperlink rules to an environment lookup and a TTY flag.
pub fn enabled_from(env: &dyn Fn(&str) -> Option<String>, tty: bool) -> bool {
    let get = |key: &str| env(key).filter(|value| !value.is_empty());
    match get("SOCLI_HYPERLINKS").as_deref() {
        Some("1") => return true,
        Some("0") => return false,
        _ => {}
    }
    let term = get("TERM").unwrap_or_default();
    if !tty || term == "dumb" {
        return false;
    }
    if get("TMUX").is_some() || get("STY").is_some() {
        return false;
    }
    let program = get("TERM_PROGRAM").unwrap_or_default();
    PROGRAMS.contains(&program.as_str())
        || TERMS.iter().any(|name| term.contains(name))
        || get("VTE_VERSION")
            .and_then(|version| version.parse::<u32>().ok())
            .is_some_and(|version| version >= 5000)
        || get("WT_SESSION").is_some()
        || get("KONSOLE_VERSION").is_some()
}

/// Returns the serialized URL when it is http(s) and printable ASCII only.
pub fn safe(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let serialized = parsed.as_str();
    serialized
        .chars()
        .all(|c| ('\x21'..='\x7e').contains(&c))
        .then(|| serialized.to_string())
}

/// Wraps `text` in an OSC 8 link to `url`, or returns it unchanged when the URL is unsafe.
pub fn wrap(url: &str, text: &str) -> String {
    match safe(url) {
        Some(url) => format!("{}{text}{CLOSE}", open(&url)),
        None => text.to_string(),
    }
}

/// Rewrites the cells under each region into OSC 8 chunks of two or three columns.
pub fn paint(buf: &mut Buffer, regions: &[Region]) {
    let area = buf.area;
    for region in regions {
        let Some(url) = safe(&region.url) else {
            continue;
        };
        if region.y < area.y || region.y >= area.bottom() {
            continue;
        }
        let start = region.x.max(area.x);
        let end = region.x.saturating_add(region.width).min(area.right());
        let open = open(&url);
        let mut x = start;
        while x < end {
            x = if buf[(x, region.y)].symbol().is_empty() {
                x + 1
            } else {
                paint_chunk(buf, &open, x, region.y, end)
            };
        }
    }
}

const CLOSE: &str = "\x1b]8;;\x1b\\";

fn open(url: &str) -> String {
    format!("\x1b]8;id={};{url}\x1b\\", link_id(url))
}

fn link_id(url: &str) -> String {
    let hash = url.bytes().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    });
    format!("{:012x}", hash >> 16)
}

fn cell_width(buf: &Buffer, x: u16, y: u16) -> usize {
    Span::raw(buf[(x, y)].symbol()).width()
}

/**
Wraps the chunk starting at `x` and returns the next x to visit.

ratatui's diff measures a chunk by the printable characters of its escape sequence, so it never
emits the cell right after the chunk's first cell. Every chunk therefore covers at least two
display columns of one style, and a cell that cannot join one stays a plain cell.
*/
fn paint_chunk(buf: &mut Buffer, open: &str, x: u16, y: u16, end: u16) -> u16 {
    let style = buf[(x, y)].style();
    let width = cell_width(buf, x, y).max(1);
    let mut members = vec![x];
    let mut after = x + width as u16;
    let mut covered = width;
    if width == 1 {
        let Some((joined, joined_width)) = same_style(buf, after, y, end, style) else {
            return x + 1;
        };
        members.push(joined);
        after = joined + joined_width as u16;
        covered += joined_width;
    }
    if covered < 3 {
        if let Some((last, 1)) = same_style(buf, after, y, end, style) {
            let rest = same_style(buf, last + 1, y, end, style);
            if rest.is_none_or(|(_, rest_width)| rest_width > 1) {
                members.push(last);
                after = last + 1;
            }
        }
    }
    let symbols: String = members.iter().map(|&m| buf[(m, y)].symbol()).collect();
    for &member in &members[1..] {
        buf[(member, y)].set_skip(true);
    }
    buf[(x, y)].set_symbol(&format!("{open}{symbols}{CLOSE}"));
    after
}

/** The next printed cell at or after `from` inside the region when it has `style`, with its width. */
fn same_style(buf: &Buffer, from: u16, y: u16, end: u16, style: Style) -> Option<(u16, usize)> {
    let mut x = from;
    while x < end && buf[(x, y)].symbol().is_empty() {
        x += 1;
    }
    (x < end && buf[(x, y)].style() == style).then(|| (x, cell_width(buf, x, y).max(1)))
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Stylize};

    use super::*;

    const URL: &str = "https://example.com/a";

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        move |key| {
            owned
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    fn linked(url: &str, text: &str) -> String {
        format!("{}{text}{CLOSE}", open(url))
    }

    #[test]
    fn id_is_stable() {
        assert_eq!(link_id(URL), link_id(URL));
        assert_eq!(link_id(URL).len(), 12);
        assert!(link_id(URL).chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(link_id(""), "cbf29ce48422");
        assert_ne!(link_id(URL), link_id("https://example.com/b"));
    }

    #[test]
    fn safe_accepts_http_and_https() {
        assert_eq!(safe(URL).as_deref(), Some(URL));
        assert_eq!(
            safe("http://example.com").as_deref(),
            Some("http://example.com/")
        );
    }

    #[test]
    fn safe_rejects_other_schemes() {
        assert_eq!(safe("javascript:alert(1)"), None);
        assert_eq!(safe("file:///etc/passwd"), None);
        assert_eq!(safe("not a url"), None);
    }

    #[test]
    fn safe_rejects_control_space_and_non_ascii() {
        assert_eq!(
            safe("https://example.com/\x1b]8;;"),
            Some("https://example.com/%1B]8;;".to_string())
        );
        for url in [
            "https://exa\x07mple.com/",
            "https://exa mple.com/",
            "https://exa\x1bmple.com/",
        ] {
            assert_eq!(safe(url), None, "{url:?}");
        }
    }

    #[test]
    fn safe_serializes_non_ascii_to_ascii() {
        assert_eq!(
            safe("https://exämple.com/").as_deref(),
            Some("https://xn--exmple-cua.com/")
        );
    }

    #[test]
    fn safe_output_is_printable_ascii() {
        let url = safe("https://example.com/日本 \x1b\x07?q=é#é").unwrap_or_default();
        assert!(url.chars().all(|c| ('\x21'..='\x7e').contains(&c)));
    }

    #[test]
    fn wrap_bytes() {
        let id = link_id(URL);
        assert_eq!(
            wrap(URL, "hi"),
            format!("\x1b]8;id={id};{URL}\x1b\\hi\x1b]8;;\x1b\\")
        );
        assert_eq!(wrap("javascript:alert(1)", "hi"), "hi");
    }

    #[test]
    fn enabled_flag_wins() {
        assert!(enabled_from(&env_of(&[("SOCLI_HYPERLINKS", "1")]), false));
        let env = env_of(&[("SOCLI_HYPERLINKS", "0"), ("TERM_PROGRAM", "ghostty")]);
        assert!(!enabled_from(&env, true));
    }

    #[test]
    fn enabled_needs_tty_and_not_dumb() {
        let env = env_of(&[("TERM_PROGRAM", "ghostty")]);
        assert!(!enabled_from(&env, false));
        let env = env_of(&[("TERM_PROGRAM", "ghostty"), ("TERM", "dumb")]);
        assert!(!enabled_from(&env, true));
    }

    #[test]
    fn enabled_off_in_multiplexers() {
        for key in ["TMUX", "STY"] {
            let env = env_of(&[("TERM_PROGRAM", "iTerm.app"), (key, "1")]);
            assert!(!enabled_from(&env, true));
        }
    }

    #[test]
    fn enabled_known_terminals() {
        for program in PROGRAMS {
            assert!(enabled_from(&env_of(&[("TERM_PROGRAM", program)]), true));
        }
        for term in ["xterm-kitty", "xterm-ghostty", "foot", "alacritty"] {
            assert!(enabled_from(&env_of(&[("TERM", term)]), true), "{term}");
        }
        assert!(enabled_from(&env_of(&[("VTE_VERSION", "6800")]), true));
        assert!(!enabled_from(&env_of(&[("VTE_VERSION", "4000")]), true));
        assert!(enabled_from(&env_of(&[("WT_SESSION", "x")]), true));
        assert!(enabled_from(&env_of(&[("KONSOLE_VERSION", "1")]), true));
    }

    #[test]
    fn enabled_unknown_terminal_is_off() {
        let env = env_of(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "Apple_Terminal"),
        ]);
        assert!(!enabled_from(&env, true));
    }

    fn row(width: u16, text: &str) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 1));
        buf.set_string(0, 0, text, Style::default());
        buf
    }

    fn region(x: u16, width: u16) -> Region {
        Region {
            x,
            y: 0,
            width,
            url: URL.to_string(),
        }
    }

    fn skipped(buf: &Buffer) -> Vec<u16> {
        (0..buf.area.width).filter(|&x| buf[(x, 0)].skip).collect()
    }

    #[test]
    fn paint_chunks_cover_at_least_two_columns() {
        let mut buf = Buffer::with_lines(["see x.com/a b"]);
        paint(&mut buf, &[region(4, 7)]);
        let symbols: Vec<&str> = (0..13).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(&symbols[..4], ["s", "e", "e", " "]);
        assert_eq!(symbols[4], linked(URL, "x."));
        assert_eq!(symbols[6], linked(URL, "co"));
        assert_eq!(symbols[8], linked(URL, "m/a"));
        assert_eq!(&symbols[11..], [" ", "b"]);
        assert_eq!(skipped(&buf), [5, 7, 9, 10]);
    }

    #[test]
    fn paint_width_three_is_one_chunk() {
        let mut buf = row(5, "abcde");
        paint(&mut buf, &[region(0, 3)]);
        assert_eq!(buf[(0, 0)].symbol(), linked(URL, "abc"));
        assert_eq!(skipped(&buf), [1, 2]);
        assert_eq!(buf[(3, 0)].symbol(), "d");
    }

    #[test]
    fn paint_width_four_is_two_pairs() {
        let mut buf = row(5, "abcde");
        paint(&mut buf, &[region(0, 4)]);
        assert_eq!(buf[(0, 0)].symbol(), linked(URL, "ab"));
        assert_eq!(buf[(2, 0)].symbol(), linked(URL, "cd"));
        assert_eq!(skipped(&buf), [1, 3]);
    }

    #[test]
    fn paint_width_five_is_a_pair_and_a_triple() {
        let mut buf = row(6, "abcdef");
        paint(&mut buf, &[region(0, 5)]);
        assert_eq!(buf[(0, 0)].symbol(), linked(URL, "ab"));
        assert_eq!(buf[(2, 0)].symbol(), linked(URL, "cde"));
        assert_eq!(skipped(&buf), [1, 3, 4]);
        assert_eq!(buf[(5, 0)].symbol(), "f");
    }

    #[test]
    fn paint_never_mixes_styles_in_a_chunk() {
        let mut buf = row(5, "abcde");
        buf.set_style(Rect::new(0, 0, 2, 1), Style::default().bold());
        buf.set_style(Rect::new(2, 0, 3, 1), Style::default().dim());
        paint(&mut buf, &[region(0, 5)]);
        assert_eq!(buf[(0, 0)].symbol(), linked(URL, "ab"));
        assert_eq!(buf[(2, 0)].symbol(), linked(URL, "cde"));
        assert_eq!(skipped(&buf), [1, 3, 4]);
    }

    #[test]
    fn paint_one_column_region_stays_plain() {
        let mut buf = row(3, "abc");
        paint(&mut buf, &[region(1, 1)]);
        assert_eq!(buf, row(3, "abc"));
    }

    #[test]
    fn paint_lone_cell_between_styles_is_left_unlinked() {
        let mut buf = row(5, "abcde");
        buf.set_style(Rect::new(0, 0, 2, 1), Style::default().bold());
        buf.set_style(Rect::new(2, 0, 1, 1), Style::default().italic());
        buf.set_style(Rect::new(3, 0, 2, 1), Style::default().dim());
        paint(&mut buf, &[region(0, 5)]);
        assert_eq!(buf[(0, 0)].symbol(), linked(URL, "ab"));
        assert_eq!(buf[(2, 0)].symbol(), "c");
        assert!(!buf[(2, 0)].skip);
        assert_eq!(buf[(3, 0)].symbol(), linked(URL, "de"));
        assert_eq!(skipped(&buf), [1, 4]);
    }

    #[test]
    fn paint_wide_character_with_a_neighbor_is_one_chunk() {
        let mut buf = row(4, "日a");
        paint(&mut buf, &[region(0, 3)]);
        assert_eq!(buf[(0, 0)].symbol(), linked(URL, "日a"));
        assert_eq!(buf[(1, 0)].symbol(), " ");
        assert_eq!(skipped(&buf), [2]);
    }

    #[test]
    fn paint_adjacent_wide_characters_are_separate_chunks() {
        let mut buf = row(4, "日本");
        paint(&mut buf, &[region(0, 4)]);
        assert_eq!(buf[(0, 0)].symbol(), linked(URL, "日"));
        assert_eq!(buf[(2, 0)].symbol(), linked(URL, "本"));
        assert!(skipped(&buf).is_empty());
    }

    #[test]
    fn paint_clips_at_the_right_edge() {
        let mut buf = Buffer::with_lines(["abcde"]);
        paint(&mut buf, &[region(3, 10)]);
        assert_eq!(buf[(3, 0)].symbol(), linked(URL, "de"));
        assert!(buf[(4, 0)].skip);
        assert_eq!(buf[(2, 0)].symbol(), "c");
    }

    fn assert_redrawn_after(width: u16, link: u16) {
        let mut previous = row(8, "abcdefgh");
        previous.set_style(previous.area, Style::default().bg(Color::Red));
        let mut next = row(8, "abcdefgh");
        paint(&mut next, &[region(0, link)]);
        let updates = previous.diff(&next);
        assert!(
            updates.iter().any(|&(x, y, _)| (x, y) == (width, 0)),
            "cell {width} is not redrawn for a region of {link}"
        );
    }

    #[test]
    fn diff_redraws_the_cell_after_a_region() {
        assert_redrawn_after(3, 3);
        assert_redrawn_after(5, 5);
        assert_redrawn_after(1, 1);
    }

    #[test]
    fn paint_ignores_unsafe_and_out_of_bounds_regions() {
        let mut buf = Buffer::with_lines(["abc"]);
        let regions = [
            Region {
                x: 0,
                y: 0,
                width: 3,
                url: "javascript:alert(1)".to_string(),
            },
            Region {
                x: 0,
                y: 5,
                width: 3,
                url: URL.to_string(),
            },
        ];
        paint(&mut buf, &regions);
        assert_eq!(buf, Buffer::with_lines(["abc"]));
    }
}
