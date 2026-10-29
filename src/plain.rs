/*!
Scrollback printer. The feed stays in the terminal, where images can paint
and where a pager, tmux copy mode, and a pipe still work.
*/

use std::io::{self, IsTerminal, Write};

use serde_json::Value;

use crate::agent;
use crate::chrome::{self, Hints};
use crate::clip;
use crate::command::{self, Cmd};
use crate::hyperlink::{self, Region};
use crate::media::{self, Origin};
use crate::model::Post;
use crate::net::Clients;
use crate::render::{self, Kind, Span, StyledLine};
use crate::screens;
use crate::state::{self, MergedTab, Outcome, State};
use crate::util;

pub struct PrintOpts {
    pub color: bool,
    pub json: bool,
    pub images: bool,
    pub prompt: bool,
    pub width: usize,
}

impl PrintOpts {
    pub fn from_flags(json: bool, images: bool, prompt: bool, width: Option<usize>) -> Self {
        let tty = io::stdout().is_terminal();
        let color = tty && std::env::var_os("NO_COLOR").is_none() && !json && !agent::active();
        Self {
            color,
            json,
            images,
            prompt: prompt && tty && io::stdin().is_terminal() && !agent::active() && !json,
            width: width.unwrap_or_else(|| util::terminal_width(80)),
        }
    }
}

/**
Writes `text` to stdout and flushes. A closed pipe ends the process with exit 0, so `socli … | head`
stays quiet; any other write error exits 1.
*/
pub fn emit(text: &str) {
    let mut out = io::stdout().lock();
    if let Err(err) = out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        if err.kind() == io::ErrorKind::BrokenPipe {
            #[cfg(feature = "native")]
            crate::program_status::finish(true);
            std::process::exit(0);
        }
        #[cfg(feature = "native")]
        crate::program_status::set(crate::program_status::Status::Error);
        eprintln!("socli: couldn't write the output: {err}");
        std::process::exit(1);
    }
}

/// `text` and a newline through [`emit`].
pub fn emit_line(text: &str) {
    emit(&format!("{text}\n"));
}

pub fn print_state(state: &State, opts: &PrintOpts) -> Result<(), String> {
    if opts.json {
        emit_line(&pretty(&state.feed_json())?);
        return Ok(());
    }
    let (feed, footer) = printed(state, opts, hyperlink::enabled());
    for line in feed {
        emit_line(&line);
    }
    if opts.images {
        print_images(state, opts)?;
    }
    for line in footer {
        emit_line(&line);
    }
    Ok(())
}

/// The printed feed and the footer that follows its images, one string per line, with OSC 8 `links` or without.
pub fn printed(state: &State, opts: &PrintOpts, links: bool) -> (Vec<String>, Vec<String>) {
    let full = opts.width.max(render::MIN_COLS);
    let width = render::inner_width_for(full, state.detail);
    let narrow = render::narrow(full);
    let margin = " ".repeat(render::margin_for(full, state.detail));
    let show = |line: &StyledLine| {
        if line.plain().is_empty() {
            String::new()
        } else {
            format!(
                "{margin}{}",
                render::paint_with(line, opts.color, &state.theme)
            )
        }
    };
    let chrome = |line: StyledLine| {
        if state.theme.ascii {
            line.ascii()
        } else {
            line
        }
    };
    let mut feed = vec![
        show(&chrome(chrome::printed_header(
            state,
            &state.crumb(),
            width,
        ))),
        show(&StyledLine::blank()),
    ];
    if state.profile().is_none() {
        feed.push(show(&chrome(chrome::subbar(state, width, None, narrow))));
        feed.push(show(&chrome(chrome::rule(width))));
        for _ in 0..state.density.rows(1, 1, 0) {
            feed.push(show(&StyledLine::blank()));
        }
    }
    let body = screens::linked_body(state, width, false);
    for (row, line) in body.lines.iter().enumerate() {
        let regions: Vec<&Region> = body
            .links
            .iter()
            .filter(|region| links && usize::from(region.y) == row)
            .collect();
        if regions.is_empty() {
            feed.push(show(line));
        } else {
            feed.push(show(&linked(line, &regions)));
        }
    }
    let mut footer = vec![
        show(&StyledLine::blank()),
        show(&chrome(chrome::rule(width))),
    ];
    if !narrow {
        footer.push(show(&chrome(chrome::nav(state, width))));
    }
    footer.push(show(&chrome(chrome::hints(
        Hints::of(state),
        None,
        "",
        width,
        state.theme.ascii,
    ))));
    if !state.status.is_empty() {
        let status = util::truncate(&state.status, width);
        footer.push(show(&StyledLine::text(Kind::Dim, &status)));
    }
    (feed, footer)
}

/// `line` with the text under each region wrapped in an OSC 8 link, split at the region edges.
fn linked(line: &StyledLine, regions: &[&Region]) -> StyledLine {
    let url_at = |col: usize| {
        regions
            .iter()
            .find(|region| {
                let start = usize::from(region.x);
                start <= col && col < start + usize::from(region.width)
            })
            .map(|region| region.url.as_str())
    };
    let mut spans = Vec::new();
    let mut col = 0;
    for span in &line.spans {
        let mut piece = String::new();
        let mut piece_url = None;
        for (grapheme, width) in util::graphemes(&span.text) {
            let url = url_at(col);
            if url != piece_url && !piece.is_empty() {
                spans.push(linked_span(span.kind, piece_url, &piece));
                piece.clear();
            }
            piece_url = url;
            piece.push_str(grapheme);
            col += width;
        }
        if !piece.is_empty() {
            spans.push(linked_span(span.kind, piece_url, &piece));
        }
    }
    StyledLine {
        spans,
        ..line.clone()
    }
}

fn linked_span(kind: Kind, url: Option<&str>, text: &str) -> Span {
    Span {
        kind,
        text: url.map_or_else(|| text.to_string(), |url| hyperlink::wrap(url, text)),
    }
}

fn print_images(state: &State, opts: &PrintOpts) -> Result<(), String> {
    let posts = ordered_posts(state);
    for post in posts {
        for media in &post.media {
            let Some(url) = media.view_url() else {
                continue;
            };
            emit_line("");
            emit_line(&paint_dim(
                &format!(
                    "▣ {} · {}",
                    media.kind.label(),
                    util::clean(&post.author.username)
                ),
                opts.color,
            ));
            let path = media::materialize(url, Origin::Post)?;
            media::show_with(&state.viewer, &path, 48, true)?;
        }
    }
    Ok(())
}

fn ordered_posts(state: &State) -> Vec<Post> {
    let posts = state.current_posts();
    let rows = state.lay(&posts);
    let mut out = Vec::new();
    for row in rows {
        if let Some(id) = crate::layout::post_id(&row) {
            if let Some(post) = state.posts.get(id) {
                if !out.iter().any(|p: &Post| p.id == post.id) {
                    out.push(post.clone());
                }
            }
        }
    }
    out
}

pub fn repl(state: &mut State, clients: &mut Clients, opts: &PrintOpts) -> Result<(), String> {
    print_state(state, opts)?;
    let stdin = io::stdin();
    let mut failed = false;
    loop {
        #[cfg(feature = "native")]
        crate::program_status::set(if failed {
            crate::program_status::Status::Error
        } else {
            crate::program_status::Status::Idle
        });
        #[cfg(not(feature = "native"))]
        let _ = failed;
        emit(if opts.color {
            "\x1b[1mSoCLI>\x1b[0m "
        } else {
            "SoCLI> "
        });
        let mut line = String::new();
        let n = stdin.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            #[cfg(feature = "native")]
            crate::program_status::cancel();
            emit_line("");
            return Ok(());
        }
        let line = line.trim();
        failed = false;
        #[cfg(feature = "native")]
        crate::program_status::acknowledge();
        if line.is_empty() {
            continue;
        }
        #[cfg(feature = "native")]
        crate::program_status::set(crate::program_status::Status::Working(
            crate::program_status::Activity::Command,
        ));
        if let Some(tab) = merged_tab(state, line) {
            match state.open_merged_tab(tab, clients) {
                Ok(()) => print_state(state, opts)?,
                Err(err) => {
                    failed = true;
                    emit_line(&util::clean(&err));
                }
            }
            continue;
        }
        let cmd = match command::parse(line) {
            Ok(cmd) => cmd,
            Err(err) => {
                failed = true;
                emit_line(&util::clean(&err));
                continue;
            }
        };
        match dispatch(state, clients, cmd, opts) {
            Ok(true) => {
                #[cfg(feature = "native")]
                crate::program_status::cancel();
                return Ok(());
            }
            Ok(false) => {}
            Err(err) => {
                failed = true;
                emit_line(&util::clean(&err));
            }
        }
    }
}

/// `home`, `discover` or `saved` in the All view open its tab; other tab words need a network first.
fn merged_tab(state: &State, line: &str) -> Option<MergedTab> {
    if state.scope_net().is_some() {
        return None;
    }
    let word = line.to_ascii_lowercase();
    MergedTab::ALL.into_iter().find(|tab| tab.word() == word)
}

/// Returns true when the prompt should exit.
fn dispatch(
    state: &mut State,
    clients: &mut Clients,
    cmd: Cmd,
    opts: &PrintOpts,
) -> Result<bool, String> {
    if matches!(cmd, Cmd::Tab(_)) && state.scope_net().is_none() {
        emit_line(state::PICK_NETWORK_FIRST);
        return Ok(false);
    }
    let redraw = matches!(
        cmd,
        Cmd::Refresh
            | Cmd::More
            | Cmd::Section(_)
            | Cmd::Tab(_)
            | Cmd::Scope(_)
            | Cmd::Search(_)
            | Cmd::User(_)
            | Cmd::Thread(_)
            | Cmd::Expand
            | Cmd::Back
            | Cmd::SelectDelta(_)
            | Cmd::SelectFirst
            | Cmd::SelectLast
            | Cmd::Filter(_)
    );
    match state.apply(cmd, clients)? {
        Outcome::Quit => Ok(true),
        Outcome::Help => {
            emit(command::HELP);
            Ok(false)
        }
        Outcome::Json(value) => {
            emit_line(&pretty(&value)?);
            Ok(false)
        }
        Outcome::Open(url) => {
            util::open_browser(&url)?;
            emit_line(&util::clean(&url));
            Ok(false)
        }
        Outcome::Share { url, profile } => {
            let what = if profile {
                "copied profile link"
            } else {
                "copied link"
            };
            match clip::copy(&url) {
                Ok(via) => {
                    let suffix = match via {
                        clip::Via::Terminal => " (terminal clipboard)",
                        clip::Via::System => "",
                    };
                    emit_line(&util::clean(&format!("{what}  {url}{suffix}")));
                    Ok(false)
                }
                Err(err) => {
                    emit_line(&util::clean(&url));
                    Err(err)
                }
            }
        }
        Outcome::View(urls) => {
            for url in urls {
                let path = media::materialize(&url, Origin::Post)?;
                let kind = media::show_with(&state.viewer, &path, 60, false)?;
                emit_line(kind.label());
            }
            Ok(false)
        }
        Outcome::None if redraw => {
            print_state(state, opts)?;
            Ok(false)
        }
        Outcome::None => {
            if !state.status.is_empty() {
                emit_line(&util::clean(&state.status));
            }
            Ok(false)
        }
    }
}

fn pretty(value: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| e.to_string())
}

fn paint_dim(text: &str, color: bool) -> String {
    if color {
        format!("\x1b[38;2;140;140;140m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::fixtures;
    use crate::net::Network;

    #[test]
    fn merged_tab_words_need_the_all_view() {
        let all = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        assert_eq!(merged_tab(&all, "Discover"), Some(MergedTab::Discover));
        assert_eq!(merged_tab(&all, "saved"), Some(MergedTab::Saved));
        assert_eq!(merged_tab(&all, "home"), Some(MergedTab::Home));
        assert_eq!(merged_tab(&all, "following"), None);
        let one = State::boot(
            true,
            15,
            &Config::default(),
            &Clients::empty(),
            Some(&[Network::X]),
        );
        assert_eq!(merged_tab(&one, "discover"), None);
    }

    #[test]
    fn links_wrap_only_the_columns_of_their_region() {
        let line = StyledLine {
            spans: vec![
                Span {
                    kind: Kind::Fg,
                    text: "see ".to_string(),
                },
                Span {
                    kind: Kind::Dim,
                    text: "@ada now".to_string(),
                },
            ],
            ..StyledLine::blank()
        };
        let region = Region {
            x: 4,
            y: 0,
            width: 4,
            url: "https://x.com/ada".to_string(),
        };
        let out = linked(&line, &[&region]);
        let texts: Vec<&str> = out.spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(texts[0], "see ");
        assert_eq!(texts[1], hyperlink::wrap("https://x.com/ada", "@ada"));
        assert_eq!(texts[2], " now");
        assert!(matches!(out.spans[1].kind, Kind::Dim));
    }

    /// The demo state with the `controls()` posts as its current feed.
    fn with_controls() -> State {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let posts = fixtures::controls();
        let feed = state.current_feed_id().unwrap();
        state.feeds.get_mut(&feed).unwrap().ids = posts.iter().map(|p| p.id.clone()).collect();
        for post in posts {
            state.posts.insert(post.id.clone(), post);
        }
        state
    }

    #[test]
    fn printed_post_text_carries_only_the_app_escapes() {
        let state = with_controls();
        let opts = PrintOpts {
            color: false,
            json: false,
            images: false,
            prompt: false,
            width: 100,
        };
        let text = |links: bool| {
            let (feed, footer) = printed(&state, &opts, links);
            let rows = feed.len() + footer.len();
            let text = [feed, footer].concat().join("\n");
            assert_eq!(text.split('\n').count(), rows, "a row holds a line break");
            text
        };
        let plain = text(false);
        for word in ["red", "click", "abc"] {
            assert!(plain.contains(word), "{word} in {plain}");
        }
        assert!(!plain.contains(['\x1b', '\x07', '\r']), "{plain:?}");
        let linked = text(true);
        assert!(linked.contains("\x1b]8;"));
        for (at, _) in linked.match_indices('\x1b') {
            let rest = &linked[at..];
            assert!(
                rest.starts_with("\x1b]8;") || rest.starts_with("\x1b\\"),
                "{rest:?}"
            );
        }
        let json = serde_json::to_string(&state.feed_json()).unwrap();
        assert!(json.contains("\\u001b[31mred"), "{json}");
    }
}
