//! Post length the way x.com counts it: a link is 23 characters, and emoji and wide characters count 2.

use unicode_segmentation::UnicodeSegmentation;

const URL_LEN: usize = 23;
const EMOJI: [(u32, u32); 7] = [
    (0x1F000, 0x1FAFF),
    (0x1F1E6, 0x1F1FF),
    (0x2300, 0x23FF),
    (0x2600, 0x27BF),
    (0x2B00, 0x2BFF),
    (0xFE0F, 0xFE0F),
    (0x200D, 0x200D),
];
const LIGHT: [(u32, u32); 4] = [
    (0x0000, 0x10FF),
    (0x2000, 0x200D),
    (0x2010, 0x201F),
    (0x2032, 0x2037),
];

/// Counts characters the way x.com does: links are 23, emoji graphemes 2, wide ranges 2.
pub fn x_len(text: &str) -> usize {
    let mut total = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let blank = rest.starts_with(char::is_whitespace);
        let end = rest
            .find(|c: char| c.is_whitespace() != blank)
            .unwrap_or(rest.len());
        let (run, tail) = rest.split_at(end);
        total += if blank {
            plain_len(run)
        } else if is_url(run) {
            URL_LEN
        } else {
            plain_len(run)
        };
        rest = tail;
    }
    total
}

fn is_url(token: &str) -> bool {
    let lower = |len: usize| token.get(..len).map(str::to_ascii_lowercase);
    lower(7).as_deref() == Some("http://") || lower(8).as_deref() == Some("https://")
}

fn plain_len(text: &str) -> usize {
    text.graphemes(true).map(grapheme_len).sum()
}

fn grapheme_len(grapheme: &str) -> usize {
    if grapheme.chars().any(|c| in_ranges(c, &EMOJI)) {
        2
    } else {
        grapheme
            .chars()
            .map(|c| if in_ranges(c, &LIGHT) { 1 } else { 2 })
            .sum()
    }
}

fn in_ranges(c: char, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|&(low, high)| (low..=high).contains(&(c as u32)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii() {
        assert_eq!(x_len("hello"), 5);
        assert_eq!(x_len(""), 0);
    }

    #[test]
    fn unicode_bold_letters_count_two() {
        assert_eq!(x_len("𝗛𝗲𝗹𝗹𝗼"), 10);
    }

    #[test]
    fn accents_count_one() {
        assert_eq!(x_len("héllo"), 5);
    }

    #[test]
    fn cjk_counts_two() {
        assert_eq!(x_len("日本"), 4);
    }

    #[test]
    fn urls_count_23() {
        assert_eq!(
            x_len("see https://example.com/a/very/long/path ok"),
            4 + 23 + 3
        );
        assert_eq!(x_len("http://a.co"), 23);
        assert_eq!(x_len("nothttps://a.co"), 15);
    }

    #[test]
    fn emoji_graphemes_count_two() {
        assert_eq!(x_len("👍"), 2);
        assert_eq!(x_len("👨‍👩‍👧‍👦"), 2);
        assert_eq!(x_len("🇮🇹"), 2);
        assert_eq!(x_len("👍🏽"), 2);
        assert_eq!(x_len("❤️"), 2);
    }

    #[test]
    fn curly_quotes_count_one() {
        assert_eq!(x_len("“quotes”"), 8);
    }

    #[test]
    fn whitespace_counts_one() {
        assert_eq!(x_len("a \n b"), 5);
    }
}
