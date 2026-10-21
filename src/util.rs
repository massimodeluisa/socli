//! Small formatting helpers shared by the reader and the printer.

use std::borrow::Cow;
use std::fmt;
use std::io::{self, Write};
use std::iter::Peekable;
use std::str::Chars;

use chrono::{DateTime, Utc};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn ago(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - then).num_seconds().max(0);
    if secs < 5 {
        "now".into()
    } else if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3_600)
    } else if secs < 14 * 86_400 {
        format!("{}d", secs / 86_400)
    } else {
        then.format("%b %d").to_string()
    }
}

/// `2m ago`, `1h ago`, `yesterday`: the drafts list and its restore notice.
pub fn ago_long(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - then).num_seconds().max(0);
    match secs {
        0..60 => "just now".into(),
        60..3_600 => format!("{}m ago", secs / 60),
        3_600..86_400 => format!("{}h ago", secs / 3_600),
        86_400..172_800 => "yesterday".into(),
        172_800..1_209_600 => format!("{}d ago", secs / 86_400),
        _ => then.format("%b %d").to_string(),
    }
}

/**
Wraps each paragraph of `text` after [`clean`], so it measures what will be shown. A line still
wider than `width` is cut between graphemes; a run of blank lines keeps one, and none lead or trail.
*/
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let text = clean(text);
    let mut out = Vec::new();
    for para in text.split('\n') {
        if para.trim().is_empty() {
            if out.last().is_some_and(|line: &String| !line.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        // HyphenSplitter breaks "sign-in" into "sign-" / "in" when the column ends on the hyphen.
        let lines = textwrap::wrap(
            para,
            textwrap::Options::new(width)
                .break_words(false)
                .word_splitter(textwrap::WordSplitter::NoHyphenation),
        );
        if lines.is_empty() {
            out.push(String::new());
        }
        for line in lines {
            push_cut(&line, width, &mut out);
        }
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Pushes `line` in pieces no wider than `width`, cut between graphemes; a wider grapheme is a piece of its own.
fn push_cut(line: &str, width: usize, out: &mut Vec<String>) {
    if line.width() <= width {
        out.push(line.to_string());
        return;
    }
    let mut piece = String::new();
    let mut used = 0;
    for (grapheme, w) in graphemes(line) {
        if used + w > width && !piece.is_empty() {
            out.push(std::mem::take(&mut piece));
            used = 0;
        }
        piece.push_str(grapheme);
        used += w;
    }
    if !piece.is_empty() {
        out.push(piece);
    }
}

/// `text` cut to `width` columns at a grapheme boundary, with `…` in the last column when it is cut.
pub fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for (grapheme, w) in graphemes(text) {
        if used + w + 1 > width {
            break;
        }
        out.push_str(grapheme);
        used += w;
    }
    out.push('…');
    out
}

// MARK: graphemes and controls

/// Each extended grapheme cluster of `text` with its width, measured the way ratatui measures it.
pub fn graphemes(text: &str) -> impl Iterator<Item = (&str, usize)> {
    text.graphemes(true)
        .map(|grapheme| (grapheme, grapheme.width()))
}

/**
`text` with nothing that can drive a terminal, borrowed when nothing changes. `\r\n`, a lone `\r`,
U+2028 and U+2029 become `\n`, a tab becomes four spaces, and whole ESC and C1 sequences go: CSI
up to its final byte, OSC up to BEL or ST, and DCS, SOS, PM and APC up to ST, or to the end when
unterminated. Every other C0 and C1 control, DEL and the bidi embedding, override and isolate
controls go too; ZWJ, ZWNJ, variation selectors and the LRM and RLM marks stay.
*/
pub fn clean(text: &str) -> Cow<'_, str> {
    if !text
        .chars()
        .any(|ch| matches!(ch, '\r' | '\t' | '\u{2028}' | '\u{2029}') || dropped(ch))
    {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push('\n');
            }
            '\n' | '\u{2028}' | '\u{2029}' => out.push('\n'),
            '\t' => out.push_str("    "),
            '\x1b' => match chars.next_if(|ch| matches!(ch, '[' | ']' | 'P' | 'X' | '^' | '_')) {
                Some('[') => skip_csi(&mut chars),
                Some(']') => skip_string(&mut chars, true, false),
                Some(_) => skip_string(&mut chars, false, false),
                None => skip_escape(&mut chars),
            },
            '\u{9b}' => skip_csi(&mut chars),
            '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => {
                skip_string(&mut chars, true, true)
            }
            ch if dropped(ch) => {}
            ch => out.push(ch),
        }
    }
    Cow::Owned(out)
}

/// C0 controls but `\n`, DEL, C1 controls, and the bidi embedding, override and isolate controls.
fn dropped(ch: char) -> bool {
    (ch.is_control() && ch != '\n')
        || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// The rest of a CSI: parameter bytes, intermediate bytes and one final byte.
fn skip_csi(chars: &mut Peekable<Chars>) {
    while chars.next_if(|ch| ('\x30'..='\x3f').contains(ch)).is_some() {}
    while chars.next_if(|ch| ('\x20'..='\x2f').contains(ch)).is_some() {}
    chars.next_if(|ch| ('\x40'..='\x7e').contains(ch));
}

/// The rest of any other ECMA-35 escape: intermediate bytes and one final byte.
fn skip_escape(chars: &mut Peekable<Chars>) {
    while chars.next_if(|ch| ('\x20'..='\x2f').contains(ch)).is_some() {}
    chars.next_if(|ch| ('\x30'..='\x7e').contains(ch));
}

/// The rest of a string sequence, through `ESC \`, BEL when `bel`, or U+009C when `st`.
fn skip_string(chars: &mut Peekable<Chars>, bel: bool, st: bool) {
    while let Some(ch) = chars.next() {
        match ch {
            '\x07' if bel => return,
            '\u{9c}' if st => return,
            '\x1b' if chars.next_if_eq(&'\\').is_some() => return,
            _ => {}
        }
    }
}

/// A link as people read it: no `https://` or `http://`, no `www.`.
pub fn bare_url(url: &str) -> &str {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.strip_prefix("www.").unwrap_or(rest)
}

pub fn terminal_width(fallback: usize) -> usize {
    #[cfg(feature = "native")]
    {
        crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(fallback)
            .max(20)
    }
    #[cfg(not(feature = "native"))]
    {
        fallback.max(20)
    }
}

// MARK: output

/// Maps a stdout write; a closed pipe (`socli feed --print | head`) ends the process quietly with status 0.
pub fn written(result: io::Result<()>) -> Result<(), String> {
    match result {
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => std::process::exit(0),
        other => other.map_err(|e| e.to_string()),
    }
}

/// Prints `text`, cleaned of terminal controls, and a newline through a locked stdout.
pub fn say(text: impl fmt::Display) -> Result<(), String> {
    let text = text.to_string();
    written(writeln!(io::stdout().lock(), "{}", clean(&text)))
}

// MARK: sign-in prompts

#[cfg(not(feature = "native"))]
pub fn prompt_choice(labels: &[&str], indent: &str) -> Result<usize, String> {
    let _ = (labels, indent);
    Err(NEEDS_PERSON.into())
}

#[cfg(feature = "native")]
pub fn prompt_choice(labels: &[&str], indent: &str) -> Result<usize, String> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal::{self, Clear, ClearType};
    if labels.is_empty() {
        return Err("no sign-in action is available".into());
    }
    if !person_present() {
        return Err(NEEDS_PERSON.into());
    }
    let mut out = std::io::stderr();
    terminal::enable_raw_mode().map_err(|err| err.to_string())?;
    let result = {
        let _cooked = CookOnDrop;
        crate::watchdog::arm(crate::watchdog::Probe::None);
        let mut selected = 0;
        loop {
            crossterm::execute!(
                out,
                crossterm::cursor::MoveToColumn(0),
                Clear(ClearType::CurrentLine)
            )
            .map_err(|err| err.to_string())?;
            write!(out, "{indent}").map_err(|err| err.to_string())?;
            for (index, label) in labels.iter().enumerate() {
                write!(
                    out,
                    "{}{}   ",
                    if index == selected { "> " } else { "  " },
                    label
                )
                .map_err(|err| err.to_string())?;
            }
            out.flush().map_err(|err| err.to_string())?;
            let selection = loop {
                crate::watchdog::beat();
                if !event::poll(std::time::Duration::from_millis(200))
                    .map_err(|err| err.to_string())?
                {
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
                    break Some(labels.len() - 1);
                }
                match key.code {
                    KeyCode::Enter => break Some(selected),
                    KeyCode::Tab | KeyCode::Right | KeyCode::Down | KeyCode::Char('j') => {
                        selected = (selected + 1) % labels.len();
                        break None;
                    }
                    KeyCode::BackTab | KeyCode::Left | KeyCode::Up | KeyCode::Char('k') => {
                        selected = (selected + labels.len() - 1) % labels.len();
                        break None;
                    }
                    KeyCode::Char(ch) => {
                        if let Some(index) = labels.iter().position(|label| {
                            label
                                .to_ascii_lowercase()
                                .starts_with(ch.to_ascii_lowercase())
                        }) {
                            break Some(index);
                        }
                    }
                    _ => {}
                }
            };
            if let Some(index) = selection {
                break Ok(index);
            }
        }
    };
    writeln!(out).map_err(|err| err.to_string())?;
    result
}

/// The start of every error a prompt returns when nobody can answer it; `main` exits 2 on it.
pub const NEEDS_PERSON: &str = "this step needs a person at a terminal.";

/// Whether a person can answer a prompt: stdin and stderr are terminals and agent mode is off.
pub fn person_present() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal() && !crate::agent::active()
}

/// Reads one visible line from the terminal after printing `label` on stderr; without a person it returns NEEDS_PERSON and `hint`.
pub fn prompt_line(label: &str, hint: &str) -> Result<String, String> {
    #[cfg(feature = "native")]
    {
        read_prompt(label, hint, false)
    }
    #[cfg(not(feature = "native"))]
    {
        let _ = label;
        Err(format!("{NEEDS_PERSON} {hint}"))
    }
}

/// Reads a masked secret; Escape and Ctrl-C cancel without terminating the reader.
pub fn prompt_secret(label: &str, hint: &str) -> Result<String, String> {
    #[cfg(feature = "native")]
    {
        read_prompt(label, hint, true)
    }
    #[cfg(not(feature = "native"))]
    {
        let _ = label;
        Err(format!("{NEEDS_PERSON} {hint}"))
    }
}

#[cfg(feature = "native")]
fn read_prompt(label: &str, hint: &str, masked: bool) -> Result<String, String> {
    use crate::watchdog;
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::terminal;

    if !person_present() {
        return Err(format!("{NEEDS_PERSON} {hint}"));
    }
    let mut out = std::io::stderr();
    crossterm::execute!(out, crossterm::cursor::Show).map_err(|e| e.to_string())?;
    write!(out, "{label}").map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    terminal::enable_raw_mode().map_err(|e| format!("the terminal refused raw mode: {e}"))?;
    let result = {
        let _cooked = CookOnDrop;
        watchdog::arm(watchdog::Probe::None);
        let mut answer = String::new();
        loop {
            watchdog::beat();
            let ready = event::poll(std::time::Duration::from_millis(200));
            let key = match ready.and_then(|ready| ready.then(event::read).transpose()) {
                Ok(Some(Event::Key(key))) if key.kind != KeyEventKind::Release => key,
                Ok(_) => continue,
                Err(e) => break Err(format!("reading the terminal: {e}")),
            };
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Enter => {
                    break Ok(if masked {
                        answer
                    } else {
                        answer.trim().to_string()
                    })
                }
                KeyCode::Esc => break Err("cancelled".to_string()),
                KeyCode::Char('c' | 'd') if ctrl => break Err("cancelled".to_string()),
                KeyCode::Backspace => {
                    if let Some(ch) = answer.pop() {
                        let cells = if masked { 1 } else { ch.to_string().width() };
                        for _ in 0..cells {
                            write!(out, "\u{8} \u{8}").map_err(|e| e.to_string())?;
                        }
                    }
                }
                KeyCode::Char(ch) if !ctrl && !ch.is_control() => {
                    answer.push(ch);
                    if masked {
                        write!(out, "•")
                    } else {
                        write!(out, "{ch}")
                    }
                    .map_err(|e| e.to_string())?;
                }
                _ => {}
            }
            out.flush().map_err(|e| e.to_string())?;
        }
    };
    writeln!(out).map_err(|e| e.to_string())?;
    result
}

/// Puts the terminal back in cooked mode if a secret prompt unwinds.
#[cfg(feature = "native")]
struct CookOnDrop;

#[cfg(feature = "native")]
impl Drop for CookOnDrop {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        crate::watchdog::disarm();
    }
}

/**
The address this page should return to: `http` only for `127.0.0.1`, otherwise `https`,
with no user, query, or fragment. X compares the callback as an exact string.
*/
pub fn page_callback(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let bad = || "this page has no address to return to.".to_string();
    if raw.is_empty()
        || raw
            .chars()
            .any(|c| c.is_whitespace() || c == '@' || c == '\\' || c == '#')
    {
        return Err(bad());
    }
    let url = url::Url::parse(raw).map_err(|_| bad())?;
    let host = url.host_str().unwrap_or("");
    let scheme_ok = match url.scheme() {
        "https" => !host.is_empty(),
        "http" => host == "127.0.0.1",
        _ => false,
    };
    if !scheme_ok
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bad());
    }
    Ok(raw.to_string())
}

/// This page's origin and path. The worker fills it. A terminal has no page to return to.
pub fn page_url() -> Result<String, String> {
    #[cfg(target_arch = "wasm32")]
    {
        return page_url_from_host();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Err("this sign-in continues in the browser.".into())
    }
}

#[cfg(target_arch = "wasm32")]
fn page_url_from_host() -> Result<String, String> {
    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        fn socli_page(buf: *mut u8, cap: usize) -> usize;
    }
    let mut buf = vec![0u8; 2048];
    // SAFETY: `buf` is a buffer we allocated and `buf.len()` is its cap; `n` is checked against that length before the slice is used.
    let n = unsafe { socli_page(buf.as_mut_ptr(), buf.len()) };
    if n == 0 || n > buf.len() {
        return Err("this page has no address to return to.".into());
    }
    buf.truncate(n);
    let text =
        String::from_utf8(buf).map_err(|_| "this page has no address to return to.".to_string())?;
    page_callback(&text)
}

/**
Opener and the one allow-listed URL.
On Windows the program is `ShellExecuteW`, so `cmd` does not split the URL on `&`.
*/
struct BrowserPlan {
    #[cfg_attr(not(feature = "native"), allow(dead_code))]
    program: &'static str,
    arg: String,
}

/// The program and the single URL argument after [`crate::hyperlink::safe`] accepts it.
fn browser_plan(url: &str) -> Result<BrowserPlan, String> {
    let url = crate::hyperlink::safe(url)
        .ok_or_else(|| "only http and https links can be opened.".to_string())?;
    let program = if cfg!(target_os = "windows") {
        "ShellExecuteW"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Ok(BrowserPlan { program, arg: url })
}

/// Opens an http or https URL in the default browser.
pub fn open_browser(url: &str) -> Result<(), String> {
    let plan = browser_plan(url)?;
    #[cfg(not(feature = "native"))]
    {
        crate::clip::note_open(&plan.arg);
        return Ok(());
    }
    #[cfg(feature = "native")]
    {
        spawn_browser(&plan)
    }
}

#[cfg(feature = "native")]
fn spawn_browser(plan: &BrowserPlan) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if plan.program != "ShellExecuteW" {
            return Err("could not open the link.".into());
        }
        return shell_execute(&plan.arg);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let url = &plan.arg;
        std::process::Command::new(plan.program)
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open {url}: {e}"))
    }
}

/// Passes `url` to `ShellExecuteW` as one `lpFile` argument.
#[cfg(all(feature = "native", target_os = "windows"))]
fn shell_execute(url: &str) -> Result<(), String> {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    const SW_SHOWNORMAL: i32 = 1;
    let operation: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0u16)).collect();
    let file: Vec<u16> = url.encode_utf16().chain(std::iter::once(0u16)).collect();
    // SAFETY: `operation` and `file` are NUL-terminated UTF-16 buffers we allocated, with no interior NUL (`file` is printable ASCII from `hyperlink::safe`), and they live for this call. ShellExecuteW does not retain the pointers. `file` is the single URL argument; hwnd, parameters, and directory are null.
    let code = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if code > 32 {
        Ok(())
    } else {
        Err(format!(
            "could not open {url}: ShellExecuteW returned {code}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;
    use chrono::TimeZone;

    #[test]
    fn relative_time_buckets() {
        let now = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap();
        assert_eq!(ago(now - chrono::Duration::seconds(30), now), "30s");
        assert_eq!(ago(now - chrono::Duration::minutes(18), now), "18m");
        assert_eq!(ago(now - chrono::Duration::hours(5), now), "5h");
        assert_eq!(ago(now - chrono::Duration::days(3), now), "3d");
    }

    #[test]
    fn bare_urls_drop_the_scheme_and_www() {
        assert_eq!(bare_url("https://www.threads.com/@kal"), "threads.com/@kal");
        assert_eq!(bare_url("http://x.com/river"), "x.com/river");
        assert_eq!(bare_url("bsky.app/profile/ada"), "bsky.app/profile/ada");
    }

    #[test]
    fn wrapping_keeps_blank_lines() {
        let lines = wrap_text("one two\n\nthree", 8);
        assert!(lines.iter().any(|l| l.is_empty()));
        assert!(lines.iter().any(|l| l.contains("three")));
    }

    #[test]
    fn wrapping_keeps_at_most_one_blank_line() {
        assert_eq!(wrap_text("\none\n\n\n\ntwo\n\n", 20), ["one", "", "two"]);
    }

    #[test]
    fn wrapping_keeps_a_hyphenated_word_together() {
        let lines = wrap_text("finish X sign-in now", 14);
        assert!(
            lines.iter().any(|line| line.contains("sign-in")),
            "{lines:?}"
        );
        assert!(
            lines.iter().all(|line| !line.ends_with("sign-")),
            "{lines:?}"
        );
    }

    /// Every fixture string, alone and as the text of its post.
    fn fixture_texts() -> Vec<String> {
        let posts = fixtures::graphemes()
            .into_iter()
            .chain(fixtures::controls());
        fixtures::GRAPHEME_TEXTS
            .iter()
            .chain(&fixtures::CONTROL_TEXTS)
            .map(|text| text.to_string())
            .chain(posts.map(|post| post.text))
            .collect()
    }

    #[test]
    fn a_string_is_as_wide_as_its_graphemes() {
        for text in fixture_texts() {
            let sum: usize = graphemes(&text).map(|(_, w)| w).sum();
            assert_eq!(text.width(), sum, "{text:?}");
        }
    }

    #[test]
    fn wrapped_fixture_lines_fit_and_keep_every_grapheme_whole() {
        let visible = |text: &str| -> Vec<String> {
            graphemes(text)
                .map(|(grapheme, _)| grapheme.to_string())
                .filter(|grapheme| grapheme != " " && grapheme != "\n")
                .collect()
        };
        for text in fixture_texts() {
            let shown = visible(&clean(&text));
            for width in 1..=40 {
                let lines = wrap_text(&text, width);
                for line in &lines {
                    let one = graphemes(line).count() == 1;
                    assert!(
                        line.width() <= width || one,
                        "{text:?} at {width}: {line:?}"
                    );
                    assert!(
                        !line.chars().any(char::is_control),
                        "{text:?} at {width}: {line:?}"
                    );
                }
                let wrapped: Vec<String> = lines.iter().flat_map(|line| visible(line)).collect();
                assert_eq!(wrapped, shown, "{text:?} at {width}");
            }
        }
    }

    #[test]
    fn truncation_keeps_whole_graphemes() {
        assert_eq!(truncate("👩‍💻👩‍💻", 3), "👩‍💻…");
        assert_eq!(truncate("e\u{301}e\u{301}e\u{301}", 2), "e\u{301}…");
        assert_eq!(truncate("日本語", 4), "日…");
        assert_eq!(truncate("日本語", 6), "日本語");
        assert_eq!(truncate("abc", 1), "…");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn cleaning_borrows_clean_text() {
        assert!(matches!(clean("José 👩‍💻 日本語\n"), Cow::Borrowed(_)));
        assert!(matches!(
            clean("a\u{200d}b\u{200c}c\u{fe0f}\u{200e}\u{200f}"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn cleaning_leaves_no_controls_and_keeps_the_words() {
        let bidi = |ch: char| matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
        for text in fixtures::CONTROL_TEXTS {
            let cleaned = clean(text);
            assert!(
                !cleaned.chars().any(|ch| (ch < ' ' && ch != '\n')
                    || ('\u{7f}'..='\u{9f}').contains(&ch)
                    || bidi(ch)),
                "{text:?} -> {cleaned:?}"
            );
        }
        let all: Vec<String> = fixtures::CONTROL_TEXTS
            .iter()
            .map(|text| clean(text).into_owned())
            .collect();
        assert_eq!(all, ["red", "click", "", "", "a\nb", "", "abc", ""]);
    }

    #[test]
    fn cleaning_follows_each_sequence_rule() {
        assert_eq!(clean("a\r\nb\rc\u{2028}d\u{2029}e"), "a\nb\nc\nd\ne");
        assert_eq!(clean("a\tb"), "a    b");
        assert_eq!(clean("\x1b[?25l\x1b[1;31mx\x1b[ q"), "x");
        assert_eq!(clean("\x1b]52;c;aGk=\x1b\\x"), "x");
        assert_eq!(clean("\x1b]0;unterminated title"), "");
        assert_eq!(clean("\x1bX sos \x07 still \x1b\\y"), "y");
        assert_eq!(clean("\x1b^pm\x1b\\\x1b_apc\x1b\\z"), "z");
        assert_eq!(clean("\x1bcreset\x1b(B!"), "reset!");
        assert_eq!(clean("\x1b(0x"), "x");
        assert_eq!(clean("\x1b#8y"), "y");
        assert_eq!(clean("\x1b"), "");
        assert_eq!(clean("\x1b\n"), "\n");
        assert_eq!(clean("trailing\x1b"), "trailing");
        assert_eq!(clean("\u{9b}31mx\u{9d}osc\u{9c}y\u{90}dcs\x07z"), "xyz");
        assert_eq!(clean("\u{85}a\u{7f}b\x00c\u{2066}d\u{2069}"), "abcd");
    }

    #[test]
    fn a_page_callback_is_this_origin_without_a_query() {
        assert_eq!(
            page_callback("http://127.0.0.1:5174/").unwrap(),
            "http://127.0.0.1:5174/"
        );
        assert_eq!(
            page_callback("http://127.0.0.1:5174/web/").unwrap(),
            "http://127.0.0.1:5174/web/"
        );
        assert_eq!(
            page_callback("https://socli.sh/").unwrap(),
            "https://socli.sh/"
        );
        assert_eq!(
            page_callback("https://web.socli.sh/web/").unwrap(),
            "https://web.socli.sh/web/"
        );
        for raw in [
            "http://localhost:5174/",
            "http://127.0.0.1:5174/?code=1",
            "https://socli.sh/#x",
            "https://user@socli.sh/",
            "javascript:alert(1)",
            "http://evil.example/",
        ] {
            assert!(page_callback(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn prompts_refuse_without_a_terminal() {
        let err = prompt_line("Handle: ", "Pass --handle.").unwrap_err();
        assert_eq!(err, format!("{NEEDS_PERSON} Pass --handle."));
        let err = prompt_secret("Password: ", "Use --password-stdin.").unwrap_err();
        assert!(err.starts_with(NEEDS_PERSON), "{err}");
    }

    #[test]
    fn open_browser_refuses_file_and_custom_schemes() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "myapp:whatever",
        ] {
            assert!(browser_plan(url).is_err(), "{url}");
            assert!(open_browser(url).is_err(), "{url}");
        }
    }

    #[test]
    fn browser_plan_keeps_an_ampersand_in_the_single_argument() {
        let url = "https://example.com/search?q=1&x=2";
        assert!(url.contains('&'));
        let argv = browser_plan(url).map(|plan| vec![plan.arg]);
        assert_eq!(argv, Ok(vec![url.to_string()]));
        let program = browser_plan(url).map(|plan| plan.program).unwrap_or("");
        assert_ne!(program, "cmd");
        let expected = if cfg!(target_os = "windows") {
            "ShellExecuteW"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        assert_eq!(program, expected);
    }
}
