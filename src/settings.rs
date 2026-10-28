/*!
The Settings screen (`,`) and the filter panel (`F`): what they draw, and what each key changes.
Every change applies to the running reader and is saved to the config file at once, except in
`--demo`, which never writes the user's config.
*/

use unicode_width::UnicodeWidthStr;

use crate::config::{self, Enter};
use crate::net::Network;
use crate::render::{Canvas, Kind, RowSpan, StyledLine};
use crate::state::{SettingsPage, State};
use crate::theme::{self, Choice, ColorBy, Replies, Scheme, Switch};
use crate::util;

/// A Settings row the cursor can rest on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Accounts,
    Net(Network),
    ColorBy,
    DefaultNetwork,
    Replies,
    Detail,
    Layout,
    Theme,
    Colors,
    Density,
    Icons,
    Images,
    Unfurl,
    Mouse,
    Enter,
}

pub const ROWS: [Row; 19] = [
    Row::Accounts,
    Row::Net(Network::X),
    Row::Net(Network::Bluesky),
    Row::Net(Network::Devnull),
    Row::Net(Network::Threads),
    Row::Net(Network::Mastodon),
    Row::ColorBy,
    Row::DefaultNetwork,
    Row::Replies,
    Row::Detail,
    Row::Layout,
    Row::Theme,
    Row::Colors,
    Row::Density,
    Row::Icons,
    Row::Images,
    Row::Unfurl,
    Row::Mouse,
    Row::Enter,
];

const NAME: usize = 4;
const ACCOUNT: usize = 15;
const HINT: usize = 34;
const COLOR: usize = 58;
/// The `●` before a network color name. Two columns after `color`.
const MARK: usize = COLOR + 7;
const VALUE: usize = 22;
const DESC: usize = 41;

/**
`text` drawn at `col`. A following field keeps the column before it blank, so a cut `…` does not
touch `login` or `logout` (that word is also the click target). With no following field the cut
uses the last column before `width`.
*/
fn fit(text: &str, col: usize, next: Option<usize>, width: usize) -> String {
    let mut room = width.saturating_sub(col);
    if let Some(next) = next {
        room = room.min(next.saturating_sub(col).saturating_sub(1));
    }
    util::truncate(text, room)
}

/// `@handle` when `net` is signed in, the hold message when that network is paused, otherwise `not signed in`.
fn account_label(state: &State, net: Network) -> String {
    if let Some(message) = state.held(net) {
        return message.to_string();
    }
    match state
        .handle_of(net)
        .filter(|_| state.available().contains(&net))
    {
        Some(handle) => format!("@{handle}"),
        None => "not signed in".into(),
    }
}

// MARK: changes

/// The status for a network that is not signed in.
pub fn not_signed_in(net: Network) -> String {
    format!(
        "{} is not signed in. Run `socli login {}`",
        net.label(),
        net.slug()
    )
}

/// Turns `net` on or off in `on`; a network that is not signed in, or the last one on, stays.
pub fn toggle(state: &State, on: &mut Vec<Network>, net: Network) -> Result<(), String> {
    if let Some(message) = state.held(net) {
        return Err(message.into());
    }
    if !state.available().contains(&net) {
        return Err(not_signed_in(net));
    }
    if on.contains(&net) {
        if on.len() == 1 {
            return Err("keep at least one network on".into());
        }
        on.retain(|n| *n != net);
    } else {
        on.push(net);
        on.sort();
    }
    Ok(())
}

/// The status of a save that wrote the config file.
pub const SAVED: &str = "saved";

/// Writes one `socli config` key and returns the status Settings shows.
pub fn save(key: &str, value: &str) -> Result<String, String> {
    let mut saved = Ok(String::new());
    config::update(|cfg| saved = config::set(cfg, key, value))?;
    saved?;
    Ok(SAVED.into())
}

/**
[`save`] for the running reader. In `--demo` it writes nothing, since the change already applies
in memory, and the status names the change as `what`.
*/
pub fn save_for(state: &State, what: &str, key: &str, value: &str) -> Result<String, String> {
    if state.demo {
        return Ok(format!("{what} · demo, not saved"));
    }
    save(key, value)
}

/// The `[feed] networks` value for an on-set.
pub fn networks_value(on: &[Network]) -> String {
    let slugs: Vec<&str> = on.iter().map(|net| net.slug()).collect();
    slugs.join(",")
}

/// Moves the value on `row` one step in `dir`, applies it, and returns the key and value to save.
/// How many rows the open Settings page has.
pub fn page_len(state: &State) -> usize {
    match state.settings_page {
        SettingsPage::Accounts => Network::ALL.len(),
        SettingsPage::Main => ROWS.len(),
    }
}

/// `login` when `net` is signed out, `logout` when it is signed in, or the hold message when it is paused.
pub fn account_action(state: &State, net: Network) -> &'static str {
    if let Some(message) = state.held(net) {
        return message;
    }
    if state.available().contains(&net) {
        "logout"
    } else {
        "login"
    }
}

/// Whether canvas column `col` is on the login, logout, or hold word of a network row.
pub fn on_account_action(state: &State, net: Network, col: usize) -> bool {
    let word = account_action(state, net);
    let start = if state.held(net).is_some() {
        ACCOUNT
    } else {
        HINT
    };
    (start..start + word.len()).contains(&col)
}

pub fn step(state: &mut State, row: Row, dir: i32) -> Option<(String, String)> {
    Some(match row {
        Row::Accounts => return None,
        Row::Net(net) => {
            let color = theme::step_color(state.theme.net_color(net), dir);
            state.theme.set_net_color(net, color);
            (format!("{}.color", net.slug()), color.to_string())
        }
        Row::ColorBy => {
            state.color_by = state.color_by.step(dir);
            ("feed.color_by".into(), state.color_by.word().into())
        }
        Row::DefaultNetwork => {
            let mut choices: Vec<Option<Network>> = vec![None];
            choices.extend(state.available().into_iter().map(Some));
            let i = choices
                .iter()
                .position(|c| *c == state.default_network)
                .unwrap_or(0) as i32;
            let next = choices[(i + dir).rem_euclid(choices.len() as i32) as usize];
            state.default_network = next;
            let value = next.map_or("none", Network::slug);
            ("default_network".into(), value.into())
        }
        Row::Replies => {
            state.replies = state.replies.step(dir);
            ("feed.replies".into(), state.replies.word().into())
        }
        Row::Detail => {
            state.detail = state.detail.step(dir);
            state.retheme();
            ("ui.detail".into(), state.detail.word().into())
        }
        Row::Layout => {
            state.layout = state.layout.step(dir);
            ("ui.layout".into(), state.layout.word().into())
        }
        Row::Theme => {
            state.scheme = state.scheme.step(dir);
            state.retheme();
            ("ui.theme".into(), state.scheme.word())
        }
        Row::Colors => {
            state.color_mode = state.color_mode.step(dir);
            state.retheme();
            ("ui.colors".into(), state.color_mode.word().into())
        }
        Row::Density => {
            state.density = state.density.step(dir);
            ("ui.density".into(), state.density.word().into())
        }
        Row::Icons => {
            state.icon_mode = state.icon_mode.step(dir);
            state.icons =
                theme::resolve_icons(false, None, Some(state.icon_mode), theme::icon_env(true));
            ("ui.icons".into(), state.icon_mode.word().into())
        }
        Row::Images => {
            state.viewer.mode = state.viewer.mode.step(dir);
            ("ui.images".into(), state.viewer.mode.word().into())
        }
        Row::Unfurl => {
            state.unfurl = !state.unfurl;
            ("ui.unfurl".into(), switch_word(state.unfurl).into())
        }
        Row::Mouse => {
            state.mouse = !state.mouse;
            ("ui.mouse".into(), switch_word(state.mouse).into())
        }
        Row::Enter => {
            state.enter = state.enter.step(dir);
            ("ui.enter".into(), state.enter.word().into())
        }
    })
}

fn switch_word(on: bool) -> &'static str {
    if on {
        Switch::On.word()
    } else {
        Switch::Off.word()
    }
}

// MARK: settings screen

/// Where a Settings row sits on the body, for the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub row: Row,
    /// The body line it is drawn on.
    pub line: usize,
    /// The columns of `‹` and `›`, on rows that step a value.
    pub arrows: Option<(usize, usize)>,
}

/// The Settings body, the cursor row's span for scrolling, and where each row went.
pub fn lines(state: &State, width: usize) -> (Vec<StyledLine>, Option<RowSpan>, Vec<Hit>) {
    if state.settings_page == SettingsPage::Accounts {
        return account_lines(state, width);
    }
    let mut canvas = Canvas::new(width);
    let cursor = ROWS.get(state.settings_row).copied();
    let mut focus = None;
    let mut hits = Vec::new();
    let section = |canvas: &mut Canvas, name: &str| {
        let y = canvas.push(false);
        canvas.put(y, 2, Kind::Bold, &fit(name, 2, None, width));
    };

    section(&mut canvas, "Networks");
    {
        let row = Row::Accounts;
        let here = cursor == Some(row);
        let y = canvas.push(here);
        hits.push(Hit {
            row,
            line: y,
            arrows: None,
        });
        if here {
            focus = Some(y);
            canvas.put(y, 0, Kind::Blue, "▍");
        }
        canvas.put(y, 2, Kind::Fg, "Accounts");
        canvas.put(
            y,
            DESC,
            Kind::Dim,
            &fit("login and logout", DESC, None, width),
        );
    }
    for net in Network::ALL {
        let row = Row::Net(net);
        let here = cursor == Some(row);
        let y = canvas.push(here);
        hits.push(Hit {
            row,
            line: y,
            arrows: None,
        });
        if here {
            focus = Some(y);
            canvas.put(y, 0, Kind::Blue, "▍");
        }
        let on = state.on.contains(&net);
        canvas.put(y, 2, net.accent(), if on { "●" } else { "○" });
        canvas.put(
            y,
            NAME,
            if on { Kind::Fg } else { Kind::Dim },
            &fit(net.title(), NAME, Some(ACCOUNT), width),
        );
        paint_session(&mut canvas, state, net, y, here, width);
        if COLOR < width {
            canvas.put(y, COLOR, Kind::Dim, &fit("color", COLOR, Some(MARK), width));
            if MARK < width {
                let x = canvas.put(y, MARK, net.accent(), "●");
                let name_col = x + 1;
                if name_col < width {
                    canvas.put(
                        y,
                        name_col,
                        if here { Kind::Bold } else { Kind::Fg },
                        &fit(state.theme.net_color(net), name_col, None, width),
                    );
                }
            }
        }
    }

    let mut option = |canvas: &mut Canvas, row: Row, label: &str, value: &str, desc: &str| {
        let here = cursor == Some(row);
        let y = canvas.push(here);
        if here {
            focus = Some(y);
            canvas.put(y, 0, Kind::Blue, "▍");
        }
        canvas.put(y, 2, Kind::Fg, &fit(label, 2, Some(VALUE), width));
        // `‹` and `›` take a column each, with a blank column on both sides of the value.
        let value_room = DESC
            .saturating_sub(VALUE + 4)
            .min(width.saturating_sub(VALUE + 2));
        let shown = util::truncate(value, value_room);
        let x = canvas.put(y, VALUE, Kind::Dim, "‹");
        let x = canvas.put(y, x + 1, if here { Kind::Bold } else { Kind::Fg }, &shown);
        canvas.put(y, x + 1, Kind::Dim, "›");
        canvas.put(y, DESC, Kind::Dim, &fit(desc, DESC, None, width));
        hits.push(Hit {
            row,
            line: y,
            arrows: Some((VALUE, x + 1)),
        });
    };

    canvas.push(false);
    section(&mut canvas, "Feed");
    let color_by = match state.color_by {
        ColorBy::Auto => "network colors when the feed mixes networks",
        ColorBy::Network => "every post in its network's color",
        ColorBy::Author => "every author in their own color",
    };
    option(
        &mut canvas,
        Row::ColorBy,
        "Color posts by",
        state.color_by.word(),
        color_by,
    );
    option(
        &mut canvas,
        Row::DefaultNetwork,
        "Default network",
        state.default_network.map_or("none", Network::label),
        "used by `socli user` and the composer",
    );
    let replies = match state.replies {
        Replies::Oldest => "oldest first",
        Replies::Newest => "newest first",
    };
    option(
        &mut canvas,
        Row::Replies,
        "Replies order",
        replies,
        "thread replies, oldest or newest on top",
    );

    canvas.push(false);
    section(&mut canvas, "Appearance");
    let appearance: [(Row, &str, &str, &str); 10] = [
        (
            Row::Detail,
            "Detail",
            state.detail.word(),
            "how much art the reader draws",
        ),
        (
            Row::Layout,
            "Layout",
            state.layout.word(),
            "side by side from 124 columns",
        ),
        (
            Row::Theme,
            "Theme",
            &state.scheme.word(),
            match state.scheme {
                Scheme::Auto => "follows your terminal background",
                scheme => scheme.describe(),
            },
        ),
        (
            Row::Colors,
            "Colors",
            state.color_mode.word(),
            "mono is black and white only",
        ),
        (
            Row::Density,
            "Density",
            state.density.word(),
            "rows between posts in medium and rich",
        ),
        (
            Row::Icons,
            "Icons",
            state.icon_mode.word(),
            "Nerd Font glyphs when the font has them",
        ),
        (
            Row::Images,
            "Images",
            state.viewer.mode.word(),
            "auto, kitty, iterm, sixel, blocks or off",
        ),
        (
            Row::Unfurl,
            "Fetch link pages",
            switch_word(state.unfurl),
            "get a preview image when the network sends none",
        ),
        (
            Row::Mouse,
            "Mouse",
            switch_word(state.mouse),
            "click, scroll and hover; shift-drag selects text",
        ),
        (
            Row::Enter,
            "Enter in composer",
            state.enter.word(),
            match state.enter {
                Enter::Post => "enter posts, shift-enter or ctrl-j adds a line",
                Enter::Newline => "enter adds a line, ctrl-s posts",
            },
        ),
    ];
    for (row, label, value, desc) in appearance {
        option(&mut canvas, row, label, value, desc);
    }
    let y = canvas.push(false);
    canvas.put(y, 2, Kind::Fg, &fit("Font and size", 2, Some(VALUE), width));
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    canvas.put(
        y,
        VALUE,
        Kind::Dim,
        &fit(font_hint(&program), VALUE, None, width),
    );
    for text in [
        "recommended: GeistMono Nerd Font",
        "brew install --cask font-geist-mono-nerd-font",
    ] {
        let y = canvas.push(false);
        canvas.put(y, VALUE, Kind::Dim, &fit(text, VALUE, None, width));
    }
    let y = canvas.push(false);
    canvas.put(y, 2, Kind::Fg, &fit("Palette", 2, Some(VALUE), width));
    canvas.put(
        y,
        VALUE,
        Kind::Dim,
        &fit("socli config set colors.<name> #rrggbb", VALUE, None, width),
    );

    let span = focus.map(|y| RowSpan {
        start: y,
        end: y + 1,
    });
    (canvas.finish(), span, hits)
}

/// The handle, or the hold message when the account column would cut it. `login` and `logout` stay at `HINT`.
fn paint_session(
    canvas: &mut Canvas,
    state: &State,
    net: Network,
    y: usize,
    here: bool,
    width: usize,
) {
    if let Some(message) = state.held(net) {
        canvas.put(
            y,
            ACCOUNT,
            if here { Kind::Bold } else { Kind::Fg },
            &fit(message, ACCOUNT, Some(COLOR), width),
        );
        return;
    }
    canvas.put(
        y,
        ACCOUNT,
        Kind::Dim,
        &fit(&account_label(state, net), ACCOUNT, Some(HINT), width),
    );
    canvas.put(
        y,
        HINT,
        if here { Kind::Bold } else { Kind::Fg },
        account_action(state, net),
    );
}

/// The accounts page: one row per network, `login` or `logout`.
fn account_lines(state: &State, width: usize) -> (Vec<StyledLine>, Option<RowSpan>, Vec<Hit>) {
    let mut canvas = Canvas::new(width);
    let mut focus = None;
    let mut hits = Vec::new();
    let y = canvas.push(false);
    canvas.put(y, 2, Kind::Bold, &fit("Accounts", 2, None, width));
    let y = canvas.push(false);
    canvas.put(
        y,
        2,
        Kind::Dim,
        &fit(
            "sign in or out. the feed switches stay on the settings page",
            2,
            None,
            width,
        ),
    );
    canvas.push(false);
    for (index, net) in Network::ALL.into_iter().enumerate() {
        let row = Row::Net(net);
        let here = state.settings_row == index;
        let y = canvas.push(here);
        hits.push(Hit {
            row,
            line: y,
            arrows: None,
        });
        if here {
            focus = Some(y);
            canvas.put(y, 0, Kind::Blue, "▍");
        }
        let signed = state.available().contains(&net);
        canvas.put(y, 2, net.accent(), if signed { "●" } else { "○" });
        canvas.put(
            y,
            NAME,
            if signed { Kind::Fg } else { Kind::Dim },
            &fit(net.title(), NAME, Some(ACCOUNT), width),
        );
        paint_session(&mut canvas, state, net, y, here, width);
    }
    let span = focus.map(|y| RowSpan {
        start: y,
        end: y + 1,
    });
    (canvas.finish(), span, hits)
}

/// Where the terminal keeps its font and size, by `TERM_PROGRAM`.
pub fn font_hint(program: &str) -> &'static str {
    match program {
        "vscode" => "Settings › terminal.integrated.fontFamily and fontSize",
        "ghostty" => "~/.config/ghostty/config: font-family, font-size",
        "iTerm.app" => "Settings › Profiles › Text",
        "Apple_Terminal" => "Settings › Profiles › Text › Font",
        "WezTerm" => "wezterm.lua: config.font, config.font_size",
        _ => "your terminal's settings",
    }
}

/// The row above the rule on Settings: a check and `saved`, or the latest status.
pub fn status_line(status: &str, check: &str, width: usize) -> StyledLine {
    let mut canvas = Canvas::new(width);
    let y = canvas.push(false);
    if status == SAVED {
        let x = canvas.put(y, 2, Kind::Green, check);
        canvas.put(y, x + 1, Kind::Dim, status);
    } else if !status.is_empty() {
        canvas.put(y, 2, Kind::Dim, status);
    }
    canvas.finish_line()
}

// MARK: filter panel

/// Outer width of the filter panel.
pub const PANEL_WIDTH: usize = 42;
pub const PANEL_COL: usize = 2;

/// Outer width of the filter panel on a canvas `width` columns wide.
pub fn panel_width(width: usize) -> usize {
    PANEL_WIDTH.min(width.saturating_sub(PANEL_COL))
}

/**
The filter panel's rows, each a full canvas row: it sits at column 2 from the rule row down, and
the rule carries on to its right on the first row.
*/
pub fn panel(state: &State, on: &[Network], cursor: usize, width: usize) -> Vec<StyledLine> {
    let outer = panel_width(width);
    let inner = outer.saturating_sub(2);
    let left = PANEL_COL;
    let right = (PANEL_COL + outer).saturating_sub(1);
    let mut canvas = Canvas::new(width);

    let y = canvas.push(false);
    let x = canvas.put(y, left, Kind::Line, "╭─");
    let x = canvas.put(
        y,
        x,
        Kind::Bold,
        &util::truncate(" Networks ", right.saturating_sub(x)),
    );
    canvas.put(y, x, Kind::Line, &"─".repeat(right.saturating_sub(x)));
    canvas.put(y, right, Kind::Line, "╮");
    canvas.put(
        y,
        right + 1,
        Kind::Line,
        &"─".repeat(width.saturating_sub(right + 1)),
    );

    let side = |canvas: &mut Canvas, y: usize| {
        canvas.put(y, left, Kind::Line, "│");
        canvas.put(y, right, Kind::Line, "│");
    };
    let available = state.available();
    for (i, net) in Network::ALL.into_iter().enumerate() {
        let y = canvas.push(false);
        side(&mut canvas, y);
        let here = i == cursor;
        if here {
            canvas.highlight(y, left + 1, right);
            canvas.put(y, left + 1, Kind::Blue, "▍");
        }
        let lit = on.contains(&net);
        let signed = available.contains(&net);
        canvas.put(y, left + 3, net.accent(), if lit { "●" } else { "○" });
        let label = if lit && signed { Kind::Fg } else { Kind::Dim };
        canvas.put(
            y,
            left + 5,
            label,
            &util::truncate(net.title(), right.saturating_sub(left + 5)),
        );
        canvas.put(
            y,
            left + 16,
            Kind::Dim,
            &util::truncate(&account_label(state, net), inner.saturating_sub(15)),
        );
    }
    let y = canvas.push(false);
    side(&mut canvas, y);
    let y = canvas.push(false);
    side(&mut canvas, y);
    let mut x = left + 2;
    let ascii = state.theme.ascii;
    // `enter` is four columns wider than `⏎`, so the plain level drops `a all` to keep the row inside.
    for (i, (key, word)) in [
        ("space", "toggle"),
        ("a", "all"),
        (theme::ascii_key("⏎", ascii), "save"),
        ("esc", "close"),
    ]
    .into_iter()
    .filter(|(key, _)| !(ascii && *key == "a"))
    .enumerate()
    {
        let start = if i > 0 { x + 2 } else { x };
        let key_width = if state.reveal_shortcuts {
            key.width() + 1
        } else {
            0
        };
        if start + key_width + word.width() >= right {
            break;
        }
        x = start;
        if state.reveal_shortcuts {
            x = canvas.put(y, x, Kind::Bold, key);
            x += 1;
        }
        x = canvas.put(y, x, Kind::Dim, word);
    }
    let y = canvas.push(false);
    canvas.put(y, left, Kind::Line, &format!("╰{}╯", "─".repeat(inner)));
    canvas.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::media::ImageMode;
    use crate::net::Clients;
    use crate::theme::{ColorMode, Density, IconMode, Scheme};
    use unicode_width::UnicodeWidthStr;

    /// Columns of `line` before display column `col`.
    fn head(line: &str, col: usize) -> String {
        let mut out = String::new();
        let mut used = 0;
        for (grapheme, w) in util::graphemes(line) {
            if used + w > col {
                break;
            }
            out.push_str(grapheme);
            used += w;
        }
        out
    }

    fn demo() -> State {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.icons = false;
        state
    }

    #[test]
    fn panel_matches_the_mockup_columns() {
        let mut state = demo();
        state.reveal_shortcuts = true;
        state.nets.retain(|info| info.net != Network::Threads);
        let on = vec![Network::X, Network::Bluesky];
        let rows: Vec<String> = panel(&state, &on, 2, 100)
            .iter()
            .map(StyledLine::plain)
            .collect();
        assert!(rows[0].starts_with("  ╭─ Networks ─"), "{}", rows[0]);
        assert_eq!(rows[0].chars().nth(43), Some('╮'));
        assert_eq!(rows[0].chars().count(), 100);
        assert_eq!(rows[1], "  │  ● X          @you                     │");
        assert_eq!(rows[3], "  │▍ ○ dev/null   @you                     │");
        assert!(
            rows[4].contains("○ Threads    not signed in"),
            "{}",
            rows[4]
        );
        assert_eq!(rows[7], "  │ space toggle  a all  ⏎ save  esc close │");
        assert_eq!(rows.len(), 9);
    }

    #[test]
    fn toggling_keeps_one_network_and_skips_signed_out_ones() {
        let mut state = demo();
        state.nets.retain(|info| info.net != Network::Threads);
        let mut on = vec![Network::X];
        assert_eq!(
            toggle(&state, &mut on, Network::X).unwrap_err(),
            "keep at least one network on"
        );
        assert!(toggle(&state, &mut on, Network::Threads)
            .unwrap_err()
            .contains("socli login threads"));
        toggle(&state, &mut on, Network::Bluesky).unwrap();
        assert_eq!(on, [Network::X, Network::Bluesky]);
        assert_eq!(networks_value(&on), "x,bluesky");
    }

    #[test]
    fn settings_rows_sit_on_the_mockup_columns() {
        let mut state = demo();
        state.settings_row = 2;
        let (lines, focus, _) = lines(&state, 100);
        let text: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert_eq!(text[0], "  Networks");
        assert!(text[1].contains("Accounts"));
        assert!(text[1].contains("login and logout"));
        let bluesky = &text[3];
        assert!(
            bluesky.starts_with("▍ ● Bluesky    @you.bsky.social"),
            "{bluesky}"
        );
        assert_eq!(
            bluesky.chars().skip(COLOR).collect::<String>(),
            "color  ● sky"
        );
        assert_eq!(focus, Some(RowSpan { start: 3, end: 4 }));
        let color_by = text.iter().find(|l| l.contains("Color posts by")).unwrap();
        assert_eq!(color_by.chars().position(|c| c == '‹'), Some(VALUE));
        assert_eq!(
            color_by.chars().skip(DESC).collect::<String>(),
            "network colors when the feed mixes networks"
        );
        assert!(text
            .iter()
            .any(|l| l.ends_with("brew install --cask font-geist-mono-nerd-font")));
    }

    #[test]
    fn font_hints_follow_the_terminal() {
        assert_eq!(
            font_hint("ghostty"),
            "~/.config/ghostty/config: font-family, font-size"
        );
        assert_eq!(font_hint("kitty"), "your terminal's settings");
    }

    #[test]
    fn steps_cycle_and_name_their_keys() {
        let mut state = demo();
        let (key, value) = step(&mut state, Row::Net(Network::Bluesky), 1).unwrap();
        assert_eq!((key.as_str(), value.as_str()), ("bluesky.color", "teal"));
        let (key, value) = step(&mut state, Row::Density, 1).unwrap();
        assert_eq!((key.as_str(), value.as_str()), ("ui.density", "roomy"));
        assert_eq!(state.density, Density::Roomy);
        let (_, value) = step(&mut state, Row::DefaultNetwork, 1).unwrap();
        assert_eq!(value, "x");
        let (key, value) = step(&mut state, Row::Theme, 1).unwrap();
        assert_eq!((key.as_str(), value.as_str()), ("ui.theme", "native"));
        assert_eq!(state.scheme, Scheme::Native);
        state.scheme = Scheme::Auto;
        let (_, value) = step(&mut state, Row::Theme, -1).unwrap();
        assert_eq!(value, "zenburn");
        state.scheme = Scheme::Black;
        let (_, value) = step(&mut state, Row::Colors, 1).unwrap();
        assert_eq!(value, "mono");
        assert!(state.theme.mono);
        assert_eq!(state.color_mode, ColorMode::Mono);
        let (_, value) = step(&mut state, Row::Icons, -1).unwrap();
        assert_eq!(value, IconMode::Unicode.word());
        let (_, value) = step(&mut state, Row::Images, 1).unwrap();
        assert_eq!(value, ImageMode::Kitty.word());
        let (key, value) = step(&mut state, Row::Enter, 1).unwrap();
        assert_eq!((key.as_str(), value.as_str()), ("ui.enter", "newline"));
        assert_eq!(state.enter, Enter::Newline);
        let (lines, _, _) = lines(&state, 100);
        let row = lines
            .iter()
            .map(StyledLine::plain)
            .find(|line| line.contains("Enter in composer"))
            .unwrap();
        assert!(row.ends_with("enter adds a line, ctrl-s posts"), "{row}");
        assert!(step(&mut state, Row::Accounts, 1).is_none());
    }

    #[test]
    fn the_accounts_page_names_login_and_logout() {
        let mut state = demo();
        state.nets.retain(|info| info.net != Network::Threads);
        let (main, _, _) = lines(&state, 100);
        let main: Vec<String> = main.iter().map(StyledLine::plain).collect();
        assert!(main.iter().any(|line| {
            line.contains("● X") && line.contains("@you") && line.contains("logout")
        }));
        assert!(main.iter().any(|line| {
            line.contains("Threads")
                && line.contains("not signed in")
                && line.contains("login")
                && !line.contains("socli login")
        }));
        state.open_accounts();
        assert_eq!(state.settings_page, SettingsPage::Accounts);
        assert_eq!(state.crumb(), "settings › accounts");
        let (lines, focus, hits) = lines(&state, 100);
        let text: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert!(
            text.iter().any(|line| line.contains("● X")
                && line.contains("@you")
                && line.contains("logout")),
            "{text:?}"
        );
        assert!(
            text.iter().any(|line| line.contains("Threads")
                && line.contains("not signed in")
                && line.contains("login")),
            "{text:?}"
        );
        assert_eq!(hits.len(), Network::ALL.len());
        assert_eq!(focus, Some(RowSpan { start: 3, end: 4 }));
        assert_eq!(account_action(&state, Network::X), "logout");
        assert_eq!(account_action(&state, Network::Threads), "login");
    }

    #[test]
    fn live_threads_shows_the_hold_instead_of_login() {
        let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
        state.icons = false;
        let (main, _, _) = lines(&state, 100);
        let main: Vec<String> = main.iter().map(StyledLine::plain).collect();
        let row = main.iter().find(|line| line.contains("Threads")).unwrap();
        assert!(row.contains("Waiting for Meta approval"), "{row}");
        assert!(!row.contains("login"), "{row}");
        state.open_accounts();
        let (page, _, _) = lines(&state, 100);
        let page: Vec<String> = page.iter().map(StyledLine::plain).collect();
        let row = page.iter().find(|line| line.contains("Threads")).unwrap();
        assert!(row.contains("Waiting for Meta approval"), "{row}");
        assert!(!row.contains("login"), "{row}");
        let panel_rows: Vec<String> = panel(&state, &[], 3, 100)
            .iter()
            .map(StyledLine::plain)
            .collect();
        let filter = panel_rows
            .iter()
            .find(|line| line.contains("Threads"))
            .unwrap();
        assert!(filter.contains("Waiting for Meta approval"), "{filter}");
        assert_eq!(
            toggle(&state, &mut vec![Network::X], Network::Threads).unwrap_err(),
            "Waiting for Meta approval"
        );
        assert_eq!(
            account_action(&state, Network::Threads),
            "Waiting for Meta approval"
        );
    }

    #[test]
    fn a_long_mastodon_handle_stops_before_logout() {
        let mut state = demo();
        let info = state
            .nets
            .iter_mut()
            .find(|info| info.net == Network::Mastodon)
            .unwrap();
        info.handle = Some("massimodeluisa@mastodon.social".into());
        for width in [120, 69] {
            let (lines, _, _) = lines(&state, width);
            let row = lines
                .iter()
                .map(StyledLine::plain)
                .find(|line| line.contains("Mastodon"))
                .unwrap();
            let at = row.find("logout").unwrap();
            assert_eq!(row[..at].width(), HINT, "{row}");
            assert_eq!(&row[at..at + "logout".len()], "logout");
            let before = head(&row, HINT);
            assert!(before.ends_with(' '), "{row}");
            assert!(before.contains('…'), "{row}");
            assert!(before.contains("@massimodeluisa"), "{row}");
            assert!(!before.contains("mastodon.social"), "{row}");
            assert!(!row.contains("logoutsocial"), "{row}");
            assert!(on_account_action(&state, Network::Mastodon, HINT));
            assert!(!on_account_action(&state, Network::Mastodon, HINT - 1));
        }
        state.open_accounts();
        let (lines, _, _) = lines(&state, 80);
        let row = lines
            .iter()
            .map(StyledLine::plain)
            .find(|line| line.contains("Mastodon"))
            .unwrap();
        let at = row.find("logout").unwrap();
        assert_eq!(row[..at].width(), HINT, "{row}");
        assert!(head(&row, HINT).contains('…'), "{row}");
        assert!(!row.contains("logoutsocial"), "{row}");
    }

    #[test]
    fn settings_text_is_cut_at_the_right_edge() {
        // 69 leaves 28 columns for a description that starts at column 41.
        let (lines, _, _) = lines(&demo(), 69);
        let text: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert!(text.iter().all(|line| line.width() <= 69));
        let detail = text.iter().find(|line| line.contains("Detail")).unwrap();
        assert!(detail.contains("how much art the reader dra…"), "{detail}");
        assert!(!detail.contains("draws"), "{detail}");
        let x = text.iter().find(|line| line.contains("● X")).unwrap();
        assert!(!x.contains("default"), "{x}");
        assert!(x.contains('…'), "{x}");
    }

    #[test]
    fn a_long_theme_name_stops_before_its_description() {
        let mut state = demo();
        state.scheme = Scheme::parse("catppuccin-macchiato").unwrap();
        let (lines, _, _) = lines(&state, 100);
        let row = lines
            .iter()
            .map(StyledLine::plain)
            .find(|line| line.contains("Theme"))
            .unwrap();
        assert_eq!(row.chars().skip(DESC).collect::<String>(), "dark", "{row}");
        assert!(row.contains('…'), "{row}");
        assert!(!row.contains("catppuccin-macchiato"), "{row}");
    }
}
