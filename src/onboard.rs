//! Signed-out home with selection-driven water ripples and character-raster network marks.

#[cfg(feature = "native")]
use rand::Rng;
use unicode_width::UnicodeWidthStr;

use crate::net::Network;
use crate::render::{Act, Canvas, Kind};
use crate::screens::Body;
use crate::state::State;
use crate::theme::Detail;
use crate::util;

/// X, Bluesky, dev/null, Threads, Mastodon, then the local demo.
pub const CHOICES: usize = Network::ALL.len() + 1;

const RAMP: &[u8] = b" .,:;ox%#@";
const LEAD: &str = "Follow the conversation.";
const ABOUT: &str = "Read, reply and post across your networks.";

/// Home, with no account and nothing loaded. Settings, Search, a thread and the demo are not it.
pub fn showing(state: &State) -> bool {
    !state.demo
        && !state.settings
        && state.section == crate::model::Section::Home
        && state.focus.is_none()
        && state.thread_focus.is_none()
        && state.profile().is_none()
        && state.available().is_empty()
        && state.current_posts().is_empty()
}

#[derive(Default)]
pub struct Scene {
    selected: Option<usize>,
    drops: Vec<u32>,
    next_glitch: Option<u32>,
    glitch_started: Option<u32>,
    glitch_seed: u32,
}

fn glitch_gap() -> u32 {
    #[cfg(feature = "native")]
    {
        rand::thread_rng().gen_range(60..=240)
    }
    #[cfg(not(feature = "native"))]
    120
}

fn glitch_seed(frame: u32) -> u32 {
    #[cfg(feature = "native")]
    {
        let _ = frame;
        rand::thread_rng().gen()
    }
    #[cfg(not(feature = "native"))]
    frame.wrapping_mul(17).wrapping_add(1)
}

impl Scene {
    fn select(&mut self, choice: usize, frame: u32) {
        self.drops.retain(|born| frame.saturating_sub(*born) < 240);
        if self.selected != Some(choice) {
            self.selected = Some(choice);
            self.drops.push(frame);
            if self.drops.len() > 8 {
                self.drops.remove(0);
            }
        }
    }
    fn glitch(&mut self, frame: u32) -> Option<(u32, u32)> {
        let next = *self
            .next_glitch
            .get_or_insert_with(|| frame.saturating_add(glitch_gap()));
        if frame >= next {
            self.glitch_started = Some(frame);
            self.glitch_seed = glitch_seed(frame);
            self.next_glitch = Some(frame.saturating_add(glitch_gap()));
        }
        self.glitch_started.and_then(|started| {
            let age = frame.saturating_sub(started);
            (age < 7).then_some((age, self.glitch_seed))
        })
    }
}

#[cfg(test)]
fn body(state: &State, width: usize, height: usize, frame: u32) -> Body {
    let mut scene = Scene::default();
    scene.select(state.gate_row % CHOICES, 0);
    animated_body(state, width, height, frame, &mut scene)
}

pub fn animated_body(
    state: &State,
    width: usize,
    height: usize,
    frame: u32,
    scene: &mut Scene,
) -> Body {
    let width = width.max(1);
    let height = height.max(1);
    let ascii = state.detail == Detail::Plain || state.theme.ascii;
    let cursor = state.gate_row % CHOICES;
    scene.select(cursor, frame);
    let plan = layout(width, height);
    let glitch = if plan.art.is_some() {
        scene.glitch(frame)
    } else {
        None
    };
    let mut canvas = Canvas::new(width);
    for _ in 0..height {
        canvas.push(false);
    }
    if let Some(art) = plan.art {
        paint_water(
            &mut canvas,
            (width, height),
            art,
            frame,
            cursor,
            scene,
            glitch,
        );
    }
    paint_copy(&mut canvas, &plan, cursor, ascii);
    let crumb_x = usize::from(width >= 28) * 2;
    put(
        &mut canvas,
        0,
        crumb_x.saturating_sub(1),
        Kind::Fg,
        "                ",
    );
    put(&mut canvas, 0, crumb_x, Kind::Bold, "socli");
    put(&mut canvas, 0, crumb_x + 5, Kind::Dim, " > login ");
    let (lines, _, targets) = canvas.finish_targeted();
    let mut body = Body::text(lines);
    body.targets = targets;
    body
}

struct Place {
    art: Option<Art>,
    copy_x: usize,
    copy_y: usize,
    copy_w: usize,
    /// Room under the lead, so a short terminal keeps the choices and drops the rest.
    room: usize,
}

#[derive(Clone, Copy)]
struct Art {
    x: usize,
    y: usize,
    cols: usize,
    rows: usize,
}

pub fn effects_visible(width: usize, height: usize) -> bool {
    width >= 80 && height >= 24
}

fn layout(width: usize, height: usize) -> Place {
    let inset = usize::from(width >= 28) * 2;
    let copy_w = width.saturating_sub(inset * 2).clamp(1, 44);
    let available = height.saturating_sub(2);
    let copy_h = copy_height(copy_w, available);
    let art = if effects_visible(width, height) {
        let x = inset + copy_w + 4;
        Some(Art {
            x,
            y: 2,
            cols: width.saturating_sub(x + inset),
            rows: available,
        })
    } else {
        None
    };
    let copy_y = 2 + available.saturating_sub(copy_h) / 2;
    Place {
        art,
        copy_x: inset,
        copy_y,
        copy_w,
        room: height.saturating_sub(copy_y),
    }
}

/// Name, lead and the choices, plus the about, the rule and the hint when `room` allows.
fn copy_height(width: usize, room: usize) -> usize {
    script(width, room, false).len()
}

enum Row {
    Gap,
    Text(Kind, String),
    Brand(usize),
    Choice(usize),
}

/// What the copy column draws, clipped to `room` rows. The choices stay; the about goes first.
fn script(width: usize, room: usize, ascii: bool) -> Vec<Row> {
    let width = width.max(1);
    let mut head = if room < 10 {
        Vec::new()
    } else if !ascii && room >= 20 && width >= crate::theme::so_width() {
        crate::theme::so_mark()
            .iter()
            .enumerate()
            .map(|(index, _)| Row::Brand(index))
            .collect::<Vec<_>>()
    } else {
        vec![Row::Text(Kind::Bold, "So_".into())]
    };
    if room >= 10 {
        head.push(Row::Gap);
        head.extend(lines_of(Kind::Bold, LEAD, width));
    }
    let tail: Vec<Row> = std::iter::once(CHOICES - 1)
        .chain(0..CHOICES - 1)
        .map(Row::Choice)
        .collect();
    let mut optional = Vec::new();
    /*
    A blank row between the lead and the about is the web cell: 17px, the
    space under the first line. A native cell is the whole row, so that blank
    row opens a hole between the two lines.
    */
    #[cfg(not(feature = "native"))]
    optional.push(Row::Gap);
    optional.extend(lines_of(Kind::Dim, ABOUT, width));
    optional.push(Row::Gap);
    let rule = if ascii { '-' } else { '─' };
    optional.push(Row::Text(
        Kind::Line,
        rule.to_string().repeat(width.clamp(1, 28)),
    ));
    optional.push(Row::Gap);
    let hint = util::truncate("↑↓ choose   enter connect", width);
    let hint = if ascii {
        hint.replace("↑↓", "jk")
    } else {
        hint
    };
    let hint = if hint.is_empty() {
        None
    } else {
        Some(Row::Text(Kind::Dim, hint))
    };
    while head.len() + optional.len() + tail.len() + usize::from(hint.is_some()) > room
        && !optional.is_empty()
    {
        optional.pop();
    }
    let mut rows = head;
    rows.extend(optional);
    rows.extend(tail);
    if let Some(hint) = hint {
        if rows.len() + 2 <= room {
            rows.push(Row::Gap);
            rows.push(hint);
        }
    }
    if rows.len() > room {
        rows.truncate(room);
    }
    rows
}

fn lines_of(kind: Kind, text: &str, width: usize) -> Vec<Row> {
    util::wrap_text(text, width.max(1))
        .into_iter()
        .map(|line| Row::Text(kind, line))
        .collect()
}

pub fn step(choice: usize, forward: bool) -> usize {
    let visual = (choice + 1) % CHOICES;
    let next = if forward {
        (visual + 1) % CHOICES
    } else {
        (visual + CHOICES - 1) % CHOICES
    };
    (next + CHOICES - 1) % CHOICES
}

const X: &[&str] = &[
    "##             ##",
    " ###          ## ",
    "  ###        ##  ",
    "   ###      ##   ",
    "    ###    ##    ",
    "     ###  ##     ",
    "      #####      ",
    "      #####      ",
    "     ##  ###     ",
    "    ##    ###    ",
    "   ##      ###   ",
    "  ##        ###  ",
    " ##          ### ",
    "##            ###",
];
const BLUESKY: &[&str] = &[
    "##             ##",
    "####         ####",
    "######     ######",
    "#######   #######",
    " ####### ####### ",
    "  #############  ",
    "    #########    ",
    "  #############  ",
    " ############### ",
    " ####### ####### ",
    "  #####   #####  ",
    "   ###     ###   ",
];
const DEVNULL: &[&str] = &[
    "           ###   ",
    "          ###    ",
    "         ###     ",
    "        ###      ",
    "       ###       ",
    "      ###        ",
    "     ###         ",
    "    ###          ",
    "   ###           ",
    "  ###     #######",
    " ###      #######",
];
const THREADS: &[&str] = &[
    "     #######     ",
    "   ###     ###   ",
    "  ##         ##  ",
    " ##    ####   ## ",
    " ##   ##  ##  ## ",
    " ##       ##  ## ",
    " ##   ########## ",
    " ##  ##   ##  ## ",
    " ##  ##   ##  ## ",
    " ##   #####  ##  ",
    "  ##        ##   ",
    "   ###    ###    ",
    "     ######      ",
];
const MASTODON: &[&str] = &[
    "   ###########   ",
    " ################",
    "#################",
    "###   ##   ## ###",
    "### # ## # ## ###",
    "### # ## # ## ###",
    "### # ## # ## ###",
    "### ######### ###",
    "###           ###",
    " ############### ",
    " ####            ",
    "  ###########    ",
    "    #########    ",
];
const SO: &[&str] = &[
    " ####            ",
    "##  ##           ",
    "##      ####     ",
    " ####  ##  ##    ",
    "    ## ##  ##    ",
    "##  ## ##  ##    ",
    " ####   ####  ###",
];

fn logo(choice: usize) -> &'static [&'static str] {
    match Network::ALL.get(choice) {
        Some(Network::X) => X,
        Some(Network::Bluesky) => BLUESKY,
        Some(Network::Devnull) => DEVNULL,
        Some(Network::Threads) => THREADS,
        Some(Network::Mastodon) => MASTODON,
        None => SO,
    }
}

fn paint_water(
    canvas: &mut Canvas,
    size: (usize, usize),
    art: Art,
    frame: u32,
    choice: usize,
    scene: &Scene,
    glitch: Option<(u32, u32)>,
) {
    let (width, height) = size;
    let speed = ((width as f64).hypot(height as f64 * 2.0) / 4.0).max(19.0);
    let mark = logo(choice);
    let accent = Network::ALL
        .get(choice)
        .map_or(Kind::Blue, |net| net.accent());
    let scale = (art.cols as f64 / 23.0)
        .min(art.rows as f64 * 2.0 / (mark.len() + 6) as f64)
        .max(0.35);
    let center = (
        art.x as f64 + art.cols as f64 * 0.5,
        art.y as f64 + art.rows as f64 * 0.5,
    );
    for row in 0..height {
        for col in 0..width {
            let x = col as f64 - center.0;
            let y = (row as f64 - center.1) * 2.0;
            let mut water = 0.0;
            for born in &scene.drops {
                let age = f64::from(frame.saturating_sub(*born)) / 30.0;
                let radius = x.hypot(y);
                let front = radius - age * speed;
                let envelope = (-front.powi(2) / 100.0).exp() * (-age * 0.22).exp();
                water += (front * 0.9).sin() * envelope;
            }
            let intensity = water.abs().min(1.0);
            if intensity > 0.075 {
                let glyph = RAMP[(intensity * 9.0).ceil().min(9.0) as usize] as char;
                put_char(
                    canvas,
                    row,
                    col,
                    if intensity > 0.65 { accent } else { Kind::Dim },
                    glyph,
                );
            }
            let glitch = glitch.filter(|(age, seed)| {
                let band = (row + *seed as usize / 17) % 7;
                band < 2 + *age as usize % 3
            });
            let shift = glitch.map_or(0.0, |(age, seed)| {
                if (age + seed % 2).is_multiple_of(2) {
                    2.0
                } else {
                    -2.0
                }
            });
            let mx = ((x + shift) / scale + 8.5).floor() as isize;
            let my = (y / scale + mark.len() as f64 * 0.5).floor() as isize;
            if mx >= 0
                && my >= 0
                && mark
                    .get(my as usize)
                    .and_then(|r| r.as_bytes().get(mx as usize))
                    == Some(&b'#')
            {
                let hash = col * 13 + row * 29;
                let glyph = if let Some((age, seed)) = glitch {
                    b"/<>_:%#"[hash.wrapping_add(age as usize).wrapping_add(seed as usize) % 7]
                        as char
                } else {
                    b"ox%#@"[hash % 5] as char
                };
                put_char(
                    canvas,
                    row,
                    col,
                    if glitch.is_some() { Kind::Fg } else { accent },
                    glyph,
                );
            }
        }
    }
    let label = Network::ALL.get(choice).map_or("So_", |net| net.title());
    if art.rows > 4 && label.width() <= art.cols {
        put(
            canvas,
            art.y + art.rows - 1,
            art.x + (art.cols - label.width()) / 2,
            Kind::Dim,
            label,
        );
    }
}

fn paint_copy(canvas: &mut Canvas, plan: &Place, cursor: usize, ascii: bool) {
    let rows = script(plan.copy_w, plan.room, ascii);
    // The web plate is opaque. A wave mark left in a gap reads as a speck on the two lines.
    let plate = " ".repeat(plan.copy_w.saturating_add(2));
    let plate_x = plan.copy_x.saturating_sub(1);
    for (i, row) in rows.into_iter().enumerate() {
        let y = plan.copy_y + i;
        if matches!(row, Row::Gap | Row::Text(..)) {
            put(canvas, y, plate_x, Kind::Fg, &plate);
        }
        match row {
            Row::Gap => {}
            Row::Brand(index) => {
                let row = &crate::theme::so_mark()[index];
                put(
                    canvas,
                    y,
                    plan.copy_x.saturating_sub(1),
                    Kind::Fg,
                    &" ".repeat(row.glyphs.width() + 2),
                );
                crate::screens::put_so(canvas, y, plan.copy_x, row);
            }
            Row::Text(kind, text) => {
                put(canvas, y, plan.copy_x, kind, &text);
            }
            Row::Choice(index) => {
                choice(
                    canvas,
                    y,
                    plan.copy_x,
                    plan.copy_w,
                    index,
                    index == cursor,
                    ascii,
                );
            }
        }
    }
}

fn choice(
    canvas: &mut Canvas,
    y: usize,
    x: usize,
    w: usize,
    index: usize,
    selected: bool,
    ascii: bool,
) {
    let (name, act, action) = if let Some(net) = Network::ALL.get(index) {
        (net.title(), Act::SignIn(*net), "sign in")
    } else {
        ("Explore the demo", Act::EnterDemo, "open")
    };
    // From column 2 the name shares the wordmark's edge and the mark sits in the gutter.
    let gutter = x >= 2;
    let origin = if gutter { x - 2 } else { x };
    let name_at = if gutter { x } else { x + 2 };
    let span = if gutter { w + 2 } else { w };
    put(
        canvas,
        y,
        origin,
        Kind::Fg,
        &" ".repeat((name.width() + 5).min(span)),
    );
    if selected {
        put(canvas, y, origin, Kind::Fg, &" ".repeat(span));
        canvas.highlight(y, origin, origin + span);
    }
    let marker = if !selected {
        " "
    } else if ascii {
        ">"
    } else {
        "▸"
    };
    let mark_kind = if selected { Kind::Blue } else { Kind::Dim };
    let name_kind = match (selected, &act) {
        (true, Act::SignIn(net)) => net.accent(),
        (true, _) => Kind::Bold,
        _ => Kind::Fg,
    };
    put(canvas, y, origin, mark_kind, marker);
    let room = if gutter {
        w.saturating_sub(1)
    } else {
        w.saturating_sub(3)
    };
    let col = put(
        canvas,
        y,
        name_at,
        name_kind,
        &format!("{} ", util::truncate(name, room)),
    );
    if selected && origin + span >= col + 2 + action.width() {
        put(canvas, y, origin + span - action.width(), Kind::Dim, action);
    }
    canvas.target(y, origin, origin + span, act);
}

fn put(canvas: &mut Canvas, y: usize, x: usize, kind: Kind, text: &str) -> usize {
    canvas.put(y, x, kind, text)
}

fn put_char(canvas: &mut Canvas, y: usize, x: usize, kind: Kind, ch: char) {
    let mut buf = [0u8; 4];
    canvas.put(y, x, kind, ch.encode_utf8(&mut buf));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::net::Clients;
    use crate::render::StyledLine;
    use unicode_width::UnicodeWidthStr;

    fn text(body: &Body) -> String {
        body.lines
            .iter()
            .map(StyledLine::plain)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn empty() -> State {
        State::boot(false, 15, &Config::default(), &Clients::empty(), None)
    }

    #[test]
    fn the_water_moves_and_the_rows_sign_in() {
        let mut state = empty();
        state.detail = Detail::Standard;
        let first = body(&state, 100, 32, 0);
        let later = body(&state, 100, 32, 40);
        let a = text(&first);
        let b = text(&later);
        assert_ne!(a, b, "the water advances\n{a}");
        assert!(a.contains(LEAD), "{a}");
        assert!(a.contains(ABOUT), "{a}");
        assert!(a.contains('%') || a.contains('@'), "{a}");
        assert!(first
            .targets
            .iter()
            .any(|target| matches!(target.act, Act::SignIn(Network::Bluesky))));
        assert!(first
            .targets
            .iter()
            .any(|target| matches!(target.act, Act::EnterDemo)));
        assert!(a.lines().all(|line| line.width() <= 100));

        state.gate_row = 1;
        let picked = text(&body(&state, 100, 32, 0));
        let bluesky = picked
            .lines()
            .find(|line| line.contains("Bluesky"))
            .unwrap_or("");
        let mastodon = picked
            .lines()
            .find(|line| line.contains("Mastodon"))
            .unwrap_or("");
        assert!(bluesky.contains('▸'), "{bluesky}");
        assert!(!mastodon.contains('▸'), "{mastodon}");
        let column_of = |line: &str, needle: &str| line.find(needle).map(|at| line[..at].width());
        let follow = picked.lines().find(|line| line.contains(LEAD)).unwrap();
        assert_eq!(column_of(follow, LEAD), Some(2), "{follow}");
        assert_eq!(column_of(bluesky, "Bluesky"), Some(2), "{bluesky}");
        let mark = picked
            .lines()
            .find(|line| line.contains("▄██████▄"))
            .unwrap();
        assert_eq!(column_of(mark, "▄██████▄"), Some(2), "{mark}");

        state.detail = Detail::Plain;
        state.theme.ascii = true;
        let plain = text(&body(&state, 100, 32, 0));
        assert!(plain.contains(LEAD), "{plain}");
        assert!(plain.is_ascii(), "{plain}");
        assert!(!plain.contains('█') && !plain.contains('▀') && !plain.contains('▄'));

        let lines: Vec<&str> = a.lines().collect();
        let lead_at = lines.iter().position(|line| line.contains(LEAD)).unwrap();
        let about_at = lines.iter().position(|line| line.contains(ABOUT)).unwrap();
        assert_eq!(about_at, lead_at + 1, "the two lines sit together\n{a}");
        let plate: String = lines[lead_at - 1].chars().take(46).collect();
        assert!(
            plate.chars().all(|ch| ch == ' '),
            "the wave stays off the introduction\n{plate:?}"
        );

        let narrow = text(&body(&empty(), 40, 18, 0));
        assert!(narrow.contains(LEAD), "{narrow}");
        assert!(narrow.contains("Bluesky"), "{narrow}");
    }

    #[test]
    fn the_demo_and_a_signed_in_network_are_not_the_welcome() {
        let mut state = empty();
        assert!(showing(&state));
        state.demo = true;
        assert!(!showing(&state));
        let mut live = empty();
        live.nets[0].signed_in = true;
        assert!(!showing(&live));
    }
    #[test]
    fn ripples_settle_and_repeated_hover_does_not_restart_them() {
        let state = empty();
        let mut scene = Scene::default();
        animated_body(&state, 100, 32, 0, &mut scene);
        scene.next_glitch = Some(1000);
        let settled = text(&animated_body(&state, 100, 32, 250, &mut scene));
        let still = text(&animated_body(&state, 100, 32, 260, &mut scene));
        assert_eq!(settled, still);
        assert!(scene.drops.is_empty());
        let periods = settled.chars().filter(|ch| *ch == '.').count();
        assert_eq!(
            periods,
            LEAD.matches('.').count() + ABOUT.matches('.').count(),
            "a settled welcome keeps only the sentence periods\n{settled}"
        );
        let mut changed = state;
        changed.gate_row = 1;
        let ripple = text(&animated_body(&changed, 100, 32, 261, &mut scene));
        assert_ne!(still, ripple);
        animated_body(&changed, 100, 32, 262, &mut scene);
        assert_eq!(scene.drops.len(), 1);
    }
    #[test]
    fn small_terminals_hide_all_effects_and_keep_every_choice() {
        for (width, height) in [(24, 8), (32, 10), (40, 15), (79, 40), (100, 23)] {
            assert!(!effects_visible(width, height));
            assert!(layout(width, height).art.is_none());
            let mut scene = Scene::default();
            let rendered = animated_body(&empty(), width, height, 0, &mut scene);
            assert_eq!(rendered.targets.len(), CHOICES, "{width}x{height}");
            assert!(rendered
                .targets
                .iter()
                .all(|target| usize::from(target.x + target.width) <= width
                    && usize::from(target.y) < height));
            assert!(scene.next_glitch.is_none());
        }
    }

    #[test]
    fn the_wave_reaches_both_window_edges_and_the_logo_is_on_the_right() {
        let state = empty();
        let plan = layout(100, 32);
        let art = plan.art.unwrap();
        assert!(art.x > plan.copy_x + plan.copy_w);
        let mut scene = Scene::default();
        let mut left = false;
        let mut right = false;
        for frame in 0..160 {
            let rendered = text(&animated_body(&state, 100, 32, frame, &mut scene));
            for row in rendered.lines().skip(1) {
                let chars = row.chars().collect::<Vec<_>>();
                left |= chars
                    .first()
                    .is_some_and(|ch| RAMP[2..].contains(&(*ch as u8)));
                right |= chars
                    .get(99)
                    .is_some_and(|ch| RAMP[2..].contains(&(*ch as u8)));
            }
        }
        assert!(left && right, "the wave must not stop at a column boundary");
    }

    #[test]
    fn glitches_are_brief_and_schedule_another_random_two_to_eight_second_gap() {
        let mut scene = Scene {
            next_glitch: Some(60),
            ..Scene::default()
        };
        assert!(scene.glitch(59).is_none());
        assert!(scene.glitch(60).is_some());
        assert!((60..=240).contains(&(scene.next_glitch.unwrap() - 60)));
        assert!(scene.glitch(66).is_some());
        assert!(scene.glitch(67).is_none());
    }
}
