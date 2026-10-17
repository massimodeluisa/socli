//! The browser's screen: a ratatui backend that records the cells that changed.
//!
//! Ratatui diffs, so [`Backend::draw`](ratatui::backend::Backend::draw) only receives the cells
//! that differ from the previous frame. A resize clears that previous frame, and a blank cell
//! then looks unchanged, so the page would keep the old glyph. After a clear the next frame is
//! the whole screen.

use std::io;

use ratatui::backend::{Backend, WindowSize};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use crate::theme::Rgb;

/// Bytes in one cell before an optional longer symbol.
pub const RECORD: usize = 15;

const BOLD: u8 = 1;
const DIM: u8 = 2;
const ITALIC: u8 = 4;
const UNDERLINE: u8 = 8;
const REVERSED: u8 = 16;
/// The symbol occupies the next cell too. The diff does not send that cell, so the page has to skip it.
const WIDE: u8 = 0x20;
/// The symbol has more than one Unicode scalar. A length byte and its UTF-8 follow the record.
const MORE: u8 = 0x80;

/// The screen the page paints, and the cells that changed since the last frame was taken.
pub struct Grid {
    buffer: Buffer,
    dirty: Vec<bool>,
    /// The next frame is every cell, not only the diff. Set when the screen is cleared.
    full: bool,
    cursor: Position,
    cursor_on: bool,
    fg: [u8; 3],
    bg: [u8; 3],
}

impl Grid {
    pub fn new(cols: u16, rows: u16) -> Self {
        let area = Rect::new(0, 0, cols.max(1), rows.max(1));
        let buffer = Buffer::empty(area);
        let dirty = vec![false; buffer.content.len()];
        Self {
            buffer,
            dirty,
            full: false,
            cursor: Position::ORIGIN,
            cursor_on: false,
            fg: [231, 233, 234],
            bg: [0, 0, 0],
        }
    }

    /// Colors for a cell whose own color is [`Color::Reset`].
    pub fn set_fallback(&mut self, fg: Rgb, bg: Rgb) {
        self.fg = [fg.0, fg.1, fg.2];
        self.bg = [bg.0, bg.1, bg.2];
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let area = Rect::new(0, 0, cols.max(1), rows.max(1));
        self.buffer.resize(area);
        self.dirty = vec![false; self.buffer.content.len()];
        self.full = true;
        self.cursor = Position::ORIGIN;
        self.cursor_on = false;
    }

    pub(crate) fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Column and row when the cursor is shown.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.cursor_on.then_some((self.cursor.x, self.cursor.y))
    }

    /// The cells that changed since the last call. After a clear, every cell.
    pub fn take_frame(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let width = self.buffer.area.width;
        let height = self.buffer.area.height;
        for y in 0..height {
            for x in 0..width {
                let index = usize::from(y) * usize::from(width) + usize::from(x);
                if self.full || self.dirty.get(index).copied().unwrap_or(false) {
                    write_cell(&mut out, x, y, &self.buffer[(x, y)], self.fg, self.bg);
                }
                if let Some(flag) = self.dirty.get_mut(index) {
                    *flag = false;
                }
            }
        }
        self.full = false;
        out
    }

    fn paint(&mut self, x: u16, y: u16, cell: &Cell) {
        let Some(index) = self.index(x, y) else {
            return;
        };
        self.buffer[(x, y)] = cell.clone();
        self.dirty[index] = true;
    }

    fn index(&self, x: u16, y: u16) -> Option<usize> {
        let width = self.buffer.area.width;
        if x >= width || y >= self.buffer.area.height {
            return None;
        }
        Some(usize::from(y) * usize::from(width) + usize::from(x))
    }
}

/// The screen as plain text. A wide glyph covers the blank cells after it, and trailing blank
/// lines go away, so a screen reader reads the same words the frame shows.
pub fn plain_text(buffer: &Buffer) -> String {
    let width = buffer.area.width;
    let height = buffer.area.height;
    let mut lines = Vec::with_capacity(usize::from(height));
    for y in 0..height {
        let mut line = String::new();
        let mut cover = 0usize;
        for x in 0..width {
            let cell = &buffer[(x, y)];
            let span = cell.symbol().width();
            if cover == 0 && !cell.skip {
                line.push_str(cell.symbol());
            }
            cover = cover.max(span).saturating_sub(1);
        }
        while line.ends_with(' ') {
            line.pop();
        }
        lines.push(line);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

fn write_cell(out: &mut Vec<u8>, x: u16, y: u16, cell: &Cell, fg: [u8; 3], bg: [u8; 3]) {
    let symbol = if cell.skip { "" } else { cell.symbol() };
    let mut scalars = symbol.chars();
    let first = scalars.next();
    let more = scalars.next().is_some();
    let scalar = first.map(u32::from).unwrap_or(0);
    let paint_fg = color_rgb(cell.fg, fg);
    let paint_bg = color_rgb(cell.bg, bg);
    let mut mods = modifier_byte(cell.modifier);
    // The diff omits the second cell of a wide grapheme. The page skips that column when this is set.
    if symbol.width() >= 2 {
        mods |= WIDE;
    }
    out.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(&y.to_le_bytes());
    out.extend_from_slice(&scalar.to_le_bytes());
    out.extend_from_slice(&paint_fg);
    out.extend_from_slice(&paint_bg);
    if more {
        mods |= MORE;
        out.push(mods);
        let bytes = symbol.as_bytes();
        let len = u8::try_from(bytes.len()).unwrap_or(u8::MAX);
        out.push(len);
        out.extend_from_slice(&bytes[..usize::from(len)]);
    } else {
        out.push(mods);
    }
}

/**
The five flags the page paints. Ratatui keeps reversed at bit 6 and also has blink, hidden and
struck, which this screen does not draw.
*/
fn modifier_byte(modifier: Modifier) -> u8 {
    let mut out = 0;
    if modifier.contains(Modifier::BOLD) {
        out |= BOLD;
    }
    if modifier.contains(Modifier::DIM) {
        out |= DIM;
    }
    if modifier.contains(Modifier::ITALIC) {
        out |= ITALIC;
    }
    if modifier.contains(Modifier::UNDERLINED) {
        out |= UNDERLINE;
    }
    if modifier.contains(Modifier::REVERSED) {
        out |= REVERSED;
    }
    out
}

fn color_rgb(color: Color, fallback: [u8; 3]) -> [u8; 3] {
    match color {
        Color::Reset => fallback,
        Color::Black => ansi(0),
        Color::Red => ansi(1),
        Color::Green => ansi(2),
        Color::Yellow => ansi(3),
        Color::Blue => ansi(4),
        Color::Magenta => ansi(5),
        Color::Cyan => ansi(6),
        Color::Gray => ansi(7),
        Color::DarkGray => ansi(8),
        Color::LightRed => ansi(9),
        Color::LightGreen => ansi(10),
        Color::LightYellow => ansi(11),
        Color::LightBlue => ansi(12),
        Color::LightMagenta => ansi(13),
        Color::LightCyan => ansi(14),
        Color::White => ansi(15),
        Color::Rgb(red, green, blue) => [red, green, blue],
        Color::Indexed(index) => ansi(index),
    }
}

/// The xterm 256-color cube. Indexes 0 to 15 are the usual ANSI colors.
fn ansi(index: u8) -> [u8; 3] {
    const BASIC: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    const STEP: [u8; 6] = [0, 95, 135, 175, 215, 255];
    if index < 16 {
        return BASIC[usize::from(index)];
    }
    if index >= 232 {
        let level = 8 + 10 * (index - 232);
        return [level, level, level];
    }
    let cube = index - 16;
    let red = cube / 36;
    let green = (cube % 36) / 6;
    let blue = cube % 6;
    [
        STEP[usize::from(red)],
        STEP[usize::from(green)],
        STEP[usize::from(blue)],
    ]
}

impl Backend for Grid {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            self.paint(x, y, cell);
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.cursor_on = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.cursor_on = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.cursor = position.into();
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        let area = self.buffer.area;
        self.buffer = Buffer::empty(area);
        self.dirty.fill(false);
        self.full = true;
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.buffer.area.as_size())
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.size()?,
            pixels: Size::new(0, 0),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ratatui::backend::Backend;

    use super::*;

    fn paint(grid: &mut Grid, x: u16, y: u16, symbol: &str, color: Color, modifier: Modifier) {
        let mut cell = Cell::default();
        cell.set_symbol(symbol);
        cell.fg = color;
        cell.bg = Color::Reset;
        cell.modifier = modifier;
        grid.draw([(x, y, &cell)].into_iter()).unwrap();
    }

    type Painted<'a> = (u16, u16, u32, [u8; 3], [u8; 3], u8, Option<&'a [u8]>);

    fn record_at(frame: &[u8], index: usize) -> Painted<'_> {
        let mut at = 0;
        let mut seen = 0;
        while at + RECORD <= frame.len() {
            let col = u16::from_le_bytes([frame[at], frame[at + 1]]);
            let row = u16::from_le_bytes([frame[at + 2], frame[at + 3]]);
            let scalar = u32::from_le_bytes(frame[at + 4..at + 8].try_into().unwrap());
            let fg = [frame[at + 8], frame[at + 9], frame[at + 10]];
            let bg = [frame[at + 11], frame[at + 12], frame[at + 13]];
            let mods = frame[at + 14];
            at += RECORD;
            let extra = if mods & MORE == 0 {
                None
            } else {
                let len = usize::from(frame[at]);
                let bytes = &frame[at + 1..at + 1 + len];
                at += 1 + len;
                Some(bytes)
            };
            if seen == index {
                return (col, row, scalar, fg, bg, mods, extra);
            }
            seen += 1;
        }
        panic!("record {index} missing");
    }

    #[test]
    fn a_changed_cell_is_one_record_and_reset_uses_the_fallback() {
        let mut grid = Grid::new(4, 2);
        grid.set_fallback(Rgb(1, 2, 3), Rgb(4, 5, 6));
        paint(
            &mut grid,
            1,
            0,
            "A",
            Color::Reset,
            Modifier::BOLD | Modifier::REVERSED,
        );
        let frame = grid.take_frame();
        assert_eq!(frame.len(), RECORD);
        let (col, row, scalar, fg, bg, mods, extra) = record_at(&frame, 0);
        assert_eq!((col, row, scalar), (1, 0, u32::from('A')));
        assert_eq!(fg, [1, 2, 3]);
        assert_eq!(bg, [4, 5, 6]);
        assert_eq!(mods, BOLD | REVERSED);
        assert!(extra.is_none());
        assert!(grid.take_frame().is_empty());
    }

    #[test]
    fn a_wide_symbol_marks_the_cell_the_page_skips() {
        let mut grid = Grid::new(4, 1);
        paint(&mut grid, 0, 0, "✨", Color::White, Modifier::empty());
        paint(&mut grid, 1, 0, "A", Color::White, Modifier::BOLD);
        let frame = grid.take_frame();
        let (_, _, spark, _, _, spark_mods, spark_extra) = record_at(&frame, 0);
        let (_, _, letter, _, _, letter_mods, _) = record_at(&frame, 1);
        assert_eq!(spark, u32::from('✨'));
        assert!(spark_extra.is_none());
        assert_eq!(spark_mods & WIDE, WIDE);
        assert_eq!(letter, u32::from('A'));
        assert_eq!(letter_mods & WIDE, 0);
        assert_eq!(letter_mods & BOLD, BOLD);
    }

    #[test]
    fn a_zwj_emoji_is_wide_and_carries_the_cluster() {
        let mut grid = Grid::new(3, 1);
        paint(
            &mut grid,
            0,
            0,
            "👩‍💻",
            Color::Rgb(9, 8, 7),
            Modifier::empty(),
        );
        let frame = grid.take_frame();
        let (_, _, _, _, _, mods, extra) = record_at(&frame, 0);
        assert_eq!(mods & MORE, MORE);
        assert_eq!(mods & WIDE, WIDE);
        assert_eq!(extra, Some("👩‍💻".as_bytes()));
    }

    #[test]
    fn a_longer_symbol_follows_the_record() {
        let mut grid = Grid::new(3, 1);
        paint(
            &mut grid,
            0,
            0,
            "👩‍💻",
            Color::Rgb(9, 8, 7),
            Modifier::empty(),
        );
        let frame = grid.take_frame();
        let (col, row, scalar, fg, _, mods, extra) = record_at(&frame, 0);
        assert_eq!((col, row, scalar), (0, 0, u32::from('👩')));
        assert_eq!(fg, [9, 8, 7]);
        assert_eq!(mods & MORE, MORE);
        assert_eq!(extra, Some("👩‍💻".as_bytes()));
    }

    #[test]
    fn indexed_red_is_the_cube_red() {
        let mut grid = Grid::new(1, 1);
        paint(&mut grid, 0, 0, "x", Color::Indexed(196), Modifier::empty());
        let (_, _, _, fg, _, _, _) = record_at(&grid.take_frame(), 0);
        assert_eq!(fg, [255, 0, 0]);
    }

    #[test]
    fn a_clear_sends_the_whole_screen() {
        let mut grid = Grid::new(2, 1);
        paint(&mut grid, 0, 0, "Z", Color::White, Modifier::empty());
        let _ = grid.take_frame();
        grid.clear().unwrap();
        let frame = grid.take_frame();
        assert_eq!(frame.len(), RECORD * 2);
    }

    #[test]
    fn plain_text_trims_and_lets_a_wide_glyph_cover_the_next_cell() {
        let mut grid = Grid::new(4, 2);
        paint(&mut grid, 0, 0, "あ", Color::Reset, Modifier::empty());
        paint(&mut grid, 2, 0, "b", Color::Reset, Modifier::empty());
        assert_eq!(plain_text(grid.buffer()), "あb");
    }
}
