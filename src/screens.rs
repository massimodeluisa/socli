//! Screens outside the feed: login, profile, notices, ask, and the key overlay.

use std::cell::Cell;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use unicode_width::UnicodeWidthStr;

use crate::hyperlink::Region;
use crate::model::{Post, Scope, Section};
use crate::net::{FeedKind, NetError, Network};
use crate::render::{self, Canvas, Kind, RowSpan, Slot, StyledLine, Target, View};
use crate::state::State;
use crate::theme::{self, author_color, Choice, Detail, Icons, Scheme};
use crate::util;

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const TAGLINE: &str = "X, Bluesky, dev/null, Threads and Mastodon in your terminal";

/// So_ at column 2, with `side` rows starting two columns after the mark.
pub(crate) fn masthead(canvas: &mut Canvas, side: &[&[(Kind, &str)]]) {
    let text_x = 2 + theme::so_width() + 2;
    let rows = theme::so_mark().len().max(side.len());
    for i in 0..rows {
        let y = canvas.push(false);
        if let Some(row) = theme::so_mark().get(i) {
            put_so(canvas, y, 2, row);
        }
        let Some(parts) = side.get(i) else {
            continue;
        };
        let mut x = text_x;
        for (kind, text) in *parts {
            x = canvas.put(y, x, *kind, text);
        }
    }
}

/// Draws one So_ row. The name is bold and the cursor pill is blue.
pub(crate) fn put_so(canvas: &mut Canvas, y: usize, x: usize, row: &theme::SoRow) {
    let Some(at) = row.pill else {
        canvas.put(y, x, Kind::Bold, row.glyphs);
        return;
    };
    let name: String = row.glyphs.chars().take(at).collect();
    let pill: String = row.glyphs.chars().skip(at).collect();
    let next = canvas.put(y, x, Kind::Bold, &name);
    canvas.put(y, next, Kind::Blue, &pill);
}

// MARK: login

static TO_STDERR: AtomicBool = AtomicBool::new(false);

/// Sends the sign-in screens to stderr, so `--json` and agent runs keep stdout for data.
pub fn to_stderr(on: bool) {
    TO_STDERR.store(on, Ordering::Relaxed);
}

/// Prints styled lines on stdout, or on stderr after `to_stderr(true)`, in color on a terminal.
pub fn print(lines: Vec<StyledLine>) -> Result<(), String> {
    #[cfg(not(feature = "native"))]
    {
        let _ = lines;
        return Ok(());
    }
    #[cfg(feature = "native")]
    {
        print_linked(lines, &[])
    }
}

fn print_linked(lines: Vec<StyledLine>, links: &[Region]) -> Result<(), String> {
    let stderr = TO_STDERR.load(Ordering::Relaxed);
    let tty = if stderr {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    };
    let color = tty && !crate::agent::active();
    let hyperlinks = crate::hyperlink::enabled_from(&|key| std::env::var(key).ok(), color);
    let mut out: Box<dyn Write> = if stderr {
        Box::new(std::io::stderr().lock())
    } else {
        Box::new(std::io::stdout().lock())
    };
    for (row, line) in lines.iter().enumerate() {
        let regions = links
            .iter()
            .filter(|link| usize::from(link.y) == row)
            .collect::<Vec<_>>();
        let painted = if hyperlinks {
            paint_linked_line(line, &regions, color)
        } else {
            render::paint(line, color)
        };
        util::written(writeln!(out, "{painted}"))?;
    }
    Ok(())
}

fn paint_linked_line(line: &StyledLine, links: &[&Region], color: bool) -> String {
    if links.is_empty() {
        return render::paint(line, color);
    }
    let mut result = String::new();
    let mut column = 0;
    for span in &line.spans {
        let mut chunk = String::new();
        let mut active: Option<&str> = None;
        for (glyph, width) in util::graphemes(&span.text) {
            let link = links
                .iter()
                .find(|link| {
                    column >= usize::from(link.x) && column < usize::from(link.x + link.width)
                })
                .map(|link| link.url.as_str());
            if link != active && !chunk.is_empty() {
                let painted = render::paint(&StyledLine::text(span.kind, &chunk), color);
                result.push_str(&active.map_or_else(
                    || painted.clone(),
                    |url| crate::hyperlink::wrap(url, &painted),
                ));
                chunk.clear();
            }
            active = link;
            chunk.push_str(glyph);
            column += width;
        }
        let painted = render::paint(&StyledLine::text(span.kind, &chunk), color);
        result.push_str(&active.map_or_else(
            || painted.clone(),
            |url| crate::hyperlink::wrap(url, &painted),
        ));
    }
    result
}

fn icons_from_env() -> bool {
    let tty = if TO_STDERR.load(Ordering::Relaxed) {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    };
    theme::resolve_icons(
        false,
        None,
        None,
        theme::icon_env(tty && !crate::agent::active()),
    )
}

/// One rail cell. `End` is the closing `└` on the row that finishes the sign-in.
struct RailCell {
    mark: RailMark,
    kind: Kind,
    text: String,
    link: Option<String>,
}

enum RailMark {
    Top,
    Bar,
    Step,
    End,
}

fn push_wrapped(rows: &mut Vec<RailCell>, first: RailMark, kind: Kind, text: &str, width: usize) {
    let mut first = Some(first);
    for raw in text.lines() {
        let link = crate::hyperlink::safe(raw.trim());
        for line in util::wrap_text(raw, width) {
            rows.push(RailCell {
                mark: first.take().unwrap_or(RailMark::Bar),
                kind,
                text: line,
                link: link.clone(),
            });
        }
    }
}

/// Draws a single sign-in column; the active input follows its label on the final row.
pub(crate) fn login_frame(
    title: &str,
    steps: &[(&str, &str)],
    aside: &str,
    prompt: Option<&str>,
    width: usize,
) -> (Vec<StyledLine>, usize) {
    let (lines, column, _) = login_frame_linked(title, steps, aside, prompt, width);
    (lines, column)
}

fn login_frame_linked(
    title: &str,
    steps: &[(&str, &str)],
    aside: &str,
    prompt: Option<&str>,
    width: usize,
) -> (Vec<StyledLine>, usize, Vec<Region>) {
    let width = width.max(1);
    // The wordmark is at column 2. Step text shares it, and the bracket sits one column left of that.
    let column = 2;
    let text_w = width.saturating_sub(column + 2).max(1);
    let mut canvas = Canvas::new(width);
    let y = canvas.push(false);
    canvas.put(y, 2, Kind::Bold, "SoCLI");
    canvas.put(y, 7, Kind::Dim, " > login");
    canvas.push(false);
    if width >= 36 {
        masthead(&mut canvas, &[]);
    } else {
        let y = canvas.push(false);
        canvas.put(y, 2, Kind::Bold, "So_");
    }
    canvas.push(false);
    let mut rows = Vec::new();
    push_wrapped(
        &mut rows,
        RailMark::Top,
        Kind::Bold,
        &format!("Sign in to {title}"),
        text_w,
    );
    for (label, detail) in steps {
        rows.push(RailCell {
            mark: RailMark::Bar,
            kind: Kind::Fg,
            text: String::new(),
            link: None,
        });
        push_wrapped(&mut rows, RailMark::Step, Kind::Fg, label, text_w);
        if !detail.is_empty() {
            push_wrapped(&mut rows, RailMark::Bar, Kind::Dim, detail, text_w);
        }
    }
    if !aside.is_empty() {
        rows.push(RailCell {
            mark: RailMark::Bar,
            kind: Kind::Fg,
            text: String::new(),
            link: None,
        });
        push_wrapped(&mut rows, RailMark::Bar, Kind::Dim, aside, text_w);
    }
    /* The result continues this bracket. Closing it here leaves those lines outside the rail. */
    if let Some(prompt) = prompt {
        rows.push(RailCell {
            mark: RailMark::Bar,
            kind: Kind::Fg,
            text: String::new(),
            link: None,
        });
        push_wrapped(&mut rows, RailMark::End, Kind::Bold, prompt, text_w);
    }
    for row in rows {
        let y = canvas.push(false);
        let glyph = match row.mark {
            RailMark::Top => "┌",
            RailMark::Bar => "│",
            RailMark::Step => "◇",
            RailMark::End => "└",
        };
        canvas.put(y, column.saturating_sub(2), Kind::Dim, glyph);
        let end = canvas.put(y, column, row.kind, &row.text);
        if let Some(url) = row.link {
            canvas.link(y, column, end, &url);
        }
    }
    let (lines, links) = canvas.finish_linked();
    (lines, column, links)
}

/// Prints the sign-in screen. An open `prompt` is the last line, with the cursor one space after it.
///
/// Returns the form column for a subsequent prompt.
pub fn present_login(
    title: &str,
    steps: &[(&str, &str)],
    aside: &str,
    prompt: Option<&str>,
) -> Result<usize, String> {
    #[cfg(not(feature = "native"))]
    {
        let _ = (title, steps, aside);
        note_login_rail(prompt.is_none());
        return Ok(2);
    }
    #[cfg(feature = "native")]
    {
        let width = util::terminal_width(100).clamp(20, 140);
        let (mut lines, column, links) = login_frame_linked(title, steps, aside, prompt, width);
        if prompt.is_some() {
            if let Some(last) = lines.pop() {
                print_linked(lines, &links)?;
                write_open(&last)?;
            }
        } else {
            print_linked(lines, &links)?;
        }
        // A prompt row is the end of the bracket. The result then stands on its own.
        note_login_rail(prompt.is_none());
        Ok(column)
    }
}

/// Writes one row and a trailing space, without a newline, so a prompt can continue on it.
fn write_open(line: &StyledLine) -> Result<(), String> {
    let stderr = TO_STDERR.load(Ordering::Relaxed);
    let color = (if stderr {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    }) && !crate::agent::active();
    let painted = format!("{} ", render::paint(line, color));
    if stderr {
        let mut out = std::io::stderr().lock();
        util::written(write!(out, "{painted}"))?;
        util::written(out.flush())
    } else {
        let mut out = std::io::stdout().lock();
        util::written(write!(out, "{painted}"))?;
        util::written(out.flush())
    }
}

pub fn print_signed_in(handle: &str, detail: Option<&str>) -> Result<(), String> {
    print(signed_in_lines(handle, detail, icons_from_env()))
}

/// The reader draws Continue or Retry under the signed-in rows, so those rows must not close the bracket.
pub fn expect_login_result(on: bool) {
    LOGIN_RESULT_FOLLOWS.with(|flag| flag.set(on));
}

fn login_result_follows() -> bool {
    LOGIN_RESULT_FOLLOWS.with(|flag| flag.get())
}

fn note_login_rail(open: bool) {
    LOGIN_RAIL_OPEN.with(|flag| flag.set(open));
}

fn login_rail_open() -> bool {
    LOGIN_RAIL_OPEN.with(|flag| flag.get())
}

thread_local! {
    static LOGIN_RESULT_FOLLOWS: Cell<bool> = const { Cell::new(false) };
    static LOGIN_RAIL_OPEN: Cell<bool> = const { Cell::new(false) };
}

/// More steps on the open sign-in bracket. No masthead, and the bracket stays open.
pub fn login_steps(steps: &[(&str, &str)], aside: &str) -> Result<(), String> {
    note_login_rail(true);
    #[cfg(not(feature = "native"))]
    {
        let _ = (steps, aside);
        return Ok(());
    }
    #[cfg(feature = "native")]
    {
        let width = login_width();
        let (lines, links) = follow_lines(steps, aside, width);
        print_linked(lines, &links)
    }
}

/// One dim line on the open bracket, after a blank row.
pub fn login_aside(text: &str) -> Result<(), String> {
    login_steps(&[], text)
}

/**
A vertical choice on the open bracket. Arrows move, Enter confirms, Escape cancels.

The title is a `◇` step. Each account is an `│` row, with `>` on the selected one.
*/
pub fn prompt_rail(title: &str, labels: &[String]) -> Result<usize, String> {
    if labels.is_empty() {
        return Err("no sign-in action is available".into());
    }
    #[cfg(not(feature = "native"))]
    {
        let _ = (title, labels);
        return Err(util::NEEDS_PERSON.into());
    }
    #[cfg(feature = "native")]
    {
        if !util::person_present() {
            return Err(format!(
                "{} pass --browser and --profile when more than one account is signed in.",
                util::NEEDS_PERSON
            ));
        }
        let width = login_width();
        let text_w = width.saturating_sub(4).max(1);
        let mut rows = vec![RailCell {
            mark: RailMark::Bar,
            kind: Kind::Fg,
            text: String::new(),
            link: None,
        }];
        push_wrapped(&mut rows, RailMark::Step, Kind::Fg, title, text_w);
        let (lines, _) = paint_rail(rows, width);
        print(lines)?;
        prompt_rail_menu(labels)
    }
}

pub fn login_result(result: Result<(), &str>) -> Result<bool, String> {
    let labels: &[&str] = if result.is_ok() {
        &["Continue"]
    } else {
        &["Retry", "Cancel"]
    };
    let on_rail = login_rail_open();
    let indent = if on_rail {
        let width = login_width();
        let (lines, links) = login_result_lines(result, width);
        print_linked(lines, &links)?;
        note_login_rail(false);
        "  "
    } else {
        detached_login_result(result)?;
        if util::terminal_width(100) < 32 {
            "  "
        } else {
            "     "
        }
    };
    Ok(util::prompt_choice(labels, indent)? == 0)
}

fn login_width() -> usize {
    util::terminal_width(100).clamp(20, 140)
}

fn follow_lines(
    steps: &[(&str, &str)],
    aside: &str,
    width: usize,
) -> (Vec<StyledLine>, Vec<Region>) {
    let text_w = width.saturating_sub(4).max(1);
    let mut rows = Vec::new();
    if !steps.is_empty() || !aside.is_empty() {
        rows.push(RailCell {
            mark: RailMark::Bar,
            kind: Kind::Fg,
            text: String::new(),
            link: None,
        });
    }
    for (label, detail) in steps {
        push_wrapped(&mut rows, RailMark::Step, Kind::Fg, label, text_w);
        if !detail.is_empty() {
            push_wrapped(&mut rows, RailMark::Bar, Kind::Dim, detail, text_w);
        }
        rows.push(RailCell {
            mark: RailMark::Bar,
            kind: Kind::Fg,
            text: String::new(),
            link: None,
        });
    }
    if !aside.is_empty() {
        push_wrapped(&mut rows, RailMark::Bar, Kind::Dim, aside, text_w);
    }
    paint_rail(rows, width)
}

fn login_result_lines(result: Result<(), &str>, width: usize) -> (Vec<StyledLine>, Vec<Region>) {
    let text_w = width.saturating_sub(4).max(1);
    let (kind, message) = match result {
        Ok(()) => (Kind::Green, "Sign-in completed. Press Enter to continue."),
        Err(error) => (Kind::Red, error),
    };
    let mut message = message.to_string();
    let urls = message
        .split_whitespace()
        .filter_map(crate::hyperlink::safe)
        .collect::<Vec<_>>();
    for url in urls {
        message = message.replace(&url, &format!("\n{url}\n"));
    }
    let mut rows = vec![RailCell {
        mark: RailMark::Bar,
        kind: Kind::Fg,
        text: String::new(),
        link: None,
    }];
    if result.is_err() {
        push_wrapped(
            &mut rows,
            RailMark::Step,
            Kind::Red,
            "Sign-in failed.",
            text_w,
        );
    }
    push_wrapped(&mut rows, RailMark::Bar, kind, &message, text_w);
    if result.is_err() {
        push_wrapped(
            &mut rows,
            RailMark::Bar,
            Kind::Dim,
            "Use arrows or Tab, then Enter. Escape cancels.",
            text_w,
        );
    }
    if let Some(last) = rows.last_mut() {
        last.mark = RailMark::End;
    }
    paint_rail(rows, width)
}

fn paint_rail(rows: Vec<RailCell>, width: usize) -> (Vec<StyledLine>, Vec<Region>) {
    let mut canvas = Canvas::new(width);
    for row in rows {
        let y = canvas.push(false);
        let glyph = match row.mark {
            RailMark::Top => "┌",
            RailMark::Bar => "│",
            RailMark::Step => "◇",
            RailMark::End => "└",
        };
        canvas.put(y, 0, Kind::Dim, glyph);
        let end = canvas.put(y, 2, row.kind, &row.text);
        if let Some(url) = row.link {
            canvas.link(y, 2, end, &url);
        }
    }
    canvas.finish_linked()
}

/// `✓ Signed in as @handle` on the same bracket as the steps above it.
///
/// A sign-in that already closed on its prompt keeps the result beside that row.
pub fn signed_in_lines(handle: &str, detail: Option<&str>, icons: bool) -> Vec<StyledLine> {
    if !login_rail_open() {
        return detached_signed_in(handle, detail, icons);
    }
    let close = !login_result_follows();
    let lines = signed_in_rows(handle, detail, icons, close);
    if close {
        note_login_rail(false);
    }
    lines
}

fn detached_signed_in(handle: &str, detail: Option<&str>, icons: bool) -> Vec<StyledLine> {
    let mut canvas = Canvas::new(80);
    let y = canvas.push(false);
    let x = canvas.put(y, 5, Kind::Green, Icons::new(icons).check);
    let x = canvas.put(y, x + 1, Kind::Fg, "Signed in as ");
    let x = canvas.put(y, x, Kind::Bold, &format!("@{handle}"));
    if let Some(detail) = detail {
        canvas.put(y, x + 1, Kind::Dim, &format!("· {detail}"));
    }
    canvas.push(false);
    let y = canvas.push(false);
    canvas.put(y, 2, Kind::Dim, "Next");
    for (command, what) in [
        ("socli", "opens your timeline"),
        ("socli post \"…\"", "posts right away"),
        ("socli --help", "lists every command"),
    ] {
        let y = canvas.push(false);
        canvas.put(y, 5, Kind::Fg, command);
        canvas.put(y, 20, Kind::Dim, what);
    }
    canvas.finish()
}

fn detached_login_result(result: Result<(), &str>) -> Result<(), String> {
    let width = util::terminal_width(100).clamp(20, 140).saturating_sub(5);
    let (kind, message) = match result {
        Ok(()) => (Kind::Green, "Sign-in completed. Press Enter to continue."),
        Err(error) => (Kind::Red, error),
    };
    let mut canvas = Canvas::new(width + 5);
    canvas.push(false);
    if result.is_err() {
        let y = canvas.push(false);
        canvas.put(y, 5, Kind::Red, "Sign-in failed.");
    }
    let mut message = message.to_string();
    let urls = message
        .split_whitespace()
        .filter_map(crate::hyperlink::safe)
        .collect::<Vec<_>>();
    for url in urls {
        message = message.replace(&url, &format!("\n{url}\n"));
    }
    let mut rows = Vec::new();
    push_wrapped(&mut rows, RailMark::Bar, kind, &message, width);
    if result.is_err() {
        push_wrapped(
            &mut rows,
            RailMark::Bar,
            Kind::Dim,
            "Use arrows or Tab, then Enter. Escape cancels.",
            width,
        );
    }
    for row in rows {
        let y = canvas.push(false);
        let end = canvas.put(y, 5, row.kind, &row.text);
        if let Some(url) = row.link {
            canvas.link(y, 5, end, &url);
        }
    }
    let (lines, links) = canvas.finish_linked();
    print_linked(lines, &links)
}

fn signed_in_rows(handle: &str, detail: Option<&str>, icons: bool, close: bool) -> Vec<StyledLine> {
    let width = login_width();
    let mut canvas = Canvas::new(width);
    rail_row(&mut canvas, "│");
    let y = rail_row(&mut canvas, "◇");
    canvas.put(y, 2, Kind::Fg, "Signed in");
    let y = rail_row(&mut canvas, "│");
    let check = Icons::new(icons).check;
    let mut x = canvas.put(y, 2, Kind::Green, check);
    x = canvas.put(y, x + 1, Kind::Fg, "Signed in as ");
    x = canvas.put(y, x, Kind::Bold, &format!("@{handle}"));
    if let Some(detail) = detail {
        canvas.put(y, x + 1, Kind::Dim, &format!("· {detail}"));
    }
    rail_row(&mut canvas, "│");
    let y = rail_row(&mut canvas, "◇");
    canvas.put(y, 2, Kind::Fg, "Next");
    let commands = [
        ("socli", "opens your timeline"),
        ("socli post \"…\"", "posts right away"),
        ("socli --help", "lists every command"),
    ];
    let last = commands.len() - 1;
    for (index, (command, what)) in commands.into_iter().enumerate() {
        let mark = if close && index == last { "└" } else { "│" };
        let y = rail_row(&mut canvas, mark);
        canvas.put(y, 2, Kind::Fg, command);
        canvas.put(y, 18, Kind::Dim, what);
    }
    canvas.finish()
}

fn rail_row(canvas: &mut Canvas, mark: &str) -> usize {
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Dim, mark);
    y
}

/// The account rows. `rewrite` paints over the previous list.
#[cfg(feature = "native")]
fn prompt_rail_menu(labels: &[String]) -> Result<usize, String> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal;

    let width = login_width();
    let color = login_color();
    terminal::enable_raw_mode().map_err(|err| err.to_string())?;
    let _cooked = RailCook;
    crate::watchdog::arm(crate::watchdog::Probe::None);
    let mut selected = 0usize;
    let mut rewrite = false;
    loop {
        paint_menu(labels, selected, width, color, rewrite)?;
        rewrite = true;
        let choice = loop {
            crate::watchdog::beat();
            if !event::poll(std::time::Duration::from_millis(200)).map_err(|err| err.to_string())? {
                continue;
            }
            let Event::Key(key) = event::read().map_err(|err| err.to_string())? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            let cancel = key.code == KeyCode::Esc
                || (key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL));
            if cancel {
                break Err("cancelled".to_string());
            }
            match key.code {
                KeyCode::Enter => break Ok(selected),
                KeyCode::Tab | KeyCode::Right | KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1) % labels.len();
                    break Ok(usize::MAX);
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Up | KeyCode::Char('k') => {
                    selected = (selected + labels.len() - 1) % labels.len();
                    break Ok(usize::MAX);
                }
                KeyCode::Char(ch) => {
                    if let Some(index) = labels.iter().position(|label| {
                        label
                            .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
                            .to_ascii_lowercase()
                            .starts_with(ch.to_ascii_lowercase())
                    }) {
                        selected = index;
                        break Ok(usize::MAX);
                    }
                }
                _ => {}
            }
        };
        match choice {
            Ok(usize::MAX) => {}
            Ok(index) => return Ok(index),
            Err(err) => return Err(err),
        }
    }
}

#[cfg(feature = "native")]
fn paint_menu(
    labels: &[String],
    selected: usize,
    width: usize,
    color: bool,
    rewrite: bool,
) -> Result<(), String> {
    if TO_STDERR.load(Ordering::Relaxed) {
        paint_menu_on(
            labels,
            selected,
            width,
            color,
            rewrite,
            &mut std::io::stderr().lock(),
        )
    } else {
        paint_menu_on(
            labels,
            selected,
            width,
            color,
            rewrite,
            &mut std::io::stdout().lock(),
        )
    }
}

/// `writeln` in raw mode is a line feed only, so each row ends with a carriage return.
#[cfg(feature = "native")]
fn paint_menu_on(
    labels: &[String],
    selected: usize,
    width: usize,
    color: bool,
    rewrite: bool,
    out: &mut impl Write,
) -> Result<(), String> {
    use crossterm::cursor::{MoveToColumn, MoveUp};
    use crossterm::terminal::{Clear, ClearType};

    let lines = menu_lines(labels, selected, width);
    if rewrite {
        let rows = u16::try_from(labels.len()).unwrap_or(u16::MAX);
        crossterm::execute!(out, MoveToColumn(0), MoveUp(rows)).map_err(|err| err.to_string())?;
    }
    for line in &lines {
        let painted = render::paint(line, color);
        write!(out, "{painted}").map_err(|err| err.to_string())?;
        crossterm::execute!(out, Clear(ClearType::UntilNewLine)).map_err(|err| err.to_string())?;
        write!(out, "\r\n").map_err(|err| err.to_string())?;
    }
    out.flush().map_err(|err| err.to_string())
}

#[cfg(feature = "native")]
fn login_color() -> bool {
    let stderr = TO_STDERR.load(Ordering::Relaxed);
    let tty = if stderr {
        std::io::stderr().is_terminal()
    } else {
        std::io::stdout().is_terminal()
    };
    tty && !crate::agent::active()
}

/// Puts the terminal back in cooked mode if the account menu unwinds.
#[cfg(feature = "native")]
struct RailCook;

#[cfg(feature = "native")]
impl Drop for RailCook {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        crate::watchdog::disarm();
    }
}

fn menu_lines(labels: &[String], selected: usize, width: usize) -> Vec<StyledLine> {
    let mut canvas = Canvas::new(width);
    for (index, label) in labels.iter().enumerate() {
        let y = canvas.push(false);
        canvas.put(y, 0, Kind::Dim, "│");
        let on = index == selected;
        let kind = if on { Kind::Fg } else { Kind::Dim };
        canvas.put(y, 2, kind, if on { "> " } else { "  " });
        canvas.put(y, 4, kind, label);
    }
    canvas.finish()
}

// MARK: profile and notices

pub fn profile_lines(name: &str, handle: Option<&str>, detail: &str) -> Vec<StyledLine> {
    let mut canvas = Canvas::new(80);
    let top = canvas.push(false);
    let mid = canvas.push(false);
    let bottom = canvas.push(false);
    let letter = name
        .trim_start_matches('@')
        .chars()
        .next()
        .unwrap_or('?')
        .to_ascii_uppercase();
    render::avatar(
        &mut canvas,
        top,
        2,
        author_color(handle.unwrap_or(name)),
        letter,
    );
    canvas.put(top, 9, Kind::Bold, name);
    let detail_row = match handle {
        Some(handle) => {
            canvas.put(mid, 9, Kind::Dim, &format!("@{handle}"));
            bottom
        }
        None => mid,
    };
    canvas.put(detail_row, 9, Kind::Dim, detail);
    canvas.finish()
}

pub fn notice_lines(title: &str, body: &str, width: usize) -> Vec<StyledLine> {
    let mut lines = vec![StyledLine::text(Kind::Bold, title), StyledLine::blank()];
    lines.extend(
        util::wrap_text(body, width)
            .iter()
            .map(|line| StyledLine::text(Kind::Dim, line)),
    );
    lines
}

// MARK: body

/// What sits between the chrome rows: its lines, the rows of the selected block, and the link regions.
pub struct Body {
    pub lines: Vec<StyledLine>,
    pub focus: Option<RowSpan>,
    pub links: Vec<Region>,
    pub previews: Vec<Slot>,
    /// Link pages the post view wants unfurled.
    pub unfurls: Vec<String>,
    /// Pressable runs, in the same coordinates as `links`.
    pub targets: Vec<Target>,
    /// The rows of each laid row of the post view, so a click finds the post under it.
    pub rows: Vec<RowSpan>,
}

impl Body {
    pub fn text(lines: Vec<StyledLine>) -> Self {
        Body {
            lines,
            focus: None,
            links: Vec::new(),
            previews: Vec::new(),
            unfurls: Vec::new(),
            targets: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// `lines` above the post view, its rows, links and image boxes shifted below them.
    fn above(mut lines: Vec<StyledLine>, view: View) -> Self {
        let shift = lines.len();
        let focus = view.focus().map(|span| RowSpan {
            start: span.start + shift,
            end: span.end + shift,
        });
        let links = view
            .links
            .into_iter()
            .filter_map(|region| {
                let y = u16::try_from(usize::from(region.y) + shift).ok()?;
                Some(Region { y, ..region })
            })
            .collect();
        let previews = view
            .previews
            .into_iter()
            .map(|slot| Slot {
                y: slot.y + shift,
                ..slot
            })
            .collect();
        let targets = view
            .targets
            .into_iter()
            .filter_map(|target| {
                let y = u16::try_from(usize::from(target.y) + shift).ok()?;
                Some(Target { y, ..target })
            })
            .collect();
        let rows = view
            .rows
            .iter()
            .map(|span| RowSpan {
                start: span.start + shift,
                end: span.end + shift,
            })
            .collect();
        lines.extend(view.lines);
        Body {
            lines,
            focus,
            links,
            previews,
            unfurls: view.unfurls,
            targets,
            rows,
        }
    }
}

pub fn linked_body(state: &State, width: usize, selection: bool) -> Body {
    linked_body_sized(state, width, selection, None)
}

/// The loading card sits under the list the reader is paging, and nowhere on a print or an open thread.
fn with_page_card(state: &State, width: usize, selection: bool, mut body: Body) -> Body {
    if selection && state.show_page_card && state.thread_focus.is_none() {
        body.lines
            .extend(render::loading_card(state.detail, state.theme.ascii, width));
    }
    body
}

pub fn linked_body_sized(
    state: &State,
    width: usize,
    selection: bool,
    height: Option<usize>,
) -> Body {
    let x_tab = match (state.scope(), &state.focus) {
        (Scope::One(Network::X), None) => state.tab_of(Network::X),
        _ => None,
    };
    if state.thread_focus.is_none() {
        match state.section {
            Section::Home if state.focus.is_none() && state.scope_tabs().is_empty() => {
                if let Scope::One(net) = state.scope() {
                    let body = NetError::SignInRequired(net).to_string();
                    return Body::text(notice_lines(net.title(), &body, width));
                }
            }
            Section::Search if state.remote.is_none() => {
                let lines = notice_lines(
                    "Search",
                    "Look up a public account with :user @handle, or :search words.",
                    width,
                );
                return Body::text(lines);
            }
            Section::Home if x_tab == Some(FeedKind::ForYou) && !state.x_web => {
                let mut lines = notice_lines(
                    "For you",
                    "X does not give apps the ranked For you feed. This tab is recent public search.",
                    width,
                );
                lines.push(StyledLine::blank());
                return with_page_card(
                    state,
                    width,
                    selection,
                    Body::above(lines, state.view_sized(width, selection, height)),
                );
            }
            _ => {}
        }
    }
    let Some(notice) = state.notice() else {
        return with_page_card(
            state,
            width,
            selection,
            Body::above(Vec::new(), state.view_sized(width, selection, height)),
        );
    };
    if state.current_posts().is_empty() && state.profile().is_none() {
        return Body::text(notice_lines("Nothing here", &notice, width));
    }
    let mut lines: Vec<StyledLine> = util::wrap_text(&notice, width)
        .iter()
        .map(|line| StyledLine::text(Kind::Dim, line))
        .collect();
    lines.push(StyledLine::blank());
    with_page_card(
        state,
        width,
        selection,
        Body::above(lines, state.view_sized(width, selection, height)),
    )
}

// MARK: key help

const KEYS: &[(&str, &[(&str, &str)])] = &[
    (
        "Move",
        &[
            ("j k  ↓ ↑", "move"),
            ("space PgDn", "next page"),
            ("PgUp", "previous page"),
            ("ctrl-d ctrl-u", "half page down / up"),
            ("g G  Home End", "first / last"),
            ("⏎ →", "open"),
            ("⌫ ← esc", "back"),
        ],
    ),
    (
        "Act",
        &[
            ("l", "like, again to undo"),
            ("t", "repost, again to undo"),
            ("b", "bookmark, again to remove"),
            ("r", "reply"),
            ("n", "new post"),
            ("o", "open link"),
            ("v", "view images"),
            ("u", "author's profile"),
            ("s", "share"),
            ("m", "load more"),
            ("ctrl-r cmd-r", "reload this page"),
            (".", "inspect the post as JSON"),
        ],
    ),
    (
        "Go",
        &[
            ("1 … 9", "feed tabs"),
            ("{ }", "previous / next feed tab"),
            ("[ ]", "previous / next section"),
            ("tab shift-tab", "focus next / previous control"),
            ("F6 shift-F6", "next / previous zone"),
            (": ctrl-k", "command line"),
            ("f /", "search"),
            ("ctrl-a", "ask"),
        ],
    ),
    (
        "View",
        &[
            ("d", "next detail level"),
            ("D", "pick the detail level"),
            ("T", "pick a theme"),
            ("hold ctrl/cmd", "reveal shortcuts and underline actions"),
            ("x", "show or hide the replies here"),
            ("a", "next network, then all"),
            ("A", "pick a network"),
            ("F", "filter networks"),
            ("0", "all networks"),
            ("< >", "one network at a time"),
            (",", "settings"),
            ("⌘/ctrl-click", "open names and links in the browser"),
        ],
    ),
    ("Other", &[("?", "this help"), ("q", "quit")]),
];

/**
The key table. When two columns leave room for every description, the groups split into two
columns of about equal height.
*/
pub fn key_help_lines(width: usize, ascii: bool) -> Vec<StyledLine> {
    fn rows(groups: &[(&str, &[(&str, &str)])]) -> usize {
        groups.iter().map(|(_, keys)| keys.len()).sum::<usize>() + groups.len().saturating_sub(1)
    }
    let widest = KEYS
        .iter()
        .flat_map(|(_, keys)| keys.iter())
        .map(|(_, what)| what.width())
        .max()
        .unwrap_or(0);
    let two = width >= 2 * (30 + widest);
    let split = if two {
        let half = rows(KEYS).div_ceil(2);
        (1..=KEYS.len())
            .find(|&k| rows(&KEYS[..k]) >= half)
            .unwrap_or(KEYS.len())
    } else {
        KEYS.len()
    };
    let (left, right) = KEYS.split_at(split);
    let column = if two { width / 2 } else { width };
    let mut canvas = Canvas::new(width);
    for _ in 0..rows(left).max(rows(right)) {
        canvas.push(false);
    }
    for (groups, x0) in [(left, 0), (right, column)] {
        let mut y = 0;
        for (group, (label, keys)) in groups.iter().enumerate() {
            if group > 0 {
                y += 1;
            }
            for (i, (key, what)) in keys.iter().enumerate() {
                if i == 0 {
                    canvas.put(y, x0 + 2, Kind::Bold, label);
                }
                canvas.put(y, x0 + 12, Kind::Fg, theme::ascii_key(key, ascii));
                canvas.put(y, x0 + 30, Kind::Dim, what);
                y += 1;
            }
        }
    }
    canvas.finish()
}

// MARK: share

/// One thing the share dialog can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareAction {
    CopyLink,
    CopyProfile,
    Repost,
    Unrepost,
    /// Reposted on a network whose API can't undo it; choosing it says so.
    Reposted,
    Quote,
}

/// What a key did in the share dialog.
#[derive(Debug, PartialEq, Eq)]
pub enum ShareStep {
    Stay,
    Status(String),
    Close,
    Run(ShareAction),
}

/// The share dialog for a post or a profile: its rows, its checkboxes and both cursors.
pub struct Share {
    /// The shared post's id; `None` shares a profile.
    pub post: Option<String>,
    pub net: Network,
    /// The post's permalink, or the profile URL when sharing a profile.
    pub link: String,
    pub profile_url: String,
    pub rows: Vec<ShareAction>,
    /// Signed-in networks that can post, the shared network first, each with its checkbox.
    pub targets: Vec<(Network, bool)>,
    pub row: usize,
    pub check: usize,
}

impl Share {
    /// `posters` are the signed-in networks that can post, in network order.
    pub fn post(post: &Post, posters: &[Network]) -> Self {
        let caps = post.net.caps();
        let mut rows = vec![ShareAction::CopyLink, ShareAction::CopyProfile];
        if caps.repost {
            rows.push(match (post.reposted, caps.unrepost) {
                (false, _) => ShareAction::Repost,
                (true, true) => ShareAction::Unrepost,
                (true, false) => ShareAction::Reposted,
            });
        }
        let targets = share_targets(post.net, posters);
        if !targets.is_empty() {
            rows.push(ShareAction::Quote);
        }
        Self {
            post: Some(post.id.clone()),
            net: post.net,
            link: post.permalink(),
            profile_url: post.net.profile_url(&post.author.username),
            rows,
            targets,
            row: 0,
            check: 0,
        }
    }

    pub fn profile(net: Network, handle: &str, posters: &[Network]) -> Self {
        let url = net.profile_url(handle);
        let targets = share_targets(net, posters);
        let mut rows = vec![ShareAction::CopyProfile];
        if !targets.is_empty() {
            rows.push(ShareAction::Quote);
        }
        Self {
            post: None,
            net,
            link: url.clone(),
            profile_url: url,
            rows,
            targets,
            row: 0,
            check: 0,
        }
    }

    pub fn title(&self) -> &'static str {
        if self.post.is_some() {
            " Share post "
        } else {
            " Share profile "
        }
    }

    /// The checked networks, in network order.
    pub fn checked(&self) -> Vec<Network> {
        Network::ALL
            .into_iter()
            .filter(|net| self.targets.contains(&(*net, true)))
            .collect()
    }

    /// Checkboxes show only when there is more than one network to choose from.
    pub fn boxes(&self) -> bool {
        self.targets.len() > 1 && self.rows.contains(&ShareAction::Quote)
    }

    /**
    `Quote on` when the shared post's own network is checked and quotes natively, `Share to`
    otherwise; with a single target the label names it.
    */
    pub fn quote_label(&self) -> String {
        let native = self.post.is_some()
            && self.net.caps().quote
            && self.targets.contains(&(self.net, true));
        let verb = if native { "Quote on" } else { "Share to" };
        match self.targets.as_slice() {
            [(net, _)] => format!("{verb} {}", net.title()),
            _ => verb.into(),
        }
    }

    /// A row's label, its value, and the key letter that runs it.
    pub fn row_text(&self, action: ShareAction) -> (String, String, char) {
        let on = self.net.title();
        match action {
            ShareAction::CopyLink => ("Copy link".into(), util::bare_url(&self.link).into(), 's'),
            ShareAction::CopyProfile => (
                "Copy profile link".into(),
                util::bare_url(&self.profile_url).into(),
                if self.post.is_some() { 'p' } else { 's' },
            ),
            ShareAction::Repost => (format!("Repost on {on}"), String::new(), 't'),
            ShareAction::Unrepost => (format!("Undo repost on {on}"), String::new(), 't'),
            ShareAction::Reposted => (format!("Reposted on {on}"), String::new(), 't'),
            ShareAction::Quote => (self.quote_label(), String::new(), 'q'),
        }
    }

    /// Turns checkbox `index` on or off; the last one on stays on.
    pub fn toggle(&mut self, index: usize) -> Result<(), String> {
        let on = self.targets.iter().filter(|(_, on)| *on).count();
        let Some((_, checked)) = self.targets.get_mut(index) else {
            return Ok(());
        };
        if *checked && on == 1 {
            return Err("keep at least one network on".into());
        }
        *checked = !*checked;
        Ok(())
    }

    pub fn key(&mut self, code: crate::key::KeyCode) -> ShareStep {
        use crate::key::KeyCode;
        let focused = self.rows.get(self.row).copied();
        let run = |wanted: ShareAction, rows: &[ShareAction]| {
            if rows.contains(&wanted) {
                ShareStep::Run(wanted)
            } else {
                ShareStep::Stay
            }
        };
        match code {
            KeyCode::Esc | KeyCode::Backspace => ShareStep::Close,
            KeyCode::Tab
                if focused == Some(ShareAction::Quote)
                    && self.boxes()
                    && self.check + 1 < self.targets.len() =>
            {
                self.check += 1;
                ShareStep::Stay
            }
            KeyCode::BackTab
                if focused == Some(ShareAction::Quote) && self.boxes() && self.check > 0 =>
            {
                self.check -= 1;
                ShareStep::Stay
            }
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                self.row = (self.row + 1).min(self.rows.len().saturating_sub(1));
                ShareStep::Stay
            }
            KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                self.row = self.row.saturating_sub(1);
                ShareStep::Stay
            }
            KeyCode::Enter => focused.map_or(ShareStep::Stay, ShareStep::Run),
            KeyCode::Char('s') if self.post.is_some() => run(ShareAction::CopyLink, &self.rows),
            KeyCode::Char('s') => run(ShareAction::CopyProfile, &self.rows),
            KeyCode::Char('p') if self.post.is_some() => run(ShareAction::CopyProfile, &self.rows),
            KeyCode::Char('t') => self
                .rows
                .iter()
                .copied()
                .find(|action| {
                    matches!(
                        action,
                        ShareAction::Repost | ShareAction::Unrepost | ShareAction::Reposted
                    )
                })
                .map_or(ShareStep::Stay, ShareStep::Run),
            KeyCode::Char('q') => run(ShareAction::Quote, &self.rows),
            KeyCode::Left | KeyCode::Char('h')
                if focused == Some(ShareAction::Quote) && self.boxes() =>
            {
                self.check = self.check.saturating_sub(1);
                ShareStep::Stay
            }
            KeyCode::Right | KeyCode::Char('l')
                if focused == Some(ShareAction::Quote) && self.boxes() =>
            {
                self.check = (self.check + 1).min(self.targets.len() - 1);
                ShareStep::Stay
            }
            KeyCode::Char(' ') if focused == Some(ShareAction::Quote) && self.boxes() => {
                match self.toggle(self.check) {
                    Ok(()) => ShareStep::Stay,
                    Err(err) => ShareStep::Status(err),
                }
            }
            _ => ShareStep::Stay,
        }
    }
}

/// Networks that can post, `net` first and checked; if `net` can't post, the first is checked.
fn share_targets(net: Network, posters: &[Network]) -> Vec<(Network, bool)> {
    let mut targets: Vec<(Network, bool)> = posters
        .iter()
        .filter(|one| **one == net)
        .chain(posters.iter().filter(|one| **one != net))
        .map(|one| (*one, *one == net))
        .collect();
    if let Some(first) = targets.first_mut() {
        first.1 = true;
    }
    targets
}

/// Outer width of the share dialog on a terminal `full` columns wide, never wider than its content width.
pub fn share_width(full: usize) -> usize {
    64.min(full.saturating_sub(8))
        .max(24)
        .min(render::inner_width(full))
}

/// Where the share dialog's rows and checkboxes sit, in its own coordinates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShareHits {
    /// The dialog row of each line an action takes: (line, action index).
    pub rows: Vec<(usize, usize)>,
    /// Each checkbox: its line, its columns and its index in `targets`.
    pub checks: Vec<(usize, std::ops::Range<usize>, usize)>,
}

/**
The share dialog as rows of its own width: a titled rounded box, one row per action with the
value in Dim and the key letter on the right, then the key hint. The regions link each value to
its full URL. Quote checkboxes that do not fit before the key letter continue on the next row.
The hits say where the rows and checkboxes went, for the mouse.
*/
pub fn share_lines(
    share: &Share,
    full: usize,
    ascii: bool,
    reveal: bool,
) -> (Vec<StyledLine>, Vec<Region>, ShareHits) {
    let mut hits = ShareHits::default();
    let width = share_width(full);
    let right = width - 1;
    let label_col = 3;
    let value_col = 22;
    let key_col = width - 3;
    let mut canvas = Canvas::new(width);
    let side = |canvas: &mut Canvas, y: usize| {
        canvas.put(y, 0, Kind::Line, "│");
        canvas.put(y, right, Kind::Line, "│");
    };

    let y = canvas.push(false);
    let x = canvas.put(y, 0, Kind::Line, "╭─");
    let x = canvas.put(y, x, Kind::Bold, share.title());
    canvas.put(y, x, Kind::Line, &"─".repeat(right.saturating_sub(x)));
    canvas.put(y, right, Kind::Line, "╮");
    let y = canvas.push(false);
    side(&mut canvas, y);

    for (i, action) in share.rows.iter().enumerate() {
        let first = canvas.push(false);
        let mut y = first;
        hits.rows.push((y, i));
        side(&mut canvas, y);
        let focused = i == share.row;
        if focused {
            canvas.highlight(y, 1, right);
            canvas.put(y, 1, Kind::Blue, "▍");
        }
        let (label, value, key) = share.row_text(*action);
        let dim = *action == ShareAction::Reposted;
        let end = canvas.put(y, label_col, if dim { Kind::Dim } else { Kind::Fg }, &label);
        let room = key_col.saturating_sub(value_col.max(end + 2) + 2);
        if *action == ShareAction::Quote && share.boxes() {
            let start = value_col.max(end + 2);
            let mut x = start;
            for (j, (net, on)) in share.targets.iter().enumerate() {
                if j > 0 {
                    if x + 2 + 2 + net.title().width() + 2 > key_col && x > start {
                        y = canvas.push(false);
                        hits.rows.push((y, i));
                        side(&mut canvas, y);
                        if focused {
                            canvas.highlight(y, 1, right);
                            canvas.put(y, 1, Kind::Blue, "▍");
                        }
                        x = start;
                    } else {
                        x += 2;
                    }
                }
                let here = focused && j == share.check;
                let (mark, word) = if *on {
                    (Kind::Blue, if here { Kind::Bold } else { Kind::Fg })
                } else {
                    (Kind::Dim, if here { Kind::Bold } else { Kind::Dim })
                };
                let start = x;
                x = canvas.put(y, x, mark, if *on { "☑" } else { "☐" });
                x = canvas.put(y, x + 1, word, net.title());
                hits.checks.push((y, start..x, j));
            }
        } else if !value.is_empty() {
            let start = value_col.max(end + 2);
            let end = canvas.put(y, start, Kind::Dim, &util::truncate(&value, room));
            let url = match action {
                ShareAction::CopyLink => &share.link,
                _ => &share.profile_url,
            };
            canvas.link(y, start, end, url);
        }
        if reveal {
            canvas.put(first, key_col, Kind::Dim, &key.to_string());
        }
    }

    let y = canvas.push(false);
    side(&mut canvas, y);
    let y = canvas.push(false);
    side(&mut canvas, y);
    let enter = theme::ascii_key("⏎", ascii);
    let hint = if !reveal {
        if share.boxes() {
            "choose  toggle  close".to_string()
        } else {
            "choose  close".to_string()
        }
    } else if share.boxes() {
        format!("{enter} choose  space toggle  esc close")
    } else {
        format!("{enter} choose  esc close")
    };
    canvas.put(y, 3, Kind::Dim, &hint);
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Line, &format!("╰{}╯", "─".repeat(width - 2)));
    let (lines, regions) = canvas.finish_linked();
    (lines, regions, hits)
}

// MARK: pickers

/// One choice in a picker.
pub struct PickRow {
    /// The digit that picks this row, drawn dim at the right edge.
    pub key: char,
    /// The radio mark's color; the mark is `●` on the current row and `○` elsewhere.
    pub mark: Kind,
    pub label: String,
    pub value: String,
    /// The row holds the current value.
    pub current: bool,
    /// False for a network that is not signed in; its label is dim.
    pub enabled: bool,
}

/// The network picker's rows: All, then every network with its handle or `not signed in`.
pub fn network_rows(state: &State) -> Vec<PickRow> {
    let available = state.available();
    let several = available.len() > 1;
    let titles: Vec<&str> = available.iter().map(|net| net.title()).collect();
    let mut rows = vec![PickRow {
        key: '0',
        mark: Kind::Fg,
        label: "All networks".into(),
        value: titles.join(", "),
        current: state.all_on(),
        enabled: true,
    }];
    for (net, key) in Network::ALL.into_iter().zip('1'..) {
        let signed_in = available.contains(&net);
        rows.push(PickRow {
            key,
            mark: if signed_in { net.accent() } else { Kind::Dim },
            label: net.title().into(),
            value: match state.handle_of(net) {
                Some(handle) if signed_in => format!("@{handle}"),
                _ => "not signed in".into(),
            },
            current: several && state.scope() == Scope::One(net),
            enabled: signed_in,
        });
    }
    rows
}

/// The detail picker's rows, in the order `d` steps through them; `rich` says what draws its pictures here.
pub fn detail_rows(state: &State) -> Vec<PickRow> {
    let rich = state.pictures.map_or_else(
        || "Images off · ui.images".to_string(),
        |pictures| format!("Images · {pictures}"),
    );
    let values = [
        "text only, ASCII, no color".to_string(),
        "a network line, counts in words".into(),
        "rails, cards and icon stats".into(),
        rich,
    ];
    Detail::ALL
        .iter()
        .zip(values)
        .zip('1'..)
        .map(|((level, value), key)| PickRow {
            key,
            mark: Kind::Blue,
            label: level.word().into(),
            value,
            current: *level == state.detail,
            enabled: true,
        })
        .collect()
}

/// The theme picker's rows: every theme `socli config themes` lists, in that order.
pub fn theme_rows(state: &State) -> Vec<PickRow> {
    Scheme::list()
        .into_iter()
        .map(|scheme| PickRow {
            key: ' ',
            mark: Kind::Blue,
            label: scheme.word(),
            value: scheme.describe().into(),
            current: scheme == state.scheme.base(),
            enabled: true,
        })
        .collect()
}

/** The first row a picker of `count` rows shows in `height` terminal rows, and how many it shows. */
pub fn pick_window(count: usize, cursor: usize, height: usize) -> (usize, usize) {
    let visible = count.min(height.saturating_sub(8).max(3));
    let first = cursor.saturating_sub(visible / 2).min(count - visible);
    (first, visible)
}

/**
A picker as rows of the share dialog's width: a titled rounded box, one row per choice with a
radio mark, the label, the value in Dim and the picking digit on the right, then the key hint.
Rows that do not fit in `height` terminal rows scroll in a window around the cursor.
*/
pub fn pick_lines(
    title: &str,
    rows: &[PickRow],
    cursor: usize,
    full: usize,
    height: usize,
    ascii: bool,
) -> Vec<StyledLine> {
    let width = share_width(full);
    let right = width - 1;
    let key_col = width - 3;
    let mut canvas = Canvas::new(width);
    let side = |canvas: &mut Canvas, y: usize| {
        canvas.put(y, 0, Kind::Line, "│");
        canvas.put(y, right, Kind::Line, "│");
    };

    let y = canvas.push(false);
    let x = canvas.put(y, 0, Kind::Line, "╭─");
    let x = canvas.put(y, x, Kind::Bold, title);
    canvas.put(y, x, Kind::Line, &"─".repeat(right.saturating_sub(x)));
    canvas.put(y, right, Kind::Line, "╮");
    let y = canvas.push(false);
    side(&mut canvas, y);

    let (first, visible) = pick_window(rows.len(), cursor, height);
    for (i, row) in rows.iter().enumerate().skip(first).take(visible) {
        let y = canvas.push(false);
        side(&mut canvas, y);
        if i == cursor {
            canvas.highlight(y, 1, right);
            canvas.put(y, 1, Kind::Blue, "▍");
        }
        canvas.put(y, 3, row.mark, if row.current { "●" } else { "○" });
        let label = if row.current {
            Kind::Bold
        } else if row.enabled {
            Kind::Fg
        } else {
            Kind::Dim
        };
        let end = canvas.put(
            y,
            5,
            label,
            &util::truncate(&row.label, key_col.saturating_sub(6)),
        );
        let start = 22.max(end + 2);
        let room = key_col.saturating_sub(start + 2);
        canvas.put(y, start, Kind::Dim, &util::truncate(&row.value, room));
        canvas.put(y, key_col, Kind::Dim, &row.key.to_string());
    }

    let y = canvas.push(false);
    side(&mut canvas, y);
    let y = canvas.push(false);
    side(&mut canvas, y);
    let enter = theme::ascii_key("⏎", ascii);
    let keys = format!("{enter} choose  esc close");
    let hint = if visible < rows.len() {
        format!("{keys}  {}/{}", cursor + 1, rows.len())
    } else {
        keys
    };
    canvas.put(
        y,
        3,
        Kind::Dim,
        &util::truncate(&hint, width.saturating_sub(5)),
    );
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Line, &format!("╰{}╯", "─".repeat(width - 2)));
    canvas.finish()
}

// MARK: ask

/// What the ask welcome box counts.
pub struct Loaded {
    pub posts: usize,
    pub mentions: usize,
    pub liked: usize,
}

pub fn ask_welcome(handle: &str, loaded: &Loaded, width: usize) -> Vec<StyledLine> {
    let right = width - 1;
    let div = 34.min(width / 3);
    let rc = div + 3;
    let mut canvas = Canvas::new(width);

    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Line, "╭─");
    let x = canvas.put(y, 3, Kind::Dim, &format!("SoCLI {VERSION}"));
    let fill = div.saturating_sub(x + 1);
    canvas.put(y, x + 1, Kind::Line, &"─".repeat(fill));
    canvas.put(y, div, Kind::Line, "┬");
    canvas.put(y, div + 1, Kind::Line, &"─".repeat(right - div - 1));
    canvas.put(y, right, Kind::Line, "╮");

    let center = |text: &str| 1 + div.saturating_sub(1 + text.width()) / 2;
    let draw_so = theme::so_width() + 2 <= div;
    let logo = theme::logo();
    let counts = [
        (Kind::Bold, loaded.posts.to_string()),
        (Kind::Fg, " posts, ".into()),
        (Kind::Bold, loaded.mentions.to_string()),
        (Kind::Fg, " mentions, ".into()),
        (Kind::Bold, loaded.liked.to_string()),
        (Kind::Fg, " liked".into()),
    ];
    let handle = format!("@{handle}");
    for row in 0..10 {
        let y = canvas.push(false);
        match row {
            1..=6 if draw_so => {
                let so_x = 1 + div.saturating_sub(1 + theme::so_width()) / 2;
                if let Some(mark) = theme::so_mark().get(row - 1) {
                    put_so(&mut canvas, y, so_x, mark);
                }
            }
            1..=5 => {
                let mark = &logo[row - 1];
                canvas.put(y, center(mark), Kind::Bold, mark);
            }
            7 => {
                canvas.put(y, center("Welcome back"), Kind::Bold, "Welcome back");
            }
            8 => {
                canvas.put(y, center(&handle), Kind::Dim, &handle);
            }
            _ => {}
        }
        match row {
            1 => {
                canvas.put(y, rc, Kind::Bold, "Loaded");
            }
            2 => {
                let mut x = rc;
                for (kind, text) in &counts {
                    x = canvas.put(y, x, *kind, text);
                }
            }
            4 => {
                canvas.put(y, rc, Kind::Line, &"─".repeat(right.saturating_sub(rc + 2)));
            }
            6 => {
                canvas.put(y, rc, Kind::Bold, "Try");
            }
            7 => {
                canvas.put(y, rc, Kind::Dim, "\"who replied to mina?\"");
            }
            8 => {
                canvas.put(y, rc, Kind::Dim, "\"posts about rust\"");
            }
            _ => {}
        }
        canvas.put(y, 0, Kind::Line, "│");
        canvas.put(y, div, Kind::Line, "│");
        canvas.put(y, right, Kind::Line, "│");
    }

    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Line, &format!("╰{}┴", "─".repeat(div - 1)));
    let label = " esc to close ";
    let tail = label.width() + 3;
    canvas.put(
        y,
        div + 1,
        Kind::Line,
        &"─".repeat(right + 1 - tail - div - 1),
    );
    canvas.put_right(y, right, &[(Kind::Dim, label), (Kind::Line, "──╯")]);
    canvas.finish()
}

/// One question and the answer read from the loaded posts. Nothing calls a model.
pub fn ask_answer(question: &str, posts: &[Post], width: usize) -> Vec<StyledLine> {
    let words: Vec<String> = question
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| w.len() > 3)
        .collect();
    let hits: Vec<&Post> = posts
        .iter()
        .filter(|p| {
            let text = p.text.to_lowercase();
            words.is_empty() || words.iter().any(|w| text.contains(w))
        })
        .take(5)
        .collect();

    let mut canvas = Canvas::new(width);
    canvas.push(false);
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Dim, "▍");
    canvas.put(y, 2, Kind::Bold, question);
    canvas.push(false);
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Fg, "●");
    let x = canvas.put(y, 2, Kind::Fg, "1 tool call");
    canvas.put(y, x + 1, Kind::Dim, &format!("· {} posts read", hits.len()));
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Dim, "└");
    let x = canvas.put(y, 2, Kind::Fg, "local.search");
    canvas.put(y, x + 1, Kind::Dim, &format!("\"{}\"", words.join(" ")));
    canvas.put_right(
        y,
        72.min(width - 1),
        &[(Kind::Dim, &format!("{} posts", hits.len()))],
    );
    canvas.push(false);
    let y = canvas.push(false);
    if hits.is_empty() {
        canvas.put(y, 2, Kind::Fg, "Nothing already loaded matches that.");
        return canvas.finish();
    }
    canvas.put(y, 2, Kind::Fg, "From the posts on this screen:");
    for post in hits {
        let y = canvas.push(false);
        let x = canvas.put(y, 4, Kind::Blue, &format!("@{}", post.author.username));
        let snippet = post.text.replace('\n', " ");
        let room = width.saturating_sub(x + 2);
        canvas.put(y, x + 2, Kind::Fg, &util::truncate(&snippet, room));
    }
    canvas.finish()
}

/// The input row and the footer under it.
pub fn ask_prompt(input: &str, handle: &str, width: usize) -> Vec<StyledLine> {
    let mut canvas = Canvas::new(width);
    canvas.push(false);
    let y = canvas.push(false);
    canvas.put(y, 0, Kind::Fg, "▍");
    canvas.put(y, 2, Kind::Fg, input);
    canvas.push(false);
    let y = canvas.push(false);
    canvas.put(
        y,
        0,
        Kind::Dim,
        &format!("local · reads the posts already loaded · @{handle}"),
    );
    canvas.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn the_sign_in_screen_keeps_the_input_in_the_form() {
        let url = "https://dev-null.sh/oauth/authorize?client_id=abc&redirect_uri=devnull%3A%2F%2Fauth%2Fcallback&code_challenge=xxxxxxxxxxxxxxxx";
        let aside = format!("{url}\n\nAfter you approve, paste the callback.");
        let (lines, column) = login_frame(
            "dev/null",
            &[
                ("Device key", "abc"),
                ("Approve in the browser", "the address is on the right"),
                ("Paste the callback", "devnull://auth/callback?code=…"),
            ],
            &aside,
            Some("Callback:"),
            100,
        );
        let text: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert!(
            text.iter()
                .any(|line| line.trim_start().starts_with("│ https://dev-null.sh")),
            "{text:#?}"
        );
        assert!(
            text.iter()
                .any(|line| line.contains('└') && line.contains("Callback:")),
            "{text:#?}"
        );
        assert!(text.iter().all(|line| line.width() <= 100), "{text:#?}");
        assert!(lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.kind == Kind::Blue && span.text.contains('█'))
        }));
        let joined = text.join("\n");
        assert!(!joined.contains("▄██████████▄"), "{joined}");
        assert!(joined.contains("▄██████▄"), "{joined}");
        assert_eq!(column, 2);
        let sign = text
            .iter()
            .find(|line| line.contains("Sign in to dev/null"))
            .unwrap();
        assert_eq!(
            sign.find("Sign in to dev/null")
                .map(|at| sign[..at].width()),
            Some(2)
        );
        let mark = text.iter().find(|line| line.contains("▄██████▄")).unwrap();
        assert_eq!(mark.find("▄██████▄").map(|at| mark[..at].width()), Some(2));
        assert!(text.last().unwrap().starts_with("└ Callback:"));
    }

    #[test]
    fn the_signed_in_rows_continue_the_login_rail() {
        let (lines, _) = login_frame(
            "X",
            &[
                ("How to sign in", "Pick the account socli should use"),
                ("Browsers", "Chrome, Brave, Edge, Arc or Chromium"),
            ],
            "",
            None,
            80,
        );
        let text: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert!(text.iter().all(|line| !line.contains('└')), "{text:#?}");
        assert!(text
            .iter()
            .any(|line| line.contains('◇') && line.contains("How to sign in")));
        note_login_rail(true);
        let signed = signed_in_lines("ada", Some("Brave · Personal"), false);
        let signed_text: Vec<String> = signed.iter().map(StyledLine::plain).collect();
        let step = signed_text
            .iter()
            .find(|line| line.contains('◇') && line.contains("Signed in"))
            .unwrap();
        assert_eq!(step.find("Signed").map(|at| step[..at].width()), Some(2));
        assert!(signed_text.iter().any(|line| {
            line.contains('│') && line.contains("@ada") && line.contains("Brave · Personal")
        }));
        assert!(
            signed_text.last().unwrap().starts_with('└'),
            "{signed_text:#?}"
        );
        let help = signed_text
            .iter()
            .find(|line| line.contains("socli post"))
            .unwrap();
        let command_at = help
            .find("socli post")
            .map(|at| help[..at].width())
            .unwrap();
        let posts_at = help.find("posts").map(|at| help[..at].width()).unwrap();
        assert!(posts_at >= command_at + 14, "{help}");
        note_login_rail(true);
        expect_login_result(true);
        let open = signed_in_lines("ada", None, false);
        expect_login_result(false);
        note_login_rail(false);
        assert!(open.iter().all(|line| !line.plain().contains('└')));
        let (done, _) = login_result_lines(Ok(()), 80);
        let done_text: Vec<String> = done.iter().map(StyledLine::plain).collect();
        let last = done_text.last().unwrap();
        assert!(
            last.contains('└') && last.contains("Sign-in completed"),
            "{done_text:#?}"
        );
        assert_eq!(last.find("Sign-in").map(|at| last[..at].width()), Some(2));
    }

    #[test]
    fn the_account_menu_marks_one_row_on_the_rail() {
        let lines = menu_lines(
            &[
                "Brave · Personal".into(),
                "A different account on x.com".into(),
            ],
            1,
            80,
        );
        let text: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert!(text[0].starts_with('│'), "{text:#?}");
        assert!(text[0].contains("Brave"), "{text:#?}");
        assert!(!text[0].contains('>'), "{text:#?}");
        assert!(text[1].starts_with('│'), "{text:#?}");
        assert!(text[1].contains('>'), "{text:#?}");
        assert!(text[1].contains("A different account"), "{text:#?}");
        assert_eq!(text[1].find('>').map(|at| text[1][..at].width()), Some(2));
    }

    #[test]
    fn a_prompt_sign_in_keeps_the_result_off_the_closed_bracket() {
        note_login_rail(false);
        let lines = signed_in_lines("ada", None, false);
        let text: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert!(
            text.iter()
                .all(|line| !line.contains('│') && !line.contains('└') && !line.contains('◇')),
            "{text:#?}"
        );
        let signed = text.iter().find(|line| line.contains("@ada")).unwrap();
        assert_eq!(signed.find('✓').map(|at| signed[..at].width()), Some(5));
    }

    fn demo_post(net: Network) -> Post {
        let state = State::boot(
            true,
            15,
            &crate::config::Config::default(),
            &crate::net::Clients::empty(),
            None,
        );
        let mut posts: Vec<&Post> = state.posts.values().filter(|p| p.net == net).collect();
        posts.sort_by(|a, b| a.id.cmp(&b.id));
        posts[0].clone()
    }

    #[test]
    fn share_rows_follow_what_the_network_can_do() {
        use ShareAction::*;
        let all = Network::ALL;
        let mut post = demo_post(Network::X);
        assert_eq!(
            Share::post(&post, &all).rows,
            [CopyLink, CopyProfile, Repost, Quote]
        );
        post.reposted = true;
        let share = Share::post(&post, &all);
        assert_eq!(share.rows[2], Unrepost);
        assert_eq!(share.row_text(Unrepost).0, "Undo repost on X");
        let mut threads = demo_post(Network::Threads);
        threads.reposted = true;
        let share = Share::post(&threads, &all);
        assert_eq!(share.rows[2], Reposted);
        assert_eq!(share.row_text(Reposted).0, "Reposted on Threads");
        let devnull = demo_post(Network::Devnull);
        assert_eq!(
            Share::post(&devnull, &all).rows,
            [CopyLink, CopyProfile, Quote]
        );
        assert_eq!(Share::post(&devnull, &[]).rows, [CopyLink, CopyProfile]);
    }

    #[test]
    fn quote_label_and_targets_follow_the_checked_networks() {
        let post = demo_post(Network::Threads);
        let mut share = Share::post(&post, &Network::ALL);
        assert_eq!(
            share
                .targets
                .iter()
                .map(|(net, on)| (net.slug(), *on))
                .collect::<Vec<_>>(),
            [
                ("threads", true),
                ("x", false),
                ("bluesky", false),
                ("devnull", false),
                ("mastodon", false)
            ]
        );
        assert_eq!(share.quote_label(), "Quote on");
        assert_eq!(share.title(), " Share post ");
        assert_eq!(
            share.row_text(ShareAction::CopyLink).1,
            util::bare_url(&post.permalink())
        );
        share.toggle(1).unwrap();
        share.toggle(0).unwrap();
        assert_eq!(share.quote_label(), "Share to");
        assert_eq!(share.checked(), [Network::X]);
        assert_eq!(share.toggle(1), Err("keep at least one network on".into()));
        let single = Share::post(&post, &[Network::Threads]);
        assert!(!single.boxes());
        assert_eq!(single.quote_label(), "Quote on Threads");
        let elsewhere = Share::post(&post, &[Network::Bluesky]);
        assert_eq!(elsewhere.quote_label(), "Share to Bluesky");
        assert_eq!(elsewhere.checked(), [Network::Bluesky]);
    }

    #[test]
    fn profile_share_copies_the_profile_and_shares_to() {
        use crate::key::KeyCode;
        let mut share = Share::profile(Network::Threads, "kal", &Network::ALL);
        assert_eq!(share.title(), " Share profile ");
        assert_eq!(share.rows, [ShareAction::CopyProfile, ShareAction::Quote]);
        assert_eq!(
            share.row_text(ShareAction::CopyProfile),
            ("Copy profile link".into(), "threads.com/@kal".into(), 's')
        );
        assert_eq!(share.quote_label(), "Share to");
        assert_eq!(
            share.key(KeyCode::Char('s')),
            ShareStep::Run(ShareAction::CopyProfile)
        );
        assert_eq!(share.key(KeyCode::Char('p')), ShareStep::Stay);
        assert_eq!(share.key(KeyCode::Char(' ')), ShareStep::Stay);
        share.key(KeyCode::Char('j'));
        share.key(KeyCode::Right);
        assert_eq!(share.key(KeyCode::Char(' ')), ShareStep::Stay);
        share.key(KeyCode::Left);
        assert_eq!(share.key(KeyCode::Char(' ')), ShareStep::Stay);
        assert_eq!(share.checked(), [Network::X]);
        share.key(KeyCode::Char('l'));
        assert_eq!(
            share.key(KeyCode::Char(' ')),
            ShareStep::Status("keep at least one network on".into())
        );
        assert_eq!(
            share.key(KeyCode::Enter),
            ShareStep::Run(ShareAction::Quote)
        );
        assert_eq!(share.key(KeyCode::Esc), ShareStep::Close);
    }

    #[test]
    fn share_dialog_draws_mockup_twelve_at_sixty_four_columns() {
        let post = demo_post(Network::Threads);
        let share = Share::post(&post, &Network::ALL);
        let (lines, links, _) = share_lines(&share, 104, false, true);
        let lines: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].url, post.permalink());
        assert_eq!((links[0].x, links[0].y), (22, 2));
        assert_eq!(lines.len(), 10);
        assert!(lines.iter().all(|line| line.width() == 64), "{lines:#?}");
        assert!(lines[0].starts_with("╭─ Share post ─"));
        assert!(lines[3].starts_with("│  Copy profile link  threads.com/@"));
        assert!(lines[3].ends_with("p │"));
        assert_eq!(
            lines[5]
                .trim_end_matches(['│', ' '])
                .trim_end_matches('q')
                .trim_end(),
            "│  Quote on           ☑ Threads  ☐ X  ☐ Bluesky  ☐ dev/null"
        );
        assert_eq!(
            lines[6].trim_end_matches(['│', ' ']),
            "│                     ☐ Mastodon"
        );
        assert!(lines[8].starts_with("│  ⏎ choose  space toggle  esc close"));
        assert_eq!(
            share_lines(&share, 60, false, true).0[0].plain().width(),
            52
        );
    }

    #[test]
    fn share_quote_boxes_wrap_in_a_narrow_dialog() {
        let post = demo_post(Network::Threads);
        let share = Share::post(&post, &Network::ALL);
        let lines: Vec<String> = share_lines(&share, 60, false, true)
            .0
            .iter()
            .map(StyledLine::plain)
            .collect();
        assert!(lines.iter().all(|line| line.width() == 52), "{lines:#?}");
        let quote = lines.iter().position(|l| l.contains("Quote on")).unwrap();
        assert!(lines[quote].ends_with("q │"), "{lines:#?}");
        let next = &lines[quote + 1];
        assert!(next.starts_with("│                     ☐ "), "{lines:#?}");
    }

    #[test]
    fn the_network_picker_draws_like_the_share_dialog() {
        let state = State::boot(
            true,
            15,
            &crate::config::Config::default(),
            &crate::net::Clients::empty(),
            None,
        );
        let rows = network_rows(&state);
        assert_eq!(rows.len(), 6);
        let lines = pick_lines(" Network ", &rows, 0, 100, 40, false);
        let width = share_width(100);
        let text: Vec<String> = lines.iter().map(StyledLine::plain).collect();
        assert!(text.iter().all(|line| line.width() == width), "{text:#?}");
        assert!(text[0].starts_with("╭─ Network "), "{text:#?}");
        let chars = |row: &str, at: usize| row.chars().nth(at).unwrap();
        assert_eq!(chars(&text[2], 1), '▍');
        assert_eq!(chars(&text[2], 3), '●');
        assert_eq!(chars(&text[3], 1), ' ');
        for (i, key) in "012345".chars().enumerate() {
            assert_eq!(chars(&text[2 + i], width - 3), key, "{text:#?}");
        }
        assert_eq!(
            text[text.len() - 2],
            format!("│  ⏎ choose  esc close{}│", " ".repeat(width - 23))
        );
    }

    #[test]
    fn with_one_network_only_all_is_current() {
        let mut state = State::boot(
            true,
            15,
            &crate::config::Config::default(),
            &crate::net::Clients::empty(),
            None,
        );
        state.nets.retain(|info| info.net == Network::Bluesky);
        state.on = vec![Network::Bluesky];
        let rows = network_rows(&state);
        let current: Vec<&str> = rows
            .iter()
            .filter(|row| row.current)
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(current, ["All networks"]);
    }

    #[test]
    fn long_pickers_scroll_around_the_cursor() {
        let state = State::boot(
            true,
            15,
            &crate::config::Config::default(),
            &crate::net::Clients::empty(),
            None,
        );
        let rows = theme_rows(&state);
        assert_eq!(rows.len(), 31);
        let text = |cursor: usize| -> Vec<String> {
            pick_lines(" Theme ", &rows, cursor, 100, 20, false)
                .iter()
                .map(StyledLine::plain)
                .collect()
        };
        let lines = text(20);
        assert_eq!(lines.len(), 12 + 5);
        assert!(lines.iter().any(|line| line.contains(&rows[20].label)));
        assert!(!lines.iter().any(|line| line.contains(" auto ")));
        assert!(lines[lines.len() - 2].contains("21/31"), "{lines:#?}");
        let first = text(0);
        assert!(first[2].contains("auto"), "{first:#?}");
        let last = text(30);
        assert!(last[last.len() - 4].contains(&rows[30].label), "{last:#?}");
    }

    #[test]
    fn key_help_follows_the_keys_mockup() {
        let lines: Vec<String> = key_help_lines(100, false)
            .iter()
            .map(|line| line.plain().trim_end().to_string())
            .collect();
        assert_eq!(lines[0], "  Move      j k  ↓ ↑          move");
        for row in [
            "            space PgDn        next page",
            "            u                 author's profile",
            "            f /               search",
            "            F6 shift-F6       next / previous zone",
            "  View      d                 next detail level",
            "            D                 pick the detail level",
            "            x                 show or hide the replies here",
            "            a                 next network, then all",
            "            A                 pick a network",
            "            ctrl-r cmd-r      reload this page",
            "            F                 filter networks",
            "            ⌘/ctrl-click      open names and links in the browser",
        ] {
            assert!(lines.iter().any(|line| line == row), "{row}");
        }
    }

    #[test]
    fn plain_key_help_spells_the_keys() {
        let text: Vec<String> = key_help_lines(100, true)
            .iter()
            .map(StyledLine::plain)
            .collect();
        assert!(text.iter().any(|line| line.contains("bksp left esc")));
        assert!(text.iter().any(|line| line.contains("1-9")));
        assert!(text.iter().all(|line| line.is_ascii()), "{text:?}");
    }

    #[test]
    fn key_help_uses_two_columns_when_wide() {
        let narrow = key_help_lines(100, false);
        let wide = key_help_lines(140, false);
        assert!(wide.len() < narrow.len());
        let right_labels = wide
            .iter()
            .filter(|line| {
                let rest: String = line.plain().chars().skip(72).collect();
                ["Act", "Go", "View", "Other"]
                    .iter()
                    .any(|label| rest.starts_with(label))
            })
            .count();
        assert!(right_labels >= 1);
        assert!(wide.iter().any(|line| line
            .plain()
            .ends_with("open names and links in the browser")));
        assert_eq!(key_help_lines(124, false).len(), narrow.len());
    }

    #[test]
    fn ask_box_is_exactly_the_inner_width() {
        let loaded = Loaded {
            posts: 9,
            mentions: 2,
            liked: 1,
        };
        for line in ask_welcome("you", &loaded, 100) {
            assert_eq!(line.plain().width(), 100, "{}", line.plain());
        }
    }
    #[test]
    fn every_wrapped_authorization_segment_keeps_the_full_query() {
        let url = "https://dev-null.sh/mobile/authorize?fingerprint=SHA256%3Atest&code_challenge=challenge&state=state&redirect_uri=devnull%3A%2F%2Fauth%2Fcallback";
        let (lines, _, links) = login_frame_linked("dev/null", &[], url, Some("Callback:"), 40);
        assert!(links.len() > 2);
        for link in &links {
            assert_eq!(link.url, url);
            let line = &lines[usize::from(link.y)];
            let painted = paint_linked_line(line, &[link], false);
            assert!(painted.contains(&format!(";{url}\x1b\\")));
            assert!(painted.contains("\x1b]8;;\x1b\\"));
        }
    }
}
