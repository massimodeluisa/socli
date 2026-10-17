//! Key and mouse events, one shape for the terminal and for the browser.
//!
//! Native builds re-export crossterm, so the reader's match arms stay as they are.
//! The browser sends one byte per key. `0xFE` starts the rare longer form: a modifier
//! byte and one UTF-8 character, for a chord or a character that does not fit the byte.

#[cfg(feature = "native")]
pub use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode, MouseButton, MouseEvent,
    MouseEventKind,
};

#[cfg(not(feature = "native"))]
mod browser {
    use std::ops::{BitOr, BitOrAssign};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Code {
        Char(char),
        Enter,
        Esc,
        Tab,
        BackTab,
        Backspace,
        Up,
        Down,
        Left,
        Right,
        Home,
        End,
        PageUp,
        PageDown,
        Delete,
        F(u8),
        Null,
        Modifier(ModKey),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum ModKey {
        LeftShift,
        LeftControl,
        LeftSuper,
        LeftMeta,
        RightControl,
        RightSuper,
        RightMeta,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Kind {
        Press,
        Repeat,
        Release,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Modifiers(u8);

    impl Modifiers {
        pub const SHIFT: Self = Self(0b0000_0001);
        pub const CONTROL: Self = Self(0b0000_0010);
        pub const ALT: Self = Self(0b0000_0100);
        pub const SUPER: Self = Self(0b0000_1000);
        pub const HYPER: Self = Self(0b0001_0000);
        pub const META: Self = Self(0b0010_0000);
        pub const NONE: Self = Self(0);

        pub const fn empty() -> Self {
            Self::NONE
        }

        pub const fn contains(self, other: Self) -> bool {
            self.0 & other.0 == other.0
        }

        pub const fn intersects(self, other: Self) -> bool {
            self.0 & other.0 != 0
        }
    }

    impl BitOr for Modifiers {
        type Output = Self;

        fn bitor(self, rhs: Self) -> Self {
            Self(self.0 | rhs.0)
        }
    }

    impl BitOrAssign for Modifiers {
        fn bitor_assign(&mut self, rhs: Self) {
            self.0 |= rhs.0;
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Key {
        pub code: Code,
        pub modifiers: Modifiers,
        pub kind: Kind,
    }

    impl Key {
        pub const fn new(code: Code, modifiers: Modifiers) -> Self {
            Self {
                code,
                modifiers,
                kind: Kind::Press,
            }
        }

        pub const fn new_with_kind(code: Code, modifiers: Modifiers, kind: Kind) -> Self {
            Self {
                code,
                modifiers,
                kind,
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Button {
        Left,
        Right,
        Middle,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum MouseKind {
        Down(Button),
        Up(Button),
        Drag(Button),
        Moved,
        ScrollDown,
        ScrollUp,
        ScrollLeft,
        ScrollRight,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Mouse {
        pub kind: MouseKind,
        pub column: u16,
        pub row: u16,
        pub modifiers: Modifiers,
    }
}

#[cfg(not(feature = "native"))]
pub use browser::{
    Button as MouseButton, Code as KeyCode, Key as KeyEvent, Kind as KeyEventKind,
    ModKey as ModifierKeyCode, Modifiers as KeyModifiers, Mouse as MouseEvent,
    MouseKind as MouseEventKind,
};

/// One key from the browser. A slice of one byte is the usual case.
pub fn decode(bytes: &[u8]) -> Option<KeyEvent> {
    let (&first, rest) = bytes.split_first()?;
    if first == EXTENDED {
        return decode_extended(rest);
    }
    Some(decode_byte(first))
}

const EXTENDED: u8 = 0xFE;
const MOD_SHIFT: u8 = 0b0000_0001;
const MOD_ALT: u8 = 0b0000_0010;
const MOD_CTRL: u8 = 0b0000_0100;
const MOD_SUPER: u8 = 0b0000_1000;
const MOD_META: u8 = 0b0001_0000;

fn mods_from(bits: u8) -> KeyModifiers {
    let mut mods = KeyModifiers::empty();
    if bits & MOD_SHIFT != 0 {
        mods |= KeyModifiers::SHIFT;
    }
    if bits & MOD_ALT != 0 {
        mods |= KeyModifiers::ALT;
    }
    if bits & MOD_CTRL != 0 {
        mods |= KeyModifiers::CONTROL;
    }
    if bits & MOD_SUPER != 0 {
        mods |= KeyModifiers::SUPER;
    }
    if bits & MOD_META != 0 {
        mods |= KeyModifiers::META;
    }
    mods
}

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn decode_byte(byte: u8) -> KeyEvent {
    let plain = KeyModifiers::empty();
    match byte {
        0x00 => press(KeyCode::Null, plain),
        0x08 | 0x7F => press(KeyCode::Backspace, plain),
        0x09 => press(KeyCode::Tab, plain),
        0x0D => press(KeyCode::Enter, plain),
        0x1B => press(KeyCode::Esc, plain),
        0x01..=0x1A => {
            let ch = char::from(b'a' + byte - 1);
            press(KeyCode::Char(ch), KeyModifiers::CONTROL)
        }
        0x20..=0x7E => press(KeyCode::Char(char::from(byte)), plain),
        0x80..=0x8F => press(nav(byte - 0x80), plain),
        0x90..=0x9F => press(nav(byte - 0x90), KeyModifiers::ALT),
        0xA0..=0xAF => press(nav(byte - 0xA0), KeyModifiers::CONTROL),
        0xB0..=0xC9 => {
            let ch = char::from(b'a' + (byte - 0xB0));
            press(KeyCode::Char(ch), KeyModifiers::ALT)
        }
        0xCA..=0xE3 => {
            let ch = char::from(b'a' + (byte - 0xCA));
            press(KeyCode::Char(ch), KeyModifiers::SUPER)
        }
        0xE4..=0xFD => {
            let ch = char::from(b'a' + (byte - 0xE4));
            press(KeyCode::Char(ch), KeyModifiers::META)
        }
        _ => press(KeyCode::Null, plain),
    }
}

fn nav(id: u8) -> KeyCode {
    match id {
        0 => KeyCode::Up,
        1 => KeyCode::Down,
        2 => KeyCode::Left,
        3 => KeyCode::Right,
        4 => KeyCode::Home,
        5 => KeyCode::End,
        6 => KeyCode::PageUp,
        7 => KeyCode::PageDown,
        8 => KeyCode::Delete,
        9 => KeyCode::BackTab,
        10 => KeyCode::F(6),
        11 => KeyCode::F(18),
        _ => KeyCode::Null,
    }
}

fn decode_extended(rest: &[u8]) -> Option<KeyEvent> {
    let (&bits, rest) = rest.split_first()?;
    let modifiers = mods_from(bits);
    let text = std::str::from_utf8(rest).ok()?;
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(press(KeyCode::Char(ch), modifiers))
}

/// The byte the browser sends for `code` with `modifiers`, when one byte can say it.
pub fn encode(code: KeyCode, modifiers: KeyModifiers) -> Option<u8> {
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let alt = modifiers.contains(KeyModifiers::ALT);
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let super_ = modifiers.contains(KeyModifiers::SUPER);
    let meta = modifiers.contains(KeyModifiers::META);
    let extra = [shift, alt, ctrl, super_, meta]
        .into_iter()
        .filter(|on| *on)
        .count();
    if extra > 1 {
        return None;
    }
    if let KeyCode::Char(ch) = code {
        if !ctrl && !alt && !super_ && !meta && ch.is_ascii() && !ch.is_ascii_control() {
            return Some(ch as u8);
        }
        if ctrl && !alt && !super_ && !meta && !shift && ch.is_ascii_lowercase() {
            return Some((ch as u8) - b'a' + 1);
        }
        if let Some(index) = letter_index(ch) {
            if alt && !ctrl && !super_ && !meta {
                return Some(0xB0 + index);
            }
            if super_ && !alt && !ctrl && !meta {
                return Some(0xCA + index);
            }
            if meta && !alt && !ctrl && !super_ {
                return Some(0xE4 + index);
            }
        }
        return None;
    }
    // These already have a C0 byte. Adding 0x80 would turn Enter into a nav id.
    if extra == 0 {
        let bare = match code {
            KeyCode::Null => Some(0x00),
            KeyCode::Tab => Some(0x09),
            KeyCode::Enter => Some(0x0D),
            KeyCode::Esc => Some(0x1B),
            KeyCode::Backspace => Some(0x7F),
            _ => None,
        };
        if let Some(byte) = bare {
            return Some(byte);
        }
    }
    let id = nav_id(code)?;
    if extra == 0 {
        return Some(0x80 + id);
    }
    if alt {
        return Some(0x90 + id);
    }
    if ctrl {
        return Some(0xA0 + id);
    }
    None
}

fn letter_index(ch: char) -> Option<u8> {
    ch.is_ascii_lowercase().then_some(ch as u8 - b'a')
}

fn nav_id(code: KeyCode) -> Option<u8> {
    Some(match code {
        KeyCode::Up => 0,
        KeyCode::Down => 1,
        KeyCode::Left => 2,
        KeyCode::Right => 3,
        KeyCode::Home => 4,
        KeyCode::End => 5,
        KeyCode::PageUp => 6,
        KeyCode::PageDown => 7,
        KeyCode::Delete => 8,
        KeyCode::BackTab => 9,
        KeyCode::F(6) => 10,
        KeyCode::F(18) => 11,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_the_reader_chords_round_trip() {
        let samples = [
            (KeyCode::Char('j'), KeyModifiers::empty()),
            (KeyCode::Char('G'), KeyModifiers::empty()),
            (KeyCode::Char('?'), KeyModifiers::empty()),
            (KeyCode::Enter, KeyModifiers::empty()),
            (KeyCode::Esc, KeyModifiers::empty()),
            (KeyCode::Char('r'), KeyModifiers::CONTROL),
            (KeyCode::Char('r'), KeyModifiers::SUPER),
            (KeyCode::Char('r'), KeyModifiers::META),
            (KeyCode::Char('b'), KeyModifiers::ALT),
            (KeyCode::Left, KeyModifiers::ALT),
            (KeyCode::Down, KeyModifiers::empty()),
        ];
        for (code, modifiers) in samples {
            let byte = encode(code, modifiers).expect("one byte");
            let key = decode(&[byte]).expect("decoded");
            assert_eq!(key.code, code);
            assert_eq!(key.modifiers, modifiers);
            assert_eq!(key.kind, KeyEventKind::Press);
        }
    }

    #[test]
    fn an_extended_character_keeps_its_modifier() {
        let mut bytes = vec![EXTENDED, MOD_ALT];
        bytes.extend("ò".as_bytes());
        let key = decode(&bytes).expect("decoded");
        assert_eq!(key.code, KeyCode::Char('ò'));
        assert!(key.modifiers.contains(KeyModifiers::ALT));
    }
}
