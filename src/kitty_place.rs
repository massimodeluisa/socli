//! One kitty placeholder per cell.

use std::io::{self, Write};

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Color;

/**
Kitty places an image with one private-use character per cell. ratatui-image 9 packs a whole
row, including the image transmit, into the first cell and skips the rest. That cell is wider
than one column, so the next cell is left out of the frame diff and the picture tears while
the feed scrolls. The table is the one kitty publishes for those row and column marks.
*/
static DIACRITICS: [char; 297] = [
    '\u{305}',
    '\u{30D}',
    '\u{30E}',
    '\u{310}',
    '\u{312}',
    '\u{33D}',
    '\u{33E}',
    '\u{33F}',
    '\u{346}',
    '\u{34A}',
    '\u{34B}',
    '\u{34C}',
    '\u{350}',
    '\u{351}',
    '\u{352}',
    '\u{357}',
    '\u{35B}',
    '\u{363}',
    '\u{364}',
    '\u{365}',
    '\u{366}',
    '\u{367}',
    '\u{368}',
    '\u{369}',
    '\u{36A}',
    '\u{36B}',
    '\u{36C}',
    '\u{36D}',
    '\u{36E}',
    '\u{36F}',
    '\u{483}',
    '\u{484}',
    '\u{485}',
    '\u{486}',
    '\u{487}',
    '\u{592}',
    '\u{593}',
    '\u{594}',
    '\u{595}',
    '\u{597}',
    '\u{598}',
    '\u{599}',
    '\u{59C}',
    '\u{59D}',
    '\u{59E}',
    '\u{59F}',
    '\u{5A0}',
    '\u{5A1}',
    '\u{5A8}',
    '\u{5A9}',
    '\u{5AB}',
    '\u{5AC}',
    '\u{5AF}',
    '\u{5C4}',
    '\u{610}',
    '\u{611}',
    '\u{612}',
    '\u{613}',
    '\u{614}',
    '\u{615}',
    '\u{616}',
    '\u{617}',
    '\u{657}',
    '\u{658}',
    '\u{659}',
    '\u{65A}',
    '\u{65B}',
    '\u{65D}',
    '\u{65E}',
    '\u{6D6}',
    '\u{6D7}',
    '\u{6D8}',
    '\u{6D9}',
    '\u{6DA}',
    '\u{6DB}',
    '\u{6DC}',
    '\u{6DF}',
    '\u{6E0}',
    '\u{6E1}',
    '\u{6E2}',
    '\u{6E4}',
    '\u{6E7}',
    '\u{6E8}',
    '\u{6EB}',
    '\u{6EC}',
    '\u{730}',
    '\u{732}',
    '\u{733}',
    '\u{735}',
    '\u{736}',
    '\u{73A}',
    '\u{73D}',
    '\u{73F}',
    '\u{740}',
    '\u{741}',
    '\u{743}',
    '\u{745}',
    '\u{747}',
    '\u{749}',
    '\u{74A}',
    '\u{7EB}',
    '\u{7EC}',
    '\u{7ED}',
    '\u{7EE}',
    '\u{7EF}',
    '\u{7F0}',
    '\u{7F1}',
    '\u{7F3}',
    '\u{816}',
    '\u{817}',
    '\u{818}',
    '\u{819}',
    '\u{81B}',
    '\u{81C}',
    '\u{81D}',
    '\u{81E}',
    '\u{81F}',
    '\u{820}',
    '\u{821}',
    '\u{822}',
    '\u{823}',
    '\u{825}',
    '\u{826}',
    '\u{827}',
    '\u{829}',
    '\u{82A}',
    '\u{82B}',
    '\u{82C}',
    '\u{82D}',
    '\u{951}',
    '\u{953}',
    '\u{954}',
    '\u{F82}',
    '\u{F83}',
    '\u{F86}',
    '\u{F87}',
    '\u{135D}',
    '\u{135E}',
    '\u{135F}',
    '\u{17DD}',
    '\u{193A}',
    '\u{1A17}',
    '\u{1A75}',
    '\u{1A76}',
    '\u{1A77}',
    '\u{1A78}',
    '\u{1A79}',
    '\u{1A7A}',
    '\u{1A7B}',
    '\u{1A7C}',
    '\u{1B6B}',
    '\u{1B6D}',
    '\u{1B6E}',
    '\u{1B6F}',
    '\u{1B70}',
    '\u{1B71}',
    '\u{1B72}',
    '\u{1B73}',
    '\u{1CD0}',
    '\u{1CD1}',
    '\u{1CD2}',
    '\u{1CDA}',
    '\u{1CDB}',
    '\u{1CE0}',
    '\u{1DC0}',
    '\u{1DC1}',
    '\u{1DC3}',
    '\u{1DC4}',
    '\u{1DC5}',
    '\u{1DC6}',
    '\u{1DC7}',
    '\u{1DC8}',
    '\u{1DC9}',
    '\u{1DCB}',
    '\u{1DCC}',
    '\u{1DD1}',
    '\u{1DD2}',
    '\u{1DD3}',
    '\u{1DD4}',
    '\u{1DD5}',
    '\u{1DD6}',
    '\u{1DD7}',
    '\u{1DD8}',
    '\u{1DD9}',
    '\u{1DDA}',
    '\u{1DDB}',
    '\u{1DDC}',
    '\u{1DDD}',
    '\u{1DDE}',
    '\u{1DDF}',
    '\u{1DE0}',
    '\u{1DE1}',
    '\u{1DE2}',
    '\u{1DE3}',
    '\u{1DE4}',
    '\u{1DE5}',
    '\u{1DE6}',
    '\u{1DFE}',
    '\u{20D0}',
    '\u{20D1}',
    '\u{20D4}',
    '\u{20D5}',
    '\u{20D6}',
    '\u{20D7}',
    '\u{20DB}',
    '\u{20DC}',
    '\u{20E1}',
    '\u{20E7}',
    '\u{20E9}',
    '\u{20F0}',
    '\u{2CEF}',
    '\u{2CF0}',
    '\u{2CF1}',
    '\u{2DE0}',
    '\u{2DE1}',
    '\u{2DE2}',
    '\u{2DE3}',
    '\u{2DE4}',
    '\u{2DE5}',
    '\u{2DE6}',
    '\u{2DE7}',
    '\u{2DE8}',
    '\u{2DE9}',
    '\u{2DEA}',
    '\u{2DEB}',
    '\u{2DEC}',
    '\u{2DED}',
    '\u{2DEE}',
    '\u{2DEF}',
    '\u{2DF0}',
    '\u{2DF1}',
    '\u{2DF2}',
    '\u{2DF3}',
    '\u{2DF4}',
    '\u{2DF5}',
    '\u{2DF6}',
    '\u{2DF7}',
    '\u{2DF8}',
    '\u{2DF9}',
    '\u{2DFA}',
    '\u{2DFB}',
    '\u{2DFC}',
    '\u{2DFD}',
    '\u{2DFE}',
    '\u{2DFF}',
    '\u{A66F}',
    '\u{A67C}',
    '\u{A67D}',
    '\u{A6F0}',
    '\u{A6F1}',
    '\u{A8E0}',
    '\u{A8E1}',
    '\u{A8E2}',
    '\u{A8E3}',
    '\u{A8E4}',
    '\u{A8E5}',
    '\u{A8E6}',
    '\u{A8E7}',
    '\u{A8E8}',
    '\u{A8E9}',
    '\u{A8EA}',
    '\u{A8EB}',
    '\u{A8EC}',
    '\u{A8ED}',
    '\u{A8EE}',
    '\u{A8EF}',
    '\u{A8F0}',
    '\u{A8F1}',
    '\u{AAB0}',
    '\u{AAB2}',
    '\u{AAB3}',
    '\u{AAB7}',
    '\u{AAB8}',
    '\u{AABE}',
    '\u{AABF}',
    '\u{AAC1}',
    '\u{FE20}',
    '\u{FE21}',
    '\u{FE22}',
    '\u{FE23}',
    '\u{FE24}',
    '\u{FE25}',
    '\u{FE26}',
    '\u{10A0F}',
    '\u{10A38}',
    '\u{1D185}',
    '\u{1D186}',
    '\u{1D187}',
    '\u{1D188}',
    '\u{1D189}',
    '\u{1D1AA}',
    '\u{1D1AB}',
    '\u{1D1AC}',
    '\u{1D1AD}',
    '\u{1D242}',
    '\u{1D243}',
    '\u{1D244}',
];

const PLACEHOLDER: char = '\u{10EEEE}';
const SAVE: &str = "\u{1b}[s";
const COLOR: &str = "\u{1b}[38;2;";

struct Place {
    transmit: String,
    fg: Color,
    row: char,
    id: char,
}

fn diacritic(index: u16) -> char {
    DIACRITICS
        .get(usize::from(index))
        .copied()
        .unwrap_or(DIACRITICS[0])
}

/// The first save-cursor in a packed kitty row, then the id color and the first placeholder's marks.
fn parse_place(symbol: &str) -> Option<Place> {
    let save = symbol.find(SAVE)?;
    let transmit = symbol[..save].to_string();
    let rest = &symbol[save + SAVE.len()..];
    let color_at = rest.find(COLOR)?;
    let nums = &rest[color_at + COLOR.len()..];
    let end = nums.find('m')?;
    let mut parts = nums[..end].split(';');
    let r = parts.next()?.parse().ok()?;
    let g = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let mut glyphs = nums[end + 1..].chars();
    if glyphs.next() != Some(PLACEHOLDER) {
        return None;
    }
    let row = glyphs.next()?;
    let _column = glyphs.next()?;
    let id = glyphs.next()?;
    Some(Place {
        transmit,
        fg: Color::Rgb(r, g, b),
        row,
        id,
    })
}

/**
Rewrites a packed kitty image in `area` so each cell holds one width-1 placeholder.
`None` means the area is not a kitty image. The string is the transmit sequence, empty when
this placement was already sent.
*/
pub(crate) fn settle(buf: &mut Buffer, area: Rect) -> Option<String> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let mut rows = Vec::with_capacity(usize::from(area.height));
    for y in 0..area.height {
        let pos = Position::new(area.x, area.y.saturating_add(y));
        let symbol = buf.cell(pos)?.symbol().to_string();
        rows.push(parse_place(&symbol)?);
    }
    let mut transmit = String::new();
    for row in &rows {
        if !row.transmit.is_empty() {
            transmit.clone_from(&row.transmit);
            break;
        }
    }
    for (y, place) in rows.iter().enumerate() {
        let Ok(y) = u16::try_from(y) else {
            continue;
        };
        let top = area.y.saturating_add(y);
        for x in 0..area.width {
            let Some(cell) = buf.cell_mut(Position::new(area.x.saturating_add(x), top)) else {
                continue;
            };
            let mut text = String::new();
            text.push(PLACEHOLDER);
            text.push(place.row);
            text.push(diacritic(x));
            text.push(place.id);
            cell.set_symbol(&text);
            cell.set_fg(place.fg);
            cell.set_skip(false);
        }
    }
    Some(transmit)
}

/// Sends `data` before the frame that places the image. Tests never write to the terminal.
pub(crate) fn emit(data: &str) {
    if data.is_empty() || cfg!(test) {
        return;
    }
    let mut out = io::stdout().lock();
    let _ = out.write_all(data.as_bytes());
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn a_packed_kitty_row_becomes_one_column_placeholders() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        let mut symbol = String::from("TRANSMIT");
        symbol.push_str(SAVE);
        symbol.push_str(COLOR);
        symbol.push_str("1;2;3m");
        symbol.push(PLACEHOLDER);
        symbol.push(diacritic(0));
        symbol.push(diacritic(0));
        symbol.push(diacritic(7));
        symbol.extend(std::iter::repeat_n(PLACEHOLDER, 4));
        symbol.push_str("\u{1b}[u\u{1b}[4C\u{1b}[1B");
        buf.cell_mut(Position::new(0, 0))
            .expect("cell")
            .set_symbol(&symbol);
        for x in 1..5 {
            buf.cell_mut(Position::new(x, 0))
                .expect("cell")
                .set_skip(true);
        }
        let transmit = settle(&mut buf, Rect::new(0, 0, 5, 1)).expect("kitty");
        assert_eq!(transmit, "TRANSMIT");
        for x in 0..5 {
            let cell = buf.cell(Position::new(x, 0)).expect("cell");
            assert!(!cell.skip);
            assert_eq!(cell.symbol().width(), 1, "{x}");
            assert!(cell.symbol().starts_with(PLACEHOLDER));
            assert_eq!(cell.fg, Color::Rgb(1, 2, 3));
        }
        assert_ne!(
            buf.cell(Position::new(0, 0)).expect("cell").symbol(),
            buf.cell(Position::new(1, 0)).expect("cell").symbol()
        );
        assert_eq!(diacritic(57).to_string().width(), 0);
    }

    #[test]
    fn a_text_cell_is_left_alone() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        buf.cell_mut(Position::new(0, 0))
            .expect("cell")
            .set_symbol("N");
        assert!(settle(&mut buf, Rect::new(0, 0, 1, 1)).is_none());
        assert_eq!(buf.cell(Position::new(0, 0)).expect("cell").symbol(), "N");
    }
}
