//! `<b>` and `<i>` in post text become Unicode sans-serif bold and italic letters.

const BOLD: (u32, u32) = (0x1D5D4, 0x1D5EE);
const ITALIC: (u32, u32) = (0x1D608, 0x1D622);
const BOLD_ITALIC: (u32, u32) = (0x1D63C, 0x1D656);
const BOLD_DIGITS: u32 = 0x1D7EC;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Bold,
    Italic,
}

/// Turns `<b>`, `<strong>`, `<i>` and `<em>` into Unicode sans-serif bold and italic.
pub fn convert(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut bold, mut italic) = (0usize, 0usize);
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.extend(
            rest[..start]
                .chars()
                .map(|c| styled(c, bold > 0, italic > 0)),
        );
        rest = &rest[start..];
        let close = rest.find('>').filter(|&end| !rest[1..end].contains('<'));
        let tag = close.and_then(|end| {
            parse(&rest[1..end].to_ascii_lowercase(), bold, italic).map(|tag| (end, tag))
        });
        match (tag, close) {
            (Some((end, (style, opens))), _) => {
                let depth = if style == Style::Bold {
                    &mut bold
                } else {
                    &mut italic
                };
                *depth = if opens { *depth + 1 } else { *depth - 1 };
                rest = &rest[end + 1..];
            }
            (None, Some(end)) => {
                out.push_str(&rest[..=end]);
                rest = &rest[end + 1..];
            }
            (None, None) => {
                out.push('<');
                rest = &rest[1..];
            }
        }
    }
    out.extend(rest.chars().map(|c| styled(c, bold > 0, italic > 0)));
    out
}

/** Returns the style and whether the tag opens it; a closer with nothing open is not a tag. */
fn parse(name: &str, bold: usize, italic: usize) -> Option<(Style, bool)> {
    match name {
        "b" | "strong" => Some((Style::Bold, true)),
        "i" | "em" => Some((Style::Italic, true)),
        "/b" | "/strong" if bold > 0 => Some((Style::Bold, false)),
        "/i" | "/em" if italic > 0 => Some((Style::Italic, false)),
        _ => None,
    }
}

fn styled(c: char, bold: bool, italic: bool) -> char {
    let (upper, lower) = match (bold, italic) {
        (false, false) => return c,
        (true, false) => BOLD,
        (false, true) => ITALIC,
        (true, true) => BOLD_ITALIC,
    };
    let offset = |base: u32, first: char| base + (c as u32 - first as u32);
    let mapped = match c {
        'A'..='Z' => offset(upper, 'A'),
        'a'..='z' => offset(lower, 'a'),
        '0'..='9' if bold => offset(BOLD_DIGITS, '0'),
        _ => return c,
    };
    char::from_u32(mapped).unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold() {
        assert_eq!(convert("<b>Hello</b> world!"), "𝗛𝗲𝗹𝗹𝗼 world!");
    }

    #[test]
    fn italic_leaves_digits() {
        assert_eq!(convert("<i>abc 123</i>"), "𝘢𝘣𝘤 123");
    }

    #[test]
    fn nested_is_bold_italic_with_bold_digits() {
        assert_eq!(convert("<b><i>Ab1</i></b>"), "𝘼𝙗𝟭");
        assert_eq!(convert("<i><b>Ab1</b></i>"), "𝘼𝙗𝟭");
    }

    #[test]
    fn partial_nesting_returns_to_the_outer_style() {
        assert_eq!(convert("<b>a<i>b</i>c</b>"), "𝗮𝙗𝗰");
    }

    #[test]
    fn tags_are_case_insensitive() {
        assert_eq!(convert("<B>x</B>"), "𝘅");
        assert_eq!(convert("<StRoNg>x</STRONG>"), "𝘅");
    }

    #[test]
    fn strong_and_em_are_aliases() {
        assert_eq!(convert("<strong>x</strong>"), convert("<b>x</b>"));
        assert_eq!(convert("<em>x</em>"), convert("<i>x</i>"));
    }

    #[test]
    fn non_ascii_is_unchanged() {
        assert_eq!(convert("<b>é日👍</b>"), "é日👍");
    }

    #[test]
    fn unclosed_tag_runs_to_the_end() {
        assert_eq!(convert("<b>abc"), "𝗮𝗯𝗰");
        assert_eq!(convert("x <i>ab"), "x 𝘢𝘣");
    }

    #[test]
    fn stray_closer_is_literal() {
        assert_eq!(convert("a </b> b"), "a </b> b");
        assert_eq!(convert("<i>a</b>"), "𝘢</b>");
    }

    #[test]
    fn unknown_tags_are_literal() {
        assert_eq!(convert("<u>x</u>"), "<u>x</u>");
        assert_eq!(convert("<b class=x>y</b>"), "<b class=x>y</b>");
    }

    #[test]
    fn angle_brackets_are_literal() {
        assert_eq!(convert("a < b > c"), "a < b > c");
        assert_eq!(convert("<"), "<");
        assert_eq!(convert("<b"), "<b");
    }

    #[test]
    fn empty_string() {
        assert_eq!(convert(""), "");
    }
}
