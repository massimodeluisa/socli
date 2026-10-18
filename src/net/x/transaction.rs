//! The `x-client-transaction-id` header the X web client sends on writes.
//!
//! CreateBookmark answers 404 in a few milliseconds without it and `Done` with it. The value is
//! not a user secret: it is a hash of the request path, the public site-verification key, and an
//! animation key read from the home page SVG. The steps match the web client's own math.

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine;
use sha2::{Digest, Sha256};

const KEYWORD: &str = "obfiowerehiring";
const EXTRA_BYTE: u8 = 3;
const EPOCH_MS: u64 = 1_682_924_400_000;

#[derive(Clone)]
pub struct Transaction {
    key_bytes: Vec<u8>,
    animation_key: String,
}

impl Transaction {
    pub fn from_page(html: &str, ondemand_js: &str) -> Option<Self> {
        let key = verification_key(html)?;
        let key_bytes = decode_key(&key)?;
        let indices = key_indices(ondemand_js)?;
        let frames = anim_paths(html)?;
        let animation_key = animation_key(&key_bytes, &indices, &frames)?;
        Some(Self {
            key_bytes,
            animation_key,
        })
    }

    pub fn header(&self, method: &str, path: &str) -> String {
        let random = (now_millis() & 0xff) as u8;
        self.header_at(method, path, now_secs(), random)
    }

    fn header_at(&self, method: &str, path: &str, time_now: u64, random: u8) -> String {
        let time_bytes = (0..4).map(|i| ((time_now >> (i * 8)) & 0xff) as u8);
        let hash = Sha256::digest(format!(
            "{method}!{path}!{time_now}{KEYWORD}{}",
            self.animation_key
        ));
        let mut bytes = Vec::with_capacity(self.key_bytes.len() + 4 + 16 + 1);
        bytes.extend_from_slice(&self.key_bytes);
        bytes.extend(time_bytes);
        bytes.extend_from_slice(&hash[..16]);
        bytes.push(EXTRA_BYTE);
        let mut out = Vec::with_capacity(bytes.len() + 1);
        out.push(random);
        out.extend(bytes.iter().map(|byte| byte ^ random));
        STANDARD.encode(out).trim_end_matches('=').to_string()
    }
}

pub fn ondemand_url(html: &str) -> Option<String> {
    let index = ondemand_index(html)?;
    let needle = format!(",{index}:\"");
    let mut from = 0;
    while let Some(rel) = html[from..].find(&needle) {
        let start = from + rel + needle.len();
        let rest = html.get(start..)?;
        let end = rest
            .find(|ch: char| !matches!(ch, '0'..='9' | 'a'..='f'))
            .unwrap_or(rest.len());
        if end > 0 && rest.as_bytes().get(end) == Some(&b'"') {
            return Some(format!(
                "https://abs.twimg.com/responsive-web/client-web/ondemand.s.{}a.js",
                &rest[..end]
            ));
        }
        from = start;
    }
    None
}

/// `,123:"ondemand.s"` in the webpack map. The hash lives in a second entry with the same index.
fn ondemand_index(html: &str) -> Option<u32> {
    let bytes = html.as_bytes();
    let mut i = 0;
    while i + 14 < bytes.len() {
        if bytes[i] == b',' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let Some(quote) = bytes.get(j + 1).copied() else {
                i += 1;
                continue;
            };
            if j > start && bytes.get(j) == Some(&b':') && (quote == b'"' || quote == b'\'') {
                let name_at = j + 2;
                if html.get(name_at..name_at + 10) == Some("ondemand.s")
                    && bytes.get(name_at + 10) == Some(&quote)
                {
                    return html.get(start..j)?.parse().ok();
                }
            }
        }
        i += 1;
    }
    None
}

fn verification_key(html: &str) -> Option<String> {
    let marker = "twitter-site-verification";
    let at = html.find(marker)?;
    let tag_start = html[..at].rfind('<').unwrap_or(0);
    let tag_end = html[at..]
        .find('>')
        .map(|end| at + end)
        .unwrap_or(html.len());
    let tag = html.get(tag_start..tag_end)?;
    let (key, quote) = if tag.contains("content=\"") {
        ("content=\"", '"')
    } else if tag.contains("content='") {
        ("content='", '\'')
    } else {
        return None;
    };
    let content_at = tag.find(key)? + key.len();
    let value = tag.get(content_at..)?;
    let end = value.find(quote)?;
    Some(value[..end].to_string())
}

fn decode_key(key: &str) -> Option<Vec<u8>> {
    STANDARD
        .decode(key)
        .or_else(|_| STANDARD_NO_PAD.decode(key))
        .ok()
}

/// The ondemand script's `(t[n], 16)` runs. A consecutive run keeps only its last index, which
/// is the one the web client's regex group retains.
fn key_indices(js: &str) -> Option<Vec<usize>> {
    let bytes = js.as_bytes();
    let mut indices = Vec::new();
    let mut i = 0;
    while i + 6 < bytes.len() {
        if let Some((index, next)) = match_index(bytes, i) {
            let mut last = index;
            let mut cursor = next;
            while let Some((index, next)) = match_index(bytes, cursor) {
                last = index;
                cursor = next;
            }
            indices.push(last);
            i = cursor;
            continue;
        }
        i += 1;
    }
    if indices.is_empty() {
        None
    } else {
        Some(indices)
    }
}

fn match_index(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    if i + 6 >= bytes.len() || bytes[i] != b'(' {
        return None;
    }
    if !bytes[i + 1].is_ascii_alphanumeric() || bytes.get(i + 2) != Some(&b'[') {
        return None;
    }
    let start = i + 3;
    let mut j = start;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j == start || bytes.get(j) != Some(&b']') || bytes.get(j + 1) != Some(&b',') {
        return None;
    }
    let mut k = j + 2;
    while k < bytes.len() && bytes[k].is_ascii_whitespace() {
        k += 1;
    }
    if bytes.get(k..k + 2) != Some(b"16".as_ref()) || bytes.get(k + 2) != Some(&b')') {
        return None;
    }
    let index = std::str::from_utf8(bytes.get(start..j)?)
        .ok()?
        .parse()
        .ok()?;
    Some((index, k + 3))
}

fn anim_paths(html: &str) -> Option<Vec<String>> {
    let mut paths = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("loading-x-anim-") {
        rest = &rest[at..];
        let end = rest[1..]
            .find("loading-x-anim-")
            .map(|next| next + 1)
            .unwrap_or(rest.len());
        let svg = &rest[..end];
        let mut found = svg.match_indices("d=\"");
        let _first = found.next();
        let second = found.next().map(|(pos, _)| pos);
        if let Some(pos) = second {
            let value = &svg[pos + 3..];
            let stop = value.find('"')?;
            paths.push(value[..stop].to_string());
        }
        if end >= rest.len() {
            break;
        }
        rest = &rest[end..];
    }
    if paths.is_empty() {
        None
    } else {
        Some(paths)
    }
}

fn animation_key(key_bytes: &[u8], indices: &[usize], paths: &[String]) -> Option<String> {
    if key_bytes.len() < 6 || indices.is_empty() || paths.is_empty() {
        return None;
    }
    let frame = paths.get(key_bytes[5] as usize % 4)?;
    let rows = path_rows(frame);
    let row_index = *key_bytes.get(*indices.first()?)? as usize % 16;
    let mut frame_time = 1u32;
    for index in indices.iter().skip(1) {
        let byte = *key_bytes.get(*index)? as u32 % 16;
        frame_time = frame_time.saturating_mul(byte);
    }
    let frame_time = js_round(f64::from(frame_time) / 10.0) * 10.0;
    let row = rows.get(row_index)?;
    Some(animate(row, frame_time / 4096.0))
}

fn path_rows(path: &str) -> Vec<Vec<i64>> {
    let body = path.get(9..).unwrap_or("");
    body.split('C')
        .map(|item| {
            item.split(|ch: char| !ch.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .filter_map(|part| part.parse().ok())
                .collect()
        })
        .collect()
}

fn animate(frames: &[i64], target_time: f64) -> String {
    let from_color = [
        frames.first().copied().unwrap_or(0) as f64,
        frames.get(1).copied().unwrap_or(0) as f64,
        frames.get(2).copied().unwrap_or(0) as f64,
        1.0,
    ];
    let to_color = [
        frames.get(3).copied().unwrap_or(0) as f64,
        frames.get(4).copied().unwrap_or(0) as f64,
        frames.get(5).copied().unwrap_or(0) as f64,
        1.0,
    ];
    let to_rotation = solve(
        frames.get(6).copied().unwrap_or(0) as f64,
        60.0,
        360.0,
        true,
    );
    let curves: Vec<f64> = frames
        .iter()
        .skip(7)
        .enumerate()
        .map(|(count, value)| {
            solve(
                *value as f64,
                if count % 2 == 1 { -1.0 } else { 0.0 },
                1.0,
                false,
            )
        })
        .collect();
    let progress = cubic_value(&curves, target_time);
    let color = interpolate(&from_color, &to_color, progress);
    let color: Vec<f64> = color
        .into_iter()
        .map(|value| value.clamp(0.0, 255.0))
        .collect();
    let rotation = interpolate(&[0.0], &[to_rotation], progress);
    let matrix = rotation_matrix(rotation.first().copied().unwrap_or(0.0));
    let mut parts = Vec::new();
    for value in color.iter().take(color.len().saturating_sub(1)) {
        let rounded = py_round(*value) as i64;
        parts.push(format!("{rounded:x}"));
    }
    for value in matrix {
        let mut rounded = py_round_digits(value, 2);
        if rounded < 0.0 {
            rounded = -rounded;
        }
        let hex = float_to_hex(rounded);
        if hex.starts_with('.') {
            parts.push(format!("0{hex}").to_lowercase());
        } else if hex.is_empty() {
            parts.push("0".to_string());
        } else {
            parts.push(hex);
        }
    }
    parts.extend(["0".to_string(), "0".to_string()]);
    parts.concat().replace(['.', '-'], "")
}

fn cubic_value(curves: &[f64], time: f64) -> f64 {
    if curves.len() < 4 {
        return time;
    }
    if time <= 0.0 {
        let gradient = if curves[0] > 0.0 {
            curves[1] / curves[0]
        } else if curves[1] == 0.0 && curves[2] > 0.0 {
            curves[3] / curves[2]
        } else {
            0.0
        };
        return gradient * time;
    }
    if time >= 1.0 {
        let gradient = if curves[2] < 1.0 {
            (curves[3] - 1.0) / (curves[2] - 1.0)
        } else if curves[2] == 1.0 && curves[0] < 1.0 {
            (curves[1] - 1.0) / (curves[0] - 1.0)
        } else {
            0.0
        };
        return 1.0 + gradient * (time - 1.0);
    }
    let mut start = 0.0;
    let mut end = 1.0;
    let mut mid = 0.0;
    for _ in 0..64 {
        if start >= end {
            break;
        }
        mid = (start + end) / 2.0;
        let estimate = cubic_calc(curves[0], curves[2], mid);
        if (time - estimate).abs() < 0.00001 {
            return cubic_calc(curves[1], curves[3], mid);
        }
        if estimate < time {
            start = mid;
        } else {
            end = mid;
        }
    }
    cubic_calc(curves[1], curves[3], mid)
}

fn cubic_calc(a: f64, b: f64, m: f64) -> f64 {
    3.0 * a * (1.0 - m) * (1.0 - m) * m + 3.0 * b * (1.0 - m) * m * m + m * m * m
}

fn interpolate(from: &[f64], to: &[f64], factor: f64) -> Vec<f64> {
    from.iter()
        .zip(to)
        .map(|(start, end)| start * (1.0 - factor) + end * factor)
        .collect()
}

fn rotation_matrix(degrees: f64) -> [f64; 4] {
    let rad = degrees.to_radians();
    [rad.cos(), -rad.sin(), rad.sin(), rad.cos()]
}

fn solve(value: f64, min: f64, max: f64, rounding: bool) -> f64 {
    let result = value * (max - min) / 255.0 + min;
    // The rotation flag is `Math.floor` in the web client. The curve points use Python `round`.
    if rounding {
        result.floor()
    } else {
        py_round_digits(result, 2)
    }
}

/// JavaScript `Math.round`: half away from zero for the positive frame time.
fn js_round(value: f64) -> f64 {
    let base = value.floor();
    let rounded = if value - base >= 0.5 {
        value.ceil()
    } else {
        base
    };
    if value.is_sign_negative() && rounded == 0.0 {
        -0.0
    } else {
        rounded.copysign(value)
    }
}

/// Python 3 `round`, half to even.
fn py_round(value: f64) -> f64 {
    let floor = value.floor();
    let diff = value - floor;
    let even = (floor as i64).rem_euclid(2) == 0;
    if diff > 0.5 || (diff == 0.5 && !even) {
        floor + 1.0
    } else {
        floor
    }
}

fn py_round_digits(value: f64, digits: u32) -> f64 {
    let scale = 10_f64.powi(digits as i32);
    py_round(value * scale) / scale
}

/// The web client's hex for a number, including the empty result for 0.
fn float_to_hex(mut value: f64) -> String {
    let mut result = String::new();
    let mut quotient = value as i64;
    let fraction = value - quotient as f64;
    while quotient > 0 {
        quotient = (value / 16.0) as i64;
        let remainder = (value - (quotient as f64) * 16.0) as i64;
        if remainder > 9 {
            result.insert(0, char::from_u32((remainder + 55) as u32).unwrap_or('0'));
        } else {
            result.insert_str(0, &remainder.to_string());
        }
        value = quotient as f64;
    }
    if fraction == 0.0 {
        return result;
    }
    result.push('.');
    let mut fraction = fraction;
    // Stop once the fraction is gone. The cap only breaks a value that never reaches zero.
    for _ in 0..64 {
        if fraction <= 0.0 {
            break;
        }
        fraction *= 16.0;
        let integer = fraction as i64;
        fraction -= integer as f64;
        if integer > 9 {
            result.push(char::from_u32((integer + 55) as u32).unwrap_or('0'));
        } else {
            result.push_str(&integer.to_string());
        }
    }
    result
}

fn now_millis() -> u64 {
    crate::clock::unix_millis()
}

fn now_secs() -> u64 {
    now_millis().saturating_sub(EPOCH_MS) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_matches_the_web_client() {
        assert_eq!(float_to_hex(10.5), "A.8");
        assert_eq!(float_to_hex(0.25), ".4");
        assert_eq!(float_to_hex(1.0), "1");
        assert_eq!(float_to_hex(0.0), "");
        assert_eq!(float_to_hex(255.0), "FF");
    }

    #[test]
    fn cubic_midpoint_matches_python() {
        let value = cubic_value(&[0.1, 0.2, 0.3, 0.4], 0.5);
        assert!((value - 0.562_469_792_515_521_8).abs() < 1e-12);
    }

    #[test]
    fn page_parser_reads_the_verification_key_and_second_path() {
        let html = r#"<meta name="twitter-site-verification" content="QUJD"/>
            <svg id="loading-x-anim-0"><g><path d="M 0"/><path d="M 10,30 C 1,2 3,4"/></g></svg>"#;
        assert_eq!(verification_key(html).as_deref(), Some("QUJD"));
        let paths = anim_paths(html).unwrap();
        assert_eq!(paths, vec!["M 10,30 C 1,2 3,4".to_string()]);
        assert_eq!(
            key_indices("(t[12], 16) xx (t[11], 16)"),
            Some(vec![12, 11])
        );
        assert_eq!(key_indices("(t[12],16)(t[11],16)"), Some(vec![11]));
        assert_eq!(
            ondemand_url(r#",59924:"ondemand.s",59924:"5192a865e9cbdaba""#).as_deref(),
            Some("https://abs.twimg.com/responsive-web/client-web/ondemand.s.5192a865e9cbdabaa.js")
        );
    }

    #[test]
    fn header_is_stable_for_a_fixed_clock() {
        let tid = Transaction {
            key_bytes: vec![1, 2, 3, 4],
            animation_key: "abc".to_string(),
        };
        let first = tid.header_at("POST", "/i/api/graphql/abc/CreateBookmark", 1_000, 7);
        let second = tid.header_at("POST", "/i/api/graphql/abc/CreateBookmark", 1_000, 7);
        assert_eq!(first, second);
        assert!(!first.contains('='));
        assert_ne!(
            first,
            tid.header_at("POST", "/i/api/graphql/abc/DeleteBookmark", 1_000, 7)
        );
    }
}
