//! Header, sub-bar, rules, footer nav and key hints, shared by the reader and `--print`.

use crate::key::KeyCode;
use unicode_width::UnicodeWidthStr;

use crate::model::{FeedId, Post, Scope, Section};
use crate::net::Network;
use crate::render::{Act, Canvas, Kind, OverflowItem, StyledLine, Target};
use crate::settings;
use crate::state::{SettingsPage, State};
use crate::theme::{self, Icons, Replies};
use crate::util;

/// Which key hints the bottom row lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hints {
    Feed,
    Thread,
    /// One account's posts.
    User,
    Notifications,
    Search,
    Inspect,
    Settings,
    /// Sign in or out of one network.
    Accounts,
}

impl Hints {
    pub fn of(state: &State) -> Self {
        if state.settings {
            match state.settings_page {
                SettingsPage::Accounts => Hints::Accounts,
                SettingsPage::Main => Hints::Settings,
            }
        } else if state.thread_focus.is_some() {
            Hints::Thread
        } else if state.section == Section::Search && matches!(state.remote, Some(FeedId::User(..)))
        {
            Hints::User
        } else if state.section == Section::Notifications {
            Hints::Notifications
        } else {
            Hints::Feed
        }
    }

    /**
    The pairs for this set, each with the keys its clicks run; with a `selected` post, only actions
    its network supports, and `v view` when it has media.
    */
    fn pairs(self, selected: Option<&Post>, ascii: bool) -> Vec<Pair> {
        let caps = selected.map(|post| post.net.caps());
        let media = selected.is_some_and(|post| !post.media.is_empty());
        let supported = |key: &str| match (key, &caps) {
            ("v", _) => media,
            (_, None) => true,
            ("r", Some(caps)) => caps.post,
            ("l", Some(caps)) => caps.like,
            ("t", Some(caps)) => caps.repost,
            ("b", Some(caps)) => caps.bookmark,
            _ => true,
        };
        self.all_pairs()
            .iter()
            .filter(|(key, _)| supported(key))
            .map(|&(key, label)| match (key, &caps) {
                ("l", Some(caps)) => (key, caps.like_verb),
                _ => (key, label),
            })
            .map(|(key, label)| Pair {
                key: theme::ascii_key(key, ascii),
                label,
                codes: hint_codes(key),
            })
            .collect()
    }

    fn all_pairs(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Hints::Feed => &[
                ("j/k", "move"),
                ("⏎", "open"),
                ("v", "view"),
                ("u", "profile"),
                ("r", "reply"),
                ("l", "like"),
                ("t", "repost"),
                ("s", "share"),
                ("f", "search"),
                ("a", "network"),
                ("?", "help"),
            ],
            Hints::Thread => &[
                ("j/k", "move"),
                ("r", "reply"),
                ("l", "like"),
                ("t", "repost"),
                ("b", "bookmark"),
                ("⌫", "back"),
                ("u", "profile"),
                ("s", "share"),
                ("?", "help"),
            ],
            Hints::User => &[
                ("j/k", "move"),
                ("⏎", "open"),
                ("s", "share"),
                ("o", "open in browser"),
                ("⌫", "back"),
                ("?", "help"),
            ],
            Hints::Notifications => &[
                ("j/k", "move"),
                ("⏎", "open"),
                ("r", "reply"),
                ("l", "like"),
                ("?", "help"),
            ],
            Hints::Search => &[("⏎", "search"), ("esc", "cancel")],
            Hints::Inspect => &[("j/k", "move"), ("⏎", "expand"), ("⌫", "back")],
            Hints::Settings => &[
                ("j/k", "move"),
                ("space", "toggle"),
                ("h/l", "change"),
                ("esc", "close"),
            ],
            Hints::Accounts => &[("j/k", "move"), ("⏎", "login"), ("esc", "back")],
        }
    }
}

#[derive(Clone, Copy)]
struct Pair {
    key: &'static str,
    label: &'static str,
    /// The key a click runs, and for a pair like `j/k` the key the second letter runs.
    codes: (KeyCode, KeyCode),
}

fn hint_codes(key: &str) -> (KeyCode, KeyCode) {
    let code = |key: &str| match key {
        "⏎" => KeyCode::Enter,
        "⌫" => KeyCode::Backspace,
        "esc" => KeyCode::Esc,
        "space" => KeyCode::Char(' '),
        _ => key.chars().next().map_or(KeyCode::Null, KeyCode::Char),
    };
    match key.split_once('/') {
        Some((a, b)) => (code(a), code(b)),
        None => (code(key), code(key)),
    }
}

fn line(width: usize) -> (Canvas, usize) {
    let mut canvas = Canvas::new(width);
    let y = canvas.push(false);
    (canvas, y)
}

fn parts_width(parts: &[(Kind, &str)]) -> usize {
    parts.iter().map(|(_, text)| text.width()).sum()
}

fn finish(canvas: Canvas) -> (StyledLine, Vec<Target>) {
    let (lines, _, targets) = canvas.finish_targeted();
    let line = lines.into_iter().next().unwrap_or_else(StyledLine::blank);
    (line, targets)
}

/// `crumb` is usually `state.crumb()`; the JSON inspector extends it.
pub fn header(state: &State, crumb: &str, width: usize) -> StyledLine {
    header_targeted(state, crumb, width).0
}

/// `header` and its targets: `⌫ back`, or the account label and the bell.
pub fn header_targeted(state: &State, crumb: &str, width: usize) -> (StyledLine, Vec<Target>) {
    header_line(state, crumb, width, true)
}

/**
The header `--print` and the `SoCLI>` prompt write: nothing in the scrollback takes keys, so it
never offers `⌫ back`.
*/
pub fn printed_header(state: &State, crumb: &str, width: usize) -> StyledLine {
    header_line(state, crumb, width, false).0
}

/// [`util::truncate`] for chrome; the plain level, which draws ASCII only, ends the cut in `...`.
pub(crate) fn cut(text: &str, width: usize, ascii: bool) -> String {
    let cut = util::truncate(text, width);
    if !ascii || cut == text {
        return cut;
    }
    if width < 3 {
        return ".".repeat(width);
    }
    let kept = util::truncate(text, width - 2);
    format!("{}...", kept.strip_suffix('…').unwrap_or(&kept))
}

/// The crumb ends in `…` where it meets the account. When the account or `back` does not fit, `[...]` holds it.
fn header_line(state: &State, crumb: &str, width: usize, keys: bool) -> (StyledLine, Vec<Target>) {
    let (mut canvas, y) = line(width);
    let mut pieces = vec![(Kind::Bold, "SoCLI".to_string())];
    for part in crumb.split(" › ") {
        pieces.push((Kind::Dim, " ›".into()));
        pieces.push((Kind::Fg, format!(" {part}")));
    }
    let account = state.account_label();
    let count = state.mention_count();
    let bell = match (state.theme.ascii, count) {
        (true, 1) => "1 notification".to_string(),
        (true, n) => format!("{n} notifications"),
        (false, n) => format!("{} {n}", Icons::new(state.icons).bell),
    };
    let back = keys && !state.settings;
    let back = back && (state.thread_focus.is_some() || state.profile().is_some());
    let right: Vec<(Kind, &str)> = if back {
        vec![
            (
                Kind::Bold,
                if state.reveal_shortcuts {
                    theme::ascii_key("⌫", state.theme.ascii)
                } else {
                    ""
                },
            ),
            (Kind::Fg, if state.reveal_shortcuts { " " } else { "" }),
            (Kind::Dim, "back"),
        ]
    } else if state.mention_count() > 0 {
        vec![
            (Kind::Dim, &account),
            (Kind::Fg, "   "),
            (Kind::Pink, &bell),
        ]
    } else {
        vec![(Kind::Dim, &account)]
    };
    let right_w = parts_width(&right);
    let crumb_w: usize = pieces.iter().map(|(_, text)| text.width()).sum();
    let show_right = right_w > 0 && crumb_w + 1 + right_w <= width;
    // The account or back stays reachable when the crumb would otherwise push it off.
    let show_more = !show_right && right_w > 0 && MORE_WIDTH + 1 < width;
    let crumb_budget = if show_right {
        width.saturating_sub(1 + right_w)
    } else if show_more {
        width.saturating_sub(1 + MORE_WIDTH)
    } else {
        width
    };
    let mut x = 0;
    for (index, (kind, text)) in pieces.iter().enumerate() {
        let shown = cut(text, crumb_budget.saturating_sub(x), state.theme.ascii);
        let cut_short = shown != *text;
        let start = x;
        x = canvas.put(y, x, *kind, &shown);
        if keys && index % 2 == 0 && x > start {
            let act = Act::Crumb(index / 2);
            canvas.target(y, start + usize::from(index > 0), x, act);
        }
        if cut_short {
            break;
        }
    }
    if show_right {
        canvas.put_right(y, width - 1, &right);
        let start = width - right_w;
        if back {
            canvas.target(y, start, width, Act::Back);
        } else if keys {
            let account_width = account.width();
            if account_width > 0 {
                canvas.target(y, start, start + account_width, header_account_act(state));
            }
            if right.len() > 1 {
                let bell_start = start + account_width + 3;
                canvas.target(y, bell_start, width, Act::Section(Section::Notifications));
            }
        }
    } else if show_more {
        let mut items = Vec::new();
        if back {
            items.push(OverflowItem {
                parts: vec![(Kind::Dim, "back".into())],
                act: Box::new(Act::Back),
            });
        } else {
            items.push(OverflowItem {
                parts: vec![(Kind::Dim, account.clone())],
                act: Box::new(header_account_act(state)),
            });
            if count > 0 {
                items.push(OverflowItem {
                    parts: vec![(Kind::Pink, bell.clone())],
                    act: Box::new(Act::Section(Section::Notifications)),
                });
            }
        }
        let start = width - MORE_WIDTH;
        let end = canvas.put(y, start, Kind::Bold, MORE);
        if keys {
            canvas.target(y, start, end, Act::Overflow(items));
        }
    }
    finish(canvas)
}

/// The header account opens that account's profile, or the accounts page when the view merges.
fn header_account_act(state: &State) -> Act {
    match state.scope() {
        Scope::One(net) if state.handle_of(net).is_some() => Act::MyProfile(net),
        _ => Act::Accounts,
    }
}

/// The thread's bar: `Post`, bold while the thread has the keys, and the reply order on the right.
pub fn thread_bar(state: &State, width: usize, focused: bool) -> StyledLine {
    let (mut canvas, y) = line(width);
    let kind = if focused { Kind::Bold } else { Kind::Dim };
    canvas.put(y, 0, kind, "Post");
    canvas.put_right(
        y,
        width - 1,
        &[
            (Kind::Dim, "Replies"),
            (Kind::Fg, " "),
            (
                Kind::Bold,
                match state.replies {
                    Replies::Oldest => "oldest first",
                    Replies::Newest => "newest first",
                },
            ),
        ],
    );
    canvas.finish_line()
}

/**
The row under the header. `typing` holds the search input while it is open. A `narrow` terminal
collapses the network chips to the current network's name.
*/
pub fn subbar(state: &State, width: usize, typing: Option<&str>, narrow: bool) -> StyledLine {
    subbar_targeted(state, width, typing, narrow, false).0
}

/// `subbar` and its targets: the network chips and the numbered feed tabs.
/// A focused thread replaces that row, unless `split` is set and the thread pane draws the bar.
pub fn subbar_targeted(
    state: &State,
    width: usize,
    typing: Option<&str>,
    narrow: bool,
    split: bool,
) -> (StyledLine, Vec<Target>) {
    // The thread pane draws its own Post bar, so a split keeps the feed chips on this row.
    if state.thread_focus.is_some() && !split {
        return (thread_bar(state, width, true), Vec::new());
    }
    let (mut canvas, y) = line(width);
    match state.section {
        Section::Home => home_bar(state, &mut canvas, y, width, narrow),
        Section::Notifications => {
            canvas.put(y, 0, Kind::Bold, "[Mentions]");
        }
        Section::Search => {
            if let (None, Some(FeedId::User(_, handle))) = (typing, &state.remote) {
                let x = canvas.put(y, 0, Kind::Bold, "Posts");
                let who = format!("@{}", handle.trim_start_matches('@'));
                canvas.put(y, x + 2, Kind::Dim, &who);
                return finish(canvas);
            }
            let x = canvas.put(y, 0, Kind::Bold, "Search");
            match typing {
                Some(input) => {
                    let x = canvas.put(y, x + 1, Kind::Blue, "❯");
                    canvas.put(y, x + 1, Kind::Fg, input);
                }
                None if !state.remote_query.is_empty() => {
                    canvas.put(y, x + 2, Kind::Dim, &state.remote_query);
                }
                None => {}
            }
        }
    }
    finish(canvas)
}

/// The sub-bar while the JSON inspector shows one post.
pub fn json_subbar(handle: &str, id: &str, width: usize) -> StyledLine {
    let (mut canvas, y) = line(width);
    let x = canvas.put(y, 0, Kind::Bold, "JSON");
    canvas.put(y, x + 2, Kind::Dim, &format!("@{handle} · {id}"));
    canvas.finish_line()
}

/**
`All` and a dot chip for every network, then for a single network a dim `│` and its feed tabs.
When the row is wider than `width`, chips that are off drop from the right first, then chips that
are on shrink to their network's initial, then tabs that are not open, then the tab numbers, then
chips that are on but not bracketed. Dropped chips and tabs stay behind a `[...]` when that marker
fits; pressing it lists them. A `narrow` row has one chip instead, `All` or the single network's
name, which opens the network picker.
*/
fn home_bar(state: &State, canvas: &mut Canvas, y: usize, width: usize, narrow: bool) {
    let all_on = state.all_on();
    let single = state.scope_net();
    let mut chips: Vec<Item> = vec![if all_on {
        Item::new(vec![(Kind::Bold, "[All]".into())], Rank::Kept)
    } else {
        Item::new(vec![(Kind::Dim, "All".into())], Rank::Off)
    }
    .with_act(Act::Scope(Scope::All))];
    for (net, on, available) in state.chips() {
        let dot = (
            if available { net.accent() } else { Kind::Dim },
            if on { "●" } else { "○" }.to_string(),
        );
        let label = format!(" {}", net.title());
        let act = Act::Scope(Scope::One(net));
        chips.push(
            if single == Some(net) {
                Item::new(
                    vec![
                        (Kind::Bold, "[".into()),
                        dot,
                        (Kind::Bold, label),
                        (Kind::Bold, "]".into()),
                    ],
                    Rank::Kept,
                )
            } else if on {
                let kind = if all_on { Kind::Fg } else { Kind::Bold };
                let initial: String = net.title().chars().take(1).collect();
                Item::new(vec![dot.clone(), (kind, label)], Rank::On)
                    .with_short(vec![dot, (kind, format!(" {initial}"))])
            } else {
                Item::new(vec![dot, (Kind::Dim, label)], Rank::Off)
            }
            .with_act(act),
        );
    }
    let tab = |i: usize, n: usize, label: &str, open: bool| {
        let digit = if n <= 9 {
            format!("{n} ")
        } else {
            String::new()
        };
        if open {
            Item::new(
                vec![
                    (Kind::Bold, "[".into()),
                    (Kind::Dim, digit),
                    (Kind::Bold, format!("{label}]")),
                ],
                Rank::Kept,
            )
        } else {
            Item::new(vec![(Kind::Dim, format!("{digit}{label}"))], Rank::Off)
        }
        .with_act(Act::Tab(i))
    };
    let tabs_for = |digits: bool| -> Vec<Item> {
        match single {
            Some(net) => {
                let open = state.tab_of(net);
                state
                    .scope_tabs()
                    .into_iter()
                    .enumerate()
                    .map(|(i, (kind, label))| {
                        tab(
                            i,
                            if digits { i + 1 } else { 10 },
                            label,
                            Some(kind) == open,
                        )
                    })
                    .collect()
            }
            None => {
                let open = state.shown_merged_tab();
                state
                    .merged_tabs()
                    .into_iter()
                    .enumerate()
                    .map(|(i, merged)| {
                        tab(
                            i,
                            if digits { i + 1 } else { 10 },
                            merged.title(),
                            merged == open,
                        )
                    })
                    .collect()
            }
        }
    };
    if narrow {
        let name = single.map_or("All", |net| net.title());
        chips = vec![Item::new(vec![(Kind::Bold, name.into())], Rank::Kept)
            .with_act(Act::Key(KeyCode::Char('A')))];
    }
    let mut tabs = tabs_for(state.reveal_shortcuts);
    for (i, chip) in chips.iter_mut().enumerate() {
        chip.order = i;
    }
    for (i, tab) in tabs.iter_mut().enumerate() {
        tab.order = i;
    }
    let mut hidden_chips = Vec::new();
    let mut hidden_tabs = Vec::new();
    let mut numbered = state.reveal_shortcuts;
    let mut short = false;
    while bar_width(&chips, &tabs, false, false) > width
        && drop_bar_item(
            &mut chips,
            &mut tabs,
            &mut hidden_chips,
            &mut hidden_tabs,
            &mut numbered,
            &mut short,
            &tabs_for,
        )
    {}
    let (mut chip_more, mut tab_more) = (false, false);
    if !hidden_chips.is_empty() || !hidden_tabs.is_empty() {
        let saved = (
            chips.clone(),
            tabs.clone(),
            hidden_chips.clone(),
            hidden_tabs.clone(),
            numbered,
            short,
        );
        chip_more = !hidden_chips.is_empty();
        tab_more = !hidden_tabs.is_empty();
        while bar_width(&chips, &tabs, chip_more, tab_more) > width {
            if !drop_bar_item(
                &mut chips,
                &mut tabs,
                &mut hidden_chips,
                &mut hidden_tabs,
                &mut numbered,
                &mut short,
                &tabs_for,
            ) {
                // The marker would hide a control that fit without it.
                (chips, tabs, hidden_chips, hidden_tabs, _, _) = saved;
                chip_more = false;
                tab_more = false;
                break;
            }
            chip_more = !hidden_chips.is_empty();
            tab_more = !hidden_tabs.is_empty();
        }
    }
    let mut x = put_items(canvas, y, 0, &chips);
    if chip_more {
        if !chips.is_empty() {
            x += ITEM_GAP;
        }
        put_more(canvas, y, x, &hidden_chips);
    }
    if !tabs.is_empty() || tab_more {
        let mut right = row_width(&tabs);
        if tab_more {
            right += if tabs.is_empty() {
                MORE_WIDTH
            } else {
                ITEM_GAP + MORE_WIDTH
            };
        }
        let start = width.saturating_sub(right);
        canvas.put(y, start.saturating_sub(3), Kind::Line, "│");
        x = put_items(canvas, y, start, &tabs);
        if tab_more {
            if !tabs.is_empty() {
                x += ITEM_GAP;
            }
            put_more(canvas, y, x, &hidden_tabs);
        }
    }
}

fn row_width(items: &[Item]) -> usize {
    items.iter().map(Item::width).sum::<usize>() + ITEM_GAP * items.len().saturating_sub(1)
}

/// Chips, the `[...]` markers and the `│` before the tabs.
fn bar_width(chips: &[Item], tabs: &[Item], chip_more: bool, tab_more: bool) -> usize {
    let mut left = row_width(chips);
    if chip_more {
        left += if chips.is_empty() {
            MORE_WIDTH
        } else {
            ITEM_GAP + MORE_WIDTH
        };
    }
    let mut right = row_width(tabs);
    if tab_more {
        right += if tabs.is_empty() {
            MORE_WIDTH
        } else {
            ITEM_GAP + MORE_WIDTH
        };
    }
    let sep = if tabs.is_empty() && !tab_more { 0 } else { 5 };
    left + sep + right
}

/// Drops the next chip or tab the narrow-row order gives up. `false` when nothing is left to drop.
fn drop_bar_item(
    chips: &mut Vec<Item>,
    tabs: &mut Vec<Item>,
    hidden_chips: &mut Vec<Item>,
    hidden_tabs: &mut Vec<Item>,
    numbered: &mut bool,
    short: &mut bool,
    tabs_for: &impl Fn(bool) -> Vec<Item>,
) -> bool {
    let droppable = |items: &[Item], rank: Rank| items.iter().rposition(|item| item.rank == rank);
    if let Some(i) = droppable(chips, Rank::Off) {
        hidden_chips.push(chips.remove(i));
        return true;
    }
    if !*short && chips.iter().any(|item| item.rank == Rank::On) {
        *short = true;
        chips.iter_mut().for_each(Item::shorten);
        return true;
    }
    if let Some(i) = droppable(tabs, Rank::Off) {
        hidden_tabs.push(tabs.remove(i));
        return true;
    }
    if *numbered {
        *numbered = false;
        let previous = std::mem::replace(tabs, tabs_for(false));
        // Rebuilding drops the digits and keeps only the open tab, as the row did before the marker.
        tabs.retain(|item| item.rank == Rank::Kept);
        for (i, tab) in tabs.iter_mut().enumerate() {
            tab.order = i;
        }
        hidden_tabs.extend(previous.into_iter().filter(|item| item.rank == Rank::Off));
        return true;
    }
    if let Some(i) = droppable(chips, Rank::On) {
        hidden_chips.push(chips.remove(i));
        return true;
    }
    false
}

/// How readily a chip or tab gives way on a narrow row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rank {
    Off,
    On,
    Kept,
}

#[derive(Clone)]
struct Item {
    parts: Vec<(Kind, String)>,
    /// The label before a short form, so a hidden chip still names the network.
    menu: Vec<(Kind, String)>,
    short: Option<Vec<(Kind, String)>>,
    rank: Rank,
    act: Option<Act>,
    order: usize,
}

impl Item {
    fn new(parts: Vec<(Kind, String)>, rank: Rank) -> Self {
        Item {
            menu: parts.clone(),
            parts,
            short: None,
            rank,
            act: None,
            order: 0,
        }
    }

    fn with_act(mut self, act: Act) -> Self {
        self.act = Some(act);
        self
    }

    fn with_short(mut self, short: Vec<(Kind, String)>) -> Self {
        self.short = Some(short);
        self
    }

    fn shorten(&mut self) {
        if let Some(short) = self.short.take() {
            self.parts = short;
        }
    }

    fn width(&self) -> usize {
        self.parts.iter().map(|(_, text)| text.width()).sum()
    }
}

const MORE: &str = "[...]";
const MORE_WIDTH: usize = 5;
const ITEM_GAP: usize = 2;

fn put_items(canvas: &mut Canvas, y: usize, mut x: usize, items: &[Item]) -> usize {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            x += ITEM_GAP;
        }
        let start = x;
        for (kind, text) in &item.parts {
            x = canvas.put(y, x, *kind, text);
        }
        if let Some(act) = &item.act {
            canvas.target(y, start, x, act.clone());
        }
    }
    x
}

/// `[...]` for `hidden`, in their original order. The marker is a tab stop.
fn put_more(canvas: &mut Canvas, y: usize, x: usize, hidden: &[Item]) -> usize {
    let mut hidden = hidden.to_vec();
    hidden.sort_by_key(|item| item.order);
    let items = hidden
        .into_iter()
        .filter_map(|item| {
            item.act.map(|act| OverflowItem {
                parts: item.menu,
                act: Box::new(act),
            })
        })
        .collect();
    let end = canvas.put(y, x, Kind::Bold, MORE);
    canvas.target(y, x, end, Act::Overflow(items));
    end
}

/// `[...]` for hidden key hints, in the order they had on the row.
fn put_hint_more(canvas: &mut Canvas, y: usize, x: usize, hidden: &[Pair]) -> usize {
    let items = hidden
        .iter()
        .map(|pair| OverflowItem {
            parts: vec![
                (Kind::Bold, pair.key.to_string()),
                (Kind::Fg, " ".into()),
                (Kind::Dim, pair.label.to_string()),
            ],
            act: Box::new(Act::Key(pair.codes.0)),
        })
        .collect();
    let end = canvas.put(y, x, Kind::Bold, MORE);
    canvas.target(y, x, end, Act::Overflow(items));
    end
}

pub fn rule(width: usize) -> StyledLine {
    StyledLine::text(Kind::Line, &"─".repeat(width))
}

pub fn nav(state: &State, width: usize) -> StyledLine {
    nav_targeted(state, width).0
}

/// `nav` and its targets: the sections and `[n] new post`.
pub fn nav_targeted(state: &State, width: usize) -> (StyledLine, Vec<Target>) {
    let (mut canvas, y) = line(width);
    let mut x = canvas.put(y, 0, Kind::Bold, "SoCLI›") + 1;
    let count = state.mention_count();
    let current = state.profile().is_none().then_some(state.section);
    for (i, section) in [Section::Home, Section::Notifications, Section::Search]
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            x += 2;
        }
        let start = x;
        x = if Some(section) == current {
            canvas.put(y, x, Kind::Bold, &format!("[{}]", section.title()))
        } else {
            canvas.put(y, x, Kind::Dim, section.title())
        };
        if section == Section::Notifications && count > 0 {
            x = canvas.put(y, x + 1, Kind::Pink, &count.to_string());
        }
        canvas.target(y, start, x, Act::Section(section));
    }
    let right = [
        (Kind::Dim, if state.reveal_shortcuts { "[n] " } else { "" }),
        (Kind::Fg, "new post"),
    ];
    let post_w = parts_width(&right);
    if x + 1 + post_w <= width {
        canvas.put_right(y, width - 1, &right);
        canvas.target(y, width - post_w, width, Act::Compose);
    } else if x + 1 + MORE_WIDTH <= width {
        let start = width - MORE_WIDTH;
        let end = canvas.put(y, start, Kind::Bold, MORE);
        let parts = right
            .iter()
            .filter(|(_, text)| !text.is_empty())
            .map(|(kind, text)| (*kind, (*text).to_string()))
            .collect();
        canvas.target(
            y,
            start,
            end,
            Act::Overflow(vec![OverflowItem {
                parts,
                act: Box::new(Act::Compose),
            }]),
        );
    }
    finish(canvas)
}

/**
Key hints on the left, `status` right-aligned in at most a third of the row and cut with `…`;
hints drop from the end to make room, keeping the first two and `? help` last. `selected` is the
selected post, so actions its network lacks are not offered. `ascii` spells the keys as the plain
level draws them.
*/
pub fn hints(
    set: Hints,
    selected: Option<&Post>,
    status: &str,
    width: usize,
    ascii: bool,
) -> StyledLine {
    hints_targeted(set, selected, status, width, ascii).0
}

/// `hints` and their targets: each pair runs its key, for the mouse only.
pub fn hints_targeted(
    set: Hints,
    selected: Option<&Post>,
    status: &str,
    width: usize,
    ascii: bool,
) -> (StyledLine, Vec<Target>) {
    let mut pairs = set.pairs(selected, ascii);
    let pinned = pairs.last().filter(|pair| pair.key == "?").copied();
    if pinned.is_some() {
        pairs.pop();
    }
    let pair_width = |pair: &Pair| pair.key.width() + 1 + pair.label.width();
    let used = |shown: &[Pair], more: bool| -> usize {
        let n = shown.len() + usize::from(more);
        shown.iter().map(pair_width).sum::<usize>()
            + usize::from(more) * MORE_WIDTH
            + ITEM_GAP * n.saturating_sub(1)
    };
    let cap = width / 3;
    let reserve = if status.is_empty() {
        0
    } else {
        status.width().min(cap) + 2
    };
    let kept = pairs.len().min(2) + usize::from(pinned.is_some());
    let mut shown = pairs;
    shown.extend(pinned);
    let mut hidden = Vec::new();
    let take = |shown: &mut Vec<Pair>, hidden: &mut Vec<Pair>| -> bool {
        if shown.len() <= kept {
            return false;
        }
        let last = if pinned.is_some() {
            shown.len() - 2
        } else {
            shown.len() - 1
        };
        hidden.insert(0, shown.remove(last));
        true
    };
    while shown.len() > kept
        && used(&shown, false) + reserve > width
        && take(&mut shown, &mut hidden)
    {}
    let mut draw_more = false;
    if !hidden.is_empty() {
        let saved_shown = shown.clone();
        let saved_hidden = hidden.clone();
        while used(&shown, true) + reserve > width && take(&mut shown, &mut hidden) {}
        if used(&shown, true) + reserve <= width {
            draw_more = true;
        } else {
            shown = saved_shown;
            hidden = saved_hidden;
        }
    }
    let (mut canvas, y) = line(width);
    let mut x = 0;
    for (i, pair) in shown.iter().enumerate() {
        if i > 0 {
            x += ITEM_GAP;
        }
        if draw_more && pinned.is_some() && i + 1 == shown.len() {
            x = put_hint_more(&mut canvas, y, x, &hidden);
            x += ITEM_GAP;
        }
        let (first, second) = pair.codes;
        let start = x;
        x = match pair.key.split_once('/').filter(|_| pair.key.len() == 3) {
            Some((a, b)) => {
                let x = canvas.put(y, x, Kind::Bold, a);
                let x = canvas.put(y, x, Kind::Dim, "/");
                let end = canvas.put(y, x, Kind::Bold, b);
                canvas.mouse_target(y, start, x, Act::Key(first));
                canvas.mouse_target(y, x, end, Act::Key(second));
                end
            }
            None => canvas.put(y, x, Kind::Bold, pair.key),
        };
        let key_end = x;
        x = canvas.put(y, x + 1, Kind::Dim, pair.label);
        let from = if first == second { start } else { key_end };
        canvas.mouse_target(y, from, x, Act::Key(first));
    }
    if draw_more && pinned.is_none() {
        if !shown.is_empty() {
            x += ITEM_GAP;
        }
        x = put_hint_more(&mut canvas, y, x, &hidden);
    }
    let room = if shown.is_empty() {
        width
    } else {
        width.saturating_sub(x + 2)
    };
    let shown_status = cut(status, room.min(cap), ascii);
    if !shown_status.is_empty() {
        canvas.put_right(y, width.saturating_sub(1), &[(Kind::Dim, &shown_status)]);
    }
    finish(canvas)
}

/// The key text a footer hint draws: arrows for `j/k` and `h/l`, the key itself otherwise.
fn context_key(pair: &Pair, ascii: bool) -> &'static str {
    match (pair.key, ascii) {
        ("j/k", true) => "^ v < >",
        ("j/k", false) => "↑ ↓ ← →",
        ("h/l", true) => "< >",
        ("h/l", false) => "← →",
        (key, _) => key,
    }
}

pub fn context_hints(
    state: &State,
    set: Hints,
    selected: Option<&Post>,
    focused: Option<&Act>,
    width: usize,
) -> (StyledLine, Vec<Target>) {
    if state.temporary_status() {
        let (mut canvas, y) = line(width);
        let kind = if state.status_error.is_some() {
            Kind::Red
        } else {
            Kind::Dim
        };
        canvas.put(y, 0, kind, &cut(&state.status, width, state.theme.ascii));
        return finish(canvas);
    }
    let ascii = state.theme.ascii;
    let mut pairs = set.pairs(selected, ascii);
    if set == Hints::Accounts {
        let action = Network::ALL
            .get(state.settings_row)
            .map(|net| settings::account_action(state, *net))
            .unwrap_or("login");
        for pair in &mut pairs {
            if pair.codes.0 == KeyCode::Enter {
                pair.label = action;
            }
        }
    }
    pairs.retain(|pair| pair.key != "?");
    if selected.is_none() && matches!(set, Hints::Feed | Hints::Thread | Hints::Notifications) {
        pairs.retain(|pair| matches!(pair.key, "f" | "a"));
    }
    if let Some(act) = focused {
        let label = match act {
            Act::Author(_) | Act::MyProfile(_) => "open profile",
            Act::Accounts => "accounts",
            Act::SignIn(_) => "sign in",
            Act::EnterDemo => "open the demo",
            Act::Link { .. } | Act::ProfileLink(_) => "open link",
            Act::Reply(_) => "reply",
            Act::Repost(_) => "toggle repost",
            Act::Like(_) => selected.map_or("like", |post| post.net.caps().like_verb),
            Act::Bookmark(_) => "toggle bookmark",
            Act::Media(_) | Act::MediaAt { .. } => "view image",
            Act::Fold(_) => "show replies",
            Act::More(id) => {
                if state.opened.contains(id) {
                    "show less"
                } else {
                    "show more"
                }
            }
            Act::Compose => "new post",
            Act::Scope(_) => "switch network",
            Act::Tab(_) | Act::Section(_) | Act::Crumb(_) => "open page",
            Act::Back => "back",
            Act::Overflow(_) => "show hidden",
            _ => "open",
        };
        pairs.retain(|pair| pair.key != theme::ascii_key("⏎", ascii));
        pairs.insert(
            0,
            Pair {
                key: theme::ascii_key("⏎", ascii),
                label,
                codes: (KeyCode::Enter, KeyCode::Enter),
            },
        );
    }
    let help = "? help";
    let help_start = width.saturating_sub(help.width());
    let marker_at = help_start.saturating_sub(ITEM_GAP + MORE_WIDTH);
    let fit_before = |limit: usize| {
        let mut x = 0;
        let mut n = 0;
        for pair in &pairs {
            let used = context_key(pair, ascii).width() + 1 + pair.label.width();
            if x + used + 2 > limit {
                break;
            }
            x += used + 2;
            n += 1;
        }
        n
    };
    let mut count = fit_before(help_start);
    let mut draw_more = false;
    if count < pairs.len() && help_start >= ITEM_GAP + MORE_WIDTH {
        let with_marker = fit_before(marker_at);
        if with_marker < pairs.len() {
            count = with_marker;
            draw_more = true;
        }
    }
    let hidden: Vec<Pair> = pairs[count..].to_vec();
    let (mut canvas, y) = line(width);
    let mut x = 0;
    for pair in &pairs[..count] {
        let moving = pair.key == "j/k";
        let changing = pair.key == "h/l";
        let key = context_key(pair, ascii);
        let after_key = canvas.put(y, x, Kind::Bold, key);
        let end = canvas.put(y, after_key + 1, Kind::Dim, pair.label);
        if moving || changing {
            let keys: &[KeyCode] = if moving {
                &[KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right]
            } else {
                &[KeyCode::Left, KeyCode::Right]
            };
            for (i, key) in keys.iter().enumerate() {
                canvas.mouse_target(y, x + i * 2, x + i * 2 + 1, Act::Key(*key));
            }
            canvas.mouse_target(
                y,
                x + if moving { 8 } else { 4 },
                end,
                Act::Key(pair.codes.0),
            );
        } else {
            canvas.mouse_target(y, x, end, Act::Key(pair.codes.0));
        }
        x = end + 2;
    }
    let block = if draw_more { marker_at } else { help_start };
    let room = block.saturating_sub(x + 2).min(width / 3);
    if room > 0 && !state.status.is_empty() {
        canvas.put(y, x, Kind::Dim, &cut(&state.status, room, ascii));
    }
    if draw_more {
        put_hint_more(&mut canvas, y, marker_at, &hidden);
    }
    let after_key = canvas.put(y, help_start, Kind::Bold, "?");
    canvas.put(y, after_key + 1, Kind::Dim, "help");
    canvas.mouse_target(y, help_start, width, Act::Key(KeyCode::Char('?')));
    finish(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::net::{Clients, Network};

    #[test]
    fn a_confirmation_replaces_the_footer_hints() {
        let mut state = demo();
        state.confirm("Detail: medium");
        let (line, targets) = context_hints(&state, Hints::Feed, None, None, 80);
        let text = line.plain();
        assert_eq!(text.trim_end(), "Detail: medium");
        assert!(targets.is_empty(), "{targets:?}");
        state.fail("bluesky is not signed in. Run `socli login bluesky`");
        let (line, targets) = context_hints(&state, Hints::Settings, None, None, 40);
        assert!(line.plain().starts_with("bluesky is not signed in"));
        assert!(!line.plain().contains("help"), "{}", line.plain());
        assert!(targets.is_empty());
    }

    #[test]
    fn plain_hints_spell_the_keys() {
        let line = hints(Hints::Thread, None, "", 104, true).plain();
        assert!(line.contains("bksp back"), "{line}");
        assert!(line.is_ascii(), "{line}");
    }

    #[test]
    fn hints_drop_pairs_before_the_status_and_keep_help() {
        let full = hints(Hints::Feed, None, "", 104, false).plain();
        assert_eq!(
            full.trim_end(),
            "j/k move  ⏎ open  u profile  r reply  l like  t repost  s share  f search  a network  ? help"
        );
        let squeezed = hints(
            Hints::Feed,
            None,
            "a long status line for this post",
            70,
            false,
        )
        .plain();
        let status = util::truncate("a long status line for this post", 70 / 3);
        assert!(squeezed.ends_with(&status), "{squeezed}");
        assert!(squeezed.contains("a long status"), "{squeezed}");
        assert!(squeezed.ends_with('…'), "{squeezed}");
        assert!(squeezed.contains("? help"), "{squeezed}");
        assert!(squeezed.contains("[...]"), "{squeezed}");
        assert!(!squeezed.contains("f search"), "{squeezed}");
        assert!(squeezed.width() <= 70);
    }

    #[test]
    fn status_alone_truncates() {
        let line = hints(Hints::Search, None, &"x".repeat(80), 40, false).plain();
        assert_eq!(line.width(), 40);
        assert!(line.ends_with('…'));
    }

    #[test]
    fn hints_offer_only_what_the_network_supports() {
        let state = demo();
        let on = |net: Network, media: bool| {
            state
                .posts
                .values()
                .find(|post| post.net == net && post.media.is_empty() != media)
                .cloned()
        };
        let devnull = on(Network::Devnull, false).unwrap();
        let line = hints(Hints::Feed, Some(&devnull), "", 104, false).plain();
        assert!(
            line.contains("l star") && !line.contains("t repost"),
            "{line}"
        );
        let threads = on(Network::Threads, false).unwrap();
        let line = hints(Hints::Feed, Some(&threads), "", 104, false).plain();
        assert!(line.contains("r reply  t repost  s share"), "{line}");
        let pictured = on(Network::Bluesky, true).unwrap();
        let line = hints(Hints::Feed, Some(&pictured), "", 104, false).plain();
        assert!(
            line.starts_with("j/k move  ⏎ open  v view  u profile"),
            "{line}"
        );
        assert!(line.trim_end().ends_with("a network  ? help"), "{line}");
        let line = hints(Hints::Thread, None, "", 104, false).plain();
        assert!(
            line.trim_end()
                .ends_with("⌫ back  u profile  s share  ? help"),
            "{line}"
        );
        for set in [Hints::Thread, Hints::User, Hints::Inspect] {
            let line = hints(set, None, "", 104, false).plain();
            assert!(line.contains("⌫ back") && !line.contains("esc"), "{line}");
        }
    }

    fn words(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn demo() -> State {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.reveal_shortcuts = true;
        state
    }

    #[test]
    fn only_the_reader_header_offers_back() {
        let mut state = demo();
        state
            .apply(
                crate::command::Cmd::Thread("x:p1".into()),
                &mut Clients::empty(),
            )
            .unwrap();
        let crumb = state.crumb();
        assert!(header(&state, &crumb, 100).plain().contains("⌫ back"));
        let printed = printed_header(&state, &crumb, 100).plain();
        assert!(!printed.contains("back"), "{printed}");
    }

    #[test]
    fn a_narrow_header_cuts_the_crumb_and_drops_what_does_not_fit() {
        let state = demo();
        let crumb = "all › home › a crumb part far too long to fit";
        let (line, targets) = header_targeted(&state, crumb, 22);
        let text = line.plain();
        assert_eq!(text.width(), 22);
        assert!(text.contains('…'), "{text}");
        assert!(text.trim_end().ends_with("[...]"), "{text}");
        assert!(!text.contains(&state.account_label()), "{text}");
        let labels = overflow_labels(&targets);
        assert!(
            labels
                .iter()
                .any(|label| label.contains(&state.account_label())),
            "{labels:?}"
        );
        assert!(
            targets.iter().any(|target| matches!(
                target.act,
                Act::Overflow(ref items)
                    if items.iter().any(|item| matches!(item.act.as_ref(), Act::Section(Section::Notifications)))
            )),
            "{targets:?}"
        );
    }

    #[test]
    fn a_narrow_subbar_keeps_one_chip_that_opens_the_network_picker() {
        let mut state = demo();
        let (line, targets) = subbar_targeted(&state, 40, None, true, false);
        assert!(
            words(&line.plain()).starts_with("All │ [1 Home] 2 Discover"),
            "{}",
            line.plain()
        );
        let chip = targets
            .iter()
            .find(|target| target.act == Act::Key(KeyCode::Char('A')))
            .unwrap();
        assert_eq!((chip.x, chip.width), (0, 3));
        state.set_on(vec![Network::Bluesky]);
        assert!(subbar(&state, 40, None, true)
            .plain()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .starts_with("Bluesky │ [1 Following]"));
    }

    #[test]
    fn a_long_status_takes_a_third_of_the_row_and_keeps_the_first_hints() {
        let status = "a status that goes on and on ".repeat(8);
        for width in [24, 40, 70, 104, 180] {
            let line = hints(Hints::Feed, None, &status, width, false).plain();
            assert!(line.width() <= width, "{width}: {line}");
            assert!(line.starts_with("j/k move  ⏎ open  "), "{width}: {line}");
            assert!(line.contains("? help"), "{width}: {line}");
            let shown = line.trim_end().rsplit("  ").next().unwrap();
            if shown != "? help" {
                assert!(shown.width() <= width / 3, "{width}: {line}");
                assert!(shown.ends_with('…'), "{width}: {line}");
            }
        }
    }

    #[test]
    fn chips_follow_the_on_set() {
        let mut state = demo();
        assert_eq!(
            words(&subbar(&state, 100, None, false).plain()),
            words("[All]  ● X  ● Bluesky  ● dev/null  ● Threads  ● Mastodon  │  [1 Home]  2 Discover  3 Saved")
        );
        state.set_on(vec![Network::Bluesky]);
        assert_eq!(
            words(&subbar(&state, 100, None, false).plain()),
            words("All  ○ X  [● Bluesky]  ○ dev/null  ○ Threads  ○ Mastodon  │  [1 Following]  2 Discover  3 Bookmarks")
        );
        state.set_on(vec![Network::Bluesky, Network::X]);
        assert_eq!(
            words(&subbar(&state, 100, None, false).plain()),
            words("All  ● X  ● Bluesky  ○ dev/null  ○ Threads  ○ Mastodon  │  [1 Home]  2 Discover  3 Saved")
        );
        state.merged_tab = crate::state::MergedTab::Saved;
        assert_eq!(
            words(&subbar(&state, 100, None, false).plain()),
            words("All  ● X  ● Bluesky  ○ dev/null  ○ Threads  ○ Mastodon  │  1 Home  2 Discover  [3 Saved]")
        );
    }

    #[test]
    fn signed_out_networks_keep_a_chip_and_leave_all_bracketed() {
        let mut state = demo();
        state.nets.retain(|info| info.net != Network::Threads);
        state.set_on(vec![
            Network::X,
            Network::Bluesky,
            Network::Devnull,
            Network::Mastodon,
        ]);
        assert_eq!(
            words(&subbar(&state, 100, None, false).plain()),
            words("[All]  ● X  ● Bluesky  ● dev/null  ○ Threads  ● Mastodon  │  [1 Home]  2 Discover  3 Saved")
        );
    }

    #[test]
    fn narrow_chip_rows_drop_chips_that_are_off_first() {
        let mut state = demo();
        state.set_on(vec![Network::Threads]);
        let full = "All  ○ X  ○ Bluesky  ○ dev/null  [● Threads]  ○ Mastodon  │  [1 Your threads]";
        assert_eq!(
            words(&subbar(&state, 100, None, false).plain()),
            words(full)
        );
        assert_eq!(
            words(&subbar(&state, 40, None, false).plain()),
            words("[● Threads]  [...]  │  [1 Your threads]")
        );
        assert_eq!(
            words(&subbar(&state, 35, None, false).plain()),
            words("[● Threads]  │  [1 Your threads]")
        );
        assert_eq!(
            words(&subbar(&state, 30, None, false).plain()),
            words("[● Threads]  │  [Your threads]")
        );
    }

    #[test]
    fn narrow_merged_rows_shorten_network_names_before_dropping_tabs() {
        let state = demo();
        assert_eq!(
            words(&subbar(&state, 90, None, false).plain()),
            words("[All]  ● X  ● Bluesky  ● dev/null  ● Threads  ● Mastodon  │  [1 Home]  2 Discover  3 Saved")
        );
        assert_eq!(
            words(&subbar(&state, 70, None, false).plain()),
            words("[All]  ● X  ● B  ● d  ● T  ● M  │  [1 Home]  2 Discover  3 Saved")
        );
        let (line, targets) = subbar_targeted(&state, 50, None, false, false);
        assert_eq!(
            words(&line.plain()),
            words("[All]  ● X  ● B  ● d  ● T  ● M  │  [1 Home]  [...]")
        );
        let labels = overflow_labels(&targets);
        assert!(
            labels.iter().any(|label| label.contains("Discover")),
            "{labels:?}"
        );
        assert!(
            labels.iter().any(|label| label.contains("Saved")),
            "{labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label.contains("Bluesky")),
            "{labels:?}"
        );
    }

    #[test]
    fn header_and_nav_carry_the_socli_name() {
        let mut state = demo();
        state.icons = false;
        let header = header(&state, &state.crumb(), 104).plain();
        assert!(header.starts_with("SoCLI › all › home"), "{header}");
        assert!(header.trim_end().ends_with("Accounts   ⚑ 2"), "{header}");
        let (_, targets) = header_targeted(&state, &state.crumb(), 104);
        assert!(targets.iter().any(|target| target.act == Act::Accounts));
        assert!(targets
            .iter()
            .any(|target| target.act == Act::Section(Section::Notifications)));
        state.set_on(vec![Network::X]);
        let (_, targets) = header_targeted(&state, &state.crumb(), 104);
        assert!(targets
            .iter()
            .any(|target| target.act == Act::MyProfile(Network::X)));
        assert!(nav(&state, 104).plain().starts_with("SoCLI› [Home]"));
    }

    #[test]
    fn a_wide_row_has_no_overflow_marker() {
        let state = demo();
        let line = subbar(&state, 100, None, false).plain();
        assert!(!line.contains("[...]"), "{line}");
    }

    #[test]
    fn dropped_chips_stay_in_the_marker_and_a_tighter_row_retracts_it() {
        let mut state = demo();
        state.set_on(vec![Network::Threads]);
        let (line, targets) = subbar_targeted(&state, 40, None, false, false);
        assert!(line.plain().contains("[...]"), "{}", line.plain());
        let labels = overflow_labels(&targets);
        assert!(
            labels.iter().any(|label| label.contains("Mastodon")),
            "{labels:?}"
        );
        assert!(
            labels.iter().any(|label| label.contains("Bluesky")),
            "{labels:?}"
        );
        assert!(
            labels.iter().any(|label| label.contains("All")),
            "{labels:?}"
        );
        assert!(
            !labels.iter().any(|label| label.contains("Threads")),
            "{labels:?}"
        );
        let tight = subbar(&state, 35, None, false).plain();
        assert!(!tight.contains("[...]"), "{tight}");
        assert_eq!(words(&tight), words("[● Threads]  │  [1 Your threads]"));
    }

    #[test]
    fn a_shortened_chip_stays_on_the_row() {
        let state = demo();
        let (line, targets) = subbar_targeted(&state, 70, None, false, false);
        assert!(!line.plain().contains("[...]"), "{}", line.plain());
        assert!(overflow_labels(&targets).is_empty());
        assert!(line.plain().contains("● B"), "{}", line.plain());
    }

    #[test]
    fn new_post_moves_behind_the_marker_when_the_nav_is_short() {
        let state = demo();
        let (line, targets) = nav_targeted(&state, 44);
        let text = line.plain();
        assert!(text.contains("[...]"), "{text}");
        assert!(!text.contains("new post"), "{text}");
        assert!(text.width() <= 44, "{text}");
        let Act::Overflow(items) = &targets
            .iter()
            .find(|target| matches!(target.act, Act::Overflow(_)))
            .unwrap()
            .act
        else {
            panic!("new post marker");
        };
        assert!(matches!(items[0].act.as_ref(), Act::Compose));
    }

    #[test]
    fn hidden_footer_hints_sit_before_help() {
        let state = demo();
        let post = state.posts.values().next().cloned().unwrap();
        let (line, targets) = context_hints(&state, Hints::Feed, Some(&post), None, 40);
        let text = line.plain();
        assert!(text.contains("[...]"), "{text}");
        assert!(text.trim_end().ends_with("help"), "{text}");
        assert!(text.width() <= 40, "{text}");
        let labels = overflow_labels(&targets);
        assert!(labels.len() > 1, "{labels:?}");
        assert!(
            labels
                .iter()
                .any(|label| label.contains("search") || label.contains("network")),
            "{labels:?}"
        );
    }

    fn overflow_labels(targets: &[Target]) -> Vec<String> {
        let Some(target) = targets
            .iter()
            .find(|target| matches!(target.act, Act::Overflow(_)))
        else {
            return Vec::new();
        };
        let Act::Overflow(items) = &target.act else {
            return Vec::new();
        };
        items
            .iter()
            .map(|item| item.parts.iter().map(|(_, text)| text.as_str()).collect())
            .collect()
    }
}
