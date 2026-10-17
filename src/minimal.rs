//! The minimal and plain detail levels: a network line beside each post instead of avatars and rails.

use std::collections::{HashMap, HashSet};

use chrono::Local;
use unicode_width::UnicodeWidthStr;

use crate::layout::{self, LaidRow, Target};
use crate::model::{Post, User};
use crate::net::Network;
use crate::render::{self, Act, Canvas, Kind, RowSpan, View, ViewOpts};
use crate::theme::{compact, Detail};
use crate::util::{self, ago};

/**
Canvas columns: a root's network line at 1 and its text at 3. Each reply depth moves both two
columns right, down to depth 2, so a deep thread keeps its room.
*/
const LINE: usize = 1;
const TEXT: usize = 3;
const STEP: usize = 2;
const DEEPEST: usize = 2;
const QUOTE_ROWS: usize = 3;

/// A feed, a profile or an open thread at the minimal or plain level.
pub fn render(posts: &[Post], expanded: &HashSet<String>, opts: &ViewOpts) -> View {
    let mut laid = layout::lay_for(posts, expanded, opts.newest, opts.overview);
    if opts.profile.is_some() {
        laid = layout::profiled(laid);
    }
    let detail = opts.thread.as_deref().map(|focus| {
        laid.iter()
            .position(|r| r.depth == 0 && layout::post_id(r) == Some(focus))
            .unwrap_or(0)
    });
    let mut pen = Pen::new(posts, opts);
    let mut rows = Vec::with_capacity(laid.len());
    for (i, row) in laid.iter().enumerate() {
        let selected = opts.selected == Some(i);
        pen.space(row);
        let start = pen.canvas.len();
        match &row.target {
            Target::Gap => {}
            Target::Profile => {
                if let Some((net, user)) = &opts.profile {
                    pen.profile(*net, user, selected);
                }
            }
            Target::Fold { parent, hidden } => pen.fold(parent, row.depth, *hidden, selected),
            Target::Post(id) => {
                if let Some(post) = pen.by_id.get(id.as_str()).copied() {
                    if detail == Some(i) {
                        pen.detail(post, selected);
                    } else {
                        let depth = row.depth.min(DEEPEST);
                        let slug = opts.tags && (depth == 0 || pen.plain);
                        pen.post(post, depth, selected, slug);
                        let replies_shown = laid.get(i + 1).is_some_and(|next| next.depth > 0);
                        if row.depth == 0
                            && !replies_shown
                            && (row.folded > 0 || post.metrics.replies > 0)
                        {
                            let text = render::replies_label(row.folded);
                            let y = pen.note(post.net, 0, selected, &text);
                            let act = Act::Fold(post.id.clone());
                            pen.canvas.target(y, TEXT, TEXT + text.width(), act);
                        }
                    }
                }
            }
        }
        rows.push(RowSpan {
            start,
            end: pen.canvas.len(),
        });
    }
    pen.finish(rows)
}

/// The mentions list at the minimal or plain level.
pub fn notifications(posts: &[Post], expanded: &HashSet<String>, opts: &ViewOpts) -> View {
    let laid = layout::lay(posts, expanded, opts.newest);
    let mut pen = Pen::new(posts, opts);
    let mut rows = Vec::with_capacity(laid.len());
    for (i, row) in laid.iter().enumerate() {
        let selected = opts.selected == Some(i);
        pen.space(row);
        let start = pen.canvas.len();
        match &row.target {
            Target::Post(id) => {
                if let Some(post) = pen.by_id.get(id.as_str()).copied() {
                    pen.mention(post, selected);
                }
            }
            Target::Fold { parent, hidden } => pen.fold(parent, 0, *hidden, selected),
            Target::Gap | Target::Profile => {}
        }
        rows.push(RowSpan {
            start,
            end: pen.canvas.len(),
        });
    }
    pen.finish(rows)
}

struct Pen<'a> {
    canvas: Canvas,
    opts: &'a ViewOpts,
    /// Plain draws ASCII lines and every span but the author names in the terminal's own color.
    plain: bool,
    by_id: HashMap<&'a str, &'a Post>,
}

impl<'a> Pen<'a> {
    fn new(posts: &'a [Post], opts: &'a ViewOpts) -> Self {
        Self {
            canvas: Canvas::new(opts.width),
            opts,
            plain: opts.level == Detail::Plain,
            by_id: posts.iter().map(|p| (p.id.as_str(), p)).collect(),
        }
    }

    fn finish(self, rows: Vec<RowSpan>) -> View {
        let (lines, links, targets) = self.canvas.finish_targeted();
        View {
            lines,
            rows,
            links,
            previews: Vec::new(),
            unfurls: Vec::new(),
            targets,
        }
    }

    /// `kind`, or in plain the terminal's own color; names stay Bold.
    fn tone(&self, kind: Kind) -> Kind {
        if self.plain && kind != Kind::Bold {
            Kind::Fg
        } else {
            kind
        }
    }

    /// One blank row between blocks; a fold stays under the block it belongs to.
    fn space(&mut self, row: &LaidRow) {
        let attached = matches!(row.target, Target::Fold { .. } | Target::Gap);
        if !attached && !self.canvas.is_empty() {
            self.canvas.push(false);
        }
    }

    /// A row with the selection bar when it is selected.
    fn bare(&mut self, selected: bool) -> usize {
        let y = self.canvas.push(selected);
        if selected {
            self.canvas.bar(y);
        }
        y
    }

    /// The network line: `▎` in the network's color, `|` in plain.
    fn line(&self, net: Network) -> (&'static str, Kind) {
        if self.plain {
            ("|", Kind::Fg)
        } else {
            ("▎", net.accent())
        }
    }

    /// A row of a block at `depth`, on `net`'s line.
    fn row(&mut self, net: Network, depth: usize, selected: bool) -> usize {
        let y = self.bare(selected);
        let (glyph, kind) = self.line(net);
        self.canvas.put(y, LINE + STEP * depth, kind, glyph);
        y
    }

    /// One row of dim text inside a block; returns the row.
    fn note(&mut self, net: Network, depth: usize, selected: bool, text: &str) -> usize {
        let y = self.row(net, depth, selected);
        let dim = self.tone(Kind::Dim);
        self.canvas.put(y, TEXT + STEP * depth, dim, text);
        y
    }

    /// `Show N more replies` under the replies shown, on the parent's line.
    fn fold(&mut self, parent: &str, depth: usize, hidden: usize, selected: bool) {
        let depth = depth.min(DEEPEST);
        let text = format!("Show {hidden} more replies");
        let col = TEXT + STEP * depth;
        let y = match self.by_id.get(parent).map(|post| post.net) {
            Some(net) => self.note(net, depth, selected, &text),
            None => {
                let y = self.bare(selected);
                let dim = self.tone(Kind::Dim);
                self.canvas.put(y, col, dim, &text);
                y
            }
        };
        self.canvas
            .target(y, col, col + text.width(), Act::Fold(parent.to_string()));
    }

    /**
    `Name @handle · 4m`, then ` · <network>` when `slug`; returns the column after it. `pressable`
    makes the byline and the time targets of the post's own block.
    */
    fn head(&mut self, y: usize, col: usize, post: &Post, slug: bool, pressable: bool) -> usize {
        let (bold, dim) = (self.tone(Kind::Bold), self.tone(Kind::Dim));
        let now = self.opts.now;
        let x = self.canvas.put(y, col, bold, &post.author.name);
        let mut x = self
            .canvas
            .put(y, x + 1, dim, &format!("@{}", post.author.username));
        self.canvas.link(y, col, x, &render::profile_url(post));
        if pressable {
            self.canvas.target(y, col, x, Act::Author(post.id.clone()));
        }
        if let Some(at) = post.created_at {
            let start = self.canvas.put(y, x, dim, " · ");
            x = self.canvas.put(y, start, dim, &ago(at, now));
            self.canvas.link(y, start, x, &post.permalink());
            if pressable {
                self.canvas
                    .target(y, start, x, Act::Thread(post.id.clone()));
            }
        }
        if slug {
            x = self.canvas.put(y, x, dim, " · ");
            x = self.canvas.put(y, x, dim, post.net.label());
        }
        x
    }

    /// The post's text wrapped to the room after its column. `full` paints every line.
    /// A list cap returns true when lines were left out.
    fn text(&mut self, post: &Post, depth: usize, selected: bool, full: bool) -> bool {
        if post.text.trim().is_empty() {
            return false;
        }
        let col = TEXT + STEP * depth;
        let link = self.tone(Kind::Link);
        let lines = util::wrap_text(&post.text, self.canvas.width().saturating_sub(col));
        let opened = self.opts.opened.contains(&post.id);
        let (limit, cut) = if full {
            (lines.len(), false)
        } else {
            render::body_window(lines.len(), opened)
        };
        for line in lines.iter().take(limit) {
            let y = self.row(post.net, depth, selected);
            render::paint_text(&mut self.canvas, y, col, line, post, link);
        }
        cut
    }

    /// `Show more` or `Show less` after the stats, before a fold the caller draws.
    fn more(&mut self, post: &Post, depth: usize, selected: bool) {
        let col = TEXT + STEP * depth;
        let open = self.opts.opened.contains(&post.id);
        let label = render::more_label(open);
        let y = self.row(post.net, depth, selected);
        let end = self.canvas.put(y, col, self.tone(Kind::Blue), label);
        self.canvas.target(y, col, end, Act::More(post.id.clone()));
    }

    /// A feed post at `depth`: who reposted it, the header, text, media, link, quote and stats.
    fn post(&mut self, post: &Post, depth: usize, selected: bool, slug: bool) {
        if let Some(by) = &post.reposted_by {
            self.note(post.net, depth, selected, &format!("@{by} reposted"));
        }
        let y = self.row(post.net, depth, selected);
        self.head(y, TEXT + STEP * depth, post, slug, true);
        let list_root = depth == 0 && self.opts.thread.is_none();
        let cut = self.text(post, depth, selected, !list_root);
        self.media(post, depth, selected);
        self.link(post, depth, selected);
        self.quote(post, depth, selected);
        self.words(post, depth, selected);
        if cut {
            self.more(post, depth, selected);
        }
    }

    /// The open thread's root: name and handle, the text, the date, then the stats in words.
    fn detail(&mut self, post: &Post, selected: bool) {
        let (bold, dim) = (self.tone(Kind::Bold), self.tone(Kind::Dim));
        let y = self.row(post.net, 0, selected);
        let x = self.canvas.put(y, TEXT, bold, &post.author.name);
        let x = self
            .canvas
            .put(y, x + 1, dim, &format!("@{}", post.author.username));
        self.canvas.link(y, TEXT, x, &render::profile_url(post));
        self.canvas.target(y, TEXT, x, Act::Author(post.id.clone()));
        self.text(post, 0, selected, true);
        self.media(post, 0, selected);
        self.link(post, 0, selected);
        self.quote(post, 0, selected);
        if let Some(at) = post.created_at {
            let when = at
                .with_timezone(&Local)
                .format("%-I:%M %p · %b %-d, %Y")
                .to_string();
            let y = self.row(post.net, 0, selected);
            let x = self.canvas.put(y, TEXT, dim, &when);
            self.canvas.link(y, TEXT, x, &post.permalink());
            self.canvas.target(y, TEXT, x, Act::Thread(post.id.clone()));
        }
        self.words(post, 0, selected);
    }

    /// A mention: the post with `mentioned you` after its header.
    fn mention(&mut self, post: &Post, selected: bool) {
        let tags = self.opts.tags;
        let blue = self.tone(Kind::Blue);
        let y = self.row(post.net, 0, selected);
        let x = self.head(y, TEXT, post, tags, true);
        self.canvas.put(y, x + 2, blue, "mentioned you");
        let cut = self.text(post, 0, selected, false);
        self.words(post, 0, selected);
        if cut {
            self.more(post, 0, selected);
        }
    }

    /// `image · <alt>` or `2 images`, then `v view` when images can open.
    fn media(&mut self, post: &Post, depth: usize, selected: bool) {
        let Some(media) = post.media.first() else {
            return;
        };
        let col = TEXT + STEP * depth;
        let mut label = match post.media.len() {
            1 => media.kind.label().to_string(),
            n => format!("{n} images"),
        };
        if post.media.len() == 1 {
            let alt = media
                .alt
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !alt.is_empty() {
                label.push_str(" · ");
                label.push_str(&alt);
            }
        }
        let tail = if self.opts.images {
            if self.opts.shortcuts {
                "  v view"
            } else {
                "  view"
            }
        } else {
            ""
        };
        let room = self.canvas.width().saturating_sub(col + 1 + tail.width());
        let dim = self.tone(Kind::Dim);
        let y = self.row(post.net, depth, selected);
        let x = self.canvas.put(y, col, dim, &util::truncate(&label, room));
        let end = self.canvas.put(y, x, dim, tail);
        self.canvas.target(y, col, end, Act::Media(post.id.clone()));
    }

    /// A link the text doesn't show already: its display URL, linked to the page.
    fn link(&mut self, post: &Post, depth: usize, selected: bool) {
        let Some(link) = post.links.first() else {
            return;
        };
        let in_text = [&link.short, &link.display, &link.expanded]
            .iter()
            .any(|url| !url.is_empty() && post.text.contains(url.as_str()));
        if in_text || link.display.is_empty() {
            return;
        }
        let col = TEXT + STEP * depth;
        let room = self.canvas.width().saturating_sub(col);
        let dim = self.tone(Kind::Dim);
        let y = self.row(post.net, depth, selected);
        let x = self
            .canvas
            .put(y, col, dim, &util::truncate(&link.display, room));
        self.canvas.link(y, col, x, &link.expanded);
        let act = Act::Link {
            post: post.id.clone(),
            url: link.expanded.clone(),
        };
        self.canvas.target(y, col, x, act);
    }

    /// The quoted post behind its own line: its header and up to three rows of its text.
    fn quote(&mut self, post: &Post, depth: usize, selected: bool) {
        let Some(quoted) = &post.quoted else {
            return;
        };
        let col = TEXT + STEP * depth;
        let (glyph, line) = self.line(quoted.net);
        let act = Act::Quote {
            post: post.id.clone(),
            quoted: quoted.id.clone(),
        };
        let y = self.row(post.net, depth, selected);
        self.canvas.put(y, col, line, glyph);
        let end = self.head(y, col + STEP, quoted, false, false);
        self.canvas.target(y, col, end, act.clone());
        if quoted.text.trim().is_empty() {
            return;
        }
        let dim = self.tone(Kind::Dim);
        let room = self.canvas.width().saturating_sub(col + STEP);
        for text in util::wrap_text(&quoted.text, room).iter().take(QUOTE_ROWS) {
            let y = self.row(post.net, depth, selected);
            self.canvas.put(y, col, line, glyph);
            let end = self.canvas.put(y, col + STEP, dim, text);
            self.canvas.mouse_target(y, col, end, act.clone());
        }
    }

    /// The stats row in words; nothing when there is nothing to count.
    fn words(&mut self, post: &Post, depth: usize, selected: bool) {
        let parts = words(post);
        if parts.is_empty() {
            return;
        }
        let y = self.row(post.net, depth, selected);
        let mut x = TEXT + STEP * depth;
        for (i, (kind, text)) in parts.iter().enumerate() {
            if i > 0 {
                x += 2;
            }
            let kind = self.tone(*kind);
            let start = x;
            x = self.canvas.put(y, x, kind, text);
            if let Some(act) = word_act(text, &post.id) {
                self.canvas.target(y, start, x, act);
            }
        }
    }

    fn profile(&mut self, net: Network, user: &User, selected: bool) {
        let bold = self.tone(Kind::Bold);
        let dim = self.tone(Kind::Dim);
        let link = self.tone(Kind::Link);
        let url = net.profile_url(&user.username);
        let room = self.canvas.width().saturating_sub(TEXT);
        let y = self.row(net, 0, selected);
        let end = self
            .canvas
            .put(y, TEXT, bold, &util::truncate(&user.name, room));
        self.canvas.link(y, TEXT, end, &url);
        self.canvas
            .target(y, TEXT, end, Act::ProfileLink(url.clone()));
        let y = self.row(net, 0, false);
        self.canvas.put(
            y,
            TEXT,
            dim,
            &util::truncate(&format!("@{} · {}", user.username, net.label()), room),
        );
        let y = self.row(net, 0, false);
        let end = self
            .canvas
            .put(y, TEXT, link, &util::truncate(util::bare_url(&url), room));
        self.canvas.link(y, TEXT, end, &url);
        self.canvas
            .target(y, TEXT, end, Act::ProfileLink(url.clone()));
        if let Some(bio) = user
            .description
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            for line in util::wrap_text(&util::clean(bio), room.min(76)) {
                let y = self.row(net, 0, false);
                self.canvas.put(y, TEXT, Kind::Fg, &line);
            }
        }
        let y = self.row(net, 0, false);
        let end = self
            .canvas
            .put(y, TEXT, bold, if self.opts.shortcuts { "o " } else { "" });
        let end = self.canvas.put(y, end, dim, "Open profile");
        self.canvas.target(y, TEXT, end, Act::ProfileLink(url));
        let (y, x) = if end + 4 + "s Share profile".width() <= self.canvas.width() {
            (y, end + 4)
        } else {
            (self.row(net, 0, false), TEXT)
        };
        let end = self
            .canvas
            .put(y, x, bold, if self.opts.shortcuts { "s " } else { "" });
        let end = self.canvas.put(y, end, dim, "Share profile");
        self.canvas
            .target(y, x, end, Act::ShareProfile(net, user.username.clone()));
        self.canvas.push(false);
        let y = self.bare(false);
        let end = self.canvas.put(y, TEXT, bold, "Posts");
        let (summary, _) = render::profile_summary(self.by_id.values().copied(), self.opts.now);
        self.canvas.put(
            y,
            end + 2,
            dim,
            &util::truncate(&summary, self.canvas.width().saturating_sub(end + 2)),
        );
    }
}

/// The act a stats word presses, from its last word; views press nothing.
fn word_act(text: &str, id: &str) -> Option<Act> {
    let id = id.to_string();
    match text.rsplit(' ').next()? {
        "reply" | "replies" => Some(Act::Reply(id)),
        "repost" | "reposts" | "reposted" => Some(Act::Repost(id)),
        "like" | "likes" | "liked" => Some(Act::Like(id)),
        "saved" => Some(Act::Bookmark(id)),
        _ => None,
    }
}

/// The stats as words: the nonzero counts the network shows, what you did in its color, `saved`.
fn words(post: &Post) -> Vec<(Kind, String)> {
    let caps = post.net.caps();
    let m = &post.metrics;
    let state = |on: bool, color: Kind| if on { color } else { Kind::Dim };
    let counts = [
        (caps.post, m.replies, "reply", "replies", Kind::Dim),
        (
            caps.repost,
            m.reposts,
            "repost",
            "reposts",
            state(post.reposted, Kind::Green),
        ),
        (
            caps.like,
            m.likes,
            "like",
            "likes",
            state(post.liked, Kind::Rose),
        ),
        (caps.views, m.views, "view", "views", Kind::Dim),
    ];
    let mut parts: Vec<(Kind, String)> = counts
        .into_iter()
        .filter(|&(shown, n, ..)| caps.counts && shown && n > 0)
        .map(|(_, n, one, many, kind)| {
            let noun = if n == 1 { one } else { many };
            (kind, format!("{} {noun}", compact(n)))
        })
        .collect();
    if !caps.counts {
        if post.reposted {
            parts.push((Kind::Green, "reposted".to_string()));
        }
        if post.liked {
            parts.push((Kind::Rose, "liked".to_string()));
        }
    }
    if post.bookmarked {
        parts.push((Kind::Gold, "saved".to_string()));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hyperlink::Region;
    use crate::model::{Link, Media, MediaKind, Metrics};
    use crate::theme::Density;
    use chrono::{DateTime, Duration, TimeZone, Utc};

    const STATS: &str = "5 replies  2 reposts  9 likes  1.2K views";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
    }

    fn post(id: &str, who: &str, parent: Option<&str>, minutes_ago: i64) -> Post {
        Post {
            id: id.into(),
            net: Network::X,
            url: None,
            cid: None,
            author: User {
                id: who.into(),
                name: who.to_uppercase(),
                username: who.into(),
                avatar_url: None,
                verified: false,
                business: false,
                description: None,
            },
            text: format!("text of {id}"),
            created_at: Some(now() - Duration::minutes(minutes_ago)),
            conversation_id: "c".into(),
            in_reply_to: parent.map(str::to_string),
            quoted: None,
            reposted_by: None,
            metrics: Metrics {
                replies: 5,
                reposts: 2,
                likes: 9,
                quotes: 0,
                bookmarks: 1,
                views: 1200,
            },
            links: Vec::new(),
            media: Vec::new(),
            liked: false,
            bookmarked: false,
            reposted: false,
        }
    }

    /// A root with two direct replies; the second has four children, so one folds.
    fn tree() -> Vec<Post> {
        vec![
            post("r", "root", None, 60),
            post("a1", "amy", Some("r"), 50),
            post("b1", "bob", Some("a1"), 45),
            post("a2", "ann", Some("r"), 40),
            post("d1", "dan", Some("a2"), 30),
            post("d2", "dee", Some("a2"), 25),
            post("d3", "don", Some("a2"), 20),
            post("d4", "dot", Some("a2"), 10),
        ]
    }

    fn opts(level: Detail, selected: Option<usize>, thread: Option<&str>) -> ViewOpts {
        ViewOpts {
            width: 100,
            height: None,
            now: now(),
            selected,
            icons: false,
            thread: thread.map(str::to_string),
            tags: false,
            density: Density::Roomy,
            net_colors: false,
            newest: false,
            profile: None,
            previews: None,
            page: false,
            images: true,
            level,
            shortcuts: true,
            avatars: false,
            overview: false,
            opened: HashSet::new(),
        }
    }

    fn text_of(view: &View) -> Vec<String> {
        view.lines.iter().map(|line| line.plain()).collect()
    }

    #[test]
    fn minimal_draws_a_network_line_and_stats_in_words() {
        let view = render(&tree(), &HashSet::new(), &opts(Detail::Minimal, None, None));
        let drawn = text_of(&view);
        assert_eq!(drawn[0], " ▎ ROOT @root · 1h");
        assert_eq!(drawn[1], " ▎ text of r");
        assert_eq!(drawn[2], format!(" ▎ {STATS}"));
        assert_eq!(drawn[3], "");
        assert_eq!(drawn[4], "   ▎ AMY @amy · 50m");
        assert_eq!(drawn[8], "     ▎ BOB @bob · 45m");
        assert_eq!(drawn[26], format!("     ▎ {STATS}"));
        assert_eq!(drawn[27], "     ▎ Show 1 more replies");
        assert_eq!(drawn.len(), 28);
        assert_eq!(view.rows.len(), 8);
        assert_eq!(view.rows[0], RowSpan { start: 0, end: 3 });
        assert_eq!(view.rows[1], RowSpan { start: 4, end: 7 });
        assert_eq!(view.rows[7], RowSpan { start: 27, end: 28 });
        assert!(view.lines[0]
            .spans
            .iter()
            .any(|span| span.kind == Kind::Net(Network::X) && span.text == "▎"));
        assert!(view.previews.is_empty());
    }

    #[test]
    fn a_root_without_loaded_replies_offers_the_thread() {
        let posts = vec![post("r", "root", None, 60)];
        let view = render(&posts, &HashSet::new(), &opts(Detail::Minimal, None, None));
        assert_eq!(text_of(&view)[3], " ▎ Show this thread");
        assert_eq!(view.rows, vec![RowSpan { start: 0, end: 4 }]);
    }

    #[test]
    fn the_open_thread_root_shows_its_date_and_no_fold() {
        let view = render(
            &tree(),
            &HashSet::new(),
            &opts(Detail::Minimal, None, Some("r")),
        );
        let drawn = text_of(&view);
        assert_eq!(drawn[0], " ▎ ROOT @root");
        assert_eq!(drawn[1], " ▎ text of r");
        assert!(
            drawn[2].starts_with(" ▎ ") && drawn[2].ends_with(", 2026"),
            "{}",
            drawn[2]
        );
        assert_eq!(drawn[3], format!(" ▎ {STATS}"));
        assert_eq!(drawn[5], "   ▎ AMY @amy · 50m");
        assert!(!drawn.iter().any(|line| line.contains("Show this thread")));
    }

    #[test]
    fn the_selected_block_carries_the_bar_on_every_row() {
        let view = render(
            &tree(),
            &HashSet::new(),
            &opts(Detail::Minimal, Some(1), None),
        );
        assert!(view.lines[4..7]
            .iter()
            .all(|line| line.selected && line.bar));
        assert!(!view.lines[3].selected && !view.lines[7].selected);
        assert_eq!(view.focus(), Some(RowSpan { start: 4, end: 7 }));
    }

    #[test]
    fn media_links_and_quotes_take_one_row_each() {
        let mut p = post("p", "pat", None, 60);
        p.text = "look at this".into();
        p.media = vec![Media {
            key: "m".into(),
            kind: MediaKind::Photo,
            url: Some("https://example.com/cat.jpg".into()),
            preview_url: None,
            alt: Some("a  cat\non a mat".into()),
            width: None,
            height: None,
        }];
        p.links = vec![Link {
            short: "https://t.co/x".into(),
            expanded: "https://example.com/page".into(),
            display: "example.com/page".into(),
            title: Some("Example page".into()),
            ..Link::default()
        }];
        p.quoted = Some(Box::new(post("q", "quin", None, 120)));
        let view = render(&[p], &HashSet::new(), &opts(Detail::Minimal, None, None));
        let drawn = text_of(&view);
        assert_eq!(drawn[0], " ▎ PAT @pat · 1h");
        assert_eq!(drawn[1], " ▎ look at this");
        assert_eq!(drawn[2], " ▎ image · a cat on a mat  v view");
        assert_eq!(drawn[3], " ▎ example.com/page");
        assert_eq!(drawn[4], " ▎ ▎ QUIN @quin · 2h");
        assert_eq!(drawn[5], " ▎ ▎ text of q");
        assert_eq!(drawn[6], format!(" ▎ {STATS}"));
        assert_eq!(drawn[7], " ▎ Show this thread");
        let label = "example.com/page";
        assert!(view.links.contains(&Region {
            x: TEXT as u16,
            y: 3,
            width: label.width() as u16,
            url: "https://example.com/page".into(),
        }));
    }

    #[test]
    fn a_long_list_root_stops_at_three_lines_and_keeps_the_image_line() {
        let mut root = post("r", "root", None, 60);
        root.text = "one\ntwo\nthree\nfour\nfive".into();
        root.media = vec![Media {
            key: "m".into(),
            kind: MediaKind::Photo,
            url: Some("https://example.com/cat.jpg".into()),
            preview_url: None,
            alt: Some("a cat".into()),
            width: None,
            height: None,
        }];
        let view = render(
            std::slice::from_ref(&root),
            &HashSet::new(),
            &opts(Detail::Minimal, None, None),
        );
        let drawn = text_of(&view);
        assert_eq!(drawn[1], " ▎ one");
        assert_eq!(drawn[2], " ▎ two");
        assert_eq!(drawn[3], " ▎ three");
        assert!(drawn.iter().any(|line| line.contains("image · a cat")));
        assert!(!drawn.iter().any(|line| line.contains("four")));
        let image = drawn
            .iter()
            .position(|line| line.contains("image ·"))
            .unwrap();
        let more = drawn
            .iter()
            .position(|line| line.contains("Show more"))
            .unwrap();
        let fold = drawn
            .iter()
            .position(|line| line.contains("Show this thread"))
            .unwrap();
        assert!(image < more && more < fold, "{drawn:#?}");
        assert!(view.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.text == "Show more" && span.kind == Kind::Blue)
        }));

        let plain = render(
            std::slice::from_ref(&root),
            &HashSet::new(),
            &opts(Detail::Plain, None, None),
        );
        let show = plain
            .lines
            .iter()
            .find(|line| line.plain().contains("Show more"))
            .expect("plain show more");
        assert!(show.spans.iter().all(|span| span.kind == Kind::Fg));

        let mut opened = HashSet::new();
        opened.insert(root.id.clone());
        let open = render(
            std::slice::from_ref(&root),
            &HashSet::new(),
            &ViewOpts {
                opened,
                ..opts(Detail::Minimal, None, None)
            },
        );
        let drawn = text_of(&open);
        assert!(drawn.iter().any(|line| line.contains("four")));
        assert!(drawn.iter().any(|line| line.contains("Show less")));

        let thread = text_of(&render(
            std::slice::from_ref(&root),
            &HashSet::new(),
            &opts(Detail::Minimal, None, Some("r")),
        ));
        assert!(thread.iter().any(|line| line.contains("five")));
        assert!(!thread.iter().any(|line| line.contains("Show more")));

        let mut reply = post("a", "amy", Some("r"), 50);
        reply.text = "one\ntwo\nthree\nfour\nfive".into();
        let mut expanded = HashSet::new();
        expanded.insert(root.id.clone());
        root.text = "short".into();
        let drawn = text_of(&render(
            &[root.clone(), reply],
            &expanded,
            &opts(Detail::Minimal, None, None),
        ));
        assert!(drawn.iter().any(|line| line.contains("four")));
        assert!(!drawn.iter().any(|line| line.contains("Show more")));

        root.text = "one\ntwo\nthree\nfour".into();
        let mention = text_of(&notifications(
            std::slice::from_ref(&root),
            &HashSet::new(),
            &opts(Detail::Minimal, None, None),
        ));
        assert!(mention.iter().any(|line| line.contains("one")));
        assert!(!mention.iter().any(|line| line.contains("four")));
        assert!(mention.iter().any(|line| line.contains("Show more")));
    }

    #[test]
    fn plain_draws_ascii_lines_names_every_network_and_bolds_only_names() {
        let tagged = ViewOpts {
            tags: true,
            ..opts(Detail::Plain, Some(0), None)
        };
        let view = render(&tree(), &HashSet::new(), &tagged);
        let drawn = text_of(&view);
        assert_eq!(drawn[0], " | ROOT @root · 1h · x");
        assert_eq!(drawn[4], "   | AMY @amy · 50m · x");
        assert!(view
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| matches!(span.kind, Kind::Fg | Kind::Bold)));
        assert!(view.lines[0]
            .spans
            .iter()
            .any(|span| span.kind == Kind::Bold && span.text == "ROOT"));
        let minimal = ViewOpts {
            tags: true,
            ..opts(Detail::Minimal, None, None)
        };
        let drawn = text_of(&render(&tree(), &HashSet::new(), &minimal));
        assert_eq!(drawn[0], " ▎ ROOT @root · 1h · x");
        assert_eq!(
            drawn[4], "   ▎ AMY @amy · 50m",
            "minimal names the network on roots only"
        );
    }

    #[test]
    fn words_count_in_the_singular_and_color_what_you_did() {
        let mut p = post("p", "pat", None, 1);
        p.metrics.replies = 1;
        p.liked = true;
        p.reposted = true;
        p.bookmarked = true;
        assert_eq!(
            words(&p),
            vec![
                (Kind::Dim, "1 reply".to_string()),
                (Kind::Green, "2 reposts".to_string()),
                (Kind::Rose, "9 likes".to_string()),
                (Kind::Dim, "1.2K views".to_string()),
                (Kind::Gold, "saved".to_string()),
            ]
        );
        p.metrics = Metrics::default();
        p.liked = false;
        p.reposted = false;
        p.bookmarked = false;
        assert!(words(&p).is_empty());
    }

    #[test]
    fn mentions_say_so_after_the_header() {
        let posts = vec![post("m", "mo", None, 5)];
        let view = notifications(&posts, &HashSet::new(), &opts(Detail::Minimal, None, None));
        let drawn = text_of(&view);
        assert_eq!(drawn[0], " ▎ MO @mo · 5m  mentioned you");
        assert_eq!(drawn[1], " ▎ text of m");
        assert_eq!(drawn[2], format!(" ▎ {STATS}"));
    }

    #[test]
    fn simple_profiles_keep_the_bio_and_actions_without_boxes_or_icons() {
        let user = User {
            id: "u1".into(),
            name: "Kal Nguyen".into(),
            username: "kal".into(),
            avatar_url: None,
            verified: false,
            business: false,
            description: Some("Writes about terminals.\nSecond line.".into()),
        };
        for level in [Detail::Minimal, Detail::Plain] {
            let with_profile = ViewOpts {
                width: 40,
                profile: Some((Network::Threads, user.clone())),
                ..opts(level, Some(0), None)
            };
            let posts = vec![Post {
                net: Network::Threads,
                ..post("t1", "kal", None, 2)
            }];
            let view = render(&posts, &HashSet::new(), &with_profile);
            let drawn = text_of(&view);
            let text = drawn.join("\n");
            assert!(drawn[0].contains("Kal Nguyen"));
            assert!(text.contains("@kal · threads"));
            assert!(text.contains("threads.com/@kal"));
            assert!(text.contains("Writes about terminals.\n"));
            assert!(text.contains("Second line."));
            assert!(text.contains("1 post loaded"));
            assert!(view
                .targets
                .iter()
                .any(|target| matches!(target.act, Act::ShareProfile(Network::Threads, _))));
            assert!(view.lines.iter().all(|line| line.plain().width() <= 40));
            assert!(!text.contains(['╭', '╮', '╰', '╯']));
            if level == Detail::Plain {
                assert!(text.chars().all(|ch| ch.is_ascii() || ch == '·'));
                assert!(view
                    .lines
                    .iter()
                    .flat_map(|line| &line.spans)
                    .all(|span| matches!(span.kind, Kind::Fg | Kind::Bold)));
            }
        }
    }

    #[test]
    fn the_reader_draws_this_layout_at_minimal_and_plain_only() {
        let posts = tree();
        for (level, first) in [
            (Detail::Minimal, " ▎ ROOT @root · 1h"),
            (Detail::Plain, " | ROOT @root · 1h"),
        ] {
            let view = crate::render::render(&posts, &HashSet::new(), &opts(level, None, None));
            assert_eq!(view.lines[0].plain(), first);
        }
        let standard =
            crate::render::render(&posts, &HashSet::new(), &opts(Detail::Standard, None, None));
        assert!(!standard.lines.iter().any(|line| line.plain().contains('▎')));
    }
}
