//! Full-screen reader. The chrome matches design/: breadcrumb, tabs, cards, footer.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::io::{self, stdout, IsTerminal};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
#[cfg(feature = "native")]
use std::sync::{MutexGuard, TryLockError};
use std::time::Duration;

use crate::clock::Instant;

use crate::key::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode, MouseButton, MouseEvent,
    MouseEventKind,
};
#[cfg(feature = "native")]
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
#[cfg(feature = "native")]
use crossterm::execute;
#[cfg(feature = "native")]
use crossterm::terminal::{
    self, disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
#[cfg(feature = "native")]
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;
#[cfg(feature = "native")]
use ratatui_image::picker::ProtocolType;
#[cfg(feature = "native")]
use ratatui_image::{Resize, ResizeEncodeRender, StatefulImage};
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::chrome::{self, Hints};
use crate::clip;
use crate::command::{self, Cmd};
use crate::config::Enter;
use crate::drafts::{self, Draft};
use crate::hyperlink::{self, Region};
use crate::intent::Intent;
use crate::jsonview;
use crate::media::{self, ImageMode, Origin};
use crate::model::{FeedId, Post, Scope, Section};
use crate::net::{self, Clients, Network};
use crate::onboard;
use crate::preview::{self, Previews, Shown};
use crate::render::{
    self, Act, Canvas, Kind, OverflowItem, RowSpan, Slot, StyledLine, Target, Zone,
};
use crate::rich;
use crate::screens::{self, Body, Share, ShareAction, ShareStep};
use crate::send;
use crate::settings::{self, Row};
use crate::state::{self, MergedTab, Outcome, SettingsPage, State};
use crate::theme::{self, Choice, Density, Detail, Icons, Layout, Rgb, Scheme, Theme};
use crate::unfurl::Unfurls;
use crate::util;
#[cfg(feature = "native")]
use crate::watchdog;

static TERMINAL_SUSPENDED: AtomicBool = AtomicBool::new(false);
static TERMINAL_HANDOFFS: AtomicUsize = AtomicUsize::new(0);

struct TerminalHandoff;

impl TerminalHandoff {
    fn begin() -> Self {
        TERMINAL_SUSPENDED.store(true, Ordering::SeqCst);
        Self
    }
}

impl Drop for TerminalHandoff {
    fn drop(&mut self) {
        TERMINAL_HANDOFFS.fetch_add(1, Ordering::SeqCst);
        TERMINAL_SUSPENDED.store(false, Ordering::SeqCst);
    }
}

/// Autosave writes a dirty composer at most this often.
const AUTOSAVE: Duration = Duration::from_secs(3);
/// The screen row of the rule under the sub-bar, where the filter panel starts.
const RULE_ROW: usize = 4;
/// Columns from which `layout auto` puts the feed and the thread side by side.
const SPLIT_AUTO: usize = 124;
/// The narrowest terminal `layout split` still splits.
const SPLIT_MIN: usize = 100;
/// The fewest columns the split's feed pane gets: one-line author rows and the action grid fit.
const FEED_MIN: usize = 44;
/// The most columns the split's feed pane gets, so the thread pane keeps the room.
const FEED_MAX: usize = 64;
/// How long a key's work may run before the footer says it is loading.
const BUSY_AFTER: Duration = Duration::from_millis(150);
/// One frame of the rich level's spinners.
const SPIN: Duration = Duration::from_millis(120);
const SPINNER: [char; 4] = ['◐', '◓', '◑', '◒'];
/// How long the reader waits for input while nothing on screen moves.
const IDLE_WAIT: Duration = Duration::from_secs(1);
/// An idle reader still redraws this often, so relative times stay fresh.
const REFRESH: Duration = Duration::from_secs(30);
/// Queued events one batch takes at most, and how long it keeps reading them.
const BATCH: usize = 64;
const BATCH_TIME: Duration = Duration::from_millis(8);

/// Undo steps an input keeps; the oldest goes first.
const HISTORY: usize = 100;

/**
A text field. `cursor` is a byte offset that always sits on an extended grapheme cluster boundary,
so every move and delete covers whole graphemes. Typed characters join one undo step until
whitespace, a cursor move or another edit ends it.
*/
struct Input {
    text: String,
    cursor: usize,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    /// The last edit typed a character the next one may join in the same undo step.
    typing: bool,
}

impl Input {
    fn new() -> Self {
        Self::from("")
    }

    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
            cursor: text.len(),
            undo: Vec::new(),
            redo: Vec::new(),
            typing: false,
        }
    }

    fn text(&self) -> &str {
        &self.text
    }

    fn before_cursor(&self) -> &str {
        &self.text[..self.cursor]
    }

    /// The grapheme boundary before `at`, or 0.
    fn prev(&self, at: usize) -> usize {
        self.text[..at]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    /// The grapheme boundary after `at`, or the end.
    fn next(&self, at: usize) -> usize {
        self.text[at..]
            .graphemes(true)
            .next()
            .map_or(at, |grapheme| at + grapheme.len())
    }

    /// `at`, or the end of the grapheme it falls inside after an edit joined two graphemes.
    fn snap(&self, at: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(i, grapheme)| (i, i + grapheme.len()))
            .find(|&(start, end)| start < at && at < end)
            .map_or(at, |(_, end)| end)
    }

    /// Records the text before an edit as one undo step and drops what undo had stepped back over.
    fn snapshot(&mut self) {
        self.undo.push((self.text.clone(), self.cursor));
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.typing = false;
    }

    fn insert(&mut self, ch: char) {
        if !self.typing {
            self.snapshot();
        }
        self.redo.clear();
        self.text.insert(self.cursor, ch);
        self.cursor = self.snap(self.cursor + ch.len_utf8());
        self.typing = !ch.is_whitespace();
    }

    /// Inserts `text` at the cursor as one undo step.
    fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.snapshot();
        self.text.insert_str(self.cursor, text);
        self.cursor = self.snap(self.cursor + text.len());
    }

    /// Removes `start..end` as one undo step and leaves the cursor at `start`.
    fn cut(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        self.snapshot();
        self.text.replace_range(start..end, "");
        self.cursor = self.snap(start);
    }

    fn backspace(&mut self) {
        self.cut(self.prev(self.cursor), self.cursor);
    }

    /// Deletes the grapheme after the cursor.
    fn delete(&mut self) {
        self.cut(self.cursor, self.next(self.cursor));
    }

    fn move_to(&mut self, at: usize) {
        self.cursor = at;
        self.typing = false;
    }

    fn left(&mut self) {
        self.move_to(self.prev(self.cursor));
    }

    fn right(&mut self) {
        self.move_to(self.next(self.cursor));
    }

    /// The start of the word before the cursor, past any whitespace.
    fn word_start(&self) -> usize {
        self.text[..self.cursor]
            .split_word_bound_indices()
            .rev()
            .find(|(_, word)| !word.trim().is_empty())
            .map_or(0, |(i, _)| i)
    }

    fn word_left(&mut self) {
        self.move_to(self.word_start());
    }

    /// Moves past any whitespace, then to the end of the next word.
    fn word_right(&mut self) {
        let end = self.text[self.cursor..]
            .split_word_bound_indices()
            .find(|(_, word)| !word.trim().is_empty())
            .map_or(self.text.len(), |(i, word)| self.cursor + i + word.len());
        self.move_to(end);
    }

    fn delete_word_back(&mut self) {
        self.cut(self.word_start(), self.cursor);
    }

    /// The start of the line the cursor is on.
    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }

    /// The end of the line the cursor is on, before its newline.
    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i)
    }

    fn home(&mut self) {
        self.move_to(self.line_start());
    }

    fn end(&mut self) {
        self.move_to(self.line_end());
    }

    /// Deletes from the start of the line to the cursor.
    fn kill_to_home(&mut self) {
        self.cut(self.line_start(), self.cursor);
    }

    /// The boundary on the line `start..end` closest to display column `col` without passing it.
    fn at_column(&self, start: usize, end: usize, col: usize) -> usize {
        let mut width = 0;
        for (i, grapheme) in self.text[start..end].grapheme_indices(true) {
            width += grapheme.width();
            if width > col {
                return start + i;
            }
        }
        end
    }

    /// Moves to the same display column on the line above, or stays on the first line.
    fn up(&mut self) {
        let start = self.line_start();
        if start == 0 {
            return;
        }
        let col = line_width(&self.text[start..self.cursor]);
        let above = self.text[..start - 1].rfind('\n').map_or(0, |i| i + 1);
        self.move_to(self.at_column(above, start - 1, col));
    }

    /// Moves to the same display column on the line below, or stays on the last line.
    fn down(&mut self) {
        let end = self.line_end();
        if end == self.text.len() {
            return;
        }
        let col = line_width(&self.text[self.line_start()..self.cursor]);
        let below = end + 1;
        let below_end = self.text[below..]
            .find('\n')
            .map_or(self.text.len(), |i| below + i);
        self.move_to(self.at_column(below, below_end, col));
    }

    fn undo(&mut self) {
        if let Some((text, cursor)) = self.undo.pop() {
            let now = std::mem::replace(&mut self.text, text);
            self.redo.push((now, self.cursor));
            self.move_to(cursor);
        }
    }

    fn redo(&mut self) {
        if let Some((text, cursor)) = self.redo.pop() {
            let now = std::mem::replace(&mut self.text, text);
            self.undo.push((now, self.cursor));
            self.move_to(cursor);
        }
    }
}

/// Display columns of `text` after its last newline, measured grapheme by grapheme.
fn line_width(text: &str) -> usize {
    let line = text.rsplit('\n').next().unwrap_or("");
    line.graphemes(true).map(UnicodeWidthStr::width).sum()
}

enum Mode {
    Normal,
    /// The in-reader sign-in form. The terminal still signs in on stdout.
    SignIn(SignInForm),
    Help,
    Search(Input),
    Compose(Box<Composer>),
    /// The filter panel: the networks it would turn on, and its cursor.
    Filter {
        on: Vec<Network>,
        cursor: usize,
    },
    Ask {
        input: Input,
        asked: Vec<String>,
    },
    Inspect {
        id: String,
        handle: String,
        value: Value,
        open: HashSet<String>,
        cursor: usize,
    },
    Share(Box<Share>),
    Gallery(Box<Gallery>),
    /// The `:` command line: what is typed, and the highlighted suggestion.
    Command {
        input: Input,
        pick: usize,
    },
    Pick(Picker),
    /// Hidden controls from a `[...]` marker, anchored on that marker.
    Overflow(Overflow),
}

/// The list a `[...]` marker opens, and the screen cell the marker sits on.
struct Overflow {
    items: Vec<OverflowItem>,
    cursor: usize,
    anchor_x: usize,
    anchor_y: usize,
}

/// A centered list that picks one value with a digit, or with the cursor and ⏎.
struct Picker {
    kind: PickKind,
    cursor: usize,
    /// The scheme a theme picker restores when it closes without a choice.
    revert: Option<Scheme>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PickKind {
    Network,
    Detail,
    Theme,
}

impl PickKind {
    fn rows(self, state: &State) -> Vec<screens::PickRow> {
        match self {
            PickKind::Network => screens::network_rows(state),
            PickKind::Detail => screens::detail_rows(state),
            PickKind::Theme => screens::theme_rows(state),
        }
    }

    fn title(self) -> &'static str {
        match self {
            PickKind::Network => " Network ",
            PickKind::Detail => " Detail ",
            PickKind::Theme => " Theme ",
        }
    }

    /// The key that opens the picker, which also closes it.
    fn opener(self) -> Option<char> {
        match self {
            PickKind::Network => Some('A'),
            PickKind::Detail => Some('D'),
            PickKind::Theme => None,
        }
    }

    /// A picker with its cursor on the current row, or on the first row when none is current.
    fn open(self, state: &State) -> Mode {
        let cursor = self.rows(state).iter().position(|row| row.current);
        Mode::Pick(Picker {
            kind: self,
            cursor: cursor.unwrap_or(0),
            revert: (self == PickKind::Theme).then_some(state.scheme),
        })
    }
}

/// The full-screen image viewer: a post's pictures and the one on screen.
struct Gallery {
    post: Option<Post>,
    pictures: Vec<Picture>,
    index: usize,
}

struct Picture {
    url: String,
    /// What `o` opens: the full-size file behind a preview still.
    original: String,
    alt: Option<String>,
}

impl Gallery {
    /** The post's media when `urls` are its images, otherwise the bare URLs (an avatar). */
    fn new(post: Option<Post>, urls: Vec<String>) -> Self {
        let from_media: Vec<Picture> = post
            .iter()
            .flat_map(|post| &post.media)
            .filter_map(|media| {
                let url = media.view_url()?.to_string();
                Some(Picture {
                    original: media.url.clone().unwrap_or_else(|| url.clone()),
                    alt: media.alt.clone().filter(|alt| !alt.trim().is_empty()),
                    url,
                })
            })
            .collect();
        let pictures = if from_media.iter().map(|p| &p.url).eq(urls.iter()) {
            from_media
        } else {
            urls.into_iter()
                .map(|url| Picture {
                    original: url.clone(),
                    url,
                    alt: None,
                })
                .collect()
        };
        Self {
            post,
            pictures,
            index: 0,
        }
    }
}

/// What a key in the image viewer asks for.
#[derive(Debug, PartialEq, Eq)]
enum GalleryStep {
    Stay,
    Close,
    Open(String),
    Share,
}

/// ← → h l step through the pictures (wrapping), `o` opens the original, `s` shares the post.
fn gallery_key(gallery: &mut Gallery, key: KeyEvent) -> GalleryStep {
    let count = gallery.pictures.len();
    match key.code {
        KeyCode::Left | KeyCode::Char('h') if count > 1 => {
            gallery.index = (gallery.index + count - 1) % count;
            GalleryStep::Stay
        }
        KeyCode::Right | KeyCode::Char('l') if count > 1 => {
            gallery.index = (gallery.index + 1) % count;
            GalleryStep::Stay
        }
        KeyCode::Char('o') => gallery
            .pictures
            .get(gallery.index)
            .map_or(GalleryStep::Stay, |picture| {
                GalleryStep::Open(picture.original.clone())
            }),
        KeyCode::Char('s') if gallery.post.is_some() => GalleryStep::Share,
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('v') | KeyCode::Char('q') => {
            GalleryStep::Close
        }
        _ => GalleryStep::Stay,
    }
}

struct Composer {
    reply_to: Option<String>,
    /// The quoted post's scoped id, or the profile URL a share carries.
    quote: Option<String>,
    input: Input,
    /// The networks the post goes to, in network order; a reply always goes to its parent's network.
    targets: Vec<Network>,
    /// Send the text as typed instead of converting `<b>` and `<i>`.
    raw: bool,
    /// The saved draft this text was loaded from, as it was on disk.
    origin: Option<Draft>,
    /// What `dirty` compares against: the loaded draft, or the empty post the composer opened as.
    base: Snapshot,
    /// The draft id saves and autosaves write under.
    draft_id: Option<String>,
    /// When autosave last wrote, and what.
    autosaved: Option<(Instant, Snapshot)>,
    /// Drafts on disk, for the `ctrl-o drafts <n>` hint.
    saved: usize,
    overlay: Overlay,
}

/// The parts of a composer a draft keeps: text, targets, and the post it answers or quotes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Snapshot {
    text: String,
    targets: Vec<Network>,
    reply_to: Option<String>,
    quote: Option<String>,
}

enum Overlay {
    None,
    /// `Unsaved draft`: y saves, n discards, then `After` runs; esc goes back to writing.
    Ask(After),
    /// The ctrl-o drafts list, newest first.
    List {
        drafts: Vec<Draft>,
        cursor: usize,
    },
}

enum After {
    Leave,
    Quit,
    Load(Box<Draft>),
}

impl Composer {
    /// A composer for `reply_to`, `quote`, or a new post, restoring the newest draft saved for it.
    fn open(
        state: &mut State,
        reply_to: Option<String>,
        quote: Option<String>,
        targets: Vec<Network>,
    ) -> Box<Self> {
        let base = Snapshot {
            text: String::new(),
            targets: targets.clone(),
            reply_to: reply_to.clone(),
            quote: quote.clone(),
        };
        let mut composer = Box::new(Self {
            reply_to,
            quote,
            input: Input::new(),
            targets,
            raw: false,
            origin: None,
            base,
            draft_id: None,
            autosaved: None,
            saved: 0,
            overlay: Overlay::None,
        });
        state.status.clear();
        if let Some(draft) = drafts::find(composer.reply_to.as_deref(), composer.quote.as_deref()) {
            state.status = draft.sending_status().unwrap_or_else(|| {
                format!(
                    "draft restored · saved {}",
                    util::ago_long(draft.updated_at, crate::clock::utc_now())
                )
            });
            composer.load(draft);
        }
        composer.count();
        composer
    }

    fn count(&mut self) {
        self.saved = drafts::load().map_or(0, |list| list.len());
    }

    fn load(&mut self, draft: Draft) {
        self.input = Input::from(&draft.text);
        let targets: Vec<Network> = Network::ALL
            .into_iter()
            .filter(|net| draft.targets.iter().any(|slug| slug == net.slug()))
            .collect();
        if !targets.is_empty() {
            self.targets = targets;
        }
        self.reply_to = draft.reply_to.clone();
        self.quote = draft.quote.clone();
        self.draft_id = Some(draft.id.clone());
        self.origin = Some(draft);
        self.base = self.snapshot();
        self.autosaved = None;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.input.text().to_string(),
            targets: self.targets.clone(),
            reply_to: self.reply_to.clone(),
            quote: self.quote.clone(),
        }
    }

    /// The text, targets, reply or quote differ from `base`; blank text counts only when the rest does.
    fn dirty(&self) -> bool {
        let now = self.snapshot();
        if now.text.trim().is_empty() {
            return Snapshot {
                text: self.base.text.clone(),
                ..now
            } != self.base;
        }
        now != self.base
    }

    fn slugs(&self) -> Vec<String> {
        self.targets.iter().map(|net| net.slug().into()).collect()
    }

    /// The first target, where a single-target composer posts.
    fn target(&self) -> Network {
        self.targets.first().copied().unwrap_or(Network::X)
    }

    fn save(&mut self, state: &State) -> Result<(), String> {
        self.save_to(state, &drafts::path())
    }

    /// `save` into the drafts file `store`.
    fn save_to(&mut self, state: &State, store: &Path) -> Result<(), String> {
        let handle = self
            .reply_to
            .as_ref()
            .or(self.quote.as_ref())
            .and_then(|id| state.posts.get(id))
            .map(|post| post.author.username.clone());
        let mut draft = self
            .origin
            .clone()
            .unwrap_or_else(|| Draft::new(self.slugs(), self.reply_to.clone(), handle));
        if let Some(id) = &self.draft_id {
            draft.id = id.clone();
        }
        draft.text = self.input.text().to_string();
        draft.updated_at = crate::clock::utc_now();
        draft.targets = self.slugs();
        draft.reply_to = self.reply_to.clone();
        draft.quote = self.quote.clone();
        drafts::upsert_at(store, &draft)?;
        self.draft_id = Some(draft.id);
        Ok(())
    }

    /// What ⏎ publishes, exactly as the composer shows it.
    fn intent(&self) -> Intent {
        Intent {
            text: self.outgoing(),
            typed: self.input.text().to_string(),
            targets: self.targets.clone(),
            reply_to: self.reply_to.clone(),
            quote: self.quote.clone(),
            draft_id: self.draft_id.clone(),
        }
    }

    /// A bracketed paste: the cleaned text at the cursor as one undo step, dropped under an overlay.
    fn paste(&mut self, text: &str) -> bool {
        let open = matches!(self.overlay, Overlay::None);
        if open {
            self.input.insert_str(&clean_paste(text));
        }
        open
    }

    /// The text as it will be sent.
    fn outgoing(&self) -> String {
        let text = self.input.text();
        if self.raw {
            text.to_string()
        } else {
            rich::convert(text)
        }
    }

    /**
    The strictest target and what it has left: per target, its limit minus the converted text, and
    minus the quoted link and its blank line where the quote can't be native. X counts a link as 23.
    */
    fn room(&self, state: &State) -> (Network, i64) {
        let text = self.outgoing();
        let link = self.quote.as_deref().map(|quote| {
            state
                .posts
                .get(quote)
                .map_or_else(|| quote.to_string(), Post::permalink)
        });
        let quoted = self
            .quote
            .as_deref()
            .and_then(net::split)
            .map(|(net, _)| net);
        self.targets
            .iter()
            .map(|&net| {
                let native = quoted == Some(net) && net.caps().quote;
                let extra = match &link {
                    Some(_) if native => 0,
                    Some(_) if net == Network::X => 23 + 2,
                    Some(link) => net.text_len(link) + 2,
                    None => 0,
                };
                let max = net.caps().max_chars as i64;
                (net, max - (net.text_len(&text) + extra) as i64)
            })
            .min_by_key(|(_, left)| *left)
            .unwrap_or((self.target(), 0))
    }

    /// Writes a dirty composer when the last autosave is old enough and anything it keeps changed.
    fn autosave(&mut self, state: &State, store: &Path) -> Result<(), String> {
        let now = self.snapshot();
        let due = self
            .autosaved
            .as_ref()
            .is_none_or(|(at, last)| at.elapsed() >= AUTOSAVE && *last != now);
        if self.dirty() && due {
            self.save_to(state, store)?;
            self.autosaved = Some((Instant::now(), now));
        }
        Ok(())
    }

    /// Undoes what autosave wrote: the loaded draft goes back as it was, a new one goes away.
    fn discard(&mut self) -> Result<(), String> {
        if self.autosaved.take().is_none() {
            return Ok(());
        }
        match (&self.origin, &self.draft_id) {
            (Some(origin), _) => drafts::upsert(origin),
            (None, Some(id)) => drafts::remove(id),
            (None, None) => Ok(()),
        }
    }
}

/// The target the focus ring is on: the `nth` tab stop with this zone and act.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Focus {
    zone: Zone,
    act: Act,
    nth: usize,
}

/// What a click on a dialog row picks: a row by its index, or a share checkbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Row(usize),
    Check(usize),
    Key(KeyCode),
}

/// A body pane on the last frame: where it sits and the screen rows of its laid rows.
struct Pane {
    zone: Zone,
    cols: Range<usize>,
    rows: Range<usize>,
    offset: usize,
    spans: Vec<RowSpan>,
}

/// What the last frame put where, for the focus ring and the mouse.
#[derive(Default)]
struct Hits {
    /// Pressable runs in screen cells.
    targets: Vec<Target>,
    /// The open dialog's rows and columns; a click outside closes it.
    dialog: Option<(Range<usize>, Range<usize>)>,
    /// A dialog row's screen row and columns, and what a click there picks.
    rows: Vec<(usize, Range<usize>, Pick)>,
    panes: Vec<Pane>,
    /// Settings rows by screen row.
    settings: Vec<(usize, settings::Hit)>,
    /// The frame's [`render::margin`], which turns a screen column into a canvas column.
    margin: usize,
}

/// A key, a mouse event or a bracketed paste, for the work `handle_key_busy` runs.
#[derive(Debug, PartialEq)]
enum Press {
    Key(KeyEvent),
    Mouse(MouseEvent),
    /// A run of wheel notches at the last notch's cell, as items to move: down when positive.
    Wheel(MouseEvent, i32),
    Paste(String),
}

/// Whether the terminal reports the mouse, so `leave` turns capture off only when it is on.
static MOUSE: AtomicBool = AtomicBool::new(false);
/// Whether the terminal also reports every pointer move, which only the hover dialogs need.
static HOVER: AtomicBool = AtomicBool::new(false);
/// Whether `enter` pushed the keyboard enhancement flags, for the watchdog's restore.
pub static ENHANCED: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "native")]
fn capture(on: bool, hover: bool) {
    let _ = report_modes(&mut stdout(), [&MOUSE, &HOVER], on, hover);
}

/**
Writes to `out` the mouse report modes that change: press, release, drag and wheel in SGR
coordinates while `mouse` is on, plus any-motion while `hover` is too. crossterm's
`EnableMouseCapture` would turn any-motion on for the whole session.
*/
#[cfg(feature = "native")]
fn report_modes(
    out: &mut impl io::Write,
    [mouse_on, hover_on]: [&AtomicBool; 2],
    mouse: bool,
    hover: bool,
) -> io::Result<()> {
    let hover = mouse && hover;
    if mouse_on.swap(mouse, Ordering::Relaxed) != mouse {
        if mouse {
            out.write_all(b"\x1b[?1000h\x1b[?1002h\x1b[?1006h")?;
        } else {
            hover_on.store(false, Ordering::Relaxed);
            // Windows DisableMouseCapture calls WinAPI and needs a mode EnableMouseCapture saved.
            out.write_all(b"\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l")?;
        }
    }
    if hover_on.swap(hover, Ordering::Relaxed) != hover {
        out.write_all(if hover {
            b"\x1b[?1003h"
        } else {
            b"\x1b[?1003l"
        })?;
    }
    out.flush()
}

/// Whether the open dialog moves its cursor to the row under the pointer.
fn hovers(mode: &Mode) -> bool {
    match mode {
        Mode::Pick(_)
        | Mode::Share(_)
        | Mode::Filter { .. }
        | Mode::Command { .. }
        | Mode::Overflow(_) => true,
        Mode::Compose(composer) => matches!(composer.overlay, Overlay::List { .. }),
        _ => false,
    }
}

/**
`SOCLI_TRACE_FRAMES`: one timing line per drawn frame, appended to a file opened once. Without the
variable, or after a failed write, nothing is measured or written.
*/
struct Trace {
    file: Option<std::fs::File>,
    started: Instant,
    events: usize,
    moves_dropped: usize,
    scroll_net: i64,
    handle: Duration,
    idle: usize,
}

impl Trace {
    fn open() -> Self {
        let file = std::env::var_os("SOCLI_TRACE_FRAMES")
            .filter(|path| !path.is_empty())
            .and_then(|path| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .ok()
            });
        Self {
            file,
            started: Instant::now(),
            events: 0,
            moves_dropped: 0,
            scroll_net: 0,
            handle: Duration::ZERO,
            idle: 0,
        }
    }

    /// A clock for one measurement, or none while tracing is off.
    fn clock(&self) -> Option<Instant> {
        self.file.as_ref().map(|_| Instant::now())
    }

    fn write(&mut self, line: &str) {
        use std::io::Write;
        let ms = self.started.elapsed().as_millis();
        if let Some(file) = &mut self.file {
            if writeln!(file, "{ms} {line}").is_err() {
                self.file = None;
            }
        }
    }

    fn frame(&mut self, build: Option<Instant>, draw: Option<Instant>, done: Option<Instant>) {
        let (Some(build), Some(draw), Some(done)) = (build, draw, done) else {
            return;
        };
        let ms = |span: Duration| span.as_secs_f64() * 1000.0;
        let line = format!(
            "events={} moves_dropped={} scroll_net={} handle_ms={:.2} build_ms={:.2} draw_ms={:.2}",
            self.events,
            self.moves_dropped,
            self.scroll_net,
            ms(self.handle),
            ms(draw - build),
            ms(done - draw),
        );
        self.write(&line);
        self.events = 0;
        self.moves_dropped = 0;
        self.scroll_net = 0;
        self.handle = Duration::ZERO;
        self.idle = 0;
    }

    /// A loop iteration that drew nothing; the count runs from the last drawn frame.
    fn idle(&mut self) {
        if self.file.is_some() {
            self.idle += 1;
            let line = format!("idle_wakeups={}", self.idle);
            self.write(&line);
        }
    }
}

pub(crate) struct Ui {
    mode: Mode,
    /// First body row on screen; draw keeps the selected block inside it.
    scroll: Cell<usize>,
    /// The screen `scroll` belongs to, so a new screen starts at the top.
    scroll_key: RefCell<String>,
    /// `scroll` and `scroll_key` for the thread pane of the split layout.
    scroll_right: Cell<usize>,
    scroll_right_key: RefCell<String>,
    /// Where the last frame drew the selected block, so a change of look keeps it on its row.
    anchor: RefCell<Option<Anchor>>,
    /// The thread pane's anchor, kept the same way while the open thread has the keys.
    anchor_right: RefCell<Option<Anchor>>,
    /// The last frame drew two panes, so the page keys scroll the focused one.
    split: Cell<bool>,
    /// Rows the help overlay or ask transcript overflows by, for the scroll keys.
    overflow: Cell<usize>,
    /// The terminal reports Shift+Enter apart from Enter.
    enhanced: bool,
    disclosure_keys: [bool; 6],
    /// OSC 8 links on names, mentions, times and URLs, decided once at startup.
    links: bool,
    /// The body's width and height at the last draw, for the page keys.
    viewport: Cell<(usize, usize)>,
    /// Inline images and the viewer's; none while images are off.
    previews: RefCell<Option<Previews>>,
    /// The image mode `previews` was built for, so a Settings change rebuilds it.
    images: Cell<ImageMode>,
    /// Page readers for rich link cards, started on the first page that needs one.
    unfurls: Option<Unfurls>,
    /// Pages the last drawn post views want unfurled.
    pages: RefCell<Vec<String>>,
    /// When the reader started; spinners count their frames from it.
    started: Instant,
    welcome: RefCell<onboard::Scene>,
    hits: RefCell<Hits>,
    focus: RefCell<Option<Focus>>,
}

impl Ui {
    /// The last frame drew the feed and the thread side by side.
    pub(crate) fn is_split(&self) -> bool {
        self.split.get()
    }
}

/// Demo pictures the page overlays. The field stays private; the browser session asks through these.
#[cfg(not(feature = "native"))]
impl Ui {
    /// Installs the page's picture store, or removes it. `true` means a store was installed or removed.
    pub(crate) fn use_page_pictures(&self, on: bool) -> bool {
        if on {
            if self.previews.borrow().is_none() {
                *self.previews.borrow_mut() = Some(crate::preview::Previews::page());
            }
            return true;
        }
        if self.previews.borrow().is_some() {
            *self.previews.borrow_mut() = None;
            return true;
        }
        false
    }

    pub(crate) fn places_json(&self) -> String {
        self.previews
            .borrow()
            .as_ref()
            .map(crate::preview::Previews::places_json)
            .unwrap_or_else(|| "[]".to_string())
    }

    pub(crate) fn picture_png(&self, url: &str) -> Vec<u8> {
        self.previews
            .borrow()
            .as_ref()
            .and_then(|previews| previews.png(url).map(<[u8]>::to_vec))
            .unwrap_or_default()
    }
}

/**
Restores the terminal if setup returns after raw mode is on.
[`TerminalGuard::disarm`] before the normal [`leave`] so that path restores once.
*/
#[cfg(feature = "native")]
struct TerminalGuard {
    enhanced: bool,
    armed: bool,
    restore: fn(bool),
}

#[cfg(feature = "native")]
impl TerminalGuard {
    fn arm(enhanced: bool) -> Self {
        Self {
            enhanced,
            armed: true,
            restore: leave,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }

    #[cfg(test)]
    fn armed_with(enhanced: bool, restore: fn(bool)) -> Self {
        Self {
            enhanced,
            armed: true,
            restore,
        }
    }
}

#[cfg(feature = "native")]
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.armed {
            self.armed = false;
            (self.restore)(self.enhanced);
        }
    }
}

/// A busy lock keeps `key` and sets loading. Poison is not busy: the guard is recovered.
#[cfg(feature = "native")]
enum ClientsKey<'a, T, U> {
    Ready(T, MutexGuard<'a, U>),
    Waiting { key: T, loading: bool },
}

#[cfg(feature = "native")]
fn next_clients_key<'a, T, U>(mutex: &'a Mutex<U>, key: T) -> ClientsKey<'a, T, U> {
    match mutex.try_lock() {
        Ok(guard) => ClientsKey::Ready(key, guard),
        Err(TryLockError::Poisoned(err)) => ClientsKey::Ready(key, err.into_inner()),
        Err(TryLockError::WouldBlock) => ClientsKey::Waiting { key, loading: true },
    }
}

/// One press against the clients lock. A busy lock parks the press and asks for the loading footer.
#[cfg(feature = "native")]
enum Gate<'a> {
    Quit,
    Ran,
    Ready(Press, MutexGuard<'a, Clients>),
    Park { key: Press, loading: bool },
}

#[cfg(feature = "native")]
fn ctrl_c_quits(press: &Press, ui: &Ui) -> bool {
    let Press::Key(key) = press else {
        return false;
    };
    key.modifiers.contains(KeyModifiers::CONTROL)
        && key.code == KeyCode::Char('c')
        && !matches!(ui.mode, Mode::Compose(_))
}

#[cfg(feature = "native")]
fn gate_press<'a>(
    clients: &'a Mutex<Clients>,
    state: &mut State,
    ui: &mut Ui,
    defer: bool,
    press: Press,
) -> Result<Gate<'a>, String> {
    if defer {
        if ctrl_c_quits(&press, ui) {
            return Ok(Gate::Quit);
        }
        return Ok(Gate::Park {
            key: press,
            loading: true,
        });
    }
    if let Some(done) = navigate(state, ui, &press) {
        return Ok(if done? { Gate::Quit } else { Gate::Ran });
    }
    Ok(match next_clients_key(clients, press) {
        ClientsKey::Ready(key, guard) => Gate::Ready(key, guard),
        ClientsKey::Waiting { key, loading } => Gate::Park { key, loading },
    })
}

#[cfg(feature = "native")]
struct InputPass<'a> {
    terminal: &'a mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    last: &'a Buffer,
    state: &'a mut State,
    ui: &'a mut Ui,
    signal: &'a AtomicBool,
    waiting: &'a mut Vec<Press>,
    waiting_since: &'a mut Option<Instant>,
    defer: bool,
    handled: &'a mut Duration,
    tracing: bool,
    dirty: &'a mut bool,
    parks: usize,
    count_parks: bool,
}

/// Runs `press`, or parks it and sets loading when the clients lock is busy.
#[cfg(feature = "native")]
fn dispatch_press(
    pass: &mut InputPass<'_>,
    clients: &Mutex<Clients>,
    press: Press,
) -> Result<bool, String> {
    let deferred = pass.defer;
    let gate = gate_press(clients, pass.state, pass.ui, deferred, press)?;
    match gate {
        Gate::Quit => Ok(true),
        Gate::Ran => {
            *pass.dirty = true;
            Ok(false)
        }
        Gate::Ready(press, mut guard) => {
            let started = pass.tracing.then(Instant::now);
            let quit = handle_key_busy(
                pass.terminal,
                pass.last,
                pass.state,
                &mut guard,
                pass.ui,
                press,
                pass.signal,
            )?;
            if let Some(started) = started {
                *pass.handled += started.elapsed();
            }
            *pass.dirty = true;
            Ok(quit)
        }
        Gate::Park { key, loading } => {
            if loading {
                let _ = pass.waiting_since.get_or_insert_with(Instant::now);
            }
            if pass.count_parks {
                pass.parks += 1;
            }
            pass.waiting.push(key);
            pass.defer = true;
            Ok(false)
        }
    }
}

#[cfg(feature = "native")]
pub fn run(mut state: State, mut clients: Clients) -> Result<(), String> {
    // The shell keeps `--count`. The reader never asks a network for a shorter page than 20.
    state.count = state::page_count(state.count);
    if let Err(err) = state.ensure_loaded(&mut clients) {
        state.fail(err);
    }
    let clients = Arc::new(Mutex::new(clients));
    let mut page_job: Option<PageRx> = None;
    let mut preview_job: Option<PageRx> = None;
    let signal = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&signal))
        .map_err(|e| e.to_string())?;
    // SIGHUP is not in signal_hook's Windows consts.
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&signal))
        .map_err(|e| e.to_string())?;
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut terminal_guard = TerminalGuard::arm(false);
    watchdog::arm(watchdog::Probe::Stdout);
    if let Some(rgb) = query_background() {
        state.theme.term_bg = Some(rgb);
        state.auto_dark = theme::is_dark(rgb);
        state.retheme();
    }
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    terminal_guard.enhanced = enhanced;
    enter(enhanced, state.mouse).map_err(|e| e.to_string())?;
    let image_mode = if onboard::showing(&state) {
        ImageMode::Off
    } else {
        state.viewer.mode
    };
    let (previews, pictures) = start_previews(image_mode);
    state.preview_ratio = previews.as_ref().map(Previews::cell_ratio);
    state.graphics = previews.as_ref().is_some_and(Previews::graphics);
    state.pictures = pictures;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave(enhanced);
        hook(info);
    }));
    let mut terminal =
        ratatui::Terminal::new(CrosstermBackend::new(stdout())).map_err(|e| e.to_string())?;
    let mut ui = Ui {
        mode: if state.start_ask {
            Mode::Ask {
                input: Input::new(),
                asked: Vec::new(),
            }
        } else {
            Mode::Normal
        },
        scroll: Cell::new(0),
        scroll_key: RefCell::new(String::new()),
        scroll_right: Cell::new(0),
        scroll_right_key: RefCell::new(String::new()),
        anchor: RefCell::new(None),
        anchor_right: RefCell::new(None),
        split: Cell::new(false),
        overflow: Cell::new(0),
        enhanced,
        disclosure_keys: [false; 6],
        links: hyperlink::enabled(),
        viewport: Cell::new((0, 0)),
        previews: RefCell::new(previews),
        images: Cell::new(image_mode),
        unfurls: None,
        pages: RefCell::new(Vec::new()),
        started: Instant::now(),
        welcome: RefCell::new(onboard::Scene::default()),
        hits: RefCell::new(Hits::default()),
        focus: RefCell::new(None),
    };
    let mut trace = Trace::open();
    let mut dirty = true;
    let mut drawn: Option<Instant> = None;
    let mut last = Buffer::empty(Rect::default());
    let mut waiting: Vec<Press> = Vec::new();
    let mut waiting_since: Option<Instant> = None;
    let mut shown_loading = String::new();
    let result = 'run: loop {
        watchdog::beat();
        capture(state.mouse, hovers(&ui.mode) || onboard::showing(&state));
        if let Mode::Compose(composer) = &mut ui.mode {
            if let Err(err) = composer.autosave(&state, &drafts::path()) {
                dirty |= state.status != err;
                state.fail(err);
            }
        }
        let image_mode = if onboard::showing(&state) {
            ImageMode::Off
        } else {
            state.viewer.mode
        };
        if image_mode != ui.images.get() {
            ui.images.set(image_mode);
            let (previews, pictures) = start_previews(image_mode);
            state.preview_ratio = previews.as_ref().map(Previews::cell_ratio);
            state.graphics = previews.as_ref().is_some_and(Previews::graphics);
            state.pictures = pictures;
            ui.previews.replace(previews);
        }
        if let Some(previews) = ui.previews.get_mut() {
            dirty |= previews.poll();
            dirty |= settle_link_images(&mut state, previews);
        }
        dirty |= unfurl_pages(&mut ui, &mut state);
        dirty |= state.expire(Instant::now());
        dirty |= take_page(&mut state, &mut page_job);
        dirty |= take_preview(&mut state, &mut preview_job);
        if signal.load(Ordering::Relaxed) {
            break save_on_exit(&mut ui, &state);
        }
        let spinning = state.detail == theme::Detail::Rich
            && ui.previews.get_mut().as_ref().is_some_and(Previews::busy);
        let alive = onboard::showing(&state)
            && matches!(ui.mode, Mode::Normal)
            && onboard::effects_visible(ui.viewport.get().0, ui.viewport.get().1);
        let mut drew = false;
        if should_draw(dirty, spinning || alive, drawn.map(|at| at.elapsed())) {
            let build = trace.clock();
            let (full, rows) = terminal.size().map_or((0, 0), |size| {
                (usize::from(size.width), usize::from(size.height))
            });
            let split = panes(&mut state, &ui, full);
            if let Some(previews) = ui.previews.get_mut() {
                previews.next_frame();
            }
            let drawing = trace.clock();
            match terminal.draw(|frame| draw(frame, &state, &ui, split.as_ref())) {
                Ok(done) => {
                    last = done.buffer.clone();
                    drew = true;
                }
                Err(err) => break Err(err.to_string()),
            }
            trace.frame(build, drawing, trace.clock());
            dirty = false;
            drawn = Some(Instant::now());
            // The card is already in this frame. The fetch itself runs beside the keys.
            let due = state.page_ask || (state.show_page_card && !too_small(full, rows));
            let mut again = false;
            if page_job.is_none() && due {
                again |= start_page(&mut state, &clients, &mut page_job);
            }
            if preview_job.is_none() && ui.split.get() {
                again |= start_preview(&mut state, &clients, &mut preview_job);
            }
            if again {
                dirty = true;
                continue;
            }
        } else if waiting.is_empty() {
            trace.idle();
        }
        if drew && !waiting.is_empty() {
            paint_loading_footer(
                &mut terminal,
                &last,
                &state,
                waiting_since.unwrap_or_else(Instant::now),
                &mut shown_loading,
                true,
            );
        }
        let encoding = ui
            .previews
            .get_mut()
            .as_ref()
            .is_some_and(Previews::encoding);
        let wait = if alive {
            Duration::from_millis(33)
        } else if spinning || encoding {
            SPIN
        } else {
            IDLE_WAIT
        };
        let wait = state.status_expiry.as_ref().map_or(wait, |(deadline, _)| {
            wait.min(deadline.saturating_duration_since(Instant::now()))
        });
        let wait = if preview_job.is_some() {
            wait.min(Duration::from_millis(50))
        } else {
            wait
        };
        let small = terminal
            .size()
            .is_ok_and(|size| too_small(usize::from(size.width), usize::from(size.height)));
        let wait = if !waiting.is_empty() && !small {
            wait.min(SPIN)
        } else {
            wait
        };
        let events = match read_batch(wait) {
            Ok(events) => events,
            Err(_) if signal.load(Ordering::Relaxed) => {
                break save_on_exit(&mut ui, &state);
            }
            Err(err) => {
                break Err(match save_on_exit(&mut ui, &state) {
                    Ok(()) => err.to_string(),
                    Err(kept) => format!("{err}; {kept}"),
                });
            }
        };
        let mut handled = Duration::ZERO;
        if !events.is_empty() && small {
            if events.iter().any(quits_too_small) {
                break save_on_exit(&mut ui, &state);
            }
            dirty |= events
                .iter()
                .any(|event| matches!(event, Event::Resize(..)));
        } else if !small {
            let panes: Vec<(Range<usize>, Range<usize>, Zone)> = if events.is_empty() {
                Vec::new()
            } else {
                ui.hits
                    .borrow()
                    .panes
                    .iter()
                    .map(|pane| (pane.cols.clone(), pane.rows.clone(), pane.zone))
                    .collect()
            };
            let mut pass = InputPass {
                terminal: &mut terminal,
                last: &last,
                state: &mut state,
                ui: &mut ui,
                signal: &signal,
                waiting: &mut waiting,
                waiting_since: &mut waiting_since,
                defer: false,
                handled: &mut handled,
                tracing: trace.clock().is_some(),
                dirty: &mut dirty,
                parks: 0,
                count_parks: false,
            };
            if !pass.waiting.is_empty() {
                let queued = std::mem::take(pass.waiting);
                for press in queued {
                    match dispatch_press(&mut pass, &clients, press) {
                        Ok(false) => {}
                        Ok(true) => break 'run Ok(()),
                        Err(err) => break 'run Err(err),
                    }
                }
            }
            if !events.is_empty() {
                trace.events += events.len();
                let pane_of = |x: usize, y: usize| {
                    panes
                        .iter()
                        .find(|(cols, rows, _)| cols.contains(&x) && rows.contains(&y))
                        .map(|(_, _, zone)| *zone)
                };
                pass.count_parks = true;
                let batch = coalesce(events, pane_of, |press| {
                    dispatch_press(&mut pass, &clients, press)
                });
                match batch {
                    Ok(batch) if batch.quit => break Ok(()),
                    Ok(batch) => {
                        trace.moves_dropped += batch.moves_dropped;
                        trace.scroll_net += batch.scroll_net;
                        *pass.dirty |= batch.processed > pass.parks;
                    }
                    Err(err) => break Err(err),
                }
            }
        }
        trace.handle += handled;
        if waiting.is_empty() {
            waiting_since = None;
            shown_loading.clear();
        } else if !small && !dirty {
            paint_loading_footer(
                &mut terminal,
                &last,
                &state,
                waiting_since.unwrap_or_else(Instant::now),
                &mut shown_loading,
                false,
            );
        }
    };
    terminal_guard.disarm();
    leave(enhanced);
    result
}

// MARK: input batches

/** Waits up to `wait` for input, then takes what is already queued, up to [`BATCH`] events or [`BATCH_TIME`]. */
#[cfg(feature = "native")]
fn read_batch(wait: Duration) -> io::Result<Vec<Event>> {
    let mut events = Vec::new();
    if !event::poll(wait)? {
        return Ok(events);
    }
    let started = Instant::now();
    events.push(event::read()?);
    while events.len() < BATCH && started.elapsed() < BATCH_TIME && event::poll(Duration::ZERO)? {
        events.push(event::read()?);
    }
    Ok(events)
}

/// What one batch of input did, for the redraw and the frame trace.
#[derive(Debug, Default, PartialEq, Eq)]
#[cfg(feature = "native")]
struct Batch {
    /// A press asked to quit, and the presses queued after it did not run.
    quit: bool,
    /// Presses run plus resize runs, so zero means the batch held only ignored events.
    processed: usize,
    moves_dropped: usize,
    /// Wheel notches down minus notches up, before the per-step cap.
    scroll_net: i64,
    resizes: usize,
}

/// A queued event as the batch sees it.
#[cfg(feature = "native")]
enum Queued {
    Press(Press),
    Moved(MouseEvent),
    Wheel(MouseEvent, i64),
    Resize,
}

#[cfg(feature = "native")]
fn queued(event: Event) -> Option<Queued> {
    match event {
        Event::Key(key)
            if key.kind != KeyEventKind::Release || matches!(key.code, KeyCode::Modifier(_)) =>
        {
            Some(Queued::Press(Press::Key(key)))
        }
        Event::FocusLost => Some(Queued::Press(Press::Key(KeyEvent::new_with_kind(
            KeyCode::Null,
            KeyModifiers::empty(),
            KeyEventKind::Release,
        )))),
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => Some(Queued::Press(Press::Mouse(mouse))),
            MouseEventKind::Moved => Some(Queued::Moved(mouse)),
            MouseEventKind::ScrollDown => Some(Queued::Wheel(mouse, 1)),
            MouseEventKind::ScrollUp => Some(Queued::Wheel(mouse, -1)),
            _ => None,
        },
        Event::Paste(text) => Some(Queued::Press(Press::Paste(text))),
        Event::Resize(..) => Some(Queued::Resize),
        _ => None,
    }
}

/**
Runs a batch of queued input through `run` in arrival order and stops at the press that quits. A
run of pointer moves keeps its last, a run of same-direction wheel notches over one pane (`pane_of` a
cell) becomes one step of its notch count, and a run of resizes counts once. Events the reader
ignores, such as releases and drags, neither run nor break a run.
*/
#[cfg(feature = "native")]
fn coalesce<Z: PartialEq>(
    events: Vec<Event>,
    pane_of: impl Fn(usize, usize) -> Option<Z>,
    mut run: impl FnMut(Press) -> Result<bool, String>,
) -> Result<Batch, String> {
    enum Pending<Z> {
        Press(Press),
        Moved(MouseEvent, usize),
        Wheel(MouseEvent, Option<Z>, i64),
        Resize,
    }
    let mut batch = Batch::default();
    let mut flush = |pending: Option<Pending<Z>>, batch: &mut Batch| -> Result<bool, String> {
        let press = match pending {
            None => return Ok(false),
            Some(Pending::Press(press)) => press,
            Some(Pending::Resize) => {
                batch.resizes += 1;
                batch.processed += 1;
                return Ok(false);
            }
            Some(Pending::Moved(mouse, count)) => {
                batch.moves_dropped += count - 1;
                Press::Mouse(mouse)
            }
            Some(Pending::Wheel(mouse, _, net)) => {
                batch.scroll_net += net;
                Press::Wheel(mouse, net as i32)
            }
        };
        batch.processed += 1;
        run(press)
    };
    let mut pending: Option<Pending<Z>> = None;
    for event in events {
        let Some(item) = queued(event) else {
            continue;
        };
        let next = match (item, pending.take()) {
            (Queued::Moved(mouse), Some(Pending::Moved(_, count))) => {
                Pending::Moved(mouse, count + 1)
            }
            (Queued::Wheel(mouse, notch), Some(Pending::Wheel(_, pane, net)))
                if net.signum() == notch.signum()
                    && pane == pane_of(usize::from(mouse.column), usize::from(mouse.row)) =>
            {
                Pending::Wheel(mouse, pane, net + notch)
            }
            (Queued::Resize, Some(Pending::Resize)) => Pending::Resize,
            (item, before) => {
                if flush(before, &mut batch)? {
                    batch.quit = true;
                    return Ok(batch);
                }
                match item {
                    Queued::Press(press) => {
                        if flush(Some(Pending::Press(press)), &mut batch)? {
                            batch.quit = true;
                            return Ok(batch);
                        }
                        continue;
                    }
                    Queued::Moved(mouse) => Pending::Moved(mouse, 1),
                    Queued::Wheel(mouse, notch) => {
                        let pane = pane_of(usize::from(mouse.column), usize::from(mouse.row));
                        Pending::Wheel(mouse, pane, notch)
                    }
                    Queued::Resize => Pending::Resize,
                }
            }
        };
        pending = Some(next);
    }
    batch.quit = flush(pending, &mut batch)?;
    Ok(batch)
}

/// Whether the loop draws: something changed, an animation is moving, or [`REFRESH`] passed.
#[cfg(feature = "native")]
fn should_draw(dirty: bool, moving: bool, since_frame: Option<Duration>) -> bool {
    dirty || moving || since_frame.is_none_or(|since| since >= REFRESH)
}

/// The background page fetch. The input loop polls it and never joins it.
#[cfg(feature = "native")]
type PageRx = mpsc::Receiver<(state::PagePlan, state::Fetched)>;

/**
Moves the selection, the page, and search without the client lock, so a page fetch cannot freeze
them. Anything that talks to a network returns `None` and the caller takes the lock.
*/
#[cfg(feature = "native")]
fn navigate(state: &mut State, ui: &mut Ui, press: &Press) -> Option<Result<bool, String>> {
    match press {
        Press::Key(key) => navigate_key(state, ui, *key),
        Press::Mouse(mouse) if mouse.kind == MouseEventKind::Moved => {
            hover(state, ui, *mouse);
            Some(Ok(false))
        }
        Press::Wheel(mouse, steps) => navigate_wheel(state, ui, *mouse, *steps),
        _ => None,
    }
}

#[cfg(feature = "native")]
fn navigate_key(state: &mut State, ui: &mut Ui, key: KeyEvent) -> Option<Result<bool, String>> {
    if disclosure(state, &mut ui.disclosure_keys, key) {
        return Some(Ok(false));
    }
    if refetch_chord(key) {
        return None;
    }
    if terminal_shortcut(key) {
        return Some(Ok(false));
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && key.code == KeyCode::Char('c') && !matches!(ui.mode, Mode::Compose(_)) {
        return Some(Ok(true));
    }
    if !matches!(ui.mode, Mode::Normal) || state.settings || onboard::showing(state) {
        return None;
    }
    let key = normalized_key(key);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if state.current_posts().is_empty() {
        if let Some(delta) = scroll_delta(key, ui.viewport.get().1) {
            begin_nav(state, ui);
            let next = ui.scroll.get() as i64 + i64::from(delta);
            ui.scroll.set(next.max(0) as usize);
            return Some(Ok(false));
        }
    } else if let Some(rows) = page_rows(key, ui.viewport.get().1) {
        begin_nav(state, ui);
        if !slide_focus(state, ui, rows) {
            page_key(state, ui, key);
        }
        return Some(Ok(false));
    }
    if is_search_key(key) {
        begin_nav(state, ui);
        search_key(state, ui, key);
        return Some(Ok(false));
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down if !ctrl && !state.current_posts().is_empty() => {
            begin_nav(state, ui);
            if !slide_focus(state, ui, 1) {
                state.step_selection(1);
            }
            Some(Ok(false))
        }
        KeyCode::Char('k') | KeyCode::Up if !ctrl && !state.current_posts().is_empty() => {
            begin_nav(state, ui);
            if !slide_focus(state, ui, -1) {
                state.step_selection(-1);
            }
            Some(Ok(false))
        }
        KeyCode::Char('g') | KeyCode::Home if !ctrl => {
            begin_nav(state, ui);
            state.selection_edge(false);
            Some(Ok(false))
        }
        KeyCode::Char('G') | KeyCode::End if !ctrl => {
            begin_nav(state, ui);
            state.selection_edge(true);
            Some(Ok(false))
        }
        KeyCode::Char('m') if !ctrl => {
            begin_nav(state, ui);
            ask_page(state);
            Some(Ok(false))
        }
        _ => None,
    }
}

#[cfg(feature = "native")]
fn navigate_wheel(
    state: &mut State,
    ui: &mut Ui,
    mouse: MouseEvent,
    steps: i32,
) -> Option<Result<bool, String>> {
    if !matches!(ui.mode, Mode::Normal) || state.settings || onboard::showing(state) {
        return None;
    }
    let (x, y) = (usize::from(mouse.column), usize::from(mouse.row));
    if state.current_posts().is_empty() {
        let next = (ui.scroll.get() as i64 + 3 * i64::from(steps)).max(0) as usize;
        ui.scroll.set(next);
        return Some(Ok(false));
    }
    let zone = {
        let hits = ui.hits.borrow();
        hits.panes
            .iter()
            .find(|pane| pane.cols.contains(&x) && pane.rows.contains(&y))
            .map(|pane| pane.zone)
    };
    // The other pane can open or close the thread, and that fetch takes the lock.
    if zone != Some(keyed_zone(state, ui)) {
        return None;
    }
    begin_nav(state, ui);
    let delta = if steps > 0 { 1 } else { -1 };
    for _ in 0..steps.unsigned_abs() {
        if !slide_focus(state, ui, delta) {
            state.step_selection(delta);
        }
    }
    Some(Ok(false))
}

/// Clears the demo banner and a stuck error, and drops the focus ring, as the key path does.
#[cfg(feature = "native")]
fn begin_nav(state: &mut State, ui: &mut Ui) {
    if state.status == state::DEMO_NOTICE {
        state.status.clear();
    }
    state.clear_error();
    if matches!(ui.mode, Mode::Normal) && !state.settings {
        ui.focus.replace(None);
    }
}

fn is_search_key(key: KeyEvent) -> bool {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return false;
    }
    matches!(key.code, KeyCode::Char('f' | '/' | 'F'))
}

/// `m` asks for one more page. The reader fetches it off the input thread.
fn ask_page(state: &mut State) {
    if state.demo {
        state.status = "demo · already local".into();
        return;
    }
    let Some(id) = state.current_feed_id() else {
        state.fail("no further page");
        return;
    };
    let blocked = matches!(id, FeedId::Thread(_))
        || state.feeds.get(&id).is_none_or(|feed| feed.next.is_empty());
    if blocked {
        state.fail("no further page");
        return;
    }
    state.page_ask = true;
}

/**
Starts the next page on its own thread. The clients lock is held only while the requests run.
Returns whether the screen changed and should be drawn before the next key.
*/
#[cfg(feature = "native")]
fn start_page(state: &mut State, clients: &Arc<Mutex<Clients>>, job: &mut Option<PageRx>) -> bool {
    if job.is_some() {
        return false;
    }
    let plan = match state.plan_more() {
        Ok(Some(plan)) => plan,
        Ok(None) => {
            if state.page_ask {
                state.fail("no further page");
            }
            state.page_ask = false;
            if let Some(id) = state.current_feed_id() {
                state.page_gate = Some((id, state.selected_post_id()));
            }
            return true;
        }
        Err(err) => {
            state.fail(err);
            state.page_ask = false;
            if let Some(id) = state.current_feed_id() {
                state.page_gate = Some((id, state.selected_post_id()));
            }
            return true;
        }
    };
    let (tx, rx) = mpsc::channel();
    let clients = Arc::clone(clients);
    state.paging = true;
    state.page_ask = false;
    if let Some(id) = state.current_feed_id() {
        state.page_gate = Some((id, state.selected_post_id()));
    }
    std::thread::spawn(move || {
        let fetched = {
            let mut guard = clients.lock().unwrap_or_else(|err| err.into_inner());
            state::fetch_page(&mut guard, &plan)
        };
        let _ = tx.send((plan, fetched));
    });
    *job = Some(rx);
    true
}

/**
Starts the selected post's thread beside the page fetch. The loading card stays the page's.
Returns whether the screen changed. Starting the job does not, so `j` and `k` are not redrawn twice.
*/
#[cfg(feature = "native")]
fn start_preview(
    state: &mut State,
    clients: &Arc<Mutex<Clients>>,
    job: &mut Option<PageRx>,
) -> bool {
    if job.is_some() {
        return false;
    }
    let plan = match state.plan_preview() {
        Ok(Some(plan)) => plan,
        Ok(None) => return false,
        Err(err) => {
            state.fail(err);
            return true;
        }
    };
    let (tx, rx) = mpsc::channel();
    let clients = Arc::clone(clients);
    std::thread::spawn(move || {
        let fetched = {
            let mut guard = clients.lock().unwrap_or_else(|err| err.into_inner());
            state::fetch_page(&mut guard, &plan)
        };
        let _ = tx.send((plan, fetched));
    });
    *job = Some(rx);
    false
}

/// Applies a finished preview thread. Leaves `paging` alone, because the page fetch owns that flag.
#[cfg(feature = "native")]
fn take_preview(state: &mut State, job: &mut Option<PageRx>) -> bool {
    let message = match job.as_mut().map(|rx| rx.try_recv()) {
        None | Some(Err(TryRecvError::Empty)) => return false,
        Some(Err(TryRecvError::Disconnected)) => Err("the thread stopped".to_string()),
        Some(Ok((plan, fetched))) => state.store_page(plan, fetched),
    };
    *job = None;
    if let Err(err) = message {
        state.fail(err);
    }
    true
}

/// Applies a finished page, or reports that its thread stopped. `false` while it is still running.
#[cfg(feature = "native")]
fn take_page(state: &mut State, job: &mut Option<PageRx>) -> bool {
    let message = match job.as_mut().map(|rx| rx.try_recv()) {
        None | Some(Err(TryRecvError::Empty)) => return false,
        Some(Err(TryRecvError::Disconnected)) => Err("the next page stopped".to_string()),
        Some(Ok((plan, fetched))) => state.store_page(plan, fetched),
    };
    *job = None;
    state.paging = false;
    if let Err(err) = message {
        state.fail(err);
    }
    true
}

/**
Runs a key's work on a scoped thread, so a slow network load leaves the screen alive. Past
[`BUSY_AFTER`] the last frame's footer turns into a loading line, and keys pressed meanwhile are
dropped. Ctrl-c quits at once, except during a send, which takes a second ctrl-c because quitting
could lose the text; SIGTERM and SIGHUP quit at once.
*/
#[cfg(feature = "native")]
fn handle_key_busy(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    last: &Buffer,
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    press: Press,
    signal: &AtomicBool,
) -> Result<bool, String> {
    let sending = matches!(
        (&ui.mode, &press),
        (Mode::Compose(_), Press::Key(key)) if submits(*key, state.enter, ui.enhanced)
    );
    let word = if sending { "sending" } else { "loading" };
    let look = state.theme.clone();
    let detail = state.detail;
    let rich = state.detail == theme::Detail::Rich;
    let enhanced = ui.enhanced;
    let started = Instant::now();
    let handoffs = TERMINAL_HANDOFFS.load(Ordering::SeqCst);
    let (done, finished) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let result = match press {
                Press::Key(key) => handle_key(state, clients, ui, key),
                Press::Mouse(mouse) => handle_mouse(state, clients, ui, mouse),
                Press::Wheel(mouse, steps) => {
                    let (x, y) = (usize::from(mouse.column), usize::from(mouse.row));
                    wheel(state, clients, ui, steps, x, y)
                }
                Press::Paste(text) => {
                    if paste(ui, &text) {
                        state.clear_error();
                    }
                    Ok(false)
                }
            };
            let _ = done.send(result);
        });
        let mut held = false;
        let mut shown = String::new();
        loop {
            watchdog::beat();
            match finished.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => {
                    if TERMINAL_HANDOFFS.load(Ordering::SeqCst) != handoffs {
                        terminal.clear().map_err(|err| err.to_string())?;
                    }
                    return result;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("the reader stopped while loading".into());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            if signal.load(Ordering::Relaxed) {
                quit_now(enhanced);
            }
            if TERMINAL_SUSPENDED.load(Ordering::SeqCst) || started.elapsed() < BUSY_AFTER {
                continue;
            }
            while event::poll(Duration::ZERO).unwrap_or(false) {
                let Ok(Event::Key(pressed)) = event::read() else {
                    continue;
                };
                let ctrl_c = pressed.kind != KeyEventKind::Release
                    && pressed.code == KeyCode::Char('c')
                    && pressed.modifiers.contains(KeyModifiers::CONTROL);
                if ctrl_c && (!sending || held) {
                    quit_now(enhanced);
                }
                held |= ctrl_c;
            }
            let line = busy_line(word, rich, look.ascii, started.elapsed(), held);
            if line != shown {
                let _ = terminal.draw(|frame| paint_busy(frame, last, &look, detail, &line));
                shown = line;
            }
        }
    })
}

/// Leaves the terminal as it was and exits with the status of an interrupted program.
#[cfg(feature = "native")]
fn quit_now(enhanced: bool) -> ! {
    leave(enhanced);
    std::process::exit(130)
}

/// The footer's loading line: `◐ loading` turning in rich, a still `loading…` elsewhere.
#[cfg(feature = "native")]
fn busy_line(word: &str, rich: bool, ascii: bool, elapsed: Duration, held: bool) -> String {
    let mut line = if rich {
        format!("{} {word}", spin_frame(elapsed))
    } else if ascii {
        format!("{word}...")
    } else {
        format!("{word}…")
    };
    if held {
        line.push_str(" · ctrl-c again to quit");
    }
    line
}

/// The spinner frame `elapsed` into its loop.
fn spin_frame(elapsed: Duration) -> char {
    let step = elapsed.as_millis() / SPIN.as_millis();
    SPINNER[(step % SPINNER.len() as u128) as usize]
}

/**
The last frame again, its bottom row replaced by `text` at the left, where a status would go. A
resize since then leaves the rest of the screen blank until the work is done.
*/
#[cfg(feature = "native")]
fn paint_busy(frame: &mut Frame, last: &Buffer, theme: &Theme, detail: Detail, text: &str) {
    let area = frame.area();
    if area.height == 0 {
        return;
    }
    let buffer = frame.buffer_mut();
    if last.area == area {
        buffer.clone_from(last);
    }
    let y = area.bottom() - 1;
    let base = render::row_style(theme, false);
    for x in area.left()..area.right() {
        buffer[(x, y)].reset();
        buffer[(x, y)].set_style(base);
    }
    let style = base.patch(render::ratatui_style(Kind::Dim, theme, false));
    let margin = render::margin_for(usize::from(area.width), detail) as u16;
    buffer.set_string(area.left() + margin, y, text, style);
}

/// The loading footer on the last frame. `force` repaints after a full draw replaced that row.
#[cfg(feature = "native")]
fn paint_loading_footer(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    last: &Buffer,
    state: &State,
    since: Instant,
    shown: &mut String,
    force: bool,
) {
    let line = busy_line(
        "loading",
        state.detail == Detail::Rich,
        state.theme.ascii,
        since.elapsed(),
        false,
    );
    if !force && line == *shown {
        return;
    }
    let _ = terminal.draw(|frame| paint_busy(frame, last, &state.theme, state.detail, &line));
    *shown = line;
}

/// How the full-screen composer ended.
pub enum Composed {
    /// The submit key: what to publish, which the caller sends through `send::send_intent`.
    Sent(Intent),
    Saved,
    Discarded,
}

/**
The composer alone on the alternate screen, for `socli post`, `reply` and `quote` without text.
The submit key returns the intent without posting it; esc and ctrl-c on dirty text ask whether to
keep a draft. The draft this text autosaved to stays, and the caller's send rewrites it.
*/
#[cfg(feature = "native")]
pub fn compose(
    state: &mut State,
    targets: Vec<Network>,
    reply_to: Option<String>,
    quote: Option<String>,
    raw: bool,
) -> Result<Composed, String> {
    let signal = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&signal))
        .map_err(|e| e.to_string())?;
    // SIGHUP is not in signal_hook's Windows consts.
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&signal))
        .map_err(|e| e.to_string())?;
    let mut composer = Composer::open(state, reply_to, quote, targets);
    composer.raw = raw;
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut terminal_guard = TerminalGuard::arm(false);
    watchdog::arm(watchdog::Probe::Stdout);
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    terminal_guard.enhanced = enhanced;
    enter(enhanced, false).map_err(|e| e.to_string())?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave(enhanced);
        hook(info);
    }));
    let result = compose_loop(state, &mut composer, enhanced, &signal);
    terminal_guard.disarm();
    leave(enhanced);
    result
}

#[cfg(feature = "native")]
fn compose_loop(
    state: &mut State,
    composer: &mut Composer,
    enhanced: bool,
    signal: &AtomicBool,
) -> Result<Composed, String> {
    let mut terminal =
        ratatui::Terminal::new(CrosstermBackend::new(stdout())).map_err(|e| e.to_string())?;
    let tab_targets = state.post_targets();
    let mut disclosure_keys = [false; 6];
    let interrupted = |composer: &mut Composer, state: &State| -> Result<Composed, String> {
        if !composer.dirty() {
            return Ok(Composed::Discarded);
        }
        composer
            .save(state)
            .map(|()| Composed::Saved)
            .map_err(|err| {
                format!(
                    "couldn't keep a draft of this text: {err}\n{}",
                    composer.input.text()
                )
            })
    };
    loop {
        watchdog::beat();
        if let Err(err) = composer.autosave(state, &drafts::path()) {
            state.fail(err);
        }
        if signal.load(Ordering::Relaxed) {
            return interrupted(composer, state);
        }
        terminal
            .draw(|frame| draw_compose(frame, state, composer, enhanced))
            .map_err(|e| e.to_string())?;
        let ready = event::poll(Duration::from_millis(200));
        let key = match ready.and_then(|ready| ready.then(event::read).transpose()) {
            Ok(Some(Event::Key(key))) => key,
            Ok(Some(Event::FocusLost)) => {
                disclosure_keys = [false; 6];
                state.reveal_shortcuts = false;
                continue;
            }
            Ok(Some(Event::Paste(text))) => {
                composer.paste(&text);
                continue;
            }
            Ok(_) => continue,
            Err(err) => {
                return Err(match interrupted(composer, state) {
                    Ok(_) => err.to_string(),
                    Err(kept) => format!("{err}; {kept}"),
                });
            }
        };
        if disclosure(state, &mut disclosure_keys, key) {
            continue;
        }
        match composer_key(state, composer, key, enhanced, &tab_targets) {
            Step::Stay => {}
            Step::Closed { saved: true, .. } => return Ok(Composed::Saved),
            Step::Closed { saved: false, .. } => return Ok(Composed::Discarded),
            Step::Submit => return Ok(Composed::Sent(composer.intent())),
        }
    }
}

/// The header, the composer box filling the screen, and its hint row.
#[cfg(feature = "native")]
fn draw_compose(frame: &mut Frame, state: &State, c: &Composer, enhanced: bool) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(render::row_style(&state.theme, false)),
        area,
    );
    let full = area.width as usize;
    let width = render::inner_width_for(full, state.detail);
    let height = area.height as usize;
    let mut screen = Screen {
        frame,
        theme: &state.theme,
        full,
        margin: render::margin_for(full, state.detail),
        height,
        simple: matches!(state.detail, Detail::Plain | Detail::Minimal),
    };
    let crumb = match (&c.reply_to, &c.quote) {
        (Some(_), _) => "reply",
        (None, Some(_)) => "quote",
        (None, None) => "post",
    };
    screen.line(1, &chrome::header(state, crumb, width));
    let top = 3;
    let block = composer(state, c, width, enhanced, Some(height.saturating_sub(top)));
    for (i, line) in block.lines.iter().enumerate() {
        screen.line(top + i, line);
    }
    if state.reveal_shortcuts {
        for target in place(block.targets, top, Zone::Footer, screen.margin) {
            underline(screen.frame.buffer_mut(), &target);
        }
    }
    if let Some((x, y)) = block.cursor {
        screen.cursor(screen.margin + x, top + y);
    }
}

/** The picker and the background loader for `mode`; none when images are off. */
/// The previews for `mode` and the name of what draws them, or neither while images are off.
#[cfg(feature = "native")]
fn start_previews(mode: ImageMode) -> (Option<Previews>, Option<&'static str>) {
    let want = match mode {
        ImageMode::Off => return (None, None),
        ImageMode::Auto => preview::Want::Auto,
        ImageMode::Kitty => preview::Want::Kitty,
        ImageMode::Iterm => preview::Want::Iterm,
        ImageMode::Sixel => preview::Want::Sixel,
        ImageMode::Blocks => preview::Want::Blocks,
    };
    let fetch: preview::Fetch = Arc::new(|url: &str| media::load(url, Origin::Post));
    let picker = preview::picker(want);
    let pictures = match picker.protocol_type() {
        ProtocolType::Kitty => "kitty",
        ProtocolType::Iterm2 => "iTerm2",
        ProtocolType::Sixel => "sixel",
        ProtocolType::Halfblocks => "blocks in this terminal",
    };
    (Some(Previews::new(picker, fetch)), Some(pictures))
}

/**
A link image that failed to load leaves its card, so the plain card comes back; a loaded one the
network sent no size for takes its decoded size, so the card gets the right shape. True when a card
changed.
*/
#[cfg(feature = "native")]
fn settle_link_images(state: &mut State, previews: &Previews) -> bool {
    let mut failed = Vec::new();
    let mut sized = Vec::new();
    for link in state.posts.values().flat_map(|post| &post.links) {
        let Some(image) = link.image.as_deref() else {
            continue;
        };
        match previews.shown(image) {
            Shown::Failed => failed.push(image.to_string()),
            Shown::Ready if link.image_width.is_none() || link.image_height.is_none() => {
                if let Some(size) = previews.size(image) {
                    sized.push((image.to_string(), size));
                }
            }
            _ => {}
        }
    }
    let changed = !failed.is_empty() || !sized.is_empty();
    for image in failed {
        state.drop_link_image(&image);
    }
    for (image, (width, height)) in sized {
        state.size_link_image(&image, width, height);
    }
    changed
}

// MARK: command line

/// Most suggestions the `:` line lists at once.
const COMMAND_ROWS: usize = 8;

/// Column of a suggestion's description, past the longest name and its arguments.
const COMMAND_TEXT: usize = 23;

/// Commands whose name starts with the first word typed, at most `COMMAND_ROWS`.
fn command_matches(typed: &str) -> Vec<&'static (&'static str, &'static str, &'static str)> {
    let word = typed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    command::COMMANDS
        .iter()
        .filter(|(name, _, _)| name.starts_with(&word))
        .take(COMMAND_ROWS)
        .collect()
}

/// `typed` with its first word replaced by `name`; a trailing space when the command takes
/// arguments and none follow yet.
fn completed(typed: &str, name: &str, args: &str) -> String {
    let rest = typed
        .trim_start()
        .split_once(char::is_whitespace)
        .map_or("", |(_, rest)| rest.trim_start());
    match (rest.is_empty(), args.is_empty()) {
        (true, true) => name.to_string(),
        (true, false) => format!("{name} "),
        (false, _) => format!("{name} {rest}"),
    }
}

/**
The line ⏎ runs: what was typed when its first word is a command name or it parses as typed,
otherwise the typed line completed with the picked suggestion.
*/
fn command_line(typed: &str, picked: Option<&(&str, &str, &str)>) -> String {
    let first = typed.split_whitespace().next().unwrap_or("");
    let named = command::COMMANDS.iter().any(|(name, _, _)| *name == first);
    match picked {
        Some((name, args, _)) if !named && command::parse(typed).is_err() => {
            completed(typed, name, args)
        }
        _ => typed.to_string(),
    }
}

/**
The suggestion box: one row per match. The picked row carries the selection bar as well as the
highlight, since the plain level draws no background.
*/
fn command_list(matches: &[&(&str, &str, &str)], pick: usize, width: usize) -> Vec<StyledLine> {
    let mut canvas = Canvas::new(width);
    let top = canvas.push(false);
    canvas.put(top, 0, Kind::Line, &format!("╭{}╮", "─".repeat(width - 2)));
    for (i, (name, args, what)) in matches.iter().enumerate() {
        let y = canvas.push(false);
        canvas.put(y, 0, Kind::Line, "│");
        if i == pick {
            canvas.highlight(y, 1, width - 1);
            canvas.put(y, 1, Kind::Blue, "▍");
        }
        let x = canvas.put(y, 3, Kind::Fg, name);
        if !args.is_empty() {
            canvas.put(y, x + 1, Kind::Dim, args);
        }
        let room = width.saturating_sub(COMMAND_TEXT + 2);
        canvas.put(y, COMMAND_TEXT, Kind::Dim, &util::truncate(what, room));
        canvas.put(y, width - 1, Kind::Line, "│");
    }
    let bottom = canvas.push(false);
    canvas.put(
        bottom,
        0,
        Kind::Line,
        &format!("╰{}╯", "─".repeat(width - 2)),
    );
    canvas.finish()
}

fn dialog_footer(
    width: usize,
    reveal: bool,
    actions: &[(&str, &str, KeyCode)],
    suffix: &str,
) -> (StyledLine, Vec<(Range<usize>, KeyCode)>) {
    let mut canvas = Canvas::new(width);
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Line, "│");
    let mut x = 3;
    let mut hits = Vec::new();
    for (key, label, code) in actions {
        let text = if reveal {
            format!("{key} {label}")
        } else {
            label.to_string()
        };
        if x + text.width() + 2 > width {
            break;
        }
        let end = canvas.put(y, x, Kind::Dim, &text);
        hits.push((x..end, *code));
        x = end + 2;
    }
    if x + suffix.width() + 2 <= width {
        canvas.put(y, x, Kind::Dim, suffix);
    }
    canvas.put(y, width.saturating_sub(1), Kind::Line, "│");
    (canvas.finish_line(), hits)
}

fn command_dialog(
    input: &Input,
    pick: usize,
    width: usize,
    height: usize,
) -> (Vec<StyledLine>, usize, usize, usize) {
    let matches = command_matches(input.text());
    let count = matches.len().min(height.saturating_sub(7)).max(1);
    let first = pick.saturating_sub(count.saturating_sub(1));
    let mut canvas = Canvas::new(width);
    let right = width.saturating_sub(1);
    let y = canvas.push(false);
    canvas.put(
        y,
        0,
        Kind::Line,
        &format!("╭{}╮", "─".repeat(width.saturating_sub(2))),
    );
    canvas.put(y, 2, Kind::Bold, " Commands ");
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Line, "│");
    canvas.put(y, 2, Kind::Blue, ":");
    let room = width.saturating_sub(6);
    let before = input.before_cursor();
    let mut start = 0;
    while line_width(&before[start..]) >= room && start < before.len() {
        start += before[start..].graphemes(true).next().map_or(1, str::len);
    }
    canvas.put(
        y,
        3,
        Kind::Fg,
        &util::truncate(&input.text()[start..], room),
    );
    canvas.put(y, right, Kind::Line, "│");
    let cursor = 3 + line_width(&before[start..]);
    let y = canvas.push(false);
    canvas.put(
        y,
        0,
        Kind::Line,
        &format!("├{}┤", "─".repeat(width.saturating_sub(2))),
    );
    for (offset, (name, args, what)) in matches.iter().skip(first).take(count).enumerate() {
        let y = canvas.push(false);
        canvas.put(y, 0, Kind::Line, "│");
        if first + offset == pick {
            canvas.highlight(y, 1, right);
            canvas.put(y, 1, Kind::Blue, "▍");
        }
        let name_room = width.saturating_sub(6).min(COMMAND_TEXT - 4);
        canvas.put(
            y,
            3,
            Kind::Fg,
            &util::truncate(format!("{name} {args}").trim_end(), name_room),
        );
        if width > COMMAND_TEXT + 10 {
            canvas.put(
                y,
                COMMAND_TEXT,
                Kind::Dim,
                &util::truncate(what, width.saturating_sub(COMMAND_TEXT + 2)),
            );
        }
        canvas.put(y, right, Kind::Line, "│");
    }
    if matches.is_empty() {
        let y = canvas.push(false);
        canvas.put(y, 0, Kind::Line, "│");
        canvas.put(
            y,
            3,
            Kind::Dim,
            &util::truncate("No matching commands", width.saturating_sub(5)),
        );
        canvas.put(y, right, Kind::Line, "│");
    }
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Line, "│");
    canvas.put(y, right, Kind::Line, "│");
    let y = canvas.push(false);
    canvas.put(
        y,
        0,
        Kind::Line,
        &format!("╰{}╯", "─".repeat(width.saturating_sub(2))),
    );
    (canvas.finish(), cursor, first, matches.len().min(count))
}

/** Asks for the pages the last frame wanted, when Settings allows it, and fills in what came back; true when anything did. */
fn unfurl_pages(ui: &mut Ui, state: &mut State) -> bool {
    let pages = ui.pages.take();
    if state.unfurl && !pages.is_empty() {
        let unfurls = ui.unfurls.get_or_insert_with(Unfurls::new);
        for page in &pages {
            unfurls.request(page);
        }
    }
    let mut changed = false;
    if let Some(unfurls) = ui.unfurls.as_mut() {
        for (page, card) in unfurls.poll() {
            state.fill_link(&page, &card);
            changed = true;
        }
    }
    changed
}

/// SIGTERM, SIGHUP or a lost terminal: a dirty composer is saved before the reader exits.
#[cfg(feature = "native")]
fn save_on_exit(ui: &mut Ui, state: &State) -> Result<(), String> {
    match &mut ui.mode {
        Mode::Compose(composer) if composer.dirty() => composer.save(state).map_err(|err| {
            format!(
                "couldn't keep a draft of this text: {err}\n{}",
                composer.input.text()
            )
        }),
        _ => Ok(()),
    }
}

/**
Asks the terminal for its background with OSC 11, in raw mode before the alternate screen. A DA1
request follows so the wait ends as soon as the terminal has answered both, or after 100 ms.
*/
#[cfg(feature = "native")]
fn query_background() -> Option<Rgb> {
    if !io::stdout().is_terminal() || std::env::var("TERM").is_ok_and(|term| term == "dumb") {
        return None;
    }
    read_background()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[cfg(feature = "native")]
fn read_background() -> Option<Rgb> {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    #[cfg(target_os = "macos")]
    const O_NONBLOCK: i32 = 0x0004;
    #[cfg(target_os = "linux")]
    const O_NONBLOCK: i32 = 0o4000;
    let mut tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(O_NONBLOCK)
        .open("/dev/tty")
        .ok()?;
    tty.write_all(b"\x1b]11;?\x07\x1b[c").ok()?;
    tty.flush().ok()?;
    let deadline = Instant::now() + Duration::from_millis(100);
    let mut reply = Vec::new();
    let mut buf = [0u8; 256];
    while Instant::now() < deadline {
        match tty.read(&mut buf) {
            Ok(n) if n > 0 => {
                reply.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&reply);
                if text
                    .rfind("\x1b[?")
                    .is_some_and(|i| text[i..].contains('c'))
                {
                    break;
                }
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(5)),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break,
        }
    }
    theme::osc11_rgb(&String::from_utf8_lossy(&reply))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[cfg(feature = "native")]
fn read_background() -> Option<Rgb> {
    None
}

#[cfg(feature = "native")]
fn enter(enhanced: bool, mouse: bool) -> io::Result<()> {
    execute!(stdout(), EnterAlternateScreen)?;
    enable_input(enhanced, mouse)
}

/**
Kitty flags for the reader.

Report-all stays off. That mode sends the key that produced a character and
hides the composed text, which crossterm does not read, so Option-ò arrives as ò.
*/
#[cfg(feature = "native")]
fn keyboard_flags() -> KeyboardEnhancementFlags {
    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
}

#[cfg(feature = "native")]
fn enable_input(enhanced: bool, mouse: bool) -> io::Result<()> {
    execute!(stdout(), EnableBracketedPaste, EnableFocusChange)?;
    if enhanced {
        execute!(stdout(), PushKeyboardEnhancementFlags(keyboard_flags()))?;
    }
    ENHANCED.store(enhanced, Ordering::Relaxed);
    capture(mouse, false);
    Ok(())
}

#[cfg(feature = "native")]
fn release_input(enhanced: bool) {
    watchdog::disarm();
    capture(false, false);
    if ENHANCED.swap(false, Ordering::Relaxed) && enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        DisableFocusChange,
        crossterm::cursor::Show
    );
}

#[cfg(feature = "native")]
pub fn leave(enhanced: bool) {
    release_input(enhanced);
    let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
}

// MARK: keys

fn normalized_key(mut key: KeyEvent) -> KeyEvent {
    // Some enhanced terminals report the base letter instead of its shifted codepoint.
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        if let KeyCode::Char(ch) = key.code {
            if ch.is_ascii_lowercase() {
                key.code = KeyCode::Char(ch.to_ascii_uppercase());
            }
        }
    }
    key
}

/**
Ctrl+R and Cmd+R reload the page on screen, the same as `:refresh`.

Matched before other keys so a composer, a dialog or Settings cannot swallow it.
Bare `r` stays reply, and Alt+r is not this chord.
*/
fn refetch_chord(key: KeyEvent) -> bool {
    let KeyCode::Char('r' | 'R') = key.code else {
        return false;
    };
    key.modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER | KeyModifiers::META)
}

fn terminal_shortcut(key: KeyEvent) -> bool {
    key.modifiers
        .intersects(KeyModifiers::SUPER | KeyModifiers::META)
}

fn disclosure(state: &mut State, disclosure_keys: &mut [bool; 6], key: KeyEvent) -> bool {
    let modifier = match key.code {
        KeyCode::Modifier(ModifierKeyCode::LeftControl) => Some(0),
        KeyCode::Modifier(ModifierKeyCode::RightControl) => Some(1),
        KeyCode::Modifier(ModifierKeyCode::LeftSuper) => Some(2),
        KeyCode::Modifier(ModifierKeyCode::RightSuper) => Some(3),
        KeyCode::Modifier(ModifierKeyCode::LeftMeta) => Some(4),
        KeyCode::Modifier(ModifierKeyCode::RightMeta) => Some(5),
        _ => None,
    };
    if let Some(index) = modifier {
        disclosure_keys[index] = key.kind != KeyEventKind::Release;
        state.reveal_shortcuts = disclosure_keys.iter().any(|held| *held);
        return true;
    }
    if key.kind == KeyEventKind::Release {
        if key.code == KeyCode::Null {
            *disclosure_keys = [false; 6];
            state.reveal_shortcuts = false;
        }
        return true;
    }
    if matches!(key.code, KeyCode::Modifier(_)) {
        return true;
    }
    false
}

pub(crate) fn handle_key(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    key: KeyEvent,
) -> Result<bool, String> {
    if disclosure(state, &mut ui.disclosure_keys, key) {
        return Ok(false);
    }
    if refetch_chord(key) && !matches!(ui.mode, Mode::SignIn(_)) {
        state.clear_error();
        return apply(state, clients, ui, Cmd::Refresh);
    }
    if terminal_shortcut(key) {
        return Ok(false);
    }
    let key = normalized_key(key);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if ctrl
        && key.code == KeyCode::Char('c')
        && !matches!(ui.mode, Mode::Compose(_) | Mode::SignIn(_))
    {
        return Ok(true);
    }
    if state.status == state::DEMO_NOTICE {
        state.status.clear();
    }
    state.clear_error();
    if matches!(ui.mode, Mode::Normal) && !state.settings {
        let focused = ui.focus.borrow().is_some();
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                step_focus(state, ui, key.code == KeyCode::Tab);
                return Ok(false);
            }
            // rxvt and the Linux console send Shift-F6 as F18.
            KeyCode::F(6) | KeyCode::F(18) => {
                let back =
                    key.code == KeyCode::F(18) || key.modifiers.contains(KeyModifiers::SHIFT);
                step_zone(state, ui, !back);
                return Ok(false);
            }
            KeyCode::Enter if focused => return press_focus(state, clients, ui),
            KeyCode::Esc if focused => {
                ui.focus.replace(None);
                return Ok(false);
            }
            _ => {
                ui.focus.replace(None);
            }
        }
    }
    let enhanced = ui.enhanced;
    match &mut ui.mode {
        Mode::SignIn(_) => {
            let taken = std::mem::replace(&mut ui.mode, Mode::Normal);
            let Mode::SignIn(mut form) = taken else {
                ui.mode = taken;
                return Ok(false);
            };
            return match edit_sign_in(&mut form, key) {
                SignStep::Close => Ok(false),
                SignStep::Stay => {
                    ui.mode = Mode::SignIn(form);
                    Ok(false)
                }
                SignStep::Submit => finish_sign_in(state, clients, ui, form),
            };
        }
        Mode::Help => {
            if ui.overflow.get() > 0 {
                if let Some(delta) = scroll_delta(key, ui.viewport.get().1) {
                    let next = ui.scroll.get() as i64 + delta as i64;
                    ui.scroll
                        .set(next.clamp(0, ui.overflow.get() as i64) as usize);
                    return Ok(false);
                }
            }
            ui.mode = Mode::Normal;
            return Ok(false);
        }
        Mode::Search(input) => {
            match key.code {
                KeyCode::Esc => ui.mode = Mode::Normal,
                KeyCode::Enter => {
                    let query = input.text().to_string();
                    ui.mode = Mode::Normal;
                    return apply(state, clients, ui, Cmd::Search(query));
                }
                _ => edit(input, key),
            }
            return Ok(false);
        }
        Mode::Command { input, pick } => {
            let typed = input.text().to_string();
            let matches = command_matches(&typed);
            let rows = matches.len().max(1);
            match key.code {
                KeyCode::Esc => ui.mode = Mode::Normal,
                KeyCode::Backspace if typed.is_empty() => ui.mode = Mode::Normal,
                KeyCode::Up => *pick = (*pick + rows - 1) % rows,
                KeyCode::Down => *pick = (*pick + 1) % rows,
                KeyCode::Char('p') if ctrl => *pick = (*pick + rows - 1) % rows,
                KeyCode::Char('n') if ctrl => *pick = (*pick + 1) % rows,
                KeyCode::Tab => {
                    if let Some((name, args, _)) = matches.get(*pick) {
                        *input = Input::from(&completed(&typed, name, args));
                        *pick = 0;
                    }
                }
                KeyCode::Enter => {
                    let picked = matches.get(*pick).copied();
                    let line = if typed.trim().is_empty() {
                        picked.map_or_else(String::new, |(name, args, _)| completed("", name, args))
                    } else {
                        command_line(&typed, picked)
                    };
                    ui.mode = Mode::Normal;
                    return match command::parse(&line) {
                        Ok(cmd) => apply(state, clients, ui, cmd),
                        Err(err) => {
                            state.fail(err);
                            Ok(false)
                        }
                    };
                }
                _ => {
                    edit(input, key);
                    *pick = 0;
                }
            }
            return Ok(false);
        }
        Mode::Compose(_) => {
            return compose_key(state, clients, ui, key, enhanced);
        }
        Mode::Share(share) => {
            match share.key(key.code) {
                ShareStep::Stay => {}
                ShareStep::Status(text) => state.fail(text),
                ShareStep::Close => ui.mode = Mode::Normal,
                ShareStep::Run(action) => {
                    let Mode::Share(share) = std::mem::replace(&mut ui.mode, Mode::Normal) else {
                        return Ok(false);
                    };
                    return run_share(state, clients, ui, &share, action);
                }
            }
            return Ok(false);
        }
        Mode::Gallery(gallery) => {
            match gallery_key(gallery, key) {
                GalleryStep::Stay => {}
                GalleryStep::Close => ui.mode = Mode::Normal,
                GalleryStep::Open(url) => match util::open_browser(&url) {
                    Ok(()) => state.confirm(url),
                    Err(err) => state.fail(err),
                },
                GalleryStep::Share => {
                    if let Some(post) = gallery.post.clone() {
                        let share = Share::post(&post, &state.post_targets());
                        ui.mode = Mode::Share(Box::new(share));
                    }
                }
            }
            return Ok(false);
        }
        Mode::Pick(picker) => {
            let kind = picker.kind;
            let rows = kind.rows(state);
            let count = rows.len();
            if kind == PickKind::Theme {
                let list = Scheme::list();
                let revert = picker.revert;
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                        picker.cursor = (picker.cursor + 1) % count;
                        state.scheme = list[picker.cursor];
                        state.retheme();
                    }
                    KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                        picker.cursor = (picker.cursor + count - 1) % count;
                        state.scheme = list[picker.cursor];
                        state.retheme();
                    }
                    KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => {
                        if let Some(scheme) = revert {
                            state.scheme = scheme;
                            state.retheme();
                        }
                        ui.mode = Mode::Normal;
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let picked = list[picker.cursor];
                        let chosen = match revert {
                            Some(scheme) if scheme.base() == picked => scheme,
                            _ => picked,
                        };
                        state.scheme = chosen;
                        state.retheme();
                        ui.mode = Mode::Normal;
                        if Some(chosen) != revert {
                            let word = chosen.word();
                            match settings::save_for(
                                state,
                                &format!("theme {word}"),
                                "ui.theme",
                                &word,
                            ) {
                                Ok(status) => state.confirm(status),
                                Err(err) => state.fail(err),
                            }
                        }
                    }
                    _ => {}
                }
                return Ok(false);
            }
            let chosen = match key.code {
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                    picker.cursor = (picker.cursor + 1) % count;
                    None
                }
                KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                    picker.cursor = (picker.cursor + count - 1) % count;
                    None
                }
                KeyCode::Enter | KeyCode::Char(' ') => Some(picker.cursor),
                KeyCode::Char('s') if kind == PickKind::Detail => {
                    let level = Detail::ALL[picker.cursor];
                    ui.mode = Mode::Normal;
                    save_detail(state, level);
                    return Ok(false);
                }
                KeyCode::Char(digit) if digit.is_ascii_digit() => {
                    rows.iter().position(|row| row.key == digit)
                }
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => {
                    ui.mode = Mode::Normal;
                    None
                }
                KeyCode::Char(typed) if kind.opener() == Some(typed) => {
                    ui.mode = Mode::Normal;
                    None
                }
                _ => None,
            };
            let Some(i) = chosen.filter(|i| *i < count) else {
                return Ok(false);
            };
            match kind {
                PickKind::Network => return pick_network(state, clients, ui, i),
                PickKind::Detail => {
                    ui.mode = Mode::Normal;
                    if !rows[i].current {
                        save_detail(state, Detail::ALL[i]);
                    }
                }
                PickKind::Theme => {}
            }
            return Ok(false);
        }
        Mode::Filter { on, cursor } => {
            match key.code {
                KeyCode::Esc | KeyCode::Backspace => ui.mode = Mode::Normal,
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                    *cursor = (*cursor + 1).min(Network::ALL.len() - 1);
                }
                KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                    *cursor = cursor.saturating_sub(1);
                }
                KeyCode::Char(' ') => {
                    if let Err(err) = settings::toggle(state, on, Network::ALL[*cursor]) {
                        state.fail(err);
                    }
                }
                KeyCode::Char('a') => *on = state.available(),
                KeyCode::Enter => {
                    let on = std::mem::take(on);
                    ui.mode = Mode::Normal;
                    let n = on.len();
                    let value = settings::networks_value(&on);
                    let what = format!("{n} networks in your feed");
                    match state
                        .switch_on(on, clients)
                        .and_then(|()| settings::save_for(state, &what, "feed.networks", &value))
                    {
                        Ok(saved) if saved == settings::SAVED => {
                            state.confirm(format!("{saved} · {what}"))
                        }
                        Ok(status) => state.confirm(status),
                        Err(err) => state.fail(err),
                    }
                }
                _ => {}
            }
            return Ok(false);
        }
        Mode::Ask { input, asked } => {
            if let Some(delta) = scroll_delta(key, ui.viewport.get().1)
                .filter(|_| matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) || ctrl)
            {
                let back = ui.scroll.get() as i64 - delta as i64;
                ui.scroll
                    .set(back.clamp(0, ui.overflow.get() as i64) as usize);
                return Ok(false);
            }
            match key.code {
                KeyCode::Esc => ui.mode = Mode::Normal,
                KeyCode::Enter => {
                    let question = input.text();
                    if !question.trim().is_empty() {
                        asked.push(question.trim().to_string());
                        *input = Input::new();
                        ui.scroll.set(0);
                    }
                }
                _ => edit(input, key),
            }
            return Ok(false);
        }
        Mode::Inspect {
            value,
            open,
            cursor,
            ..
        } => {
            let nodes = jsonview::visible(value, open);
            let tab = match key.code {
                KeyCode::Tab => Some(1),
                KeyCode::BackTab => Some(-1),
                _ => None,
            };
            if let Some(delta) = tab.or_else(|| scroll_delta(key, ui.viewport.get().1)) {
                let next = *cursor as i64 + i64::from(delta);
                *cursor = next.clamp(0, nodes.len().saturating_sub(1) as i64) as usize;
                return Ok(false);
            }
            match key.code {
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => ui.mode = Mode::Normal,
                KeyCode::Enter => {
                    if let Some((node, _)) = nodes.get(*cursor) {
                        jsonview::toggle(open, node);
                    }
                }
                _ => {}
            }
            return Ok(false);
        }
        Mode::Overflow(_) => {
            let taken = std::mem::replace(&mut ui.mode, Mode::Normal);
            let Mode::Overflow(mut menu) = taken else {
                return Ok(false);
            };
            let count = menu.items.len();
            if count == 0 {
                return Ok(false);
            }
            match key.code {
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                    menu.cursor = (menu.cursor + 1) % count;
                    ui.mode = Mode::Overflow(menu);
                }
                KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                    menu.cursor = (menu.cursor + count - 1) % count;
                    ui.mode = Mode::Overflow(menu);
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let act = (*menu.items[menu.cursor].act).clone();
                    let target = Target {
                        x: 0,
                        y: 0,
                        width: 0,
                        act,
                        zone: Zone::Subbar,
                        tab: false,
                    };
                    return activate(state, clients, ui, &target);
                }
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => {}
                _ => ui.mode = Mode::Overflow(menu),
            }
            return Ok(false);
        }
        Mode::Normal => {}
    }

    if key.code == KeyCode::Char('T') && !ctrl {
        ui.mode = PickKind::Theme.open(state);
        return Ok(false);
    }
    if state.settings {
        return settings_key(state, clients, ui, key);
    }

    if onboard::showing(state) && matches!(ui.mode, Mode::Normal) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down if !ctrl => {
                state.gate_row = onboard::step(state.gate_row, true);
                return Ok(false);
            }
            KeyCode::Char('k') | KeyCode::Up if !ctrl => {
                state.gate_row = onboard::step(state.gate_row, false);
                return Ok(false);
            }
            KeyCode::Char('g') | KeyCode::Home if !ctrl => {
                state.gate_row = onboard::CHOICES - 1;
                return Ok(false);
            }
            KeyCode::Char('G') | KeyCode::End if !ctrl => {
                state.gate_row = onboard::CHOICES - 2;
                return Ok(false);
            }
            KeyCode::Enter | KeyCode::Char(' ') if !ctrl => {
                return welcome_choice(state, clients, ui);
            }
            _ => {}
        }
    }

    if state.current_posts().is_empty() {
        if let Some(delta) = scroll_delta(key, ui.viewport.get().1) {
            let next = ui.scroll.get() as i64 + delta as i64;
            ui.scroll.set(next.max(0) as usize);
            return Ok(false);
        }
    }

    if let Some(rows) = page_rows(key, ui.viewport.get().1) {
        if slide_focus(state, ui, rows) {
            return Ok(false);
        }
    }
    if page_key(state, ui, key) || search_key(state, ui, key) {
        return Ok(false);
    }
    let cmd = match key.code {
        KeyCode::Char(':') => {
            ui.mode = Mode::Command {
                input: Input::new(),
                pick: 0,
            };
            return Ok(false);
        }
        KeyCode::Char('k') if ctrl => {
            ui.mode = Mode::Command {
                input: Input::new(),
                pick: 0,
            };
            return Ok(false);
        }
        KeyCode::Char('q') if !ctrl => return Ok(true),
        KeyCode::Char('?') => {
            ui.mode = Mode::Help;
            ui.scroll.set(0);
            return Ok(false);
        }
        KeyCode::Char('d') if !ctrl => {
            step_detail(state, 1);
            return Ok(false);
        }
        KeyCode::Char('D') => {
            ui.mode = PickKind::Detail.open(state);
            return Ok(false);
        }
        KeyCode::Char(',') => {
            state.open_settings();
            state.status.clear();
            return Ok(false);
        }
        KeyCode::Char('n') if !ctrl => {
            match state.initial_target() {
                Some(target) => {
                    ui.mode = Mode::Compose(Composer::open(state, None, None, vec![target]));
                }
                None => state.fail(
                    "no signed-in network can post. `socli login bluesky` (or x, devnull, threads, mastodon)",
                ),
            }
            return Ok(false);
        }
        KeyCode::Char('r') if !ctrl => {
            if let Some(id) = state.selected_post_id() {
                let Some((target, _)) = net::split(&id) else {
                    return Ok(false);
                };
                state.thread_focus = state.posts.get(&id).map(|p| p.conversation_id.clone());
                ui.mode = Mode::Compose(Composer::open(state, Some(id), None, vec![target]));
            }
            return Ok(false);
        }
        KeyCode::Char('.') => {
            if let Some(post) = state.selected_post_id().and_then(|id| state.posts.get(&id)) {
                ui.mode = Mode::Inspect {
                    id: post.id.clone(),
                    handle: post.author.username.clone(),
                    value: post.to_json(),
                    open: HashSet::new(),
                    cursor: 0,
                };
            }
            return Ok(false);
        }
        KeyCode::Char('a') if ctrl => {
            ui.mode = Mode::Ask {
                input: Input::new(),
                asked: Vec::new(),
            };
            ui.scroll.set(0);
            return Ok(false);
        }
        KeyCode::Char('j') | KeyCode::Down if !ctrl => {
            if slide_focus(state, ui, 1) {
                return Ok(false);
            }
            Cmd::SelectDelta(1)
        }
        KeyCode::Char('k') | KeyCode::Up if !ctrl => {
            if slide_focus(state, ui, -1) {
                return Ok(false);
            }
            Cmd::SelectDelta(-1)
        }
        KeyCode::Char('g') | KeyCode::Home if !ctrl => Cmd::SelectFirst,
        KeyCode::Char('G') | KeyCode::End if !ctrl => Cmd::SelectLast,

        KeyCode::Enter | KeyCode::Right => Cmd::Expand,
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => Cmd::Back,
        KeyCode::Char('l') if !ctrl => {
            if selected_flag(state, |p| p.liked) {
                Cmd::Unlike { n: None }
            } else {
                Cmd::Like { n: None }
            }
        }
        KeyCode::Char('t') if !ctrl => {
            if selected_flag(state, |p| p.reposted) {
                Cmd::Unrepost { n: None }
            } else {
                Cmd::Repost { n: None }
            }
        }
        KeyCode::Char('b') if !ctrl && !alt => {
            if selected_flag(state, |p| p.bookmarked) {
                Cmd::Unbookmark { n: None }
            } else {
                Cmd::Bookmark { n: None }
            }
        }
        KeyCode::Char('x') if !ctrl => {
            state.toggle_replies();
            return Ok(false);
        }
        KeyCode::Char('v') if !ctrl => Cmd::View { n: None },
        KeyCode::Char('u') if !ctrl => {
            if let Err(err) = state.open_author(clients) {
                state.fail(err);
            }
            return Ok(false);
        }
        KeyCode::Char('s') if !ctrl => {
            let posters = state.post_targets();
            let share = match (state.profile(), state.selected_profile_url()) {
                (Some((net, user)), Some(_)) => Share::profile(net, &user.username, &posters),
                _ => match state.selected_post() {
                    Some(post) => Share::post(post, &posters),
                    None => {
                        if let Err(err) = state.resolve(None) {
                            state.fail(err);
                        }
                        return Ok(false);
                    }
                },
            };
            ui.mode = Mode::Share(Box::new(share));
            return Ok(false);
        }
        KeyCode::Char('o') if !ctrl => Cmd::Open { n: None },
        KeyCode::Char('m') if !ctrl => {
            ask_page(state);
            return Ok(false);
        }
        KeyCode::Char(brace @ ('{' | '}')) => {
            let dir = if brace == '}' { 1 } else { -1 };
            if state.scope_net().is_none() {
                if let Some(tab) = state.cycle_merged_tab(dir) {
                    open_merged(state, clients, tab);
                }
                return Ok(false);
            }
            match state.cycle_tab(dir) {
                Some(cmd) => cmd,
                None => return Ok(false),
            }
        }
        KeyCode::Char(']') => Cmd::Section(state.section.cycle(1)),
        KeyCode::Char('[') => Cmd::Section(state.section.cycle(-1)),
        KeyCode::Char('a') if !ctrl => match state.next_scope() {
            Some(scope) => Cmd::Scope(scope),
            None => return Ok(false),
        },
        KeyCode::Char('A') => {
            ui.mode = PickKind::Network.open(state);
            return Ok(false);
        }
        KeyCode::Char('<') => match state.cycle_scope(-1) {
            Some(scope) => Cmd::Scope(scope),
            None => return Ok(false),
        },
        KeyCode::Char('>') => match state.cycle_scope(1) {
            Some(scope) => Cmd::Scope(scope),
            None => return Ok(false),
        },
        KeyCode::Char('0') => Cmd::Scope(Scope::All),
        KeyCode::Char(digit @ '1'..='9') => {
            match open_tab(state, clients, digit as usize - '1' as usize) {
                Some(cmd) => cmd,
                None => return Ok(false),
            }
        }
        _ => return Ok(false),
    };
    apply(state, clients, ui, cmd)
}

/// Keys inside the composer, its unsaved-draft dialog and its drafts list.
/// What a key did to the composer.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Stay,
    /// ⏎ on non-blank text.
    Submit,
    /// The composer closed after esc or ctrl-c; `quit` when ctrl-c asked to leave the reader.
    Closed {
        saved: bool,
        quit: bool,
    },
}

fn compose_key(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    key: KeyEvent,
    enhanced: bool,
) -> Result<bool, String> {
    let Mode::Compose(composer) = &mut ui.mode else {
        return Ok(false);
    };
    let targets = state.post_targets();
    match composer_key(state, composer, key, enhanced, &targets) {
        Step::Stay => {}
        Step::Closed { quit: true, .. } => return Ok(true),
        Step::Closed { quit: false, .. } => ui.mode = Mode::Normal,
        Step::Submit => {
            if submit(state, clients, composer, &drafts::path()) {
                ui.mode = Mode::Normal;
                if let Err(err) = state.ensure_loaded(clients) {
                    state.fail(err);
                }
            }
        }
    }
    Ok(false)
}

/**
Sends the composer's intent through `send::send_intent` with the drafts file `store`, and returns
whether the composer closes. It stays open when the record before the send could not be written,
and when the draft could not be updated after a network refused the post, then with only the
networks that failed.
*/
fn submit(state: &mut State, clients: &mut Clients, composer: &mut Composer, store: &Path) -> bool {
    let intent = composer.intent();
    let delivery = match send::send_intent(state, clients, &intent, store, send::TUI_HINT) {
        Ok(delivery) => delivery,
        Err(err) => {
            state.fail(err);
            return false;
        }
    };
    let notes: Vec<(Network, Result<String, String>)> = delivery
        .results
        .iter()
        .map(|(net, result)| {
            (
                *net,
                result
                    .as_ref()
                    .map(|sent| sent.note.clone())
                    .map_err(Clone::clone),
            )
        })
        .collect();
    let failed = delivery.failed();
    let posted = posted_status(&notes);
    match delivery.kept {
        Ok(()) if failed.is_empty() => {
            state.confirm(posted);
            true
        }
        Ok(()) => {
            state.fail(format!("{posted} · draft saved"));
            true
        }
        Err(err) => {
            state.fail(format!("{posted} · couldn't update the draft: {err}"));
            if failed.is_empty() {
                return true;
            }
            composer.targets = failed;
            composer.draft_id = Some(delivery.id);
            false
        }
    }
}

/// `posted to Threads and X`, then `· x failed: <error>` for each network that refused.
fn posted_status(results: &[(Network, Result<String, String>)]) -> String {
    let done: Vec<&str> = results
        .iter()
        .filter(|(_, result)| result.is_ok())
        .map(|(net, _)| net.title())
        .collect();
    let mut parts = Vec::new();
    if !done.is_empty() {
        let names = match done.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => done.join(""),
        };
        parts.push(format!("posted to {names}"));
    }
    for (net, result) in results {
        if let Err(err) = result {
            parts.push(format!("{} failed: {err}", net.slug()));
        }
    }
    parts.join(" · ")
}

/**
Applies one key to the composer: text editing, the drafts list, and the unsaved-draft dialog that
esc and ctrl-c raise on dirty text. `targets` are the networks tab cycles a new post through.
*/
fn composer_key(
    state: &mut State,
    composer: &mut Composer,
    key: KeyEvent,
    enhanced: bool,
    targets: &[Network],
) -> Step {
    if terminal_shortcut(key) {
        return Step::Stay;
    }
    let key = normalized_key(key);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    state.status.clear();
    if ctrl && key.code == KeyCode::Char('c') {
        if matches!(composer.overlay, Overlay::Ask(_)) {
            if let Err(err) = composer.save(state) {
                state.fail(err);
                return Step::Stay;
            }
            return Step::Closed {
                saved: true,
                quit: true,
            };
        }
        if !composer.dirty() {
            return Step::Closed {
                saved: false,
                quit: true,
            };
        }
        composer.overlay = Overlay::Ask(After::Quit);
        return Step::Stay;
    }
    match &mut composer.overlay {
        Overlay::Ask(_) => {
            let keep = match key.code {
                KeyCode::Char('y') => true,
                KeyCode::Char('n') => false,
                KeyCode::Esc => {
                    composer.overlay = Overlay::None;
                    return Step::Stay;
                }
                _ => return Step::Stay,
            };
            let Overlay::Ask(after) = std::mem::replace(&mut composer.overlay, Overlay::None)
            else {
                return Step::Stay;
            };
            let done = if keep {
                composer
                    .save(state)
                    .map(|()| "draft saved · ctrl-o in the composer opens your drafts".to_string())
            } else {
                composer.discard().map(|()| "draft discarded".to_string())
            };
            match done {
                Ok(status) => state.confirm(status),
                Err(err) => {
                    state.fail(err);
                    return Step::Stay;
                }
            }
            return match after {
                After::Leave => Step::Closed {
                    saved: keep,
                    quit: false,
                },
                After::Quit => Step::Closed {
                    saved: keep,
                    quit: true,
                },
                After::Load(draft) => {
                    if let Some(status) = draft.sending_status() {
                        state.status = status;
                    }
                    composer.load(*draft);
                    composer.count();
                    Step::Stay
                }
            };
        }
        Overlay::List { drafts, cursor } => {
            match key.code {
                KeyCode::Esc | KeyCode::Backspace => composer.overlay = Overlay::None,
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                    *cursor = (*cursor + 1).min(drafts.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                    *cursor = cursor.saturating_sub(1);
                }
                KeyCode::Char('d') => {
                    if let Some(draft) = drafts.get(*cursor) {
                        if let Err(err) = drafts::remove(&draft.id) {
                            state.fail(err);
                            return Step::Stay;
                        }
                        if composer.draft_id.as_deref() == Some(draft.id.as_str()) {
                            composer.draft_id = None;
                            composer.origin = None;
                            composer.autosaved = None;
                        }
                        drafts.remove(*cursor);
                        *cursor = (*cursor).min(drafts.len().saturating_sub(1));
                        composer.saved = drafts.len();
                        state.confirm("draft deleted");
                        if drafts.is_empty() {
                            composer.overlay = Overlay::None;
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some(draft) = drafts.get(*cursor).cloned() {
                        if composer.dirty() {
                            composer.overlay = Overlay::Ask(After::Load(Box::new(draft)));
                        } else {
                            composer.overlay = Overlay::None;
                            if let Err(err) = composer.discard() {
                                state.fail(err);
                                return Step::Stay;
                            }
                            if let Some(status) = draft.sending_status() {
                                state.status = status;
                            }
                            composer.load(draft);
                        }
                    }
                }
                _ => {}
            }
            return Step::Stay;
        }
        Overlay::None => {}
    }
    if submits(key, state.enter, enhanced) {
        if composer.input.text().trim().is_empty() {
            state.fail("nothing to post");
            return Step::Stay;
        }
        return Step::Submit;
    }
    if breaks_line(key, state.enter, enhanced) {
        composer.input.insert('\n');
        return Step::Stay;
    }
    match key.code {
        KeyCode::Esc => {
            if composer.dirty() {
                composer.overlay = Overlay::Ask(After::Leave);
            } else {
                if let Err(err) = composer.discard() {
                    state.fail(err);
                }
                return Step::Closed {
                    saved: false,
                    quit: false,
                };
            }
        }
        KeyCode::Char('o') if ctrl => match drafts::load() {
            Ok(list) if list.is_empty() => state.fail("no saved drafts yet"),
            Ok(list) => {
                composer.saved = list.len();
                composer.overlay = Overlay::List {
                    drafts: list,
                    cursor: 0,
                };
            }
            Err(err) => state.fail(err),
        },
        KeyCode::Tab | KeyCode::BackTab
            if composer.reply_to.is_none()
                && composer.quote.is_none()
                && composer.targets.len() == 1 =>
        {
            let back = key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
            if let Some(i) = targets.iter().position(|net| *net == composer.target()) {
                let step = if back { targets.len() - 1 } else { 1 };
                composer.targets = vec![targets[(i + step) % targets.len()]];
            }
        }
        KeyCode::Up => composer.input.up(),
        KeyCode::Down => composer.input.down(),
        _ => edit(&mut composer.input, key),
    }
    Step::Stay
}

/**
The key posts from the composer: ⏎ under `ui.enter = post`; ctrl-s, or ctrl-⏎ where the terminal
reports it, under `newline`. Raw mode turns XON/XOFF off, so ctrl-s reaches the reader.
*/
fn submits(key: KeyEvent, enter: Enter, enhanced: bool) -> bool {
    if terminal_shortcut(key) {
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match enter {
        Enter::Post => key.code == KeyCode::Enter && !(enhanced && shift),
        Enter::Newline => {
            (ctrl && key.code == KeyCode::Char('s'))
                || (enhanced && ctrl && key.code == KeyCode::Enter)
        }
    }
}

/// The key starts a new line in the composer: ctrl-j, ⇧⏎ where reported, and ⏎ under `newline`.
fn breaks_line(key: KeyEvent, enter: Enter, enhanced: bool) -> bool {
    if terminal_shortcut(key) {
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let enter_key = key.code == KeyCode::Enter;
    (ctrl && key.code == KeyCode::Char('j'))
        || (enhanced && shift && enter_key)
        || (enter == Enter::Newline && enter_key && !submits(key, enter, enhanced))
}

/// `d`: the next detail level, wrapping after the last, saved as `ui.detail`.
fn step_detail(state: &mut State, dir: i32) {
    save_detail(state, state.detail.step(dir));
}

/// Applies a detail level in the running reader.
fn set_detail(state: &mut State, level: Detail) {
    state.detail = level;
    state.retheme();
    state.confirm(format!("Detail: {}", level.word()));
}

/// Applies `level` and writes it to `ui.detail` in the user config.
fn save_detail(state: &mut State, level: Detail) {
    set_detail(state, level);
    let word = level.word();
    let what = format!("Detail: {word}");
    match settings::save_for(state, &what, "ui.detail", word) {
        Ok(saved) if saved == settings::SAVED => state.confirm(format!("{what} · {saved}")),
        Ok(status) => state.confirm(status),
        Err(err) => state.fail(format!("{what} · {err}")),
    }
}

/// Keys on the Settings screen: every change applies at once and is saved.
fn settings_key(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    key: KeyEvent,
) -> Result<bool, String> {
    if state.settings_page == SettingsPage::Accounts {
        return accounts_key(state, clients, ui, key);
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let row = settings::ROWS
        .get(state.settings_row)
        .copied()
        .unwrap_or(Row::ColorBy);
    if let Some(rows) =
        page_rows(key, ui.viewport.get().1).filter(|_| key.code != KeyCode::Char(' '))
    {
        let last = settings::page_len(state) as i64 - 1;
        state.settings_row = (state.settings_row as i64 + i64::from(rows)).clamp(0, last) as usize;
        return Ok(false);
    }
    let dir = match key.code {
        KeyCode::Char('q') if !ctrl => return Ok(true),
        KeyCode::Char('?') => {
            ui.mode = Mode::Help;
            ui.scroll.set(0);
            return Ok(false);
        }
        KeyCode::Esc | KeyCode::Backspace => {
            return apply(state, clients, ui, Cmd::Back);
        }
        KeyCode::Char('d') if !ctrl => {
            step_detail(state, 1);
            return Ok(false);
        }
        KeyCode::Char('D') => {
            ui.mode = PickKind::Detail.open(state);
            return Ok(false);
        }
        KeyCode::Char('j') | KeyCode::Down if !ctrl => {
            state.settings_row = (state.settings_row + 1).min(settings::page_len(state) - 1);
            return Ok(false);
        }
        KeyCode::Tab => {
            state.settings_row = (state.settings_row + 1) % settings::page_len(state);
            return Ok(false);
        }
        KeyCode::BackTab => {
            let count = settings::page_len(state);
            state.settings_row = (state.settings_row + count - 1) % count;
            return Ok(false);
        }
        KeyCode::Char('k') | KeyCode::Up if !ctrl => {
            state.settings_row = state.settings_row.saturating_sub(1);
            return Ok(false);
        }
        KeyCode::Char(' ') => {
            if let Row::Net(net) = row {
                if !state.available().contains(&net) {
                    return account_session(state, clients, ui);
                }
                let mut on = state.on.clone();
                match settings::toggle(state, &mut on, net) {
                    Ok(()) => {
                        let value = settings::networks_value(&on);
                        let switched = state.switch_on(on, clients);
                        state.settings = true;
                        let what = format!("feed.networks = {value}");
                        match switched.and_then(|()| {
                            settings::save_for(state, &what, "feed.networks", &value)
                        }) {
                            Ok(status) => state.confirm(status),
                            Err(err) => state.fail(err),
                        }
                    }
                    Err(err) => state.fail(err),
                }
            }
            return Ok(false);
        }
        KeyCode::Enter if matches!(row, Row::Accounts) => {
            state.open_accounts();
            return Ok(false);
        }
        KeyCode::Enter if matches!(row, Row::Net(_)) => {
            return account_session(state, clients, ui);
        }
        KeyCode::Enter if matches!(row, Row::Theme) => {
            ui.mode = PickKind::Theme.open(state);
            return Ok(false);
        }
        KeyCode::Char('h') | KeyCode::Left => -1,
        KeyCode::Char('l') | KeyCode::Right => 1,
        _ => return Ok(false),
    };
    if let Some((key, value)) = settings::step(state, row, dir) {
        save_step(state, &key, &value);
    }
    Ok(false)
}

/// Keys on the accounts page. Enter and space sign in or out of the row's network.
fn accounts_key(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    key: KeyEvent,
) -> Result<bool, String> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let count = Network::ALL.len();
    if let Some(rows) =
        page_rows(key, ui.viewport.get().1).filter(|_| key.code != KeyCode::Char(' '))
    {
        let last = count as i64 - 1;
        state.settings_row = (state.settings_row as i64 + i64::from(rows)).clamp(0, last) as usize;
        return Ok(false);
    }
    match key.code {
        KeyCode::Char('q') if !ctrl => Ok(true),
        KeyCode::Char('?') => {
            ui.mode = Mode::Help;
            ui.scroll.set(0);
            Ok(false)
        }
        KeyCode::Esc | KeyCode::Backspace => {
            state.settings_page = SettingsPage::Main;
            state.settings_row = settings::ROWS
                .iter()
                .position(|row| *row == Row::Accounts)
                .unwrap_or(0);
            Ok(false)
        }
        KeyCode::Char('d') if !ctrl => {
            step_detail(state, 1);
            Ok(false)
        }
        KeyCode::Char('D') => {
            ui.mode = PickKind::Detail.open(state);
            Ok(false)
        }
        KeyCode::Char('j') | KeyCode::Down if !ctrl => {
            state.settings_row = (state.settings_row + 1).min(count - 1);
            Ok(false)
        }
        KeyCode::Tab => {
            state.settings_row = (state.settings_row + 1) % count;
            Ok(false)
        }
        KeyCode::BackTab => {
            state.settings_row = (state.settings_row + count - 1) % count;
            Ok(false)
        }
        KeyCode::Char('k') | KeyCode::Up if !ctrl => {
            state.settings_row = state.settings_row.saturating_sub(1);
            Ok(false)
        }
        KeyCode::Char(' ') | KeyCode::Enter => account_session(state, clients, ui),
        _ => Ok(false),
    }
}

/// Not U+258C. The page paints that half-block as a filled cell, so a field cursor became a bar.
const FIELD_CURSOR: char = '▍';

struct SignField {
    label: &'static str,
    value: String,
    secret: bool,
}

struct SignInForm {
    net: Network,
    fields: Vec<SignField>,
    guides: Vec<(String, String)>,
    aside: String,
    pending_state: String,
    pending_verifier: String,
    cursor: usize,
    error: Option<String>,
}

enum SignStep {
    Stay,
    Close,
    Submit,
}

enum SignResult {
    SignedIn,
    /// The browser is leaving for the network's approve page. The form stays until it does.
    Left,
}

impl SignInForm {
    /// The welcome screen opens this on wasm. Native tests build it directly.
    #[cfg(any(test, target_arch = "wasm32"))]
    fn open(net: Network) -> Self {
        let field = |label, secret| SignField {
            label,
            value: String::new(),
            secret,
        };
        let mut error = None;
        let (fields, guides, aside, pending_state, pending_verifier) = match net {
            Network::Bluesky => (
                vec![field("Username", false), field("App password", true)],
                Vec::new(),
                "An app password from bsky.app/settings/app-passwords. Escape cancels.".into(),
                String::new(),
                String::new(),
            ),
            Network::Mastodon => (
                vec![SignField {
                    label: "Server",
                    value: crate::net::mastodon::DEFAULT_SERVER.to_string(),
                    secret: false,
                }],
                vec![(
                    "Sign-in method".into(),
                    "Approve SoCLI on your Mastodon server".into(),
                )],
                format!(
                    "Enter your server, or press Enter to use {}. socli registers itself there and opens the approval page. Escape closes this.",
                    crate::net::mastodon::DEFAULT_SERVER
                ),
                String::new(),
                String::new(),
            ),
            Network::Threads => {
                let (guides, aside) = crate::net::threads::page_copy();
                (Vec::new(), guides, aside, String::new(), String::new())
            }
            Network::X => {
                let (guides, aside) = crate::net::x::auth::page_copy();
                (Vec::new(), guides, aside, String::new(), String::new())
            }
            Network::Devnull => {
                let opened = devnull_sign_in(field);
                error = opened.error;
                (
                    opened.fields,
                    opened.guides,
                    opened.aside,
                    opened.state,
                    opened.verifier,
                )
            }
        };
        Self {
            net,
            fields,
            guides,
            aside,
            pending_state,
            pending_verifier,
            cursor: 0,
            error,
        }
    }

    fn shown(&self, index: usize) -> String {
        let Some(field) = self.fields.get(index) else {
            return String::new();
        };
        let mut text = if field.secret {
            "•".repeat(field.value.chars().count())
        } else {
            field.value.clone()
        };
        if index == self.cursor {
            text.push(FIELD_CURSOR);
        }
        text
    }

    fn value(&self, label: &str) -> String {
        self.fields
            .iter()
            .find(|field| field.label == label)
            .map(|field| field.value.trim().to_string())
            .unwrap_or_default()
    }

    fn required(&self, label: &str) -> Result<String, String> {
        let text = self.value(label);
        if text.is_empty() {
            Err(format!("{label} is required"))
        } else {
            Ok(text)
        }
    }

    fn submit(&self) -> Result<SignResult, String> {
        match self.net {
            Network::Bluesky => {
                crate::net::bluesky::login_with_password(
                    self.required("Username")?,
                    None,
                    self.required("App password")?,
                )?;
                Ok(SignResult::SignedIn)
            }
            Network::Mastodon => {
                let url = crate::net::mastodon::begin_page(
                    &self.value("Server"),
                    &crate::util::page_url()?,
                )?;
                crate::util::open_browser(&url)?;
                Ok(SignResult::Left)
            }
            Network::Threads => {
                let url = crate::net::threads::begin_page(&crate::util::page_url()?)?;
                crate::util::open_browser(&url)?;
                Ok(SignResult::Left)
            }
            Network::X => {
                let id = crate::net::x::auth::compiled_client_id()
                    .ok_or_else(|| crate::net::x::auth::NO_APP.to_string())?;
                let url = crate::net::x::auth::begin_page(id, &crate::util::page_url()?)?;
                crate::util::open_browser(&url)?;
                Ok(SignResult::Left)
            }
            Network::Devnull => {
                let callback = self.value("Callback");
                if callback.is_empty() {
                    return Err("Paste the full devnull:// address from the browser's address bar, or the Location header of the approval request.".into());
                }
                #[cfg(not(feature = "native"))]
                {
                    crate::net::devnull::finish(
                        &callback,
                        &self.pending_state,
                        &self.pending_verifier,
                    )?;
                    return Ok(SignResult::SignedIn);
                }
                #[cfg(feature = "native")]
                {
                    let _ = (&callback, &self.pending_state, &self.pending_verifier);
                    Err("paste that callback on the page. this build signs in with `socli login devnull`.".into())
                }
            }
        }
    }
}

/// The page copy for dev/null. The terminal's non-macOS path is the same paste.
#[cfg(any(test, target_arch = "wasm32"))]
fn devnull_page_copy() -> (Vec<(String, String)>, String) {
    (
        vec![
            (
                "Device key".into(),
                "A key for this page. The approval page shows its fingerprint.".into(),
            ),
            (
                "Approve in the browser".into(),
                "SoCLI opens dev-null.sh so you can approve this page.".into(),
            ),
            (
                "Paste the callback".into(),
                "devnull://auth/callback?code=…".into(),
            ),
        ],
        "Approve SoCLI in the browser. If the browser cannot open devnull://, copy the full callback URL from its address bar or the approval request's Location response header.".into(),
    )
}

/// What `open` needs for dev/null. The page also keeps the PKCE pair until the callback is pasted.
#[cfg(any(test, target_arch = "wasm32"))]
struct OpenedDevnull {
    fields: Vec<SignField>,
    guides: Vec<(String, String)>,
    aside: String,
    state: String,
    verifier: String,
    error: Option<String>,
}

/// Opens the approval when this is the page. A native test only draws the same steps.
#[cfg(any(test, target_arch = "wasm32"))]
fn devnull_sign_in(field: impl Fn(&'static str, bool) -> SignField) -> OpenedDevnull {
    let fields = vec![field("Callback", false)];
    #[cfg(not(feature = "native"))]
    {
        let (mut guides, mut aside) = devnull_page_copy();
        return match crate::net::devnull::prepare() {
            Ok(prepared) => {
                let _ = crate::util::open_browser(&prepared.url);
                if let Some(step) = guides.first_mut() {
                    step.1 = format!("{} (ed25519, kept on this page)", prepared.fingerprint);
                }
                if let Some(step) = guides.get_mut(1) {
                    step.1 = prepared.url.clone();
                }
                aside = format!("{aside}\n\n{}", prepared.url);
                OpenedDevnull {
                    fields,
                    guides,
                    aside,
                    state: prepared.state,
                    verifier: prepared.verifier,
                    error: None,
                }
            }
            Err(error) => OpenedDevnull {
                fields,
                guides,
                aside,
                state: String::new(),
                verifier: String::new(),
                error: Some(error),
            },
        };
    }
    #[cfg(feature = "native")]
    {
        let (guides, aside) = devnull_page_copy();
        OpenedDevnull {
            fields,
            guides,
            aside,
            state: String::new(),
            verifier: String::new(),
            error: None,
        }
    }
}

fn edit_sign_in(form: &mut SignInForm, key: KeyEvent) -> SignStep {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => SignStep::Close,
        KeyCode::Char('c') if ctrl => SignStep::Close,
        KeyCode::Backspace | KeyCode::Delete => {
            if let Some(field) = form.fields.get_mut(form.cursor) {
                field.value.pop();
            }
            form.error = None;
            SignStep::Stay
        }
        KeyCode::Enter => {
            if !form.fields.is_empty() && form.cursor + 1 < form.fields.len() {
                form.cursor += 1;
                form.error = None;
                SignStep::Stay
            } else {
                SignStep::Submit
            }
        }
        KeyCode::Tab | KeyCode::Down => {
            if form.cursor + 1 < form.fields.len() {
                form.cursor += 1;
            }
            SignStep::Stay
        }
        KeyCode::BackTab | KeyCode::Up => {
            form.cursor = form.cursor.saturating_sub(1);
            SignStep::Stay
        }
        KeyCode::Char(ch) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
            if let Some(field) = form.fields.get_mut(form.cursor) {
                field.value.push(ch);
                form.error = None;
            }
            SignStep::Stay
        }
        _ => SignStep::Stay,
    }
}

fn finish_sign_in(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    mut form: SignInForm,
) -> Result<bool, String> {
    match form.submit() {
        Ok(SignResult::Left) => {
            ui.mode = Mode::SignIn(form);
        }
        Ok(SignResult::SignedIn) => {
            ui.mode = Mode::Normal;
            match reload_accounts(state, clients, Some(form.net)) {
                Ok(()) => {
                    let signed = match state.handle_of(form.net) {
                        Some(handle) => format!("signed in to {} as @{handle}", form.net.title()),
                        None => format!("signed in to {}", form.net.title()),
                    };
                    match state.ensure_loaded(clients) {
                        Ok(()) => state.confirm(signed),
                        Err(error) => {
                            state.fail(format!("{signed}; could not load the feed: {error}"))
                        }
                    }
                }
                Err(err) => state.fail(err),
            }
        }
        Err(err) => {
            form.error = Some(err);
            ui.mode = Mode::SignIn(form);
        }
    }
    Ok(false)
}

fn draw_sign_in(screen: &mut Screen<'_, '_>, ui: &Ui, form: &SignInForm) {
    // The value sits on the label row. A following rail row drew the cursor as its own bar.
    let shown: Vec<String> = form
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let value = form.shown(index);
            if value.is_empty() {
                field.label.to_string()
            } else {
                format!("{}  {value}", field.label)
            }
        })
        .collect();
    let mut steps: Vec<(&str, &str)> = form
        .guides
        .iter()
        .map(|(label, detail)| (label.as_str(), detail.as_str()))
        .collect();
    steps.extend(shown.iter().map(|text| (text.as_str(), "")));
    // `screen.line` adds the feed margin, so the frame is the inner width and the wordmark shares the reader's column.
    let width = screen
        .full
        .saturating_sub(screen.margin.saturating_mul(2))
        .max(1);
    let (mut lines, _) = screens::login_frame(form.net.title(), &steps, &form.aside, None, width);
    if let Some(error) = &form.error {
        lines.push(StyledLine::text(Kind::Red, error));
    }
    for (y, line) in lines.iter().enumerate() {
        if y >= screen.height {
            break;
        }
        screen.line(y, line);
    }
    ui.viewport.set((screen.full, screen.height));
    ui.hits.replace(Hits::default());
}

/// Signs the cursor's network in or out. Demo never writes, and agent mode never opens a browser.
fn account_session(state: &mut State, clients: &mut Clients, ui: &mut Ui) -> Result<bool, String> {
    let net = match state.settings_page {
        SettingsPage::Accounts => Network::ALL.get(state.settings_row).copied(),
        SettingsPage::Main => match settings::ROWS.get(state.settings_row).copied() {
            Some(Row::Net(net)) => Some(net),
            _ => None,
        },
    };
    let Some(net) = net else {
        return Ok(false);
    };
    if state.demo {
        state.confirm(format!("{} · demo, not saved", net.title()));
        return Ok(false);
    }
    if state.available().contains(&net) {
        match logout_net(net) {
            Ok(note) => match reload_accounts(state, clients, None) {
                Ok(()) => state.confirm(signed_out_status(net, note.is_some())),
                Err(err) => state.fail(err),
            },
            Err(err) => state.fail(err),
        }
        return Ok(false);
    }
    sign_in(state, clients, ui, net)
}

/// The welcome mark's cursor: a network signs in, and the last row opens the local demo.
fn welcome_choice(state: &mut State, clients: &mut Clients, ui: &mut Ui) -> Result<bool, String> {
    match Network::ALL.get(state.gate_row % onboard::CHOICES) {
        Some(net) => sign_in(state, clients, ui, *net),
        None => {
            state.enter_demo();
            Ok(false)
        }
    }
}

/// Signs `net` in. Demo never writes, and agent mode never opens a browser.
///
/// The browser opens the form inside the reader. A terminal still signs in on stdout.
fn sign_in(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    net: Network,
) -> Result<bool, String> {
    if state.demo {
        state.confirm(format!("{} · demo, not saved", net.title()));
        return Ok(false);
    }
    if crate::agent::active() {
        state.fail(format!(
            "signing in to {} needs a person at a terminal. Run `socli login {}`",
            net.title(),
            net.slug()
        ));
        return Ok(false);
    }
    #[cfg(target_arch = "wasm32")]
    {
        ui.mode = Mode::SignIn(SignInForm::open(net));
        ui.hits.replace(Hits::default());
        ui.viewport.set((0, 0));
        return Ok(false);
    }
    #[cfg(not(target_arch = "wasm32"))]
    match with_terminal_suspended(ui, state, || {
        login_with_result(|| login_net(net), screens::login_result)
    }) {
        Ok(true) => match reload_accounts(state, clients, Some(net)) {
            Ok(()) => {
                let signed = match state.handle_of(net) {
                    Some(handle) => format!("signed in to {} as @{handle}", net.title()),
                    None => format!("signed in to {}", net.title()),
                };
                match state.ensure_loaded(clients) {
                    Ok(()) => state.confirm(signed),
                    Err(error) => state.fail(format!("{signed}; could not load the feed: {error}")),
                }
            }
            Err(err) => state.fail(err),
        },
        Ok(false) => {}
        Err(err) => state.fail(err),
    }
    Ok(false)
}

/**
Each attempt starts on a blank screen. Retry used to print the next sign-in under the
previous error, and a short terminal kept both.
*/
fn clear_login_screen() -> Result<(), String> {
    #[cfg(all(feature = "native", not(test)))]
    if io::stdout().is_terminal() {
        execute!(
            stdout(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::Purge),
            crossterm::cursor::MoveTo(0, 0),
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn login_with_result(
    mut attempt: impl FnMut() -> Result<(), String>,
    mut result: impl FnMut(Result<(), &str>) -> Result<bool, String>,
) -> Result<bool, String> {
    screens::expect_login_result(true);
    let _close = ClearLoginResult;
    login_attempts(&mut attempt, &mut result)
}

struct ClearLoginResult;

impl Drop for ClearLoginResult {
    fn drop(&mut self) {
        screens::expect_login_result(false);
    }
}

fn login_attempts(
    attempt: &mut impl FnMut() -> Result<(), String>,
    result: &mut impl FnMut(Result<(), &str>) -> Result<bool, String>,
) -> Result<bool, String> {
    loop {
        clear_login_screen()?;
        match attempt() {
            Ok(()) => {
                result(Ok(()))?;
                return Ok(true);
            }
            Err(error) if error == "cancelled" => return Ok(false),
            Err(error) => {
                if !result(Err(&error))? {
                    return Ok(false);
                }
            }
        }
    }
}

fn login_net(net: Network) -> Result<(), String> {
    #[cfg(not(feature = "native"))]
    {
        let _ = net;
        return Err(crate::util::NEEDS_PERSON.into());
    }
    #[cfg(feature = "native")]
    match net {
        Network::X => crate::net::x::auth::login_browser(None, None),
        Network::Bluesky => crate::net::bluesky::login(None, None),
        Network::Devnull => crate::net::devnull::login(None),
        Network::Threads => crate::net::threads::login(None, None),
        Network::Mastodon => crate::net::mastodon::login(None, None),
    }
}

/// Status after a network is signed out. dev/null keeps its device key file, so that row says so.
fn signed_out_status(net: Network, key_stays: bool) -> String {
    let mut text = format!("signed out of {}", net.title());
    if key_stays {
        text.push_str(". The device key stays on disk. Revoke it at dev-null.sh/settings/devices");
    }
    text
}

fn logout_net(net: Network) -> Result<Option<String>, String> {
    match net {
        Network::X => crate::net::x::auth::logout().map(|()| None),
        Network::Bluesky => crate::net::bluesky::logout().map(|()| None),
        Network::Devnull => crate::net::devnull::logout(),
        Network::Threads => crate::net::threads::logout().map(|()| None),
        Network::Mastodon => crate::net::mastodon::logout().map(|()| None),
    }
}

fn reload_accounts(
    state: &mut State,
    clients: &mut Clients,
    also_on: Option<Network>,
) -> Result<(), String> {
    *clients = Clients::load(&crate::config::load()?)?;
    state.adopt_accounts(clients, also_on);
    Ok(())
}

/// Keeps login output in the alternate screen so it never enters the shell's scrollback.
fn with_terminal_suspended<T>(
    ui: &Ui,
    state: &State,
    body: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    #[cfg(not(feature = "native"))]
    {
        let _ = (ui, state, body);
        return Err(crate::util::NEEDS_PERSON.into());
    }
    #[cfg(feature = "native")]
    {
        let _handoff = TerminalHandoff::begin();
        release_input(ui.enhanced);
        let result = execute!(
            stdout(),
            terminal::Clear(terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0)
        )
        .map_err(|err| err.to_string())
        .and_then(|()| body());
        let resumed = resume_terminal(ui, state);
        match (result, resumed) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(body), Ok(())) => Err(body),
            (Ok(_), Err(resume)) => Err(resume),
            (Err(body), Err(resume)) => Err(format!("{body} ({resume})")),
        }
    }
}

#[cfg(feature = "native")]
fn resume_terminal(ui: &Ui, state: &State) -> Result<(), String> {
    // `run` still holds the session guard. leave() here would drop the alternate screen while the loop keeps drawing.
    enable_raw_mode().map_err(|e| e.to_string())?;
    enable_input(ui.enhanced, state.mouse).map_err(|e| e.to_string())?;
    watchdog::arm(watchdog::Probe::Stdout);
    // A stuck event stream must not keep the reader from coming back.
    for _ in 0..32 {
        if !event::poll(Duration::ZERO).unwrap_or(false) {
            break;
        }
        let _ = event::read();
    }
    Ok(())
}

/// Saves a Settings change, naming it `key = value` when the demo keeps it unsaved.
fn save_step(state: &mut State, key: &str, value: &str) {
    match settings::save_for(state, &format!("{key} = {value}"), key, value) {
        Ok(status) => state.confirm(status),
        Err(err) => state.fail(err),
    }
}

/// Rows a scroll key moves by in a view `height` rows tall, when it is one.
fn scroll_delta(key: KeyEvent, height: usize) -> Option<i32> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('j') | KeyCode::Down if !ctrl => Some(1),
        KeyCode::Char('k') | KeyCode::Up if !ctrl => Some(-1),
        _ => page_rows(key, height),
    }
}

/// H−2 rows for PgDn, PgUp, space, ctrl-f and ctrl-b, H/2 for ctrl-d and ctrl-u, in a view H rows tall.
fn page_rows(key: KeyEvent, height: usize) -> Option<i32> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let page = height.saturating_sub(2).max(1) as i32;
    let half = (height / 2).max(1) as i32;
    match key.code {
        KeyCode::Char('d') if ctrl => Some(half),
        KeyCode::Char('u') if ctrl => Some(-half),
        KeyCode::Char('f') if ctrl => Some(page),
        KeyCode::Char('b') if ctrl => Some(-page),
        KeyCode::PageDown => Some(page),
        KeyCode::Char(' ') if !ctrl => Some(page),
        KeyCode::PageUp => Some(-page),
        _ => None,
    }
}

fn selected_flag(state: &State, flag: impl Fn(&crate::model::Post) -> bool) -> bool {
    state
        .selected_post_id()
        .and_then(|id| state.posts.get(&id))
        .is_some_and(flag)
}

/// The editing keys of every text input; the composer adds ↑ and ↓, ask keeps ctrl-u for scrolling, and ctrl-k stays unbound.
fn edit(input: &mut Input, key: KeyEvent) {
    if terminal_shortcut(key) {
        return;
    }
    let key = normalized_key(key);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Backspace if alt => input.delete_word_back(),
        KeyCode::Char('w') if ctrl => input.delete_word_back(),
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),
        KeyCode::Left if alt || ctrl => input.word_left(),
        KeyCode::Right if alt || ctrl => input.word_right(),
        // Ghostty's default Option-Left and Option-Right send `esc b` and `esc f`.
        KeyCode::Char('b') if alt && !ctrl => input.word_left(),
        KeyCode::Char('f') if alt && !ctrl => input.word_right(),
        KeyCode::Left => input.left(),
        KeyCode::Right => input.right(),
        KeyCode::Home => input.home(),
        KeyCode::Char('a') if ctrl => input.home(),
        KeyCode::End => input.end(),
        KeyCode::Char('e') if ctrl => input.end(),
        KeyCode::Char('u') if ctrl => input.kill_to_home(),
        KeyCode::Char('Z') if ctrl => input.redo(),
        KeyCode::Char('z') if ctrl && shift => input.redo(),
        KeyCode::Char('z') if ctrl => input.undo(),
        KeyCode::Char('y') if ctrl => input.redo(),
        KeyCode::Char(ch) if !ctrl => input.insert(ch),
        _ => {}
    }
}

/// Bracketed paste text as an input takes it: [`util::clean`], so a paste can never act as a key.
fn clean_paste(text: &str) -> Cow<'_, str> {
    util::clean(text)
}

/**
Puts a bracketed paste into the open text input: whole into the composer, with newlines as spaces
into search, ask and the command line. Every other mode, and a composer overlay, drops it, and
then it returns false.
*/
pub(crate) fn paste(ui: &mut Ui, text: &str) -> bool {
    match &mut ui.mode {
        Mode::Compose(composer) => return composer.paste(text),
        Mode::Search(input) | Mode::Ask { input, .. } => {
            input.insert_str(&clean_paste(text).replace('\n', " "));
        }
        Mode::Command { input, pick } => {
            input.insert_str(&clean_paste(text).replace('\n', " "));
            *pick = 0;
        }
        Mode::SignIn(form) => {
            let Some(field) = form.fields.get_mut(form.cursor) else {
                return false;
            };
            field.value.push_str(&text.replace(['\n', '\r'], ""));
            form.error = None;
        }
        _ => return false,
    }
    true
}

fn open_merged(state: &mut State, clients: &mut Clients, tab: MergedTab) {
    if let Err(err) = state.open_merged_tab(tab, clients) {
        state.fail(err);
    }
}

fn apply(state: &mut State, clients: &mut Clients, ui: &mut Ui, cmd: Cmd) -> Result<bool, String> {
    match cmd {
        Cmd::Theme(None) => {
            ui.mode = PickKind::Theme.open(state);
            return Ok(false);
        }
        Cmd::Theme(Some(scheme)) => {
            state.scheme = scheme;
            state.retheme();
            let word = scheme.word();
            match settings::save_for(state, &format!("theme {word}"), "ui.theme", &word) {
                Ok(status) => state.confirm(status),
                Err(err) => state.fail(err),
            }
            return Ok(false);
        }
        _ => {}
    }
    let viewed = match &cmd {
        Cmd::View { n } | Cmd::Avatar { n } => state.resolve(*n).ok(),
        _ => None,
    };
    let outcome = match state.apply(cmd, clients) {
        Ok(outcome) => outcome,
        Err(err) => {
            state.fail(err);
            return Ok(false);
        }
    };
    match outcome {
        Outcome::Quit => return Ok(true),
        Outcome::Help => ui.mode = Mode::Help,
        Outcome::View(_) if state.viewer.mode == ImageMode::Off => {
            state.fail("images are off · socli config set ui.images auto");
        }
        Outcome::View(urls) => {
            let post = viewed.and_then(|id| state.posts.get(&id)).cloned();
            ui.mode = Mode::Gallery(Box::new(Gallery::new(post, urls)));
        }
        Outcome::Open(url) => match crate::util::open_browser(&url) {
            Ok(()) => state.confirm(url),
            Err(err) => state.fail(err),
        },
        Outcome::Share { url, profile } => {
            let what = if profile { "profile link" } else { "link" };
            match clip::copy(&url) {
                Ok(clip::Via::System) => state.confirm(format!("copied {what}")),
                Ok(clip::Via::Terminal) => {
                    state.confirm(format!("copied {what} (terminal clipboard)"))
                }
                Err(err) => state.fail(err),
            }
        }
        Outcome::Json(_) | Outcome::None => {
            if let Err(err) = state.ensure_loaded(clients) {
                state.fail(err);
            }
        }
    }
    Ok(false)
}

/// The network picker's choice of row `i`: All networks, then each network in order.
fn pick_network(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    i: usize,
) -> Result<bool, String> {
    let rows = screens::network_rows(state);
    let Some(row) = rows.get(i) else {
        return Ok(false);
    };
    let net = i.checked_sub(1).map(|n| Network::ALL[n]);
    if !row.enabled {
        state.fail(net.map(settings::not_signed_in).unwrap_or_default());
        return Ok(false);
    }
    ui.mode = Mode::Normal;
    if row.current {
        return Ok(false);
    }
    let scope = net.map_or(Scope::All, Scope::One);
    apply(state, clients, ui, Cmd::Scope(scope))
}

/// Opens feed tab `index` as its digit does; a merged tab opens here and leaves no command.
fn open_tab(state: &mut State, clients: &mut Clients, index: usize) -> Option<Cmd> {
    if state.scope_net().is_none() {
        if let Some(tab) = state.merged_tab_at(index) {
            open_merged(state, clients, tab);
        }
        return None;
    }
    state.tab_at(index)
}

// MARK: focus and mouse

fn plain_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// The body zone that has the keys: the thread pane while the split shows an open thread.
fn keyed_zone(state: &State, ui: &Ui) -> Zone {
    if ui.split.get() && state.thread_focus.is_some() {
        Zone::BodyRight
    } else {
        Zone::BodyLeft
    }
}

/// The tab stops in focus order: by zone, then row, then column.
fn tab_stops(targets: &[Target]) -> Vec<Target> {
    let mut stops: Vec<Target> = targets.iter().filter(|t| t.tab).cloned().collect();
    stops.sort_by_key(|t| (t.zone, t.y, t.x));
    stops
}

fn focus_of(stops: &[Target], i: usize) -> Focus {
    let target = &stops[i];
    let nth = stops[..i]
        .iter()
        .filter(|t| t.zone == target.zone && t.act == target.act)
        .count();
    Focus {
        zone: target.zone,
        act: target.act.clone(),
        nth,
    }
}

fn focused_index(stops: &[Target], focus: &Focus) -> Option<usize> {
    stops
        .iter()
        .enumerate()
        .filter(|(_, t)| t.zone == focus.zone && t.act == focus.act)
        .nth(focus.nth)
        .map(|(i, _)| i)
}

/// Tab and Shift-Tab, wrapping; a stop in the pane with the keys selects its post.
fn step_focus(state: &mut State, ui: &Ui, forward: bool) {
    let stops = tab_stops(&ui.hits.borrow().targets);
    let count = stops.len();
    if count == 0 {
        return;
    }
    let current = ui
        .focus
        .borrow()
        .as_ref()
        .and_then(|focus| focused_index(&stops, focus));
    let next = match (current, forward) {
        (Some(i), true) => (i + 1) % count,
        (Some(i), false) => (i + count - 1) % count,
        (None, true) => 0,
        (None, false) => count - 1,
    };
    let focus = focus_of(&stops, next);
    if focus.zone == keyed_zone(state, ui) {
        select_for(state, &focus.act);
    }
    ui.focus.replace(Some(focus));
}

/**
F6 and Shift-F6, wrapping: the first stop of the next zone that has stops. In the pane with the
keys that is the selected post's first stop, so the selection stays.
*/
fn step_zone(state: &mut State, ui: &Ui, forward: bool) {
    let stops = tab_stops(&ui.hits.borrow().targets);
    let mut zones: Vec<Zone> = stops.iter().map(|t| t.zone).collect();
    zones.dedup();
    let count = zones.len();
    if count == 0 {
        return;
    }
    let keyed = keyed_zone(state, ui);
    let current = ui.focus.borrow().as_ref().map_or(keyed, |focus| focus.zone);
    let next = match (zones.iter().position(|zone| *zone == current), forward) {
        (Some(i), true) => zones[(i + 1) % count],
        (Some(i), false) => zones[(i + count - 1) % count],
        (None, true) => zones[0],
        (None, false) => zones[count - 1],
    };
    let selected = state.selected_post_id();
    let own = stops.iter().position(|t| {
        t.zone == next && next == keyed && selected.is_some() && t.act.post() == selected.as_deref()
    });
    let Some(i) = own.or_else(|| stops.iter().position(|t| t.zone == next)) else {
        return;
    };
    let focus = focus_of(&stops, i);
    if focus.zone == keyed {
        select_for(state, &focus.act);
    }
    ui.focus.replace(Some(focus));
}

/// Selects the post a post-bound act belongs to, or for a fold its fold row.
fn select_for(state: &mut State, act: &Act) {
    if onboard::showing(state) {
        match act {
            Act::SignIn(net) => {
                if let Some(index) = Network::ALL.iter().position(|candidate| candidate == net) {
                    state.gate_row = index;
                }
            }
            Act::EnterDemo => state.gate_row = onboard::CHOICES - 1,
            _ => {}
        }
    }
    let Some(id) = act.post() else {
        return;
    };
    if !(matches!(act, Act::Fold(_)) && state.select_post(id, true)) {
        state.select_post(id, false);
    }
}

/// ⏎ on the focus ring; the focus stays unless the selection key or the mode changed.
fn press_focus(state: &mut State, clients: &mut Clients, ui: &mut Ui) -> Result<bool, String> {
    let Some(focus) = ui.focus.take() else {
        return Ok(false);
    };
    let stops = tab_stops(&ui.hits.borrow().targets);
    let Some(target) = focused_index(&stops, &focus).map(|i| stops[i].clone()) else {
        return Ok(false);
    };
    let key = state.selection_key();
    let mode = std::mem::discriminant(&ui.mode);
    let quit = activate(state, clients, ui, &target)?;
    if state.selection_key() == key && std::mem::discriminant(&ui.mode) == mode {
        ui.focus.replace(Some(focus));
    }
    Ok(quit)
}

/// Gives the keys to the body pane in `zone`: the feed closes the thread, the preview opens it.
fn move_keys(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    zone: Zone,
) -> Result<bool, String> {
    let cmd = if zone == Zone::BodyLeft {
        Cmd::Back
    } else {
        Cmd::Expand
    };
    apply(state, clients, ui, cmd)
}

/// Presses `target`. A body target moves the keys to its pane first and selects its post.
fn activate(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    target: &Target,
) -> Result<bool, String> {
    if matches!(target.zone, Zone::BodyLeft | Zone::BodyRight) {
        if target.zone != keyed_zone(state, ui) && move_keys(state, clients, ui, target.zone)? {
            return Ok(true);
        }
        select_for(state, &target.act);
    }
    let cmd = match &target.act {
        Act::Crumb(index) => {
            if *index == 1 && state.settings && state.settings_page == SettingsPage::Accounts {
                state.settings_page = SettingsPage::Main;
                state.settings_row = settings::ROWS
                    .iter()
                    .position(|row| *row == Row::Accounts)
                    .unwrap_or(0);
                return Ok(false);
            }
            if *index > 0 && (state.settings || matches!(ui.mode, Mode::Help)) {
                return Ok(false);
            }
            ui.mode = Mode::Normal;
            state.settings = false;
            if *index >= 3 {
                return Ok(false);
            }
            let section = if *index < 2 {
                Section::Home
            } else {
                state.section
            };
            return apply(state, clients, ui, Cmd::Section(section));
        }
        Act::Back => Cmd::Back,
        Act::Section(Section::Search) => {
            search_key(state, ui, plain_key(KeyCode::Char('f')));
            return Ok(false);
        }
        Act::Section(section) => Cmd::Section(*section),
        Act::Compose => return handle_key(state, clients, ui, plain_key(KeyCode::Char('n'))),
        Act::Scope(scope) => {
            let row = match scope {
                Scope::All => 0,
                Scope::One(net) => 1 + Network::ALL.iter().position(|n| n == net).unwrap_or(0),
            };
            return pick_network(state, clients, ui, row);
        }
        Act::Tab(index) => match open_tab(state, clients, *index) {
            Some(cmd) => cmd,
            None => return Ok(false),
        },
        Act::Input(code, modifiers) => {
            return handle_key(state, clients, ui, KeyEvent::new(*code, *modifiers))
        }
        Act::Key(code) => return handle_key(state, clients, ui, plain_key(*code)),
        Act::Author(_) => {
            if let Err(err) = state.open_author(clients) {
                state.fail(err);
            }
            return Ok(false);
        }
        Act::Thread(_) | Act::Fold(_) => Cmd::Expand,
        Act::More(id) => {
            if !state.opened.insert(id.clone()) {
                state.opened.remove(id);
            }
            return Ok(false);
        }
        Act::ShareProfile(net, handle) => {
            ui.mode = Mode::Share(Box::new(Share::profile(
                *net,
                handle,
                &state.post_targets(),
            )));
            return Ok(false);
        }
        Act::Link { url, .. } | Act::ProfileLink(url) => {
            match util::open_browser(url) {
                Ok(()) => state.confirm(url.clone()),
                Err(err) => state.fail(err),
            }
            return Ok(false);
        }
        Act::Mention { post, handle } => {
            let net = net::split(post).map_or(Network::X, |(net, _)| net);
            if let Err(err) = state.open_user(Scope::One(net), handle.clone(), clients) {
                state.fail(err);
            }
            return Ok(false);
        }
        Act::Hashtag { tag, .. } => Cmd::Search(format!("#{tag}")),
        Act::Quote { quoted, .. } => Cmd::Thread(quoted.clone()),
        Act::Media(_) => Cmd::View { n: None },
        Act::MediaAt { index, .. } => {
            let index = *index;
            let quit = apply(state, clients, ui, Cmd::View { n: None })?;
            if let Mode::Gallery(gallery) = &mut ui.mode {
                gallery.index = index.min(gallery.pictures.len().saturating_sub(1));
            }
            return Ok(quit);
        }
        Act::Reply(_) => return handle_key(state, clients, ui, plain_key(KeyCode::Char('r'))),
        Act::Repost(_) => return handle_key(state, clients, ui, plain_key(KeyCode::Char('t'))),
        Act::Like(_) => return handle_key(state, clients, ui, plain_key(KeyCode::Char('l'))),
        Act::Bookmark(_) => return handle_key(state, clients, ui, plain_key(KeyCode::Char('b'))),
        Act::MyProfile(net) => {
            let Some(handle) = state.handle_of(*net) else {
                state.open_accounts();
                return Ok(false);
            };
            if let Err(err) = state.open_user(Scope::One(*net), handle, clients) {
                state.fail(err);
            } else {
                state.settings = false;
            }
            return Ok(false);
        }
        Act::Accounts => {
            state.open_accounts();
            return Ok(false);
        }
        Act::SignIn(net) => return sign_in(state, clients, ui, *net),
        Act::EnterDemo => {
            state.enter_demo();
            return Ok(false);
        }
        Act::Overflow(items) => {
            if items.is_empty() {
                return Ok(false);
            }
            let anchor_x = usize::from(target.x);
            let anchor_y = usize::from(target.y);
            if let Mode::Overflow(open) = &ui.mode {
                if open.anchor_x == anchor_x && open.anchor_y == anchor_y {
                    ui.mode = Mode::Normal;
                    return Ok(false);
                }
            }
            ui.mode = Mode::Overflow(Overflow {
                items: items.clone(),
                cursor: 0,
                anchor_x,
                anchor_y,
            });
            return Ok(false);
        }
    };
    apply(state, clients, ui, cmd)
}

/// Pointer motion: the welcome row under the cursor, and a dialog's highlighted row.
fn hover(state: &mut State, ui: &mut Ui, mouse: MouseEvent) {
    let (x, y) = (usize::from(mouse.column), usize::from(mouse.row));
    let under: Vec<Pick> = ui
        .hits
        .borrow()
        .rows
        .iter()
        .filter(|(row, cols, _)| *row == y && cols.contains(&x))
        .map(|(_, _, pick)| *pick)
        .collect();
    let pick = under
        .iter()
        .copied()
        .find(|pick| matches!(pick, Pick::Check(_)))
        .or_else(|| under.first().copied());
    if onboard::showing(state) && matches!(ui.mode, Mode::Normal) {
        if let Some(target) = ui.hits.borrow().targets.iter().find(|target| {
            usize::from(target.y) == y
                && x >= usize::from(target.x)
                && x < usize::from(target.x) + usize::from(target.width)
        }) {
            match target.act {
                Act::SignIn(net) => {
                    if let Some(index) = Network::ALL.iter().position(|candidate| *candidate == net)
                    {
                        state.gate_row = index;
                    }
                }
                Act::EnterDemo => state.gate_row = onboard::CHOICES - 1,
                _ => {}
            }
        }
    }
    if let (Some(Pick::Row(i)), false) = (pick, matches!(ui.mode, Mode::Inspect { .. })) {
        set_cursor(state, ui, i);
    }
}

/// Left clicks, the wheel and hover, by where the last frame put things.
pub(crate) fn handle_mouse(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    mouse: MouseEvent,
) -> Result<bool, String> {
    let (x, y) = (usize::from(mouse.column), usize::from(mouse.row));
    let under: Vec<Pick> = ui
        .hits
        .borrow()
        .rows
        .iter()
        .filter(|(row, cols, _)| *row == y && cols.contains(&x))
        .map(|(_, _, pick)| *pick)
        .collect();
    let pick = under
        .iter()
        .copied()
        .find(|pick| matches!(pick, Pick::Check(_)))
        .or_else(|| under.first().copied());
    match mouse.kind {
        MouseEventKind::Moved => {
            hover(state, ui, mouse);
            Ok(false)
        }
        MouseEventKind::ScrollDown => wheel(state, clients, ui, 1, x, y),
        MouseEventKind::ScrollUp => wheel(state, clients, ui, -1, x, y),
        MouseEventKind::Down(MouseButton::Left) => {
            state.clear_error();
            click(state, clients, ui, x, y, pick)
        }
        _ => Ok(false),
    }
}

/// Puts the open dialog's cursor on row `i`; the theme picker previews the scheme there.
fn set_cursor(state: &mut State, ui: &mut Ui, i: usize) {
    match &mut ui.mode {
        Mode::Pick(picker) => {
            if picker.kind == PickKind::Theme && picker.cursor != i {
                if let Some(scheme) = Scheme::list().get(i) {
                    state.scheme = *scheme;
                    state.retheme();
                }
            }
            picker.cursor = i;
        }
        Mode::Share(share) => share.row = i,
        Mode::Filter { cursor, .. } | Mode::Inspect { cursor, .. } => *cursor = i,
        Mode::Command { pick, .. } => *pick = i,
        Mode::Compose(composer) => {
            if let Overlay::List { cursor, .. } = &mut composer.overlay {
                *cursor = i;
            }
        }
        Mode::Overflow(menu) if !menu.items.is_empty() => {
            menu.cursor = i.min(menu.items.len() - 1);
        }
        _ => {}
    }
}

/**
`steps` wheel notches, down when positive: a dialog's cursor, the Settings row, the selection or a
picture moves one item per notch, and the key help or a page without posts three rows.
*/
fn wheel(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    steps: i32,
    x: usize,
    y: usize,
) -> Result<bool, String> {
    let down = steps > 0;
    let arrow = if down { KeyCode::Down } else { KeyCode::Up };
    let scroll = |ui: &Ui, limit: Option<usize>| {
        let next = (ui.scroll.get() as i64 + 3 * i64::from(steps)).max(0) as usize;
        ui.scroll.set(limit.map_or(next, |limit| next.min(limit)));
    };
    let times = steps.unsigned_abs();
    match &ui.mode {
        Mode::Pick(_)
        | Mode::Share(_)
        | Mode::Filter { .. }
        | Mode::Inspect { .. }
        | Mode::Command { .. }
        | Mode::Overflow(_) => return repeat_key(state, clients, ui, arrow, times),
        Mode::Compose(composer) if matches!(composer.overlay, Overlay::List { .. }) => {
            return repeat_key(state, clients, ui, arrow, times);
        }
        Mode::Gallery(_) => {
            let code = if down { KeyCode::Right } else { KeyCode::Left };
            return repeat_key(state, clients, ui, code, times);
        }
        Mode::Help => {
            scroll(ui, Some(ui.overflow.get()));
            return Ok(false);
        }
        Mode::Normal => {}
        _ => return Ok(false),
    }
    if state.settings {
        return repeat_key(state, clients, ui, arrow, times);
    }
    let zone = ui
        .hits
        .borrow()
        .panes
        .iter()
        .find(|pane| pane.cols.contains(&x) && pane.rows.contains(&y))
        .map(|pane| pane.zone);
    let Some(zone) = zone else {
        return Ok(false);
    };
    if state.current_posts().is_empty() {
        scroll(ui, None);
        return Ok(false);
    }
    if zone != keyed_zone(state, ui) && move_keys(state, clients, ui, zone)? {
        return Ok(true);
    }
    repeat_key(state, clients, ui, arrow, times)
}

/// Presses `code` `times` times and stops early when a press quits.
fn repeat_key(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    code: KeyCode,
    times: u32,
) -> Result<bool, String> {
    for _ in 0..times {
        if handle_key(state, clients, ui, plain_key(code))? {
            return Ok(true);
        }
    }
    Ok(false)
}

/**
A left click: in a dialog it chooses the row under it or closes the dialog from outside; elsewhere
it presses the target under it, or selects the post under it and opens it when already selected.
*/
fn click(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    x: usize,
    y: usize,
    pick: Option<Pick>,
) -> Result<bool, String> {
    if matches!(ui.mode, Mode::Compose(_) | Mode::Command { .. }) {
        if let Some(target) =
            target_at(ui, x, y).filter(|target| matches!(target.act, Act::Input(..)))
        {
            return activate(state, clients, ui, &target);
        }
    }
    if matches!(ui.mode, Mode::Normal | Mode::Help) {
        if let Some(target) = target_at(ui, x, y).filter(|target| target.zone == Zone::Header) {
            return activate(state, clients, ui, &target);
        }
    }
    if let Some(Pick::Key(code)) = pick {
        return handle_key(state, clients, ui, plain_key(code));
    }
    let inside = ui
        .hits
        .borrow()
        .dialog
        .as_ref()
        .is_some_and(|(rows, cols)| rows.contains(&y) && cols.contains(&x));
    match &ui.mode {
        Mode::Pick(_)
        | Mode::Share(_)
        | Mode::Filter { .. }
        | Mode::Inspect { .. }
        | Mode::Overflow(_) => {
            return match pick {
                Some(pick) => choose(state, clients, ui, pick),
                None if !inside => handle_key(state, clients, ui, plain_key(KeyCode::Esc)),
                None => Ok(false),
            };
        }
        Mode::Compose(composer) => {
            if matches!(composer.overlay, Overlay::None) {
                return Ok(false);
            }
            if !matches!(composer.overlay, Overlay::List { .. }) {
                return Ok(false);
            }
            return match pick {
                Some(pick) => choose(state, clients, ui, pick),
                None if !inside => handle_key(state, clients, ui, plain_key(KeyCode::Esc)),
                None => Ok(false),
            };
        }
        Mode::Command { input, .. } => {
            let Some(Pick::Row(i)) = pick else {
                if !inside && matches!(state.detail, Detail::Standard | Detail::Rich) {
                    ui.mode = Mode::Normal;
                }
                return Ok(false);
            };
            let typed = input.text();
            let Some(picked) = command_matches(typed).get(i).copied() else {
                return Ok(false);
            };
            let line = if typed.trim().is_empty() {
                completed("", picked.0, picked.1)
            } else {
                command_line(typed, Some(picked))
            };
            ui.mode = Mode::Normal;
            return match command::parse(&line) {
                Ok(cmd) => apply(state, clients, ui, cmd),
                Err(err) => {
                    state.fail(err);
                    Ok(false)
                }
            };
        }
        Mode::Normal => {}
        _ => return Ok(false),
    }
    if state.settings {
        return match settings_click(state, ui, x, y) {
            SettingsClick::Step(row, dir) => {
                if let Some((key, value)) = settings::step(state, row, dir) {
                    save_step(state, &key, &value);
                }
                Ok(false)
            }
            SettingsClick::Press(code) => settings_key(state, clients, ui, plain_key(code)),
            SettingsClick::None => Ok(false),
        };
    }
    if let Some(target) = target_at(ui, x, y) {
        ui.focus.replace(None);
        return activate(state, clients, ui, &target);
    }
    let found = ui.hits.borrow().panes.iter().find_map(|pane| {
        if !(pane.cols.contains(&x) && pane.rows.contains(&y)) {
            return None;
        }
        let line = y - pane.rows.start + pane.offset;
        let row = pane
            .spans
            .iter()
            .position(|span| (span.start..span.end).contains(&line))?;
        Some((pane.zone, row))
    });
    let Some((zone, row)) = found else {
        return Ok(false);
    };
    if zone != keyed_zone(state, ui) {
        if move_keys(state, clients, ui, zone)? {
            return Ok(true);
        }
        state.select_row(row);
        return Ok(false);
    }
    if state.selected_index() == Some(row) {
        return apply(state, clients, ui, Cmd::Expand);
    }
    state.select_row(row);
    Ok(false)
}

/// The target the last frame put at screen cell (`x`, `y`).
fn target_at(ui: &Ui, x: usize, y: usize) -> Option<Target> {
    ui.hits
        .borrow()
        .targets
        .iter()
        .find(|t| {
            let start = usize::from(t.x);
            usize::from(t.y) == y && (start..start + usize::from(t.width)).contains(&x)
        })
        .cloned()
}

/// A click on a dialog row: the cursor goes there and the row is chosen as ⏎ or space would.
fn choose(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    pick: Pick,
) -> Result<bool, String> {
    match pick {
        Pick::Key(code) => handle_key(state, clients, ui, plain_key(code)),
        Pick::Check(j) => {
            if let Mode::Share(share) = &mut ui.mode {
                if let Some(row) = share.rows.iter().position(|a| *a == ShareAction::Quote) {
                    share.row = row;
                }
                share.check = j;
                if let Err(err) = share.toggle(j) {
                    state.fail(err);
                }
            }
            Ok(false)
        }
        Pick::Row(i) => {
            set_cursor(state, ui, i);
            let code = match ui.mode {
                Mode::Filter { .. } => KeyCode::Char(' '),
                _ => KeyCode::Enter,
            };
            handle_key(state, clients, ui, plain_key(code))
        }
    }
}

/// What a click on Settings asks for.
#[derive(Debug, PartialEq, Eq)]
enum SettingsClick {
    None,
    /// `‹` or `›`: step the row's value.
    Step(Row, i32),
    /// The row that has the cursor: the key it answers, ⏎ or space.
    Press(KeyCode),
}

/// Moves the Settings cursor to the row under the click, and says what else the click does.
fn settings_click(state: &mut State, ui: &Ui, x: usize, y: usize) -> SettingsClick {
    let hit = ui
        .hits
        .borrow()
        .settings
        .iter()
        .find(|(row, _)| *row == y)
        .map(|(_, hit)| *hit);
    let Some(hit) = hit else {
        return SettingsClick::None;
    };
    let index = if state.settings_page == SettingsPage::Accounts {
        let Row::Net(net) = hit.row else {
            return SettingsClick::None;
        };
        let Some(index) = Network::ALL.iter().position(|one| *one == net) else {
            return SettingsClick::None;
        };
        index
    } else {
        let Some(index) = settings::ROWS.iter().position(|row| *row == hit.row) else {
            return SettingsClick::None;
        };
        index
    };
    let had = state.settings_row == index;
    state.settings_row = index;
    let col = x.checked_sub(ui.hits.borrow().margin);
    match (hit.arrows, col) {
        (Some((left, _)), Some(col)) if col == left => return SettingsClick::Step(hit.row, -1),
        (Some((_, right)), Some(col)) if col == right => return SettingsClick::Step(hit.row, 1),
        _ => {}
    }
    match (had, state.settings_page, hit.row) {
        (false, _, _) => SettingsClick::None,
        (true, SettingsPage::Accounts, _) => SettingsClick::Press(KeyCode::Enter),
        (true, SettingsPage::Main, Row::Net(net)) => {
            if col.is_some_and(|c| settings::on_account_action(state, net, c)) {
                SettingsClick::Press(KeyCode::Enter)
            } else {
                SettingsClick::Press(KeyCode::Char(' '))
            }
        }
        (true, SettingsPage::Main, _) => SettingsClick::Press(KeyCode::Enter),
    }
}

// MARK: drawing

// MARK: split

/// The feed and the selected post's thread side by side, built before drawing because the thread
/// side borrows `thread_focus`.
struct Panes {
    /// Content columns of the feed pane.
    left: usize,
    /// Content columns of the thread pane.
    right: usize,
    thread_bar: StyledLine,
    feed: Body,
    thread: Body,
    feed_key: String,
    thread_key: String,
    /// A thread is open, so the thread pane has the keys.
    thread_focused: bool,
}

/// Whether a frame `full` columns wide draws two panes: the layout allows it, the mode and the
/// section have a feed, and neither Settings nor a profile is open.
fn splits(state: &State, ui: &Ui, full: usize) -> bool {
    let wide = match state.layout {
        Layout::Auto => full >= SPLIT_AUTO,
        Layout::Split => full >= SPLIT_MIN,
        Layout::Single => false,
    };
    wide && !onboard::showing(state)
        && !state.settings
        && state.profile().is_none()
        && matches!(
            ui.mode,
            Mode::Normal
                | Mode::Search(_)
                | Mode::Command { .. }
                | Mode::Filter { .. }
                | Mode::Share(_)
                | Mode::Pick(_)
                | Mode::Compose(_)
                | Mode::Overflow(_)
        )
        && matches!(
            state.section,
            Section::Home | Section::Notifications | Section::Search
        )
}

/**
The two panes for a frame `full` columns wide, or `None` when it doesn't split. The thread pane
shows the open thread, or the selected post's conversation once that thread is loaded.
`thread_focus` is back to what it was when this returns.
*/
fn panes(state: &mut State, ui: &Ui, full: usize) -> Option<Panes> {
    let split = splits(state, ui, full);
    state.set_overview(split);
    if !state.past_prefetch() {
        state.page_gate = None;
    }
    state.show_page_card =
        matches!(ui.mode, Mode::Normal) && !state.settings && (state.page_due() || state.paging);
    if !split {
        return None;
    }
    let width = render::inner_width_for(full, state.detail);
    let left = ((width - 4) * 3 / 8).clamp(FEED_MIN, FEED_MAX);
    let right = width - 4 - left;
    let open = state.thread_focus.take();
    let feed = screens::linked_body(state, left, true);
    let feed_key = state.selection_key();
    // The feed draw cleared `thread_focus`, so this id is the list's post, not a reply in the thread.
    if state.demo {
        if let Some(id) = state.selected_post_id() {
            state.ensure_demo_thread(&id);
        }
    }
    let thread_focused = open.is_some();
    let shown = open.clone().or_else(|| {
        state
            .selected_post()
            .map(|post| post.conversation_id.clone())
    });
    state.thread_focus = shown.clone();
    let thread = match &shown {
        Some(_) => screens::linked_body(state, right, thread_focused),
        None => Body::text(vec![StyledLine::text(
            Kind::Dim,
            "Select a post to read it here",
        )]),
    };
    let thread_key = match &shown {
        Some(_) if thread_focused => state.selection_key(),
        Some(id) => format!("preview:{id}"),
        None => String::new(),
    };
    let thread_bar = chrome::thread_bar(state, right, thread_focused);
    state.thread_focus = open;
    Some(Panes {
        left,
        right,
        thread_bar,
        feed,
        thread,
        feed_key,
        thread_key,
        thread_focused,
    })
}

/// One side of the body: its lines, where its area starts and how wide it is, and its scroll.
struct Side<'a> {
    body: &'a Body,
    /// First screen column of the side's area; the content starts a margin later.
    x: usize,
    /// Columns of the area, margins included.
    area: usize,
    focused: bool,
    /// The padded document's slice that this frame shows.
    window: Window,
    /// A row drawn above the posts, after any top pad still on screen. The thread pane uses it
    /// for the reply order.
    head: Option<&'a StyledLine>,
}

/// Pad blanks and body lines inside one viewport.
struct Window {
    /// Blank rows of the top pad that are still on screen.
    top: usize,
    /// Body lines on screen, as indexes into [`Body::lines`].
    lines: Range<usize>,
    /// Blank rows of the bottom pad that are on screen.
    bottom: usize,
}

/**
The slice of a document that is `pad` blanks, then `lines` body rows, then `pad` blanks, inside a
window of `rows`. A short document does not grow extra blanks to fill the window.
*/
fn pad_window(offset: usize, pad: usize, lines: usize, rows: usize) -> Window {
    let total = pad.saturating_add(lines).saturating_add(pad);
    let offset = offset.min(total.saturating_sub(rows));
    let top = pad.saturating_sub(offset).min(rows);
    let line_from = offset.saturating_sub(pad).min(lines);
    let line_to = (line_from + rows.saturating_sub(top)).min(lines);
    let shown = line_to - line_from;
    let win_end = offset.saturating_add(rows).min(total);
    let bottom = win_end
        .saturating_sub((pad + lines).max(offset))
        .min(pad)
        .min(rows.saturating_sub(top + shown));
    Window {
        top,
        lines: line_from..line_to,
        bottom,
    }
}

/// Screen row where this side's body lines start, and those lines.
fn content_rows(side: &Side<'_>, body_top: usize) -> (usize, Range<usize>) {
    let y = body_top + side.window.top + usize::from(side.head.is_some());
    (y, side.window.lines.clone())
}

/**
Rows of breathing room that scroll with a list. One row at every density, including compact.
Settings, help and a profile already start under the header blank, so they stay flush.
*/
fn reader_pad(state: &State, ui: &Ui) -> usize {
    if state.settings || matches!(ui.mode, Mode::Help) || state.profile().is_some() {
        0
    } else {
        state.density.rows(1, 1, 0).max(1)
    }
}

/// What a detail, density, layout or resize change alters: the frame's levels and its size.
type Look = (Detail, Density, Layout, usize, usize);

/**
Where the selected block sat on the frame drawn with `look`: `start` is its first body row and
`offset` the first body row on screen. `row` is the screen row it should keep, counted from the
body's top; it outlives a frame that had to clamp the block elsewhere until the reader scrolls or
the block moves.
*/
#[derive(Clone, Debug, PartialEq, Eq)]
struct Anchor {
    look: Look,
    key: String,
    id: Option<String>,
    start: usize,
    offset: usize,
    row: usize,
}

/// The last frame's anchor, this frame's look and its selected post id, for [`scrolled`].
type Anchoring<'a> = (&'a RefCell<Option<Anchor>>, Look, Option<String>);

/// The look of a frame `full` columns wide and `height` rows tall.
fn look_of(state: &State, full: usize, height: usize) -> Look {
    (state.detail, state.density, state.layout, full, height)
}

/**
First document row for `body` on the screen called `key`: the top on a new screen, then kept around
the focused block. `pad` blanks sit before and after the body lines, and the focus is shifted by
that many rows so anchors stay in the same document. With an `anchor`, the first frame after a
change of look puts the selected block back on the row the last frame drew it on, clamped to the
body, and records where it lands.
*/
fn scrolled(
    scroll: &Cell<usize>,
    scroll_key: &RefCell<String>,
    key: &str,
    body: &Body,
    height: usize,
    pad: usize,
    anchor: Option<Anchoring>,
) -> usize {
    if *scroll_key.borrow() != key {
        scroll.set(0);
        scroll_key.replace(key.to_string());
    }
    let total = pad + body.lines.len() + pad;
    let focus = body.focus.map(|span| RowSpan {
        start: span.start + pad,
        end: span.end + pad,
    });
    let is_blank = |doc: usize| {
        doc < pad
            || doc >= pad + body.lines.len()
            || body.lines[doc - pad].plain().trim().is_empty()
    };
    let Some((last, look, id)) = anchor else {
        let offset = follow_past_blanks(scroll.get(), focus, height, total, is_blank);
        scroll.set(offset);
        return offset;
    };
    let same = last
        .borrow()
        .clone()
        .filter(|last| last.key == key && last.id == id);
    let kept = same
        .as_ref()
        .filter(|last| last.look != look)
        .zip(focus)
        .map(|(last, span)| {
            let len = span.end.saturating_sub(span.start).max(1);
            let row = last.row.min(height.saturating_sub(len));
            span.start
                .saturating_sub(row)
                .min(total.saturating_sub(height))
        });
    let offset =
        kept.unwrap_or_else(|| follow_past_blanks(scroll.get(), focus, height, total, is_blank));
    scroll.set(offset);
    last.replace(focus.map(|span| {
        let still = same
            .as_ref()
            .filter(|last| kept.is_some() || (last.offset == offset && last.start == span.start));
        Anchor {
            look,
            key: key.to_string(),
            id,
            start: span.start,
            offset,
            row: still.map_or(span.start.saturating_sub(offset), |last| last.row),
        }
    }));
    offset
}

/// A rule across both panes with `joint` where the divider crosses it.
fn split_rule(width: usize, left: usize, joint: char) -> StyledLine {
    let text: String = (0..width)
        .map(|col| if col == left + 1 { joint } else { '─' })
        .collect();
    StyledLine::text(Kind::Line, &text)
}

/// Hands the open thread's scroll and anchor to the pane that draws it once the split turns on or off.
fn carry(ui: &Ui, into_split: bool) {
    let main = (&ui.scroll, &ui.scroll_key, &ui.anchor);
    let right = (&ui.scroll_right, &ui.scroll_right_key, &ui.anchor_right);
    let (from, to) = if into_split {
        (main, right)
    } else {
        (right, main)
    };
    to.0.set(from.0.get());
    to.1.replace(from.1.borrow().clone());
    to.2.replace(from.2.borrow().clone());
}

fn draw(frame: &mut Frame, state: &State, ui: &Ui, panes: Option<&Panes>) {
    #[cfg(not(feature = "native"))]
    if let Some(previews) = ui.previews.borrow_mut().as_mut() {
        previews.clear_places();
    }
    let area = frame.area();
    frame.render_widget(
        Block::default().style(render::row_style(&state.theme, false)),
        area,
    );
    let full = area.width as usize;
    let margin = render::margin_for(full, state.detail);
    let narrow = render::narrow(full);
    let width = render::inner_width_for(full, state.detail);
    let height = area.height as usize;
    let mut screen = Screen {
        frame,
        theme: &state.theme,
        full,
        margin,
        height,
        simple: matches!(state.detail, Detail::Plain | Detail::Minimal),
    };
    ui.hits.replace(Hits::default());
    if onboard::showing(state) && matches!(ui.mode, Mode::Normal) && !too_small(full, height) {
        screen.margin = 0;
        let frame = u32::try_from(ui.started.elapsed().as_millis() / 33).unwrap_or(u32::MAX);
        let body = onboard::animated_body(state, full, height, frame, &mut ui.welcome.borrow_mut());
        for (y, line) in body.lines.iter().enumerate() {
            screen.line(y, line);
        }
        if !state.status.is_empty() {
            screen.line(
                1,
                &settings::status_line(&state.status, Icons::new(state.icons).check, full),
            );
        }
        let targets = place(body.targets, 0, Zone::BodyLeft, 0);
        if let Some(focus) = ui.focus.borrow().as_ref() {
            if let Some(target) = targets.iter().find(|target| target.act == focus.act) {
                highlight(screen.frame.buffer_mut(), target, &state.theme, &[]);
            }
        }
        ui.viewport.set((full, height));
        ui.split.set(false);
        ui.scroll.set(0);
        ui.hits.replace(Hits {
            targets,
            ..Hits::default()
        });
        return;
    }
    if too_small(full, height) {
        draw_too_small(&mut screen);
        return;
    }

    if let Mode::SignIn(form) = &ui.mode {
        draw_sign_in(&mut screen, ui, form);
        return;
    }

    if let Mode::Ask { input, asked } = &ui.mode {
        draw_ask(&mut screen, state, ui, input, asked, width);
        return;
    }

    if let Mode::Gallery(gallery) = &ui.mode {
        draw_gallery(&mut screen, state, ui, gallery, width);
        return;
    }

    let typing = match &ui.mode {
        Mode::Search(input) => Some(input),
        _ => None,
    };
    let (crumb, (subbar, subbar_targets)) = match &ui.mode {
        Mode::Inspect { id, handle, .. } => (
            format!("{} › json", state.crumb()),
            (chrome::json_subbar(handle, id, width), Vec::new()),
        ),
        Mode::Help => ("keys".into(), (StyledLine::blank(), Vec::new())),
        _ => (
            state.crumb(),
            chrome::subbar_targeted(
                state,
                width,
                typing.map(|i| i.text()),
                render::narrow(full),
                panes.is_some(),
            ),
        ),
    };
    let (header, header_targets) = chrome::header_targeted(state, &crumb, width);
    let mut targets = place(header_targets, 1, Zone::Header, margin);
    let mut top = vec![header, StyledLine::blank()];
    if !state.settings && !matches!(ui.mode, Mode::Help) && state.profile().is_none() {
        top.extend([subbar, chrome::rule(width)]);
        targets.extend(place(subbar_targets, 3, Zone::Subbar, margin));
    }
    for (i, line) in top.iter().enumerate() {
        screen.line(1 + i, line);
    }
    if let Some(p) = panes {
        // Tabs stay on this full-width row. A feed-pane bar would leave them short of the right edge.
        screen.line(RULE_ROW, &split_rule(width, p.left, '┬'));
    }
    if let Some(input) = typing {
        let x = margin + "Search ❯ ".width() + line_width(input.before_cursor());
        screen.cursor(x, 3);
    }

    let focused_act = ui.focus.borrow().as_ref().map(|focus| focus.act.clone());
    let hint_post = focused_act
        .as_ref()
        .and_then(Act::post)
        .and_then(|id| state.posts.get(id))
        .or_else(|| state.selected_post());
    let simple = matches!(state.detail, Detail::Plain | Detail::Minimal);
    let bottom = match &ui.mode {
        Mode::Compose(c) => composer(state, c, width, ui.enhanced, None),
        Mode::Help => Bottom::default(),
        Mode::Command { input, .. } if simple => {
            let mut canvas = Canvas::new(width);
            let y = canvas.push(false);
            let x = canvas.put(y, 0, Kind::Bold, ":");
            canvas.put(y, x, Kind::Fg, input.text());
            let enter = theme::ascii_key("⏎", state.theme.ascii);
            let actions = [
                ("tab", "complete", KeyCode::Tab),
                (enter, "run", KeyCode::Enter),
                ("esc", "close", KeyCode::Esc),
            ];
            let labels: Vec<_> = actions
                .iter()
                .map(|(key, label, code)| {
                    (
                        if state.reveal_shortcuts {
                            format!("{key} {label}")
                        } else {
                            label.to_string()
                        },
                        *code,
                    )
                })
                .collect();
            let total = labels.iter().map(|(text, _)| text.width()).sum::<usize>() + 4;
            let mut start = width.saturating_sub(total);
            let input_end = 1 + line_width(input.text());
            for (text, code) in labels {
                if start <= input_end {
                    start += text.width() + 2;
                    continue;
                }
                let end = canvas.put(y, start, Kind::Dim, &text);
                canvas.mouse_target(y, start, end, Act::Input(code, KeyModifiers::empty()));
                start = end + 2;
            }
            let mut lines = vec![chrome::rule(width)];
            let mut targets = Vec::new();
            if !narrow {
                let (nav, nav_targets) = chrome::nav_targeted(state, width);
                lines.push(nav);
                targets = below(nav_targets, 1);
            }
            let row = lines.len();
            let (input_lines, _, input_targets) = canvas.finish_targeted();
            targets.extend(below(input_targets, row as u16));
            lines.extend(input_lines);
            Bottom {
                lines,
                cursor: Some((1 + line_width(input.before_cursor()), row)),
                targets,
                ..Bottom::default()
            }
        }
        Mode::Normal if state.settings => {
            let (hints, hint_targets) =
                chrome::context_hints(state, Hints::of(state), None, focused_act.as_ref(), width);
            Bottom {
                lines: vec![
                    settings::status_line(&state.status, Icons::new(state.icons).check, width),
                    chrome::rule(width),
                    hints,
                ],
                targets: below(hint_targets, 2),
                ..Bottom::default()
            }
        }
        _ => {
            let set = match ui.mode {
                Mode::Search(_) => Hints::Search,
                Mode::Inspect { .. } => Hints::Inspect,
                _ => Hints::of(state),
            };
            let mut lines = vec![chrome::rule(width)];
            let mut targets = Vec::new();
            if !narrow {
                let (nav, nav_targets) = chrome::nav_targeted(state, width);
                lines.push(nav);
                targets = below(nav_targets, 1);
            }
            let (hints, hint_targets) =
                chrome::context_hints(state, set, hint_post, focused_act.as_ref(), width);
            targets.extend(below(hint_targets, lines.len() as u16));
            lines.push(hints);
            Bottom {
                lines,
                targets,
                ..Bottom::default()
            }
        }
    };
    let body_top = 1 + top.len();
    let body_end = height.saturating_sub(bottom.lines.len()).max(body_top);
    let body_height = body_end - body_top;
    let focused_width = match panes {
        Some(p) if p.thread_focused => p.right,
        Some(p) => p.left,
        None => width,
    };
    // The thread pane spends its first row on the reply order.
    let focused_height = match panes {
        Some(p) if p.thread_focused => body_height.saturating_sub(1),
        _ => body_height,
    };
    ui.viewport.set((focused_width, focused_height));
    if ui.split.get() != panes.is_some() && state.thread_focus.is_some() {
        carry(ui, panes.is_some());
    }
    ui.split.set(panes.is_some());
    for (i, line) in bottom.lines.iter().enumerate() {
        screen.line(body_end + i, line);
    }
    targets.extend(place(bottom.targets, body_end, Zone::Footer, margin));
    if let Some(p) = panes {
        if !matches!(ui.mode, Mode::Compose(_)) {
            screen.line(body_end, &split_rule(width, p.left, '┴'));
        }
    }
    if let Some((x, y)) = bottom.cursor {
        screen.cursor(margin + x, body_end + y);
    }

    let pad = reader_pad(state, ui);
    let single;
    let mut settings_hits = Vec::new();
    let sides: Vec<Side> = match panes {
        Some(p) => {
            let thread_x = margin + p.left + 2;
            let mut feed_offset = scrolled(
                &ui.scroll,
                &ui.scroll_key,
                &p.feed_key,
                &p.feed,
                body_height,
                pad,
                Some((
                    &ui.anchor,
                    look_of(state, full, height),
                    state.selected_post_id(),
                )),
            );
            if state.show_page_card {
                let total = pad + p.feed.lines.len() + pad;
                feed_offset = total.saturating_sub(body_height);
                ui.scroll.set(feed_offset);
            }
            let thread_rows = body_height.saturating_sub(1);
            let thread_offset = scrolled(
                &ui.scroll_right,
                &ui.scroll_right_key,
                &p.thread_key,
                &p.thread,
                thread_rows,
                pad,
                p.thread_focused.then(|| {
                    (
                        &ui.anchor_right,
                        look_of(state, full, height),
                        state.selected_post_id(),
                    )
                }),
            );
            vec![
                Side {
                    body: &p.feed,
                    x: 0,
                    area: thread_x - 1,
                    focused: !p.thread_focused,
                    window: pad_window(feed_offset, pad, p.feed.lines.len(), body_height),
                    head: None,
                },
                Side {
                    body: &p.thread,
                    x: thread_x,
                    area: full - thread_x,
                    focused: p.thread_focused,
                    window: pad_window(thread_offset, pad, p.thread.lines.len(), thread_rows),
                    head: Some(&p.thread_bar),
                },
            ]
        }
        None => {
            single = match &ui.mode {
                Mode::Help => {
                    let lines = screens::key_help_lines(width, state.theme.ascii);
                    ui.overflow.set(lines.len().saturating_sub(body_height));
                    Body::text(lines)
                }
                Mode::Inspect {
                    value,
                    open,
                    cursor,
                    ..
                } => {
                    let lines = inspect_lines(value, open, *cursor);
                    let start = (*cursor).min(lines.len().saturating_sub(1));
                    let focus = RowSpan {
                        start,
                        end: start + 1,
                    };
                    Body {
                        focus: Some(focus),
                        ..Body::text(lines)
                    }
                }
                _ if state.settings => {
                    let (lines, focus, hits) = settings::lines(state, width);
                    settings_hits = hits;
                    Body {
                        focus,
                        ..Body::text(lines)
                    }
                }
                _ => screens::linked_body_sized(state, width, true, Some(body_height)),
            };
            let key = match &ui.mode {
                Mode::Help => "help".to_string(),
                Mode::Inspect { id, .. } => format!("json:{id}"),
                _ if state.settings => match state.settings_page {
                    SettingsPage::Accounts => "settings:accounts".to_string(),
                    SettingsPage::Main => "settings".to_string(),
                },
                _ => state.selection_key(),
            };
            let mut offset = scrolled(
                &ui.scroll,
                &ui.scroll_key,
                &key,
                &single,
                body_height,
                pad,
                Some((
                    &ui.anchor,
                    look_of(state, full, height),
                    state.selected_post_id(),
                )),
            );
            if state.show_page_card {
                let total = pad + single.lines.len() + pad;
                offset = total.saturating_sub(body_height);
                ui.scroll.set(offset);
            }
            vec![Side {
                body: &single,
                x: 0,
                area: full,
                focused: true,
                window: pad_window(offset, pad, single.lines.len(), body_height),
                head: None,
            }]
        }
    };
    ui.pages.replace(
        sides
            .iter()
            .flat_map(|side| side.body.unfurls.iter().cloned())
            .collect(),
    );
    let chrome_body = matches!(ui.mode, Mode::Help) || state.settings;
    let blank = StyledLine::blank();
    for side in &sides {
        let mut y = body_top;
        for _ in 0..side.window.top {
            screen.pane(side.x, y, side.area, &blank, side.focused, chrome_body);
            y += 1;
        }
        if let Some(head) = side.head {
            screen.pane(side.x, y, side.area, head, side.focused, true);
            y += 1;
        }
        for line in side
            .body
            .lines
            .get(side.window.lines.clone())
            .unwrap_or(&[])
        {
            screen.pane(side.x, y, side.area, line, side.focused, chrome_body);
            y += 1;
        }
        for i in 0..side.window.bottom {
            screen.pane(side.x, y + i, side.area, &blank, side.focused, chrome_body);
        }
    }
    if let Some(p) = panes {
        for y in RULE_ROW + 1..body_end {
            screen.glyph(margin + p.left + 1, y, '│', Kind::Line);
        }
    }
    let mut hits = Hits {
        margin,
        ..Hits::default()
    };
    for (i, side) in sides.iter().enumerate() {
        let zone = if panes.is_some() && i == 1 {
            Zone::BodyRight
        } else {
            Zone::BodyLeft
        };
        let (content_y, lines) = content_rows(side, body_top);
        let lead = side.x + margin;
        let end = side.x + side.area;
        targets.extend(side.body.targets.iter().filter_map(|target| {
            let row = usize::from(target.y).checked_sub(lines.start)?;
            let x = usize::from(target.x) + lead;
            let fits = row < lines.len() && x + usize::from(target.width) <= end;
            fits.then_some(Target {
                x: u16::try_from(x).ok()?,
                y: u16::try_from(content_y + row).ok()?,
                zone,
                ..target.clone()
            })
        }));
        hits.panes.push(Pane {
            zone,
            cols: side.x..end,
            rows: content_y..body_end,
            offset: lines.start,
            spans: side.body.rows.clone(),
        });
    }
    if let Some(side) = sides.first().filter(|_| state.settings) {
        let (content_y, lines) = content_rows(side, body_top);
        hits.settings = settings_hits
            .into_iter()
            .filter_map(|hit| {
                let row = hit.line.checked_sub(lines.start)?;
                (row < lines.len()).then_some((content_y + row, hit))
            })
            .collect();
    }
    if let Mode::Inspect { .. } = &ui.mode {
        hits.dialog = Some((body_top..body_end, 0..full));
        if let Some(side) = sides.first() {
            let (content_y, lines) = content_rows(side, body_top);
            for (row, line) in lines.enumerate() {
                hits.rows.push((content_y + row, 0..full, Pick::Row(line)));
            }
        }
    }
    if let Mode::Compose(_) = &ui.mode {
        hits.dialog = bottom.dialog.map(|(lines, box_cols)| {
            (
                body_end + lines.start..body_end + lines.end,
                margin + box_cols.start..margin + box_cols.end,
            )
        });
        hits.rows = bottom
            .rows
            .into_iter()
            .map(|(line, box_cols, i)| {
                (
                    body_end + line,
                    margin + box_cols.start..margin + box_cols.end,
                    Pick::Row(i),
                )
            })
            .collect();
    }
    let (mut rows, mut cols) = (0..0, 0..full);
    let mut dialog_links = Vec::new();
    if let Mode::Filter { on, cursor } = &ui.mode {
        let mut panel = settings::panel(state, on, *cursor, width);
        let panel_width = settings::panel_width(width);
        let hint_row = panel.len().saturating_sub(2);
        let (hint, hint_hits) = dialog_footer(
            panel_width,
            state.reveal_shortcuts,
            &[
                ("space", "toggle", KeyCode::Char(' ')),
                ("a", "all", KeyCode::Char('a')),
                ("⏎", "save", KeyCode::Enter),
                ("esc", "close", KeyCode::Esc),
            ],
            "",
        );
        let mut hint_canvas = Canvas::new(width);
        let y = hint_canvas.push(false);
        let mut x = settings::PANEL_COL;
        for span in hint.spans {
            x = hint_canvas.put(y, x, span.kind, &span.text);
        }
        panel[hint_row] = hint_canvas.finish_line();
        rows = RULE_ROW..RULE_ROW + panel.len();
        for (i, line) in panel.iter().enumerate() {
            screen.overlay(margin, RULE_ROW + i, width, line);
        }
        let left = margin + settings::PANEL_COL;
        let outer = settings::panel_width(width);
        hits.dialog = Some((rows.clone(), left..left + outer));
        for (range, key) in hint_hits {
            hits.rows.push((
                RULE_ROW + hint_row,
                left + range.start..left + range.end,
                Pick::Key(key),
            ));
        }
        for i in 0..Network::ALL.len() {
            let cells = left + 1..(left + outer).saturating_sub(1);
            hits.rows.push((RULE_ROW + 1 + i, cells, Pick::Row(i)));
        }
    }
    if let Mode::Share(share) = &ui.mode {
        let (mut dialog, regions, share_hits) =
            screens::share_lines(share, full, state.theme.ascii, state.reveal_shortcuts);
        let wide = screens::share_width(full);
        let hint_row = dialog.len().saturating_sub(2);
        let mut actions = vec![("⏎", "choose", KeyCode::Enter)];
        if share.boxes() {
            actions.push(("space", "toggle", KeyCode::Char(' ')));
        }
        actions.push(("esc", "close", KeyCode::Esc));
        let (hint, hint_hits) = dialog_footer(wide, state.reveal_shortcuts, &actions, "");
        dialog[hint_row] = hint;
        let (left, top) = ((full - wide) / 2, height.saturating_sub(dialog.len()) / 2);
        (rows, cols) = (top..top + dialog.len(), left..left + wide);
        for (i, line) in dialog.iter().enumerate() {
            screen.overlay(left, top + i, wide, line);
        }
        hits.dialog = Some((rows.clone(), cols.clone()));
        for (range, key) in hint_hits {
            hits.rows.push((
                top + hint_row,
                left + range.start..left + range.end,
                Pick::Key(key),
            ));
        }
        for (line, check_cols, j) in share_hits.checks {
            let cells = left + check_cols.start..left + check_cols.end;
            hits.rows.push((top + line, cells, Pick::Check(j)));
        }
        for (line, i) in share_hits.rows {
            hits.rows
                .push((top + line, left + 1..left + wide - 1, Pick::Row(i)));
        }
        dialog_links = regions
            .into_iter()
            .filter_map(|region| {
                Some(Region {
                    x: region.x.checked_add(u16::try_from(left).ok()?)?,
                    y: region.y.checked_add(u16::try_from(top).ok()?)?,
                    ..region
                })
            })
            .collect();
    }
    if let Mode::Pick(picker) = &ui.mode {
        let mut pick_rows = picker.kind.rows(state);
        if !state.reveal_shortcuts {
            for row in &mut pick_rows {
                row.key = ' ';
            }
        }
        let mut dialog = screens::pick_lines(
            picker.kind.title(),
            &pick_rows,
            picker.cursor,
            full,
            height,
            state.theme.ascii,
        );
        let (first, visible) = screens::pick_window(pick_rows.len(), picker.cursor, height);
        let suffix = if visible < pick_rows.len() {
            format!("{}/{}", picker.cursor + 1, pick_rows.len())
        } else {
            String::new()
        };
        let actions = [
            ("⏎", "choose", KeyCode::Enter),
            ("esc", "close", KeyCode::Esc),
        ];
        let hint_row = dialog.len().saturating_sub(2);
        let (hint, hint_hits) = dialog_footer(
            screens::share_width(full),
            state.reveal_shortcuts,
            &actions,
            &suffix,
        );
        dialog[hint_row] = hint;
        let wide = screens::share_width(full);
        let (left, top) = ((full - wide) / 2, height.saturating_sub(dialog.len()) / 2);
        (rows, cols) = (top..top + dialog.len(), left..left + wide);
        for (i, line) in dialog.iter().enumerate() {
            screen.overlay(left, top + i, wide, line);
        }
        hits.dialog = Some((rows.clone(), cols.clone()));
        for k in 0..visible {
            hits.rows
                .push((top + 2 + k, left + 1..left + wide - 1, Pick::Row(first + k)));
        }
        for (range, key) in hint_hits {
            hits.rows.push((
                top + hint_row,
                left + range.start..left + range.end,
                Pick::Key(key),
            ));
        }
    }
    if let Mode::Command { input, pick } = &ui.mode {
        if !simple {
            let wide = width.min(76);
            let (mut dialog, cursor, first, visible) = command_dialog(input, *pick, wide, height);
            let hint_row = dialog.len().saturating_sub(2);
            let (hint, hint_hits) = dialog_footer(
                wide,
                state.reveal_shortcuts,
                &[
                    ("tab", "complete", KeyCode::Tab),
                    ("⏎", "run", KeyCode::Enter),
                    ("esc", "close", KeyCode::Esc),
                ],
                "",
            );
            dialog[hint_row] = hint;
            let left = (full - wide) / 2;
            let top = height.saturating_sub(dialog.len()) / 2;
            rows = top..top + dialog.len();
            cols = left..left + wide;
            for (i, line) in dialog.iter().enumerate() {
                screen.overlay(left, top + i, wide, line);
            }
            screen.cursor(left + cursor, top + 1);
            hits.dialog = Some((rows.clone(), cols.clone()));
            for i in 0..visible {
                hits.rows
                    .push((top + 3 + i, left + 1..left + wide - 1, Pick::Row(first + i)));
            }
            for (range, key) in hint_hits {
                hits.rows.push((
                    top + hint_row,
                    left + range.start..left + range.end,
                    Pick::Key(key),
                ));
            }
        } else {
            let matches = command_matches(input.text());
            let visible = matches.len().min(body_end.saturating_sub(body_top + 2));
            if visible > 0 {
                let first = pick.saturating_sub(visible.saturating_sub(1));
                let shown = &matches[first..first + visible];
                let wide = width.min(72);
                let list = command_list(shown, pick.saturating_sub(first), wide);
                let top = body_end.saturating_sub(list.len());
                let clear = top.saturating_sub(1).max(body_top);
                (rows, cols) = (clear..body_end, 0..full);
                for y in clear..top {
                    screen.overlay(0, y, full, &StyledLine::blank());
                }
                for (i, line) in list.iter().enumerate() {
                    screen.overlay(0, top + i, full, &StyledLine::blank());
                    screen.overlay(margin, top + i, wide, line);
                }
                for i in 0..visible {
                    hits.rows.push((
                        top + 1 + i,
                        margin + 1..margin + wide - 1,
                        Pick::Row(first + i),
                    ));
                }
            }
        }
    }
    if let Mode::Overflow(menu) = &ui.mode {
        if let Some((lines, left, top, wide, first)) = overflow_popup(menu, full, height) {
            (rows, cols) = (top..top + lines.len(), left..left + wide);
            for (i, line) in lines.iter().enumerate() {
                screen.overlay(left, top + i, wide, line);
            }
            hits.dialog = Some((rows.clone(), cols.clone()));
            let inner = (left + 1)..(left + wide).saturating_sub(1);
            for i in 0..lines.len().saturating_sub(2) {
                hits.rows
                    .push((top + 1 + i, inner.clone(), Pick::Row(first + i)));
            }
        }
    }
    if let Some(previews) = ui.previews.borrow_mut().as_mut() {
        let Rgb(r, g, b) = state.theme.row_bg(false);
        previews.set_backdrop([r, g, b]);
        for side in &sides {
            // Pictures sit on the body lines, under the pad and the sticky head.
            let (origin_y, lines) = content_rows(side, body_top);
            paint_previews(
                &mut screen,
                previews,
                &side.body.previews,
                side.x + margin,
                lines.start,
                origin_y,
                lines.len(),
                &rows,
                &cols,
                spin_frame(ui.started.elapsed()),
            );
        }
    }
    if ui.links {
        let mut regions: Vec<Region> = Vec::new();
        for side in &sides {
            let lead = side.x + margin;
            let end = side.x + side.area;
            let (origin_y, lines) = content_rows(side, body_top);
            regions.extend(side.body.links.iter().filter_map(|region| {
                let row = usize::from(region.y).checked_sub(lines.start)?;
                let y = origin_y + row;
                let x = usize::from(region.x) + lead;
                let right = x + usize::from(region.width);
                let covered = rows.contains(&y) && x < cols.end && right > cols.start;
                (row < lines.len() && !covered && right <= end).then_some(Region {
                    x: u16::try_from(x).ok()?,
                    y: u16::try_from(y).ok()?,
                    ..region.clone()
                })
            }));
        }
        regions.extend(dialog_links);
        hyperlink::paint(screen.frame.buffer_mut(), &regions);
    }
    targets.retain(|target| {
        let (x, y) = (usize::from(target.x), usize::from(target.y));
        let right = x + usize::from(target.width);
        let covered = rows.contains(&y) && x < cols.end && right > cols.start;
        !covered && y < height && right <= full
    });
    if state.reveal_shortcuts {
        for target in &targets {
            underline(screen.frame.buffer_mut(), target);
        }
        for (y, _) in &hits.settings {
            underline(
                screen.frame.buffer_mut(),
                &Target {
                    x: margin as u16,
                    y: *y as u16,
                    width: width as u16,
                    act: Act::Key(KeyCode::Enter),
                    zone: Zone::BodyLeft,
                    tab: false,
                },
            );
        }
        for (y, cols, _) in &hits.rows {
            underline(
                screen.frame.buffer_mut(),
                &Target {
                    x: cols.start as u16,
                    y: *y as u16,
                    width: cols.len() as u16,
                    act: Act::Key(KeyCode::Enter),
                    zone: Zone::Footer,
                    tab: false,
                },
            );
        }
    }
    let picture_cells: Vec<Rect> = sides
        .iter()
        .flat_map(|side| {
            let (origin_y, lines) = content_rows(side, body_top);
            let room = lines.len();
            let offset = lines.start;
            let origin = side.x + margin;
            side.body.previews.iter().filter_map(move |slot| {
                let start = slot.y.saturating_sub(offset);
                let end = slot
                    .y
                    .saturating_add(slot.rows)
                    .saturating_sub(offset)
                    .min(room);
                if start >= end {
                    return None;
                }
                Some(Rect::new(
                    (origin + slot.x) as u16,
                    (origin_y + start) as u16,
                    slot.cols as u16,
                    (end - start) as u16,
                ))
            })
        })
        .collect();
    let stops = tab_stops(&targets);
    let lit = ui
        .focus
        .borrow()
        .as_ref()
        .and_then(|focus| focused_index(&stops, focus));
    match lit {
        Some(i) if matches!(ui.mode, Mode::Normal) => {
            highlight(
                screen.frame.buffer_mut(),
                &stops[i],
                &state.theme,
                &picture_cells,
            );
        }
        Some(_) => {}
        None => {
            ui.focus.replace(None);
        }
    }
    clear_picture_underlines(
        screen.frame.buffer_mut(),
        &picture_cells,
        Rect::new(
            cols.start as u16,
            rows.start as u16,
            cols.len() as u16,
            rows.len() as u16,
        ),
    );
    hits.targets = targets;
    ui.hits.replace(hits);
}

/// Whether a terminal `full` columns wide and `height` rows tall is below the reader's minimum size.
fn too_small(full: usize, height: usize) -> bool {
    full < render::MIN_COLS || height < render::MIN_ROWS
}

/// The only thing a terminal below the minimum size shows: what size SoCLI needs, centered.
fn draw_too_small(screen: &mut Screen) {
    if screen.full == 0 || screen.height == 0 {
        return;
    }
    let by = if screen.theme.ascii { 'x' } else { '×' };
    let text = format!(
        "SoCLI needs {}{by}{} · q quits",
        render::MIN_COLS,
        render::MIN_ROWS
    );
    let text = chrome::cut(&text, screen.full, screen.theme.ascii);
    let x = (screen.full - text.width()) / 2;
    let y = (screen.height - 1) / 2;
    let style = render::ratatui_style(Kind::Fg, screen.theme, false)
        .patch(render::row_style(screen.theme, false));
    let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
        return;
    };
    screen.frame.buffer_mut().set_string(x, y, &text, style);
}

/// Whether `event` quits the minimum-size screen: q, esc or ctrl-c.
#[cfg(feature = "native")]
fn quits_too_small(event: &Event) -> bool {
    let Event::Key(key) = event else {
        return false;
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    key.kind != KeyEventKind::Release
        && match key.code {
            KeyCode::Char('q') => !ctrl,
            KeyCode::Char('c') => ctrl,
            KeyCode::Esc => true,
            _ => false,
        }
}

/// Chrome targets from a line drawn at screen row `y`, so its column 0 is screen column `margin`.
fn place(targets: Vec<Target>, y: usize, zone: Zone, margin: usize) -> Vec<Target> {
    targets
        .into_iter()
        .filter_map(|target| {
            Some(Target {
                x: target.x.checked_add(u16::try_from(margin).ok()?)?,
                y: u16::try_from(y + usize::from(target.y)).ok()?,
                zone,
                ..target
            })
        })
        .collect()
}

/// Line targets moved to row `row` of a block of lines.
fn below(targets: Vec<Target>, row: u16) -> Vec<Target> {
    targets
        .into_iter()
        .map(|target| Target { y: row, ..target })
        .collect()
}

fn highlight(buffer: &mut Buffer, target: &Target, theme: &Theme, pictures: &[Rect]) {
    for x in target.x..target.x.saturating_add(target.width) {
        if pictures
            .iter()
            .any(|area| area.contains(Position::new(x, target.y)))
        {
            continue;
        }
        let Some(cell) = buffer.cell_mut(Position::new(x, target.y)) else {
            continue;
        };
        cell.modifier.insert(Modifier::UNDERLINED | Modifier::BOLD);
        if !theme.mono && !theme.plain && !theme.native {
            cell.set_fg(theme.fg(Kind::Blue, false));
        }
    }
}

fn underline(buffer: &mut Buffer, target: &Target) {
    for x in target.x..target.x.saturating_add(target.width) {
        if let Some(cell) = buffer.cell_mut(Position::new(x, target.y)) {
            if !cell.symbol().trim().is_empty() {
                cell.modifier.insert(Modifier::UNDERLINED);
            }
        }
    }
}

fn clear_picture_underlines(buffer: &mut Buffer, pictures: &[Rect], overlay: Rect) {
    for picture in pictures {
        let visible = picture.intersection(buffer.area);
        for y in visible.top()..visible.bottom() {
            for x in visible.left()..visible.right() {
                if overlay.contains(Position::new(x, y)) {
                    continue;
                }
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.modifier.remove(Modifier::UNDERLINED);
                }
            }
        }
    }
}

/// Body row of `slot`'s top when the whole box is inside the viewport.
fn on_screen(slot: &Slot, offset: usize, height: usize) -> Option<usize> {
    (slot.y >= offset && slot.y + slot.rows <= offset + height).then(|| slot.y - offset)
}

/// Whether `slot` lies within one viewport above or below the visible rows.
fn wanted(slot: &Slot, offset: usize, height: usize) -> bool {
    slot.y + slot.rows + height > offset && slot.y <= offset + 2 * height
}

/// Up to two lines of alt text for `room` columns, the second ending in … when more is left.
fn alt_lines(alt: &str, room: usize) -> Vec<String> {
    let mut lines = util::wrap_text(alt.trim(), room);
    if lines.len() > 2 {
        let rest = lines[1..].join(" ");
        lines.truncate(1);
        lines.push(util::truncate(&rest, room));
    }
    lines.retain(|line| !line.is_empty());
    lines
}

/// Blanks the cells of `area`, keeping their colors, so no frame line shows around a fitted image.
fn blank(screen: &mut Screen, area: Rect) {
    let buffer = screen.frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(" ");
            }
        }
    }
}

/// Draws the loading or failure frame of an image box at screen column `x`, row `y`.
fn frame_at(screen: &mut Screen, x: usize, y: usize, cols: usize, rows: usize, label: &str) {
    let theme = screen.theme;
    let buffer = screen.frame.buffer_mut();
    for part in render::preview_frame(cols, rows, label) {
        let (Ok(px), Ok(py)) = (u16::try_from(x + part.col), u16::try_from(y + part.row)) else {
            continue;
        };
        let style = render::ratatui_style(part.kind, theme, false);
        buffer.set_string(px, py, &part.text, style);
    }
}

/// Fits `size` inside `area`, centered.
fn centered(area: Rect, size: Rect) -> Rect {
    let width = size.width.min(area.width);
    let height = size.height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/**
Paints inline images that sit wholly inside the body, and kitty profile pictures even when the
viewport clips them. Other pictures that are only partly on screen keep their loading frame.
A box under the dialog is left alone. Asks for images one viewport around. A box still loading
shows `spin`: centered in a rounded outline, or over an avatar's letter.
*/
#[allow(clippy::too_many_arguments, reason = "called once per pane from draw")]
fn paint_previews(
    screen: &mut Screen,
    previews: &mut Previews,
    slots: &[Slot],
    lead: usize,
    offset: usize,
    body_top: usize,
    body_height: usize,
    rows: &Range<usize>,
    cols: &Range<usize>,
    spin: char,
) {
    #[cfg(feature = "native")]
    let kitty = previews.kitty();
    for slot in slots {
        if wanted(slot, offset, body_height) {
            if slot.avatar {
                previews.request_avatar(&slot.url, slot.square);
            } else {
                previews.request(&slot.url);
            }
        }
        #[cfg(feature = "native")]
        if slot.avatar && kitty {
            place_kitty_avatar(
                screen,
                previews,
                slot,
                lead,
                offset,
                body_top,
                body_height,
                rows,
                cols,
                spin,
            );
            continue;
        }
        let Some(row) = on_screen(slot, offset, body_height) else {
            continue;
        };
        let (x, y) = (lead + slot.x, body_top + row);
        let hidden = rows.start < y + slot.rows
            && y < rows.end
            && cols.start < x + slot.cols
            && x < cols.end;
        if hidden {
            continue;
        }
        let (Ok(ax), Ok(ay), Ok(aw), Ok(ah)) = (
            u16::try_from(x),
            u16::try_from(y),
            u16::try_from(slot.cols),
            u16::try_from(slot.rows),
        ) else {
            continue;
        };
        let area = Rect::new(ax, ay, aw, ah);
        let mut shown = previews.shown(&slot.url);
        #[cfg(feature = "native")]
        if shown == Shown::Ready {
            if let Some((protocol, fill)) = previews.encoded(&slot.url, area) {
                blank(screen, area);
                let fill = Rect::new(area.x, area.y, fill.width, fill.height);
                let image = StatefulImage::default().resize(Resize::Scale(None));
                screen.frame.render_stateful_widget(image, fill, protocol);
                if kitty {
                    if let Some(data) = crate::kitty_place::settle(screen.frame.buffer_mut(), fill)
                    {
                        crate::kitty_place::emit(&data);
                    }
                }
                continue;
            }
            shown = Shown::Loading;
        }
        #[cfg(not(feature = "native"))]
        if shown == Shown::Ready {
            blank(screen, area);
            previews.note(preview::Place {
                url: slot.url.clone(),
                col: ax,
                row: ay,
                cols: aw,
                rows: ah,
                avatar: slot.avatar,
                square: slot.square,
            });
            continue;
        }
        match shown {
            Shown::Failed if slot.avatar => {}
            Shown::Failed => {
                blank(screen, area);
                frame_at(
                    screen,
                    x,
                    y,
                    slot.cols,
                    slot.rows,
                    "image unavailable · o opens it",
                );
            }
            Shown::Ready | Shown::Missing | Shown::Loading if slot.avatar => {
                let style = render::ratatui_style(Kind::Dim, screen.theme, false);
                if let Some(cell) = screen.frame.buffer_mut().cell_mut((ax + 2, ay + 1)) {
                    cell.set_symbol(&spin.to_string());
                    cell.set_style(style);
                }
            }
            Shown::Ready | Shown::Missing | Shown::Loading => {
                blank(screen, area);
                frame_at(screen, x, y, slot.cols, slot.rows, &spin.to_string());
            }
        }
    }
}

/// A kitty profile picture, including the rows the viewport clips. Other protocols stay on the whole-box path.
#[cfg(feature = "native")]
#[allow(
    clippy::too_many_arguments,
    reason = "the pane paint passes the same viewport paint_previews has"
)]
fn place_kitty_avatar(
    screen: &mut Screen,
    previews: &mut Previews,
    slot: &Slot,
    lead: usize,
    offset: usize,
    body_top: usize,
    body_height: usize,
    rows: &Range<usize>,
    cols: &Range<usize>,
    spin: char,
) {
    let visible = (0..slot.rows).any(|local_y| {
        avatar_cell(slot, lead, offset, body_top, body_height, 0, local_y).is_some()
    });
    if !visible {
        return;
    }
    let shown = previews.shown(&slot.url);
    if shown == Shown::Failed {
        return;
    }
    if shown == Shown::Ready {
        let (Ok(width), Ok(height)) = (u16::try_from(slot.cols), u16::try_from(slot.rows)) else {
            return;
        };
        let box_area = Rect::new(0, 0, width, height);
        if let Some((protocol, fill)) = previews.encoded(&slot.url, box_area) {
            let place = Rect::new(0, 0, fill.width, fill.height);
            if place.width > 0 && place.height > 0 {
                let mut scratch = Buffer::empty(place);
                protocol.render(place, &mut scratch);
                if let Some(data) = crate::kitty_place::settle(&mut scratch, place) {
                    crate::kitty_place::emit(&data);
                    blit_kitty(
                        screen,
                        &scratch,
                        slot,
                        lead,
                        offset,
                        body_top,
                        body_height,
                        rows,
                        cols,
                    );
                    return;
                }
            }
        }
    }
    let Some((x, y)) = avatar_cell(slot, lead, offset, body_top, body_height, 2, 1) else {
        return;
    };
    if covered(x, y, rows, cols) {
        return;
    }
    let (Ok(ax), Ok(ay)) = (u16::try_from(x), u16::try_from(y)) else {
        return;
    };
    let style = render::ratatui_style(Kind::Dim, screen.theme, false);
    if let Some(cell) = screen.frame.buffer_mut().cell_mut((ax, ay)) {
        cell.set_symbol(&spin.to_string());
        cell.set_style(style);
    }
}

/// Copies settled kitty placeholders onto the visible cells, leaving the cell background in place.
#[cfg(feature = "native")]
#[allow(
    clippy::too_many_arguments,
    reason = "blits one avatar into the pane viewport"
)]
fn blit_kitty(
    screen: &mut Screen,
    scratch: &Buffer,
    slot: &Slot,
    lead: usize,
    offset: usize,
    body_top: usize,
    body_height: usize,
    rows: &Range<usize>,
    cols: &Range<usize>,
) {
    let image_cols = usize::from(scratch.area.width);
    let image_rows = usize::from(scratch.area.height);
    for local_y in 0..image_rows {
        for local_x in 0..image_cols {
            let Some((x, y)) =
                avatar_cell(slot, lead, offset, body_top, body_height, local_x, local_y)
            else {
                continue;
            };
            if covered(x, y, rows, cols) {
                continue;
            }
            let Some(src) = scratch.cell(Position::new(local_x as u16, local_y as u16)) else {
                continue;
            };
            let symbol = src.symbol().to_string();
            let fg = src.fg;
            let (Ok(sx), Ok(sy)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let Some(dst) = screen.frame.buffer_mut().cell_mut(Position::new(sx, sy)) else {
                continue;
            };
            dst.set_symbol(&symbol);
            dst.set_fg(fg);
            dst.set_skip(false);
        }
    }
}

/// Screen column and row of one cell in `slot`, when that cell is inside the body.
#[cfg(feature = "native")]
fn avatar_cell(
    slot: &Slot,
    lead: usize,
    offset: usize,
    body_top: usize,
    body_height: usize,
    local_x: usize,
    local_y: usize,
) -> Option<(usize, usize)> {
    if local_x >= slot.cols || local_y >= slot.rows {
        return None;
    }
    let rel = slot.y as isize - offset as isize + local_y as isize;
    if rel < 0 || rel >= body_height as isize {
        return None;
    }
    Some((lead + slot.x + local_x, body_top + rel as usize))
}

/// Whether the dialog covers screen cell `(x, y)`.
#[cfg(feature = "native")]
fn covered(x: usize, y: usize, rows: &Range<usize>, cols: &Range<usize>) -> bool {
    rows.contains(&y) && cols.contains(&x)
}

/// The full-screen image viewer of mockup 13-image: byline, the picture centered, alt text, keys.
fn draw_gallery(screen: &mut Screen, state: &State, ui: &Ui, gallery: &Gallery, width: usize) {
    let height = screen.height;
    let count = gallery.pictures.len();
    let picture = gallery.pictures.get(gallery.index);

    let mut canvas = Canvas::new(width);
    let y = canvas.push(false);
    if let Some(post) = &gallery.post {
        let x = canvas.put(y, 0, Kind::Bold, &post.author.name);
        let byline = format!(" @{} · {}", post.author.username, post.net.label());
        canvas.put(y, x, Kind::Dim, &byline);
    }
    if count > 1 {
        let of = format!("{} of {count}", gallery.index + 1);
        canvas.put_right(y, width - 1, &[(Kind::Dim, &of)]);
    }
    screen.line(1, &canvas.finish_line());
    screen.line(2, &chrome::rule(width));

    let alt = picture
        .and_then(|picture| picture.alt.as_deref())
        .map(|alt| alt_lines(alt, width.saturating_sub(4)))
        .unwrap_or_default();
    let rows = height.saturating_sub(7 + alt.len());
    let cols = width.saturating_sub(4);
    let (box_x, box_y) = (screen.margin + 2, 4);
    if rows >= 3 && cols >= 10 {
        let area = Rect::new(box_x as u16, box_y as u16, cols as u16, rows as u16);
        let mut previews = ui.previews.borrow_mut();
        match (previews.as_mut(), picture) {
            (Some(previews), Some(picture)) => {
                previews.request(&picture.url);
                if count > 1 {
                    let next = &gallery.pictures[(gallery.index + 1) % count].url;
                    let previous = &gallery.pictures[(gallery.index + count - 1) % count].url;
                    previews.request(next);
                    previews.request(previous);
                }
                let mut shown = previews.shown(&picture.url);
                #[cfg(feature = "native")]
                {
                    let kitty = previews.kitty();
                    if shown == Shown::Ready {
                        match previews.encoded(&picture.url, area) {
                            Some((protocol, fill)) => {
                                let place = centered(area, fill);
                                let image = StatefulImage::default().resize(Resize::Scale(None));
                                screen.frame.render_stateful_widget(image, place, protocol);
                                if kitty {
                                    if let Some(data) =
                                        crate::kitty_place::settle(screen.frame.buffer_mut(), place)
                                    {
                                        crate::kitty_place::emit(&data);
                                    }
                                }
                            }
                            None => shown = Shown::Loading,
                        }
                    }
                }
                #[cfg(not(feature = "native"))]
                if shown == Shown::Ready {
                    blank(screen, area);
                    previews.note(preview::Place {
                        url: picture.url.clone(),
                        col: area.x,
                        row: area.y,
                        cols: area.width,
                        rows: area.height,
                        avatar: false,
                        square: false,
                    });
                }
                match shown {
                    Shown::Ready => {}
                    Shown::Failed => frame_at(
                        screen,
                        box_x,
                        box_y,
                        cols,
                        rows,
                        "image unavailable · o opens it",
                    ),
                    Shown::Missing | Shown::Loading => {
                        let label = if state.detail == theme::Detail::Rich {
                            spin_frame(ui.started.elapsed()).to_string()
                        } else {
                            "image".to_string()
                        };
                        frame_at(screen, box_x, box_y, cols, rows, &label);
                    }
                }
            }
            _ => frame_at(
                screen,
                box_x,
                box_y,
                cols,
                rows,
                "image unavailable · o opens it",
            ),
        }
    }

    let first_alt = height.saturating_sub(2 + alt.len());
    for (i, line) in alt.iter().enumerate() {
        let mut canvas = Canvas::new(width);
        let y = canvas.push(false);
        canvas.put(y, width.saturating_sub(line.width()) / 2, Kind::Dim, line);
        screen.line(first_alt + i, &canvas.finish_line());
    }
    screen.line(height.saturating_sub(2), &chrome::rule(width));

    let mut canvas = Canvas::new(width);
    let y = canvas.push(false);
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    if count > 1 {
        pairs.push((theme::ascii_key("← →", state.theme.ascii), "previous, next"));
    }
    pairs.push(("o", "open original"));
    if gallery.post.is_some() {
        pairs.push(("s", "share"));
    }
    pairs.push(("esc", "close"));
    let mut x = 0;
    for (i, (key, label)) in pairs.iter().enumerate() {
        if i > 0 {
            x += 2;
        }
        if state.reveal_shortcuts {
            x = canvas.put(y, x, Kind::Bold, key);
            x += 1;
        }
        x = canvas.put(y, x, Kind::Dim, label);
    }
    let free = width.saturating_sub(x + 2);
    if !state.status.is_empty() && free >= 10 {
        let status = util::truncate(&state.status, free);
        canvas.put_right(y, width - 1, &[(Kind::Dim, &status)]);
    }
    screen.line(height.saturating_sub(1), &canvas.finish_line());
}

/// Runs a share dialog action; the dialog is already closed.
fn run_share(
    state: &mut State,
    clients: &mut Clients,
    ui: &mut Ui,
    share: &Share,
    action: ShareAction,
) -> Result<bool, String> {
    let copy = |state: &mut State, text: &str, what: &str| match clip::copy(text) {
        Ok(clip::Via::System) => state.confirm(format!("copied {what}")),
        Ok(clip::Via::Terminal) => state.confirm(format!("copied {what} (terminal clipboard)")),
        Err(err) => state.fail(err),
    };
    match action {
        ShareAction::CopyLink => copy(state, &share.link, "link"),
        ShareAction::CopyProfile => copy(state, &share.profile_url, "profile link"),
        ShareAction::Repost => return apply(state, clients, ui, Cmd::Repost { n: None }),
        ShareAction::Unrepost | ShareAction::Reposted => {
            return apply(state, clients, ui, Cmd::Unrepost { n: None });
        }
        ShareAction::Quote => {
            let quote = share
                .post
                .clone()
                .unwrap_or_else(|| share.profile_url.clone());
            ui.mode = Mode::Compose(Composer::open(state, None, Some(quote), share.checked()));
        }
    }
    Ok(false)
}

/// `f` and `/` open the search input, `F` the network filter panel.
fn search_key(state: &mut State, ui: &mut Ui, key: KeyEvent) -> bool {
    // Alt-f is Option-Right on Ghostty (`esc f`), not the search key.
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return false;
    }
    match key.code {
        KeyCode::Char('f' | '/') => {
            ui.mode = Mode::Search(Input::new());
            if state.section != Section::Search {
                state.push_place();
            }
            state.section = Section::Search;
        }
        KeyCode::Char('F') => {
            ui.mode = Mode::Filter {
                on: state.on.clone(),
                cursor: 0,
            };
        }
        _ => return false,
    }
    true
}

/// A page key on a post list: the selection moves a page and its block scrolls to the top.
fn page_key(state: &mut State, ui: &Ui, key: KeyEvent) -> bool {
    let (width, height) = ui.viewport.get();
    let Some(rows) = page_rows(key, height) else {
        return false;
    };
    let before = state.selected_index();
    state.page(width, i64::from(rows));
    // The far edge is already on screen and there is no further post to land on.
    if state.selected_index() == before {
        return true;
    }
    if let Some(focus) = screens::linked_body(state, width, true).focus {
        let scroll = if ui.split.get() && state.thread_focus.is_some() {
            &ui.scroll_right
        } else {
            &ui.scroll
        };
        scroll.set(focus.start + reader_pad(state, ui));
    }
    true
}

/**
Offsets that keep a post taller than `height` on screen. The window may show one row before the
post and one row after it. `None` when `height` is zero or the post fits.
*/
fn tall_range(span: RowSpan, height: usize, total: usize) -> Option<(usize, usize)> {
    if height == 0 || span.end.saturating_sub(span.start) <= height {
        return None;
    }
    let start = span.start.saturating_sub(1);
    let end = (span.end + 1).min(total).saturating_sub(height).max(start);
    Some((start, end))
}

/**
Moves the keyed pane through a selected post taller than that pane. One `j`, wheel notch or page
key stays inside the post until its far edge is on screen. Returns whether the offset moved, so
the caller leaves the selection where it is. A viewport that has not been drawn yet does nothing.
*/
fn slide_focus(state: &State, ui: &Ui, delta: i32) -> bool {
    if delta == 0 {
        return false;
    }
    let (width, height) = ui.viewport.get();
    if width == 0 || height == 0 {
        return false;
    }
    // A single pane lays its pictures out for this height. A split pane does not.
    let laid = (!ui.split.get()).then_some(height);
    let body = screens::linked_body_sized(state, width, true, laid);
    let Some(focus) = body.focus else {
        return false;
    };
    let pad = reader_pad(state, ui);
    let span = RowSpan {
        start: focus.start + pad,
        end: focus.end + pad,
    };
    let total = pad + body.lines.len() + pad;
    let Some((start, end)) = tall_range(span, height, total) else {
        return false;
    };
    let scroll = if ui.split.get() && state.thread_focus.is_some() {
        &ui.scroll_right
    } else {
        &ui.scroll
    };
    let offset = scroll.get();
    let next = if delta > 0 {
        if offset >= end {
            return false;
        }
        (offset as i64)
            .saturating_add(i64::from(delta))
            .min(end as i64)
    } else if offset <= start {
        return false;
    } else {
        (offset as i64)
            .saturating_add(i64::from(delta))
            .max(start as i64)
    };
    if next < 0 || next as usize == offset {
        return false;
    }
    scroll.set(next as usize);
    true
}

/**
Moves `offset` so the focused block shows, with a row of context when there is room and it had
to scroll; a block whose top row is already the first row stays there. A block taller than the
pane keeps an offset already inside it, shows its top when the window was above it, and shows its
bottom when the window was below it. A blank context row is not left glued to the viewport edge:
the neighboring line comes inside with it when it fits, and the blank is dropped when that line
does not. A blank at the document's own start or end stays, so the list's padding is still there
at rest.
*/
fn follow_past_blanks(
    offset: usize,
    focus: Option<RowSpan>,
    height: usize,
    total: usize,
    is_blank: impl Fn(usize) -> bool,
) -> usize {
    let mut offset = offset;
    if let Some(span) = focus {
        if let Some((start, end)) = tall_range(span, height, total) {
            if offset >= span.end {
                offset = end;
            } else if offset.saturating_add(height) <= span.start {
                offset = start;
            } else {
                offset = offset.clamp(start, end);
            }
        } else {
            let (mut top, mut bottom) = if span.end - span.start + 2 <= height {
                (span.start.saturating_sub(1), (span.end + 1).min(total))
            } else {
                (span.start, span.end)
            };
            if top < span.start && is_blank(top) {
                if top > 0 && bottom - (top - 1) <= height {
                    top -= 1;
                } else if top > 0 {
                    top = span.start;
                }
            }
            if bottom > span.end && bottom > 0 && is_blank(bottom - 1) {
                if bottom < total && bottom + 1 - top <= height {
                    bottom += 1;
                } else if bottom < total {
                    bottom = span.end;
                }
            }
            if span.start < offset {
                offset = top;
            } else if bottom > offset + height {
                offset = (bottom - height).min(top);
            }
        }
    }
    offset.min(total.saturating_sub(height))
}

/// JSON rows shifted two columns so the cursor row carries the focus bar like a selected post.
fn inspect_lines(value: &Value, open: &HashSet<String>, cursor: usize) -> Vec<StyledLine> {
    jsonview::visible(value, open)
        .into_iter()
        .enumerate()
        .map(|(i, (_, mut line))| {
            line.selected = i == cursor;
            let (kind, bar) = if line.selected {
                (Kind::Blue, "▍ ")
            } else {
                (Kind::Fg, "  ")
            };
            line.spans.insert(
                0,
                render::Span {
                    kind,
                    text: bar.into(),
                },
            );
            line
        })
        .collect()
}

struct Screen<'a, 'b> {
    frame: &'a mut Frame<'b>,
    theme: &'a Theme,
    full: usize,
    /// [`render::margin`] of `full`: the columns before every row's content.
    margin: usize,
    height: usize,
    simple: bool,
}

impl Screen<'_, '_> {
    fn line(&mut self, y: usize, line: &StyledLine) {
        if y >= self.height {
            return;
        }
        let area = Rect::new(0, y as u16, self.full as u16, 1);
        self.frame.render_widget(
            Paragraph::new(to_line_in(
                line,
                self.theme,
                self.full,
                self.margin,
                Kind::Blue,
                true,
            )),
            area,
        );
    }

    /**
    One pane row in the `width` columns from screen column `x`; an unfocused pane keeps its
    selection bar, dimmed, without the selected background. `chrome` rows take the plain level's
    ASCII glyphs, while post rows keep their text as written.
    */
    fn pane(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        line: &StyledLine,
        focused: bool,
        chrome: bool,
    ) {
        if y >= self.height || x + width > self.full {
            return;
        }
        let dimmed;
        let (line, bar) = if focused {
            (line, Kind::Blue)
        } else {
            dimmed = StyledLine {
                selected: false,
                hl: None,
                ..line.clone()
            };
            (&dimmed, Kind::Line)
        };
        let area = Rect::new(x as u16, y as u16, width as u16, 1);
        self.frame.render_widget(
            Paragraph::new(to_line_in(
                line,
                self.theme,
                width,
                self.margin,
                bar,
                chrome,
            )),
            area,
        );
    }

    /// One glyph in one cell, over whatever is drawn there.
    fn glyph(&mut self, x: usize, y: usize, glyph: char, kind: Kind) {
        if y >= self.height || x >= self.full {
            return;
        }
        let glyph = if self.theme.ascii {
            theme::ascii_char(glyph)
        } else {
            glyph
        };
        let style = render::ratatui_style(kind, self.theme, false)
            .patch(render::row_style(self.theme, false));
        let area = Rect::new(x as u16, y as u16, 1, 1);
        self.frame.render_widget(
            Paragraph::new(Line::from(Span::styled(glyph.to_string(), style))),
            area,
        );
    }

    /// A dialog row `width` columns wide at column `x`, over whatever is drawn there.
    fn overlay(&mut self, x: usize, y: usize, width: usize, line: &StyledLine) {
        if y >= self.height || x + width > self.full {
            return;
        }
        let lit = |col: usize| line.hl.is_some_and(|(a, b)| (a..b).contains(&col));
        let mut spans = Vec::new();
        let mut col = 0;
        let cell = |text: String, kind: Kind, col: usize, spans: &mut Vec<Span<'static>>| {
            let on = lit(col);
            let style = render::ratatui_style(kind, self.theme, on)
                .patch(render::row_style(self.theme, on));
            spans.push(Span::styled(text, style));
        };
        for span in &line.spans {
            for (grapheme, width) in util::graphemes(&span.text) {
                let text = if self.simple && span.kind == Kind::Line {
                    " ".repeat(width)
                } else if self.theme.ascii {
                    grapheme.chars().map(theme::ascii_char).collect()
                } else {
                    grapheme.to_string()
                };
                cell(text, span.kind, col, &mut spans);
                col += width;
            }
        }
        while col < width {
            cell(" ".into(), Kind::Fg, col, &mut spans);
            col += 1;
        }
        let area = Rect::new(x as u16, y as u16, width as u16, 1);
        self.frame.render_widget(Clear, area);
        self.frame
            .render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn block(&mut self, y: usize, height: usize, lines: &[StyledLine], offset: usize) {
        for (i, line) in lines.iter().skip(offset).take(height).enumerate() {
            self.line(y + i, line);
        }
    }

    fn cursor(&mut self, x: usize, y: usize) {
        if y < self.height && x < self.full {
            self.frame
                .set_cursor_position(Position::new(x as u16, y as u16));
        }
    }
}

/**
One canvas row painted across the full terminal width, `margin` columns before its content, its
selection bar in `bar`'s color: in the margin's last column, or over canvas column 0 when there is
no margin. A highlight range gets the selected-row background over just its columns. At the plain
level the bar is `>`, or `:` in an unfocused pane (drawn in `Kind::Line`) since there is no dim to
tell them apart, and a `chrome` row has its glyphs drawn in ASCII.
*/
fn to_line_in(
    line: &StyledLine,
    theme: &Theme,
    full: usize,
    margin: usize,
    bar: Kind,
    chrome: bool,
) -> Line<'static> {
    let ascii = chrome && theme.ascii;
    let hl = line.hl.map(|(a, b)| (a + margin, b + margin));
    let lit = |col: usize| line.selected || hl.is_some_and(|(a, b)| (a..b).contains(&col));
    let row_style = render::row_style(theme, line.selected);
    let mut spans = if line.bar {
        vec![
            Span::styled(" ".repeat(margin.saturating_sub(1)), row_style),
            Span::styled(
                match (theme.ascii, bar) {
                    (false, _) => "▍",
                    (true, Kind::Line) => ":",
                    (true, _) => ">",
                },
                render::ratatui_style(bar, theme, line.selected).patch(row_style),
            ),
        ]
    } else {
        vec![Span::styled(" ".repeat(margin), row_style)]
    };
    let covered = margin + usize::from(line.bar && margin == 0);
    let mut col = margin;
    let push = |text: String, kind: Kind, on: bool, spans: &mut Vec<Span<'static>>| {
        let style = render::ratatui_style(kind, theme, on).patch(render::row_style(theme, on));
        spans.push(Span::styled(text, style));
    };
    for span in &line.spans {
        let mut run = String::new();
        let mut run_on = lit(col);
        for (grapheme, width) in util::graphemes(&span.text) {
            let on = lit(col);
            if on != run_on && !run.is_empty() {
                push(std::mem::take(&mut run), span.kind, run_on, &mut spans);
            }
            run_on = on;
            if col + width <= covered {
                col += width;
                continue;
            }
            if col < covered {
                run.push_str(&" ".repeat(col + width - covered));
            } else if ascii {
                run.extend(grapheme.chars().map(theme::ascii_char));
            } else {
                run.push_str(grapheme);
            }
            col += width;
        }
        if !run.is_empty() {
            push(run, span.kind, run_on, &mut spans);
        }
    }
    while col < full {
        let on = lit(col);
        let mut end = col;
        while end < full && lit(end) == on {
            end += 1;
        }
        spans.push(Span::styled(
            " ".repeat(end - col),
            render::row_style(theme, on),
        ));
        col = end;
    }
    Line::from(spans)
}

// MARK: ask

fn draw_ask(
    screen: &mut Screen,
    state: &State,
    ui: &Ui,
    input: &Input,
    asked: &[String],
    width: usize,
) {
    ui.scroll_key.replace("ask".into());
    let posts = state.current_posts();
    let loaded = screens::Loaded {
        posts: posts.len(),
        mentions: state.mention_count(),
        liked: state.posts.values().filter(|p| p.liked).count(),
    };
    let handle = &state.you().username;
    let mut lines = screens::ask_welcome(handle, &loaded, width);
    for question in asked {
        lines.extend(screens::ask_answer(question, &posts, width));
    }
    lines.insert(0, StyledLine::blank());
    lines.push(StyledLine::blank());
    let prompt = screens::ask_prompt(input.text(), handle, width);
    let bottom = screen.height.saturating_sub(prompt.len()).max(1);
    let room = bottom - 1;
    let overflow = lines.len().saturating_sub(room);
    ui.overflow.set(overflow);
    let back = ui.scroll.get().min(overflow);
    screen.block(1, room, &lines, overflow - back);
    for (i, line) in prompt.iter().enumerate() {
        screen.line(bottom + i, line);
    }
    screen.cursor(
        screen.margin + 2 + line_width(input.before_cursor()),
        bottom + 1,
    );
}

// MARK: composer

#[derive(Default)]
struct Bottom {
    lines: Vec<StyledLine>,
    /// Cursor inside these rows, as (column within the inner width, row).
    cursor: Option<(usize, usize)>,
    /// Targets on these rows, each `y` a row of `lines`.
    targets: Vec<Target>,
    /// The drafts list's rows: the row of `lines`, its columns, and the draft it shows.
    rows: Vec<(usize, Range<usize>, usize)>,
    /// The drafts list's box, in rows of `lines` and columns.
    dialog: Option<(Range<usize>, Range<usize>)>,
}

/// Width of the unsaved-draft dialog and of the drafts list, from design/src/screens/draft*.js.
const DIALOG_WIDTH: usize = 45;
const LIST_WIDTH: usize = 84;
/// The inner composer row each overlay starts on.
const DIALOG_ROW: usize = 5;
const LIST_ROW: usize = 2;
/// Drafts the list shows at once.
const LIST_ROWS: usize = 8;

/**
The composer box and its hint row. `fill` is the row count of the full-screen composer, where the
box grows to fill it; without it the box shows two to six text rows above the hints.
*/
fn composer(
    state: &State,
    c: &Composer,
    width: usize,
    enhanced: bool,
    fill: Option<usize>,
) -> Bottom {
    let input = &c.input;
    let right = width - 1;
    let replying = c.reply_to.as_deref().and_then(|id| state.posts.get(id));
    let quoted = c
        .quote
        .as_deref()
        .map(|quote| (quote, state.posts.get(quote)));
    let mut canvas = Canvas::new(width);

    let y = canvas.push(false);
    let x = canvas.put(y, 0, Kind::Blue, "╭─");
    let x = match (replying, quoted) {
        (Some(post), _) => {
            let x = canvas.put(y, x + 1, Kind::Bold, "Replying to");
            canvas.put(y, x + 1, Kind::Blue, &format!("@{}", post.author.username))
        }
        (None, Some((_, Some(_)))) => canvas.put(y, x + 1, Kind::Bold, "Quote post"),
        (None, Some((_, None))) => canvas.put(y, x + 1, Kind::Bold, "Share profile"),
        (None, None) => canvas.put(y, x + 1, Kind::Bold, "New post"),
    };
    canvas.put(
        y,
        x + 1,
        Kind::Blue,
        &"─".repeat(right.saturating_sub(x + 1)),
    );
    canvas.put(y, right, Kind::Blue, "╮");
    let side = |canvas: &mut Canvas| {
        let y = canvas.push(false);
        canvas.put(y, 0, Kind::Blue, "│");
        canvas.put(y, right, Kind::Blue, "│");
        y
    };

    let mut head = 0;
    if quoted.is_some() {
        let y = side(&mut canvas);
        let mut x = canvas.put(y, 2, Kind::Dim, "to");
        for net in &c.targets {
            x = canvas.put(y, x + 2, net.accent(), "●");
            x = canvas.put(y, x + 1, Kind::Fg, net.title());
        }
        head = 1;
    }
    let card = quoted.map_or_else(Vec::new, |(quote, post)| {
        quote_card(quote, post, width.saturating_sub(4).min(QUOTE_CARD))
    });
    let tail = if card.is_empty() { 0 } else { 1 + card.len() };

    let text_width = width.saturating_sub(4).max(1);
    let rows = wrap_ranges(input.text(), text_width);
    let cursor_row = rows
        .iter()
        .rposition(|(start, _)| *start <= input.cursor)
        .unwrap_or(0);
    let (top, box_width) = match &c.overlay {
        Overlay::Ask(_) => (DIALOG_ROW, DIALOG_WIDTH.min(width)),
        _ => (LIST_ROW, LIST_WIDTH.min(width)),
    };
    let overlay = overlay_rows(
        &c.overlay,
        box_width,
        state.theme.ascii,
        state.reveal_shortcuts,
    );
    let room = match &c.overlay {
        Overlay::None => 0,
        Overlay::Ask(_) => DIALOG_ROW + overlay.len(),
        Overlay::List { .. } => LIST_ROW + overlay.len(),
    };
    let lines = match fill {
        Some(fill) => fill.saturating_sub(3 + head + tail).max(1),
        None => rows.len().clamp(2, 6),
    };
    let shown = lines.max(room.saturating_sub(head + tail));
    let first = (cursor_row + 1).saturating_sub(lines);
    let text_top = canvas.len();
    for i in first..first + shown {
        let y = side(&mut canvas);
        if let Some((start, end)) = rows.get(i) {
            canvas.put(y, 2, Kind::Fg, &input.text()[*start..*end]);
        }
    }
    if !card.is_empty() {
        side(&mut canvas);
        for cells in &card {
            let y = side(&mut canvas);
            let mut x = 2;
            for (kind, text) in cells {
                x = canvas.put(y, x, *kind, text);
            }
        }
    }
    let (start, _) = rows[cursor_row];
    let cursor_x = 2 + line_width(&input.text()[start..input.cursor]);
    let cursor =
        matches!(c.overlay, Overlay::None).then_some((cursor_x, 1 + head + cursor_row - first));
    let col = width.saturating_sub(box_width) / 2;
    for (i, cells) in overlay.iter().enumerate() {
        let y = text_top - head + top + i;
        canvas.put(y, col, Kind::Fg, &" ".repeat(box_width));
        let mut x = col;
        for (kind, text) in cells {
            x = canvas.put(y, x, *kind, text);
        }
    }
    let mut list_rows = Vec::new();
    let mut dialog = None;
    if let Overlay::List { drafts, cursor } = &c.overlay {
        let first = text_top - head + top;
        dialog = Some((first..first + overlay.len(), col..col + box_width));
        let start = (cursor + 1).saturating_sub(LIST_ROWS);
        let shown = drafts.len().saturating_sub(start).min(LIST_ROWS);
        for k in 0..shown {
            list_rows.push((
                first + 2 + k,
                col + 1..col + box_width.saturating_sub(1),
                start + k,
            ));
        }
    }

    let (strictest, left) = c.room(state);
    let max = strictest.caps().max_chars.max(1);
    let used = (max as i64 - left).max(0) as usize;
    let step = ((used * 8).div_ceil(max)).clamp(1, 8) - 1;
    let gauge = Icons::new(state.icons && !state.theme.ascii).gauge[step];
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Blue, "╰");
    let count = left.to_string();
    let tail = 1 + gauge.width() + 1 + count.width() + 1 + 2;
    canvas.put(
        y,
        1,
        Kind::Blue,
        &"─".repeat(width.saturating_sub(1 + tail)),
    );
    canvas.put_right(
        y,
        right,
        &[
            (Kind::Fg, " "),
            (Kind::Blue, gauge),
            (Kind::Fg, " "),
            (if left < 0 { Kind::Red } else { Kind::Dim }, &count),
            (Kind::Fg, " "),
            (Kind::Blue, "─╯"),
        ],
    );

    let y = canvas.push(false);
    let ascii = state.theme.ascii;
    let enter = theme::ascii_key("⏎", ascii);
    let (send, newline) = match state.enter {
        Enter::Post if enhanced => (
            enter.to_string(),
            format!("{}/ctrl-j", theme::ascii_key("⇧⏎", ascii)),
        ),
        Enter::Post => (enter.to_string(), "ctrl-j".to_string()),
        Enter::Newline if enhanced => (format!("ctrl-s/ctrl-{enter}"), enter.to_string()),
        Enter::Newline => ("ctrl-s".to_string(), enter.to_string()),
    };
    let mut actions = vec![
        (
            send.as_str(),
            if replying.is_some() { "reply" } else { "post" },
            if state.enter == Enter::Post {
                KeyCode::Enter
            } else {
                KeyCode::Char('s')
            },
            if state.enter == Enter::Post {
                KeyModifiers::empty()
            } else {
                KeyModifiers::CONTROL
            },
        ),
        (
            newline.as_str(),
            "newline",
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        ),
    ];
    let drafts_label = format!("drafts {}", c.saved);
    if c.saved > 0 {
        actions.push((
            "ctrl-o",
            drafts_label.as_str(),
            KeyCode::Char('o'),
            KeyModifiers::CONTROL,
        ));
    }
    actions.push(("esc", "cancel", KeyCode::Esc, KeyModifiers::empty()));
    let mut x = 0;
    for (key, word, code, modifiers) in actions {
        let text = if state.reveal_shortcuts {
            format!("{key} {word}")
        } else {
            word.to_string()
        };
        if x + text.width() > width {
            break;
        }
        let end = canvas.put(y, x, Kind::Dim, &text);
        if matches!(c.overlay, Overlay::None) {
            canvas.mouse_target(y, x, end, Act::Input(code, modifiers));
        }
        x = end + 2;
    }
    let target = c.target();
    let handle = state.handle_of(target).unwrap_or_else(|| "you".into());
    let as_handle = format!(" as @{handle}");
    let mut who: Vec<(Kind, &str)> = vec![(
        Kind::Dim,
        if replying.is_some() {
            "replying on "
        } else {
            "posting to "
        },
    )];
    if c.targets.len() > 1 {
        for (i, net) in c.targets.iter().enumerate() {
            if i > 0 {
                let joint = if i + 1 == c.targets.len() {
                    " and "
                } else {
                    ", "
                };
                who.push((Kind::Dim, joint));
            }
            who.push((net.accent(), net.label()));
        }
    } else {
        who.extend([(target.accent(), target.label()), (Kind::Dim, &as_handle)]);
    }
    let who_width: usize = who.iter().map(|(_, t)| t.width()).sum();
    let room = width.saturating_sub(x + 2);
    if who_width <= room {
        let status = util::truncate(&state.status, room.saturating_sub(who_width + 2));
        if !status.is_empty() {
            who.extend([(Kind::Dim, "  "), (Kind::Dim, status.as_str())]);
        }
        canvas.put_right(y, right, &who);
    } else if !state.status.is_empty() {
        let status = util::truncate(&state.status, room);
        canvas.put_right(y, right, &[(Kind::Dim, &status)]);
    }
    let (mut lines, _, targets) = canvas.finish_targeted();
    if matches!(state.detail, Detail::Plain | Detail::Minimal) {
        for line in &mut lines {
            for span in &mut line.spans {
                if matches!(span.kind, Kind::Blue | Kind::Line) {
                    span.text = span
                        .text
                        .chars()
                        .map(|ch| {
                            if "╭╮╰╯─│".contains(ch) {
                                ' '
                            } else {
                                ch
                            }
                        })
                        .collect();
                }
            }
        }
    }
    Bottom {
        lines,
        targets,
        cursor,
        rows: list_rows,
        dialog,
    }
}

/// Width of the quoted card in the composer, from design/src/screens/quote.js.
const QUOTE_CARD: usize = 64;

/** The quoted card's rows: the author line and the first two lines of the text, or the shared profile URL. */
fn quote_card(quote: &str, post: Option<&Post>, width: usize) -> Vec<Vec<(Kind, String)>> {
    let inner = width.saturating_sub(2);
    let room = inner.saturating_sub(2);
    let mut rows: Vec<Vec<(Kind, String)>> = match post {
        Some(post) => {
            let mut rows = vec![vec![
                (Kind::Bold, util::truncate(&post.author.name, room)),
                (
                    Kind::Dim,
                    util::truncate(
                        &format!(" @{} · {}", post.author.username, post.net.label()),
                        room.saturating_sub(post.author.name.width()),
                    ),
                ),
            ]];
            for line in util::wrap_text(&post.text, room).into_iter().take(2) {
                rows.push(vec![(Kind::Dim, line)]);
            }
            rows
        }
        None => vec![vec![(Kind::Dim, util::truncate(quote, room))]],
    };
    for row in &mut rows {
        let used: usize = row.iter().map(|(_, text)| text.width()).sum();
        row.insert(0, (Kind::Line, "│ ".into()));
        row.push((
            Kind::Line,
            format!("{}│", " ".repeat(inner.saturating_sub(1 + used))),
        ));
    }
    let bar = "─".repeat(inner);
    rows.insert(0, vec![(Kind::Line, format!("╭{bar}╮"))]);
    rows.push(vec![(Kind::Line, format!("╰{bar}╯"))]);
    rows
}

/**
The `[...]` list, anchored under its marker, or above it when the rows below would not hold the box.
Returns the lines, the screen column, the screen row, the width, and the index of the first shown item.
*/
fn overflow_popup(
    menu: &Overflow,
    full: usize,
    height: usize,
) -> Option<(Vec<StyledLine>, usize, usize, usize, usize)> {
    if full < 4 || menu.items.is_empty() {
        return None;
    }
    let need = menu.items.len() + 2;
    let below = height.saturating_sub(menu.anchor_y.saturating_add(1));
    let above = menu.anchor_y;
    let (place_below, room) = if below >= need {
        (true, below)
    } else if above >= need {
        (false, above)
    } else if below >= above && below >= 3 {
        (true, below)
    } else if above >= 3 {
        (false, above)
    } else {
        return None;
    };
    let visible = menu.items.len().min(room - 2);
    if visible == 0 {
        return None;
    }
    let first = menu
        .cursor
        .saturating_sub(visible - 1)
        .min(menu.items.len() - visible);
    let inner_needed = menu
        .items
        .iter()
        .map(|item| {
            item.parts
                .iter()
                .map(|(_, text)| text.width())
                .sum::<usize>()
        })
        .max()
        .unwrap_or(1)
        .max(1);
    let wide = (inner_needed + 2).clamp(4, full);
    let inner = wide - 2;
    let edge = |top: bool| {
        let (a, b) = if top { ('╭', '╮') } else { ('╰', '╯') };
        StyledLine::text(Kind::Line, &format!("{a}{}{b}", "─".repeat(inner)))
    };
    let mut lines = vec![edge(true)];
    for item in &menu.items[first..first + visible] {
        let mut spans = vec![render::Span {
            kind: Kind::Line,
            text: "│".into(),
        }];
        let mut used = 0;
        for (kind, text) in &item.parts {
            if used >= inner {
                break;
            }
            let text = util::truncate(text, inner - used);
            let width = text.width();
            if width == 0 {
                continue;
            }
            used += width;
            spans.push(render::Span { kind: *kind, text });
        }
        spans.push(render::Span {
            kind: Kind::Fg,
            text: " ".repeat(inner - used),
        });
        spans.push(render::Span {
            kind: Kind::Line,
            text: "│".into(),
        });
        let on = first + lines.len() - 1 == menu.cursor;
        lines.push(StyledLine {
            spans,
            selected: false,
            hl: on.then_some((1, wide - 1)),
            bar: false,
        });
    }
    lines.push(edge(false));
    let top = if place_below {
        menu.anchor_y + 1
    } else {
        menu.anchor_y - lines.len()
    };
    let left = menu.anchor_x.min(full.saturating_sub(wide));
    Some((lines, left, top, wide, first))
}

/// The cells of a rounded box titled `title`, `width` columns wide, one row per entry of `rows`.
/// `rows` in a titled rounded box `width` columns wide; cells past the box's inner width are cut.
fn boxed(title: &str, width: usize, rows: Vec<Vec<(Kind, String)>>) -> Vec<Vec<(Kind, String)>> {
    let inner = width.saturating_sub(2);
    let label = util::truncate(&format!(" {title} "), inner.saturating_sub(1));
    let mut out = vec![vec![
        (Kind::Line, "╭─".to_string()),
        (Kind::Bold, label.clone()),
        (
            Kind::Line,
            format!("{}╮", "─".repeat(inner.saturating_sub(1 + label.width()))),
        ),
    ]];
    for cells in rows {
        let mut row = vec![(Kind::Line, "│".to_string())];
        let mut used = 0;
        for (kind, text) in cells {
            let text = util::truncate(&text, inner - used);
            used += text.width();
            row.push((kind, text));
        }
        row.push((Kind::Fg, " ".repeat(inner - used)));
        row.push((Kind::Line, "│".into()));
        out.push(row);
    }
    out.push(vec![(Kind::Line, format!("╰{}╯", "─".repeat(inner)))]);
    out
}

/// Key and word pairs as `{b key} {dim word}`, two spaces apart, after a leading space.
fn key_cells(pairs: &[(&str, &str)], gap: &str, reveal: bool) -> Vec<(Kind, String)> {
    let mut cells = vec![(Kind::Fg, " ".to_string())];
    for (i, (key, word)) in pairs.iter().enumerate() {
        if i > 0 {
            cells.push((Kind::Fg, gap.to_string()));
        }
        if reveal {
            match key.split_once('/').filter(|_| key.len() == 3) {
                Some((a, b)) => cells.extend([
                    (Kind::Bold, a.to_string()),
                    (Kind::Dim, "/".to_string()),
                    (Kind::Bold, b.to_string()),
                ]),
                None => cells.push((Kind::Bold, key.to_string())),
            }
            cells.push((Kind::Fg, " ".to_string()));
        }
        cells.push((Kind::Dim, word.to_string()));
    }
    cells
}

/// The composer overlay's rows, `width` columns wide: the unsaved-draft dialog or the drafts list.
fn overlay_rows(
    overlay: &Overlay,
    width: usize,
    ascii: bool,
    reveal: bool,
) -> Vec<Vec<(Kind, String)>> {
    match overlay {
        Overlay::None => Vec::new(),
        Overlay::Ask(_) => boxed(
            "Unsaved draft",
            width,
            vec![
                vec![(Kind::Fg, " Would you like to save the draft?".into())],
                Vec::new(),
                key_cells(
                    &[("y", "save"), ("n", "discard"), ("esc", "keep writing")],
                    "   ",
                    reveal,
                ),
            ],
        ),
        Overlay::List { drafts, cursor } => {
            let now = crate::clock::utc_now();
            let start = (cursor + 1).saturating_sub(LIST_ROWS);
            let mut rows = vec![Vec::new()];
            for (i, draft) in drafts.iter().enumerate().skip(start).take(LIST_ROWS) {
                let net: Option<Network> = draft.targets.first().and_then(|s| s.parse().ok());
                let label = match (net, draft.targets.len()) {
                    (Some(net), n) if n > 1 => format!("{} +{}", net.label(), n - 1),
                    (Some(net), _) => net.label().to_string(),
                    (None, _) => "?".into(),
                };
                let first = draft.text.lines().next().unwrap_or("");
                let bar = if i == *cursor {
                    (Kind::Blue, "▍")
                } else {
                    (Kind::Fg, " ")
                };
                let cells: [(usize, Kind, String); 5] = [
                    (0, bar.0, bar.1.to_string()),
                    (2, Kind::Dim, util::ago_long(draft.updated_at, now)),
                    (13, Kind::Fg, util::truncate(&draft.target(), 19)),
                    (33, net.map_or(Kind::Dim, Network::accent), label),
                    (43, Kind::Fg, util::truncate(first, 38)),
                ];
                let mut row = Vec::new();
                let mut col = 0;
                for (at, kind, text) in cells {
                    row.push((Kind::Fg, " ".repeat(at.saturating_sub(col))));
                    col = at.max(col) + text.width();
                    row.push((kind, text));
                }
                rows.push(row);
            }
            rows.push(Vec::new());
            rows.push(key_cells(
                &[
                    ("j/k", "move"),
                    (theme::ascii_key("⏎", ascii), "load"),
                    ("d", "delete"),
                    ("esc", "close"),
                ],
                "  ",
                reveal,
            ));
            boxed("Drafts", width, rows)
        }
    }
}

/**
Splits text into rows no wider than `width`, breaking after a space when one fits. Rows are byte
ranges on grapheme boundaries, measured in display columns.
*/
fn wrap_ranges(text: &str, width: usize) -> Vec<(usize, usize)> {
    let graphemes: Vec<(usize, &str)> = text.grapheme_indices(true).collect();
    let at = |k: usize| graphemes.get(k).map_or(text.len(), |(i, _)| *i);
    let mut rows = Vec::new();
    let mut start = 0;
    let mut col = 0;
    let mut space = None;
    let mut k = 0;
    while k < graphemes.len() {
        let grapheme = graphemes[k].1;
        if grapheme == "\n" || grapheme == "\r\n" {
            rows.push((at(start), at(k)));
            k += 1;
            start = k;
            col = 0;
            space = None;
            continue;
        }
        let w = grapheme.width();
        if col + w > width && grapheme == " " {
            rows.push((at(start), at(k + 1)));
            k += 1;
            start = k;
            col = 0;
            space = None;
            continue;
        }
        if col + w > width && k > start {
            let cut = match space {
                Some(s) if s >= start && s < k => s + 1,
                _ => k,
            };
            rows.push((at(start), at(cut)));
            start = cut;
            col = graphemes[start..k].iter().map(|(_, g)| g.width()).sum();
            space = None;
            continue;
        }
        if grapheme == " " {
            space = Some(k);
        }
        col += w;
        k += 1;
    }
    rows.push((at(start), text.len()));
    rows
}

/// Whether the page should draw again without a key: the welcome water, or a picture still loading.
pub(crate) fn clock_runs(state: &State, ui: &Ui, width: usize, height: usize) -> bool {
    let welcome = onboard::showing(state)
        && matches!(ui.mode, Mode::Normal)
        && onboard::effects_visible(width, height);
    welcome
        || (state.detail == Detail::Rich
            && ui.previews.borrow().as_ref().is_some_and(Previews::busy))
}

/// The reader chrome the browser starts with. Demo pictures are attached while drawing on the page.
pub(crate) fn demo_ui() -> Ui {
    Ui {
        mode: Mode::Normal,
        scroll: Cell::new(0),
        scroll_key: RefCell::new(String::new()),
        scroll_right: Cell::new(0),
        scroll_right_key: RefCell::new(String::new()),
        anchor: RefCell::new(None),
        anchor_right: RefCell::new(None),
        split: Cell::new(false),
        overflow: Cell::new(0),
        enhanced: false,
        disclosure_keys: [false; 6],
        links: false,
        viewport: Cell::new((0, 0)),
        previews: RefCell::new(None),
        images: Cell::new(ImageMode::Off),
        unfurls: None,
        pages: RefCell::new(Vec::new()),
        started: Instant::now(),
        welcome: RefCell::new(onboard::Scene::default()),
        hits: RefCell::new(Hits::default()),
        focus: RefCell::new(None),
    }
}

/// One frame, including the second pane when the width splits.
pub(crate) fn render_frame(frame: &mut Frame, state: &mut State, ui: &Ui) {
    let full = usize::from(frame.area().width);
    let split = panes(state, ui, full);
    draw(frame, state, ui, split.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::fixtures;
    use unicode_width::UnicodeWidthStr;

    fn composer_for(text: &str, target: Network, overlay: Overlay) -> Composer {
        Composer {
            reply_to: None,
            quote: None,
            input: Input::from(text),
            targets: vec![target],
            raw: false,
            origin: None,
            base: Snapshot {
                targets: vec![target],
                ..Snapshot::default()
            },
            draft_id: None,
            autosaved: None,
            saved: 0,
            overlay,
        }
    }

    #[test]
    fn signing_out_of_devnull_keeps_the_key_note_in_the_status() {
        assert_eq!(
            signed_out_status(Network::Bluesky, false),
            "signed out of Bluesky"
        );
        assert_eq!(
            signed_out_status(Network::Devnull, true),
            "signed out of dev/null. The device key stays on disk. Revoke it at dev-null.sh/settings/devices"
        );
    }

    fn border_and_hint(text: &str, target: Network) -> (String, String) {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.status.clear();
        let lines = composer(
            &state,
            &composer_for(text, target, Overlay::None),
            100,
            false,
            None,
        )
        .lines;
        (lines[3].plain(), lines[4].plain())
    }

    #[test]
    fn composer_counts_against_the_target_network() {
        let (border, hint) = border_and_hint("", Network::Bluesky);
        assert!(border.ends_with(" 300 ─╯"), "{border}");
        assert!(
            hint.ends_with("posting to bluesky as @you.bsky.social"),
            "{hint}"
        );
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let (border, _) = border_and_hint(family, Network::Bluesky);
        assert!(border.ends_with(" 299 ─╯"), "{border}");
        let (border, _) = border_and_hint(family, Network::X);
        assert!(border.ends_with(" 278 ─╯"), "{border}");
        let (border, _) = border_and_hint("hi", Network::Threads);
        assert!(border.ends_with(" 498 ─╯"), "{border}");
        let (border, hint) = border_and_hint("", Network::Devnull);
        assert!(border.ends_with(" 4000 ─╯"), "{border}");
        assert!(hint.ends_with("posting to dev/null as @you"), "{hint}");
    }

    #[test]
    fn the_selection_bar_sits_at_terminal_col_1() {
        let mut line = StyledLine::text(Kind::Fg, " ╭───╮");
        line.selected = true;
        line.bar = true;
        let text: String = to_line_in(&line, &Theme::default(), 20, 2, Kind::Blue, true)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text.chars().position(|c| c == '▍'), Some(1));
        assert_eq!(text.chars().position(|c| c == '╭'), Some(3));
        assert_eq!(text.chars().count(), 20);
    }

    #[test]
    fn composer_status_follows_the_target() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.status = "draft restored · saved 2m ago".into();
        let lines = composer(
            &state,
            &composer_for("hi", Network::X, Overlay::None),
            100,
            false,
            None,
        )
        .lines;
        let hint = lines[4].plain();
        assert!(
            hint.ends_with("posting to x as @you  draft restored · saved 2m ago"),
            "{hint}"
        );
        let lines = composer(
            &state,
            &composer_for("hi", Network::X, Overlay::None),
            72,
            false,
            None,
        )
        .lines;
        let hint = lines[4].plain();
        assert!(hint.contains("posting to x as @you  draft"), "{hint}");
        assert!(hint.ends_with('…'), "{hint}");
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn typed(input: &mut Input, text: &str) {
        for ch in text.chars() {
            input.insert(ch);
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "socli-tui-{name}-{}-{:08x}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn input_moves_and_deletes_whole_graphemes() {
        let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
        let flag = "\u{1f1ee}\u{1f1f9}";
        let mut input = Input::from(&format!("e\u{301}{family}{flag}日本"));
        input.backspace();
        assert_eq!(input.text(), format!("e\u{301}{family}{flag}日"));
        input.left();
        input.left();
        assert_eq!(input.before_cursor(), format!("e\u{301}{family}"));
        input.backspace();
        assert_eq!(input.text(), format!("e\u{301}{flag}日"));
        input.delete();
        assert_eq!(input.text(), "e\u{301}日");
        input.left();
        assert_eq!(input.before_cursor(), "");
        input.right();
        assert_eq!(input.before_cursor(), "e\u{301}");
        assert_eq!(line_width(input.before_cursor()), 1);
        input.end();
        assert_eq!(line_width(input.before_cursor()), 3);
        input.insert('\u{301}');
        assert_eq!(input.text(), "e\u{301}日\u{301}");
        assert_eq!(input.cursor, input.text().len());
    }

    #[test]
    fn input_moves_and_deletes_by_word() {
        let mut input = Input::from("hello brave  new");
        input.word_left();
        assert_eq!(input.before_cursor(), "hello brave  ");
        input.word_left();
        assert_eq!(input.before_cursor(), "hello ");
        input.word_right();
        assert_eq!(input.before_cursor(), "hello brave");
        input.delete_word_back();
        assert_eq!(input.text(), "hello   new");
        assert_eq!(input.before_cursor(), "hello ");
        input.word_right();
        assert_eq!(input.before_cursor(), "hello   new");
        edit(&mut input, with(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "hello   ");
        edit(&mut input, with(KeyCode::Backspace, KeyModifiers::ALT));
        assert_eq!(input.text(), "");
        let mut input = Input::from("one two");
        edit(&mut input, with(KeyCode::Left, KeyModifiers::CONTROL));
        assert_eq!(input.before_cursor(), "one ");
        edit(&mut input, with(KeyCode::Left, KeyModifiers::ALT));
        assert_eq!(input.before_cursor(), "");
        edit(&mut input, with(KeyCode::Right, KeyModifiers::ALT));
        assert_eq!(input.before_cursor(), "one");
    }

    #[test]
    fn option_text_inserts_and_alt_b_f_skip_words() {
        let mut input = Input::from("one two three");
        edit(&mut input, with(KeyCode::Char('b'), KeyModifiers::ALT));
        assert_eq!(input.text(), "one two three");
        assert_eq!(input.before_cursor(), "one two ");
        edit(&mut input, with(KeyCode::Char('f'), KeyModifiers::ALT));
        assert_eq!(input.before_cursor(), "one two three");
        edit(&mut input, with(KeyCode::Char('@'), KeyModifiers::NONE));
        for ch in ['#', '`', '[', ']', 'ò', 'à', 'ù', 'è', '+'] {
            edit(&mut input, with(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(input.text(), "one two three@#`[]òàùè+");
        edit(&mut input, with(KeyCode::Char('ò'), KeyModifiers::ALT));
        assert_eq!(input.text(), "one two three@#`[]òàùè+ò");
        let flags = keyboard_flags();
        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn input_lines_keep_the_display_column() {
        let mut input = Input::from("abcd\nxy\n日本");
        input.up();
        assert_eq!(input.before_cursor(), "abcd\nxy");
        input.up();
        assert_eq!(input.before_cursor(), "ab");
        input.up();
        assert_eq!(input.before_cursor(), "ab");
        input.down();
        assert_eq!(input.before_cursor(), "abcd\nxy");
        input.down();
        assert_eq!(input.before_cursor(), "abcd\nxy\n日");
        input.down();
        assert_eq!(input.before_cursor(), "abcd\nxy\n日");
        input.home();
        assert_eq!(input.before_cursor(), "abcd\nxy\n");
        input.end();
        assert_eq!(input.before_cursor(), "abcd\nxy\n日本");
        input.up();
        edit(&mut input, with(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert_eq!(input.before_cursor(), "abcd\n");
        edit(&mut input, with(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(input.before_cursor(), "abcd\nxy");
        edit(&mut input, with(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "abcd\n\n日本");
        assert_eq!(input.before_cursor(), "abcd\n");
        edit(&mut input, key(KeyCode::Home));
        edit(&mut input, key(KeyCode::End));
        assert_eq!(input.before_cursor(), "abcd\n");
        edit(&mut input, with(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "abcd\n\n日本");
    }

    #[test]
    fn undo_steps_group_typing_and_a_new_edit_drops_redo() {
        let mut input = Input::new();
        typed(&mut input, "hello world");
        input.undo();
        assert_eq!(input.text(), "hello ");
        input.undo();
        assert_eq!(input.text(), "");
        input.redo();
        assert_eq!(input.text(), "hello ");
        edit(&mut input, with(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "hello world");
        edit(&mut input, with(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "hello ");
        edit(
            &mut input,
            with(
                KeyCode::Char('z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        );
        assert_eq!(input.text(), "hello world");
        edit(
            &mut input,
            with(
                KeyCode::Char('Z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        );
        assert_eq!(input.text(), "hello world");

        let mut input = Input::new();
        typed(&mut input, "ab");
        input.left();
        typed(&mut input, "c");
        assert_eq!(input.text(), "acb");
        input.undo();
        assert_eq!((input.text(), input.before_cursor()), ("ab", "a"));
        input.insert_str("one\ntwo");
        assert_eq!(input.text(), "aone\ntwob");
        input.undo();
        assert_eq!(input.text(), "ab");
        input.insert('x');
        input.redo();
        assert_eq!(input.text(), "axb");
        input.undo();
        input.undo();
        assert_eq!(input.text(), "");
    }

    #[test]
    fn undo_keeps_the_last_hundred_steps() {
        let mut input = Input::new();
        for _ in 0..150 {
            input.insert_str("a");
        }
        for _ in 0..120 {
            input.undo();
        }
        assert_eq!(input.text(), "a".repeat(50));
    }

    #[test]
    fn a_paste_is_cleaned_and_never_acts_as_a_key() {
        assert_eq!(clean_paste("a\r\nb\rc\td\x1b[31me\x03"), "a\nb\nc    de");
        let mut ui = test_ui();
        ui.mode = Mode::Compose(Box::new(composer_for("", Network::X, Overlay::None)));
        paste(&mut ui, "line one\r\n:quit\n\x1b[31mred\x03");
        let Mode::Compose(c) = &mut ui.mode else {
            panic!("the paste left the composer");
        };
        assert_eq!(c.input.text(), "line one\n:quit\nred");
        c.input.undo();
        assert_eq!(c.input.text(), "");
        c.overlay = Overlay::Ask(After::Leave);
        paste(&mut ui, "dropped");
        let Mode::Compose(c) = &ui.mode else {
            panic!("the paste left the composer");
        };
        assert_eq!(c.input.text(), "");

        let mut ui = test_ui();
        paste(&mut ui, ":quit\n");
        assert!(matches!(ui.mode, Mode::Normal));
        ui.mode = Mode::Command {
            input: Input::new(),
            pick: 2,
        };
        paste(&mut ui, "quit\r\nnow\x1b");
        let Mode::Command { input, pick } = &ui.mode else {
            panic!("the paste left the command line");
        };
        assert_eq!((input.text(), *pick), ("quit now", 0));
        ui.mode = Mode::Search(Input::new());
        paste(&mut ui, "a\nb");
        let Mode::Search(input) = &ui.mode else {
            panic!("the paste left search");
        };
        assert_eq!(input.text(), "a b");
    }

    #[test]
    fn a_composer_paste_drops_whole_escape_sequences() {
        let mut ui = test_ui();
        ui.mode = Mode::Compose(Box::new(composer_for("", Network::X, Overlay::None)));
        paste(&mut ui, "safe\x1b[31mred\x1b[0m\r\n:quit\n\x03end");
        let Mode::Compose(c) = &ui.mode else {
            panic!("the paste left the composer");
        };
        assert_eq!(c.input.text(), "safered\n:quit\nend");
    }

    #[test]
    fn the_enter_setting_picks_the_submit_and_newline_keys() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.reveal_shortcuts = true;
        let targets = [Network::X];
        let mut c = composer_for("hi", Network::X, Overlay::None);
        let ctrl = |code| with(code, KeyModifiers::CONTROL);
        let shift_enter = with(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(state.enter, Enter::Post);
        assert_eq!(
            composer_key(&mut state, &mut c, key(KeyCode::Enter), false, &targets),
            Step::Submit
        );
        assert_eq!(
            composer_key(
                &mut state,
                &mut c,
                ctrl(KeyCode::Char('s')),
                false,
                &targets
            ),
            Step::Stay
        );
        assert_eq!(
            composer_key(
                &mut state,
                &mut c,
                ctrl(KeyCode::Char('j')),
                false,
                &targets
            ),
            Step::Stay
        );
        assert_eq!(
            composer_key(&mut state, &mut c, shift_enter, true, &targets),
            Step::Stay
        );
        assert_eq!(c.input.text(), "hi\n\n");

        state.enter = Enter::Newline;
        let mut c = composer_for("hi", Network::X, Overlay::None);
        assert_eq!(
            composer_key(&mut state, &mut c, key(KeyCode::Enter), false, &targets),
            Step::Stay
        );
        assert_eq!(
            composer_key(&mut state, &mut c, shift_enter, true, &targets),
            Step::Stay
        );
        assert_eq!(c.input.text(), "hi\n\n");
        assert_eq!(
            composer_key(
                &mut state,
                &mut c,
                ctrl(KeyCode::Char('s')),
                false,
                &targets
            ),
            Step::Submit
        );
        assert_eq!(
            composer_key(&mut state, &mut c, ctrl(KeyCode::Enter), true, &targets),
            Step::Submit
        );
        assert_eq!(
            composer_key(&mut state, &mut c, ctrl(KeyCode::Enter), false, &targets),
            Step::Stay
        );

        let hint = |state: &State, enhanced| {
            composer(
                state,
                &composer_for("hi", Network::X, Overlay::None),
                100,
                enhanced,
                None,
            )
            .lines[4]
                .plain()
        };
        assert!(hint(&state, false).starts_with("ctrl-s post  ⏎ newline  esc cancel"));
        assert!(hint(&state, true).starts_with("ctrl-s/ctrl-⏎ post  ⏎ newline"));
        state.enter = Enter::Post;
        assert!(hint(&state, false).starts_with("⏎ post  ctrl-j newline  esc cancel"));
        assert!(hint(&state, true).starts_with("⏎ post  ⇧⏎/ctrl-j newline"));
    }

    #[test]
    fn tab_retargets_what_the_composer_sends() {
        let dir = temp("retarget");
        let store = dir.join("drafts.json");
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let targets = [Network::X, Network::Bluesky];
        let mut c = composer_for("hi", Network::X, Overlay::None);
        composer_key(&mut state, &mut c, key(KeyCode::Tab), false, &targets);
        assert_eq!(c.intent().targets, [Network::Bluesky]);
        assert!(submit(&mut state, &mut Clients::empty(), &mut c, &store));
        assert_eq!(state.status, "posted to Bluesky");
        assert!(drafts::load_from(&store).unwrap().is_empty());

        let mut reply = composer_for("hi", Network::X, Overlay::None);
        reply.reply_to = Some("x:p1".into());
        reply.quote = Some("x:p2".into());
        reply.draft_id = Some("d1".into());
        let intent = reply.intent();
        assert_eq!(intent.reply_to.as_deref(), Some("x:p1"));
        assert_eq!(intent.quote.as_deref(), Some("x:p2"));
        assert_eq!(intent.draft_id.as_deref(), Some("d1"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_draft_write_keeps_the_composer_and_its_text() {
        let dir = temp("kept");
        let store = dir.join("drafts.json");
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let long = "a".repeat(290);
        let mut c = composer_for(&long, Network::X, Overlay::None);
        c.targets = vec![Network::X, Network::Bluesky];
        drafts::fail_after(0);
        assert!(!submit(&mut state, &mut Clients::empty(), &mut c, &store));
        assert!(
            state.status.contains("nothing was sent"),
            "{}",
            state.status
        );
        assert_eq!(c.input.text(), long);
        assert_eq!(c.targets, [Network::X, Network::Bluesky]);

        drafts::fail_after(1);
        assert!(!submit(&mut state, &mut Clients::empty(), &mut c, &store));
        assert!(
            state.status.contains("couldn't update the draft"),
            "{}",
            state.status
        );
        assert_eq!(c.input.text(), long);
        assert_eq!(c.targets, [Network::X]);
        let id = c.draft_id.clone().unwrap();
        let kept = drafts::load_from(&store).unwrap();
        assert_eq!(
            (kept[0].id.as_str(), kept[0].sending.len()),
            (id.as_str(), 2)
        );

        c.targets = vec![Network::X, Network::Bluesky];
        assert!(submit(&mut state, &mut Clients::empty(), &mut c, &store));
        assert!(state.status.ends_with(" · draft saved"), "{}", state.status);
        let kept = drafts::load_from(&store).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(
            (kept[0].id.as_str(), kept[0].targets.as_slice()),
            (id.as_str(), ["x".to_string()].as_slice())
        );
        assert!(kept[0].sending.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_restored_draft_that_was_sending_asks_for_a_check() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut draft = Draft::new(vec!["bluesky".into()], None, None);
        draft.text = "maybe posted".into();
        draft.sending = vec!["bluesky".into(), "x".into()];
        let overlay = Overlay::List {
            drafts: vec![draft],
            cursor: 0,
        };
        let mut c = composer_for("", Network::X, overlay);
        composer_key(
            &mut state,
            &mut c,
            key(KeyCode::Enter),
            false,
            &[Network::X],
        );
        assert_eq!(
            state.status,
            "last send may have reached X, Bluesky · check before posting again"
        );
        assert_eq!(c.input.text(), "maybe posted");
        assert_eq!(c.targets, [Network::Bluesky]);
        assert!(!c.dirty());
    }

    #[test]
    fn dirty_compares_text_targets_reply_and_quote() {
        let dir = temp("dirty");
        let store = dir.join("drafts.json");
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let targets = [Network::X, Network::Bluesky];
        let mut c = composer_for("", Network::X, Overlay::None);
        assert!(!c.dirty());
        composer_key(&mut state, &mut c, key(KeyCode::Char('a')), false, &targets);
        assert!(c.dirty());
        composer_key(&mut state, &mut c, key(KeyCode::Backspace), false, &targets);
        assert!(!c.dirty());
        c.autosave(&state, &store).unwrap();
        assert!(!store.exists());

        composer_key(&mut state, &mut c, key(KeyCode::Tab), false, &targets);
        assert!(c.dirty());
        c.autosave(&state, &store).unwrap();
        let kept = drafts::load_from(&store).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].targets, ["bluesky"]);
        assert_eq!(c.draft_id.as_deref(), Some(kept[0].id.as_str()));

        let mut loaded = composer_for("", Network::X, Overlay::None);
        let mut draft = Draft::new(vec!["x".into()], None, None);
        draft.text = "A".into();
        loaded.load(draft);
        assert!(!loaded.dirty());
        composer_key(
            &mut state,
            &mut loaded,
            key(KeyCode::Char('B')),
            false,
            &targets,
        );
        assert!(loaded.dirty());
        composer_key(
            &mut state,
            &mut loaded,
            key(KeyCode::Backspace),
            false,
            &targets,
        );
        assert!(!loaded.dirty());
        loaded.quote = Some("x:p1".into());
        assert!(loaded.dirty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn quote_composer_counts_the_link_for_the_strictest_target() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.status.clear();
        let quoted = state.posts["threads:p10"].clone();
        let mut c = composer_for("<b>hi</b>", Network::Threads, Overlay::None);
        c.quote = Some(quoted.id.clone());
        c.targets = vec![Network::X, Network::Threads];
        let (net, left) = c.room(&state);
        assert_eq!((net, left), (Network::X, 280 - 4 - 25));
        c.raw = true;
        assert_eq!(c.room(&state).1, 280 - 9 - 25);
        let lines: Vec<String> = composer(&state, &c, 100, false, None)
            .lines
            .iter()
            .map(StyledLine::plain)
            .collect();
        assert!(lines[0].starts_with("╭─ Quote post ─"), "{}", lines[0]);
        assert_eq!(
            lines[1].trim_end_matches(['│', ' ']),
            "│ to  ● X  ● Threads"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains(&format!("@{} · threads", quoted.author.username))),
            "{lines:#?}"
        );
        assert!(
            lines
                .last()
                .is_some_and(|l| l.ends_with("posting to x and threads")),
            "{lines:#?}"
        );
    }

    #[test]
    fn full_screen_composer_fills_the_rows_it_gets() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.status.clear();
        let c = composer_for("hello", Network::Bluesky, Overlay::None);
        let block = composer(&state, &c, 100, false, Some(40));
        assert_eq!(block.lines.len(), 40);
        assert_eq!(block.cursor, Some((7, 1)));
        assert!(block.lines[38].plain().starts_with("╰"));
    }

    #[test]
    fn the_composer_cursor_follows_wrapped_wide_text() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.status.clear();
        let mut c = composer_for("日本語 日本語 日本", Network::Bluesky, Overlay::None);
        let block = composer(&state, &c, 14, false, Some(40));
        assert_eq!(block.cursor, Some((6, 3)));
        c.input.word_left();
        let block = composer(&state, &c, 14, false, Some(40));
        assert_eq!(block.cursor, Some((4, 3)));
        c.input.home();
        c.input.word_right();
        let block = composer(&state, &c, 14, false, Some(40));
        assert_eq!(block.cursor, Some((4, 1)));
        c.input.cursor = "日本語 ".len();
        let block = composer(&state, &c, 14, false, Some(40));
        assert_eq!(block.cursor, Some((2, 2)));
    }

    #[test]
    fn composer_keys_close_through_the_draft_dialog() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let targets = [Network::X, Network::Bluesky];
        let mut c = composer_for("", Network::X, Overlay::None);
        let mut step =
            |c: &mut Composer, code| composer_key(&mut state, c, key(code), false, &targets);
        assert_eq!(step(&mut c, KeyCode::Enter), Step::Stay);
        assert_eq!(step(&mut c, KeyCode::Tab), Step::Stay);
        assert_eq!(c.targets, [Network::Bluesky]);
        assert_eq!(step(&mut c, KeyCode::Char('h')), Step::Stay);
        assert_eq!(step(&mut c, KeyCode::Esc), Step::Stay);
        assert!(matches!(c.overlay, Overlay::Ask(After::Leave)));
        assert_eq!(
            step(&mut c, KeyCode::Char('n')),
            Step::Closed {
                saved: false,
                quit: false
            }
        );
        assert_eq!(step(&mut c, KeyCode::Enter), Step::Submit);
        let mut clean = composer_for("", Network::X, Overlay::None);
        assert_eq!(
            step(&mut clean, KeyCode::Esc),
            Step::Closed {
                saved: false,
                quit: false
            }
        );
    }

    #[test]
    fn posted_status_names_successes_then_failures() {
        let results = vec![
            (Network::X, Err("x is down".to_string())),
            (Network::Threads, Ok("demo".to_string())),
        ];
        assert_eq!(
            posted_status(&results),
            "posted to Threads · x failed: x is down"
        );
        let results = vec![
            (Network::X, Ok(String::new())),
            (Network::Threads, Ok(String::new())),
        ];
        assert_eq!(posted_status(&results), "posted to X and Threads");
    }

    fn test_ui() -> Ui {
        demo_ui()
    }

    #[test]
    fn the_sign_in_form_draws_and_steps_to_the_next_field() {
        let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        ui.mode = Mode::SignIn(SignInForm::open(Network::Bluesky));
        press(&mut state, &mut ui, KeyCode::Char('a'));
        press(&mut state, &mut ui, KeyCode::Enter);
        let Mode::SignIn(form) = &ui.mode else {
            panic!("the form closed");
        };
        assert_eq!(form.cursor, 1);
        assert_eq!(form.fields[0].value, "a");
        let buffer = frame(&mut state, &ui, 80, 24);
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Sign in to Bluesky"), "{text}");
        assert!(text.contains("Username"), "{text}");
        assert!(text.contains("App password"), "{text}");
        assert!(text.contains(FIELD_CURSOR), "{text}");
        assert!(!text.contains('▌'), "{text}");
    }

    #[test]
    fn the_x_sign_in_screen_approves_the_socli_app() {
        let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        ui.mode = Mode::SignIn(SignInForm::open(Network::X));
        state.detail = Detail::Rich;
        let buffer = frame(&mut state, &ui, 92, 36);
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Approve SoCLI on X"), "{text}");
        assert!(text.contains("Enter opens X"), "{text}");
        assert!(!text.contains("this build has no SoCLI X app"), "{text}");
        assert!(!text.contains("Client id"), "{text}");
        assert!(!text.contains("console.x.com"), "{text}");
        assert!(!text.contains("Create an app"), "{text}");
        assert!(!text.contains("socli login x"), "{text}");
        assert!(!text.contains("auth_token"), "{text}");
        assert!(!text.contains("This page cannot"), "{text}");
        assert!(
            !text.contains("this sign-in continues in the browser"),
            "{text}"
        );
        assert!(!text.contains("127.0.0.1:8741"), "{text}");
        let rows: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect();
        let column_of = |needle: &str| {
            rows.iter()
                .find_map(|row| row.find(needle).map(|at| row[..at].width()))
        };
        let column = 2 + render::margin_for(92, Detail::Rich);
        assert_eq!(column_of("▄██████▄"), Some(column), "{rows:#?}");
        assert_eq!(column_of("Sign in to X"), Some(column), "{rows:#?}");
        assert_eq!(column_of("Sign-in method"), Some(column), "{rows:#?}");
        assert_eq!(column_of("Approve SoCLI on X"), Some(column), "{rows:#?}");
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(matches!(ui.mode, Mode::SignIn(_)));
        let buffer = frame(&mut state, &ui, 92, 36);
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(
            text.contains("this sign-in continues in the browser"),
            "{text}"
        );
        assert!(!text.contains("Create an app"), "{text}");
        assert!(!text.contains("console.x.com"), "{text}");
    }

    fn sign_in_text(net: Network, width: u16, height: u16) -> String {
        let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
        state.detail = Detail::Rich;
        let mut ui = test_ui();
        ui.mode = Mode::SignIn(SignInForm::open(net));
        let buffer = frame(&mut state, &ui, width, height);
        buffer.content().iter().map(|cell| cell.symbol()).collect()
    }

    #[test]
    fn the_mastodon_sign_in_screen_opens_the_server_approval() {
        let mut ui = test_ui();
        ui.mode = Mode::SignIn(SignInForm::open(Network::Mastodon));
        let Mode::SignIn(form) = &ui.mode else {
            panic!("the form closed");
        };
        assert_eq!(form.value("Server"), "mastodon.social");
        let text = sign_in_text(Network::Mastodon, 92, 36);
        assert!(text.contains("mastodon.social"), "{text}");
        assert!(text.contains("Approve SoCLI"), "{text}");
        assert!(text.contains("Server"), "{text}");
        assert!(!text.contains("Access token"), "{text}");
    }

    #[test]
    fn the_threads_sign_in_screen_approves_the_socli_app() {
        let text = sign_in_text(Network::Threads, 92, 36);
        assert!(text.contains("Approve SoCLI on Threads"), "{text}");
        assert!(text.contains("Enter opens Threads"), "{text}");
        assert!(
            !text.contains("this build has no SoCLI Threads app"),
            "{text}"
        );
        assert!(!text.contains("Pass the token"), "{text}");
        assert!(!text.contains("Threads Tester"), "{text}");
        assert!(!text.contains("developers.facebook.com"), "{text}");
        assert!(!text.contains("Access token"), "{text}");
    }

    #[test]
    fn the_devnull_sign_in_screen_asks_for_the_callback() {
        let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
        state.detail = Detail::Rich;
        let mut ui = test_ui();
        ui.mode = Mode::SignIn(SignInForm::open(Network::Devnull));
        let buffer = frame(&mut state, &ui, 92, 40);
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("devnull://"), "{text}");
        assert!(text.contains("Paste the callback"), "{text}");
        assert!(text.contains("Location"), "{text}");
        assert!(text.contains("Callback"), "{text}");
        assert!(!text.contains("stays in the terminal"), "{text}");
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(matches!(ui.mode, Mode::SignIn(_)));
        let buffer = frame(&mut state, &ui, 92, 40);
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Paste the full devnull:// address"), "{text}");
    }

    fn press(state: &mut State, ui: &mut Ui, code: KeyCode) {
        handle_key(state, &mut Clients::empty(), ui, key(code)).unwrap();
    }

    fn cursor_of(ui: &Ui) -> Option<(PickKind, usize)> {
        match &ui.mode {
            Mode::Pick(picker) => Some((picker.kind, picker.cursor)),
            _ => None,
        }
    }

    #[test]
    fn the_network_picker_opens_moves_closes_and_picks() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('A'));
        assert_eq!(cursor_of(&ui), Some((PickKind::Network, 0)));
        press(&mut state, &mut ui, KeyCode::Char('j'));
        assert_eq!(cursor_of(&ui), Some((PickKind::Network, 1)));
        press(&mut state, &mut ui, KeyCode::BackTab);
        press(&mut state, &mut ui, KeyCode::Char('k'));
        assert_eq!(cursor_of(&ui), Some((PickKind::Network, 5)));
        press(&mut state, &mut ui, KeyCode::Esc);
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.scope(), Scope::All);

        press(&mut state, &mut ui, KeyCode::Char('A'));
        press(&mut state, &mut ui, KeyCode::Char('A'));
        assert!(matches!(ui.mode, Mode::Normal));

        press(&mut state, &mut ui, KeyCode::Char('A'));
        press(&mut state, &mut ui, KeyCode::Char('3'));
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.scope(), Scope::One(Network::Devnull));

        press(&mut state, &mut ui, KeyCode::Char('A'));
        assert_eq!(cursor_of(&ui), Some((PickKind::Network, 3)));
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Enter);
        assert_eq!(state.scope(), Scope::One(Network::Threads));
    }

    #[test]
    fn a_network_that_is_not_signed_in_keeps_the_picker_open() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.nets.retain(|info| info.net != Network::Threads);
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('A'));
        press(&mut state, &mut ui, KeyCode::Char('4'));
        assert!(matches!(ui.mode, Mode::Pick(_)));
        assert_eq!(
            state.status,
            "threads is not signed in. Run `socli login threads`"
        );
    }

    #[test]
    fn a_steps_the_scope_and_ctrl_a_still_asks() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('a'));
        assert_eq!(state.scope(), Scope::One(Network::X));
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        handle_key(&mut state, &mut Clients::empty(), &mut ui, ctrl_a).unwrap();
        assert!(matches!(ui.mode, Mode::Ask { .. }));
        assert_eq!(state.scope(), Scope::One(Network::X));
    }

    #[test]
    fn the_detail_picker_opens_and_closes_without_changing_the_level() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let level = state.detail;
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('D'));
        let expected = Detail::ALL.iter().position(|l| *l == level).unwrap();
        assert_eq!(cursor_of(&ui), Some((PickKind::Detail, expected)));
        press(&mut state, &mut ui, KeyCode::Char('9'));
        press(&mut state, &mut ui, KeyCode::Char('x'));
        assert!(matches!(ui.mode, Mode::Pick(_)));
        press(&mut state, &mut ui, KeyCode::Esc);
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.detail, level);

        state.open_settings();
        press(&mut state, &mut ui, KeyCode::Char('D'));
        assert_eq!(cursor_of(&ui).map(|(kind, _)| kind), Some(PickKind::Detail));
        press(&mut state, &mut ui, KeyCode::Char('D'));
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.detail, level);
    }

    fn theme_picker(state: &mut State, ui: &mut Ui) {
        let mut clients = Clients::empty();
        handle_key(state, &mut clients, ui, key(KeyCode::Char(':'))).unwrap();
        for ch in "theme".chars() {
            handle_key(state, &mut clients, ui, key(KeyCode::Char(ch))).unwrap();
        }
        handle_key(state, &mut clients, ui, key(KeyCode::Enter)).unwrap();
    }

    #[test]
    fn colon_theme_opens_the_picker_on_the_current_row() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.scheme = Scheme::parse("nord-dark").unwrap();
        let mut ui = test_ui();
        theme_picker(&mut state, &mut ui);
        let at = Scheme::list()
            .iter()
            .position(|s| *s == state.scheme.base())
            .unwrap();
        assert_eq!(cursor_of(&ui), Some((PickKind::Theme, at)));
    }

    #[test]
    fn the_theme_picker_previews_live_and_escape_restores() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        theme_picker(&mut state, &mut ui);
        assert_eq!(state.scheme, Scheme::Auto);
        let before = state.theme.rgb(Kind::Blue);
        press(&mut state, &mut ui, KeyCode::Char('j'));
        assert_eq!(state.scheme, Scheme::Native);
        assert!(state.theme.native);
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Char('j'));
        assert_eq!(state.scheme, Scheme::list()[4]);
        assert!(!state.theme.native);
        assert_ne!(state.theme.rgb(Kind::Blue), before);
        press(&mut state, &mut ui, KeyCode::Char('5'));
        assert_eq!(state.scheme, Scheme::list()[4]);
        press(&mut state, &mut ui, KeyCode::Esc);
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.scheme, Scheme::Auto);
        assert_eq!(state.theme.rgb(Kind::Blue), before);
    }

    #[test]
    fn enter_on_the_settings_theme_row_opens_the_picker() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        state.open_settings();
        state.settings_row = settings::ROWS
            .iter()
            .position(|row| matches!(row, Row::Theme))
            .unwrap();
        press(&mut state, &mut ui, KeyCode::Enter);
        assert_eq!(cursor_of(&ui), Some((PickKind::Theme, 0)));
        press(&mut state, &mut ui, KeyCode::Esc);
        assert!(matches!(ui.mode, Mode::Normal));
    }

    #[test]
    fn a_tall_theme_picker_scrolls_to_the_cursor() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        theme_picker(&mut state, &mut ui);
        for _ in 0..20 {
            press(&mut state, &mut ui, KeyCode::Char('j'));
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
        let frame = terminal
            .draw(|frame| draw(frame, &state, &ui, None))
            .unwrap();
        let text: Vec<String> = (0..16)
            .map(|y| {
                (0..100)
                    .map(|x| frame.buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let label = Scheme::list()[20].word();
        assert!(text.iter().any(|line| line.contains(&label)), "{text:#?}");
        assert!(text.iter().any(|line| line.contains("21/31")), "{text:#?}");
    }

    /**
    Draws at 104×44 and returns the selected post, the first body row of its block and the
    first body row on screen.
    */
    fn drawn(state: &State, ui: &Ui) -> (Option<String>, usize, usize) {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
        terminal.draw(|frame| draw(frame, state, ui, None)).unwrap();
        let width = render::inner_width_for(104, state.detail);
        let focus = screens::linked_body(state, width, true).focus.unwrap();
        (state.selected_post_id(), focus.start, ui.scroll.get())
    }

    #[test]
    fn f_and_slash_search_and_shift_f_filters() {
        for typed in ['f', '/'] {
            let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
            let mut ui = test_ui();
            assert!(search_key(&mut state, &mut ui, key(KeyCode::Char(typed))));
            assert!(matches!(ui.mode, Mode::Search(_)), "{typed}");
            assert_eq!(state.section, Section::Search);
            state.apply(Cmd::Back, &mut Clients::empty()).unwrap();
            assert_eq!(state.section, Section::Home, "{typed}");
        }
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        let shift_f = KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT);
        assert!(search_key(&mut state, &mut ui, shift_f));
        assert!(matches!(ui.mode, Mode::Filter { .. }));
        assert_eq!(state.section, Section::Home);
        let ctrl_f = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert!(!search_key(&mut state, &mut test_ui(), ctrl_f));
        let alt_f = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT);
        assert!(!search_key(&mut state, &mut test_ui(), alt_f));
    }

    #[test]
    fn alt_b_does_not_bookmark_and_alt_f_stays_on_the_feed() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut clients = Clients::empty();
        let mut ui = test_ui();
        let section = state.section;
        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('b'), KeyModifiers::ALT),
        )
        .unwrap();
        assert!(state.status_error.is_none());
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.section, section);
        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('f'), KeyModifiers::ALT),
        )
        .unwrap();
        assert!(state.status_error.is_none());
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.section, section);
    }

    #[test]
    fn colon_runs_the_picked_command() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut clients = Clients::empty();
        let mut ui = test_ui();
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char(':'))).unwrap();
        assert!(matches!(ui.mode, Mode::Command { .. }));
        for ch in "noti".chars() {
            handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char(ch))).unwrap();
        }
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Enter)).unwrap();
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.section, crate::model::Section::Notifications);
    }

    #[test]
    fn tab_completes_and_escape_closes_the_command_line() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut clients = Clients::empty();
        let mut ui = test_ui();
        let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
        handle_key(&mut state, &mut clients, &mut ui, ctrl_k).unwrap();
        for ch in "sh".chars() {
            handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char(ch))).unwrap();
        }
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Tab)).unwrap();
        match &ui.mode {
            Mode::Command { input, .. } => assert_eq!(input.text(), "share "),
            _ => panic!("the command line closed"),
        }
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Esc)).unwrap();
        assert!(matches!(ui.mode, Mode::Normal));
    }

    #[test]
    fn completion_keeps_the_arguments() {
        assert_eq!(completed("rep 2 hi", "reply", "[n] <text>"), "reply 2 hi");
        assert_eq!(completed("sh", "share", "[n]"), "share ");
        assert_eq!(completed("ho", "home", ""), "home");
        assert_eq!(command_line("sh", Some(&("share", "[n]", ""))), "share ");
        assert_eq!(command_line("like 2", Some(&("like", "[n]", ""))), "like 2");
        assert_eq!(command_matches("no").len(), 1);
    }

    #[test]
    fn the_picked_suggestion_carries_the_bar() {
        let matches = command_matches("s");
        let lines = command_list(&matches, 1, 60);
        assert_eq!(lines.len(), matches.len() + 2);
        assert!(lines[2].plain().starts_with("│▍ "), "{}", lines[2].plain());
        assert!(lines[1].plain().starts_with("│  "), "{}", lines[1].plain());
        assert!(lines.iter().all(|line| line.plain().width() == 60));
    }

    #[test]
    fn page_keys_move_the_selection_and_the_viewport_a_screen() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let ui = test_ui();
        let (first, top, _) = drawn(&state, &ui);
        let height = ui.viewport.get().1;
        assert_eq!(
            ui.viewport.get(),
            (render::inner_width_for(104, state.detail), 36)
        );
        assert!(page_key(&mut state, &ui, key(KeyCode::PageDown)));
        let (paged, block, offset) = drawn(&state, &ui);
        assert_ne!(paged, first);
        assert!(block >= top + height - 2, "{block}");
        assert_eq!(offset, block + 1);
        assert!(page_key(&mut state, &ui, key(KeyCode::Char(' '))));
        let (again, further, _) = drawn(&state, &ui);
        assert_ne!(again, paged);
        assert!(further >= block + height - 2, "{further}");
        assert!(page_key(&mut state, &ui, key(KeyCode::PageUp)));
        let (back, earlier, offset) = drawn(&state, &ui);
        assert_ne!(back, again);
        assert!(earlier + height - 2 <= further, "{earlier}");
        assert_eq!(offset, earlier + 1);
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(page_rows(ctrl_d, height), Some(18));
        assert!(!page_key(&mut state, &ui, key(KeyCode::Char('j'))));
    }

    #[test]
    fn follow_keeps_the_block_and_one_row_of_context() {
        let span = RowSpan { start: 30, end: 36 };
        let text = |_: usize| false;
        assert_eq!(follow_past_blanks(0, Some(span), 20, 100, text), 17);
        let span = RowSpan { start: 4, end: 8 };
        assert_eq!(follow_past_blanks(17, Some(span), 20, 100, text), 3);
        assert_eq!(follow_past_blanks(50, None, 20, 30, text), 10);
        let span = RowSpan { start: 10, end: 16 };
        let blank = |row| row == 9 || row == 16;
        // The block already fits, so the blank context rows stay put.
        assert_eq!(follow_past_blanks(0, Some(span), 20, 40, blank), 0);
        // Scrolling up to it pulls the line before the blank onto the top edge.
        assert_eq!(follow_past_blanks(30, Some(span), 20, 40, blank), 8);
        // No room for that line: the blank is dropped and the block's own row is the edge.
        assert_eq!(follow_past_blanks(30, Some(span), 8, 40, blank), 10);
        // A blank on the document's first row is the list pad, and it stays.
        let edge = RowSpan { start: 1, end: 5 };
        assert_eq!(
            follow_past_blanks(12, Some(edge), 10, 20, |row| row == 0),
            0
        );
    }

    #[test]
    fn a_tall_block_keeps_an_offset_inside_it() {
        let span = RowSpan { start: 30, end: 70 };
        let text = |_: usize| false;
        assert_eq!(follow_past_blanks(0, Some(span), 10, 100, text), 29);
        assert_eq!(follow_past_blanks(80, Some(span), 10, 100, text), 61);
        assert_eq!(follow_past_blanks(40, Some(span), 10, 100, text), 40);
        assert_eq!(follow_past_blanks(65, Some(span), 10, 100, text), 61);
        let fitted = RowSpan { start: 30, end: 36 };
        assert_eq!(follow_past_blanks(0, Some(fitted), 20, 100, text), 17);
    }

    #[test]
    fn pad_window_keeps_the_pads_at_the_ends() {
        let window = pad_window(0, 1, 100, 30);
        assert_eq!((window.top, window.lines, window.bottom), (1, 0..29, 0));
        let window = pad_window(1, 1, 100, 30);
        assert_eq!((window.top, window.lines, window.bottom), (0, 0..30, 0));
        let window = pad_window(72, 1, 100, 30);
        assert_eq!((window.top, window.lines, window.bottom), (0, 71..100, 1));
        let window = pad_window(71, 1, 100, 30);
        assert_eq!((window.top, window.lines, window.bottom), (0, 70..100, 0));
        let window = pad_window(0, 0, 10, 30);
        assert_eq!((window.top, window.lines, window.bottom), (0, 0..10, 0));
        let window = pad_window(0, 1, 2, 30);
        assert_eq!((window.top, window.lines, window.bottom), (1, 0..2, 1));
    }

    #[test]
    fn reader_pad_is_one_row_on_a_list_and_none_on_settings_or_help() {
        let mut state = demo_state();
        let mut ui = test_ui();
        assert_eq!(reader_pad(&state, &ui), 1);
        state.density = Density::Compact;
        assert_eq!(reader_pad(&state, &ui), 1);
        state.open_settings();
        assert_eq!(reader_pad(&state, &ui), 0);
        state.settings = false;
        ui.mode = Mode::Help;
        assert_eq!(reader_pad(&state, &ui), 0);
    }

    #[test]
    fn the_edge_pad_scrolls_out_of_the_row_under_the_rule() {
        let mut state = demo_state();
        let ui = test_ui();
        let rows = rows_of(&frame(&mut state, &ui, 100, 40));
        assert!(rows[5].trim().is_empty(), "top pad: {:?}", rows[5]);
        ui.scroll.set(1);
        let rows = rows_of(&frame(&mut state, &ui, 100, 40));
        assert!(
            rows[5].chars().any(|ch| !ch.is_whitespace()),
            "one row of scroll uses the row under the rule: {:?}",
            rows[5]
        );

        let mut state = demo_state();
        state.layout = Layout::Split;
        let ui = test_ui();
        let rows = rows_of(&frame(&mut state, &ui, 160, 40));
        assert!(
            rows[6].contains("Post"),
            "the thread bar sits under the pad: {:?}",
            rows[6]
        );
        assert!(
            !rows[5].contains("Post"),
            "the pad stays above Post: {:?}",
            rows[5]
        );
        let divider = rows[5].find('│').expect("divider");
        assert!(
            rows[5][..divider].trim().is_empty(),
            "feed pad: {:?}",
            rows[5]
        );
        assert!(
            rows[5][divider + '│'.len_utf8()..].trim().is_empty(),
            "thread pad: {:?}",
            rows[5]
        );
        assert!(
            rows[6][..divider].chars().any(|ch| !ch.is_whitespace()),
            "the feed's first line lines up with Post: {:?}",
            rows[6]
        );
    }

    #[test]
    fn a_scrolled_post_uses_both_edges_of_the_body() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 100, 40);
        for _ in 0..8 {
            press(&mut state, &mut ui, KeyCode::Char('j'));
            frame(&mut state, &ui, 100, 40);
            if ui.scroll.get() > 1 {
                break;
            }
        }
        assert!(ui.scroll.get() > 1, "the selection never left the top pad");
        let rows = rows_of(&frame(&mut state, &ui, 100, 40));
        let nav = rows.iter().rposition(|row| row.contains("Home")).unwrap();
        assert!(
            rows[5].chars().any(|ch| !ch.is_whitespace()),
            "top edge: {:?}",
            rows[5]
        );
        assert!(
            rows[nav - 2].chars().any(|ch| !ch.is_whitespace()),
            "bottom edge: {:?}",
            rows[nav - 2]
        );
    }

    /// Columns of one frame from `from_col` on, one row per line.
    fn pane_text(buffer: &Buffer, from_col: u16) -> String {
        (0..buffer.area.height)
            .map(|y| {
                (from_col..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_short_split_scrolls_the_open_post_before_leaving_it() {
        let mut state = demo_state();
        state.layout = Layout::Split;
        state.detail = Detail::Rich;
        state.preview_ratio = Some(0.5);
        let mut ui = test_ui();
        let id = state
            .posts
            .values()
            .find(|post| post.text.starts_with("Half-block") && post.reposted_by.is_none())
            .map(|post| post.id.clone())
            .expect("river's post");
        frame(&mut state, &ui, 160, 40);
        assert!(state.select_post(&id, false), "the feed lists the post");
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(state.thread_focus.is_some());
        let buffer = frame(&mut state, &ui, 160, 22);
        let (width, height) = ui.viewport.get();
        let body = screens::linked_body(&state, width, true);
        let focus = body.focus.expect("the open post");
        let pad = reader_pad(&state, &ui);
        let span = RowSpan {
            start: focus.start + pad,
            end: focus.end + pad,
        };
        assert!(
            span.end - span.start > height,
            "the post fits the pane: {span:?} in {height} rows"
        );
        let divider = {
            let laid = panes(&mut state, &ui, 160).expect("split");
            (render::margin_for(160, state.detail) + laid.left + 1) as u16
        };
        let right = pane_text(&buffer, divider + 1);
        assert!(
            right.contains("Half-block"),
            "the open post starts on screen:\n{right}"
        );
        assert!(
            !right.contains("views"),
            "the short pane already shows the tail:\n{right}"
        );
        let opened = state.selected_post_id();
        let top = ui.scroll_right.get();
        press(&mut state, &mut ui, KeyCode::Char('j'));
        frame(&mut state, &ui, 160, 22);
        assert_eq!(state.selected_post_id(), opened);
        assert!(ui.scroll_right.get() > top, "j left the scroll at {top}");
        press(&mut state, &mut ui, KeyCode::Char('k'));
        frame(&mut state, &ui, 160, 22);
        assert_eq!(state.selected_post_id(), opened);
        assert_eq!(ui.scroll_right.get(), top);
        press(&mut state, &mut ui, KeyCode::Char(' '));
        frame(&mut state, &ui, 160, 22);
        assert_eq!(state.selected_post_id(), opened);
        assert!(ui.scroll_right.get() > top + 1, "{}", ui.scroll_right.get());

        let mut saw_tail = false;
        let mut previous = ui.scroll_right.get();
        for _ in 0..80 {
            let staying = state.selected_post_id();
            press(&mut state, &mut ui, KeyCode::Char('j'));
            let buffer = frame(&mut state, &ui, 160, 22);
            if pane_text(&buffer, divider + 1).contains("views") {
                saw_tail = true;
            }
            let now = ui.scroll_right.get();
            if state.selected_post_id() != staying {
                assert!(saw_tail, "j left the post before its tail was on screen");
                break;
            }
            if now == previous {
                assert!(saw_tail, "j stopped at {now} before the tail");
                break;
            }
            assert!(now > previous, "{now} yanked back from {previous}");
            previous = now;
        }
        assert!(saw_tail, "the tail never reached the pane");
        let at_tail = ui.scroll_right.get();
        let still = state.selected_post_id();
        press(&mut state, &mut ui, KeyCode::PageDown);
        frame(&mut state, &ui, 160, 22);
        if state.selected_post_id() == still {
            assert!(
                ui.scroll_right.get() >= at_tail,
                "page yanked a finished post back to {}",
                ui.scroll_right.get()
            );
        }
    }

    #[test]
    fn the_end_pad_sits_on_the_row_above_the_footer() {
        let mut state = fixtures::demo_state(Detail::Standard);
        state.demo = false;
        let id = state.current_feed_id().unwrap();
        state
            .feeds
            .get_mut(&id)
            .unwrap()
            .next
            .insert(Network::X, "cursor".into());
        let posts = state.current_posts();
        let selectable = crate::layout::selectable(&state.lay(&posts));
        assert!(state.select_row(*selectable.last().unwrap()));
        let ui = test_ui();
        let rows = rows_of(&frame(&mut state, &ui, 80, 24));
        let nav = rows.iter().rposition(|row| row.contains("Home")).unwrap();
        assert!(
            rows[nav - 2].trim().is_empty(),
            "bottom pad: {:?} above {:?}",
            rows[nav - 2],
            rows[nav - 1]
        );
        assert!(rows[..nav - 2].iter().any(|row| row.contains("loading")));
    }

    #[test]
    fn composer_rows_break_after_spaces() {
        let text = "hello brave new world";
        let rows: Vec<&str> = wrap_ranges(text, 11)
            .iter()
            .map(|(a, b)| &text[*a..*b])
            .collect();
        assert_eq!(rows, ["hello brave ", "new world"]);
        assert_eq!(wrap_ranges("a\nb", 10), [(0, 1), (2, 3)]);
    }

    #[test]
    fn inspector_marks_the_cursor_row_and_expands_on_toggle() {
        let value = serde_json::json!({ "id": "p1", "metrics": { "likes": 48 } });
        let mut open = HashSet::new();
        let lines = inspect_lines(&value, &open, 1);
        let text: Vec<String> = lines.iter().map(|l| l.plain()).collect();
        assert_eq!(text, ["  id: \"p1\"", "▍ metrics: {… 1 }"]);
        assert!(!lines[0].selected && lines[1].selected);
        let nodes = jsonview::visible(&value, &open);
        jsonview::toggle(&mut open, &nodes[1].0);
        let text: Vec<String> = inspect_lines(&value, &open, 1)
            .iter()
            .map(|l| l.plain())
            .collect();
        assert_eq!(text, ["  id: \"p1\"", "▍ metrics: {", "    likes: 48"]);
    }

    #[test]
    fn gallery_arrows_wrap_and_close_keys_close() {
        let urls = vec!["demo://wave".to_string(), "demo://spark".to_string()];
        let mut gallery = Gallery::new(None, urls);
        assert_eq!(
            gallery_key(&mut gallery, key(KeyCode::Right)),
            GalleryStep::Stay
        );
        assert_eq!(gallery.index, 1);
        assert_eq!(
            gallery_key(&mut gallery, key(KeyCode::Right)),
            GalleryStep::Stay
        );
        assert_eq!(gallery.index, 0);
        assert_eq!(
            gallery_key(&mut gallery, key(KeyCode::Char('h'))),
            GalleryStep::Stay
        );
        assert_eq!(gallery.index, 1);
        assert_eq!(
            gallery_key(&mut gallery, key(KeyCode::Char('o'))),
            GalleryStep::Open("demo://spark".into())
        );
        assert_eq!(
            gallery_key(&mut gallery, key(KeyCode::Char('s'))),
            GalleryStep::Stay
        );
        for code in [
            KeyCode::Esc,
            KeyCode::Backspace,
            KeyCode::Char('v'),
            KeyCode::Char('q'),
        ] {
            assert_eq!(gallery_key(&mut gallery, key(code)), GalleryStep::Close);
        }
    }

    #[test]
    fn a_single_picture_ignores_the_arrows() {
        let mut gallery = Gallery::new(None, vec!["demo://spark".to_string()]);
        assert_eq!(
            gallery_key(&mut gallery, key(KeyCode::Left)),
            GalleryStep::Stay
        );
        assert_eq!(gallery.index, 0);
    }

    #[test]
    fn slots_paint_only_when_wholly_on_screen_and_load_one_screen_around() {
        let slot = Slot {
            url: "u".into(),
            x: 6,
            y: 20,
            cols: 40,
            rows: 10,
            avatar: false,
            square: false,
        };
        assert_eq!(on_screen(&slot, 0, 30), Some(20));
        assert_eq!(on_screen(&slot, 0, 29), None);
        assert_eq!(on_screen(&slot, 21, 30), None);
        assert!(wanted(&slot, 0, 10));
        assert!(!wanted(&slot, 0, 6));
        assert!(wanted(&slot, 35, 10));
        assert!(!wanted(&slot, 41, 10));
    }

    #[test]
    fn alt_text_keeps_two_lines_and_ends_with_an_ellipsis() {
        assert_eq!(
            alt_lines("a short alt", 40),
            vec!["a short alt".to_string()]
        );
        let lines = alt_lines("one two three four five six seven eight nine ten", 10);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'), "{lines:?}");
        assert!(alt_lines("   ", 10).is_empty());
    }

    #[test]
    fn centered_puts_a_smaller_box_in_the_middle() {
        let area = Rect::new(4, 4, 100, 30);
        assert_eq!(
            centered(area, Rect::new(0, 0, 60, 30)),
            Rect::new(24, 4, 60, 30)
        );
        assert_eq!(
            centered(area, Rect::new(4, 4, 100, 10)),
            Rect::new(4, 14, 100, 10)
        );
    }

    #[test]
    fn the_spinner_turns_every_120_ms() {
        let frames: String = [0, 119, 120, 240, 360, 480]
            .map(|ms| spin_frame(Duration::from_millis(ms)))
            .iter()
            .collect();
        assert_eq!(frames, "◐◐◓◑◒◐");
    }

    #[test]
    fn the_loading_line_turns_only_in_rich() {
        let line = busy_line("loading", true, false, Duration::from_millis(130), false);
        assert_eq!(line, "◓ loading");
        assert_eq!(
            busy_line("loading", false, false, Duration::ZERO, false),
            "loading…"
        );
        assert_eq!(
            busy_line("sending", false, true, Duration::ZERO, true),
            "sending... · ctrl-c again to quit"
        );
    }

    #[test]
    fn the_loading_line_keeps_the_last_frame() {
        let state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let ui = test_ui();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
        let last = terminal
            .draw(|frame| draw(frame, &state, &ui, None))
            .unwrap()
            .buffer
            .clone();
        terminal
            .draw(|frame| paint_busy(frame, &last, &state.theme, state.detail, "loading…"))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |buffer: &Buffer, y: u16| {
            (0..104)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        };
        assert_eq!(row(&buffer, 1), row(&last, 1));
        assert_eq!(row(&buffer, 20), row(&last, 20));
        assert_eq!(row(&buffer, 43).trim_end(), "loading…");
    }

    #[test]
    fn rich_avatars_paint_over_the_letter_box_with_a_graphics_protocol() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.detail = theme::Detail::Rich;
        state.retheme();
        state.preview_ratio = Some(0.5);
        assert!(state
            .view(100, true)
            .previews
            .iter()
            .all(|slot| !slot.avatar));
        state.graphics = true;
        assert!(state
            .view(100, true)
            .previews
            .iter()
            .any(|slot| slot.avatar));

        let ui = test_ui();
        let mut picker = ratatui_image::picker::Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Iterm2);
        let mut png = Vec::new();
        image::RgbaImage::new(8, 8)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let fetch: preview::Fetch = Arc::new(move |_| Ok(png.clone()));
        ui.previews.replace(Some(Previews::new(picker, fetch)));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, &ui, None))
            .unwrap();
        let center = render::margin(104) as u16 + 3;
        let buffer = terminal.backend().buffer().clone();
        let spinning = (0..44).any(|y| {
            let symbol = buffer[(center, y)].symbol();
            SPINNER.iter().any(|frame| symbol == frame.to_string())
        });
        assert!(spinning, "no avatar showed the spinner while loading");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(previews) = ui.previews.borrow_mut().as_mut() {
                previews.poll();
            }
            terminal
                .draw(|frame| draw(frame, &state, &ui, None))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let corner = render::margin(104) as u16 + 1;
            if (0..44).any(|y| buffer[(corner, y)].symbol().contains("1337;File")) {
                break;
            }
            assert!(Instant::now() < deadline, "no avatar was painted");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn plain_detail_draws_ascii_chrome_without_color() {
        use ratatui::style::{Color, Modifier};
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.detail = theme::Detail::Plain;
        state.retheme();
        let ui = test_ui();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, &ui, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16| {
            (0..104)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        };
        for y in [1, 3, 4, 41, 42, 43] {
            let text = row(y);
            assert!(text.chars().all(|ch| ch.is_ascii() || ch == '·'), "{text}");
        }
        assert!(row(4).starts_with("  ---"), "{}", row(4));
        assert!(row(43).contains("move  enter open"), "{}", row(43));
        assert!((5..41).any(|y| buffer[(1, y)].symbol() == ">"));
        for y in 0..44 {
            for x in 0..104 {
                let cell = &buffer[(x, y)];
                assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{x},{y}");
                assert!(
                    !cell.modifier.intersects(Modifier::DIM | Modifier::REVERSED),
                    "{x},{y}"
                );
            }
        }
    }

    #[test]
    fn the_viewer_draws_byline_count_rules_and_keys() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.reveal_shortcuts = true;
        let mut ui = test_ui();
        let urls = vec!["demo://wave".to_string(), "demo://spark".to_string()];
        ui.mode = Mode::Gallery(Box::new(Gallery::new(None, urls)));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, &ui, None))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16| {
            (0..104)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        };
        assert!(row(1).trim_end().ends_with("1 of 2"), "{}", row(1));
        assert!(row(2).contains("────"));
        assert!(row(42).contains("────"));
        assert!(
            row(43).contains("← → previous, next  o open original  esc close"),
            "{}",
            row(43)
        );
        assert!(
            row(22).contains("image unavailable · o opens it"),
            "{}",
            row(22)
        );
    }

    #[test]
    fn enter_opens_the_thread_and_backspace_goes_back() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut clients = Clients::empty();
        let mut ui = test_ui();
        assert!(state.selected_post().is_some());
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Enter)).unwrap();
        assert!(state.thread_focus.is_some());
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Backspace)).unwrap();
        assert!(state.thread_focus.is_none());
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('}'))).unwrap();
        assert_eq!(state.shown_merged_tab(), MergedTab::Discover);
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('1'))).unwrap();
        assert_eq!(state.shown_merged_tab(), MergedTab::Home);
    }

    #[test]
    fn wide_screens_split_into_feed_and_thread() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.layout = Layout::Auto;
        let ui = test_ui();
        assert!(panes(&mut state, &ui, 120).is_none());
        let p = panes(&mut state, &ui, 160).expect("160 columns split under auto");
        assert!(state.thread_focus.is_none());
        assert_eq!((p.left, p.right), (58, 98));
        assert!(!p.thread_focused);
        let root = state.selected_post().unwrap().text.clone();
        let first_words: String = root.chars().take(12).collect();
        assert!(p
            .thread
            .lines
            .iter()
            .any(|line| line.plain().contains(&first_words)));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 44)).unwrap();
        terminal
            .draw(|frame| draw(frame, &state, &ui, Some(&p)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let divider = (render::margin_for(160, state.detail) + p.left + 1) as u16;
        assert_eq!(buffer[(divider, RULE_ROW as u16)].symbol(), "┬");
        assert_eq!(buffer[(divider, 10)].symbol(), "│");
        let sub = (0..160)
            .map(|x| buffer[(x, 3)].symbol().to_string())
            .collect::<String>();
        assert!(
            sub.trim_end().ends_with("Saved"),
            "feed tabs stay on the right edge, got {sub}"
        );
        assert!(
            !sub.contains("oldest"),
            "reply order stays in the thread pane, got {sub}"
        );
        assert_eq!(ui.viewport.get().0, 58);
        state.layout = Layout::Split;
        assert!(panes(&mut state, &ui, 120).is_some());
        assert!(panes(&mut state, &ui, 99).is_none());
        state.layout = Layout::Single;
        assert!(panes(&mut state, &ui, 200).is_none());
    }

    #[test]
    fn a_focused_split_keeps_the_feed_bar_and_seats_the_avatar_under_post() {
        let mut state = fixtures::demo_state(Detail::Rich);
        state.layout = Layout::Split;
        state.graphics = true;
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Right);
        assert!(state.thread_focus.is_some());
        let mut picker = ratatui_image::picker::Picker::halfblocks();
        picker.set_protocol_type(ratatui_image::picker::ProtocolType::Iterm2);
        let mut png = Vec::new();
        image::RgbaImage::new(8, 8)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let fetch: preview::Fetch = std::sync::Arc::new(move |_| Ok(png.clone()));
        ui.previews.replace(Some(Previews::new(picker, fetch)));

        let buffer = frame(&mut state, &ui, 160, 40);
        let row = |y: u16| {
            (0..160)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        };
        let sub = row(3);
        assert!(
            sub.trim_end().ends_with("Saved"),
            "feed tabs stay on the right edge while the thread has the keys, got {sub}"
        );
        assert!(
            !sub.contains("oldest"),
            "reply order stays in the thread pane, got {sub}"
        );
        let header = (0..40u16)
            .find(|y| row(*y).contains("oldest"))
            .expect("the thread pane header");
        let post_at = (0..157u16)
            .rev()
            .find(|&x| {
                buffer[(x, header)].symbol() == "P"
                    && buffer[(x + 1, header)].symbol() == "o"
                    && buffer[(x + 2, header)].symbol() == "s"
                    && buffer[(x + 3, header)].symbol() == "t"
            })
            .expect("Post");
        let top = post_at + 1;
        let mid = post_at + 3;
        assert_eq!(
            buffer[(top, header + 1)].symbol(),
            "╭",
            "{}",
            row(header + 1)
        );
        assert_eq!(
            buffer[(mid, header + 1)].symbol(),
            "─",
            "the avatar top stays under Post, got {}",
            row(header + 1)
        );
        let letter = buffer[(mid, header + 2)].symbol();
        assert!(
            SPINNER.iter().any(|frame| letter == frame.to_string()),
            "the loading picture sits on the letter, got {letter} on {}",
            row(header + 2)
        );

        state.layout = Layout::Single;
        let buffer = frame(&mut state, &ui, 120, 40);
        let sub: String = (0..120)
            .map(|x| buffer[(x, 3)].symbol().to_string())
            .collect();
        assert!(
            sub.contains("Post") && sub.contains("oldest"),
            "a single column still uses the subbar as the thread bar, got {sub}"
        );
    }

    #[test]
    fn an_open_thread_takes_the_keys_in_the_split() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.layout = Layout::Auto;
        let mut clients = Clients::empty();
        let mut ui = test_ui();
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Enter)).unwrap();
        let open = state.thread_focus.clone();
        assert!(open.is_some());
        let p = panes(&mut state, &ui, 160).unwrap();
        assert!(p.thread_focused);
        assert_eq!(state.thread_focus, open);
        assert!(p.feed.lines.iter().any(|line| line.bar));
    }

    #[test]
    fn the_split_keeps_every_post_when_the_right_pane_has_the_keys() {
        let mut state = fixtures::demo_state(Detail::Standard);
        state.layout = Layout::Auto;
        let mut ui = test_ui();
        frame(&mut state, &ui, 160, 40);
        assert!(state.select_post("devnull:p2", false));
        let preview = panes(&mut state, &ui, 160).expect("split");
        assert!(!preview.thread_focused);
        assert!(
            preview
                .thread
                .lines
                .iter()
                .any(|line| line.plain().contains("Hype")),
            "the reply is in the right pane before the keys move"
        );
        press(&mut state, &mut ui, KeyCode::Right);
        assert!(state.thread_focus.is_some());
        assert!(!matches!(
            state.focus,
            Some(crate::model::FeedId::Thread(_))
        ));
        let open = panes(&mut state, &ui, 160).expect("split");
        let feed: String = open.feed.lines.iter().map(|line| line.plain()).collect();
        assert!(feed.contains("Step 1"), "{feed}");
        assert!(feed.contains("ok devs"), "{feed}");
        press(&mut state, &mut ui, KeyCode::Left);
        assert!(state.thread_focus.is_none());
        let back = panes(&mut state, &ui, 160).expect("split");
        let feed: String = back.feed.lines.iter().map(|line| line.plain()).collect();
        assert!(
            feed.contains("Step 1") && feed.contains("ok devs"),
            "{feed}"
        );
    }

    #[test]
    fn the_split_feed_lists_roots_and_keeps_the_selection_across_the_threshold() {
        let mut state = fixtures::demo_state(Detail::Standard);
        state.layout = Layout::Auto;
        let mut ui = test_ui();
        frame(&mut state, &ui, 100, 40);
        for _ in 0..12 {
            if state
                .selected_post()
                .is_some_and(|p| p.in_reply_to.is_some())
            {
                break;
            }
            press(&mut state, &mut ui, KeyCode::Char('j'));
        }
        let reply = state.selected_post().cloned().expect("a reply");
        assert!(reply.in_reply_to.is_some());
        frame(&mut state, &ui, 160, 40);
        assert!(ui.split.get());
        assert!(state.overview);
        let root = state.selected_post().cloned().expect("a selection");
        assert_ne!(root.id, reply.id);
        assert_eq!(root.conversation_id, reply.conversation_id);
        assert!(state
            .lay(&state.current_posts())
            .iter()
            .all(|r| r.depth == 0));
        press(&mut state, &mut ui, KeyCode::Char('j'));
        let next = state.selected_post_id();
        assert_ne!(next.as_deref(), Some(root.id.as_str()));
        frame(&mut state, &ui, 123, 40);
        assert!(!ui.split.get());
        assert!(!state.overview);
        assert_eq!(state.selected_post_id(), next);
    }

    #[test]
    fn x_shows_and_hides_the_replies_in_place() {
        let mut state = fixtures::demo_state(Detail::Standard);
        state.layout = Layout::Auto;
        let mut ui = test_ui();
        frame(&mut state, &ui, 160, 40);
        let deep = |state: &State| {
            state
                .lay(&state.current_posts())
                .iter()
                .any(|r| r.depth > 0)
        };
        assert!(!deep(&state));
        assert!(state.select_post("devnull:p2", false));
        let id = state.selected_post_id();
        press(&mut state, &mut ui, KeyCode::Char('x'));
        frame(&mut state, &ui, 160, 40);
        assert_eq!(state.selected_post_id(), id);
        assert!(deep(&state), "x shows the replies inline");
        press(&mut state, &mut ui, KeyCode::Char('x'));
        frame(&mut state, &ui, 160, 40);
        assert_eq!(state.selected_post_id(), id);
        assert!(!deep(&state));
    }

    #[test]
    fn an_open_thread_keeps_its_place_across_the_threshold() {
        let mut state = fixtures::demo_state(Detail::Standard);
        state.layout = Layout::Auto;
        let mut ui = test_ui();
        frame(&mut state, &ui, 160, 30);
        assert!(state.select_post("devnull:p2", false));
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(state.thread_focus.is_some());
        for _ in 0..3 {
            press(&mut state, &mut ui, KeyCode::Char('j'));
        }
        frame(&mut state, &ui, 160, 30);
        let id = state.selected_post_id();
        let key = state.selection_key();
        let buffer = frame(&mut state, &ui, 123, 30);
        assert!(!ui.split.get());
        assert_eq!(state.selected_post_id(), id);
        assert_eq!(*ui.scroll_key.borrow(), key);
        assert!(
            !bar_rows(&buffer, state.detail).is_empty(),
            "the selected reply is on screen"
        );
        frame(&mut state, &ui, 160, 30);
        assert!(ui.split.get());
        assert_eq!(state.selected_post_id(), id);
        assert_eq!(*ui.scroll_right_key.borrow(), key);
    }

    // MARK: input batches

    fn pointer(kind: MouseEventKind, column: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row: 20,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn typed_key(ch: char) -> Event {
        Event::Key(key(KeyCode::Char(ch)))
    }

    /// Runs `events` through `coalesce` over one pane and records each press; `q` quits.
    fn batch_of(events: Vec<Event>) -> (Vec<Press>, Batch) {
        let mut ran = Vec::new();
        let batch = coalesce(
            events,
            |_, _| Some(Zone::BodyLeft),
            |press| {
                let quits = matches!(&press, Press::Key(key) if key.code == KeyCode::Char('q'));
                ran.push(press);
                Ok(quits)
            },
        );
        (ran, batch.unwrap())
    }

    #[test]
    fn a_pointer_flood_runs_only_its_last_move() {
        let columns = (1..=100).chain((1..=100).rev());
        let events = columns
            .map(|column| Event::Mouse(pointer(MouseEventKind::Moved, column)))
            .collect();
        let (ran, batch) = batch_of(events);
        assert_eq!(ran, [Press::Mouse(pointer(MouseEventKind::Moved, 1))]);
        assert_eq!(batch.moves_dropped, 199);
        assert_eq!(batch.processed, 1);
    }

    #[test]
    fn each_same_direction_wheel_run_over_one_pane_is_one_step() {
        let mut events = vec![Event::Mouse(pointer(MouseEventKind::ScrollDown, 10)); 30];
        events.extend(vec![Event::Mouse(pointer(MouseEventKind::ScrollUp, 10)); 2]);
        let (ran, batch) = batch_of(events);
        assert_eq!(
            ran,
            [
                Press::Wheel(pointer(MouseEventKind::ScrollDown, 10), 30),
                Press::Wheel(pointer(MouseEventKind::ScrollUp, 10), -2),
            ]
        );
        assert_eq!(batch.scroll_net, 28);
        assert_eq!(batch.processed, 2);
    }

    /// The selected post after `events` arrive one by one, after they arrive as one batch, and the batch's steps.
    fn wheel_paths(events: &[MouseEventKind]) -> (Option<String>, Option<String>, Vec<i32>) {
        let at = |kind: MouseEventKind| MouseEvent {
            kind,
            column: 20,
            row: 12,
            modifiers: KeyModifiers::NONE,
        };
        let mut single = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        frame(&mut single, &ui, 104, 44);
        for kind in events {
            handle_mouse(&mut single, &mut Clients::empty(), &mut ui, at(*kind)).unwrap();
        }
        let mut batched = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        frame(&mut batched, &ui, 104, 44);
        let batch = events.iter().map(|kind| Event::Mouse(at(*kind))).collect();
        let mut taken = Vec::new();
        coalesce(
            batch,
            |_, _| Some(Zone::BodyLeft),
            |press| match press {
                Press::Wheel(mouse, steps) => {
                    taken.push(steps);
                    wheel(
                        &mut batched,
                        &mut Clients::empty(),
                        &mut ui,
                        steps,
                        usize::from(mouse.column),
                        usize::from(mouse.row),
                    )
                }
                _ => Ok(false),
            },
        )
        .unwrap();
        (single.selected_post_id(), batched.selected_post_id(), taken)
    }

    #[test]
    fn a_batched_wheel_run_moves_as_far_as_its_single_notches() {
        let down = vec![MouseEventKind::ScrollDown; 20];
        let (single, batched, steps) = wheel_paths(&down);
        assert!(single.is_some());
        assert_eq!(batched, single);
        assert_eq!(steps, [20]);
        let mut mixed = vec![MouseEventKind::ScrollDown; 30];
        mixed.extend(vec![MouseEventKind::ScrollUp; 4]);
        let (single, batched, steps) = wheel_paths(&mixed);
        assert_eq!(single.as_deref(), Some("x:l1"));
        assert_eq!(batched, single);
        assert_eq!(steps, [30, -4]);
    }

    #[test]
    fn keys_and_pastes_run_in_arrival_order() {
        let moved = pointer(MouseEventKind::Moved, 3);
        let events = vec![
            typed_key('j'),
            typed_key('k'),
            Event::Mouse(moved),
            Event::Mouse(pointer(MouseEventKind::Up(MouseButton::Left), 3)),
            Event::Mouse(moved),
            typed_key('l'),
            Event::Paste("hi".into()),
            typed_key('b'),
        ];
        let (ran, batch) = batch_of(events);
        assert_eq!(
            ran,
            [
                Press::Key(key(KeyCode::Char('j'))),
                Press::Key(key(KeyCode::Char('k'))),
                Press::Mouse(moved),
                Press::Key(key(KeyCode::Char('l'))),
                Press::Paste("hi".into()),
                Press::Key(key(KeyCode::Char('b'))),
            ]
        );
        assert_eq!(batch.moves_dropped, 1);
        assert!(!batch.quit);
    }

    #[test]
    fn a_key_that_quits_stops_the_batch() {
        let events = vec![typed_key('j'), typed_key('q'), typed_key('k')];
        let (ran, batch) = batch_of(events);
        assert_eq!(
            ran,
            [
                Press::Key(key(KeyCode::Char('j'))),
                Press::Key(key(KeyCode::Char('q'))),
            ]
        );
        assert!(batch.quit);
    }

    #[test]
    fn a_resize_burst_counts_once() {
        let events = (0..12).map(|i| Event::Resize(80 + i, 40)).collect();
        let (ran, batch) = batch_of(events);
        assert!(ran.is_empty());
        assert_eq!(batch.resizes, 1);
        assert_eq!(batch.processed, 1);
    }

    #[test]
    fn hover_reports_turn_on_with_the_network_picker_and_off_when_it_closes() {
        let flags = [AtomicBool::new(false), AtomicBool::new(false)];
        let mut state = demo_state();
        let mut ui = test_ui();
        let sync = |ui: &Ui| {
            let mut out = Vec::new();
            report_modes(&mut out, [&flags[0], &flags[1]], true, hovers(&ui.mode)).unwrap();
            String::from_utf8(out).unwrap()
        };
        assert_eq!(sync(&ui), "\x1b[?1000h\x1b[?1002h\x1b[?1006h");
        press(&mut state, &mut ui, KeyCode::Char('A'));
        assert_eq!(sync(&ui), "\x1b[?1003h");
        assert_eq!(sync(&ui), "");
        press(&mut state, &mut ui, KeyCode::Esc);
        assert_eq!(sync(&ui), "\x1b[?1003l");
        assert_eq!(sync(&ui), "");

        press(&mut state, &mut ui, KeyCode::Char('A'));
        assert_eq!(sync(&ui), "\x1b[?1003h");
        let mut out = Vec::new();
        report_modes(&mut out, [&flags[0], &flags[1]], false, true).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("\x1b[?1003l"));
        assert!(!flags[1].load(Ordering::Relaxed));
    }

    #[test]
    fn an_idle_step_draws_nothing_until_the_refresh() {
        let recent = Some(Duration::from_secs(29));
        assert!(!should_draw(false, false, recent));
        assert!(should_draw(true, false, recent));
        assert!(should_draw(false, true, recent));
        assert!(should_draw(false, false, Some(REFRESH)));
        assert!(should_draw(false, false, None));
    }

    // MARK: focus and mouse

    fn demo_state() -> State {
        State::boot(true, 15, &Config::default(), &Clients::empty(), None)
    }

    /// Draws one frame `width` columns wide, split when the layout says so, and returns it.
    fn frame(state: &mut State, ui: &Ui, width: u16, height: u16) -> Buffer {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        let split = panes(state, ui, usize::from(width));
        terminal
            .draw(|frame| draw(frame, state, ui, split.as_ref()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    const VIEWPORTS: [(u16, u16); 11] = [
        (24, 8),
        (32, 16),
        (40, 20),
        (43, 20),
        (44, 20),
        (60, 20),
        (80, 24),
        (100, 32),
        (123, 32),
        (124, 40),
        (180, 45),
    ];
    const DETAILS: [Detail; 4] = [
        Detail::Plain,
        Detail::Minimal,
        Detail::Standard,
        Detail::Rich,
    ];

    /**
    What every viewport keeps to after a frame `width`×`height`: body lines fit the content width,
    every target and dialog row the frame registered lies inside it, and the printed feed, without
    color, fits the terminal.
    */
    fn assert_inside(state: &State, ui: &Ui, width: u16, height: u16, what: &str) {
        let inner = render::inner_width_for(usize::from(width), state.detail);
        for line in screens::linked_body(state, inner, true).lines {
            let text = line.plain();
            assert!(text.width() <= inner, "{what}: {text:?}");
        }
        let hits = ui.hits.borrow();
        for target in &hits.targets {
            assert!(
                target.x + target.width <= width && target.y < height,
                "{what}: {target:?}"
            );
        }
        for (y, cols, _) in &hits.rows {
            assert!(
                cols.end <= usize::from(width) && *y < usize::from(height),
                "{what}: row {y} {cols:?}"
            );
        }
        let opts = crate::plain::PrintOpts {
            color: false,
            json: false,
            images: false,
            prompt: false,
            width: usize::from(width),
        };
        let (feed, footer) = crate::plain::printed(state, &opts, false);
        for line in feed.iter().chain(&footer) {
            assert!(
                line.width() <= usize::from(width),
                "{what} printed: {line:?}"
            );
        }
    }

    #[test]
    fn f6_steps_through_the_zones_that_have_stops() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        frame(&mut state, &ui, 100, 40);
        let mut zones: Vec<Zone> = tab_stops(&ui.hits.borrow().targets)
            .iter()
            .map(|t| t.zone)
            .collect();
        zones.dedup();
        assert!(zones.len() >= 2, "{zones:?}");
        let selected = state.selected_post_id();
        let mut seen = Vec::new();
        for _ in 0..zones.len() {
            press(&mut state, &mut ui, KeyCode::F(6));
            frame(&mut state, &ui, 100, 40);
            let zone = ui.focus.borrow().as_ref().map(|f| f.zone).unwrap();
            if zone == Zone::BodyLeft {
                assert_eq!(state.selected_post_id(), selected);
            }
            seen.push(zone);
        }
        let mut each = seen.clone();
        each.sort();
        each.dedup();
        assert_eq!(each, zones, "{seen:?}");
        handle_key(
            &mut state,
            &mut Clients::empty(),
            &mut ui,
            with(KeyCode::F(6), KeyModifiers::SHIFT),
        )
        .unwrap();
        let back = ui.focus.borrow().as_ref().map(|f| f.zone).unwrap();
        assert_eq!(back, seen[seen.len() - 2]);
        press(&mut state, &mut ui, KeyCode::F(18));
        let back = ui.focus.borrow().as_ref().map(|f| f.zone).unwrap();
        assert_eq!(back, seen[(seen.len() * 3 - 3) % seen.len()]);
    }

    #[test]
    fn every_focus_range_ends_on_its_own_content() {
        let rail = |c: char| c.is_whitespace() || "│╎┊┆╵╷▍┃".contains(c);
        for detail in [Detail::Standard, Detail::Rich] {
            let mut state = fixtures::demo_state(detail);
            let mut ui = test_ui();
            for step in 0..10 {
                frame(&mut state, &ui, 100, 40);
                let body = screens::linked_body(&state, render::inner_width_for(100, detail), true);
                if let Some(span) = body.focus {
                    let end = span.end.max(span.start + 1);
                    let block: Vec<String> = body.lines[span.start..end]
                        .iter()
                        .map(|l| l.plain())
                        .collect();
                    assert!(
                        !block[block.len() - 1].chars().all(rail),
                        "{detail:?} step {step} {:?}: {block:#?}",
                        state.selected_post_id()
                    );
                }
                press(&mut state, &mut ui, KeyCode::Char('j'));
            }
        }
    }

    #[test]
    fn every_viewport_and_detail_level_draws() {
        for (width, height) in VIEWPORTS {
            for detail in DETAILS {
                let mut state = fixtures::demo_state(detail);
                let ui = test_ui();
                frame(&mut state, &ui, width, height);
                assert_inside(
                    &state,
                    &ui,
                    width,
                    height,
                    &format!("{width}x{height} {detail:?}"),
                );
            }
        }
    }

    #[test]
    fn the_end_of_a_feed_draws_a_loading_card_once() {
        let mut state = fixtures::demo_state(Detail::Standard);
        state.demo = false;
        let id = state.current_feed_id().unwrap();
        state
            .feeds
            .get_mut(&id)
            .unwrap()
            .next
            .insert(Network::X, "cursor".into());
        let ui = test_ui();
        let rows = rows_of(&frame(&mut state, &ui, 80, 24));
        assert!(
            !rows.iter().any(|row| row.contains("loading")),
            "the first row does not fetch: {rows:#?}"
        );
        assert!(!state.show_page_card);

        let posts = state.current_posts();
        let selectable = crate::layout::selectable(&state.lay(&posts));
        assert!(state.select_row(*selectable.last().unwrap()));
        let rows = rows_of(&frame(&mut state, &ui, 80, 24));
        assert!(state.show_page_card);
        assert!(state.page_gate.is_none());
        let at = rows
            .iter()
            .position(|row| row.contains("loading…"))
            .unwrap_or_else(|| panic!("missing card: {rows:#?}"));
        assert!(rows[at.saturating_sub(1)].contains('╭'), "card: {rows:#?}");
        let hidden = screens::linked_body(&state, 80, false);
        assert!(!hidden
            .lines
            .iter()
            .any(|line| line.plain().contains("loading")));

        state.page_gate = Some((id, state.selected_post_id()));
        let rows = rows_of(&frame(&mut state, &ui, 80, 24));
        assert!(!state.show_page_card);
        assert!(!rows.iter().any(|row| row.contains("loading")));

        let posts = state.current_posts();
        let first = crate::layout::selectable(&state.lay(&posts))[0];
        assert!(state.select_row(first));
        frame(&mut state, &ui, 80, 24);
        assert!(state.page_gate.is_none());
    }

    fn rows_of(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn below_the_minimum_size_only_the_size_notice_shows() {
        for (width, height) in [(23, 8), (60, 7)] {
            let mut state = fixtures::demo_state(Detail::Standard);
            let ui = test_ui();
            let rows = rows_of(&frame(&mut state, &ui, width, height));
            let shown: Vec<&String> = rows.iter().filter(|row| !row.trim().is_empty()).collect();
            assert_eq!(shown.len(), 1, "{width}x{height}: {rows:#?}");
            assert!(
                shown[0].contains("SoCLI needs 24×8"),
                "{width}x{height}: {rows:#?}"
            );
            assert!(ui.hits.borrow().targets.is_empty());
        }
        let rows = rows_of(&frame(
            &mut fixtures::demo_state(Detail::Standard),
            &test_ui(),
            60,
            7,
        ));
        assert_eq!(rows[3].trim(), "SoCLI needs 24×8 · q quits");
        for quit in [
            key(KeyCode::Char('q')),
            key(KeyCode::Esc),
            with(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            assert!(quits_too_small(&Event::Key(quit)));
        }
        assert!(!quits_too_small(&Event::Key(key(KeyCode::Char('j')))));
        assert!(!quits_too_small(&Event::Resize(80, 24)));
    }

    /// A 32-column list keeps three wrapped lines and `Show more`. Opening the post shows every word, including one wrapped across rows.
    #[test]
    fn a_32_column_feed_shows_every_word_of_the_first_post() {
        for detail in DETAILS {
            let mut state = fixtures::demo_state(detail);
            let post = state.selected_post().unwrap();
            let text = post.text.clone();
            let id = post.id.clone();
            let closed = frame(&mut state, &test_ui(), 32, 40);
            let capped = block_text(&closed, detail, &text);
            assert!(capped.contains("Show more"), "{detail:?}: {capped:?}");
            assert!(
                !capped.contains("make"),
                "{detail:?}: the tail stays behind show more: {capped:?}"
            );
            state.opened.insert(id);
            let open = frame(&mut state, &test_ui(), 32, 40);
            let shown = block_text(&open, detail, &text);
            for word in text.split_whitespace() {
                assert!(shown.contains(word), "{detail:?}: {word:?} in {shown:?}");
            }
            assert!(shown.contains("Show less"), "{detail:?}: {shown:?}");
        }
    }

    /// The selected block's text from the first word's column, rows joined so a wrapped word stays whole.
    fn block_text(buffer: &Buffer, detail: Detail, text: &str) -> String {
        let rows = rows_of(buffer);
        let block: Vec<Vec<char>> = bar_rows(buffer, detail)
            .into_iter()
            .map(|y| rows[usize::from(y)].chars().collect())
            .collect();
        let first = text.split_whitespace().next().unwrap_or("");
        let col = block
            .iter()
            .find_map(|row| {
                let row: String = row.iter().collect();
                row.find(first).map(|at| row[..at].chars().count())
            })
            .unwrap_or(0);
        block
            .iter()
            .map(|row| {
                let text: String = row[col.min(row.len())..].iter().collect();
                text.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// The screen rows of the selected block: the rows with the selection bar, `▍` or the plain `>`.
    fn bar_rows(buffer: &Buffer, detail: Detail) -> Vec<u16> {
        let col =
            render::margin_for(usize::from(buffer.area.width), detail).saturating_sub(1) as u16;
        (0..buffer.area.height)
            .filter(|&y| matches!(buffer[(col, y)].symbol(), "▍" | ">"))
            .collect()
    }

    fn bar_row(buffer: &Buffer, detail: Detail) -> Option<u16> {
        bar_rows(buffer, detail).first().copied()
    }

    #[test]
    fn d_cycles_the_level_for_the_session_and_keeps_the_reading_row() {
        let config = crate::config::path();
        let before = std::fs::metadata(&config)
            .and_then(|meta| meta.modified())
            .ok();
        let mut state = fixtures::demo_state(Detail::Standard);
        state.layout = Layout::Single;
        let mut ui = test_ui();
        for _ in 0..3 {
            press(&mut state, &mut ui, KeyCode::Char('j'));
        }
        let row = bar_row(&frame(&mut state, &ui, 80, 24), state.detail);
        let id = state.selected_post_id();
        assert!(row.is_some());
        for _ in 0..4 {
            press(&mut state, &mut ui, KeyCode::Char('d'));
            assert_eq!(
                state.status,
                format!("Detail: {} · demo, not saved", state.detail.word())
            );
            frame(&mut state, &ui, 80, 24);
            assert_eq!(state.selected_post_id(), id);
        }
        assert_eq!(state.detail, Detail::Standard);
        assert_eq!(bar_row(&frame(&mut state, &ui, 80, 24), state.detail), row);
        let after = std::fs::metadata(&config)
            .and_then(|meta| meta.modified())
            .ok();
        assert_eq!(before, after);
    }

    #[test]
    fn a_resize_keeps_the_selected_post_on_its_row() {
        let mut state = fixtures::demo_state(Detail::Standard);
        state.layout = Layout::Single;
        let mut ui = test_ui();
        for _ in 0..2 {
            press(&mut state, &mut ui, KeyCode::Char('j'));
        }
        let id = state.selected_post_id();
        let row = bar_row(&frame(&mut state, &ui, 120, 40), state.detail).unwrap();
        for (width, height) in [(80, 24), (32, 16)] {
            let buffer = frame(&mut state, &ui, width, height);
            assert_eq!(state.selected_post_id(), id);
            let footer = if render::narrow(usize::from(width)) {
                2
            } else {
                3
            };
            let last = height - footer - 1;
            let span = screens::linked_body(
                &state,
                render::inner_width_for(usize::from(width), state.detail),
                true,
            )
            .focus
            .expect("a selected block");
            let len = span.end - span.start;
            let body = ui.viewport.get().1;
            let bars = bar_rows(&buffer, state.detail);
            assert_eq!(
                bars.len(),
                len.min(body),
                "{width}x{height}: the whole block shows"
            );
            assert!(bars[0] <= row, "{width}x{height}");
            assert!(
                usize::from(*bars.last().unwrap()) <= usize::from(last),
                "{width}x{height}"
            );
        }
    }

    #[test]
    fn an_error_outlasts_any_wait_and_clears_on_the_next_press() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        state.status.clear();
        state.fail("no link on this post");
        let now = Instant::now();
        for second in 0..=60 {
            assert!(!state.expire(now + Duration::from_secs(second)));
        }
        assert_eq!(state.status, "no link on this post");
        press(&mut state, &mut ui, KeyCode::Char('j'));
        assert_eq!(state.status, "");
        assert_eq!(state.status_error, None);

        state.fail("no link on this post");
        press(&mut state, &mut ui, KeyCode::Char('d'));
        assert_eq!(state.status, "Detail: rich · demo, not saved");
        assert_eq!(state.status_error, None);
    }

    #[test]
    fn the_demo_applies_every_saved_change_and_writes_no_config() {
        let config = crate::config::path();
        let before = std::fs::metadata(&config)
            .and_then(|meta| meta.modified())
            .ok();
        let mut state = fixtures::demo_state(Detail::Standard);
        assert!(state.demo);
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char(','));
        state.settings_row = settings::ROWS
            .iter()
            .position(|row| *row == settings::Row::Density)
            .unwrap();
        let density = state.density;
        press(&mut state, &mut ui, KeyCode::Char('l'));
        assert_ne!(state.density, density);
        assert_eq!(
            state.status,
            format!("ui.density = {} · demo, not saved", state.density.word())
        );
        press(&mut state, &mut ui, KeyCode::Esc);
        assert!(!state.settings);

        press(&mut state, &mut ui, KeyCode::Char('D'));
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Char('s'));
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.detail, Detail::Rich);
        assert_eq!(state.status, "Detail: rich · demo, not saved");

        theme_picker(&mut state, &mut ui);
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Enter);
        assert_eq!(
            state.status,
            format!("theme {} · demo, not saved", state.scheme.word())
        );

        press(&mut state, &mut ui, KeyCode::Char('F'));
        assert!(matches!(ui.mode, Mode::Filter { .. }));
        let n = state.on.len();
        press(&mut state, &mut ui, KeyCode::Enter);
        assert_eq!(
            state.status,
            format!("{n} networks in your feed · demo, not saved")
        );
        let after = std::fs::metadata(&config)
            .and_then(|meta| meta.modified())
            .ok();
        assert_eq!(before, after);
    }

    #[test]
    fn the_detail_picker_describes_rich_and_choosing_applies_it() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let rich = |state: &State| screens::detail_rows(state)[3].value.clone();
        assert_eq!(rich(&state), "Images off · ui.images");
        for (pictures, shown) in [
            ("kitty", "Images · kitty"),
            ("iTerm2", "Images · iTerm2"),
            ("sixel", "Images · sixel"),
            (
                "blocks in this terminal",
                "Images · blocks in this terminal",
            ),
        ] {
            state.pictures = Some(pictures);
            assert_eq!(rich(&state), shown);
        }
        assert_eq!(
            screens::detail_rows(&state)[2].value,
            "rails, cards and icon stats"
        );
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('D'));
        let rows = rows_of(&frame(&mut state, &ui, 80, 24));
        assert!(
            rows.iter().any(|row| row.contains("choose  close")),
            "{rows:#?}"
        );
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Enter);
        assert_eq!(state.detail, Detail::Rich);
        assert_eq!(state.status, "Detail: rich · demo, not saved");
    }

    #[test]
    fn a_chosen_detail_level_and_theme_are_written_to_the_user_config() {
        let _guard = crate::config::lock_config_env();
        let dir = temp("prefs");
        let previous = std::env::var_os("SOCLI_CONFIG_DIR");
        std::env::set_var("SOCLI_CONFIG_DIR", &dir);
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(value) => std::env::set_var("SOCLI_CONFIG_DIR", value),
                    None => std::env::remove_var("SOCLI_CONFIG_DIR"),
                }
            }
        }
        let restore = Restore(previous);
        let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('d'));
        assert_eq!(state.detail, Detail::Rich);
        assert_eq!(state.status, "Detail: rich · saved");
        press(&mut state, &mut ui, KeyCode::Char('T'));
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Enter);
        let theme = state.scheme.word();
        let config = crate::config::load().unwrap();
        assert_eq!(config.ui.detail.as_deref(), Some("rich"));
        assert_eq!(config.ui.theme.as_deref(), Some(theme.as_str()));
        drop(restore);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plain_detail_without_color_draws_no_escape() {
        let mut state = fixtures::demo_state(Detail::Plain);
        state.forced_mono = true;
        state.retheme();
        let buffer = frame(&mut state, &test_ui(), 80, 24);
        assert!(buffer
            .content
            .iter()
            .all(|cell| !cell.symbol().contains('\x1b')));
    }

    #[test]
    fn overlays_draw_in_short_viewports_and_after_a_resize() {
        let opens: [fn(&mut State, &mut Ui); 5] = [
            theme_picker,
            |state, ui| press(state, ui, KeyCode::Char('A')),
            |state, ui| press(state, ui, KeyCode::Char('?')),
            |state, ui| press(state, ui, KeyCode::Char(',')),
            |_, ui| {
                ui.mode = Mode::Compose(Box::new(composer_for(
                    "hello 👩‍💻 日本語",
                    Network::X,
                    Overlay::None,
                )))
            },
        ];
        for open in opens {
            for height in [8, 10] {
                let mut state = demo_state();
                let mut ui = test_ui();
                open(&mut state, &mut ui);
                frame(&mut state, &ui, 60, height);
                frame(&mut state, &ui, 40, 12);
            }
        }
    }

    fn focused(ui: &Ui) -> Option<Target> {
        let stops = tab_stops(&ui.hits.borrow().targets);
        let focus = ui.focus.borrow().clone()?;
        focused_index(&stops, &focus).map(|i| stops[i].clone())
    }

    /// Puts the focus ring on the first tab stop `wanted` accepts on the last frame.
    fn focus_on(ui: &Ui, wanted: impl Fn(&Target) -> bool) -> Target {
        let stops = tab_stops(&ui.hits.borrow().targets);
        let i = stops.iter().position(wanted).expect("no such tab stop");
        ui.focus.replace(Some(focus_of(&stops, i)));
        stops[i].clone()
    }

    fn target_where(ui: &Ui, wanted: impl Fn(&Target) -> bool) -> Target {
        ui.hits
            .borrow()
            .targets
            .iter()
            .find(|t| wanted(t))
            .cloned()
            .expect("no such target")
    }

    fn mouse(state: &mut State, ui: &mut Ui, kind: MouseEventKind, x: usize, y: usize) {
        let event = MouseEvent {
            kind,
            column: x as u16,
            row: y as u16,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(state, &mut Clients::empty(), ui, event).unwrap();
    }

    fn click_at(state: &mut State, ui: &mut Ui, x: usize, y: usize) {
        mouse(state, ui, MouseEventKind::Down(MouseButton::Left), x, y);
    }

    fn click_on(state: &mut State, ui: &mut Ui, target: &Target) {
        click_at(state, ui, usize::from(target.x), usize::from(target.y));
    }

    fn dialog_row(ui: &Ui, pick: Pick) -> (usize, usize) {
        let hits = ui.hits.borrow();
        let (y, cols, _) = hits
            .rows
            .iter()
            .find(|(_, _, p)| *p == pick)
            .expect("no such dialog row");
        (cols.start + 2, *y)
    }

    #[test]
    fn profile_share_targets_the_profile_even_with_a_post_selected() {
        for detail in DETAILS {
            let mut state = fixtures::demo_state(detail);
            state
                .open_user(
                    Scope::One(Network::X),
                    "river".into(),
                    &mut Clients::empty(),
                )
                .unwrap();
            let mut ui = test_ui();
            press(&mut state, &mut ui, KeyCode::Char('j'));
            assert!(state.selected_post().is_some());
            frame(&mut state, &ui, 100, 40);
            let target = target_where(&ui, |target| matches!(target.act, Act::ShareProfile(..)));
            click_on(&mut state, &mut ui, &target);
            let Mode::Share(share) = &ui.mode else {
                panic!("profile sharing is open")
            };
            assert_eq!(share.post, None);
            assert_eq!(share.profile_url, "https://x.com/river");
        }
    }

    #[test]
    fn the_overflow_marker_opens_the_hidden_tabs() {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.reveal_shortcuts = true;
        let mut ui = test_ui();
        frame(&mut state, &ui, 32, 20);
        let marker = target_where(&ui, |target| {
            matches!(
                target.act,
                Act::Overflow(ref items)
                    if items.iter().any(|item| matches!(item.act.as_ref(), Act::Tab(_)))
            )
        });
        click_on(&mut state, &mut ui, &marker);
        assert!(matches!(ui.mode, Mode::Overflow(_)));
        let text: String = frame(&mut state, &ui, 32, 20)
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Discover"), "{text}");
        assert!(text.contains("Saved"), "{text}");
        click_at(&mut state, &mut ui, 0, 0);
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.merged_tab, MergedTab::Home);
        click_on(&mut state, &mut ui, &marker);
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.merged_tab, MergedTab::Saved);
    }

    #[test]
    fn profile_views_draw_at_every_viewport_and_detail_level() {
        for (width, height) in VIEWPORTS {
            for detail in DETAILS {
                let mut state = fixtures::demo_state(detail);
                state
                    .open_user(
                        Scope::One(Network::X),
                        "river".into(),
                        &mut Clients::empty(),
                    )
                    .unwrap();
                let mut ui = test_ui();
                let buffer = frame(&mut state, &ui, width, height);
                let text = buffer
                    .content()
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                if width >= 44 {
                    assert!(text.contains("River Hale"), "{width}x{height} {detail:?}");
                }
                if ui.viewport.get().1 >= 6 && width >= 40 {
                    assert!(
                        text.contains("Half-block"),
                        "the first post at {width}x{height} {detail:?}"
                    );
                }
                if ui.viewport.get().1 >= 6 && width < 40 {
                    let mut after = String::new();
                    for _ in 0..40 {
                        press(&mut state, &mut ui, KeyCode::Char('j'));
                        after = frame(&mut state, &ui, width, height)
                            .content()
                            .iter()
                            .map(|cell| cell.symbol())
                            .collect();
                        if after.contains("Half-block") {
                            break;
                        }
                    }
                    assert!(
                        after.contains("Half-block"),
                        "the first post after j at {width}x{height} {detail:?}"
                    );
                }
                assert_eq!(buffer.area.width, width);
                assert_eq!(buffer.area.height, height);
                assert_inside(
                    &state,
                    &ui,
                    width,
                    height,
                    &format!("profile {width}x{height} {detail:?}"),
                );
            }
        }
    }

    #[test]
    fn clicking_the_padding_of_a_repost_button_activates_that_post() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let target = target_where(&ui, |target| matches!(target.act, Act::Repost(_)));
        let Act::Repost(id) = &target.act else {
            unreachable!()
        };
        let before = state.posts[id].reposted;
        assert_eq!(target.width, 7);
        click_at(
            &mut state,
            &mut ui,
            usize::from(target.x + target.width - 1),
            usize::from(target.y),
        );
        assert_eq!(state.posts[id].reposted, !before);
        assert_eq!(state.selected_post_id().as_deref(), Some(id.as_str()));
    }

    #[test]
    fn tab_from_no_focus_takes_the_first_header_target() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        press(&mut state, &mut ui, KeyCode::Tab);
        let first = focused(&ui).expect("a focus");
        assert_eq!(first.zone, Zone::Header);
        assert_eq!(first.act, Act::Crumb(0));
    }

    #[test]
    fn tab_follows_zone_row_and_column_skips_hints_and_wraps() {
        let mut state = demo_state();
        let mut ui = test_ui();
        let mut seen = Vec::new();
        let mut first = None;
        for _ in 0..400 {
            frame(&mut state, &ui, 104, 44);
            press(&mut state, &mut ui, KeyCode::Tab);
            let target = focused(&ui).expect("a focus");
            let key = (target.zone, target.y, target.x, target.act.clone());
            if first.is_none() {
                first = Some(key);
            } else if first.as_ref() == Some(&key) && seen.len() > 3 {
                break;
            }
            seen.push(target);
        }
        let zones: Vec<Zone> = seen.iter().map(|t| t.zone).collect();
        assert!(zones.windows(2).all(|w| w[0] <= w[1]), "{zones:?}");
        for zone in [Zone::Header, Zone::Subbar, Zone::BodyLeft, Zone::Footer] {
            assert!(zones.contains(&zone), "{zone:?} {zones:?}");
        }
        assert!(seen.iter().all(|t| !matches!(t.act, Act::Key(_))));
        assert!(seen.iter().all(|t| t.tab));
        let header: Vec<&Target> = seen.iter().filter(|t| t.zone == Zone::Header).collect();
        assert!(header
            .windows(2)
            .all(|w| (w[0].y, w[0].x) < (w[1].y, w[1].x)));
        assert!(seen.iter().any(|target| target.act == Act::Compose));
        let last = seen.last().expect("a stop");
        assert_eq!(last.zone, Zone::Footer);
        match &last.act {
            Act::Compose => {}
            Act::Overflow(items) => assert!(
                items
                    .iter()
                    .any(|item| matches!(item.act.as_ref(), Act::Key(_))),
                "{items:?}"
            ),
            other => panic!("the ring ends on new post or the hidden hints, not {other:?}"),
        }
    }

    #[test]
    fn shift_tab_from_no_focus_takes_the_last_target() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        press(&mut state, &mut ui, KeyCode::BackTab);
        let last = focused(&ui).expect("a focus");
        assert_eq!((last.zone, last.act), (Zone::Footer, Act::Compose));
        frame(&mut state, &ui, 104, 44);
        press(&mut state, &mut ui, KeyCode::Tab);
        assert_eq!(focused(&ui).map(|t| t.zone), Some(Zone::Header));
    }

    #[test]
    fn enter_on_a_focused_mention_opens_that_users_posts() {
        let mut state = demo_state();
        let mut ui = test_ui();
        let mut found = false;
        for _ in 0..40 {
            frame(&mut state, &ui, 104, 44);
            found = ui
                .hits
                .borrow()
                .targets
                .iter()
                .any(|t| t.tab && matches!(t.act, Act::Mention { .. }));
            if found {
                break;
            }
            press(&mut state, &mut ui, KeyCode::Char('j'));
        }
        assert!(found, "no mention on screen");
        let target = focus_on(&ui, |t| matches!(t.act, Act::Mention { .. }));
        let Act::Mention { post, handle } = target.act.clone() else {
            unreachable!()
        };
        let net = net::split(&post).unwrap().0;
        press(&mut state, &mut ui, KeyCode::Enter);
        assert_eq!(state.section, Section::Search);
        assert_eq!(
            state.remote,
            Some(crate::model::FeedId::User(Scope::One(net), handle))
        );
        assert!(ui.focus.borrow().is_none());
    }

    #[test]
    fn enter_on_a_focused_nav_section_switches_section() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        focus_on(&ui, |t| {
            t.zone == Zone::Footer && t.act == Act::Section(Section::Notifications)
        });
        press(&mut state, &mut ui, KeyCode::Enter);
        assert_eq!(state.section, Section::Notifications);
    }

    #[test]
    fn j_clears_the_focus_and_moves_the_selection() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let before = state.selected_post_id();
        press(&mut state, &mut ui, KeyCode::Tab);
        assert!(ui.focus.borrow().is_some());
        press(&mut state, &mut ui, KeyCode::Char('j'));
        assert!(ui.focus.borrow().is_none());
        assert_ne!(state.selected_post_id(), before);
    }

    #[test]
    fn esc_with_a_focus_only_clears_it() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        press(&mut state, &mut ui, KeyCode::Char('j'));
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(state.thread_focus.is_some());
        frame(&mut state, &ui, 104, 44);
        let selected = state.selected_post_id();
        press(&mut state, &mut ui, KeyCode::Tab);
        press(&mut state, &mut ui, KeyCode::Esc);
        assert!(ui.focus.borrow().is_none());
        assert!(state.thread_focus.is_some());
        assert_eq!(state.selected_post_id(), selected);
    }

    #[test]
    fn tab_in_the_keyed_pane_selects_the_post_it_lands_on() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let first = state.selected_post_id().unwrap();
        let target = focus_on(&ui, |t| {
            t.zone == Zone::BodyLeft && t.act.post().is_some_and(|id| id != first)
        });
        let other = target.act.post().unwrap().to_string();
        let stops = tab_stops(&ui.hits.borrow().targets);
        let i = focused_index(&stops, ui.focus.borrow().as_ref().unwrap()).unwrap();
        ui.focus.replace(Some(focus_of(&stops, i - 1)));
        press(&mut state, &mut ui, KeyCode::Tab);
        assert_eq!(state.selected_post_id(), Some(other));
    }

    #[test]
    fn tab_into_the_preview_only_moves_the_highlight_and_enter_moves_the_keys() {
        let mut state = demo_state();
        state.layout = Layout::Auto;
        let mut ui = test_ui();
        frame(&mut state, &ui, 160, 44);
        assert!(ui.split.get());
        let selected = state.selected_post_id();
        let key = state.selection_key();
        for _ in 0..200 {
            frame(&mut state, &ui, 160, 44);
            press(&mut state, &mut ui, KeyCode::Tab);
            if focused(&ui).is_some_and(|t| t.zone == Zone::BodyRight) {
                break;
            }
        }
        assert_eq!(focused(&ui).map(|t| t.zone), Some(Zone::BodyRight));
        let feed_selected = state.selected_post_id();
        for _ in 0..3 {
            frame(&mut state, &ui, 160, 44);
            press(&mut state, &mut ui, KeyCode::Tab);
        }
        assert!(state.thread_focus.is_none());
        assert_eq!(state.selection_key(), key);
        assert_eq!(state.selected_post_id(), feed_selected);
        assert!(selected.is_some());

        // The post under the focus ring may belong to a network with no like. Step to one that has.
        for _ in 0..12 {
            frame(&mut state, &ui, 160, 44);
            let ready = tab_stops(&ui.hits.borrow().targets)
                .iter()
                .any(|t| t.zone == Zone::BodyRight && matches!(t.act, Act::Like(_)));
            if ready {
                break;
            }
            press(&mut state, &mut ui, KeyCode::Char('k'));
        }
        let target = focus_on(&ui, |t| {
            t.zone == Zone::BodyRight && matches!(t.act, Act::Like(_))
        });
        let id = target.act.post().unwrap().to_string();
        let liked = state.posts[&id].liked;
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(state.thread_focus.is_some());
        assert_eq!(state.selected_post_id(), Some(id.clone()));
        assert_ne!(state.posts[&id].liked, liked);
    }

    #[test]
    fn the_focus_ring_uses_underlines_in_every_theme() {
        let mut state = demo_state();
        state.scheme = Scheme::parse("black").unwrap();
        state.retheme();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        press(&mut state, &mut ui, KeyCode::Tab);
        let buffer = frame(&mut state, &ui, 104, 44);
        let target = focused(&ui).unwrap();
        let cell = &buffer[(target.x, target.y)];
        assert_eq!(cell.fg, state.theme.fg(Kind::Blue, false));
        assert!(cell
            .modifier
            .contains(Modifier::UNDERLINED | Modifier::BOLD));

        for native in [true, false] {
            let mut state = demo_state();
            if native {
                state.scheme = Scheme::parse("native").unwrap();
            } else {
                state.color_mode = crate::theme::ColorMode::Mono;
            }
            state.retheme();
            assert!(state.theme.native || state.theme.mono);
            let mut ui = test_ui();
            frame(&mut state, &ui, 104, 44);
            press(&mut state, &mut ui, KeyCode::Tab);
            let buffer = frame(&mut state, &ui, 104, 44);
            let target = focused(&ui).unwrap();
            let cell = &buffer[(target.x, target.y)];
            assert!(
                cell.modifier
                    .contains(Modifier::UNDERLINED | Modifier::BOLD),
                "native {native}"
            );
        }
    }

    #[test]
    fn a_click_on_a_nav_section_switches_section() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let target = target_where(&ui, |t| {
            t.zone == Zone::Footer && t.act == Act::Section(Section::Notifications)
        });
        click_on(&mut state, &mut ui, &target);
        assert_eq!(state.section, Section::Notifications);
        assert!(ui.focus.borrow().is_none());
    }

    #[test]
    fn a_click_selects_a_post_and_a_second_click_opens_it() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let selected = state.selected_index().unwrap();
        let (x, y) = {
            let hits = ui.hits.borrow();
            let pane = &hits.panes[0];
            let next = crate::layout::selectable(&state.lay(&state.current_posts()))
                .into_iter()
                .find(|row| *row > selected)
                .unwrap();
            let span = pane.spans[next];
            (
                pane.cols.end - 2,
                pane.rows.start + span.start - pane.offset,
            )
        };
        click_at(&mut state, &mut ui, x, y);
        assert_ne!(state.selected_index(), Some(selected));
        assert!(state.thread_focus.is_none());
        frame(&mut state, &ui, 104, 44);
        click_at(&mut state, &mut ui, x, y);
        assert!(state.thread_focus.is_some());
    }

    #[test]
    fn a_click_on_a_link_card_row_resolves_to_its_link() {
        let mut state = demo_state();
        let ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let card = target_where(&ui, |t| !t.tab && matches!(t.act, Act::Link { .. }));
        let Act::Link { post, url } = &card.act else {
            unreachable!()
        };
        assert_eq!(*url, state.posts[post].links[0].expanded);
        let rows: Vec<Target> = ui
            .hits
            .borrow()
            .targets
            .iter()
            .filter(|t| t.act == card.act && t.x == card.x && t.width == card.width)
            .cloned()
            .collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows.iter().filter(|t| t.tab).count(), 1);
        for row in &rows {
            let x = usize::from(row.x + row.width / 2);
            let hit = target_at(&ui, x, usize::from(row.y)).expect("a target");
            assert_eq!(hit.act, card.act);
        }
    }

    #[test]
    fn a_click_on_a_hint_runs_its_key() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let target = target_where(&ui, |t| t.act == Act::Key(KeyCode::Char('?')));
        assert!(!target.tab);
        click_on(&mut state, &mut ui, &target);
        assert!(matches!(ui.mode, Mode::Help));
    }

    #[test]
    fn a_click_outside_a_picker_closes_it_and_the_theme_picker_restores() {
        let mut state = demo_state();
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('A'));
        frame(&mut state, &ui, 104, 44);
        click_at(&mut state, &mut ui, 0, 0);
        assert!(matches!(ui.mode, Mode::Normal));

        let before = state.scheme;
        ui.mode = PickKind::Theme.open(&state);
        press(&mut state, &mut ui, KeyCode::Char('j'));
        assert_ne!(state.scheme, before);
        frame(&mut state, &ui, 104, 44);
        click_at(&mut state, &mut ui, 0, 0);
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.scheme, before);
    }

    #[test]
    fn a_click_on_a_picker_row_picks_it() {
        let mut state = demo_state();
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('A'));
        frame(&mut state, &ui, 104, 44);
        let (x, y) = dialog_row(&ui, Pick::Row(3));
        click_at(&mut state, &mut ui, x, y);
        assert!(matches!(ui.mode, Mode::Normal));
        assert_eq!(state.scope(), Scope::One(Network::Devnull));
    }

    #[test]
    fn the_wheel_moves_the_selection_the_cursor_and_the_help() {
        let mut state = demo_state();
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let before = state.selected_post_id();
        mouse(&mut state, &mut ui, MouseEventKind::ScrollDown, 20, 12);
        assert_ne!(state.selected_post_id(), before);
        frame(&mut state, &ui, 104, 44);
        mouse(&mut state, &mut ui, MouseEventKind::ScrollUp, 20, 12);
        assert_eq!(state.selected_post_id(), before);

        press(&mut state, &mut ui, KeyCode::Char('A'));
        frame(&mut state, &ui, 104, 44);
        mouse(&mut state, &mut ui, MouseEventKind::ScrollDown, 20, 12);
        assert_eq!(cursor_of(&ui), Some((PickKind::Network, 1)));
        press(&mut state, &mut ui, KeyCode::Esc);

        press(&mut state, &mut ui, KeyCode::Char('?'));
        frame(&mut state, &ui, 60, 16);
        assert!(ui.overflow.get() >= 3);
        mouse(&mut state, &mut ui, MouseEventKind::ScrollDown, 20, 8);
        assert_eq!(ui.scroll.get(), 3);
        assert!(matches!(ui.mode, Mode::Help));
    }

    #[test]
    fn hovering_a_picker_row_moves_the_cursor() {
        let mut state = demo_state();
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('A'));
        frame(&mut state, &ui, 104, 44);
        let (x, y) = dialog_row(&ui, Pick::Row(2));
        mouse(&mut state, &mut ui, MouseEventKind::Moved, x, y);
        assert_eq!(cursor_of(&ui), Some((PickKind::Network, 2)));
        assert_eq!(state.scope(), Scope::All);
    }

    #[test]
    fn settings_clicks_step_values_and_move_the_cursor() {
        let mut state = demo_state();
        state.open_settings();
        let ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        let (y, hit) = ui
            .hits
            .borrow()
            .settings
            .iter()
            .copied()
            .find(|(_, hit)| hit.row == Row::Density)
            .unwrap();
        let (left, right) = hit.arrows.unwrap();
        let margin = ui.hits.borrow().margin;
        assert_eq!(
            settings_click(&mut state, &ui, margin + right, y),
            SettingsClick::Step(Row::Density, 1)
        );
        assert_eq!(
            settings_click(&mut state, &ui, margin + left, y),
            SettingsClick::Step(Row::Density, -1)
        );
        let theme = ui
            .hits
            .borrow()
            .settings
            .iter()
            .find(|(_, hit)| hit.row == Row::Theme)
            .map(|(y, _)| *y)
            .unwrap();
        assert_eq!(
            settings_click(&mut state, &ui, margin + 3, theme),
            SettingsClick::None
        );
        assert_eq!(settings::ROWS[state.settings_row], Row::Theme);
        assert_eq!(
            settings_click(&mut state, &ui, margin + 3, theme),
            SettingsClick::Press(KeyCode::Enter)
        );
        let rows = settings::ROWS.len();
        let mut ui = test_ui();
        state.settings_row = rows - 1;
        press(&mut state, &mut ui, KeyCode::Tab);
        assert_eq!(state.settings_row, 0);
        press(&mut state, &mut ui, KeyCode::BackTab);
        assert_eq!(settings::ROWS[state.settings_row], Row::Enter);
    }

    #[test]
    fn the_accounts_page_signs_in_from_the_reader_without_saving_the_demo() {
        let mut state = demo_state();
        state.nets.retain(|info| info.net != Network::Threads);
        let on = state.on.clone();
        let mut ui = test_ui();
        state.open_settings();
        press(&mut state, &mut ui, KeyCode::Enter);
        assert_eq!(state.settings_page, SettingsPage::Accounts);
        assert_eq!(state.crumb(), "settings › accounts");
        assert!(state.settings);
        press(&mut state, &mut ui, KeyCode::Char(' '));
        assert!(
            state.status.contains("X · demo, not saved"),
            "{}",
            state.status
        );
        assert!(state.temporary_status());
        press(&mut state, &mut ui, KeyCode::Esc);
        assert_eq!(state.settings_page, SettingsPage::Main);
        assert!(state.settings);
        assert_eq!(settings::ROWS[state.settings_row], Row::Accounts);

        state.settings_row = settings::ROWS
            .iter()
            .position(|row| *row == Row::Net(Network::Threads))
            .unwrap();
        press(&mut state, &mut ui, KeyCode::Char(' '));
        assert!(
            state.status.contains("Threads · demo, not saved"),
            "{}",
            state.status
        );
        assert_eq!(state.on, on);
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(
            state.status.contains("Threads · demo, not saved"),
            "{}",
            state.status
        );
    }

    #[test]
    fn tab_moves_the_cursor_in_the_filter_panel_and_the_share_dialog() {
        let mut state = demo_state();
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('F'));
        press(&mut state, &mut ui, KeyCode::Tab);
        assert!(matches!(ui.mode, Mode::Filter { cursor: 1, .. }));
        press(&mut state, &mut ui, KeyCode::BackTab);
        assert!(matches!(ui.mode, Mode::Filter { cursor: 0, .. }));
        press(&mut state, &mut ui, KeyCode::Esc);

        press(&mut state, &mut ui, KeyCode::Char('s'));
        press(&mut state, &mut ui, KeyCode::Tab);
        let Mode::Share(share) = &ui.mode else {
            panic!("the share dialog is open")
        };
        assert_eq!(share.row, 1);
    }
    #[test]
    fn holding_control_or_command_reveals_shortcuts_without_changing_focus_or_selection() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        frame(&mut state, &ui, 100, 40);
        press(&mut state, &mut ui, KeyCode::Tab);
        let focus = ui.focus.borrow().clone();
        let selected = state.selected_post_id();
        let modifier = |side, kind| {
            KeyEvent::new_with_kind(KeyCode::Modifier(side), KeyModifiers::CONTROL, kind)
        };
        let left = modifier(ModifierKeyCode::LeftControl, KeyEventKind::Press);
        handle_key(&mut state, &mut Clients::empty(), &mut ui, left).unwrap();
        assert!(state.reveal_shortcuts);
        assert_eq!(*ui.focus.borrow(), focus);
        assert_eq!(state.selected_post_id(), selected);
        let buffer = frame(&mut state, &ui, 100, 40);
        let rows = rows_of(&buffer);
        assert!(rows[3].contains("[1 Home]"));
        assert!(rows[38].contains("[n] new post"));
        for target in &ui.hits.borrow().targets {
            for x in target.x..target.x + target.width {
                let cell = &buffer[(x, target.y)];
                if !cell.symbol().trim().is_empty() {
                    assert!(cell.modifier.contains(Modifier::UNDERLINED));
                }
            }
        }
        let right = modifier(ModifierKeyCode::RightSuper, KeyEventKind::Press);
        handle_key(&mut state, &mut Clients::empty(), &mut ui, right).unwrap();
        let released = modifier(ModifierKeyCode::LeftControl, KeyEventKind::Release);
        assert!(matches!(
            queued(Event::Key(released)),
            Some(Queued::Press(Press::Key(_)))
        ));
        handle_key(&mut state, &mut Clients::empty(), &mut ui, released).unwrap();
        assert!(state.reveal_shortcuts);
        handle_key(
            &mut state,
            &mut Clients::empty(),
            &mut ui,
            modifier(ModifierKeyCode::RightSuper, KeyEventKind::Release),
        )
        .unwrap();
        assert!(!state.reveal_shortcuts);
        let rows = rows_of(&frame(&mut state, &ui, 100, 40));
        assert!(!rows[3].contains("[1 Home]"));
        assert!(!rows[38].contains("[n]"));
        assert!(rows[39].trim_end().ends_with("help"));
        assert!(rows[39].contains("? help"));
        handle_key(&mut state, &mut Clients::empty(), &mut ui, left).unwrap();
        let Some(Queued::Press(Press::Key(lost))) = queued(Event::FocusLost) else {
            panic!("focus loss must clear disclosure");
        };
        handle_key(&mut state, &mut Clients::empty(), &mut ui, lost).unwrap();
        assert!(!state.reveal_shortcuts);
    }

    #[test]
    fn palette_uses_the_detail_level_and_keeps_every_hit_inside_the_viewport() {
        for detail in DETAILS {
            for (width, height) in VIEWPORTS {
                let mut state = fixtures::demo_state(detail);
                let mut ui = test_ui();
                press(&mut state, &mut ui, KeyCode::Char(':'));
                let buffer = frame(&mut state, &ui, width, height);
                let simple = matches!(detail, Detail::Plain | Detail::Minimal);
                let hits = ui.hits.borrow();
                assert_eq!(
                    hits.dialog.is_some(),
                    !simple,
                    "{detail:?} {width}x{height}"
                );
                for (row, cols, _) in &hits.rows {
                    assert!(
                        *row < height as usize && cols.end <= width as usize,
                        "{detail:?} {width}x{height}: {row}, {cols:?}"
                    );
                }
                if let Some((rows, cols)) = &hits.dialog {
                    assert!(rows.end <= height as usize && cols.end <= width as usize);
                    assert!(cols.start.abs_diff(width as usize - cols.end) <= 1);
                    assert!(rows.start.abs_diff(height as usize - rows.end) <= 1);
                }
                if simple {
                    assert!(!rows_of(&buffer).iter().any(|row| row.contains('╭')));
                }
            }
        }
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        ui.mode = Mode::Command {
            input: Input::from("unrecognized"),
            pick: 0,
        };
        let rows = rows_of(&frame(&mut state, &ui, 40, 12));
        assert!(rows.iter().any(|row| row.contains("No matching commands")));
        press(&mut state, &mut ui, KeyCode::Esc);
        press(&mut state, &mut ui, KeyCode::Char('T'));
        assert!(matches!(
            ui.mode,
            Mode::Pick(Picker {
                kind: PickKind::Theme,
                ..
            })
        ));
    }

    #[test]
    fn breadcrumb_clicks_return_to_the_parent_and_leave_settings() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Enter);
        assert!(state.thread_focus.is_some());
        frame(&mut state, &ui, 100, 40);
        let current = target_where(&ui, |t| t.act == Act::Crumb(3));
        click_on(&mut state, &mut ui, &current);
        assert!(state.thread_focus.is_some());
        let parent = target_where(&ui, |t| t.act == Act::Crumb(2));
        click_on(&mut state, &mut ui, &parent);
        assert!(state.thread_focus.is_none());
        state.open_settings();
        frame(&mut state, &ui, 100, 40);
        let home = target_where(&ui, |t| t.act == Act::Crumb(0));
        click_on(&mut state, &mut ui, &home);
        assert!(!state.settings);
        assert_eq!(state.section, Section::Home);
    }

    #[test]
    fn footer_suggests_the_focused_action_and_keeps_help_on_the_right() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let ui = test_ui();
        frame(&mut state, &ui, 100, 40);
        focus_on(&ui, |t| matches!(t.act, Act::Link { .. }));
        let rows = rows_of(&frame(&mut state, &ui, 100, 40));
        assert!(rows[39].contains("open link"));
        assert!(rows[39].trim_end().ends_with("help"));
        assert!(!rows[39].contains("j/k"));
    }

    #[test]
    fn standard_keeps_thread_rails_without_letter_avatars() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let text = state
            .view(100, true)
            .lines
            .iter()
            .map(StyledLine::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains("│ N │") && !text.contains("│ P │"));
        assert!(text.contains('╎') && text.contains("├╌╌╌"));
        state.detail = Detail::Rich;
        let rich = state
            .view(100, true)
            .lines
            .iter()
            .map(StyledLine::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rich.contains("│ N │") && rich.contains("┤ P │"));
    }

    #[test]
    fn refinement_gallery_covers_all_detail_levels_and_themes() {
        for detail in DETAILS {
            for light in [false, true] {
                for reveal in [false, true] {
                    for palette in [false, true] {
                        let mut state = fixtures::demo_state(detail);
                        state.scheme = if light {
                            Scheme::parse("white").unwrap()
                        } else {
                            Scheme::parse("black").unwrap()
                        };
                        state.retheme();
                        state.reveal_shortcuts = reveal;
                        let mut ui = test_ui();
                        if palette {
                            ui.mode = Mode::Command {
                                input: Input::from("theme"),
                                pick: 0,
                            };
                        }
                        let buffer = frame(&mut state, &ui, 100, 40);
                        if let Ok(folder) = std::env::var("SOCLI_UI_SNAPSHOTS") {
                            let path = std::path::Path::new(&folder);
                            std::fs::create_dir_all(path).unwrap();
                            let cells: Vec<_> = buffer.content.iter().map(|cell| serde_json::json!({ "text": cell.symbol(), "fg": format!("{:?}", cell.fg), "bg": format!("{:?}", cell.bg), "modifiers": format!("{:?}", cell.modifier) })).collect();
                            let name = format!(
                                "{}-{}-{}-{}.json",
                                detail.word(),
                                if light { "light" } else { "dark" },
                                if reveal { "shift" } else { "quiet" },
                                if palette { "palette" } else { "feed" }
                            );
                            std::fs::write(
                                path.join(name),
                                serde_json::to_vec(
                                    &serde_json::json!({"width":100,"height":40,"cells":cells}),
                                )
                                .unwrap(),
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn composer_actions_hide_keys_and_accept_mouse_clicks() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('n'));
        let rows = rows_of(&frame(&mut state, &ui, 100, 40));
        assert!(rows.last().unwrap().contains("post  newline"));
        assert!(rows.last().unwrap().contains("cancel"));
        assert!(!rows.last().unwrap().contains("ctrl-j"));
        let newline = target_where(&ui, |t| {
            t.act == Act::Input(KeyCode::Char('j'), KeyModifiers::CONTROL)
        });
        click_on(&mut state, &mut ui, &newline);
        let Mode::Compose(composer) = &ui.mode else {
            panic!("composer remains open");
        };
        assert_eq!(composer.input.text(), "\n");
        state.reveal_shortcuts = true;
        let rows = rows_of(&frame(&mut state, &ui, 100, 40));
        assert!(rows.last().unwrap().contains("ctrl-j newline"));
    }
    #[test]
    fn shift_preserves_uppercase_shortcuts_and_composer_input() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        let mut clients = Clients::empty();
        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            KeyEvent::new(
                KeyCode::Modifier(ModifierKeyCode::LeftShift),
                KeyModifiers::SHIFT,
            ),
        )
        .unwrap();
        assert!(!state.reveal_shortcuts);
        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('d'), KeyModifiers::SHIFT),
        )
        .unwrap();
        assert_eq!(cursor_of(&ui).map(|(kind, _)| kind), Some(PickKind::Detail));
        press(&mut state, &mut ui, KeyCode::Esc);
        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('a'), KeyModifiers::SHIFT),
        )
        .unwrap();
        assert_eq!(
            cursor_of(&ui).map(|(kind, _)| kind),
            Some(PickKind::Network)
        );
        press(&mut state, &mut ui, KeyCode::Esc);
        press(&mut state, &mut ui, KeyCode::Char('n'));
        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('a'), KeyModifiers::SHIFT),
        )
        .unwrap();
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('?'))).unwrap();
        ui.enhanced = true;
        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Enter, KeyModifiers::SHIFT),
        )
        .unwrap();
        let Mode::Compose(composer) = &ui.mode else {
            panic!("Shift-Enter must not submit");
        };
        assert_eq!(composer.input.text(), "A?\n");
        assert!(!state.reveal_shortcuts);
    }

    #[test]
    fn native_command_chords_do_not_type_activate_or_quit() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        let selected = state.selected_post_id();
        for modifier in [KeyModifiers::SUPER, KeyModifiers::META] {
            for ch in ['c', 'v', 'n', 'q', 'D'] {
                assert!(!handle_key(
                    &mut state,
                    &mut Clients::empty(),
                    &mut ui,
                    with(KeyCode::Char(ch), modifier)
                )
                .unwrap());
                assert!(matches!(ui.mode, Mode::Normal));
                assert_eq!(state.selected_post_id(), selected);
            }
        }
        press(&mut state, &mut ui, KeyCode::Char('n'));
        for modifier in [KeyModifiers::SUPER, KeyModifiers::META] {
            handle_key(
                &mut state,
                &mut Clients::empty(),
                &mut ui,
                with(KeyCode::Char('c'), modifier),
            )
            .unwrap();
            assert!(!submits(with(KeyCode::Enter, modifier), Enter::Post, true));
            assert!(!breaks_line(
                with(KeyCode::Enter, modifier),
                Enter::Newline,
                true
            ));
        }
        let Mode::Compose(composer) = &ui.mode else {
            panic!("composer remains open");
        };
        assert!(composer.input.text().is_empty());
        ui.mode = Mode::Normal;
        assert!(handle_key(
            &mut state,
            &mut Clients::empty(),
            &mut ui,
            with(KeyCode::Char('c'), KeyModifiers::CONTROL)
        )
        .unwrap());
    }

    #[test]
    fn ctrl_or_cmd_r_reloads_the_current_view_and_r_still_replies() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        let mut clients = Clients::empty();

        fn reload(state: &mut State, clients: &mut Clients, ui: &mut Ui, key: KeyEvent) {
            state.status = "stale".into();
            state.status_error = None;
            state.status_expiry = None;
            assert!(!handle_key(state, clients, ui, key).unwrap());
            assert_eq!(
                state.status,
                "demo · already local",
                "{key:?} feed {:?} section {:?}",
                state.current_feed_id(),
                state.section
            );
        }

        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('r'))).unwrap();
        assert_ne!(state.status, "demo · already local");
        assert!(matches!(ui.mode, Mode::Compose(_)));
        press(&mut state, &mut ui, KeyCode::Esc);
        assert!(matches!(ui.mode, Mode::Normal));

        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::ALT),
        )
        .unwrap();
        assert_ne!(state.status, "demo · already local");
        assert!(matches!(ui.mode, Mode::Compose(_)));
        press(&mut state, &mut ui, KeyCode::Esc);

        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::SUPER,
            KeyModifiers::META,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyModifiers::SUPER | KeyModifiers::SHIFT,
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ] {
            reload(
                &mut state,
                &mut clients,
                &mut ui,
                with(KeyCode::Char('r'), modifiers),
            );
            reload(
                &mut state,
                &mut clients,
                &mut ui,
                with(KeyCode::Char('R'), modifiers),
            );
            assert!(matches!(ui.mode, Mode::Normal));
        }

        press(&mut state, &mut ui, KeyCode::Char('n'));
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('h'))).unwrap();
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('i'))).unwrap();
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::SUPER),
        );
        let Mode::Compose(composer) = &ui.mode else {
            panic!("composer stays open");
        };
        assert_eq!(composer.input.text(), "hi");
        ui.mode = Mode::Normal;

        press(&mut state, &mut ui, KeyCode::Char(','));
        assert!(state.settings);
        let row = state.settings_row;
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert!(state.settings);
        assert_eq!(state.settings_row, row);
        press(&mut state, &mut ui, KeyCode::Esc);
        assert!(!state.settings);

        press(&mut state, &mut ui, KeyCode::Char(':'));
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('h'))).unwrap();
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('R'), KeyModifiers::SUPER),
        );
        let Mode::Command { input, .. } = &ui.mode else {
            panic!("command line stays open");
        };
        assert_eq!(input.text(), "h");
        press(&mut state, &mut ui, KeyCode::Esc);

        press(&mut state, &mut ui, KeyCode::Char('?'));
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert!(matches!(ui.mode, Mode::Help));
        press(&mut state, &mut ui, KeyCode::Esc);

        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('a'), KeyModifiers::CONTROL),
        )
        .unwrap();
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('x'))).unwrap();
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        let Mode::Ask { input, .. } = &ui.mode else {
            panic!("ask stays open");
        };
        assert_eq!(input.text(), "x");
        press(&mut state, &mut ui, KeyCode::Esc);

        press(&mut state, &mut ui, KeyCode::Char('.'));
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert!(matches!(ui.mode, Mode::Inspect { .. }));
        press(&mut state, &mut ui, KeyCode::Esc);

        press(&mut state, &mut ui, KeyCode::Char('s'));
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::SUPER),
        );
        assert!(matches!(ui.mode, Mode::Share(_)));
        press(&mut state, &mut ui, KeyCode::Esc);

        press(&mut state, &mut ui, KeyCode::Char('D'));
        let cursor = cursor_of(&ui);
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert_eq!(cursor_of(&ui), cursor);
        press(&mut state, &mut ui, KeyCode::Esc);

        press(&mut state, &mut ui, KeyCode::Char('F'));
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::META),
        );
        assert!(matches!(ui.mode, Mode::Filter { .. }));
        press(&mut state, &mut ui, KeyCode::Esc);

        press(&mut state, &mut ui, KeyCode::Char('f'));
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('q'))).unwrap();
        state.status = "stale".into();
        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::META),
        )
        .unwrap();
        let Mode::Search(input) = &ui.mode else {
            panic!("search stays open");
        };
        assert_eq!(input.text(), "q");
        assert_eq!(state.status, "stale");
        press(&mut state, &mut ui, KeyCode::Enter);
        let remote = state.remote.clone();
        assert!(remote.is_some());
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.section, Section::Search);
        assert_eq!(state.remote, remote);

        state
            .apply(Cmd::Section(Section::Notifications), &mut clients)
            .unwrap();
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.section, Section::Notifications);
        assert!(matches!(ui.mode, Mode::Normal));

        state
            .open_user(Scope::One(Network::X), "river".into(), &mut clients)
            .unwrap();
        let remote = state.remote.clone();
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::SUPER),
        );
        assert_eq!(state.section, Section::Search);
        assert_eq!(state.remote, remote);

        loop {
            let before = (state.section, state.focus.clone(), state.crumb());
            state.apply(Cmd::Back, &mut clients).unwrap();
            if (state.section, state.focus.clone(), state.crumb()) == before {
                break;
            }
        }
        assert_eq!(state.section, Section::Home);
        assert!(state.focus.is_none());
        state.apply(Cmd::Expand, &mut clients).unwrap();
        let focus = state.focus.clone();
        assert!(focus.is_some());
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert_eq!(state.focus, focus);
        assert!(matches!(ui.mode, Mode::Normal));

        handle_key(
            &mut state,
            &mut clients,
            &mut ui,
            KeyEvent::new(
                KeyCode::Modifier(ModifierKeyCode::LeftControl),
                KeyModifiers::CONTROL,
            ),
        )
        .unwrap();
        assert!(state.reveal_shortcuts);
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert!(state.reveal_shortcuts);
    }

    #[test]
    fn control_chords_keep_their_actions_while_disclosure_is_held() {
        let mut state = fixtures::demo_state(Detail::Standard);
        let mut ui = test_ui();
        handle_key(
            &mut state,
            &mut Clients::empty(),
            &mut ui,
            KeyEvent::new(
                KeyCode::Modifier(ModifierKeyCode::LeftControl),
                KeyModifiers::CONTROL,
            ),
        )
        .unwrap();
        handle_key(
            &mut state,
            &mut Clients::empty(),
            &mut ui,
            with(KeyCode::Char('k'), KeyModifiers::CONTROL),
        )
        .unwrap();
        assert!(state.reveal_shortcuts);
        assert!(matches!(ui.mode, Mode::Command { .. }));
        let mut input = Input::new();
        edit(&mut input, key(KeyCode::Char('A')));
        edit(&mut input, with(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "");
        edit(
            &mut input,
            with(
                KeyCode::Char('z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        );
        assert_eq!(input.text(), "A");
    }

    #[test]
    fn picture_cells_keep_their_colors_and_never_receive_action_underlines() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 4));
        for cell in &mut buffer.content {
            cell.set_symbol("▀");
            cell.set_fg(ratatui::style::Color::Rgb(17, 88, 143));
            cell.modifier.insert(Modifier::UNDERLINED);
        }
        let picture = Rect::new(1, 1, 5, 2);
        let overlay = Rect::new(4, 1, 2, 1);
        clear_picture_underlines(&mut buffer, &[picture], overlay);
        let target = Target {
            x: 1,
            y: 1,
            width: 5,
            act: Act::Media("demo".into()),
            zone: Zone::BodyLeft,
            tab: true,
        };
        highlight(
            &mut buffer,
            &target,
            &fixtures::demo_state(Detail::Rich).theme,
            &[picture],
        );
        for y in 1..3 {
            for x in 1..6 {
                let cell = &buffer[(x, y)];
                assert_eq!(cell.fg, ratatui::style::Color::Rgb(17, 88, 143));
                assert_eq!(cell.symbol(), "▀");
                assert_eq!(
                    cell.modifier.contains(Modifier::UNDERLINED),
                    overlay.contains(Position::new(x, y))
                );
            }
        }
        assert!(buffer[(0, 0)].modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn rich_preview_slots_are_excluded_from_disclosure_in_the_actual_frame() {
        let mut state = fixtures::demo_state(Detail::Rich);
        state.preview_ratio = Some(0.5);
        state.graphics = true;
        state.reveal_shortcuts = true;
        let ui = test_ui();
        let buffer = frame(&mut state, &ui, 100, 40);
        let body = state.view_sized(96, true, Some(31));
        let mut checked = 0;
        for slot in body.previews.iter().filter(|slot| slot.y + slot.rows <= 31) {
            for y in slot.y..slot.y + slot.rows {
                for x in slot.x..slot.x + slot.cols {
                    assert!(
                        !buffer[((x + 2) as u16, (y + 6) as u16)]
                            .modifier
                            .contains(Modifier::UNDERLINED),
                        "picture at {x},{y}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 15);
    }
    #[test]
    fn welcome_keeps_only_breadcrumbs_and_all_six_choices_at_every_size() {
        for (width, height) in [
            (24, 8),
            (32, 10),
            (40, 15),
            (40, 20),
            (60, 20),
            (80, 24),
            (100, 32),
            (180, 45),
        ] {
            let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
            state.status.clear();
            let ui = test_ui();
            let buffer = frame(&mut state, &ui, width, height);
            let text = buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("SoCLI > login"), "{width}x{height}: {text}");
            assert!(!text.contains("Notifications"), "{text}");
            let targets = &ui.hits.borrow().targets;
            assert_eq!(
                targets
                    .iter()
                    .filter(|target| matches!(target.act, Act::SignIn(_) | Act::EnterDemo))
                    .count(),
                6,
                "{width}x{height}"
            );
            assert!(
                targets
                    .iter()
                    .all(|target| matches!(target.act, Act::SignIn(_) | Act::EnterDemo)),
                "{targets:?}"
            );
            let demo = targets
                .iter()
                .find(|target| target.act == Act::EnterDemo)
                .unwrap();
            assert!(targets
                .iter()
                .filter(|target| matches!(target.act, Act::SignIn(_)))
                .all(|target| target.y > demo.y));
        }
    }

    #[test]
    fn welcome_hover_and_keyboard_select_the_same_network_without_signing_in() {
        let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        frame(&mut state, &ui, 100, 32);
        let target = target_where(&ui, |target| target.act == Act::SignIn(Network::Bluesky));
        mouse(
            &mut state,
            &mut ui,
            MouseEventKind::Moved,
            target.x.into(),
            target.y.into(),
        );
        assert_eq!(Network::ALL[state.gate_row], Network::Bluesky);
        press(&mut state, &mut ui, KeyCode::Up);
        assert_eq!(Network::ALL[state.gate_row], Network::X);
        press(&mut state, &mut ui, KeyCode::Up);
        assert_eq!(state.gate_row, onboard::CHOICES - 1);
        press(&mut state, &mut ui, KeyCode::Down);
        assert_eq!(Network::ALL[state.gate_row], Network::X);
        assert!(onboard::showing(&state));
    }
    #[test]
    fn login_results_wait_for_continue_and_can_retry_or_cancel() {
        let mut attempts = 0;
        let mut outcomes = Vec::new();
        let completed = login_with_result(
            || {
                attempts += 1;
                if attempts == 1 {
                    Err("rejected".into())
                } else {
                    Ok(())
                }
            },
            |result| {
                outcomes.push(result.map_err(str::to_string));
                Ok(true)
            },
        )
        .unwrap();
        assert!(completed);
        assert_eq!(attempts, 2);
        assert_eq!(outcomes, [Err("rejected".into()), Ok(())]);
        let mut attempts = 0;
        assert!(!login_with_result(
            || {
                attempts += 1;
                Err("rejected".into())
            },
            |_| Ok(false)
        )
        .unwrap());
        assert_eq!(attempts, 1);
        assert!(!login_with_result(
            || Err("cancelled".into()),
            |_| panic!("an explicit cancellation returns to the reader")
        )
        .unwrap());
    }

    #[cfg(feature = "native")]
    #[test]
    fn a_terminal_guard_dropped_without_disarm_restores_the_terminal() {
        static RESTORED: AtomicBool = AtomicBool::new(false);
        fn mark(enhanced: bool) {
            RESTORED.store(enhanced, Ordering::SeqCst);
        }
        RESTORED.store(false, Ordering::SeqCst);
        {
            let _guard = TerminalGuard::armed_with(true, mark);
        }
        assert!(
            RESTORED.load(Ordering::SeqCst),
            "drop without disarm must restore"
        );
        RESTORED.store(false, Ordering::SeqCst);
        {
            let mut guard = TerminalGuard::armed_with(true, mark);
            guard.disarm();
        }
        assert!(
            !RESTORED.load(Ordering::SeqCst),
            "disarm must skip the second restore"
        );
    }

    #[cfg(feature = "native")]
    #[test]
    fn a_busy_clients_lock_keeps_the_key_and_sets_loading() {
        let mutex = Mutex::new(0);
        let _held = mutex.lock().unwrap();
        match next_clients_key(&mutex, 'l') {
            ClientsKey::Waiting { key, loading } => {
                assert_eq!(key, 'l');
                assert!(loading);
            }
            ClientsKey::Ready(_, _) => panic!("a busy lock must not handle the key"),
        };
    }

    #[cfg(feature = "native")]
    #[test]
    fn a_free_clients_lock_handles_the_key() {
        let mutex = Mutex::new(0);
        match next_clients_key(&mutex, 'l') {
            ClientsKey::Ready(key, guard) => {
                assert_eq!(key, 'l');
                assert_eq!(*guard, 0);
            }
            ClientsKey::Waiting { .. } => panic!("a free lock must handle the key"),
        };
    }

    #[cfg(feature = "native")]
    #[test]
    fn a_poisoned_clients_lock_still_handles_the_key() {
        let mutex = Mutex::new(0);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap();
            panic!("poison the clients lock");
        }));
        match next_clients_key(&mutex, 'l') {
            ClientsKey::Ready(key, guard) => {
                assert_eq!(key, 'l');
                assert_eq!(*guard, 0);
            }
            ClientsKey::Waiting { .. } => panic!("poison is not a busy lock"),
        };
    }
}
