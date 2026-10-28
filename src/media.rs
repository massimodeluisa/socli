//! Pictures for `--print` and `socli view` through the viuer library, and the image cache.

#[cfg(feature = "native")]
use std::fs;
use std::io::Cursor;
#[cfg(feature = "native")]
use std::io::Read;
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageError, ImageFormat, ImageReader, Limits, Rgba, RgbaImage};
#[cfg(feature = "native")]
use reqwest::redirect::Policy;
#[cfg(feature = "native")]
use sha2::{Digest, Sha256};
#[cfg(feature = "native")]
use viuer::Config;

use crate::theme::{author_color, choice, Choice, Rgb, Theme};
#[cfg(any(test, feature = "native"))]
use crate::unfurl;

#[cfg(any(test, feature = "native"))]
const MAX_DOWNLOAD: u64 = 20 * 1024 * 1024;
#[cfg(any(test, feature = "native"))]
const MAX_REDIRECTS: usize = 5;
const MAX_SIDE: u32 = 8192;
const MAX_ALLOC: u64 = 64 * 1024 * 1024;
const UNDECODABLE: &str = "couldn't decode the image";
const TOO_LARGE: &str = "the image is too large to show";

/// Where an image URL came from, which decides what may load it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Post, profile or link card data: the reader's previews and avatars, `--print --images`, the prompt's view and `socli view <post>`.
    Post,
    /// The target typed after `socli view`.
    Typed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewerKind {
    Builtin,
    Off,
}

impl ViewerKind {
    pub fn label(self) -> &'static str {
        match self {
            ViewerKind::Builtin => "built-in viuer",
            ViewerKind::Off => "off",
        }
    }
}

choice!(ImageMode {
    Auto => "auto",
    Kitty => "kitty",
    Iterm => "iterm",
    Sixel => "sixel",
    Blocks => "blocks",
    Off => "off",
});

impl ImageMode {
    /// `[ui] images` or `SOCLI_IMAGE`, including the values `image_viewer` took before.
    pub fn from_setting(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "viu" | "builtin" | "viuer" => Some(ImageMode::Auto),
            "none" => Some(ImageMode::Off),
            other => Self::parse(other),
        }
    }

    /// The flag, then `SOCLI_IMAGE`, then `[ui] images`, then auto.
    pub fn resolve(flag: Option<Self>, config: &crate::config::Config) -> Self {
        flag.or_else(|| {
            std::env::var("SOCLI_IMAGE")
                .ok()
                .and_then(|value| Self::from_setting(&value))
        })
        .or_else(|| config.ui.images.as_deref().and_then(Self::from_setting))
        .unwrap_or(ImageMode::Auto)
    }
}

/// How images reach the terminal: auto lets viuer pick, kitty and iterm force one, the rest draw blocks.
#[derive(Clone, Debug)]
pub struct Viewer {
    pub mode: ImageMode,
}

impl Viewer {
    pub fn new(mode: ImageMode) -> Self {
        Self { mode }
    }

    pub fn resolve(&self) -> ViewerKind {
        match self.mode {
            ImageMode::Off => ViewerKind::Off,
            _ => ViewerKind::Builtin,
        }
    }
}

pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SOCLI_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("socli");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".cache").join("socli");
    }
    std::env::temp_dir().join("socli")
}

/// Rejects a `socli view` target that [`materialize`] could never load, before anything is fetched.
pub fn check_source(url: &str) -> Result<(), String> {
    let loadable = ["demo://", "file://", "https://", "http://"]
        .iter()
        .any(|prefix| url.starts_with(prefix))
        || Path::new(url).is_file();
    if loadable {
        Ok(())
    } else {
        Err(format!("not an image url: {url}"))
    }
}

/**
The bytes of the image at `url`. A `demo://` name is generated here. http(s) from a post goes
through the link reader's client, which refuses every non-public address and redirect hop, with an
8 MiB cap; a typed one may reach any host, with a 20 MiB cap and five redirects. A local path or
`file://` loads only when typed, and any other scheme is refused before anything is fetched.
*/
#[cfg(not(feature = "native"))]
pub fn load(url: &str, origin: Origin) -> Result<Vec<u8>, String> {
    let _ = origin;
    if let Some(name) = url.strip_prefix("demo://") {
        return png(&demo_image(name)?);
    }
    Err("images are off in the browser demo".into())
}

#[cfg(feature = "native")]
pub fn load(url: &str, origin: Origin) -> Result<Vec<u8>, String> {
    if let Some(name) = url.strip_prefix("demo://") {
        return png(&demo_image(name)?);
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        return match origin {
            Origin::Post => unfurl::image_here(url),
            Origin::Typed => download(url),
        };
    }
    let file = url.strip_prefix("file://");
    if file.is_none() && has_scheme(url) {
        return Err(format!("not an image url: {url}"));
    }
    if origin == Origin::Post {
        return Err("not loading a local file from a post".into());
    }
    let path = Path::new(file.unwrap_or(url));
    if !path.is_file() {
        return Err(match file {
            Some(_) => format!("no image at {}", path.display()),
            None => format!("not an image url: {url}"),
        });
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("couldn't read {}: {error}", path.display()))?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_DOWNLOAD {
        return Err(TOO_LARGE.into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("couldn't read {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err(TOO_LARGE.into());
    }
    Ok(bytes)
}

/// [`load`] written to the image cache, for the viewer that draws from a file.
#[cfg(not(feature = "native"))]
pub fn materialize(url: &str, origin: Origin) -> Result<PathBuf, String> {
    let _ = (url, origin);
    Err("images are off in the browser demo".into())
}

#[cfg(feature = "native")]
pub fn materialize(url: &str, origin: Origin) -> Result<PathBuf, String> {
    let bytes = load(url, origin)?;
    let ext = sniff_ext(&bytes);
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let dir = cache_dir().join("media");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{hex}.{ext}"));
    fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Whether `url` starts with a URL scheme and `://`, which no local path does.
#[cfg(any(test, feature = "native"))]
fn has_scheme(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, _)| {
        scheme.starts_with(|ch: char| ch.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    })
}

fn demo_image(name: &str) -> Result<DynamicImage, String> {
    Ok(match name {
        "spark" => spark_image(),
        "wave" => wave_image(),
        "og-wide" => og_wide_image(),
        "og-square" => og_square_image(),
        _ => match name.strip_prefix("avatar/") {
            Some(username) => avatar_image(username),
            None => return Err(format!("no demo image named {name}")),
        },
    })
}

fn png(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|e| format!("could not write the demo image: {e}"))?;
    Ok(bytes)
}

/// A typed http(s) image from any host, at most 20 MiB after at most five http(s) redirects.
#[cfg(feature = "native")]
fn download(url: &str) -> Result<Vec<u8>, String> {
    let policy = Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if matches!(attempt.url().scheme(), "http" | "https") {
            attempt.follow()
        } else {
            attempt.error("not an image url")
        }
    });
    let client = reqwest::blocking::Client::builder()
        .user_agent(crate::net::http::USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .redirect(policy)
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("image download failed ({})", response.status()));
    }
    let too_large = || "image is larger than 20 MiB. Press o to open it in the browser".to_string();
    if response.content_length().unwrap_or(0) > MAX_DOWNLOAD {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_DOWNLOAD {
        return Err(too_large());
    }
    Ok(bytes)
}

/**
Decodes an image within [`MAX_SIDE`] pixels a side and [`MAX_ALLOC`] bytes of memory, so a header
that claims a huge picture is refused before anything that large is allocated. Every image path
decodes here: previews, avatars, `--print --images` and `socli view`.
*/
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| UNDECODABLE.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader.decode().map_err(|error| match error {
        ImageError::Limits(_) => TOO_LARGE.to_string(),
        _ => UNDECODABLE.to_string(),
    })
}

#[cfg(any(test, feature = "native"))]
fn sniff_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "jpg"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
        "mp4"
    } else {
        "img"
    }
}

#[cfg(not(feature = "native"))]
pub fn show_with(
    viewer: &Viewer,
    path: &Path,
    width: u32,
    once: bool,
) -> Result<ViewerKind, String> {
    let _ = (viewer, path, width, once);
    Err("images are off in the browser demo".into())
}

#[cfg(feature = "native")]
pub fn show_with(
    viewer: &Viewer,
    path: &Path,
    width: u32,
    _once: bool,
) -> Result<ViewerKind, String> {
    match viewer.resolve() {
        ViewerKind::Off => {
            Err("images are off. `socli config set ui.images auto` turns them on".into())
        }
        ViewerKind::Builtin => {
            show_builtin(path, width, viewer.mode)?;
            Ok(ViewerKind::Builtin)
        }
    }
}

#[cfg(feature = "native")]
fn show_builtin(path: &Path, width: u32, mode: ImageMode) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let img = decode(&bytes)?;
    // viuer's kitty check writes the same probe Apple Terminal prints as text.
    let graphics = !crate::preview::prints_graphics_as_text(&|key| {
        std::env::var(key).ok().filter(|value| !value.is_empty())
    });
    let conf = Config {
        absolute_offset: false,
        width: Some(width),
        height: None,
        transparent: true,
        x: 0,
        y: 0,
        use_kitty: graphics && matches!(mode, ImageMode::Auto | ImageMode::Kitty),
        use_iterm: graphics && matches!(mode, ImageMode::Auto | ImageMode::Iterm),
        ..Config::default()
    };
    viuer::print(&img, &conf).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn spark_image() -> DynamicImage {
    let w = 360u32;
    let h = 180u32;
    let mut img = RgbaImage::from_pixel(w, h, Rgba([8, 8, 8, 255]));
    let amber = Rgba([245, 166, 35, 255]);
    let blue = Rgba([120, 162, 186, 255]);
    let grid = Rgba([42, 42, 42, 255]);
    for x in 28..(w - 16) {
        for t in 0..2 {
            put(&mut img, x, h - 32 + t, grid);
        }
    }
    for y in 20..(h - 32) {
        put(&mut img, 28, y, grid);
    }
    stroke(
        &mut img,
        &[
            (36, 130),
            (90, 122),
            (150, 78),
            (210, 46),
            (270, 58),
            (336, 34),
        ],
        amber,
        4,
    );
    stroke(
        &mut img,
        &[(36, 148), (120, 140), (200, 136), (280, 128), (336, 124)],
        blue,
        3,
    );
    DynamicImage::ImageRgba8(img)
}

/** The demo's second picture: a rising wave over a dark gradient sky with faint grid lines. */
pub fn wave_image() -> DynamicImage {
    let (w, h) = (384u32, 296u32);
    let mix = |a: [u8; 3], b: [u8; 3], t: f32| {
        let t = t.clamp(0.0, 1.0);
        let channel =
            |i: usize| (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8;
        Rgba([channel(0), channel(1), channel(2), 255])
    };
    let mut img = RgbaImage::new(w, h);
    for x in 0..w {
        let t = x as f32 / (w - 1) as f32;
        let series = 0.2 + 0.52 * t + 0.1 * (t * 17.0).sin() + 0.05 * (t * 41.0 + 1.0).sin();
        let top = h as f32 * (1.0 - series);
        for y in 0..h {
            let fy = y as f32;
            let color = if (fy - top).abs() < 4.0 {
                Rgba([0x8f, 0xc4, 0xff, 255])
            } else if fy > top {
                mix(
                    [0x2f, 0x5a, 0x9a],
                    [0x16, 0x26, 0x4a],
                    (fy - top) / (h as f32 - top),
                )
            } else if (y / 4 + 1) % 12 == 0 {
                mix([0x10, 0x16, 0x2e], [0x2a, 0x3a, 0x62], 0.5)
            } else {
                mix([0x10, 0x16, 0x2e], [0x1b, 0x2a, 0x4d], fy / h as f32)
            };
            img.put_pixel(x, y, color);
        }
    }
    DynamicImage::ImageRgba8(img)
}

/** The demo's wide link preview: a 1200×630 two-tone gradient with a large rounded mark. */
pub fn og_wide_image() -> DynamicImage {
    let (w, h) = (1200u32, 630u32);
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let t = 0.6 * x as f32 / w as f32 + 0.4 * y as f32 / h as f32;
            img.put_pixel(x, y, mix([0x1d, 0x2b, 0x53], [0x6e, 0x3a, 0x8c], t));
        }
    }
    let (cx, cy) = (w / 2, h / 2);
    rounded(&mut img, (cx, cy), 150, 56, Rgba([0xf3, 0xf0, 0xff, 255]));
    let ink = Rgba([0x1d, 0x2b, 0x53, 255]);
    stroke(
        &mut img,
        &[(cx - 80, cy - 60), (cx - 20, cy), (cx - 80, cy + 60)],
        ink,
        22,
    );
    line(&mut img, (cx + 10, cy + 60), (cx + 90, cy + 60), ink, 22);
    DynamicImage::ImageRgba8(img)
}

/** The demo's square link preview: 150×150, amber to coral, with a small rounded mark. */
pub fn og_square_image() -> DynamicImage {
    let side = 150u32;
    let mut img = RgbaImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let t = (x + y) as f32 / (2 * side) as f32;
            img.put_pixel(x, y, mix([0xf5, 0xa6, 0x23], [0xe0, 0x5d, 0x4f], t));
        }
    }
    rounded(
        &mut img,
        (side / 2, side / 2),
        36,
        12,
        Rgba([0xff, 0xf8, 0xee, 255]),
    );
    DynamicImage::ImageRgba8(img)
}

/**
A demo author's profile picture: 64×64, a diagonal gradient from the author's color to a darker
shade, with a lighter disc in the middle.
*/
pub fn avatar_image(username: &str) -> DynamicImage {
    let Rgb(r, g, b) = Theme::default().rgb(author_color(username));
    let base = [r, g, b];
    let dark = base.map(|c| c / 2);
    let light = base.map(|c| c + (255 - c) / 2);
    let side = 64u32;
    let mut img = RgbaImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let t = (x + y) as f32 / (2 * (side - 1)) as f32;
            img.put_pixel(x, y, mix(base, dark, t));
        }
    }
    let (center, radius) = (31.5f32, 14.0f32);
    for y in 0..side {
        for x in 0..side {
            let (dx, dy) = (x as f32 - center, y as f32 - center);
            if dx * dx + dy * dy <= radius * radius {
                img.put_pixel(x, y, Rgba([light[0], light[1], light[2], 255]));
            }
        }
    }
    DynamicImage::ImageRgba8(img)
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> Rgba<u8> {
    let t = t.clamp(0.0, 1.0);
    let channel =
        |i: usize| (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8;
    Rgba([channel(0), channel(1), channel(2), 255])
}

/// Fills a square of half side `half` with corners of radius `radius`, centered on `center`.
fn rounded(img: &mut RgbaImage, center: (u32, u32), half: u32, radius: u32, color: Rgba<u8>) {
    let (cx, cy) = (i64::from(center.0), i64::from(center.1));
    let (half, radius) = (i64::from(half), i64::from(radius));
    for y in cy - half..cy + half {
        for x in cx - half..cx + half {
            let dx = ((x - cx).abs() - (half - radius)).max(0);
            let dy = ((y - cy).abs() - (half - radius)).max(0);
            if dx * dx + dy * dy <= radius * radius {
                if let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) {
                    put(img, x, y, color);
                }
            }
        }
    }
}

fn put(img: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>) {
    if x < img.width() && y < img.height() {
        img.put_pixel(x, y, color);
    }
}

fn stroke(img: &mut RgbaImage, points: &[(u32, u32)], color: Rgba<u8>, thick: i32) {
    for pair in points.windows(2) {
        line(img, pair[0], pair[1], color, thick);
    }
    for (x, y) in points {
        for dy in -thick..=thick {
            for dx in -thick..=thick {
                if dx * dx + dy * dy <= thick * thick {
                    put(
                        img,
                        x.saturating_add_signed(dx),
                        y.saturating_add_signed(dy),
                        color,
                    );
                }
            }
        }
    }
}

fn line(img: &mut RgbaImage, a: (u32, u32), b: (u32, u32), color: Rgba<u8>, thick: i32) {
    let (x0, y0) = (a.0 as i32, a.1 as i32);
    let (x1, y1) = (b.0 as i32, b.1 as i32);
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let mut x = x0;
    let mut y = y0;
    loop {
        for t in -(thick / 2)..=(thick / 2) {
            put(img, (x + t).max(0) as u32, y.max(0) as u32, color);
            put(img, x.max(0) as u32, (y + t).max(0) as u32, color);
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_targets_must_be_loadable() {
        for url in [
            "demo://spark",
            "file:///nowhere.png",
            "https://example.com/a.png",
            "http://example.com/a.png",
            "Cargo.toml",
        ] {
            assert_eq!(check_source(url), Ok(()), "{url}");
        }
        assert_eq!(
            check_source("nope"),
            Err("not an image url: nope".to_string())
        );
    }

    #[test]
    fn demo_avatars_are_a_gradient_with_a_light_disc() {
        let image = avatar_image("nuno").to_rgba8();
        assert_eq!(image.dimensions(), (64, 64));
        let sum = |p: &Rgba<u8>| p.0[..3].iter().map(|&c| u32::from(c)).sum::<u32>();
        assert!(sum(image.get_pixel(32, 32)) > sum(image.get_pixel(0, 0)));
        assert!(sum(image.get_pixel(0, 0)) > sum(image.get_pixel(63, 63)));
    }

    #[test]
    fn spark_has_pixels() {
        let img = spark_image();
        assert_eq!(img.width(), 360);
        assert!(img.height() > 100);
    }

    #[test]
    fn wave_has_the_sky_on_top_and_the_fill_below() {
        let img = wave_image().to_rgba8();
        assert_eq!((img.width(), img.height()), (384, 296));
        assert_eq!(img.get_pixel(0, 0).0, [0x10, 0x16, 0x2e, 255]);
        assert_ne!(img.get_pixel(0, 295), img.get_pixel(0, 0));
    }

    #[test]
    fn post_images_refuse_private_and_local_sources_before_any_io() {
        let private = [
            "http://127.0.0.1/x.png",
            "http://[::1]/x.png",
            "http://10.0.0.1/x.png",
            "http://192.168.1.2/x.png",
            "http://169.254.169.254/latest",
            "http://localhost/x.png",
        ];
        for url in private {
            assert_eq!(
                load(url, Origin::Post),
                Err("not a public address".into()),
                "{url}"
            );
        }
        for url in ["file:///etc/hosts", "/etc/hosts"] {
            assert_eq!(
                load(url, Origin::Post),
                Err("not loading a local file from a post".into()),
                "{url}"
            );
        }
        for origin in [Origin::Post, Origin::Typed] {
            assert_eq!(
                load("ftp://example.com/x", origin),
                Err("not an image url: ftp://example.com/x".into())
            );
        }
    }

    #[test]
    fn the_private_address_refusal_is_the_unfurl_policy() {
        let url = "http://10.0.0.1/x.png";
        assert_eq!(load(url, Origin::Post), unfurl::image_here(url));
    }

    #[test]
    fn typed_targets_read_local_files_and_demo_works_for_both() {
        let dir = std::env::temp_dir().join(format!(
            "socli-media-{}-{:08x}",
            std::process::id(),
            rand::random::<u32>()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.png");
        let bytes = png(&spark_image()).unwrap();
        fs::write(&file, &bytes).unwrap();
        let path = file.to_str().unwrap();
        assert_eq!(load(path, Origin::Typed), Ok(bytes.clone()));
        assert_eq!(load(&format!("file://{path}"), Origin::Typed), Ok(bytes));
        for origin in [Origin::Post, Origin::Typed] {
            let demo = load("demo://spark", origin).unwrap();
            assert!(demo.starts_with(&[0x89, b'P', b'N', b'G']));
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_oversized_local_image_is_refused_before_reading_its_bytes() {
        let path = std::env::temp_dir().join(format!(
            "socli-media-large-{}-{:08x}",
            std::process::id(),
            rand::random::<u32>()
        ));
        let file = fs::File::create(&path).expect("temporary image");
        file.set_len(MAX_DOWNLOAD + 1).expect("sparse image");
        let result = load(path.to_str().expect("image path"), Origin::Typed);
        fs::remove_file(path).expect("remove image");
        assert_eq!(result, Err(TOO_LARGE.into()));
    }

    /// A PNG chunk: length, type, data and the CRC-32 of type and data.
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut crc = !0u32;
        for byte in kind.iter().chain(data) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        let length = u32::try_from(data.len()).unwrap();
        [&length.to_be_bytes()[..], kind, data, &(!crc).to_be_bytes()].concat()
    }

    #[test]
    fn a_header_claiming_a_huge_image_is_refused_before_decoding() {
        let mut header = Vec::new();
        header.extend(100_000u32.to_be_bytes());
        header.extend(100_000u32.to_be_bytes());
        header.extend([8, 6, 0, 0, 0]);
        let bytes = [
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..],
            &chunk(b"IHDR", &header),
            &chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]),
            &chunk(b"IEND", &[]),
        ]
        .concat();
        assert_eq!(decode(&bytes).map(|_| ()), Err(TOO_LARGE.to_string()));
    }

    #[test]
    fn broken_images_fail_to_decode_and_good_ones_decode() {
        let bytes = png(&spark_image()).unwrap();
        let image = decode(&bytes).unwrap();
        assert_eq!(image.width(), 360);
        let truncated = &bytes[..bytes.len() / 2];
        assert_eq!(decode(truncated).map(|_| ()), Err(UNDECODABLE.to_string()));
        assert_eq!(
            decode(b"not an image").map(|_| ()),
            Err(UNDECODABLE.to_string())
        );
    }

    #[test]
    fn demo_url_writes_a_png() {
        let dir = std::env::temp_dir().join(format!("socli-test-{}", std::process::id()));
        std::env::set_var("SOCLI_CACHE_DIR", &dir);
        let path = materialize("demo://spark", Origin::Typed).unwrap();
        assert!(path.exists());
        assert!(fs::metadata(&path).unwrap().len() > 32);
        let _ = fs::remove_dir_all(&dir);
    }
}
