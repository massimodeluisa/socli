//! Post bodies from design/: feed rails, the thread detail, mentions, and the painters.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::key::{KeyCode, KeyModifiers};
use chrono::{DateTime, Local, Utc};
use unicode_width::UnicodeWidthStr;

use crate::hyperlink::Region;
use crate::layout::{self, LaidRow};
use crate::minimal;
use crate::model::{Link, Media, MediaKind, Post, Scope, Section, User};
use crate::net::Network;
use crate::preview;
use crate::theme::{self, author_color, compact, Density, Detail, Icons, Theme};

pub use crate::theme::Kind;
use crate::util::{self, ago};

/// Wrapped body lines a list post shows before `Show more`.
pub(crate) const BODY_LINES: usize = 3;

/// `Show more` while the list cap holds, `Show less` once the post is open.
pub(crate) fn more_label(open: bool) -> &'static str {
    if open {
        "Show less"
    } else {
        "Show more"
    }
}

// MARK: grid

/// [`util::clean`] `text` with each newline it keeps turned into a space, since one row can't hold a line break.
fn one_row(text: &str) -> Cow<'_, str> {
    let cleaned = util::clean(text);
    if cleaned.contains('\n') {
        Cow::Owned(cleaned.replace('\n', " "))
    } else {
        cleaned
    }
}

/**
Columns inside the [`margin`], so terminal column = canvas column + margin: the selection bar
at 0, the root avatar at 1 to 5 with its rail at 3, root text at 8, the reply avatar at 7 to 11
with its chain at 9, and reply text at 14. Medium posts skip the letter avatar, so their text
uses [`text_origin`] instead. The profile header keeps [`TX`], beside the letter frame.
*/
pub const TX: usize = 8;
const ROOT_AVATAR: usize = 1;
const ROOT_RAIL: usize = 3;
const ELBOW: usize = 3;
const REPLY_AVATAR: usize = 7;
const CHAIN: usize = 9;
const REPLY_TX: usize = 14;
const CARD_MAX: usize = 58;
const PREVIEW_MAX: usize = 58;
/// The most image boxes one post shows side by side.
const GALLERY_MAX: usize = 4;
/// The narrowest picture shape a gallery box takes, width over height; a taller picture is fit inside.
const ASPECT_MIN: f32 = 0.5;
/// The shape assumed for a picture the network gives no size for.
const ASPECT_UNKNOWN: f32 = 4.0 / 3.0;
/// Rich link cards are this wide, or the room after `TX` when narrower.
const RICH_CARD: usize = 64;
/// Most rows a large rich card's image takes; a 1.91:1 preview fills the card's width under it.
const RICH_IMAGE_MAX: u16 = 18;
/// A small rich card's thumbnail, in cells.
const THUMB_COLS: usize = 12;
const THUMB_ROWS: usize = 6;
const ACTION_STEP: usize = 8;
const ACTION_WIDTH: usize = 7;
/// Below this width the rich action columns contract and omit counts. [`Pen::grid_from`] shifts
/// it with the text column, so medium packs from 36.
const GRID_FROM: usize = TX + 3 * ACTION_STEP + ACTION_WIDTH;
/// A link card narrower than this becomes the single link row minimal draws.
const CARD_MIN: usize = 16;
const STATS: [usize; 4] = [
    TX,
    TX + ACTION_STEP,
    TX + 2 * ACTION_STEP,
    TX + 3 * ACTION_STEP,
];

/// The smallest terminal the reader draws in; `--print` lays narrower widths out at this one.
pub const MIN_COLS: usize = 24;
pub const MIN_ROWS: usize = 8;

/// Terminals at least this wide keep a two-column margin on each side; narrower ones draw to the edge.
const MARGIN_FROM: usize = 60;

/// The blank columns on each side of a terminal `full` columns wide.
pub fn margin(full: usize) -> usize {
    if full >= MARGIN_FROM {
        2
    } else {
        0
    }
}

/// [`margin`] for a detail level. Medium draws to both edges at every width.
pub fn margin_for(full: usize, detail: Detail) -> usize {
    if detail == Detail::Standard {
        0
    } else {
        margin(full)
    }
}

/// Width left for content once the margins are taken off both sides.
pub fn inner_width(full: usize) -> usize {
    full.saturating_sub(2 * margin(full))
}

/// [`inner_width`] for a detail level.
pub fn inner_width_for(full: usize, detail: Detail) -> usize {
    full.saturating_sub(2 * margin_for(full, detail))
}

/// Whether a terminal `full` columns wide gets the narrow layout: one footer row and the sub-bar's chips collapsed.
pub fn narrow(full: usize) -> bool {
    full < MARGIN_FROM
}

/// Canvas column of a root post's text.
///
/// Rich keeps the letter badge, so the name starts at [`TX`]. Medium draws no badge on a post,
/// and the name starts one column after the rail.
fn text_origin(detail: Detail) -> usize {
    if detail == Detail::Standard {
        ROOT_RAIL + 2
    } else {
        TX
    }
}

/// Canvas column of a reply's text. Medium starts one column after the chain.
fn reply_origin(detail: Detail) -> usize {
    if detail == Detail::Standard {
        CHAIN + 2
    } else {
        REPLY_TX
    }
}

#[derive(Clone, Debug)]
pub struct Span {
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct StyledLine {
    pub spans: Vec<Span>,
    pub selected: bool,
    /// Columns `[start, end)` on the selected-row background, for a cursor row inside a panel.
    pub hl: Option<(usize, usize)>,
    /// The ▍ selection bar, drawn in the left margin one column before the content, or over canvas column 0 without a margin; --print leaves it out.
    pub bar: bool,
}

impl StyledLine {
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn blank() -> Self {
        Self::text(Kind::Fg, "")
    }

    /// The row with the reader's glyphs in ASCII at the same width, for the plain level.
    pub fn ascii(&self) -> Self {
        let spans = self
            .spans
            .iter()
            .map(|span| Span {
                kind: span.kind,
                text: span.text.chars().map(theme::ascii_char).collect(),
            })
            .collect();
        Self {
            spans,
            ..self.clone()
        }
    }

    /// One span of `text`, cleaned for one row by [`one_row`].
    pub fn text(kind: Kind, text: &str) -> Self {
        Self {
            spans: vec![Span {
                kind,
                text: one_row(text).into_owned(),
            }],
            selected: false,
            hl: None,
            bar: false,
        }
    }
}

/// Canvas rows `[start, end)` drawn for one laid row; `View::rows[i]` belongs to laid row `i`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct View {
    pub lines: Vec<StyledLine>,
    pub rows: Vec<RowSpan>,
    /// Link targets in canvas coordinates: `y` indexes `lines`, `x` counts from the content's first column.
    pub links: Vec<Region>,
    /// Inline image boxes, in the same coordinates as `links`.
    pub previews: Vec<Slot>,
    /// Pages of rich link cards the network sent no preview image for, for the reader to unfurl.
    pub unfurls: Vec<String>,
    /// Pressable runs, in the same coordinates as `links`.
    pub targets: Vec<Target>,
}

/// The box an inline image fills once it loads; the rows hold its loading frame until then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub url: String,
    pub x: usize,
    pub y: usize,
    pub cols: usize,
    pub rows: usize,
    /// A profile picture over a 5×3 letter box, which keeps its letter until the picture loads.
    pub avatar: bool,
    /// A business account's picture, drawn as a rounded square instead of a circle.
    pub square: bool,
}

impl View {
    /// Rows of the selected block, if anything is selected.
    pub fn focus(&self) -> Option<RowSpan> {
        self.rows
            .iter()
            .copied()
            .find(|r| r.end > r.start && self.lines.get(r.start).is_some_and(|l| l.selected))
    }
}

// MARK: targets

/// The band a target sits in; the focus ring steps through the bands in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Zone {
    Header,
    Subbar,
    /// The single pane, or the feed side of the split.
    BodyLeft,
    BodyRight,
    Footer,
}

/// What pressing a target does, with ⏎ on the focus ring or with a click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Back,
    Crumb(usize),
    Section(Section),
    Compose,
    Scope(Scope),
    /// The signed-in account named in the header.
    MyProfile(Network),
    /// The accounts page, from the All header or a signed-out label.
    Accounts,
    /// Sign in to this network from the welcome mark.
    SignIn(Network),
    /// Open the local demo from the welcome mark.
    EnterDemo,
    /// The feed tab at this index, from 0.
    Tab(usize),
    /// A key hint; a click runs the key.
    Key(KeyCode),
    Input(KeyCode, KeyModifiers),
    Author(String),
    Thread(String),
    Link {
        post: String,
        url: String,
    },
    Mention {
        post: String,
        handle: String,
    },
    Hashtag {
        post: String,
        tag: String,
    },
    Quote {
        post: String,
        quoted: String,
    },
    Media(String),
    /// One image box of a post's row of images: `index` is the picture the viewer opens at.
    MediaAt {
        post: String,
        index: usize,
    },
    Reply(String),
    Repost(String),
    Like(String),
    Bookmark(String),
    /// A fold row: the parent whose replies it hides, or the root of `Show this thread`.
    Fold(String),
    /// `Show more` or `Show less` on a list post whose text is longer than [`BODY_LINES`].
    More(String),
    ProfileLink(String),
    ShareProfile(Network, String),
    /// Controls that did not fit on their row. Choosing one runs its act.
    Overflow(Vec<OverflowItem>),
}

/// One hidden control in an [`Act::Overflow`] list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverflowItem {
    pub parts: Vec<(Kind, String)>,
    pub act: Box<Act>,
}

impl Act {
    /// The post a post-bound act belongs to.
    pub fn post(&self) -> Option<&str> {
        match self {
            Act::Author(id)
            | Act::Thread(id)
            | Act::Media(id)
            | Act::MediaAt { post: id, .. }
            | Act::Reply(id)
            | Act::Repost(id)
            | Act::Like(id)
            | Act::Bookmark(id)
            | Act::Fold(id)
            | Act::More(id)
            | Act::Link { post: id, .. }
            | Act::Mention { post: id, .. }
            | Act::Hashtag { post: id, .. }
            | Act::Quote { post: id, .. } => Some(id),
            _ => None,
        }
    }
}

/// A pressable run of cells, in canvas coordinates until the reader puts it on the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub act: Act,
    pub zone: Zone,
    /// Tab stops on it; key hints and the other rows of a box are for the mouse only.
    pub tab: bool,
}

pub struct ViewOpts {
    pub width: usize,
    pub height: Option<usize>,
    pub now: DateTime<Utc>,
    pub selected: Option<usize>,
    pub icons: bool,
    /// Conversation id of the open thread; its root gets the detail layout.
    pub thread: Option<String>,
    /// Name each post's network after its time, for timelines that merge networks.
    pub tags: bool,
    pub density: Density,
    /// Avatars, rails and folds in the network's color instead of the author's.
    pub net_colors: bool,
    /// Thread replies newest first.
    pub newest: bool,
    /// The account whose posts these are, drawn as a header above them.
    pub profile: Option<(Network, User)>,
    /// Cell width over height while inline previews are on; a first image then replaces the media card.
    pub previews: Option<f32>,
    /// The browser reader. Its pictures use the page cap and keep their shape.
    pub page: bool,
    /// Whether `v` can show images (`ui.images` is not off); the media row then ends in `v view`.
    pub images: bool,
    /// How much art the reader draws; minimal and plain trade avatars and rails for a network line.
    pub level: Detail,
    pub shortcuts: bool,
    /// Profile pictures over the letter boxes: rich, with a terminal that draws real pictures.
    pub avatars: bool,
    /// Lay the posts out as the split's feed pane: roots alone (see `layout::overview`).
    pub overview: bool,
    /// List posts whose text is open past [`BODY_LINES`].
    pub opened: HashSet<String>,
}

pub(crate) fn profile_summary<'a>(
    posts: impl Iterator<Item = &'a Post>,
    now: DateTime<Utc>,
) -> (String, Option<String>) {
    let mut count = 0;
    let mut media = 0;
    let mut latest = None;
    for post in posts {
        count += 1;
        media += usize::from(!post.media.is_empty());
        latest = latest.max(post.created_at);
    }
    let noun = if count == 1 { "post" } else { "posts" };
    let mut summary = format!("{count} {noun} loaded");
    if media > 0 {
        summary.push_str(&format!(" · {media} with media"));
    }
    (
        summary,
        latest.map(|date| format!("latest {}", util::ago_long(date, now))),
    )
}

// MARK: canvas

/// What one canvas cell shows: a one-scalar grapheme, a longer cluster, or the second column of a wide one.
#[derive(Clone)]
enum Glyph {
    Char(char),
    Cluster(Box<str>),
    Cont,
}

impl Glyph {
    fn of(grapheme: &str) -> Self {
        let mut chars = grapheme.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => Glyph::Char(ch),
            _ => Glyph::Cluster(grapheme.into()),
        }
    }
}

#[derive(Clone)]
struct Cell {
    kind: Kind,
    glyph: Glyph,
}

pub struct Canvas {
    width: usize,
    rows: Vec<Vec<Cell>>,
    selected: Vec<bool>,
    hl: Vec<Option<(usize, usize)>>,
    bar: Vec<bool>,
    links: Vec<Region>,
    targets: Vec<Target>,
}

impl Canvas {
    pub fn new(width: usize) -> Self {
        Self {
            width: width.max(20),
            rows: Vec::new(),
            selected: Vec::new(),
            hl: Vec::new(),
            bar: Vec::new(),
            links: Vec::new(),
            targets: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn bar(&mut self, row: usize) {
        if let Some(bar) = self.bar.get_mut(row) {
            *bar = true;
        }
    }

    pub fn push(&mut self, selected: bool) -> usize {
        self.rows.push(vec![
            Cell {
                kind: Kind::Fg,
                glyph: Glyph::Char(' '),
            };
            self.width
        ]);
        self.selected.push(selected);
        self.hl.push(None);
        self.bar.push(false);
        self.rows.len() - 1
    }

    /// Gives columns `[start, end)` of `row` the selected-row background.
    pub fn highlight(&mut self, row: usize, start: usize, end: usize) {
        if let Some(hl) = self.hl.get_mut(row) {
            *hl = Some((start, end.min(self.width)));
        }
    }

    /**
    Writes `text`, cleaned for one row by [`one_row`], from `col` one grapheme at a time and returns
    the column just after it. A grapheme takes as many cells as it is wide, and a zero-width one joins
    the cell before it. The write stops at the first grapheme that doesn't fit.
    */
    pub fn put(&mut self, row: usize, mut col: usize, kind: Kind, text: &str) -> usize {
        let text = one_row(text);
        if row >= self.rows.len() {
            return col + text.width();
        }
        for (grapheme, w) in util::graphemes(&text) {
            if w == 0 {
                self.join(row, col, grapheme);
                continue;
            }
            if col + w > self.width {
                break;
            }
            self.split_wide(row, col, col + w);
            let cells = &mut self.rows[row];
            cells[col] = Cell {
                kind,
                glyph: Glyph::of(grapheme),
            };
            for cell in &mut cells[col + 1..col + w] {
                *cell = Cell {
                    kind,
                    glyph: Glyph::Cont,
                };
            }
            col += w;
        }
        col
    }

    /// Adds a zero-width grapheme to the cell before `col`; at the row's start there is none, and it is dropped.
    fn join(&mut self, row: usize, col: usize, grapheme: &str) {
        let cells = &mut self.rows[row];
        if col == 0 || col > cells.len() {
            return;
        }
        let mut lead = col - 1;
        while lead > 0 && matches!(cells[lead].glyph, Glyph::Cont) {
            lead -= 1;
        }
        let glyph = &mut cells[lead].glyph;
        *glyph = match glyph {
            Glyph::Char(ch) => Glyph::Cluster(format!("{ch}{grapheme}").into()),
            Glyph::Cluster(cluster) => Glyph::Cluster(format!("{cluster}{grapheme}").into()),
            Glyph::Cont => return,
        };
    }

    /// Blanks the halves of wide graphemes that a write over columns `[start, end)` cuts into.
    fn split_wide(&mut self, row: usize, start: usize, end: usize) {
        let cells = &mut self.rows[row];
        let mut lead = start;
        while lead > 0 && matches!(cells[lead].glyph, Glyph::Cont) {
            lead -= 1;
        }
        for cell in &mut cells[lead..start] {
            cell.glyph = Glyph::Char(' ');
        }
        for cell in cells[end..].iter_mut() {
            if !matches!(cell.glyph, Glyph::Cont) {
                break;
            }
            cell.glyph = Glyph::Char(' ');
        }
    }

    /// Links columns `[start, end)` of `row` to `url`.
    pub fn link(&mut self, row: usize, start: usize, end: usize, url: &str) {
        let end = end.min(self.width);
        if row >= self.rows.len() || end <= start || url.is_empty() {
            return;
        }
        let (Ok(x), Ok(y), Ok(width)) = (
            u16::try_from(start),
            u16::try_from(row),
            u16::try_from(end - start),
        ) else {
            return;
        };
        self.links.push(Region {
            x,
            y,
            width,
            url: url.to_string(),
        });
    }

    /// Makes columns `[start, end)` of `row` a tab stop that does `act`.
    pub fn target(&mut self, row: usize, start: usize, end: usize, act: Act) {
        self.mark(row, start, end, act, true);
    }

    /// Like `target`, for the mouse only: Tab passes it by.
    pub fn mouse_target(&mut self, row: usize, start: usize, end: usize, act: Act) {
        self.mark(row, start, end, act, false);
    }

    fn mark(&mut self, row: usize, start: usize, end: usize, act: Act, tab: bool) {
        let end = end.min(self.width);
        if row >= self.rows.len() || end <= start {
            return;
        }
        let (Ok(x), Ok(y), Ok(width)) = (
            u16::try_from(start),
            u16::try_from(row),
            u16::try_from(end - start),
        ) else {
            return;
        };
        self.targets.push(Target {
            x,
            y,
            width,
            act,
            zone: Zone::BodyLeft,
            tab,
        });
    }

    /// Writes the parts in order so the last one ends at column `end`.
    pub fn put_right(&mut self, row: usize, end: usize, parts: &[(Kind, &str)]) {
        let width: usize = parts.iter().map(|(_, text)| text.width()).sum();
        let Some(mut col) = (end + 1).checked_sub(width) else {
            return;
        };
        for (kind, text) in parts {
            col = self.put(row, col, *kind, text);
        }
    }

    pub fn finish(self) -> Vec<StyledLine> {
        self.finish_linked().0
    }

    /// The lines and the link regions on them.
    pub fn finish_linked(self) -> (Vec<StyledLine>, Vec<Region>) {
        let (lines, links, _) = self.finish_targeted();
        (lines, links)
    }

    /// The lines, the link regions and the targets on them.
    pub fn finish_targeted(self) -> (Vec<StyledLine>, Vec<Region>, Vec<Target>) {
        let lines = self
            .rows
            .into_iter()
            .zip(self.selected)
            .zip(self.hl)
            .zip(self.bar)
            .map(|(((mut cells, selected), hl), bar)| {
                while cells
                    .last()
                    .is_some_and(|c| matches!(c.glyph, Glyph::Char(' ')))
                {
                    cells.pop();
                }
                let mut spans: Vec<Span> = Vec::new();
                let mut buf = [0u8; 4];
                for cell in &cells {
                    let text: &str = match &cell.glyph {
                        Glyph::Char(ch) => ch.encode_utf8(&mut buf),
                        Glyph::Cluster(cluster) => cluster,
                        Glyph::Cont => continue,
                    };
                    match spans.last_mut() {
                        Some(span) if span.kind == cell.kind => span.text.push_str(text),
                        _ => spans.push(Span {
                            kind: cell.kind,
                            text: text.to_string(),
                        }),
                    }
                }
                if spans.is_empty() {
                    spans.push(Span {
                        kind: Kind::Fg,
                        text: String::new(),
                    });
                }
                StyledLine {
                    spans,
                    selected,
                    hl,
                    bar,
                }
            })
            .collect();
        (lines, self.links, self.targets)
    }

    pub fn finish_line(self) -> StyledLine {
        self.finish()
            .into_iter()
            .next()
            .unwrap_or_else(StyledLine::blank)
    }
}

/// A card under the last row while the next page loads. Medium and rich draw a box. Minimal, plain, and a narrow width write the word only.
pub fn loading_card(detail: Detail, ascii: bool, width: usize) -> Vec<StyledLine> {
    let label = if ascii { "loading..." } else { "loading…" };
    let origin = text_origin(detail);
    let room = width.saturating_sub(origin);
    let need = label.width() + 4;
    let boxed = matches!(detail, Detail::Standard | Detail::Rich) && room >= need;
    if !boxed {
        return vec![
            StyledLine::blank(),
            StyledLine::text(Kind::Dim, &format!(" {label}")),
        ];
    }
    let cw = need.max(28).min(room);
    let mut canvas = Canvas::new(width);
    canvas.push(false);
    let bar = "─".repeat(cw.saturating_sub(2));
    let y = canvas.push(false);
    canvas.put(y, origin, Kind::Line, &format!("╭{bar}╮"));
    let y = canvas.push(false);
    canvas.put(y, origin, Kind::Line, "│");
    canvas.put(y, origin + 2, Kind::Dim, label);
    canvas.put(y, origin + cw - 1, Kind::Line, "│");
    let y = canvas.push(false);
    canvas.put(y, origin, Kind::Line, &format!("╰{bar}╯"));
    canvas.finish()
}

// MARK: feed

pub fn render(posts: &[Post], expanded: &HashSet<String>, opts: &ViewOpts) -> View {
    if matches!(opts.level, Detail::Minimal | Detail::Plain) {
        return minimal::render(posts, expanded, opts);
    }
    let mut laid = layout::lay_for(posts, expanded, opts.newest, opts.overview);
    if opts.profile.is_some() {
        laid = layout::profiled(laid);
    }
    let mut pen = Pen::new(posts, opts);
    let detail = opts.thread.as_deref().map(|focus| {
        laid.iter()
            .position(|r| r.depth == 0 && layout::post_id(r) == Some(focus))
            .unwrap_or(0)
    });
    let mut rows = Vec::with_capacity(laid.len());
    let mut i = 0;
    while i < laid.len() {
        if layout::is_gap(&laid[i]) {
            let start = pen.canvas.len();
            pen.blanks(opts.density.rows(1, 1, 0));
            rows.push(RowSpan {
                start,
                end: pen.canvas.len(),
            });
            i += 1;
            continue;
        }
        if let (layout::Target::Profile, Some((net, user))) = (&laid[i].target, &opts.profile) {
            let start = pen.canvas.len();
            pen.profile(*net, user, opts.selected == Some(i));
            rows.push(RowSpan {
                start,
                end: pen.canvas.len(),
            });
            i += 1;
            continue;
        }
        let end = laid[i..]
            .iter()
            .position(layout::is_gap)
            .map_or(laid.len(), |n| i + n);
        pen.tree(&laid[i..end], i, detail == Some(i), &mut rows);
        i = end;
    }
    let (mut lines, links, targets) = pen.canvas.finish_targeted();
    for row in &mut rows {
        while row.end > row.start + 1
            && lines[row.end - 1]
                .plain()
                .chars()
                .all(|c| c.is_whitespace() || "│╎┊┆╵╷▍┃".contains(c))
        {
            row.end -= 1;
            lines[row.end].selected = false;
            lines[row.end].bar = false;
        }
    }
    View {
        lines,
        rows,
        links,
        previews: pen.slots,
        unfurls: pen.unfurls,
        targets,
    }
}

struct Pen<'a> {
    canvas: Canvas,
    icons: Icons,
    opts: &'a ViewOpts,
    by_id: HashMap<&'a str, &'a Post>,
    slots: Vec<Slot>,
    unfurls: Vec<String>,
}

/// How one reply joins the rails around it.
struct Joints {
    up: bool,
    down: bool,
    /// `Some(true)` for `├╌╌╌`, `Some(false)` for the closing `╰╌╌╌`; depth-1 replies only.
    elbow: Option<bool>,
    /// Root rail on the avatar top row.
    rail_top: bool,
    /// Root rail on every other row.
    rail: bool,
}

impl<'a> Pen<'a> {
    fn new(posts: &'a [Post], opts: &'a ViewOpts) -> Self {
        Self {
            canvas: Canvas::new(opts.width),
            icons: Icons::new(opts.icons),
            opts,
            by_id: posts.iter().map(|p| (p.id.as_str(), p)).collect(),
            slots: Vec::new(),
            unfurls: Vec::new(),
        }
    }

    fn right(&self) -> usize {
        self.canvas.width - 1
    }

    fn tx(&self) -> usize {
        text_origin(self.opts.level)
    }

    fn reply_tx(&self) -> usize {
        reply_origin(self.opts.level)
    }

    /// [`STATS`] on this level's text column. Rich keeps the constant. Medium moves left with the rail.
    fn stat_cols(&self) -> [usize; 4] {
        let shift = TX.saturating_sub(self.tx());
        STATS.map(|col| col - shift)
    }

    /// Width where the four action cells keep [`ACTION_STEP`]. A narrower row packs them.
    fn grid_from(&self) -> usize {
        GRID_FROM.saturating_sub(TX.saturating_sub(self.tx()))
    }

    fn row(&mut self, selected: bool) -> usize {
        let y = self.canvas.push(selected);
        if selected {
            self.canvas.bar(y);
        }
        y
    }

    fn rail(&mut self, y: usize, color: Kind) {
        self.canvas.put(y, ROOT_RAIL, color, "╎");
    }

    fn rail_row(&mut self, selected: bool, color: Kind) -> usize {
        let y = self.row(selected);
        self.rail(y, color);
        y
    }

    /// A row under a root, with its rail when the root has one.
    fn line(&mut self, selected: bool, rail: Option<Kind>) -> usize {
        match rail {
            Some(color) => self.rail_row(selected, color),
            None => self.row(selected),
        }
    }

    fn blanks(&mut self, n: usize) {
        for _ in 0..n {
            self.canvas.push(false);
        }
    }

    /// The color of a post's avatar and rails.
    fn color(&self, post: &Post) -> Kind {
        if self.opts.net_colors {
            post.net.accent()
        } else {
            author_color(&post.author.username)
        }
    }

    fn profile(&mut self, net: Network, user: &User, selected: bool) {
        let color = if self.opts.net_colors {
            net.accent()
        } else {
            author_color(&user.username)
        };
        let compact = self.opts.height.is_some_and(|height| height < 24);
        let right = self.canvas.width.saturating_sub(1);
        let room = right.saturating_sub(TX + 2);
        let url = net.profile_url(&user.username);
        let top = self.row(selected);
        self.canvas.put(
            top,
            0,
            Kind::Line,
            &format!("╭{}╮", "─".repeat(right.saturating_sub(1))),
        );
        self.canvas.put(top, 2, Kind::Bold, " Profile ");
        let label = format!(" {} ", net.title());
        self.canvas.put(
            top,
            right.saturating_sub(label.width() + 2),
            net.accent(),
            &label,
        );

        let y = self.profile_line(right);
        avatar_top(&mut self.canvas, y, 2, color, false);
        self.avatar_slot(user.avatar_url.as_deref(), y, 2, user.business);
        let name_room = room.saturating_sub(if user.verified { 2 } else { 0 });
        let end = self
            .canvas
            .put(y, TX, Kind::Bold, &util::truncate(&user.name, name_room));
        if user.verified {
            self.canvas.put(y, end + 1, Kind::Blue, self.icons.verified);
        }
        self.canvas.link(y, TX, end, &url);
        self.canvas
            .target(y, TX, end, Act::ProfileLink(url.clone()));

        let y = self.profile_line(right);
        self.canvas.put(y, 2, color, "│");
        self.canvas.put(y, 4, Kind::Bold, &user.badge().to_string());
        self.canvas.put(y, 6, color, "│");
        self.canvas.put(
            y,
            TX,
            Kind::Dim,
            &util::truncate(&format!("@{}", user.username), room),
        );
        let y = self.profile_line(right);
        avatar_bottom(&mut self.canvas, y, 2, color, false);
        let end = self.canvas.put(
            y,
            TX,
            Kind::Link,
            &util::truncate(util::bare_url(&url), room),
        );
        self.canvas.link(y, TX, end, &url);
        self.canvas
            .target(y, TX, end, Act::ProfileLink(url.clone()));

        if let Some(bio) = user
            .description
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            if !compact {
                self.profile_line(right);
            }
            for line in util::wrap_text(&util::clean(bio), room.min(76)) {
                let y = self.profile_line(right);
                self.canvas.put(y, TX, Kind::Fg, &line);
            }
        }
        if !compact {
            self.profile_line(right);
        }
        let y = self.profile_line(right);
        let end = self.canvas.put(
            y,
            TX,
            Kind::Bold,
            if self.opts.shortcuts { "o " } else { "" },
        );
        let end = self.canvas.put(y, end, Kind::Dim, "Open profile");
        self.canvas.target(y, TX, end, Act::ProfileLink(url));
        let (y, x) = if end + 4 + "s Share profile".width() <= right.saturating_sub(1) {
            (y, end + 4)
        } else {
            (self.profile_line(right), TX)
        };
        let end = self.canvas.put(
            y,
            x,
            Kind::Bold,
            if self.opts.shortcuts { "s " } else { "" },
        );
        let end = self.canvas.put(y, end, Kind::Dim, "Share profile");
        self.canvas
            .target(y, x, end, Act::ShareProfile(net, user.username.clone()));
        let y = self.row(false);
        self.canvas.put(
            y,
            0,
            Kind::Line,
            &format!("╰{}╯", "─".repeat(right.saturating_sub(1))),
        );
        if !compact {
            self.blanks(1);
        }
        let y = self.row(false);
        self.canvas.put(y, 0, Kind::Bold, "Posts");
        let (summary, latest) = profile_summary(self.by_id.values().copied(), self.opts.now);
        let end = self.canvas.put(
            y,
            TX,
            Kind::Dim,
            &util::truncate(&summary, self.canvas.width.saturating_sub(TX)),
        );
        if let Some(latest) = latest.filter(|text| end + text.width() + 3 < self.canvas.width) {
            self.canvas
                .put(y, self.canvas.width - latest.width(), Kind::Dim, &latest);
        }
        if !compact {
            let y = self.row(false);
            self.canvas
                .put(y, 0, Kind::Line, &"─".repeat(self.canvas.width));
        }
    }

    fn profile_line(&mut self, right: usize) -> usize {
        let y = self.row(false);
        self.canvas.put(y, 0, Kind::Line, "│");
        self.canvas.put(y, right, Kind::Line, "│");
        y
    }

    /// One root and its replies: `tree[0]` is the root, `base` its index in the laid rows.
    fn tree(&mut self, tree: &[LaidRow], base: usize, detail: bool, rows: &mut Vec<RowSpan>) {
        let Some(root) = layout::post_id(&tree[0]).and_then(|id| self.by_id.get(id).copied())
        else {
            for _ in tree {
                let y = self.canvas.push(false);
                rows.push(RowSpan {
                    start: y,
                    end: y + 1,
                });
            }
            return;
        };
        let root_color = self.color(root);
        let has_replies = tree[0].own_rail && tree.len() > 1;
        let start = self.canvas.len();
        let selected = self.opts.selected == Some(base);
        if detail {
            self.detail_root(root, selected, has_replies);
        } else {
            self.root(root, selected, has_replies, tree[0].folded);
        }
        rows.push(RowSpan {
            start,
            end: self.canvas.len(),
        });
        if !has_replies {
            return;
        }
        for _ in 0..self.opts.density.rows(1, 1, 0) {
            self.rail_row(false, root_color);
        }

        let last_d1 = tree
            .iter()
            .rposition(|r| r.depth == 1 && matches!(r.target, layout::Target::Post(_)))
            .unwrap_or(0);
        let mut above = root_color;
        for (k, row) in tree.iter().enumerate().skip(1) {
            let selected = self.opts.selected == Some(base + k);
            let start = self.canvas.len();
            match &row.target {
                layout::Target::Post(id) => {
                    if let Some(post) = self.by_id.get(id.as_str()).copied() {
                        let joints = Joints {
                            up: k > 1,
                            down: k + 1 < tree.len(),
                            elbow: (row.depth == 1).then_some(k < last_d1),
                            rail_top: k <= last_d1,
                            rail: k < last_d1,
                        };
                        self.reply(post, &joints, root_color, selected);
                        above = self.color(post);
                    }
                }
                layout::Target::Fold { hidden, parent } => {
                    let y = self.row(selected);
                    if k < last_d1 {
                        self.rail(y, root_color);
                    }
                    self.canvas.put(y, CHAIN, above, "○");
                    let reply_tx = self.reply_tx();
                    let end = self.canvas.put(
                        y,
                        reply_tx,
                        Kind::Dim,
                        &format!("Show {hidden} more replies"),
                    );
                    self.canvas.target(y, CHAIN, end, Act::Fold(parent.clone()));
                }
                layout::Target::Gap | layout::Target::Profile => {}
            }
            rows.push(RowSpan {
                start,
                end: self.canvas.len().max(start),
            });
        }
    }

    /**
    A feed root. Roomy always hangs a rail under the avatar; cozy and compact close the avatar when
    nothing follows, and put the stats on its bottom row when the body is one line with no cards.
    */
    fn root(&mut self, post: &Post, selected: bool, has_replies: bool, folded: usize) {
        let color = self.color(post);
        let tx = self.tx();
        let density = self.opts.density;
        let body = util::wrap_text(&post.text, self.canvas.width.saturating_sub(tx));
        let more = !has_replies && (folded > 0 || post.metrics.replies > 0);
        let rail = (density == Density::Roomy || has_replies || more).then_some(color);
        let cards = has_cards(post);
        if let Some(by) = &post.reposted_by {
            let y = self.row(selected);
            let x = self.canvas.put(y, tx, Kind::Dim, self.icons.repost);
            self.canvas
                .put(y, x + 1, Kind::Dim, &format!("@{by} reposted"));
        }
        let y = self.row(selected);
        self.post_avatar_top(y, ROOT_AVATAR, color, false);
        self.avatar_slot(
            post.author.avatar_url.as_deref(),
            y,
            ROOT_AVATAR,
            post.author.business,
        );
        let tags = self.opts.tags;
        let room = self.head_room(tx);
        let split = self.author_width(post, true, tags) > room;
        if split {
            self.name_row(y, tx, post, room);
        } else {
            self.head(y, tx, post, tags);
        }
        let right = self.right();
        self.canvas
            .put_right(y, right, &[(Kind::Dim, self.icons.more)]);

        let opened = self.opts.opened.contains(&post.id);
        let (limit, cut) = body_window(body.len(), opened);
        let y = self.row(selected);
        self.post_avatar_mid(y, ROOT_AVATAR, color, post, false);
        let drawn = usize::from(!split);
        if split {
            self.byline(y, tx, post, tags, room);
        } else if limit > 0 {
            paint_body(
                &mut self.canvas,
                y,
                tx,
                body.first().map_or("", String::as_str),
                post,
            );
        }

        let y = self.row(selected);
        self.post_avatar_bottom(y, ROOT_AVATAR, color, rail.is_some());
        self.avatar_targets(y - 2, ROOT_AVATAR, &post.id);
        if body.len() <= drawn && !cards && !has_replies && density != Density::Roomy {
            self.actions(y, post, true);
            if more {
                self.fold(selected, color, &post.id, folded);
            }
            return;
        }
        if drawn < limit {
            paint_body(
                &mut self.canvas,
                y,
                tx,
                body.get(drawn).map_or("", String::as_str),
                post,
            );
        }
        for (index, text) in body.iter().enumerate().skip(drawn + 1) {
            if index >= limit {
                break;
            }
            let y = self.line(selected, rail);
            paint_body(&mut self.canvas, y, tx, text, post);
        }
        let spacer = if self.boxed_cards(post) {
            density.rows(1, 1, 0)
        } else {
            density.rows(1, 0, 0)
        };
        for _ in 0..spacer {
            self.line(selected, rail);
        }
        self.cards(post, selected, rail, false);
        let y = self.line(selected, rail);
        self.actions(y, post, true);
        if cut {
            self.more_row(selected, rail, &post.id, opened);
        }
        if more {
            self.fold(selected, color, &post.id, folded);
        }
    }

    fn post_avatar_top(&mut self, y: usize, col: usize, color: Kind, up: bool) {
        if self.opts.level == Detail::Rich {
            avatar_top(&mut self.canvas, y, col, color, up);
        } else if up {
            self.canvas.put(y, col + 2, color, "╎");
        }
    }

    fn post_avatar_mid(&mut self, y: usize, col: usize, color: Kind, post: &Post, join: bool) {
        if self.opts.level == Detail::Rich {
            avatar_mid(&mut self.canvas, y, col, color, post, join);
        } else if join {
            self.canvas.put(y, col, color, "╌╌╎");
        }
    }

    fn post_avatar_bottom(&mut self, y: usize, col: usize, color: Kind, down: bool) {
        if self.opts.level == Detail::Rich {
            avatar_bottom(&mut self.canvas, y, col, color, down);
        } else if down {
            self.canvas.put(y, col + 2, color, "╎");
        }
    }

    /// Reserves the letter box from row `y`, column `col` for the author's picture when avatars are on.
    fn avatar_slot(&mut self, url: Option<&str>, y: usize, col: usize, square: bool) {
        if let Some(url) = url.filter(|_| self.opts.avatars) {
            self.slots.push(Slot {
                url: url.to_string(),
                x: col,
                y,
                cols: 5,
                rows: 3,
                avatar: true,
                square,
            });
        }
    }

    /**
    `○ Show this thread` under the root `id` whose replies aren't loaded, or `○ Show N replies`
    for the `folded` loaded replies the split's feed pane leaves out.
    */
    fn fold(&mut self, selected: bool, color: Kind, id: &str, folded: usize) {
        let y = self.row(selected);
        let tx = self.tx();
        self.canvas.put(y, ROOT_RAIL, color, "○");
        let end = self.canvas.put(y, tx, Kind::Dim, &replies_label(folded));
        self.canvas
            .target(y, ROOT_RAIL, end, Act::Fold(id.to_string()));
    }

    /// `Show more` or `Show less` on its own row, after the stats and before a fold.
    fn more_row(&mut self, selected: bool, rail: Option<Kind>, id: &str, open: bool) {
        let y = self.line(selected, rail);
        let tx = self.tx();
        let label = more_label(open);
        let end = self.canvas.put(y, tx, Kind::Blue, label);
        self.canvas.target(y, tx, end, Act::More(id.to_string()));
    }

    /// The 5×3 avatar box from row `top`, column `col`: the letter row is the tab stop.
    fn avatar_targets(&mut self, top: usize, col: usize, id: &str) {
        if self.opts.level == Detail::Standard {
            return;
        }
        let act = Act::Author(id.to_string());
        self.canvas.mouse_target(top, col, col + 5, act.clone());
        self.canvas.target(top + 1, col, col + 5, act.clone());
        self.canvas.mouse_target(top + 2, col, col + 5, act);
    }

    /// The open thread's root. Roomy gives the handle its own row; cozy and compact put it beside the name.
    fn detail_root(&mut self, post: &Post, selected: bool, has_replies: bool) {
        let color = self.color(post);
        let tx = self.tx();
        let density = self.opts.density;
        let roomy = density == Density::Roomy;
        let rail = (roomy || has_replies).then_some(color);
        let right = self.right();
        let body = util::wrap_text(&post.text, self.canvas.width.saturating_sub(tx));
        let handle = format!("@{}", post.author.username);

        let profile = profile_url(post);
        let y = self.row(selected);
        self.post_avatar_top(y, ROOT_AVATAR, color, false);
        self.avatar_slot(
            post.author.avatar_url.as_deref(),
            y,
            ROOT_AVATAR,
            post.author.business,
        );
        let room = self.head_room(tx);
        let split = !roomy && self.author_width(post, false, false) > room;
        let mut x = self.name(y, tx, post, room);
        if !roomy && !split {
            x = self.canvas.put(y, x + 1, Kind::Dim, &handle);
        }
        self.canvas.link(y, tx, x, &profile);
        self.canvas.target(y, tx, x, Act::Author(post.id.clone()));
        self.canvas
            .put_right(y, right, &[(Kind::Dim, self.icons.more)]);

        let y = self.row(selected);
        self.post_avatar_mid(y, ROOT_AVATAR, color, post, false);
        if roomy || split {
            let x = self
                .canvas
                .put(y, tx, Kind::Dim, &util::truncate(&handle, room));
            self.canvas.link(y, tx, x, &profile);
            self.canvas
                .mouse_target(y, tx, x, Act::Author(post.id.clone()));
        } else {
            paint_body(
                &mut self.canvas,
                y,
                tx,
                body.first().map_or("", String::as_str),
                post,
            );
        }

        let y = self.row(selected);
        self.post_avatar_bottom(y, ROOT_AVATAR, color, rail.is_some());
        self.avatar_targets(y - 2, ROOT_AVATAR, &post.id);
        let rest = if roomy {
            &body[..]
        } else {
            let drawn = usize::from(!split);
            paint_body(
                &mut self.canvas,
                y,
                tx,
                body.get(drawn).map_or("", String::as_str),
                post,
            );
            body.get(drawn + 1..).unwrap_or_default()
        };
        for text in rest {
            let y = self.line(selected, rail);
            paint_body(&mut self.canvas, y, tx, text, post);
        }
        let before_cards = match (roomy, self.boxed_cards(post)) {
            (true, _) => 1,
            (false, true) => density.rows(1, 1, 0),
            (false, false) => 0,
        };
        for _ in 0..before_cards {
            self.line(selected, rail);
        }
        self.cards(post, selected, rail, true);
        for _ in 0..density.rows(1, 0, 0) {
            self.line(selected, rail);
        }
        if let Some(at) = post.created_at {
            let y = self.line(selected, rail);
            let when = at
                .with_timezone(&Local)
                .format("%-I:%M %p · %b %-d, %Y")
                .to_string();
            let x = self.canvas.put(y, tx, Kind::Dim, &when);
            self.canvas.link(y, tx, x, &post.permalink());
            self.canvas.target(y, tx, x, Act::Thread(post.id.clone()));
            if post.net.caps().views && post.metrics.views > 0 {
                let x = self.canvas.put(y, x, Kind::Dim, " ·");
                let x = self
                    .canvas
                    .put(y, x + 1, Kind::Bold, &compact(post.metrics.views));
                self.canvas.put(y, x + 1, Kind::Dim, "views");
            }
        }
        let y = self.line(selected, rail);
        self.canvas
            .put(y, tx, Kind::Line, &"─".repeat(right + 1 - tx));
        let y = self.line(selected, rail);
        let actions_end = self.actions(y, post, false);
        if actions_end + 2 <= right {
            self.canvas
                .put_right(y, right, &[(Kind::Dim, self.icons.link)]);
        }
    }

    fn reply(&mut self, post: &Post, joints: &Joints, root_color: Kind, selected: bool) {
        let color = self.color(post);
        let reply_tx = self.reply_tx();
        let body = util::wrap_text(&post.text, self.canvas.width.saturating_sub(reply_tx));
        let right = self.right();

        let y = self.row(selected);
        if joints.rail_top {
            self.rail(y, root_color);
        }
        self.post_avatar_top(y, REPLY_AVATAR, color, joints.up);
        self.avatar_slot(
            post.author.avatar_url.as_deref(),
            y,
            REPLY_AVATAR,
            post.author.business,
        );
        let mut marks: Vec<(Kind, &str)> = Vec::new();
        if post.liked {
            marks.push((Kind::Pink, self.icons.liked));
        }
        if post.bookmarked {
            if !marks.is_empty() {
                marks.push((Kind::Fg, " "));
            }
            marks.push((Kind::Blue, self.icons.bookmarked));
        }
        let marked: usize = marks.iter().map(|(_, text)| text.width()).sum();
        let room = (right + 1).saturating_sub(reply_tx + marked + usize::from(marked > 0));
        let split = self.author_width(post, true, false) > room;
        if split {
            self.name_row(y, reply_tx, post, room);
        } else {
            self.head(y, reply_tx, post, false);
        }
        self.canvas.put_right(y, right, &marks);

        let y = self.row(selected);
        match joints.elbow {
            Some(more) => {
                let elbow = if more { "├╌╌╌" } else { "╰╌╌╌" };
                self.canvas.put(y, ELBOW, root_color, elbow);
            }
            None if joints.rail => self.rail(y, root_color),
            None => {}
        }
        self.post_avatar_mid(y, REPLY_AVATAR, color, post, joints.elbow.is_some());
        let drawn = usize::from(!split);
        if split {
            self.byline(y, reply_tx, post, false, room);
        } else {
            paint_body(
                &mut self.canvas,
                y,
                reply_tx,
                body.first().map_or("", String::as_str),
                post,
            );
        }

        let y = self.row(selected);
        if joints.rail {
            self.rail(y, root_color);
        }
        self.post_avatar_bottom(y, REPLY_AVATAR, color, joints.down);
        self.avatar_targets(y - 2, REPLY_AVATAR, &post.id);
        paint_body(
            &mut self.canvas,
            y,
            reply_tx,
            body.get(drawn).map_or("", String::as_str),
            post,
        );

        for line in body.iter().skip(drawn + 1) {
            let y = self.row(selected);
            if joints.rail {
                self.rail(y, root_color);
            }
            if joints.down {
                self.canvas.put(y, CHAIN, color, "╎");
            }
            paint_body(&mut self.canvas, y, reply_tx, line, post);
        }
    }

    fn head(&mut self, y: usize, col: usize, post: &Post, tag: bool) {
        let mut x = self.canvas.put(y, col, Kind::Bold, &post.author.name);
        if post.author.verified {
            x = self.canvas.put(y, x + 1, Kind::Blue, self.icons.verified);
        }
        x = self
            .canvas
            .put(y, x + 1, Kind::Dim, &format!("@{}", post.author.username));
        self.canvas.link(y, col, x, &profile_url(post));
        self.canvas.target(y, col, x, Act::Author(post.id.clone()));
        if let Some(at) = post.created_at {
            let start = self.canvas.put(y, x, Kind::Dim, " · ");
            x = self
                .canvas
                .put(y, start, Kind::Dim, &ago(at, self.opts.now));
            self.canvas.link(y, start, x, &post.permalink());
            self.canvas
                .target(y, start, x, Act::Thread(post.id.clone()));
        }
        if tag {
            self.tag(y, x, post);
        }
    }

    /// Columns an author line from `col` may take on a row that ends in the `more` icon, a blank before it.
    fn head_room(&self, col: usize) -> usize {
        (self.right() + 1).saturating_sub(col + self.icons.more.width() + 1)
    }

    /// Columns of `post`'s author line: the name and badge, `@handle`, then ` · time` and ` · network` when asked.
    fn author_width(&self, post: &Post, time: bool, tag: bool) -> usize {
        let mut width = post.author.name.width() + 2 + post.author.username.width();
        if post.author.verified {
            width += 1 + self.icons.verified.width();
        }
        if let Some(at) = post.created_at.filter(|_| time) {
            width += 3 + ago(at, self.opts.now).width();
        }
        if tag {
            width += 3 + post.net.label().width();
        }
        width
    }

    /// The author's name and badge from `col`, cut to `room`. Returns the column after it.
    fn name(&mut self, y: usize, col: usize, post: &Post, room: usize) -> usize {
        let badge = if post.author.verified {
            1 + self.icons.verified.width()
        } else {
            0
        };
        let name = util::truncate(&post.author.name, room.saturating_sub(badge));
        let mut x = self.canvas.put(y, col, Kind::Bold, &name);
        if post.author.verified {
            x = self.canvas.put(y, x + 1, Kind::Blue, self.icons.verified);
        }
        x
    }

    /// The name row of an author line split in two, linked to the profile. Returns the column after it.
    fn name_row(&mut self, y: usize, col: usize, post: &Post, room: usize) -> usize {
        let x = self.name(y, col, post, room);
        self.canvas.link(y, col, x, &profile_url(post));
        self.canvas.target(y, col, x, Act::Author(post.id.clone()));
        x
    }

    /// `@handle · time · network` on its own row under the name, cut with `…` at `room` columns.
    fn byline(&mut self, y: usize, col: usize, post: &Post, tag: bool, room: usize) {
        let mut pieces = vec![(Kind::Dim, format!("@{}", post.author.username))];
        if let Some(at) = post.created_at {
            pieces.push((Kind::Dim, " · ".into()));
            pieces.push((Kind::Dim, ago(at, self.opts.now)));
        }
        if tag {
            pieces.push((Kind::Dim, " · ".into()));
            pieces.push((post.net.accent(), post.net.label().into()));
        }
        let end = col + room;
        let mut x = col;
        for (i, (kind, text)) in pieces.iter().enumerate() {
            let shown = util::truncate(text, end.saturating_sub(x));
            let start = x;
            x = self.canvas.put(y, x, *kind, &shown);
            match i {
                0 => {
                    self.canvas.link(y, start, x, &profile_url(post));
                    self.canvas
                        .mouse_target(y, start, x, Act::Author(post.id.clone()));
                }
                2 if post.created_at.is_some() => {
                    self.canvas.link(y, start, x, &post.permalink());
                    self.canvas
                        .target(y, start, x, Act::Thread(post.id.clone()));
                }
                _ => {}
            }
            if shown != *text {
                break;
            }
        }
    }

    /// ` · <network>` after `x`, the label in the network's accent.
    fn tag(&mut self, y: usize, x: usize, post: &Post) -> usize {
        let x = self.canvas.put(y, x, Kind::Dim, " ·");
        self.canvas
            .put(y, x + 1, post.net.accent(), post.net.label())
    }

    fn stat(&mut self, y: usize, x: usize, kind: Kind, text: &str, act: Act) -> usize {
        let end = (x + ACTION_WIDTH).min(self.canvas.width);
        let room = end.saturating_sub(x);
        let shown = if text.width() > room {
            Cow::Owned(util::truncate(text, room))
        } else {
            Cow::Borrowed(text)
        };
        self.canvas.put(y, x, kind, &shown);
        self.canvas.target(y, x, end, act);
        end
    }

    /// The actions `post`'s network offers, in row order: each one's grid column, color, text and act.
    fn action_cells(&self, post: &Post, counts: bool) -> Vec<(usize, Kind, String, Act)> {
        let caps = post.net.caps();
        let m = &post.metrics;
        let icons = self.icons;
        let cell = |icon: &str, n: u64| {
            if caps.counts && counts {
                format!("{icon} {}", compact(n))
            } else {
                icon.to_string()
            }
        };
        let id = &post.id;
        let stats = self.stat_cols();
        let mut cells = Vec::new();
        if caps.post {
            cells.push((
                stats[0],
                Kind::Dim,
                cell(icons.reply, m.replies),
                Act::Reply(id.clone()),
            ));
        }
        if caps.repost {
            let kind = if post.reposted {
                Kind::Green
            } else {
                Kind::Dim
            };
            cells.push((
                stats[1],
                kind,
                cell(icons.repost, m.reposts),
                Act::Repost(id.clone()),
            ));
        }
        if caps.like {
            let (like, kind) = if post.liked {
                (icons.liked, Kind::Pink)
            } else {
                (icons.like, Kind::Dim)
            };
            cells.push((stats[2], kind, cell(like, m.likes), Act::Like(id.clone())));
        }
        if caps.bookmark {
            let (mark, kind) = if post.bookmarked {
                (icons.bookmarked, Kind::Blue)
            } else {
                (icons.bookmark, Kind::Dim)
            };
            cells.push((stats[3], kind, mark.to_string(), Act::Bookmark(id.clone())));
        }
        cells
    }

    /**
    The action row at `y` for every post view, and the column after its last action. From
    [`Self::grid_from`] columns the actions keep their grid columns, with the views on the right when
    `views` asks; narrower rows are [`packed`](Self::packed) and show no views.
    */
    fn actions(&mut self, y: usize, post: &Post, views: bool) -> usize {
        if self.canvas.width < self.grid_from() {
            return self.packed(y, post);
        }
        let mut end = self.tx();
        for (col, kind, text, act) in self.action_cells(post, true) {
            end = self.stat(y, col, kind, &text, act);
        }
        if views {
            self.views(y, post, end);
        }
        end
    }

    /**
    Reserve all four semantic columns even when a network lacks an action, so changing the
    selected network never moves the remaining controls into a different column.
    */
    fn packed(&mut self, y: usize, post: &Post) -> usize {
        let width = self.canvas.width;
        let tx = self.tx();
        let step = width.saturating_sub(tx) / 4;
        let mut end = tx;
        for (col, kind, text, act) in self.action_cells(post, false) {
            let slot = (col - tx) / ACTION_STEP;
            let x = tx + slot * step;
            let right = (x + step.saturating_sub(1)).min(width);
            if right <= x || text.width() > right - x {
                continue;
            }
            end = self.canvas.put(y, x, kind, &text);
            self.canvas.target(y, x, right, act);
        }
        end
    }

    fn views(&mut self, y: usize, post: &Post, actions_end: usize) {
        if post.net.caps().views && post.metrics.views > 0 {
            let views = format!("{} {}", self.icons.views, compact(post.metrics.views));
            let right = self.right();
            let start = (right + 1).saturating_sub(views.width());
            if start >= actions_end + 2 {
                self.canvas.put_right(y, right, &[(Kind::Dim, &views)]);
            }
        }
    }

    // MARK: cards

    fn card_width(&self) -> usize {
        CARD_MAX.min(self.canvas.width.saturating_sub(self.tx()))
    }

    /// Link and quote cards, and an inline image in rich; the media row alone needs no spacer.
    fn boxed_cards(&self, post: &Post) -> bool {
        !post.links.is_empty()
            || post.quoted.is_some()
            || (self.opts.previews.is_some() && post.media.first().and_then(preview_url).is_some())
    }

    fn cards(&mut self, post: &Post, selected: bool, rail: Option<Kind>, tall: bool) {
        if let Some(link) = post.links.first() {
            match (self.opts.previews, link.image.as_deref()) {
                (Some(ratio), Some(image)) => {
                    self.rich_link_card(post, link, image, ratio, selected, rail);
                }
                (Some(_), None) => {
                    self.unfurls.push(link.expanded.clone());
                    self.link_card(post, link, selected, rail);
                }
                (None, _) => self.link_card(post, link, selected, rail),
            }
        }
        if let Some(media) = post.media.first() {
            match (self.opts.previews, preview_url(media)) {
                (Some(ratio), Some(_)) => self.preview(post, ratio, selected, rail, tall),
                _ => self.media_card(post, selected, rail),
            }
        }
        if let Some(quoted) = &post.quoted {
            self.quote_card(post, quoted, selected, rail);
        }
    }

    /// Draws the four rows of a card box and returns the two inner rows.
    fn card(&mut self, selected: bool, rail: Option<Kind>) -> (usize, usize) {
        let tx = self.tx();
        let cw = self.card_width();
        let bar = "─".repeat(cw.saturating_sub(2));
        let y = self.line(selected, rail);
        self.canvas.put(y, tx, Kind::Line, &format!("╭{bar}╮"));
        let inner = [self.line(selected, rail), self.line(selected, rail)];
        for y in inner {
            self.canvas.put(y, tx, Kind::Line, "│");
            self.canvas.put(y, tx + cw - 1, Kind::Line, "│");
        }
        let y = self.line(selected, rail);
        self.canvas.put(y, tx, Kind::Line, &format!("╰{bar}╯"));
        (inner[0], inner[1])
    }

    fn card_room(&self) -> usize {
        self.card_width().saturating_sub(4)
    }

    fn link_card(&mut self, post: &Post, link: &Link, selected: bool, rail: Option<Kind>) {
        if self.card_width() < CARD_MIN {
            self.link_row(post, link, selected, rail);
            return;
        }
        let tx = self.tx();
        let (a, b) = self.card(selected, rail);
        let act = Act::Link {
            post: post.id.clone(),
            url: link.expanded.clone(),
        };
        let end = tx + self.card_width();
        self.canvas.mouse_target(a - 1, tx, end, act.clone());
        self.canvas.mouse_target(a, tx, end, act.clone());
        self.canvas.target(b, tx, end, act.clone());
        self.canvas.mouse_target(b + 1, tx, end, act);
        let room = self.card_room();
        let host = link.display.split('/').next().unwrap_or(&link.display);
        let label = util::truncate(&format!("{} {host}", self.icons.link), room);
        let x = self.canvas.put(a, tx + 2, Kind::Dim, &label);
        self.canvas.link(a, tx + 2, x, &link.expanded);
        let x = match &link.title {
            Some(title) => {
                let title = util::truncate(title, room);
                let x = self.canvas.put(b, tx + 2, Kind::Bold, &title);
                let rest = room.saturating_sub(title.width() + 1);
                match link.description.as_deref().filter(|_| rest >= 2) {
                    Some(description) => {
                        self.canvas
                            .put(b, x + 1, Kind::Dim, &util::truncate(description, rest))
                    }
                    None => x,
                }
            }
            None => self
                .canvas
                .put(b, tx + 2, Kind::Fg, &util::truncate(&link.display, room)),
        };
        self.canvas.link(b, tx + 2, x, &link.expanded);
    }

    /// A link the text doesn't show already as one row, as minimal draws it: its display URL, linked to the page.
    fn link_row(&mut self, post: &Post, link: &Link, selected: bool, rail: Option<Kind>) {
        let in_text = [&link.short, &link.display, &link.expanded]
            .iter()
            .any(|url| !url.is_empty() && post.text.contains(url.as_str()));
        if in_text || link.display.is_empty() {
            return;
        }
        let y = self.line(selected, rail);
        let tx = self.tx();
        let room = self.canvas.width.saturating_sub(tx);
        let x = self
            .canvas
            .put(y, tx, Kind::Dim, &util::truncate(&link.display, room));
        self.canvas.link(y, tx, x, &link.expanded);
        let act = Act::Link {
            post: post.id.clone(),
            url: link.expanded.clone(),
        };
        self.canvas.target(y, tx, x, act);
    }

    /**
    A link card with the page's preview image, like X's. A wide image fills the top of the card
    with the title and the site under it; a small or square one sits at the left as a thumbnail
    with the title, description and domain beside it. Until the image loads, its box says so.
    */
    fn rich_link_card(
        &mut self,
        post: &Post,
        link: &Link,
        image: &str,
        ratio: f32,
        selected: bool,
        rail: Option<Kind>,
    ) {
        let tx = self.tx();
        let cw = RICH_CARD.min(self.canvas.width.saturating_sub(tx));
        if cw < THUMB_COLS + 16 {
            self.link_card(post, link, selected, rail);
            return;
        }
        let inner = cw - 2;
        let large = match (link.image_width, link.image_height) {
            (Some(width), Some(height)) if width > 0 && height > 0 => {
                width >= 400 && width as f32 / height as f32 >= 1.3
            }
            _ => true,
        };
        let (cols, rows) = if large {
            let (width, height) = match (link.image_width, link.image_height) {
                (Some(width), Some(height)) if width > 0 && height > 0 => (width, height),
                _ => (1200, 630),
            };
            let room = u16::try_from(inner).unwrap_or(u16::MAX);
            let natural = preview::rows_for(ratio, Some(width), Some(height), room, u16::MAX);
            let rows = natural.min(RICH_IMAGE_MAX);
            let cols = if natural > rows {
                let fitted = f32::from(rows) * width as f32 / height as f32 / ratio;
                (fitted.floor() as usize).clamp(1, inner)
            } else {
                inner
            };
            (cols, usize::from(rows))
        } else {
            (THUMB_COLS, THUMB_ROWS)
        };
        let slot_x = if large {
            tx + 1 + (inner - cols) / 2
        } else {
            tx + 1
        };
        let text_rows = if large { 2 } else { 0 };
        let lines: Vec<usize> = (0..rows + text_rows + 2)
            .map(|_| self.line(selected, rail))
            .collect();
        let last = lines.len() - 1;
        let bar = "─".repeat(inner);
        self.canvas
            .put(lines[0], tx, Kind::Line, &format!("╭{bar}╮"));
        for &y in &lines[1..last] {
            self.canvas.put(y, tx, Kind::Line, "│");
            self.canvas.put(y, tx + cw - 1, Kind::Line, "│");
        }
        self.canvas
            .put(lines[last], tx, Kind::Line, &format!("╰{bar}╯"));
        let loading = util::truncate("loading preview", cols);
        self.canvas.put(
            lines[1 + (rows - 1) / 2],
            slot_x + (cols - loading.width()) / 2,
            Kind::Dim,
            &loading,
        );
        self.slots.push(Slot {
            url: image.to_string(),
            x: slot_x,
            y: lines[1],
            cols,
            rows,
            avatar: false,
            square: false,
        });
        let host = link.display.split('/').next().unwrap_or(&link.display);
        let title = link
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty());
        let mut texts: Vec<(Kind, String)> = Vec::new();
        let (x, room, top) = if large {
            let room = inner - 2;
            let site = link
                .site
                .as_deref()
                .map(str::trim)
                .filter(|site| !site.is_empty())
                .unwrap_or(host);
            texts.push(match title {
                Some(title) => (Kind::Bold, util::truncate(title, room)),
                None => (Kind::Fg, util::truncate(&link.display, room)),
            });
            texts.push((Kind::Dim, util::truncate(site, room)));
            (tx + 2, room, 1 + rows)
        } else {
            let room = cw - THUMB_COLS - 4;
            match title {
                Some(title) => texts.extend(
                    rows_of(title, room, 2)
                        .into_iter()
                        .map(|row| (Kind::Bold, row)),
                ),
                None => texts.push((Kind::Fg, util::truncate(&link.display, room))),
            }
            if let Some(description) = link
                .description
                .as_deref()
                .map(str::trim)
                .filter(|description| !description.is_empty())
            {
                texts.extend(
                    rows_of(description, room, 2)
                        .into_iter()
                        .map(|row| (Kind::Dim, row)),
                );
            }
            texts.push((Kind::Dim, util::truncate(host, room)));
            (tx + 2 + THUMB_COLS, room, 1)
        };
        let act = Act::Link {
            post: post.id.clone(),
            url: link.expanded.clone(),
        };
        for (i, &y) in lines.iter().enumerate() {
            if i == top {
                self.canvas.target(y, tx, tx + cw, act.clone());
            } else {
                self.canvas.mouse_target(y, tx, tx + cw, act.clone());
            }
        }
        for (i, (kind, text)) in texts.into_iter().take(last - top).enumerate() {
            let y = lines[top + i];
            let end = self.canvas.put(y, x, kind, &util::truncate(&text, room));
            self.canvas.link(y, x, end, &link.expanded);
        }
    }

    /// One dim row for a post's images: `image · <alt>` or `2 images`, then `v view`.
    fn media_card(&mut self, post: &Post, selected: bool, rail: Option<Kind>) {
        let Some(media) = post.media.first() else {
            return;
        };
        let y = self.line(selected, rail);
        let what = match post.media.len() {
            1 => media.kind.label().to_string(),
            n => format!("{n} images"),
        };
        let mut label = format!("{} {what}", self.icons.image);
        if post.media.len() == 1 {
            if let Some(alt) = media
                .alt
                .as_deref()
                .map(str::trim)
                .filter(|alt| !alt.is_empty())
            {
                label.push_str(" · ");
                label.push_str(alt);
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
        let tx = self.tx();
        let room = self.canvas.width.saturating_sub(tx + 1 + tail.width());
        let x = self
            .canvas
            .put(y, tx, Kind::Dim, &util::truncate(&label, room));
        let end = self.canvas.put(y, x, Kind::Dim, tail);
        self.canvas.target(y, tx, end, Act::Media(post.id.clone()));
    }

    /**
    Up to four image boxes side by side at the text column, sharing the tallest box's rows. Each
    box's height is fixed from the media sizes before the pictures load so rows never jump, and it
    is drawn as the loading frame; then a caption for a video or for images the row leaves out.
    */
    fn preview(&mut self, post: &Post, ratio: f32, selected: bool, rail: Option<Kind>, tall: bool) {
        let tx = self.tx();
        let room = self.canvas.width.saturating_sub(tx + 2);
        let stills: Vec<(usize, &Media)> = post
            .media
            .iter()
            .enumerate()
            .filter(|(_, media)| preview_url(media).is_some())
            .take(GALLERY_MAX)
            .collect();
        let max = image_max_rows(
            self.opts.profile.is_some(),
            tall,
            self.opts.page,
            self.opts.height,
            self.opts.density,
        );
        let sizes: Vec<(Option<u32>, Option<u32>)> = stills
            .iter()
            .map(|(_, media)| (media.width, media.height))
            .collect();
        let (mut widths, mut rows) = gallery(&sizes, ratio, room, max);
        if self.opts.page && widths.len() == 1 {
            if let Some((cols, fitted)) = sizes
                .first()
                .and_then(|&(width, height)| fit_one(width, height, ratio, widths[0], max))
            {
                widths[0] = cols;
                rows = fitted;
            }
        }
        let shown = &stills[..widths.len().min(stills.len())];
        let lines: Vec<usize> = (0..rows).map(|_| self.line(selected, rail)).collect();
        let first = Act::Media(post.id.clone());
        let mut x = tx;
        for (k, ((index, media), &cols)) in shown.iter().zip(&widths).enumerate() {
            for line in preview_frame(cols, rows, "image") {
                let y = lines[line.row];
                self.canvas.put(y, x + line.col, line.kind, &line.text);
            }
            let act = if k == 0 {
                first.clone()
            } else {
                Act::MediaAt {
                    post: post.id.clone(),
                    index: viewer_index(post, *index),
                }
            };
            for (i, y) in lines.iter().enumerate() {
                if i == 0 && k == 0 {
                    self.canvas.target(*y, x, x + cols, act.clone());
                } else {
                    self.canvas.mouse_target(*y, x, x + cols, act.clone());
                }
            }
            self.slots.push(Slot {
                url: preview_url(media).unwrap_or_default().to_string(),
                x,
                y: lines[0],
                cols,
                rows,
                avatar: false,
                square: false,
            });
            x += cols + 1;
        }
        let total = post.media.len();
        let caption = match post.media[0].kind {
            MediaKind::Video | MediaKind::Gif => {
                Some("video · o plays it in the browser".to_string())
            }
            _ if shown.len() < total => Some(format!(
                "{} of {total} · {}view",
                shown.len(),
                if self.opts.shortcuts { "v " } else { "" }
            )),
            _ if total > 1 => Some(format!(
                "{total} images · {}view",
                if self.opts.shortcuts { "v " } else { "" }
            )),
            _ => None,
        };
        if let Some(caption) = caption {
            let y = self.line(selected, rail);
            let end = self.canvas.put(y, tx, Kind::Dim, &caption);
            self.canvas.mouse_target(y, tx, end, first);
        }
    }

    fn quote_card(&mut self, post: &Post, quoted: &Post, selected: bool, rail: Option<Kind>) {
        let tx = self.tx();
        let (a, b) = self.card(selected, rail);
        let act = Act::Quote {
            post: post.id.clone(),
            quoted: quoted.id.clone(),
        };
        let end = tx + self.card_width();
        self.canvas.mouse_target(a - 1, tx, end, act.clone());
        self.canvas.target(a, tx, end, act.clone());
        self.canvas.mouse_target(b, tx, end, act.clone());
        self.canvas.mouse_target(b + 1, tx, end, act);
        let room = self.card_room();
        let name = util::truncate(&quoted.author.name, room);
        let mut x = self.canvas.put(a, tx + 2, Kind::Bold, &name);
        let handle = format!("@{}", quoted.author.username);
        let when = quoted
            .created_at
            .map(|at| ago(at, self.opts.now))
            .unwrap_or_default();
        let byline = if when.is_empty() {
            handle.clone()
        } else {
            format!("{handle} · {when}")
        };
        let rest = room.saturating_sub(name.width() + 1);
        if rest >= 2 {
            let shown = util::truncate(&byline, rest);
            let start = x + 1;
            x = self.canvas.put(a, start, Kind::Dim, &shown);
            if shown == byline && !when.is_empty() {
                let handle_end = start + handle.width();
                self.canvas.link(a, handle_end + 3, x, &quoted.permalink());
                x = handle_end;
            }
        }
        self.canvas.link(a, tx + 2, x, &profile_url(quoted));
        let first = quoted.text.lines().next().unwrap_or("");
        self.canvas
            .put(b, tx + 2, Kind::Fg, &util::truncate(first, room));
    }
}

/// `text` wrapped to `room` columns in at most `max` rows; a cut ends the last row with `…`.
fn rows_of(text: &str, room: usize, max: usize) -> Vec<String> {
    let mut rows = util::wrap_text(text, room);
    if rows.len() > max {
        rows.truncate(max);
        if let Some(last) = rows.last_mut() {
            *last = util::truncate(&format!("{last}…"), room);
        }
    }
    rows
}

fn has_cards(post: &Post) -> bool {
    !post.links.is_empty() || !post.media.is_empty() || post.quoted.is_some()
}

/// Lines to paint, and whether the post is longer than the list cap.
pub(crate) fn body_window(len: usize, opened: bool) -> (usize, bool) {
    let cut = len > BODY_LINES;
    let limit = if opened || !cut { len } else { BODY_LINES };
    (limit, cut)
}

/// What a root's fold says: `Show this thread`, or the count of loaded replies it leaves out.
pub(crate) fn replies_label(folded: usize) -> String {
    match folded {
        0 => "Show this thread".to_string(),
        1 => "Show 1 reply".to_string(),
        n => format!("Show {n} replies"),
    }
}

/**
The boxes of a row of pictures with these pixel `sizes`: each box's columns, and the rows they share.
One picture takes the width left of the text column, up to `PREVIEW_MAX`. Two to four share the
tallest height up to `max` rows that fits `room` with a column between boxes, and each box is as
wide as its picture at that height, so every picture meets the same top and bottom rows. The row
drops its last picture while that height would be under half of `max`.
*/
fn gallery(
    sizes: &[(Option<u32>, Option<u32>)],
    ratio: f32,
    room: usize,
    max: usize,
) -> (Vec<usize>, usize) {
    let ratio = if ratio > 0.0 { ratio } else { 0.5 };
    for count in (2..=sizes.len()).rev() {
        let aspects: Vec<f32> = sizes[..count]
            .iter()
            .map(|&(width, height)| aspect(width, height))
            .collect();
        // The thousandth keeps float error from adding a column to a picture that fits exactly.
        let widths = |rows: usize| -> Vec<usize> {
            aspects
                .iter()
                .map(|a| (rows as f32 * a / ratio - 0.001).ceil().max(1.0) as usize)
                .collect()
        };
        let fit = (1..=max).rev().find(|&rows| {
            let boxes = widths(rows);
            boxes.iter().all(|cols| *cols <= PREVIEW_MAX)
                && boxes.iter().sum::<usize>() + count - 1 <= room
        });
        /* Half of a tall page or thread cap is more than two pictures can share, so the
        floor stays at the old thread cap and a pair does not collapse into one image. */
        let floor = max.min(12);
        if let Some(rows) = fit.filter(|rows| rows.saturating_mul(2) >= floor) {
            return (widths(rows), rows);
        }
    }
    let cols = tile_width(room, 1);
    let (width, height) = sizes.first().copied().unwrap_or((None, None));
    let rows = preview::rows_for(
        ratio,
        width,
        height,
        u16::try_from(cols).unwrap_or(u16::MAX),
        u16::try_from(max).unwrap_or(u16::MAX),
    );
    (vec![cols], usize::from(rows))
}

/**
How many rows an inline picture may take.

A feed stays at 4, 6 or 8. An open thread starts at 6, 10 or 12 and grows to two fifths of a tall
pane, at most 22, 32 or 36. The page uses about three fifths of the pane (at least 12, 16 or 18,
at most 28, 40 or 48) and [`fit_one`] narrows a picture that would still be sliced.
*/
fn image_max_rows(
    profile: bool,
    tall: bool,
    page: bool,
    height: Option<usize>,
    density: Density,
) -> usize {
    if page {
        let pane = height.unwrap_or(48);
        let floor = density.rows(18, 16, 12);
        let ceiling = density.rows(48, 40, 28);
        return (pane.saturating_mul(3) / 5).clamp(floor, ceiling);
    }
    if profile {
        if height.is_some_and(|height| height < 24) {
            return 4;
        }
        return density.rows(8, 6, 4);
    }
    if tall {
        let base = density.rows(12, 10, 6);
        let grown = height.unwrap_or(0).saturating_mul(2) / 5;
        let ceiling = density.rows(36, 32, 22);
        return grown.max(base).min(ceiling);
    }
    density.rows(8, 6, 4)
}

/**
Columns and rows for one picture so the box matches its pixels.

Full width when that height fits in `max`. A taller picture gets narrower instead of a slice.
`None` when the network sent no size, and the caller keeps the capped box.
*/
fn fit_one(
    width: Option<u32>,
    height: Option<u32>,
    ratio: f32,
    cols: usize,
    max: usize,
) -> Option<(usize, usize)> {
    let (Some(width), Some(height)) = (width, height) else {
        return None;
    };
    if width == 0 || height == 0 || cols == 0 || max == 0 {
        return None;
    }
    let ratio = if ratio > 0.0 { ratio } else { 0.5 };
    let natural = usize::from(preview::rows_for(
        ratio,
        Some(width),
        Some(height),
        u16::try_from(cols).unwrap_or(u16::MAX),
        u16::MAX,
    ));
    if natural <= max {
        return Some((cols, natural.max(1)));
    }
    let shape = width as f32 / height as f32;
    let narrowed = ((max as f32) * shape / ratio).floor().max(1.0) as usize;
    let narrowed = narrowed.min(cols).max(1);
    let rows = usize::from(preview::rows_for(
        ratio,
        Some(width),
        Some(height),
        u16::try_from(narrowed).unwrap_or(u16::MAX),
        u16::try_from(max).unwrap_or(u16::MAX),
    ));
    Some((narrowed, rows.max(1)))
}

/// A picture's width over its height for its box, no narrower than `ASPECT_MIN`.
fn aspect(width: Option<u32>, height: Option<u32>) -> f32 {
    match (width, height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => {
            (width as f32 / height as f32).max(ASPECT_MIN)
        }
        _ => ASPECT_UNKNOWN,
    }
}

/// Columns of each of `count` image boxes sharing `room` columns with a one-column gap between them.
fn tile_width(room: usize, count: usize) -> usize {
    PREVIEW_MAX.min(room.saturating_sub(count.saturating_sub(1)) / count.max(1))
}

/// Where `post.media[index]` sits among the pictures the viewer shows, which skips media it cannot show.
fn viewer_index(post: &Post, index: usize) -> usize {
    post.media[..index]
        .iter()
        .filter(|media| media.view_url().is_some())
        .count()
}

/// The still a preview shows: a photo itself, or a video's or GIF's preview image.
fn preview_url(media: &Media) -> Option<&str> {
    match media.kind {
        MediaKind::Photo => media.url.as_deref().or(media.preview_url.as_deref()),
        _ => media.preview_url.as_deref(),
    }
}

/// One run of an image frame, relative to the box's top-left cell.
pub struct FramePart {
    pub row: usize,
    pub col: usize,
    pub kind: Kind,
    pub text: String,
}

/**
The rounded Line-color frame an image box shows while it loads or after it fails, with `label` in
Dim centered; a box under three rows has room for the label only.
*/
pub fn preview_frame(cols: usize, rows: usize, label: &str) -> Vec<FramePart> {
    let mut parts = Vec::new();
    if cols == 0 || rows == 0 {
        return parts;
    }
    let framed = rows >= 3 && cols >= 2;
    if framed {
        let bar = "─".repeat(cols - 2);
        let part = |row, text: String| FramePart {
            row,
            col: 0,
            kind: Kind::Line,
            text,
        };
        parts.push(part(0, format!("╭{bar}╮")));
        for row in 1..rows - 1 {
            parts.push(part(row, "│".into()));
            parts.push(FramePart {
                col: cols - 1,
                ..part(row, "│".into())
            });
        }
        parts.push(part(rows - 1, format!("╰{bar}╯")));
    }
    let room = if framed { cols - 2 } else { cols };
    let label = util::truncate(label, room);
    parts.push(FramePart {
        row: (rows - 1) / 2,
        col: (cols - label.width()) / 2,
        kind: Kind::Dim,
        text: label,
    });
    parts
}

// MARK: avatars

fn avatar_top(canvas: &mut Canvas, y: usize, col: usize, color: Kind, up: bool) {
    canvas.put(
        y,
        col,
        color,
        if up {
            "╭─┴─╮"
        } else {
            "╭───╮"
        },
    );
}

fn avatar_mid(canvas: &mut Canvas, y: usize, col: usize, color: Kind, post: &Post, join: bool) {
    canvas.put(y, col, color, if join { "┤" } else { "│" });
    canvas.put(y, col + 2, Kind::Bold, &post.author.badge().to_string());
    canvas.put(y, col + 4, color, "│");
}

fn avatar_bottom(canvas: &mut Canvas, y: usize, col: usize, color: Kind, down: bool) {
    canvas.put(
        y,
        col,
        color,
        if down {
            "╰─┬─╯"
        } else {
            "╰───╯"
        },
    );
}

/// A plain three-row avatar for screens outside the feed.
pub fn avatar(canvas: &mut Canvas, top: usize, col: usize, color: Kind, letter: char) {
    canvas.put(top, col, color, "╭───╮");
    canvas.put(top + 1, col, color, "│");
    canvas.put(top + 1, col + 2, Kind::Bold, &letter.to_string());
    canvas.put(top + 1, col + 4, color, "│");
    canvas.put(top + 2, col, color, "╰───╯");
}

/** Mentions and links in the link color, each linked to the profile or the full URL; the rest in the default color. */
fn paint_body(canvas: &mut Canvas, row: usize, col: usize, text: &str, post: &Post) {
    paint_text(canvas, row, col, text, post, Kind::Link);
}

/// One line of a post's text, its links drawn in `link` and linked to where they go.
pub(crate) fn paint_text(
    canvas: &mut Canvas,
    row: usize,
    col: usize,
    text: &str,
    post: &Post,
    link: Kind,
) {
    let mut x = col;
    let mut rest = text;
    while !rest.is_empty() {
        let space = rest.len() - rest.trim_start().len();
        if space > 0 {
            x = canvas.put(row, x, Kind::Fg, &rest[..space]);
            rest = &rest[space..];
            continue;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..end];
        rest = &rest[end..];
        let Some((n, url)) = link_in(word, post) else {
            let start = x;
            x = canvas.put(row, x, Kind::Fg, word);
            if let Some(tag) = hashtag(word) {
                let act = Act::Hashtag {
                    post: post.id.clone(),
                    tag: tag.to_string(),
                };
                canvas.target(row, start, start + 1 + tag.width(), act);
            }
            continue;
        };
        let start = x;
        x = canvas.put(row, x, link, &word[..n]);
        canvas.link(row, start, x, &url);
        let named = post
            .links
            .iter()
            .any(|link| word == link.short || word == link.display);
        let act = match word.strip_prefix('@') {
            Some(tail) if !named => Act::Mention {
                post: post.id.clone(),
                handle: tail[..n - 1].to_string(),
            },
            _ => Act::Link {
                post: post.id.clone(),
                url,
            },
        };
        canvas.target(row, start, x, act);
        x = canvas.put(row, x, Kind::Fg, &word[n..]);
    }
}

/// The tag of a `#tag` word: the letters, digits and underscores after the `#`.
fn hashtag(word: &str) -> Option<&str> {
    let tail = word.strip_prefix('#')?;
    let n = tail
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(tail.len());
    (n > 0).then(|| &tail[..n])
}

/** The length of the link that starts `word` and where it goes: a URL (expanded when the post knows its short form) or an @mention's profile. */
fn link_in(word: &str, post: &Post) -> Option<(usize, String)> {
    if let Some(link) = post
        .links
        .iter()
        .find(|link| word == link.short || word == link.display)
    {
        return Some((word.len(), link.expanded.clone()));
    }
    if word.starts_with("http://") || word.starts_with("https://") {
        let url = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\'']);
        return Some((url.len(), url.to_string()));
    }
    let tail = word.strip_prefix('@')?;
    let n = tail
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
        .unwrap_or(tail.len());
    let handle = tail[..n].trim_end_matches(['.', '-']);
    (!handle.is_empty()).then(|| (handle.len() + 1, post.net.profile_url(handle)))
}

pub(crate) fn profile_url(post: &Post) -> String {
    post.net.profile_url(&post.author.username)
}

// MARK: mentions

pub fn notifications(posts: &[Post], expanded: &HashSet<String>, opts: &ViewOpts) -> View {
    if matches!(opts.level, Detail::Minimal | Detail::Plain) {
        return minimal::notifications(posts, expanded, opts);
    }
    let laid = layout::lay(posts, expanded, opts.newest);
    let mut pen = Pen::new(posts, opts);
    let mut rows = Vec::with_capacity(laid.len());
    let between = opts.density.rows(1, 1, 0);
    for (index, row) in laid.iter().enumerate() {
        let selected = opts.selected == Some(index);
        if index > 0 && !layout::is_gap(row) && !layout::is_gap(&laid[index - 1]) {
            pen.blanks(between);
        }
        let start = pen.canvas.len();
        match &row.target {
            layout::Target::Gap => pen.blanks(between),
            layout::Target::Fold { hidden, parent } => {
                let y = pen.row(selected);
                let tx = pen.tx();
                let end = pen
                    .canvas
                    .put(y, tx, Kind::Dim, &format!("Show {hidden} more replies"));
                pen.canvas.target(y, tx, end, Act::Fold(parent.clone()));
            }
            layout::Target::Post(id) => {
                if let Some(post) = pen.by_id.get(id.as_str()).copied() {
                    pen.mention(post, selected);
                }
            }
            layout::Target::Profile => {}
        }
        rows.push(RowSpan {
            start,
            end: pen.canvas.len(),
        });
    }
    let (lines, links, targets) = pen.canvas.finish_targeted();
    View {
        lines,
        rows,
        links,
        previews: Vec::new(),
        unfurls: Vec::new(),
        targets,
    }
}

impl Pen<'_> {
    fn mention(&mut self, post: &Post, selected: bool) {
        let color = self.color(post);
        let tx = self.tx();
        let right = self.right();
        let text = util::wrap_text(&post.text, self.canvas.width.saturating_sub(tx));
        let tags = self.opts.tags;
        let mentioned = format!("{} mentioned you", self.icons.at);
        let room = self.canvas.width.saturating_sub(tx);
        let when = post.created_at.map(|at| ago(at, self.opts.now));
        let split = self.author_width(post, false, tags)
            + 2
            + mentioned.width()
            + when.as_ref().map_or(0, |when| 1 + when.width())
            > room;
        let top = self.row(selected);
        if split {
            let x = self.name_row(top, tx, post, room);
            let rest = (tx + room).saturating_sub(x + 2);
            if rest > 1 {
                self.canvas
                    .put(top, x + 2, Kind::Blue, &util::truncate(&mentioned, rest));
            }
        } else {
            let x = self.canvas.put(top, tx, Kind::Bold, &post.author.name);
            let mut x =
                self.canvas
                    .put(top, x + 1, Kind::Dim, &format!("@{}", post.author.username));
            self.canvas.link(top, tx, x, &profile_url(post));
            self.canvas.target(top, tx, x, Act::Author(post.id.clone()));
            if tags {
                x = self.tag(top, x, post);
            }
            self.canvas.put(top, x + 2, Kind::Blue, &mentioned);
            if let Some(when) = &when {
                self.canvas.put_right(top, right, &[(Kind::Dim, when)]);
                let start = (right + 1).saturating_sub(when.width());
                self.canvas.link(top, start, right + 1, &post.permalink());
                self.canvas
                    .target(top, start, right + 1, Act::Thread(post.id.clone()));
            }
        }
        let opened = self.opts.opened.contains(&post.id);
        let (limit, cut) = body_window(text.len(), opened);
        let drawn = usize::from(!split);
        let mid = self.row(selected);
        if split {
            self.byline(mid, tx, post, tags, room);
        } else if limit > 0 {
            paint_body(
                &mut self.canvas,
                mid,
                tx,
                text.first().map_or("", String::as_str),
                post,
            );
        }
        let bottom = self.row(selected);
        if drawn < limit {
            paint_body(
                &mut self.canvas,
                bottom,
                tx,
                text.get(drawn).map_or("", String::as_str),
                post,
            );
        }
        if self.opts.level == Detail::Rich {
            avatar(
                &mut self.canvas,
                top,
                ROOT_AVATAR,
                color,
                post.author.badge(),
            );
        }
        self.avatar_targets(top, ROOT_AVATAR, &post.id);
        for (index, line) in text.iter().enumerate().skip(drawn + 1) {
            if index >= limit {
                break;
            }
            let y = self.row(selected);
            paint_body(&mut self.canvas, y, tx, line, post);
        }
        for _ in 0..self.opts.density.rows(1, 1, 0) {
            self.row(selected);
        }
        let y = self.row(selected);
        self.actions(y, post, true);
        if cut {
            self.more_row(selected, None, &post.id, opened);
        }
    }
}

// MARK: painters

/// The dark theme at this terminal's color depth, for output that has no settings.
pub fn paint(line: &StyledLine, color: bool) -> String {
    paint_with(line, color, &Theme::from_env())
}

pub fn paint_with(line: &StyledLine, color: bool, theme: &Theme) -> String {
    if !color {
        return line.plain();
    }
    if theme.ascii {
        let mut out = String::new();
        for span in &line.spans {
            let bold = Theme::bold(span.kind);
            let underline = theme.underline(span.kind);
            if bold {
                out.push_str("\x1b[1m");
            }
            if underline {
                out.push_str("\x1b[4m");
            }
            out.push_str(&span.text);
            if bold || underline {
                out.push_str("\x1b[22;24m");
            }
        }
        return out;
    }
    let mut out = String::new();
    if let Some(bg) = theme.bg_sgr(true).filter(|_| line.selected) {
        out.push_str(&format!("\x1b[{bg}m"));
    }
    if theme.reverse(line.selected) {
        out.push_str("\x1b[7m");
    }
    for span in &line.spans {
        let weight = if Theme::bold(span.kind) {
            1
        } else if theme.dim_attr(span.kind) {
            2
        } else {
            22
        };
        let underline = if theme.underline(span.kind) { 4 } else { 24 };
        let fg = theme.fg_sgr(span.kind, line.selected);
        out.push_str(&format!("\x1b[{weight};{underline};{fg}m"));
        out.push_str(&span.text);
    }
    out.push_str("\x1b[0m");
    out
}

pub fn ratatui_style(kind: Kind, theme: &Theme, selected: bool) -> ratatui::style::Style {
    use ratatui::style::{Modifier, Style};
    let mut style = Style::default().fg(theme.fg(kind, selected));
    if Theme::bold(kind) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if theme.underline(kind) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if theme.reverse(selected) {
        style = style.add_modifier(Modifier::REVERSED);
    }
    if theme.dim_attr(kind) {
        style = style.add_modifier(Modifier::DIM);
    }
    style
}

/// A row's background in the TUI: the surface when selected, reversed in mono.
pub fn row_style(theme: &Theme, selected: bool) -> ratatui::style::Style {
    use ratatui::style::{Modifier, Style};
    let style = Style::default().bg(theme.bg(selected));
    if theme.reverse(selected) {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loading_card_is_a_box_in_medium_and_words_otherwise() {
        let medium = loading_card(Detail::Standard, false, 80);
        let text = medium
            .iter()
            .map(StyledLine::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("loading…"), "{text}");
        assert!(text.contains('╭'), "{text}");
        assert_eq!(
            medium.iter().find_map(|line| line.plain().find('╭')),
            Some(text_origin(Detail::Standard))
        );
        assert!(medium.iter().all(|line| !line.bar && !line.selected));
        assert!(medium.iter().all(|line| line.plain().width() <= 80));
        let rich = loading_card(Detail::Rich, false, 80);
        assert_eq!(
            rich.iter().find_map(|line| line.plain().find('╭')),
            Some(TX)
        );

        let plain = loading_card(Detail::Plain, true, 80);
        let text = plain
            .iter()
            .map(StyledLine::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("loading..."), "{text}");
        assert!(!text.contains('╭'), "{text}");

        let minimal = loading_card(Detail::Minimal, false, 80);
        let text = minimal
            .iter()
            .map(StyledLine::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("loading…"), "{text}");
        assert!(!text.contains('╭'), "{text}");

        let narrow = loading_card(Detail::Standard, false, 10);
        let text = narrow
            .iter()
            .map(StyledLine::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("loading…"), "{text}");
        assert!(!text.contains('╭'), "{text}");
        assert!(narrow.iter().all(|line| line.plain().width() <= 10));
    }

    #[test]
    fn medium_posts_close_the_gap_after_the_rail() {
        let mut view_opts = opts(None, None);
        view_opts.level = Detail::Standard;
        let origin = text_origin(Detail::Standard);
        let reply_at = reply_origin(Detail::Standard);
        let rows = grid(&render(&tree(), &HashSet::new(), &view_opts));
        let root = row_with(&rows, "ROOT");
        assert_eq!(root.get(origin - 1), Some(&' '));
        assert_eq!(at(root, origin, 4), "ROOT");
        let reply = row_with(&rows, "text of a1");
        assert_eq!(at(reply, ELBOW, 7), "├╌╌╌╌╌╎");
        assert_eq!(at(reply, reply_at, 10), "text of a1");
        let fold = row_with(&rows, "Show 1 more replies");
        assert_eq!(fold.get(CHAIN), Some(&'○'));
        assert_eq!(fold.get(CHAIN + 1), Some(&' '));
        assert_eq!(at(fold, reply_at, 19), "Show 1 more replies");
        let stats = row_with(&rows, "↩ 5");
        assert_eq!(at(stats, origin, 3), "↩ 5");

        let rows = grid(&render(
            &[post("r", "root", None, 10)],
            &HashSet::new(),
            &view_opts,
        ));
        let fold = row_with(&rows, "Show this thread");
        assert_eq!(fold.get(ROOT_RAIL), Some(&'○'));
        assert_eq!(at(fold, origin, 16), "Show this thread");

        let user = User {
            id: "u1".into(),
            name: "Kal Nguyen".into(),
            username: "kal".into(),
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        };
        let profiled = ViewOpts {
            profile: Some((Network::X, user)),
            ..view_opts
        };
        let rows = grid(&render(&[], &HashSet::new(), &profiled));
        let name = row_with(&rows, "Kal Nguyen");
        assert_eq!(at(name, 2, 5), "╭───╮");
        assert_eq!(at(name, TX, 10), "Kal Nguyen");
    }

    #[test]
    fn medium_draws_to_the_edge() {
        assert_eq!(margin_for(104, Detail::Standard), 0);
        assert_eq!(margin_for(40, Detail::Standard), 0);
        assert_eq!(inner_width_for(104, Detail::Standard), 104);
        assert_eq!(margin_for(104, Detail::Rich), 2);
        assert_eq!(margin_for(40, Detail::Plain), 0);
    }

    #[test]
    fn native_prints_the_terminal_colors_and_black_prints_rgb() {
        let line = StyledLine {
            spans: vec![
                Span {
                    kind: Kind::Fg,
                    text: "text".into(),
                },
                Span {
                    kind: Kind::Blue,
                    text: "blue".into(),
                },
            ],
            selected: false,
            hl: None,
            bar: false,
        };
        let mut theme = Theme::default();
        theme.set_scheme(None, true);
        let native = paint_with(&line, true, &theme);
        assert!(
            native.contains(";39m") && native.contains(";34m"),
            "{native:?}"
        );
        assert!(!native.contains("38;2;"), "{native:?}");
        let (palette, dark) = crate::theme::Scheme::Black.resolve(true);
        theme.set_scheme(palette, dark);
        let black = paint_with(&line, true, &theme);
        assert!(black.contains("38;2;231;233;234"), "{black:?}");
    }

    use crate::model::{Metrics, User};
    use crate::net::Network;
    use chrono::{Duration, TimeZone};

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

    fn opts(selected: Option<usize>, thread: Option<&str>) -> ViewOpts {
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
            level: Detail::Rich,
            shortcuts: true,
            avatars: false,
            overview: false,
            opened: HashSet::new(),
        }
    }

    fn on(net: Network, posts: Vec<Post>) -> Vec<Post> {
        posts.into_iter().map(|p| Post { net, ..p }).collect()
    }

    #[test]
    fn profiles_keep_multiline_bios_and_profile_actions_at_narrow_widths() {
        let user = User {
            id: "u1".into(),
            name: "Kal Nguyen".into(),
            username: "kal".into(),
            avatar_url: None,
            verified: true,
            business: false,
            description: Some("Writes about terminals.\nSecond line with 界 and e\u{301}.".into()),
        };
        for width in [40, 60, 80, 100, 140] {
            let opts = ViewOpts {
                width,
                profile: Some((Network::Threads, user.clone())),
                ..opts(Some(0), None)
            };
            let posts = on(Network::Threads, vec![post("t1", "kal", None, 2)]);
            let view = render(&posts, &HashSet::new(), &opts);
            let text: String = view
                .lines
                .iter()
                .map(|line| format!("{}\n", line.plain()))
                .collect();
            assert!(text.contains("Writes about terminals."));
            assert!(text.contains("Second line with 界 and e\u{301}."));
            assert!(text.contains("1 post loaded"));
            assert!(view.lines.iter().all(|line| line.plain().width() <= width));
            assert_eq!(view.focus(), Some(view.rows[0]));
            assert_eq!(view.lines.iter().filter(|line| line.selected).count(), 1);
            let share = view
                .targets
                .iter()
                .find(|target| matches!(target.act, Act::ShareProfile(..)))
                .unwrap();
            assert_eq!(share.act, Act::ShareProfile(Network::Threads, "kal".into()));
            assert!(share.tab);
            assert!(view.targets.iter().any(|target| target.tab
                && target.act == Act::ProfileLink("https://www.threads.com/@kal".into())));
            assert!(text.contains("╭─ Profile "));
        }
    }

    #[test]
    fn a_feed_image_is_shorter_than_the_same_image_in_a_thread() {
        let world = crate::demo::world(now());
        let post = world.posts["x:l1"].clone();
        let media = |view: &View| view.previews.iter().find(|slot| !slot.avatar).unwrap().rows;
        let feed = render(std::slice::from_ref(&post), &HashSet::new(), &rich());
        assert_eq!(media(&feed), 8);
        let thread = render(
            std::slice::from_ref(&post),
            &HashSet::new(),
            &ViewOpts {
                thread: Some(post.id.clone()),
                ..rich()
            },
        );
        assert_eq!(media(&thread), 12);
        let profile = render(
            std::slice::from_ref(&post),
            &HashSet::new(),
            &ViewOpts {
                profile: Some((Network::X, post.author.clone())),
                ..rich()
            },
        );
        assert_eq!(media(&profile), 8);
        let short = render(
            std::slice::from_ref(&post),
            &HashSet::new(),
            &ViewOpts {
                height: Some(20),
                profile: Some((Network::X, post.author.clone())),
                ..rich()
            },
        );
        assert_eq!(media(&short), 4);
        assert!(profile
            .targets
            .iter()
            .any(|target| target.act == Act::Media("x:l1".into())));
    }

    #[test]
    fn a_tall_thread_grows_one_picture_and_the_page_keeps_its_shape() {
        assert_eq!(image_max_rows(false, true, false, None, Density::Roomy), 12);
        assert_eq!(
            image_max_rows(false, true, false, Some(80), Density::Roomy),
            32
        );
        assert_eq!(
            image_max_rows(false, true, false, Some(80), Density::Cozy),
            32
        );
        assert_eq!(
            image_max_rows(false, true, false, Some(80), Density::Compact),
            22
        );
        assert_eq!(
            image_max_rows(false, false, false, Some(80), Density::Roomy),
            8
        );
        assert_eq!(
            image_max_rows(false, false, true, Some(50), Density::Cozy),
            30
        );
        assert_eq!(fit_one(Some(1600), Some(900), 0.5, 58, 30), Some((58, 17)));
        assert_eq!(fit_one(Some(300), Some(900), 0.5, 58, 18), Some((12, 18)));
        assert_eq!(fit_one(None, None, 0.5, 58, 18), None);

        let post = with_photo(360, 180);
        let feed = render(std::slice::from_ref(&post), &HashSet::new(), &rich());
        assert_eq!(
            feed.previews.iter().find(|slot| !slot.avatar).unwrap().rows,
            8
        );
        let thread = render(
            std::slice::from_ref(&post),
            &HashSet::new(),
            &ViewOpts {
                thread: Some(post.id.clone()),
                height: Some(80),
                ..rich()
            },
        );
        let slot = thread.previews.iter().find(|slot| !slot.avatar).unwrap();
        assert_eq!((slot.cols, slot.rows), (58, 15));

        let page = render(
            std::slice::from_ref(&post),
            &HashSet::new(),
            &ViewOpts {
                page: true,
                height: Some(40),
                ..rich()
            },
        );
        let slot = page.previews.iter().find(|slot| !slot.avatar).unwrap();
        assert_eq!((slot.cols, slot.rows), (58, 15));

        let portrait = with_photo(300, 900);
        let narrow = render(
            std::slice::from_ref(&portrait),
            &HashSet::new(),
            &ViewOpts {
                page: true,
                height: Some(30),
                ..rich()
            },
        );
        let slot = narrow.previews.iter().find(|slot| !slot.avatar).unwrap();
        assert_eq!((slot.cols, slot.rows), (12, 18));
    }

    fn with_photo(width: u32, height: u32) -> Post {
        let mut post = post("pic", "ada", None, 5);
        post.media.push(Media {
            key: "pic".into(),
            kind: MediaKind::Photo,
            url: Some("https://example.com/pic.jpg".into()),
            preview_url: None,
            alt: None,
            width: Some(width),
            height: Some(height),
        });
        post
    }

    #[test]
    fn two_images_share_a_row_when_they_fit() {
        let world = crate::demo::world(now());
        let post = world.posts["x:l1"].clone();
        let opts = ViewOpts {
            width: 100,
            ..rich()
        };
        let view = render(std::slice::from_ref(&post), &HashSet::new(), &opts);
        let tiles: Vec<&Slot> = view.previews.iter().filter(|slot| !slot.avatar).collect();
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].x, TX);
        assert_eq!(tiles[0].cols, 21);
        assert_eq!(tiles[1].x, TX + 22);
        assert_eq!(tiles[1].cols, 32);
        assert_eq!(tiles[0].rows, 8);
        assert_eq!(tiles[0].y, tiles[1].y);
        assert_eq!(tiles[0].rows, tiles[1].rows);
        assert!(view
            .lines
            .iter()
            .any(|l| l.plain().contains("2 images · v view")));
        assert!(view
            .targets
            .iter()
            .any(|t| t.act == Act::Media("x:l1".into())));
        assert!(view.targets.iter().any(|t| t.act
            == Act::MediaAt {
                post: "x:l1".into(),
                index: 1
            }));
    }

    #[test]
    fn a_narrow_row_shrinks_both_images_to_share_it() {
        let world = crate::demo::world(now());
        let post = world.posts["x:l1"].clone();
        let opts = ViewOpts {
            width: 40,
            ..rich()
        };
        let view = render(std::slice::from_ref(&post), &HashSet::new(), &opts);
        let tiles: Vec<&Slot> = view.previews.iter().filter(|slot| !slot.avatar).collect();
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].cols, 11);
        assert_eq!(tiles[1].cols, 16);
        assert_eq!(tiles[0].rows, 4);
        assert_eq!(tiles[0].rows, tiles[1].rows);
        assert!(view
            .lines
            .iter()
            .any(|l| l.plain().contains("2 images · v view")));
    }

    #[test]
    fn a_row_shows_at_most_four_images() {
        let world = crate::demo::world(now());
        let mut post = world.posts["x:l1"].clone();
        let extra = post.media[1].clone();
        for n in 3..=5 {
            let mut media = extra.clone();
            media.key = format!("demo-extra-{n}");
            media.url = Some(format!("demo://extra-{n}"));
            post.media.push(media);
        }
        let opts = ViewOpts {
            width: 200,
            ..rich()
        };
        let view = render(std::slice::from_ref(&post), &HashSet::new(), &opts);
        let tiles: Vec<&Slot> = view.previews.iter().filter(|slot| !slot.avatar).collect();
        assert_eq!(tiles.len(), 4);
        assert!(tiles
            .iter()
            .all(|slot| slot.y == tiles[0].y && slot.rows == 8));
        let cols: Vec<usize> = tiles.iter().map(|slot| slot.cols).collect();
        assert_eq!(cols, vec![21, 32, 32, 32]);
        for pair in tiles.windows(2) {
            assert_eq!(pair[1].x, pair[0].x + pair[0].cols + 1);
        }
        assert!(view
            .lines
            .iter()
            .any(|l| l.plain().contains("4 of 5 · v view")));
    }

    #[test]
    fn a_long_list_post_keeps_three_lines_its_image_and_show_more() {
        let world = crate::demo::world(now());
        let mut post = world.posts["x:l1"].clone();
        post.text = "one\ntwo\nthree\nfour\nfive".into();
        post.in_reply_to = None;
        post.metrics.replies = 1;
        let lines_of =
            |view: &View| -> Vec<String> { view.lines.iter().map(|line| line.plain()).collect() };
        let view = render(std::slice::from_ref(&post), &HashSet::new(), &rich());
        let lines = lines_of(&view);
        let text = lines.join("\n");
        assert!(text.contains("one") && text.contains("three"), "{text}");
        assert!(!text.contains("four"), "{text}");
        assert!(text.contains("Show more"), "{text}");
        assert!(!text.contains("Show less"), "{text}");
        let image = view.previews.iter().find(|slot| !slot.avatar).unwrap();
        assert_eq!(image.rows, 8);
        let more = lines
            .iter()
            .position(|line| line.contains("Show more"))
            .unwrap();
        let fold = lines
            .iter()
            .position(|line| line.contains("Show this thread"))
            .unwrap();
        assert!(image.y < more && more < fold, "{lines:#?}");
        assert!(view
            .targets
            .iter()
            .any(|target| target.act == Act::More(post.id.clone())));

        let mut opened = HashSet::new();
        opened.insert(post.id.clone());
        let open = render(
            std::slice::from_ref(&post),
            &HashSet::new(),
            &ViewOpts { opened, ..rich() },
        );
        let text = lines_of(&open).join("\n");
        assert!(text.contains("four") && text.contains("five"), "{text}");
        assert!(text.contains("Show less"), "{text}");
        assert!(!text.contains("Show more"), "{text}");

        let thread = render(
            std::slice::from_ref(&post),
            &HashSet::new(),
            &ViewOpts {
                thread: Some(post.id.clone()),
                ..rich()
            },
        );
        let text = lines_of(&thread).join("\n");
        assert!(text.contains("five"), "{text}");
        assert!(
            !text.contains("Show more") && !text.contains("Show less"),
            "{text}"
        );
        assert_eq!(
            thread
                .previews
                .iter()
                .find(|slot| !slot.avatar)
                .unwrap()
                .rows,
            12
        );
    }

    #[test]
    fn a_gallery_row_gives_each_picture_its_own_width_at_one_height() {
        let sizes = [(Some(384), Some(296)), (Some(360), Some(180))];
        assert_eq!(gallery(&sizes, 0.5, 90, 12), (vec![32, 48], 12));
        assert_eq!(gallery(&sizes, 0.5, 60, 12), (vec![21, 32], 8));
        assert_eq!(gallery(&sizes, 0.5, 60, 36), (vec![21, 32], 8));
        let (one, _) = gallery(&sizes, 0.5, 30, 12);
        assert_eq!(one, vec![30]);
    }

    #[test]
    fn a_picture_without_a_size_gets_a_four_by_three_box() {
        let sizes = [(None, None), (Some(360), Some(180))];
        assert_eq!(gallery(&sizes, 0.5, 90, 12), (vec![32, 48], 12));
        assert_eq!(aspect(Some(100), Some(1000)), ASPECT_MIN);
    }

    fn grid(view: &View) -> Vec<Vec<char>> {
        view.lines
            .iter()
            .map(|l| l.plain().chars().collect())
            .collect()
    }

    fn at(row: &[char], col: usize, len: usize) -> String {
        row.iter().skip(col).take(len).collect()
    }

    fn row_with<'a>(rows: &'a [Vec<char>], needle: &str) -> &'a [char] {
        rows.iter()
            .find(|r| r.iter().collect::<String>().contains(needle))
            .unwrap_or_else(|| panic!("no row with {needle}"))
    }

    fn linked(image: Option<&str>, size: Option<(u32, u32)>) -> Post {
        let mut root = post("r", "root", None, 5);
        root.links = vec![Link {
            short: "https://example.com/page".into(),
            expanded: "https://example.com/page".into(),
            display: "example.com/page".into(),
            title: Some("A page with a preview".into()),
            description: Some("What the page is about, in a sentence or two of text.".into()),
            image: image.map(str::to_string),
            image_width: size.map(|(width, _)| width),
            image_height: size.map(|(_, height)| height),
            site: None,
        }];
        root
    }

    fn rich() -> ViewOpts {
        ViewOpts {
            previews: Some(0.5),
            ..opts(None, None)
        }
    }

    #[test]
    fn avatars_reserve_the_letter_box_only_when_asked() {
        let mut posts = tree();
        posts[0].author.avatar_url = Some("https://example.com/root.png".into());
        assert!(render(&posts, &HashSet::new(), &rich()).previews.is_empty());
        let opts = ViewOpts {
            avatars: true,
            ..rich()
        };
        let view = render(&posts, &HashSet::new(), &opts);
        let avatars: Vec<&Slot> = view.previews.iter().filter(|slot| slot.avatar).collect();
        assert_eq!(avatars.len(), 1);
        let slot = avatars[0];
        assert_eq!((slot.x, slot.cols, slot.rows), (ROOT_AVATAR, 5, 3));
        let rows = grid(&view);
        assert_eq!(at(&rows[slot.y], ROOT_AVATAR, 5), "╭───╮");
        assert_eq!(at(&rows[slot.y + 2], ROOT_AVATAR, 1), "╰");
    }

    #[test]
    fn wide_link_images_fill_the_top_of_the_card() {
        let posts = [linked(
            Some("https://example.com/og.png"),
            Some((1200, 630)),
        )];
        let view = render(&posts, &HashSet::new(), &rich());
        assert_eq!(view.previews.len(), 1);
        let slot = &view.previews[0];
        assert_eq!((slot.x, slot.cols, slot.rows), (TX + 1, 62, 17));
        let rows = grid(&view);
        let top = slot.y - 1;
        assert_eq!(at(&rows[top], TX, 1), "╭");
        assert_eq!(at(&rows[top], TX + 63, 1), "╮");
        assert_eq!(at(&rows[slot.y + 17], TX + 2, 21), "A page with a preview");
        assert_eq!(at(&rows[slot.y + 18], TX + 2, 11), "example.com");
        assert_eq!(at(&rows[slot.y + 19], TX, 1), "╰");
        assert!(view.unfurls.is_empty());
    }

    #[test]
    fn tall_link_images_are_centered_at_the_row_cap() {
        let posts = [linked(Some("https://example.com/og.png"), Some((800, 600)))];
        let view = render(&posts, &HashSet::new(), &rich());
        let slot = &view.previews[0];
        assert_eq!((slot.x, slot.cols, slot.rows), (TX + 8, 48, 18));
    }

    #[test]
    fn small_link_images_sit_beside_the_text() {
        let posts = [linked(Some("https://example.com/sq.png"), Some((150, 150)))];
        let view = render(&posts, &HashSet::new(), &rich());
        let slot = &view.previews[0];
        assert_eq!((slot.x, slot.cols, slot.rows), (TX + 1, 12, 6));
        let rows = grid(&view);
        assert_eq!(at(&rows[slot.y], TX + 14, 21), "A page with a preview");
        assert_eq!(at(&rows[slot.y + 1], TX + 14, 16), "What the page is");
        assert_eq!(at(&rows[slot.y + 6], TX, 1), "╰");
    }

    #[test]
    fn rich_cards_without_an_image_ask_for_an_unfurl() {
        let posts = [linked(None, None)];
        let view = render(&posts, &HashSet::new(), &rich());
        assert!(view.previews.is_empty());
        assert_eq!(view.unfurls, ["https://example.com/page"]);
        let standard = render(
            &[linked(Some("https://example.com/og.png"), None)],
            &HashSet::new(),
            &opts(None, None),
        );
        assert!(standard.previews.is_empty() && standard.unfurls.is_empty());
    }

    fn dense(density: Density, thread: Option<&str>) -> ViewOpts {
        ViewOpts {
            density,
            ..opts(None, thread)
        }
    }

    const DENSITIES: [Density; 3] = [Density::Roomy, Density::Cozy, Density::Compact];

    /// A row holding only the root rail.
    fn rail_only(row: &[char]) -> bool {
        row.iter()
            .enumerate()
            .all(|(i, c)| *c == if i == ROOT_RAIL { '╎' } else { ' ' })
    }

    fn with_link(post: Post) -> Post {
        Post {
            links: vec![Link {
                short: "https://t.co/x".into(),
                expanded: "https://example.com".into(),
                display: "example.com".into(),
                title: Some("Example".into()),
                description: None,
                ..Default::default()
            }],
            ..post
        }
    }

    fn index_of(rows: &[Vec<char>], found: impl Fn(&[char]) -> bool) -> usize {
        rows.iter()
            .position(|r| found(r))
            .unwrap_or_else(|| panic!("no such row"))
    }

    /// The text under `region` on its line, by column.
    fn linked_text(view: &View, region: &Region) -> String {
        let row: Vec<char> = view.lines[usize::from(region.y)]
            .plain()
            .chars()
            .chain(std::iter::repeat(' '))
            .take(200)
            .collect();
        at(&row, usize::from(region.x), usize::from(region.width))
    }

    #[test]
    fn a_header_links_the_byline_to_the_profile_and_the_time_to_the_post() {
        let mut root = with_link(post("r", "river", None, 120));
        root.url = Some("https://x.com/river/status/1".into());
        root.text = "ping @nuno.bsky and https://t.co/x now".into();
        let view = render(&[root], &HashSet::new(), &opts(None, None));
        let found: Vec<(String, &str)> = view
            .links
            .iter()
            .map(|region| (linked_text(&view, region), region.url.as_str()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("RIVER @river".into(), "https://x.com/river"),
                ("2h".into(), "https://x.com/river/status/1"),
                ("@nuno.bsky".into(), "https://x.com/nuno.bsky"),
                ("https://t.co/x".into(), "https://example.com"),
                ("↗ example.com".into(), "https://example.com"),
                ("Example".into(), "https://example.com"),
            ]
        );
        assert_eq!((view.links[0].x, view.links[0].y), (TX as u16, 0));
        assert_eq!(view.links[2].y, 1);
    }

    #[test]
    fn link_words_trim_trailing_punctuation() {
        let p = post("r", "river", None, 1);
        assert_eq!(
            link_in("https://example.com/a).", &p),
            Some((21, "https://example.com/a".into()))
        );
        assert_eq!(
            link_in("@nuno.", &p),
            Some((5, "https://x.com/nuno".into()))
        );
        assert_eq!(link_in("@", &p), None);
        assert_eq!(link_in("plain", &p), None);
    }

    #[test]
    fn demo_posts_sit_on_the_m_grid() {
        use crate::config::Config;
        use crate::net::Clients;
        let mut state =
            crate::state::State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.icons = false;
        state.detail = Detail::Rich;
        let view = state.view(100, true);
        let rows = grid(&view);
        let root = index_of(&rows, |r| at(r, TX, 9) == "Nuno Park");
        assert!(
            view.lines[root].bar,
            "the selected root carries the margin bar"
        );
        assert_eq!(
            rows[root][0], ' ',
            "terminal col 2 stays blank, the bar sits at col 1"
        );
        assert_eq!(
            at(&rows[root], 1, 5),
            "╭───╮",
            "root avatar at terminal cols 3–7"
        );
        assert_eq!(at(&rows[root + 2], 1, 5), "╰─┬─╯");
        assert_eq!(rows[root + 3][3], '╎', "root rail at terminal col 5");
        let reply = row_with(&rows, "Priya Shah");
        assert_eq!(
            at(reply, 7, 5),
            "╭───╮",
            "reply avatar at terminal cols 9–13"
        );
        assert_eq!(at(reply, 14, 5), "Priya", "reply text at terminal col 16");
        assert_eq!(at(row_with(&rows, "┤ P │"), 3, 9), "├╌╌╌┤ P │");
        let stats = row_with(&rows, "⇄ 7");
        assert_eq!(at(stats, 8, 3), "↩ 2", "stats at terminal col 10");
        assert_eq!(at(stats, 16, 3), "⇄ 7");
        assert_eq!(at(stats, 24, 4), "♡ 40");
        assert_eq!(at(stats, 32, 1), "☆");
        assert_eq!(stats.len(), 100);
        assert!(stats.iter().collect::<String>().ends_with("◉ 4K"));
        let card = index_of(&rows, |r| r.get(TX) == Some(&'╭'));
        assert_eq!(rows[card][TX + 57], '╮', "cards are 58 columns wide");
    }

    #[test]
    fn standard_shows_one_media_row() {
        use crate::config::Config;
        use crate::net::Clients;
        let mut state =
            crate::state::State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.icons = false;
        let view = state.view(100, true);
        let rows = grid(&view);
        let text = |row: &[char]| row.iter().collect::<String>();
        let media = index_of(&rows, |r| text(r).contains("image · A rising sparkline"));
        let start = rows[..media]
            .iter()
            .rposition(|r| text(r).contains("ada.bsky.social"))
            .unwrap();
        assert_eq!(
            text(&rows[media]).trim().trim_start_matches(['╎', ' ']),
            "▣ image · A rising sparkline on a black field.  view"
        );
        let stats = text(&rows[media + 1]);
        assert!(
            stats.contains("↩ 1") && stats.contains("⇄ 2") && stats.contains("♡ 21"),
            "{stats}"
        );
        assert!(rows[start..=media + 1]
            .iter()
            .all(|row| row.get(TX) != Some(&'╭')));
    }

    #[test]
    fn density_sets_the_rows_between_root_trees() {
        let mut a = post("a", "amy", None, 10);
        a.conversation_id = "a".into();
        let mut b = post("b", "bob", None, 20);
        b.conversation_id = "b".into();
        let posts = vec![a, b];
        let laid = layout::lay(&posts, &HashSet::new(), false);
        let gap = laid.iter().position(layout::is_gap).unwrap();
        for (density, want) in DENSITIES.into_iter().zip([1, 1, 0]) {
            let view = render(&posts, &HashSet::new(), &dense(density, None));
            let span = view.rows[gap];
            assert_eq!(span.end - span.start, want, "{density:?}");
        }
    }

    #[test]
    fn density_sets_the_rail_spacer_before_root_stats() {
        for (density, want) in DENSITIES.into_iter().zip([true, false, false]) {
            let rows = grid(&render(&tree(), &HashSet::new(), &dense(density, None)));
            let stats = index_of(&rows, |r| at(r, TX, 3) == "↩ 5");
            assert_eq!(rail_only(&rows[stats - 1]), want, "{density:?}");
        }
    }

    #[test]
    fn density_sets_the_blank_before_a_card() {
        let mut root = with_link(post("r", "root", None, 10));
        root.text = "one\ntwo\nthree".into();
        let posts = [root];
        for (density, want) in DENSITIES.into_iter().zip([true, true, false]) {
            let rows = grid(&render(&posts, &HashSet::new(), &dense(density, None)));
            let card = index_of(&rows, |r| r.get(TX) == Some(&'╭'));
            assert_eq!(rail_only(&rows[card - 1]), want, "{density:?}");
        }
    }

    #[test]
    fn density_moves_the_detail_handle_to_the_name_row() {
        for (density, own_row) in DENSITIES.into_iter().zip([true, false, false]) {
            let rows = grid(&render(
                &tree(),
                &HashSet::new(),
                &dense(density, Some("r")),
            ));
            let name = index_of(&rows, |r| at(r, TX, 4) == "ROOT");
            assert_eq!(
                at(&rows[name + 1], TX, 5) == "@root",
                own_row,
                "{density:?}"
            );
            assert_eq!(
                at(&rows[name], TX + 5, 5) == "@root",
                !own_row,
                "{density:?}"
            );
        }
    }

    #[test]
    fn density_sets_the_blank_before_the_detail_timestamp() {
        let mut posts = tree();
        posts[0] = with_link(posts[0].clone());
        for (density, want) in DENSITIES.into_iter().zip([true, false, false]) {
            let rows = grid(&render(&posts, &HashSet::new(), &dense(density, Some("r"))));
            let when = index_of(&rows, |r| r.iter().collect::<String>().contains(" views"));
            assert_eq!(rail_only(&rows[when - 1]), want, "{density:?}");
            assert_eq!(rows[when - 1].get(TX) == Some(&'╰'), !want, "{density:?}");
        }
    }

    #[test]
    fn density_sets_the_rows_between_stats_and_the_first_reply() {
        for (density, want) in DENSITIES.into_iter().zip([1, 1, 0]) {
            let rows = grid(&render(&tree(), &HashSet::new(), &dense(density, None)));
            let stats = index_of(&rows, |r| at(r, TX, 3) == "↩ 5");
            let reply = index_of(&rows, |r| at(r, REPLY_TX, 3) == "AMY");
            assert_eq!(reply - stats - 1, want, "{density:?}");
            for row in &rows[stats + 1..reply] {
                assert!(rail_only(row), "{density:?}");
            }
        }
    }

    #[test]
    fn cozy_closes_a_lone_root_and_lifts_its_stats() {
        let mut lone = post("r", "root", None, 10);
        lone.metrics.replies = 0;
        let posts = [lone];
        let rows = grid(&render(
            &posts,
            &HashSet::new(),
            &dense(Density::Cozy, None),
        ));
        assert_eq!(rows.len(), 3);
        assert_eq!(at(&rows[2], 1, 5), "╰───╯");
        assert_eq!(at(&rows[2], TX, 3), "↩ 0");
        let rows = grid(&render(
            &posts,
            &HashSet::new(),
            &dense(Density::Roomy, None),
        ));
        assert_eq!(at(&rows[2], 1, 5), "╰─┬─╯");
        assert!(rows.len() > 3);
    }

    #[test]
    fn replies_hang_off_elbows_and_chains() {
        let view = render(&tree(), &HashSet::new(), &opts(None, None));
        let rows = grid(&view);
        let a1 = row_with(&rows, "text of a1");
        assert_eq!(at(a1, ELBOW, 5), "├╌╌╌┤");
        assert_eq!(at(a1, REPLY_TX, 10), "text of a1");
        let a2 = row_with(&rows, "text of a2");
        assert_eq!(at(a2, ELBOW, 5), "╰╌╌╌┤");
        let b1 = row_with(&rows, "text of b1");
        assert_eq!(a1.get(CHAIN), Some(&'A'));
        assert_eq!(b1.get(ROOT_RAIL), Some(&'╎'));
        let d1 = row_with(&rows, "text of d1");
        assert_eq!(d1.get(ROOT_RAIL), Some(&' '));
        let heads: Vec<String> = rows
            .iter()
            .filter(|r| at(r, REPLY_TX, 3) == "AMY" || at(r, REPLY_TX, 3) == "BOB")
            .map(|r| at(r, REPLY_AVATAR, 5))
            .collect();
        assert_eq!(heads, ["╭───╮", "╭─┴─╮"]);
        let fold = row_with(&rows, "Show 1 more replies");
        assert_eq!(fold.get(CHAIN), Some(&'○'));
        assert_eq!(at(fold, REPLY_TX, 19), "Show 1 more replies");
    }

    #[test]
    fn stats_sit_on_the_grid() {
        let view = render(&tree(), &HashSet::new(), &opts(None, None));
        let rows = grid(&view);
        let stats = row_with(&rows, "↩ 5");
        assert_eq!(at(stats, TX, 3), "↩ 5");
        assert_eq!(at(stats, TX + 8, 3), "⇄ 2");
        assert_eq!(at(stats, TX + 16, 3), "♡ 9");
        assert_eq!(at(stats, TX + 24, 1), "☆");
        assert!(stats.iter().collect::<String>().ends_with("◉ 1.2K"));
        assert_eq!(stats.len(), 100);
    }

    #[test]
    fn action_targets_are_equal_and_never_collide_with_views() {
        for width in [40, 48, 60, 80, 100] {
            for icons in [false, true] {
                for thread in [None, Some("r")] {
                    let mut post = post("r", "root", None, 5);
                    post.metrics.replies = 12_300;
                    post.metrics.reposts = 99_900;
                    post.metrics.likes = 1_200_000;
                    let opts = ViewOpts {
                        width,
                        icons,
                        ..opts(None, thread)
                    };
                    let view = render(&[post], &HashSet::new(), &opts);
                    let actions: Vec<_> = view
                        .targets
                        .iter()
                        .filter(|target| {
                            matches!(
                                target.act,
                                Act::Reply(_) | Act::Repost(_) | Act::Like(_) | Act::Bookmark(_)
                            )
                        })
                        .collect();
                    assert_eq!(actions.len(), 4);
                    assert_eq!(
                        actions.iter().map(|target| target.x).collect::<Vec<_>>(),
                        [8, 16, 24, 32]
                    );
                    assert!(actions.iter().all(|target| target.width == 7 && target.tab));
                    let row = &view.lines[usize::from(actions[0].y)];
                    let text = row.plain();
                    let glyphs = Icons::new(icons);
                    assert!(text.contains(&format!("{} 99.9K", glyphs.repost)));
                    assert!(text.contains(glyphs.bookmark));
                    assert!(row.plain().width() <= width);
                    if thread.is_none() {
                        assert_eq!(text.contains(glyphs.views), width >= 48);
                    }
                }
            }
        }
    }

    #[test]
    fn narrow_action_rows_pack_from_the_text_column() {
        for width in [38, 30, 20] {
            for icons in [false, true] {
                for thread in [None, Some("r")] {
                    let mut post = post("r", "root", None, 5);
                    post.metrics.replies = 12_300;
                    post.metrics.reposts = 99_900;
                    post.metrics.likes = 1_200_000;
                    post.metrics.views = 45_000;
                    let opts = ViewOpts {
                        width,
                        icons,
                        ..opts(None, thread)
                    };
                    let view = render(&[post], &HashSet::new(), &opts);
                    let actions: Vec<&Target> = view
                        .targets
                        .iter()
                        .filter(|target| {
                            matches!(
                                target.act,
                                Act::Reply(_) | Act::Repost(_) | Act::Like(_) | Act::Bookmark(_)
                            )
                        })
                        .collect();
                    let what = format!("{width} {icons} {thread:?}");
                    assert_eq!(usize::from(actions[0].x), TX, "{what}");
                    let row: Vec<char> = view.lines[usize::from(actions[0].y)]
                        .plain()
                        .chars()
                        .collect();
                    for pair in actions.windows(2) {
                        assert!(pair[0].x + pair[0].width <= pair[1].x, "{what}");
                    }
                    for target in &actions {
                        let end = usize::from(target.x + target.width);
                        assert!(end <= width, "{what}: {target:?}");
                        assert_eq!(row.get(end - 1).unwrap_or(&' '), &' ', "{what}: {target:?}");
                        assert_ne!(row[usize::from(target.x)], ' ', "{what}: {target:?}");
                    }
                    let glyphs = Icons::new(icons);
                    let icons_only = TX
                        + [glyphs.reply, glyphs.repost, glyphs.like, glyphs.bookmark]
                            .iter()
                            .map(|icon| icon.width() + 1)
                            .sum::<usize>();
                    if icons_only <= width {
                        assert!(
                            actions
                                .iter()
                                .any(|target| matches!(target.act, Act::Bookmark(_))),
                            "{what}"
                        );
                    }
                    let text: String = row.iter().collect();
                    assert!(!text.contains(glyphs.views), "{what}: {text}");
                    assert!(!text.contains("45K"), "{what}: {text}");
                }
            }
        }
    }

    #[test]
    fn a_long_author_line_moves_the_handle_under_the_name() {
        let mut root = post("r", "rivers_and_lakes", None, 5);
        root.author.name = "River Hale of the Long Name".into();
        let mut reply = post("a", "ann_the_replier", Some("r"), 3);
        reply.author.name = "Ann Who Replies At Length".into();
        for thread in [None, Some("r")] {
            let opts = ViewOpts {
                width: 32,
                tags: true,
                ..opts(None, thread)
            };
            let view = render(&[root.clone(), reply.clone()], &HashSet::new(), &opts);
            let rows = grid(&view);
            for row in &rows {
                assert!(
                    row.len() <= 32,
                    "{thread:?}: {}",
                    row.iter().collect::<String>()
                );
            }
            let name = rows
                .iter()
                .position(|row| row.iter().collect::<String>().contains("River Hale"))
                .unwrap();
            assert!(!rows[name].contains(&'@'), "{thread:?}");
            let under: String = rows[name + 1].iter().collect();
            assert!(
                under[under.find('@').unwrap()..].starts_with("@rivers_and_lakes"),
                "{thread:?}: {under}"
            );
            assert!(view.targets.iter().any(
                |target| usize::from(target.y) == name && matches!(target.act, Act::Author(_))
            ));
            let body = rows
                .iter()
                .any(|row| row.iter().collect::<String>().contains("text of r"));
            assert!(body, "{thread:?}");
        }
    }

    #[test]
    fn a_link_card_too_narrow_to_box_becomes_the_link_row() {
        let mut root = with_link(post("r", "root", None, 5));
        root.text = "a link below".into();
        let opts = ViewOpts {
            width: 20,
            ..opts(None, None)
        };
        let view = render(&[root], &HashSet::new(), &opts);
        let rows: Vec<String> = view.lines.iter().map(StyledLine::plain).collect();
        assert!(
            rows.iter().all(|row| row.chars().nth(TX) != Some('╭')),
            "{rows:#?}"
        );
        assert!(
            rows.iter().any(|row| row.ends_with("    example.com")),
            "{rows:#?}"
        );
        let link = view
            .targets
            .iter()
            .find(|target| matches!(target.act, Act::Link { .. }))
            .unwrap();
        assert_eq!(usize::from(link.x), TX);
        assert!(view.links.iter().any(|region| region.y == link.y));
    }

    #[test]
    fn extreme_counts_stay_inside_their_action_targets() {
        let mut post = post("r", "root", None, 5);
        post.metrics.replies = u64::MAX;
        post.metrics.reposts = u64::MAX;
        post.metrics.likes = u64::MAX;
        let opts = ViewOpts {
            width: 40,
            ..opts(None, None)
        };
        let view = render(&[post], &HashSet::new(), &opts);
        let row = view
            .lines
            .iter()
            .find(|line| line.plain().contains("↩"))
            .unwrap()
            .plain();
        let cells: Vec<_> = row.chars().collect();
        for (col, glyph) in STATS.into_iter().zip(['↩', '⇄', '♡', '☆']) {
            assert_eq!(cells[col], glyph);
        }
        assert_eq!(cells[15], ' ');
        assert_eq!(cells[23], ' ');
        assert_eq!(cells[31], ' ');
    }

    #[test]
    fn thread_root_uses_the_detail_layout() {
        let view = render(&tree(), &HashSet::new(), &opts(None, Some("r")));
        let rows = grid(&view);
        let handle = row_with(&rows, "@root");
        assert_eq!(at(handle, TX, 5), "@root");
        let stats = row_with(&rows, "↩ 5");
        assert_eq!(at(stats, TX + 8, 3), "⇄ 2");
        assert_eq!(at(stats, TX + 16, 3), "♡ 9");
        assert_eq!(at(stats, TX + 24, 1), "☆");
    }

    #[test]
    fn selection_covers_exactly_the_block() {
        let posts = tree();
        let laid = layout::lay(&posts, &HashSet::new(), false);
        let a2 = laid
            .iter()
            .position(|r| layout::post_id(r) == Some("a2"))
            .unwrap();
        let view = render(&posts, &HashSet::new(), &opts(Some(a2), None));
        let span = view.focus().unwrap();
        assert_eq!(view.rows[a2], span);
        assert_eq!(span.end - span.start, 3);
        for (i, line) in view.lines.iter().enumerate() {
            let inside = (span.start..span.end).contains(&i);
            assert_eq!(line.selected, inside, "row {i}");
            assert_eq!(line.bar, inside, "row {i}");
        }
    }

    #[test]
    fn unicode_icons_stay_out_of_the_private_use_area() {
        let mut posts = tree();
        posts[0].liked = true;
        posts[0].bookmarked = true;
        posts[0].reposted_by = Some("kal".into());
        let mut text = String::new();
        for thread in [None, Some("r")] {
            for line in render(&posts, &HashSet::new(), &opts(Some(0), thread)).lines {
                text.push_str(&line.plain());
            }
        }
        for line in notifications(&posts, &HashSet::new(), &opts(Some(0), None)).lines {
            text.push_str(&line.plain());
        }
        let pua = |c: char| matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD);
        assert!(!text.chars().any(pua), "{text}");
    }

    #[test]
    fn body_mentions_and_links_use_the_link_color() {
        let mut canvas = Canvas::new(60);
        let y = canvas.push(false);
        let p = post("r", "river", None, 1);
        paint_body(&mut canvas, y, 0, "hi @mina, see https://x.com now @", &p);
        let line = canvas.finish_line();
        let blue: Vec<&str> = line
            .spans
            .iter()
            .filter(|s| s.kind == Kind::Link)
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(blue, ["@mina", "https://x.com"]);
    }

    #[test]
    fn devnull_stats_leave_the_repost_column_empty() {
        let posts = on(Network::Devnull, vec![post("r", "root", None, 60)]);
        let view = render(&posts, &HashSet::new(), &opts(None, None));
        let rows = grid(&view);
        let stats = row_with(&rows, "↩ 5");
        assert_eq!(at(stats, TX, 8), "↩ 5     ");
        assert_eq!(at(stats, TX + 8, 8), "        ");
        assert_eq!(at(stats, TX + 16, 3), "♡ 9");
        assert_eq!(stats.len(), TX + 19, "no bookmark cell");
        assert!(!stats.iter().collect::<String>().contains("1.2K"));
    }

    #[test]
    fn threads_stats_are_icons_without_counts() {
        let posts = on(Network::Threads, vec![post("r", "root", None, 60)]);
        let view = render(&posts, &HashSet::new(), &opts(None, None));
        let rows = grid(&view);
        let stats = row_with(&rows, "↩");
        assert_eq!(at(stats, TX, 8), "↩       ");
        assert_eq!(at(stats, TX + 8, 1), "⇄");
        assert_eq!(stats.len(), TX + 9, "no like or bookmark cell");
    }

    #[test]
    fn merged_timelines_tag_roots_with_their_network() {
        let posts = on(Network::Bluesky, tree());
        let tagged = ViewOpts {
            tags: true,
            ..opts(None, None)
        };
        let rows = grid(&render(&posts, &HashSet::new(), &tagged));
        let root = row_with(&rows, "ROOT");
        assert_eq!(at(root, TX, 26), "ROOT @root · 1h · bluesky ");
        let amy = row_with(&rows, "AMY");
        assert!(!amy.iter().collect::<String>().contains("bluesky"));
        let plain = grid(&render(&posts, &HashSet::new(), &opts(None, None)));
        assert!(!row_with(&plain, "ROOT")
            .iter()
            .collect::<String>()
            .contains("bluesky"));
    }

    // MARK: graphemes and controls

    fn put_line(width: usize, col: usize, text: &str) -> (usize, Canvas) {
        let mut canvas = Canvas::new(width);
        let y = canvas.push(false);
        let end = canvas.put(y, col, Kind::Fg, text);
        (end, canvas)
    }

    #[test]
    fn graphemes_round_trip_through_the_canvas() {
        let texts = crate::fixtures::graphemes()
            .into_iter()
            .map(|post| post.text);
        for text in crate::fixtures::GRAPHEME_TEXTS
            .iter()
            .map(|t| t.to_string())
            .chain(texts)
        {
            let (end, canvas) = put_line(80, 0, &text);
            assert_eq!(end, text.width(), "{text:?}");
            assert_eq!(canvas.finish_line().plain(), text, "{text:?}");
        }
    }

    #[test]
    fn a_zwj_emoji_takes_two_cells_and_a_decomposed_accent_one() {
        let (end, canvas) = put_line(20, 0, "👩‍💻x");
        assert_eq!(end, 3);
        assert!(matches!(&canvas.rows[0][0].glyph, Glyph::Cluster(c) if &**c == "👩‍💻"));
        assert!(matches!(canvas.rows[0][1].glyph, Glyph::Cont));
        assert!(matches!(canvas.rows[0][2].glyph, Glyph::Char('x')));
        let (end, canvas) = put_line(20, 0, "e\u{301}x");
        assert_eq!(end, 2);
        assert!(matches!(&canvas.rows[0][0].glyph, Glyph::Cluster(c) if &**c == "e\u{301}"));
        assert!(matches!(canvas.rows[0][1].glyph, Glyph::Char('x')));
    }

    #[test]
    fn a_wide_grapheme_at_the_last_column_stops_the_write() {
        let (end, canvas) = put_line(20, 17, "ab日c");
        assert_eq!(end, 19);
        assert!(matches!(canvas.rows[0][19].glyph, Glyph::Char(' ')));
        assert_eq!(
            canvas.finish_line().plain(),
            format!("{}ab", " ".repeat(17))
        );
    }

    #[test]
    fn a_zero_width_grapheme_joins_the_cell_before_it() {
        let (end, canvas) = put_line(20, 0, "\u{200b}a\u{200b}日\u{200b}");
        assert_eq!(end, 3);
        assert!(matches!(&canvas.rows[0][0].glyph, Glyph::Cluster(c) if &**c == "a\u{200b}"));
        assert!(matches!(&canvas.rows[0][1].glyph, Glyph::Cluster(c) if &**c == "日\u{200b}"));
        assert_eq!(canvas.finish_line().plain(), "a\u{200b}日\u{200b}");
    }

    #[test]
    fn overwriting_half_a_wide_grapheme_blanks_the_other_half() {
        let mut canvas = Canvas::new(20);
        let y = canvas.push(false);
        canvas.put(y, 0, Kind::Fg, "日本");
        canvas.put(y, 1, Kind::Fg, "x");
        canvas.put(y, 2, Kind::Fg, "y");
        assert_eq!(canvas.finish_line().plain(), " xy");
    }

    #[test]
    fn the_canvas_and_text_lines_drop_terminal_controls() {
        for text in crate::fixtures::CONTROL_TEXTS {
            let shown = util::clean(text).replace('\n', " ");
            let (_, canvas) = put_line(80, 0, text);
            assert_eq!(canvas.finish_line().plain(), shown, "{text:?}");
            assert_eq!(StyledLine::text(Kind::Fg, text).plain(), shown);
        }
    }

    #[test]
    fn a_line_break_left_by_cleaning_becomes_a_space_in_the_row() {
        let (end, canvas) = put_line(20, 0, "a\rb");
        assert_eq!(end, 3);
        assert!(matches!(canvas.rows[0][1].glyph, Glyph::Char(' ')));
        assert_eq!(canvas.finish_line().plain(), "a b");
    }
    #[test]
    fn action_columns_stay_the_same_across_networks_at_every_width() {
        let state = crate::fixtures::demo_state(Detail::Standard);
        let origin = text_origin(Detail::Standard);
        let full = origin + 3 * ACTION_STEP + ACTION_WIDTH;
        for width in [24, 32, 38, 39, 60, 100] {
            let view = state.view(width, true);
            let step = if width < full {
                width.saturating_sub(origin) / 4
            } else {
                ACTION_STEP
            };
            let mut checked = 0;
            for target in &view.targets {
                let slot = match target.act {
                    Act::Reply(_) => 0,
                    Act::Repost(_) => 1,
                    Act::Like(_) => 2,
                    Act::Bookmark(_) => 3,
                    _ => continue,
                };
                assert_eq!(
                    usize::from(target.x),
                    origin + slot * step,
                    "{width}: {target:?}"
                );
                assert!(usize::from(target.x + target.width) <= width);
                checked += 1;
            }
            assert!(checked > 4);
        }
    }
}
