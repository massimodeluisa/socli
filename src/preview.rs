//! Inline images. The terminal decodes them in the background. The page draws `demo://` pictures
//! itself and loads an `http(s)` picture from its URL.

#[cfg(feature = "native")]
#[path = "preview_native.rs"]
mod native;

#[cfg(feature = "native")]
pub use native::*;

#[cfg(not(feature = "native"))]
mod offline {
    use std::collections::HashMap;

    use ratatui::layout::Rect;

    /// The image protocol the user asked for. The page never starts a picker.
    pub enum Want {
        Auto,
        Kitty,
        Iterm,
        Sixel,
        Blocks,
    }

    /// Where a preview is in its life.
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub enum Shown {
        Missing,
        Loading,
        Failed,
        Ready,
    }

    /// One picture the page should paint over the cells, in screen columns and rows.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Place {
        pub url: String,
        pub col: u16,
        pub row: u16,
        pub cols: u16,
        pub rows: u16,
        pub avatar: bool,
        pub square: bool,
    }

    struct Entry {
        shown: Shown,
        bytes: Option<Vec<u8>>,
        size: Option<(u32, u32)>,
    }

    /// Demo pictures for the page. Bytes are generated while drawing, then the page overlays them.
    pub struct Previews {
        entries: HashMap<String, Entry>,
        places: Vec<Place>,
    }

    impl Previews {
        pub fn page() -> Self {
            Self {
                entries: HashMap::new(),
                places: Vec::new(),
            }
        }

        pub fn request_avatar(&mut self, url: &str, _square: bool) {
            self.request(url);
        }

        pub fn set_backdrop(&mut self, _rgb: [u8; 3]) {}

        /// The page draws the demo's real pictures, which is the same gate the terminal uses for avatars.
        pub fn graphics(&self) -> bool {
            true
        }

        pub fn kitty(&self) -> bool {
            false
        }

        pub fn busy(&self) -> bool {
            false
        }

        pub fn request(&mut self, url: &str) {
            if self.entries.contains_key(url) {
                return;
            }
            // The browser paints an http(s) URL. Pulling the bytes in here fails, and a proxy is out.
            let entry = if url.starts_with("https://") || url.starts_with("http://") {
                Entry {
                    shown: Shown::Ready,
                    bytes: None,
                    size: None,
                }
            } else {
                match crate::media::load(url, crate::media::Origin::Post) {
                    Ok(bytes) => {
                        let size = image::load_from_memory(&bytes)
                            .ok()
                            .map(|img| (img.width(), img.height()));
                        Entry {
                            shown: Shown::Ready,
                            bytes: Some(bytes),
                            size,
                        }
                    }
                    Err(_) => Entry {
                        shown: Shown::Failed,
                        bytes: None,
                        size: None,
                    },
                }
            };
            self.entries.insert(url.to_string(), entry);
        }

        pub fn poll(&mut self) -> bool {
            false
        }

        pub fn next_frame(&mut self) {}

        pub fn shown(&self, url: &str) -> Shown {
            self.entries
                .get(url)
                .map(|entry| entry.shown)
                .unwrap_or(Shown::Missing)
        }

        pub fn size(&self, url: &str) -> Option<(u32, u32)> {
            self.entries.get(url).and_then(|entry| entry.size)
        }

        pub fn cell_ratio(&self) -> f32 {
            0.5
        }

        /// The page has no terminal protocol. The signature matches the terminal so call sites type-check.
        pub fn encoded(&mut self, _url: &str, _area: Rect) -> Option<(&mut (), Rect)> {
            None
        }

        pub fn clear_places(&mut self) {
            self.places.clear();
        }

        pub fn note(&mut self, place: Place) {
            self.places.push(place);
        }

        pub fn places_json(&self) -> String {
            let mut out = String::from("[");
            for (i, place) in self.places.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!(
                    r#"{{"url":{},"col":{},"row":{},"cols":{},"rows":{},"avatar":{},"square":{}}}"#,
                    json_string(&place.url),
                    place.col,
                    place.row,
                    place.cols,
                    place.rows,
                    place.avatar,
                    place.square,
                ));
            }
            out.push(']');
            out
        }

        pub fn png(&self, url: &str) -> Option<&[u8]> {
            self.entries
                .get(url)
                .and_then(|entry| entry.bytes.as_deref())
        }
    }

    fn json_string(text: &str) -> String {
        let mut out = String::from("\"");
        for ch in text.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                c if c.is_control() => {
                    let code = u32::from(c);
                    out.push_str(&format!("\\u{code:04x}"));
                }
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_demo_picture_is_ready_png() {
            let mut previews = Previews::page();
            previews.request("demo://spark");
            assert_eq!(previews.shown("demo://spark"), Shown::Ready);
            assert!(previews
                .png("demo://spark")
                .is_some_and(|bytes| bytes.starts_with(b"\x89PNG")));
            assert_eq!(previews.shown("https://example.com/a.png"), Shown::Missing);
            previews.request("https://example.com/a.png");
            assert_eq!(previews.shown("https://example.com/a.png"), Shown::Ready);
            assert!(previews.png("https://example.com/a.png").is_none());
            previews.request("file:///etc/passwd");
            assert_eq!(previews.shown("file:///etc/passwd"), Shown::Failed);
            previews.note(Place {
                url: "demo://spark".into(),
                col: 2,
                row: 3,
                cols: 4,
                rows: 5,
                avatar: false,
                square: false,
            });
            assert!(previews.places_json().contains("demo://spark"));
        }
    }
}

#[cfg(not(feature = "native"))]
pub use offline::{Place, Previews, Shown, Want};

/// Rows for a preview `cols` wide at `ratio` cell width over height, capped at `max_rows`.
///
/// Kept here as well as in the native module so both builds share one formula. The native
/// file keeps its own copy because its tests call it as a sibling.
#[cfg(not(feature = "native"))]
pub fn rows_for(
    ratio: f32,
    media_w: Option<u32>,
    media_h: Option<u32>,
    cols: u16,
    max_rows: u16,
) -> u16 {
    let (Some(width), Some(height)) = (media_w, media_h) else {
        return max_rows;
    };
    if width == 0 || height == 0 {
        return max_rows;
    }
    let rows = f32::from(cols) * height as f32 / width as f32 * ratio;
    let rows = rows.ceil().min(f32::from(u16::MAX)) as u16;
    rows.max(1).min(max_rows)
}
