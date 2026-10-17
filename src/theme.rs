//! Lights-out and light palettes, mono, network colors, the appearance settings, and the glyphs.

use crate::net::Network;
use crate::palettes::NAMED;
use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Fg,
    Dim,
    Bold,
    Blue,
    Pink,
    Green,
    Red,
    Amber,
    Rose,
    Violet,
    Sage,
    Coral,
    Teal,
    Gold,
    Sky,
    Line,
    /// Mentions and links: blue, or underlined in mono.
    Link,
    /// The color a network is drawn in, `[<net>] color`.
    Net(Network),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`, case-insensitive.
    pub fn parse(value: &str) -> Option<Self> {
        let hex = value.strip_prefix('#')?;
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Rgb,
    pub surface: Rgb,
    pub line: Rgb,
    pub fg: Rgb,
    pub dim: Rgb,
    pub blue: Rgb,
    pub pink: Rgb,
    pub green: Rgb,
    pub red: Rgb,
    pub amber: Rgb,
    pub rose: Rgb,
    pub violet: Rgb,
    pub sage: Rgb,
    pub coral: Rgb,
    pub teal: Rgb,
    pub gold: Rgb,
    pub sky: Rgb,
    pub indigo: Rgb,
}

impl Palette {
    pub fn get(self_dark: bool) -> Self {
        if self_dark {
            Self {
                bg: Rgb(0, 0, 0),
                surface: Rgb(22, 24, 28),
                line: Rgb(47, 51, 54),
                fg: Rgb(231, 233, 234),
                dim: Rgb(113, 118, 123),
                blue: Rgb(29, 155, 240),
                pink: Rgb(249, 24, 128),
                green: Rgb(0, 186, 124),
                red: Rgb(244, 33, 46),
                amber: Rgb(208, 138, 60),
                rose: Rgb(229, 104, 154),
                violet: Rgb(165, 127, 224),
                sage: Rgb(125, 184, 107),
                coral: Rgb(238, 110, 78),
                teal: Rgb(60, 184, 166),
                gold: Rgb(214, 179, 74),
                sky: Rgb(106, 168, 232),
                indigo: Rgb(132, 128, 255),
            }
        } else {
            Self {
                bg: Rgb(255, 255, 255),
                surface: Rgb(242, 245, 246),
                line: Rgb(217, 225, 229),
                fg: Rgb(15, 20, 25),
                dim: Rgb(83, 100, 113),
                blue: Rgb(26, 140, 216),
                pink: Rgb(224, 23, 111),
                green: Rgb(0, 163, 110),
                red: Rgb(217, 28, 41),
                amber: Rgb(179, 106, 28),
                rose: Rgb(204, 74, 127),
                violet: Rgb(126, 82, 199),
                sage: Rgb(76, 154, 58),
                coral: Rgb(210, 76, 43),
                teal: Rgb(19, 145, 127),
                gold: Rgb(168, 132, 26),
                sky: Rgb(58, 130, 204),
                indigo: Rgb(86, 58, 204),
            }
        }
    }

    /// Black and white only, from design/src/term.css `.mono`.
    fn mono() -> Self {
        let fg = Rgb(230, 230, 230);
        Self {
            bg: Rgb(0, 0, 0),
            surface: fg,
            line: Rgb(128, 128, 128),
            fg,
            dim: Rgb(138, 138, 138),
            blue: fg,
            pink: fg,
            green: fg,
            red: fg,
            amber: fg,
            rose: fg,
            violet: fg,
            sage: fg,
            coral: fg,
            teal: fg,
            gold: fg,
            sky: fg,
            indigo: fg,
        }
    }

    pub fn kind(&self, kind: Kind) -> Rgb {
        match kind {
            Kind::Fg | Kind::Bold => self.fg,
            Kind::Dim => self.dim,
            Kind::Blue | Kind::Link => self.blue,
            Kind::Pink => self.pink,
            Kind::Green => self.green,
            Kind::Red => self.red,
            Kind::Amber => self.amber,
            Kind::Rose => self.rose,
            Kind::Violet => self.violet,
            Kind::Sage => self.sage,
            Kind::Coral => self.coral,
            Kind::Teal => self.teal,
            Kind::Gold => self.gold,
            Kind::Sky => self.sky,
            Kind::Line => self.line,
            Kind::Net(_) => self.fg,
        }
    }

    fn slot(&mut self, key: &str) -> Option<&mut Rgb> {
        Some(match key {
            "fg" => &mut self.fg,
            "dim" => &mut self.dim,
            "line" => &mut self.line,
            "surface" => &mut self.surface,
            "blue" => &mut self.blue,
            "sky" => &mut self.sky,
            "teal" => &mut self.teal,
            "green" => &mut self.green,
            "sage" => &mut self.sage,
            "gold" => &mut self.gold,
            "amber" => &mut self.amber,
            "coral" => &mut self.coral,
            "rose" => &mut self.rose,
            "pink" => &mut self.pink,
            "red" => &mut self.red,
            "violet" => &mut self.violet,
            "indigo" => &mut self.indigo,
            _ => return None,
        })
    }

    /// A network color name; `default` is the foreground.
    fn named(&self, name: &str) -> Option<Rgb> {
        Some(match name {
            "default" => self.fg,
            "blue" => self.blue,
            "sky" => self.sky,
            "teal" => self.teal,
            "green" => self.green,
            "sage" => self.sage,
            "gold" => self.gold,
            "amber" => self.amber,
            "coral" => self.coral,
            "rose" => self.rose,
            "pink" => self.pink,
            "red" => self.red,
            "violet" => self.violet,
            "indigo" => self.indigo,
            _ => return None,
        })
    }
}

/// A palette entry's value under a scheme, before `[colors]`; `terminal` for the native scheme.
pub fn palette_value(name: &str, scheme: Scheme) -> Option<String> {
    if !PALETTE_KEYS.contains(&name) {
        return None;
    }
    Some(match scheme.resolve(true).0 {
        Some(mut palette) => palette.slot(name)?.hex(),
        None => "terminal".to_string(),
    })
}

// MARK: scheme

/// Which palette of a named theme to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Follow,
    Dark,
    Light,
}

/// A theme choice: `Named` indexes `palettes::NAMED`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Auto,
    Native,
    Black,
    White,
    Named(usize, Variant),
}

impl Scheme {
    /// Auto, Native, Black, White, then every named theme in palette order.
    pub fn list() -> Vec<Scheme> {
        let named = (0..NAMED.len()).map(|i| Scheme::Named(i, Variant::Follow));
        [Scheme::Auto, Scheme::Native, Scheme::Black, Scheme::White]
            .into_iter()
            .chain(named)
            .collect()
    }

    pub fn parse(value: &str) -> Option<Scheme> {
        let value = value.trim().to_ascii_lowercase();
        match value.as_str() {
            "auto" => return Some(Scheme::Auto),
            "native" => return Some(Scheme::Native),
            "black" | "dark" => return Some(Scheme::Black),
            "white" | "light" => return Some(Scheme::White),
            _ => {}
        }
        let find = |name: &str| NAMED.iter().position(|n| n.name == name);
        if let Some(i) = find(&value) {
            return Some(Scheme::Named(i, Variant::Follow));
        }
        if let Some(i) = value.strip_suffix("-dark").and_then(find) {
            return Some(Scheme::Named(i, Variant::Dark));
        }
        let i = value.strip_suffix("-light").and_then(find)?;
        NAMED[i]
            .light
            .is_some()
            .then_some(Scheme::Named(i, Variant::Light))
    }

    /// The canonical form, which `parse` reads back.
    pub fn word(self) -> String {
        match self {
            Scheme::Auto => "auto".into(),
            Scheme::Native => "native".into(),
            Scheme::Black => "black".into(),
            Scheme::White => "white".into(),
            Scheme::Named(i, Variant::Follow) => NAMED[i].name.into(),
            Scheme::Named(i, Variant::Dark) => format!("{}-dark", NAMED[i].name),
            Scheme::Named(i, Variant::Light) => format!("{}-light", NAMED[i].name),
        }
    }

    /// The scheme without a forced variant: the row it takes in `list()`.
    pub fn base(self) -> Scheme {
        match self {
            Scheme::Named(i, _) => Scheme::Named(i, Variant::Follow),
            other => other,
        }
    }

    pub fn step(self, dir: i32) -> Scheme {
        let list = Scheme::list();
        let at = list.iter().position(|s| *s == self.base()).unwrap_or(0) as i32;
        list[(at + dir).rem_euclid(list.len() as i32) as usize]
    }

    /// Every word, comma-separated, for errors.
    pub fn words() -> String {
        let words: Vec<String> = Scheme::list().into_iter().map(Scheme::word).collect();
        words.join(", ")
    }

    pub fn describe(self) -> &'static str {
        match self {
            Scheme::Auto => "black or white, following the terminal",
            Scheme::Native => "the terminal's own colors and background",
            Scheme::Black => "light text on black",
            Scheme::White => "dark text on white",
            Scheme::Named(i, _) if NAMED[i].light.is_some() => "dark and light",
            Scheme::Named(..) => "dark",
        }
    }

    pub fn variants(self) -> &'static [&'static str] {
        match self {
            Scheme::Named(i, _) if NAMED[i].light.is_some() => &["dark", "light"],
            Scheme::Named(..) => &["dark"],
            _ => &[],
        }
    }

    /// The palette (none for native) and whether it is dark, on a terminal that is or is not dark.
    pub fn resolve(self, dark_terminal: bool) -> (Option<Palette>, bool) {
        let t = dark_terminal;
        match self {
            Scheme::Auto => (Some(Palette::get(t)), t),
            Scheme::Native => (None, t),
            Scheme::Black => (Some(Palette::get(true)), true),
            Scheme::White => (Some(Palette::get(false)), false),
            Scheme::Named(i, variant) => {
                let named = &NAMED[i];
                let light = match variant {
                    Variant::Follow if !t => named.light,
                    Variant::Light => named.light,
                    _ => None,
                };
                match light {
                    Some(palette) => (Some(palette), false),
                    None => (Some(named.dark), true),
                }
            }
        }
    }
}

// MARK: colors

/// The names a network color accepts besides `#rrggbb`, in the order Settings cycles them.
pub const COLOR_NAMES: [&str; 14] = [
    "default", "blue", "sky", "teal", "green", "sage", "gold", "amber", "coral", "rose", "pink",
    "red", "violet", "indigo",
];

/// The palette entries `[colors]` can override.
pub const PALETTE_KEYS: [&str; 17] = [
    "fg", "dim", "line", "surface", "blue", "sky", "teal", "green", "sage", "gold", "amber",
    "coral", "rose", "pink", "red", "violet", "indigo",
];

/// The default color of each network.
pub fn default_net_color(net: Network) -> &'static str {
    match net {
        Network::X => "default",
        Network::Bluesky => "sky",
        Network::Devnull => "amber",
        Network::Threads => "violet",
        Network::Mastodon => "indigo",
    }
}

/// A palette name or `#rrggbb`, normalized to lowercase.
pub fn valid_color(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    (COLOR_NAMES.contains(&value.as_str()) || Rgb::parse(&value).is_some()).then_some(value)
}

/// The next or previous palette name; a hex value steps on from `default`.
pub fn step_color(value: &str, dir: i32) -> &'static str {
    let n = COLOR_NAMES.len() as i32;
    match COLOR_NAMES.iter().position(|name| *name == value) {
        Some(i) => COLOR_NAMES[(i as i32 + dir).rem_euclid(n) as usize],
        None if dir > 0 => COLOR_NAMES[1],
        None => COLOR_NAMES[0],
    }
}

/// The xterm-256 index closest to `rgb` by squared distance, over the color cube and the gray ramp.
pub fn nearest_256(rgb: Rgb) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let distance = |a: Rgb, b: Rgb| {
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
    };
    let mut best = (i32::MAX, 16u8);
    for (i, r) in LEVELS.iter().enumerate() {
        for (j, g) in LEVELS.iter().enumerate() {
            for (k, b) in LEVELS.iter().enumerate() {
                let d = distance(rgb, Rgb(*r, *g, *b));
                if d < best.0 {
                    best = (d, (16 + 36 * i + 6 * j + k) as u8);
                }
            }
        }
    }
    for i in 0..24u8 {
        let v = 8 + 10 * i;
        let d = distance(rgb, Rgb(v, v, v));
        if d < best.0 {
            best = (d, 232 + i);
        }
    }
    best.1
}

/**
True color when `SOCLI_TRUECOLOR` says so, when `COLORTERM` is `truecolor` or `24bit`, on iTerm2,
VS Code, Ghostty and WezTerm, or when `TERM` names a direct-color or Ghostty terminal.
*/
pub fn truecolor(env: impl Fn(&str) -> Option<String>) -> bool {
    match env("SOCLI_TRUECOLOR").as_deref().map(str::trim) {
        Some("1") => return true,
        Some("0") => return false,
        _ => {}
    }
    if env("COLORTERM").is_some_and(|v| matches!(v.trim(), "truecolor" | "24bit")) {
        return true;
    }
    let program = env("TERM_PROGRAM").unwrap_or_default();
    if ["iTerm.app", "vscode", "ghostty", "WezTerm"]
        .iter()
        .any(|p| program.eq_ignore_ascii_case(p))
    {
        return true;
    }
    env("TERM").is_some_and(|term| term.contains("direct") || term.contains("ghostty"))
}

/**
The detail level when `--detail`, `SOCLI_DETAIL` and `ui.detail` are all unset.
A normal terminal gets rich. An ESP client, or another low-resource terminal, gets plain.
*/
pub fn default_detail(env: impl Fn(&str) -> Option<String>) -> Detail {
    if constrained_client(&env) {
        Detail::Plain
    } else {
        Detail::Rich
    }
}

/// ESP boards, and terminfo names that mean a dumb serial session or a hardware VT.
fn constrained_client(env: &impl Fn(&str) -> Option<String>) -> bool {
    let term = normalized(env("TERM"));
    let program = normalized(env("TERM_PROGRAM"));
    esp_name(&term) || esp_name(&program) || low_resource_term(&term)
}

fn normalized(value: Option<String>) -> String {
    value.unwrap_or_default().trim().to_ascii_lowercase()
}

/**
An ESP client, matched on whole names so `WezTerm` and `xterm-256color` stay rich.
`esp-idf` splits into `esp`, and a suffix such as `esp32s3` still counts.
*/
fn esp_name(name: &str) -> bool {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .any(|part| {
            matches!(
                part,
                "esp" | "espterm" | "esp32" | "esp8266" | "espressif" | "espidf"
            ) || part.starts_with("esp32")
                || part.starts_with("esp8266")
                || part.starts_with("espterm")
        })
}

/// `vt102-w` and `ansi.sys` use the name before `-` or `.`.
fn low_resource_term(term: &str) -> bool {
    let head = term.split(['-', '.']).next().unwrap_or(term);
    matches!(
        head,
        "dumb" | "unknown" | "ansi" | "vt52" | "vt100" | "vt102" | "vt220" | "network"
    )
}

/// The background color in an OSC 11 reply such as `\x1b]11;rgb:0000/0000/0000\x07`.
pub fn osc11_rgb(reply: &str) -> Option<Rgb> {
    let start = reply.find("rgb:")? + 4;
    let body: String = reply[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '/')
        .collect();
    let parts: Vec<u8> = body
        .split('/')
        .map(|part| {
            let max = 16f64.powi(part.len() as i32) - 1.0;
            u32::from_str_radix(part, 16)
                .ok()
                .filter(|_| (1..=4).contains(&part.len()))
                .map(|v| (v as f64 / max * 255.0).round() as u8)
        })
        .collect::<Option<_>>()?;
    let [r, g, b] = parts.as_slice() else {
        return None;
    };
    Some(Rgb(*r, *g, *b))
}

/// Whether the background reads as dark, by luminance.
pub fn is_dark(Rgb(r, g, b): Rgb) -> bool {
    let channel = |v: u8| v as f64 / 255.0;
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b) < 0.5
}

/// The dark or light reading of an OSC 11 reply.
#[cfg(test)]
pub fn osc11_dark(reply: &str) -> Option<bool> {
    osc11_rgb(reply).map(is_dark)
}

/// `COLORFGBG` names the background last; indexes 0 to 6 and 8 are dark.
pub fn colorfgbg_dark(value: &str) -> Option<bool> {
    let bg: u8 = value.rsplit(';').next()?.trim().parse().ok()?;
    Some(bg <= 6 || bg == 8)
}

// MARK: theme

/// The palette in use, with the overrides, the network colors, mono and the color depth applied.
#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub mono: bool,
    /**
    NO_COLOR in the reader: the terminal's own colors, with Dim as the dim attribute. crossterm
    writes a color change as a reset under NO_COLOR, so no color is ever set.
    */
    pub plain: bool,
    /**
    The plain detail level: no color and no dim anywhere, `>` beside the selected post instead of
    a background, and the reader's own glyphs drawn in ASCII.
    */
    pub ascii: bool,
    pub truecolor: bool,
    /// The terminal's own colors and background: no color is set except ANSI accents and overrides.
    pub native: bool,
    /// The terminal background from OSC 11, when the terminal answered.
    pub term_bg: Option<Rgb>,
    base: Palette,
    overrides: Vec<(String, Rgb)>,
    nets: [String; 5],
}

#[derive(Clone, Copy)]
enum Paint {
    Default,
    Indexed(u8),
    Rgb(Rgb),
}

/// The palette role a kind draws from; a network color has none.
fn role(kind: Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::Fg | Kind::Bold => "fg",
        Kind::Dim => "dim",
        Kind::Line => "line",
        Kind::Blue | Kind::Link => "blue",
        Kind::Pink => "pink",
        Kind::Green => "green",
        Kind::Red => "red",
        Kind::Amber => "amber",
        Kind::Rose => "rose",
        Kind::Violet => "violet",
        Kind::Sage => "sage",
        Kind::Coral => "coral",
        Kind::Teal => "teal",
        Kind::Gold => "gold",
        Kind::Sky => "sky",
        Kind::Net(_) => return None,
    })
}

/// The ANSI color a palette accent takes in the native scheme.
fn ansi_index(name: &str) -> Option<u8> {
    Some(match name {
        "red" => 1,
        "green" => 2,
        "amber" => 3,
        "blue" => 4,
        "violet" => 5,
        "teal" => 6,
        "coral" => 9,
        "sage" => 10,
        "gold" => 11,
        "indigo" => 12,
        "pink" | "rose" => 13,
        "sky" => 14,
        _ => return None,
    })
}

fn mix(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let channel = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * amount).round() as u8;
    Rgb(
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            dark: true,
            mono: false,
            plain: false,
            ascii: false,
            truecolor: true,
            native: false,
            term_bg: None,
            base: Palette::get(true),
            overrides: Vec::new(),
            nets: Network::ALL.map(|net| default_net_color(net).to_string()),
        }
    }
}

impl Theme {
    /// The dark palette at the color depth this terminal reports.
    pub fn from_env() -> Self {
        Self {
            truecolor: truecolor(|key| std::env::var(key).ok()),
            ..Self::default()
        }
    }

    /**
    Applies `[colors]` and the network colors. A value that is not a color keeps its default and
    adds a notice naming the config key.
    */
    pub fn with_colors<'a>(
        mut self,
        overrides: impl IntoIterator<Item = (&'a str, &'a str)>,
        nets: impl IntoIterator<Item = (Network, Option<&'a str>)>,
        notices: &mut Vec<String>,
    ) -> Self {
        let mut notice = |value: &str, key: String| {
            notices.push(format!(
                "\"{value}\" is not a color for {key}. Use a palette name or #rrggbb"
            ));
        };
        self.overrides.clear();
        for (name, value) in overrides {
            match Rgb::parse(value.trim()).filter(|_| PALETTE_KEYS.contains(&name)) {
                Some(rgb) => self.overrides.push((name.to_string(), rgb)),
                None => notice(value, format!("colors.{name}")),
            }
        }
        for (net, value) in nets {
            let color = match value {
                None => default_net_color(net).to_string(),
                Some(value) => valid_color(value).unwrap_or_else(|| {
                    notice(value, format!("{}.color", net.slug()));
                    default_net_color(net).to_string()
                }),
            };
            self.nets[net_index(net)] = color;
        }
        self
    }

    pub fn net_color(&self, net: Network) -> &str {
        &self.nets[net_index(net)]
    }

    pub fn set_net_color(&mut self, net: Network, color: &str) {
        self.nets[net_index(net)] = color.to_string();
    }

    /// Sets the resolved palette; none is the native scheme.
    pub fn set_scheme(&mut self, palette: Option<Palette>, dark: bool) {
        self.native = palette.is_none();
        self.base = palette.unwrap_or_else(|| Palette::get(dark));
        self.dark = dark;
    }

    fn palette(&self) -> Palette {
        if self.mono {
            return Palette::mono();
        }
        let mut palette = self.base;
        for (name, rgb) in &self.overrides {
            if let Some(slot) = palette.slot(name) {
                *slot = *rgb;
            }
        }
        palette
    }

    pub fn rgb(&self, kind: Kind) -> Rgb {
        let palette = self.palette();
        match kind {
            Kind::Net(net) if !self.mono => {
                let name = self.net_color(net);
                palette
                    .named(name)
                    .or_else(|| Rgb::parse(name))
                    .unwrap_or(palette.fg)
            }
            _ => palette.kind(kind),
        }
    }

    fn overridden(&self, name: &str) -> Option<Rgb> {
        self.overrides
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, rgb)| *rgb)
    }

    /// The selected row's background in the native scheme: the terminal's, lifted 5% toward the text.
    fn native_selected(&self, bg: Rgb) -> Rgb {
        mix(
            bg,
            if self.dark {
                Rgb(255, 255, 255)
            } else {
                Rgb(0, 0, 0)
            },
            0.05,
        )
    }

    /// A row's background; mono keeps it and reverses the selected row instead.
    pub fn row_bg(&self, selected: bool) -> Rgb {
        if self.native {
            let bg = self.term_bg.unwrap_or(if self.dark {
                Rgb(0, 0, 0)
            } else {
                Rgb(255, 255, 255)
            });
            return match (selected && !self.mono, self.term_bg) {
                (false, _) => bg,
                (true, Some(term)) => self.native_selected(term),
                (true, None) if self.dark => Rgb(48, 48, 48),
                (true, None) => Rgb(228, 228, 228),
            };
        }
        let palette = self.palette();
        if selected && !self.mono {
            mix(palette.bg, palette.surface, 0.4)
        } else {
            palette.bg
        }
    }

    fn paint(&self, kind: Kind) -> Paint {
        if !self.native {
            return Paint::Rgb(self.rgb(kind));
        }
        if self.mono {
            return Paint::Default;
        }
        if let Kind::Net(net) = kind {
            let name = self.net_color(net);
            if name == "default" {
                return Paint::Default;
            }
            if let Some(index) = ansi_index(name) {
                return Paint::Indexed(index);
            }
            return Rgb::parse(name).map_or(Paint::Default, Paint::Rgb);
        }
        let role = role(kind).unwrap_or("fg");
        if let Some(rgb) = self.overridden(role) {
            return Paint::Rgb(rgb);
        }
        match kind {
            Kind::Fg | Kind::Bold | Kind::Dim | Kind::Line => Paint::Default,
            _ => ansi_index(role).map_or(Paint::Default, Paint::Indexed),
        }
    }

    fn paint_fg(&self, kind: Kind, selected: bool) -> Paint {
        self.paint(if self.reverse(selected) {
            Kind::Fg
        } else {
            kind
        })
    }

    fn paint_bg(&self, selected: bool) -> Paint {
        if !self.native {
            return Paint::Rgb(self.row_bg(selected));
        }
        if self.mono || !selected {
            return Paint::Default;
        }
        if let Some(rgb) = self.overridden("surface") {
            return Paint::Rgb(rgb);
        }
        match self.term_bg {
            Some(bg) => Paint::Rgb(self.native_selected(bg)),
            None if self.dark => Paint::Indexed(236),
            None => Paint::Indexed(254),
        }
    }

    fn to_color(&self, paint: Paint) -> Color {
        if self.plain {
            return Color::Reset;
        }
        match paint {
            Paint::Default => Color::Reset,
            Paint::Indexed(n) => Color::Indexed(n),
            Paint::Rgb(rgb) => self.color(rgb),
        }
    }

    /**
    Mono shows the selected row in reverse video, an attribute that survives NO_COLOR. Every span on
    it takes the foreground so the reversed row is one even block. The plain level marks the
    selection with `>` instead.
    */
    pub fn reverse(&self, selected: bool) -> bool {
        self.mono && selected && !self.ascii
    }

    /// A span's foreground on its row.
    pub fn fg(&self, kind: Kind, selected: bool) -> Color {
        self.to_color(self.paint_fg(kind, selected))
    }

    /// A row's background.
    pub fn bg(&self, selected: bool) -> Color {
        self.to_color(self.paint_bg(selected))
    }

    /// The SGR parameters for a span's foreground.
    pub fn fg_sgr(&self, kind: Kind, selected: bool) -> String {
        match self.paint_fg(kind, selected) {
            Paint::Default => "39".into(),
            Paint::Indexed(n) => indexed_sgr(n, 30, 90, 38),
            Paint::Rgb(rgb) => self.sgr(38, rgb),
        }
    }

    /// The SGR parameters for a row's background; none leaves the terminal's own.
    pub fn bg_sgr(&self, selected: bool) -> Option<String> {
        match self.paint_bg(selected) {
            Paint::Default => None,
            Paint::Indexed(n) => Some(indexed_sgr(n, 40, 100, 48)),
            Paint::Rgb(rgb) => Some(self.sgr(48, rgb)),
        }
    }

    /// Whether a kind is drawn with the dim attribute instead of a dim color.
    pub fn dim_attr(&self, kind: Kind) -> bool {
        let Some(role) = role(kind).filter(|_| matches!(kind, Kind::Dim | Kind::Line)) else {
            return false;
        };
        !self.ascii && (self.plain || (self.native && self.overridden(role).is_none()))
    }

    pub fn bold(kind: Kind) -> bool {
        kind == Kind::Bold
    }

    pub fn underline(&self, kind: Kind) -> bool {
        self.mono && kind == Kind::Link
    }

    /// The SGR parameters for a foreground (`38`) or background (`48`).
    pub fn sgr(&self, layer: u8, rgb: Rgb) -> String {
        if self.truecolor {
            let Rgb(r, g, b) = rgb;
            format!("{layer};2;{r};{g};{b}")
        } else {
            format!("{layer};5;{}", nearest_256(rgb))
        }
    }

    pub fn color(&self, rgb: Rgb) -> Color {
        if self.plain {
            Color::Reset
        } else if self.truecolor {
            Color::Rgb(rgb.0, rgb.1, rgb.2)
        } else {
            Color::Indexed(nearest_256(rgb))
        }
    }
}

fn indexed_sgr(n: u8, low: u8, high: u8, extended: u8) -> String {
    match n {
        0..=7 => (low + n).to_string(),
        8..=15 => (high + n - 8).to_string(),
        _ => format!("{extended};5;{n}"),
    }
}

fn net_index(net: Network) -> usize {
    Network::ALL.iter().position(|n| *n == net).unwrap_or(0)
}

// MARK: ascii

/**
A glyph the reader draws, in ASCII at the same width, for the plain level; any other character
comes back unchanged. Width matters because the row is already laid out when it is painted.
*/
pub fn ascii_char(ch: char) -> char {
    match ch {
        '●' | '◉' | '☑' => '*',
        '○' | '◌' | '☐' | '◐' | '◓' | '◑' | '◒' | '◔' | '◕' => 'o',
        '›' | '❯' | '▍' => '>',
        '‹' => '<',
        '│' | '┃' | '╎' | '┆' | '▎' | '▏' => '|',
        '─' | '━' | '╌' | '┄' => '-',
        '╭' | '╮' | '╰' | '╯' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' => {
            '+'
        }
        '✓' => '+',
        '✗' => 'x',
        _ => ch,
    }
}

/**
A key as the plain level spells it, `⏎` as `enter`. These change width, so the code that lays
out the row asks for them before it measures.
*/
pub fn ascii_key(key: &'static str, ascii: bool) -> &'static str {
    if !ascii {
        return key;
    }
    match key {
        "⏎" => "enter",
        "⌫" => "bksp",
        "⏎ →" => "enter right",
        "j k  ↓ ↑" => "j k  down up",
        "⌫ ← esc" => "bksp left esc",
        "← →" => "left right",
        "⇧⏎" => "shift-enter",
        "1 … 9" => "1-9",
        "⌘/ctrl-click" => "cmd/ctrl-click",
        _ => key,
    }
}

// MARK: appearance settings

/// A setting with a fixed set of words, cycled by Settings and validated by `socli config set`.
pub trait Choice: Copy + PartialEq + Sized + 'static {
    const ALL: &'static [Self];

    fn word(self) -> &'static str;

    fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        Self::ALL.iter().copied().find(|c| c.word() == value)
    }

    fn step(self, dir: i32) -> Self {
        let n = Self::ALL.len() as i32;
        let i = Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as i32;
        Self::ALL[(i + dir).rem_euclid(n) as usize]
    }

    /// Every word, comma-separated, for errors.
    fn words() -> String {
        let words: Vec<&str> = Self::ALL.iter().map(|c| c.word()).collect();
        words.join(", ")
    }
}

macro_rules! choice {
    ($name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl Choice for $name {
            const ALL: &'static [Self] = &[$(Self::$variant),+];

            fn word(self) -> &'static str {
                match self {
                    $(Self::$variant => $word),+
                }
            }
        }
    };
}

pub(crate) use choice;

choice!(ColorMode { Full => "full", Mono => "mono" });
choice!(Density { Compact => "compact", Cozy => "cozy", Roomy => "roomy" });
choice!(ColorBy { Auto => "auto", Network => "network", Author => "author" });
choice!(Replies { Oldest => "oldest", Newest => "newest" });
choice!(Layout { Auto => "auto", Single => "single", Split => "split" });
choice!(Switch { On => "on", Off => "off" });

/// How much art the reader draws. `standard` is the old word for [`Detail::Standard`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Plain,
    Minimal,
    Standard,
    Rich,
}

impl Choice for Detail {
    const ALL: &'static [Self] = &[Self::Plain, Self::Minimal, Self::Standard, Self::Rich];

    fn word(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Minimal => "minimal",
            Self::Standard => "medium",
            Self::Rich => "rich",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        if value == "standard" {
            return Some(Self::Standard);
        }
        Self::ALL.iter().copied().find(|item| item.word() == value)
    }
}

impl Density {
    /// Rows for one spacer slot of the L1 table: roomy, cozy, compact.
    pub fn rows(self, roomy: usize, cozy: usize, compact: usize) -> usize {
        match self {
            Density::Roomy => roomy,
            Density::Cozy => cozy,
            Density::Compact => compact,
        }
    }
}

/// The glyphs the reader draws, as a Nerd Font set and a Menlo-safe Unicode set.
#[derive(Clone, Copy, Debug)]
pub struct Icons {
    pub reply: &'static str,
    pub repost: &'static str,
    pub like: &'static str,
    pub liked: &'static str,
    pub bookmark: &'static str,
    pub bookmarked: &'static str,
    pub views: &'static str,
    pub more: &'static str,
    pub verified: &'static str,
    pub bell: &'static str,
    pub link: &'static str,
    pub image: &'static str,
    pub check: &'static str,
    pub cross: &'static str,
    pub at: &'static str,
    pub gauge: [&'static str; 8],
}

impl Icons {
    pub fn new(nerd: bool) -> Self {
        if nerd {
            Self {
                reply: "\u{f0182}",      // nf-md-comment_outline U+F0182
                repost: "\u{f0456}",     // nf-md-repeat U+F0456
                like: "\u{f02d5}",       // nf-md-heart_outline U+F02D5
                liked: "\u{f02d1}",      // nf-md-heart U+F02D1
                bookmark: "\u{f00c3}",   // nf-md-bookmark_outline U+F00C3
                bookmarked: "\u{f00c0}", // nf-md-bookmark U+F00C0
                views: "\u{f0128}",      // nf-md-chart_bar U+F0128
                more: "\u{f01d8}",       // nf-md-dots_horizontal U+F01D8
                verified: "\u{f0791}",   // nf-md-check_decagram U+F0791
                bell: "\u{f009c}",       // nf-md-bell_outline U+F009C
                link: "\u{f0339}",       // nf-md-link_variant U+F0339
                image: "\u{f0976}",      // nf-md-image_outline U+F0976
                check: "\u{f012c}",      // nf-md-check U+F012C
                cross: "\u{f0156}",      // nf-md-close U+F0156
                at: "\u{f0065}",         // nf-md-at U+F0065
                gauge: [
                    "\u{f0a9e}", // nf-md-circle_slice_1 U+F0A9E
                    "\u{f0a9f}", // nf-md-circle_slice_2 U+F0A9F
                    "\u{f0aa0}", // nf-md-circle_slice_3 U+F0AA0
                    "\u{f0aa1}", // nf-md-circle_slice_4 U+F0AA1
                    "\u{f0aa2}", // nf-md-circle_slice_5 U+F0AA2
                    "\u{f0aa3}", // nf-md-circle_slice_6 U+F0AA3
                    "\u{f0aa4}", // nf-md-circle_slice_7 U+F0AA4
                    "\u{f0aa5}", // nf-md-circle_slice_8 U+F0AA5
                ],
            }
        } else {
            Self {
                reply: "\u{21a9}",      // ↩ U+21A9
                repost: "\u{21c4}",     // ⇄ U+21C4
                like: "\u{2661}",       // ♡ U+2661
                liked: "\u{2665}",      // ♥ U+2665
                bookmark: "\u{2606}",   // ☆ U+2606
                bookmarked: "\u{2605}", // ★ U+2605
                views: "\u{25c9}",      // ◉ U+25C9
                more: "\u{22ef}",       // ⋯ U+22EF
                verified: "\u{2713}",   // ✓ U+2713
                bell: "\u{2691}",       // ⚑ U+2691
                link: "\u{2197}",       // ↗ U+2197
                image: "\u{25a3}",      // ▣ U+25A3
                check: "\u{2713}",      // ✓ U+2713
                cross: "\u{2717}",      // ✗ U+2717
                at: "@",
                gauge: [
                    "\u{25d4}", // ◔ U+25D4
                    "\u{25d4}", // ◔ U+25D4
                    "\u{25d1}", // ◑ U+25D1
                    "\u{25d1}", // ◑ U+25D1
                    "\u{25d5}", // ◕ U+25D5
                    "\u{25d5}", // ◕ U+25D5
                    "\u{25cf}", // ● U+25CF
                    "\u{25cf}", // ● U+25CF
                ],
            }
        }
    }
}

choice!(IconMode { Auto => "auto", Nerd => "nerd", Unicode => "unicode" });

/**
 * True when the Nerd Font set should be used. Order: `--no-icons`, `--icons`, `SOCLI_ICONS`,
 * `NO_ICONS`, `[ui] icons`, then auto-detection, which only trusts terminals that ship a Nerd
 * Font fallback.
 */
pub fn resolve_icons(
    no_icons: bool,
    flag: Option<IconMode>,
    config: Option<IconMode>,
    env: impl Fn(&str) -> Option<String>,
) -> bool {
    if no_icons {
        return false;
    }
    let explicit = |mode: Option<IconMode>| mode.filter(|mode| *mode != IconMode::Auto);
    let chosen = explicit(flag)
        .or_else(|| explicit(env("SOCLI_ICONS").and_then(|value| IconMode::parse(&value))));
    match chosen {
        Some(IconMode::Nerd) => return true,
        Some(IconMode::Unicode) => return false,
        Some(IconMode::Auto) | None => {}
    }
    if env("NO_ICONS").is_some_and(|value| !value.is_empty()) {
        return false;
    }
    match explicit(config) {
        Some(IconMode::Nerd) => return true,
        Some(IconMode::Unicode) => return false,
        Some(IconMode::Auto) | None => {}
    }
    let program = env("TERM_PROGRAM").unwrap_or_default();
    program.eq_ignore_ascii_case("ghostty")
        || program.eq_ignore_ascii_case("WezTerm")
        || env("TERM").as_deref() == Some("xterm-ghostty")
}

/**
The environment lookup for `resolve_icons`. Without a terminal (piped output, or an AI agent) it
hides `TERM` and `TERM_PROGRAM`, so `auto` picks the Unicode set even when the agent started in
Ghostty; `--icons`, `SOCLI_ICONS`, `NO_ICONS` and the config still decide as usual.
*/
pub fn icon_env(tty: bool) -> impl Fn(&str) -> Option<String> {
    move |key: &str| {
        if !tty && matches!(key, "TERM" | "TERM_PROGRAM") {
            return None;
        }
        std::env::var(key).ok()
    }
}

const AUTHOR: [Kind; 8] = [
    Kind::Amber,
    Kind::Rose,
    Kind::Coral,
    Kind::Sage,
    Kind::Violet,
    Kind::Teal,
    Kind::Gold,
    Kind::Sky,
];

pub fn author_color(username: &str) -> Kind {
    let n = username
        .bytes()
        .fold(0usize, |acc, b| acc.wrapping_add(b as usize));
    AUTHOR[n % AUTHOR.len()]
}

/// One row of the So_ mark. `pill` is the column where the blue cursor starts.
pub struct SoRow {
    pub glyphs: &'static str,
    pub pill: Option<usize>,
}

/// The socli mark: So in the foreground, the cursor pill in blue (`#1d9bf0`).
///
/// Sampled from the dark-blue wordmark. The rounded square is the terminal ground, so it is not drawn.
pub fn so_mark() -> &'static [SoRow] {
    const ROWS: &[SoRow] = &[
        SoRow {
            glyphs: "▄██████▄",
            pill: None,
        },
        SoRow {
            glyphs: "███  ▀██   ▄▄██▄▄",
            pill: None,
        },
        SoRow {
            glyphs: "▀████▄▄   ██▀▀▀██▄",
            pill: None,
        },
        SoRow {
            glyphs: "▄▄▄▀▀███ ███   ███",
            pill: None,
        },
        SoRow {
            glyphs: "███▄▄▄██  ███▄▄██▀",
            pill: None,
        },
        SoRow {
            glyphs: " ▀▀▀▀▀▀    ▀▀▀▀▀  ████████",
            pill: Some(18),
        },
    ];
    ROWS
}

/// Columns the widest So_ row occupies.
pub fn so_width() -> usize {
    so_mark()
        .iter()
        .map(|row| row.glyphs.chars().count())
        .max()
        .unwrap_or(0)
}

/// The socli S mark, five rows of half blocks. The ask box uses it when So_ does not fit.
pub fn logo() -> [String; 5] {
    const ROWS: [&str; 10] = [
        "..##########..",
        ".############.",
        "###.......###.",
        "###...........",
        ".###########..",
        "..###########.",
        "...........###",
        ".###.......###",
        ".############.",
        "..##########..",
    ];
    let mut out = std::array::from_fn(|_| String::new());
    for (i, pair) in ROWS.chunks(2).enumerate() {
        let (a, b) = (pair[0].as_bytes(), pair[1].as_bytes());
        for x in 0..a.len() {
            let top = a[x] == b'#';
            let bot = b[x] == b'#';
            out[i].push(match (top, bot) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
    }
    out
}

pub fn compact(n: u64) -> String {
    if n >= 1_000_000 {
        trim_tenths(n as f64 / 1_000_000.0, "M")
    } else if n >= 1_000 {
        trim_tenths(n as f64 / 1_000.0, "K")
    } else {
        n.to_string()
    }
}

fn trim_tenths(n: f64, suffix: &str) -> String {
    let tenths = (n * 10.0).round() / 10.0;
    if (tenths - tenths.floor()).abs() < 0.05 {
        format!("{}{suffix}", tenths as u64)
    } else {
        format!("{tenths:.1}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ascii_glyphs_keep_their_width() {
        use unicode_width::UnicodeWidthChar;
        for ch in "●◉☑○◌☐◐◓◑◒◔◕›❯▍‹│┃╎┆▎▏─━╌┄╭╮╰╯┌┐└┘├┤┬┴┼✓✗".chars()
        {
            let mapped = super::ascii_char(ch);
            assert!(mapped.is_ascii(), "{ch}");
            assert_eq!(mapped.width(), ch.width(), "{ch}");
        }
        assert_eq!(super::ascii_char('·'), '·');
        assert_eq!(super::ascii_char('é'), 'é');
    }

    #[test]
    fn ascii_keys_spell_the_glyph() {
        assert_eq!(super::ascii_key("⏎", true), "enter");
        assert_eq!(super::ascii_key("⌫ ← esc", true), "bksp left esc");
        assert_eq!(super::ascii_key("⏎", false), "⏎");
        assert_eq!(super::ascii_key("j/k", true), "j/k");
    }

    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn nearest_256_picks_the_cube_or_the_gray_ramp() {
        assert_eq!(nearest_256(Rgb(0, 0, 0)), 16);
        assert_eq!(nearest_256(Rgb(255, 255, 255)), 231);
        assert_eq!(nearest_256(Rgb(255, 0, 0)), 196);
        assert_eq!(nearest_256(Rgb(128, 128, 128)), 244);
        assert_eq!(nearest_256(Rgb(29, 155, 240)), 33);
    }

    #[test]
    fn truecolor_follows_colorterm_known_terminals_and_the_override() {
        assert!(truecolor(env(&[("COLORTERM", "truecolor")])));
        assert!(truecolor(env(&[("COLORTERM", "24bit")])));
        assert!(truecolor(env(&[("TERM_PROGRAM", "iTerm.app")])));
        assert!(truecolor(env(&[("TERM", "xterm-direct")])));
        assert!(!truecolor(env(&[("TERM_PROGRAM", "Apple_Terminal")])));
        assert!(!truecolor(env(&[
            ("COLORTERM", "truecolor"),
            ("SOCLI_TRUECOLOR", "0")
        ])));
        assert!(truecolor(env(&[("SOCLI_TRUECOLOR", "1")])));
        let theme = Theme {
            truecolor: false,
            ..Theme::default()
        };
        assert_eq!(theme.sgr(38, Rgb(255, 0, 0)), "38;5;196");
        assert_eq!(Theme::default().sgr(48, Rgb(1, 2, 3)), "48;2;1;2;3");
    }

    #[test]
    fn background_replies_decide_dark_or_light() {
        assert_eq!(osc11_dark("\x1b]11;rgb:0000/0000/0000\x07"), Some(true));
        assert_eq!(osc11_dark("\x1b]11;rgb:ffff/ffff/ffff\x1b\\"), Some(false));
        assert_eq!(osc11_dark("\x1b]11;rgb:fd/f6/e3\x07"), Some(false));
        assert_eq!(osc11_dark("\x1b[?62;22c"), None);
        assert_eq!(colorfgbg_dark("15;0"), Some(true));
        assert_eq!(colorfgbg_dark("0;15"), Some(false));
        assert_eq!(colorfgbg_dark("7;default;8"), Some(true));
        assert_eq!(colorfgbg_dark("nope"), None);
    }

    #[test]
    fn osc11_replies_give_the_color() {
        assert_eq!(
            osc11_rgb("\x1b]11;rgb:ffff/8000/0000\x07"),
            Some(Rgb(255, 128, 0))
        );
        assert_eq!(osc11_rgb("\x1b[?62;22c"), None);
    }

    #[test]
    fn every_scheme_round_trips_through_its_word() {
        let list = Scheme::list();
        assert_eq!(list.len(), 31);
        for scheme in list {
            assert_eq!(Scheme::parse(&scheme.word()), Some(scheme), "{scheme:?}");
        }
        let forced = Scheme::parse("catppuccin-dark").unwrap();
        assert_eq!(Scheme::parse(&forced.word()), Some(forced));
    }

    #[test]
    fn scheme_words_keep_the_old_values_and_check_variants() {
        assert_eq!(Scheme::parse("dark"), Some(Scheme::Black));
        assert_eq!(Scheme::parse(" LIGHT "), Some(Scheme::White));
        assert!(Scheme::parse("catppuccin-light").is_some());
        assert_eq!(Scheme::parse("matrix-light"), None);
        assert_eq!(Scheme::parse("nope"), None);
        assert!(Scheme::words().contains("native, black, white, aura"));
    }

    #[test]
    fn schemes_resolve_against_the_terminal() {
        let catppuccin = Scheme::parse("catppuccin").unwrap();
        let (palette, dark) = catppuccin.resolve(false);
        assert!(!dark);
        assert_eq!(
            palette.unwrap().bg,
            NAMED[catppuccin_index()].light.unwrap().bg
        );
        let matrix = Scheme::parse("matrix").unwrap();
        let (palette, dark) = matrix.resolve(false);
        assert!(dark);
        assert_eq!(palette.unwrap().bg, NAMED[matrix_index()].dark.bg);
        assert!(Scheme::Native.resolve(true).0.is_none());
        assert!(Scheme::Black.resolve(false).1);
        assert!(!Scheme::Auto.resolve(false).1);
    }

    fn catppuccin_index() -> usize {
        NAMED.iter().position(|n| n.name == "catppuccin").unwrap()
    }

    fn matrix_index() -> usize {
        NAMED.iter().position(|n| n.name == "matrix").unwrap()
    }

    fn native() -> Theme {
        let mut theme = Theme::default();
        theme.set_scheme(None, true);
        theme
    }

    #[test]
    fn native_leaves_text_and_background_to_the_terminal() {
        let theme = native();
        assert_eq!(theme.fg(Kind::Fg, false), Color::Reset);
        assert_eq!(theme.fg(Kind::Blue, false), Color::Indexed(4));
        assert_eq!(theme.fg(Kind::Coral, false), Color::Indexed(9));
        assert!(theme.dim_attr(Kind::Dim));
        assert!(!theme.dim_attr(Kind::Blue));
        assert_eq!(theme.bg(false), Color::Reset);
        assert_eq!(theme.fg_sgr(Kind::Blue, false), "34");
        assert_eq!(theme.fg_sgr(Kind::Coral, false), "91");
        assert_eq!(theme.fg_sgr(Kind::Fg, false), "39");
        assert_eq!(theme.bg_sgr(false), None);
    }

    #[test]
    fn native_selection_lifts_the_terminal_background() {
        let mut theme = native();
        theme.term_bg = Some(Rgb(0, 0, 0));
        assert_eq!(theme.bg(true), Color::Rgb(13, 13, 13));
        assert_eq!(theme.row_bg(false), Rgb(0, 0, 0));
        assert_eq!(theme.row_bg(true), Rgb(13, 13, 13));
        theme.term_bg = None;
        assert_eq!(theme.bg(true), Color::Indexed(236));
        assert_eq!(theme.bg_sgr(true).as_deref(), Some("48;5;236"));
        theme.set_scheme(None, false);
        assert_eq!(theme.bg(true), Color::Indexed(254));
        assert_eq!(theme.row_bg(false), Rgb(255, 255, 255));
        assert_eq!(theme.row_bg(true), Rgb(228, 228, 228));
    }

    #[test]
    fn native_keeps_overrides_and_network_colors() {
        let mut notices = Vec::new();
        let theme = native().with_colors(
            [("blue", "#112233")],
            [(Network::X, None), (Network::Threads, Some("#123456"))],
            &mut notices,
        );
        assert_eq!(theme.fg(Kind::Blue, false), Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(theme.fg(Kind::Net(Network::X), false), Color::Reset);
        assert_eq!(
            theme.fg(Kind::Net(Network::Threads), false),
            Color::Rgb(0x12, 0x34, 0x56)
        );
        assert!(theme.dim_attr(Kind::Dim));
    }

    #[test]
    fn native_mono_sets_no_color_and_still_reverses() {
        let mut theme = native();
        theme.mono = true;
        for kind in [Kind::Fg, Kind::Blue, Kind::Dim, Kind::Net(Network::Bluesky)] {
            assert_eq!(theme.fg(kind, false), Color::Reset);
            assert_eq!(theme.fg(kind, true), Color::Reset);
        }
        assert_eq!(theme.bg(true), Color::Reset);
        assert!(theme.reverse(true));
    }

    #[test]
    fn palette_values_follow_the_scheme() {
        assert_eq!(
            palette_value("blue", Scheme::Auto).as_deref(),
            Some("#1d9bf0")
        );
        assert_eq!(
            palette_value("blue", Scheme::Native).as_deref(),
            Some("terminal")
        );
        assert_eq!(palette_value("bg", Scheme::Auto), None);
    }

    #[test]
    fn bad_colors_keep_their_default_and_leave_a_notice() {
        let mut notices = Vec::new();
        let theme = Theme::default().with_colors(
            [("blue", "#112233"), ("pink", "hotpink")],
            [
                (Network::Bluesky, Some("teal")),
                (Network::Threads, Some("purple")),
                (Network::X, None),
            ],
            &mut notices,
        );
        assert_eq!(
            notices,
            [
                "\"hotpink\" is not a color for colors.pink. Use a palette name or #rrggbb",
                "\"purple\" is not a color for threads.color. Use a palette name or #rrggbb",
            ]
        );
        assert_eq!(theme.rgb(Kind::Blue), Rgb(0x11, 0x22, 0x33));
        assert_eq!(theme.rgb(Kind::Pink), Palette::get(true).pink);
        assert_eq!(theme.net_color(Network::Bluesky), "teal");
        assert_eq!(theme.net_color(Network::Threads), "violet");
        assert_eq!(
            theme.rgb(Kind::Net(Network::Bluesky)),
            Palette::get(true).teal
        );
        assert_eq!(theme.rgb(Kind::Net(Network::X)), Palette::get(true).fg);
    }

    #[test]
    fn mono_drops_colors_and_reverses_the_selected_row() {
        let theme = Theme {
            mono: true,
            ..Theme::default()
        };
        assert_eq!(theme.rgb(Kind::Net(Network::Bluesky)), theme.rgb(Kind::Fg));
        assert_eq!(theme.rgb(Kind::Pink), theme.rgb(Kind::Fg));
        assert_eq!(theme.row_bg(true), theme.row_bg(false));
        assert_eq!(theme.fg(Kind::Dim, true), theme.color(theme.rgb(Kind::Fg)));
        assert!(theme.reverse(true) && !theme.reverse(false));
        assert!(!Theme::default().reverse(true));
        assert!(theme.underline(Kind::Link));
        assert!(!Theme::default().underline(Kind::Link));
    }

    #[test]
    fn no_color_leaves_every_color_to_the_terminal() {
        let theme = Theme {
            mono: true,
            plain: true,
            ..Theme::default()
        };
        assert_eq!(
            theme.color(theme.rgb(Kind::Pink)),
            ratatui::style::Color::Reset
        );
        assert_eq!(
            theme.color(theme.row_bg(true)),
            ratatui::style::Color::Reset
        );
    }

    #[test]
    fn ghostty_and_wezterm_get_nerd_icons() {
        assert!(resolve_icons(
            false,
            None,
            None,
            env(&[("TERM_PROGRAM", "ghostty")])
        ));
        assert!(resolve_icons(
            false,
            None,
            None,
            env(&[("TERM_PROGRAM", "WezTerm")])
        ));
        assert!(resolve_icons(
            false,
            None,
            None,
            env(&[("TERM", "xterm-ghostty")])
        ));
    }

    #[test]
    fn vscode_and_unknown_terminals_get_unicode() {
        assert!(!resolve_icons(
            false,
            None,
            None,
            env(&[("TERM_PROGRAM", "vscode")])
        ));
        assert!(!resolve_icons(
            false,
            None,
            None,
            env(&[("TERM_PROGRAM", "kitty")])
        ));
        assert!(!resolve_icons(false, None, None, env(&[])));
    }

    #[test]
    fn env_override_wins_over_detection() {
        let nerd = [("TERM_PROGRAM", "vscode"), ("SOCLI_ICONS", "NERD")];
        assert!(resolve_icons(false, None, None, env(&nerd)));
        let unicode = [("TERM_PROGRAM", "ghostty"), ("SOCLI_ICONS", "unicode")];
        assert!(!resolve_icons(false, None, None, env(&unicode)));
        let junk = [("TERM_PROGRAM", "ghostty"), ("SOCLI_ICONS", "maybe")];
        assert!(resolve_icons(false, None, None, env(&junk)));
    }

    #[test]
    fn no_icons_flag_beats_everything() {
        let nerd = [("TERM_PROGRAM", "ghostty"), ("SOCLI_ICONS", "nerd")];
        assert!(!resolve_icons(true, None, None, env(&nerd)));
        assert!(!resolve_icons(true, Some(IconMode::Nerd), None, env(&nerd)));
    }

    #[test]
    fn icons_flag_beats_env() {
        let unicode = [("SOCLI_ICONS", "unicode")];
        assert!(resolve_icons(
            false,
            Some(IconMode::Nerd),
            None,
            env(&unicode)
        ));
        let nerd = [("SOCLI_ICONS", "nerd")];
        assert!(!resolve_icons(
            false,
            Some(IconMode::Unicode),
            None,
            env(&nerd)
        ));
        assert!(resolve_icons(false, Some(IconMode::Auto), None, env(&nerd)));
    }

    #[test]
    fn no_icons_env_turns_detection_off() {
        let set = [("TERM_PROGRAM", "ghostty"), ("NO_ICONS", "1")];
        assert!(!resolve_icons(false, None, None, env(&set)));
        let empty = [("TERM_PROGRAM", "ghostty"), ("NO_ICONS", "")];
        assert!(resolve_icons(false, None, None, env(&empty)));
    }

    #[test]
    fn unicode_set_is_single_width_and_outside_the_private_use_area() {
        let icons = Icons::new(false);
        let single = [
            icons.reply,
            icons.repost,
            icons.like,
            icons.liked,
            icons.bookmark,
            icons.bookmarked,
            icons.views,
            icons.more,
            icons.verified,
            icons.bell,
            icons.link,
            icons.image,
            icons.check,
            icons.cross,
            icons.at,
        ];
        for glyph in single.iter().chain(icons.gauge.iter()) {
            assert_eq!(glyph.width(), 1, "{glyph}");
            assert!(!glyph.chars().any(is_private_use), "{glyph}");
        }
    }

    fn is_private_use(ch: char) -> bool {
        matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
    }

    #[test]
    fn icon_env_hides_the_terminal_without_a_tty() {
        let piped = icon_env(false);
        assert_eq!(piped("TERM"), None);
        assert_eq!(piped("TERM_PROGRAM"), None);
    }

    #[test]
    fn detail_steps_wrap_both_ways() {
        assert_eq!(Detail::Standard.step(1), Detail::Rich);
        assert_eq!(Detail::Rich.step(1), Detail::Plain);
        assert_eq!(Detail::Plain.step(-1), Detail::Rich);
        assert_eq!(Detail::parse("MINIMAL"), Some(Detail::Minimal));
        assert_eq!(Detail::Standard.word(), "medium");
        assert_eq!(Detail::parse("standard"), Some(Detail::Standard));
        assert_eq!(Detail::parse("medium"), Some(Detail::Standard));
    }

    #[test]
    fn an_unset_detail_is_rich_unless_the_client_is_constrained() {
        assert_eq!(default_detail(env(&[])), Detail::Rich);
        for (term, program) in [
            ("xterm-256color", "vscode"),
            ("xterm-256color", "iTerm.app"),
            ("xterm-256color", "Apple_Terminal"),
            ("xterm-ghostty", "ghostty"),
            ("wezterm", "WezTerm"),
            ("linux", ""),
            ("screen", ""),
            ("tmux-256color", ""),
            ("xterm", ""),
        ] {
            assert_eq!(
                default_detail(env(&[("TERM", term), ("TERM_PROGRAM", program)])),
                Detail::Rich,
                "{term} / {program}"
            );
        }
        for term in [
            "dumb", "unknown", "ansi", "vt52", "vt100", "vt102", "vt220", "network", "vt102-w",
            "ansi.sys", "DUMB",
        ] {
            assert_eq!(
                default_detail(env(&[("TERM", term), ("TERM_PROGRAM", "vscode")])),
                Detail::Plain,
                "{term}"
            );
        }
        for (term, program) in [
            ("espterm", ""),
            ("espterm-256color", "xterm"),
            ("ESP32", ""),
            ("esp8266", ""),
            ("esp-idf", ""),
            ("xterm-256color", "ESPTerm"),
            ("xterm", "espressif"),
            ("esp32s3", ""),
        ] {
            assert_eq!(
                default_detail(env(&[("TERM", term), ("TERM_PROGRAM", program)])),
                Detail::Plain,
                "{term} / {program}"
            );
        }
    }
}
