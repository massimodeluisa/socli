//! Inline images for the rich detail level: background download and decode, drawn as kitty, iTerm2, sixel or half blocks through ratatui-image.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui_image::picker::{Capability, Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, ResizeEncodeRender};

const WORKERS: usize = 3;
const MAX_READY: usize = 64;
const MAX_VARIANTS: usize = 2;
const SCALE: Resize = Resize::Scale(None);

/// Numbers every encoder request, so an answer for a slot that has moved on is dropped.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The image protocol the user asked for.
pub enum Want {
    Auto,
    Kitty,
    Iterm,
    Sixel,
    Blocks,
}

/// Fetches the bytes behind a URL.
pub type Fetch = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

/// Where a preview is in its life.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Shown {
    Missing,
    Loading,
    Failed,
    Ready,
}

/// One URL for a download worker, with the shape to cut when it is a profile picture.
struct Download {
    url: String,
    avatar: Option<Avatar>,
}

/// A profile picture's cut: the pixel box it fills, its mask, and the color behind its corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Avatar {
    width: u32,
    height: u32,
    square: bool,
    /// Set when the protocol cannot draw transparency.
    backdrop: Option<[u8; 3]>,
}

/// A finished download: the prepared picture, its first protocol, its pixel size, and the cut it was made with.
struct Loaded {
    source: Arc<DynamicImage>,
    protocol: StatefulProtocol,
    size: (u32, u32),
    avatar: Option<Avatar>,
}

type Decoded = (String, Result<Loaded, String>);

/// A URL and the slot size, in cells, it is drawn at.
type Key = (String, u16, u16);

/// Work for the encoder thread.
enum Job {
    Encode {
        key: Key,
        gen: u64,
        protocol: Box<StatefulProtocol>,
        resize: Resize,
        area: Rect,
    },
    Build {
        key: Key,
        gen: u64,
        source: Arc<DynamicImage>,
    },
}

/// The encoder thread's answer to one [`Job`].
struct Done {
    key: Key,
    gen: u64,
    result: Result<StatefulProtocol, ()>,
}

/** A picture at one slot size. Like ratatui-image's `ThreadProtocol`, it hands its protocol to the encoder thread when a resize is due, but answers come back by key. */
pub struct KeyedProtocol {
    inner: Option<StatefulProtocol>,
    tx: Sender<Job>,
    key: Key,
    gen: u64,
}

impl KeyedProtocol {
    fn size_for(&self, resize: Resize, area: Rect) -> Option<Rect> {
        self.inner
            .as_ref()
            .map(|protocol| protocol.size_for(resize, area))
    }
}

impl ResizeEncodeRender for KeyedProtocol {
    fn needs_resize(&self, resize: &Resize, area: Rect) -> Option<Rect> {
        self.inner
            .as_ref()
            .and_then(|protocol| protocol.needs_resize(resize, area))
    }

    fn resize_encode(&mut self, resize: &Resize, area: Rect) {
        if let Some(protocol) = self.inner.take() {
            self.gen = next_generation();
            let _ = self.tx.send(Job::Encode {
                key: self.key.clone(),
                gen: self.gen,
                protocol: Box::new(protocol),
                resize: resize.clone(),
                area,
            });
        }
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if let Some(protocol) = self.inner.as_mut() {
            protocol.render(area, buf);
        }
    }
}

fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::Relaxed) + 1
}

/// A [`KeyedProtocol`] and the frame it was last drawn in.
struct Variant {
    protocol: KeyedProtocol,
    drawn: u64,
}

enum Entry {
    Loading,
    Failed,
    Ready {
        source: Arc<DynamicImage>,
        /// The download worker's protocol, until the first slot that draws the picture claims it.
        unclaimed: Option<Box<StatefulProtocol>>,
        variants: Vec<Variant>,
        used: u64,
        size: (u32, u32),
    },
}

/// The encoder thread's ends of its two channels.
struct Encoder {
    jobs: Receiver<Job>,
    done: Sender<Done>,
}

impl Encoder {
    fn run(self, picker: &Picker) {
        for job in self.jobs {
            if self.done.send(encode(picker, job)).is_err() {
                return;
            }
        }
    }
}

struct Queue {
    jobs: Mutex<VecDeque<Download>>,
    wake: Condvar,
    closed: AtomicBool,
}

/// Background image loader with a bounded set of decoded previews.
pub struct Previews {
    picker: Picker,
    queue: Arc<Queue>,
    results: Receiver<Decoded>,
    encoder: Sender<Job>,
    encoded: Receiver<Done>,
    entries: HashMap<String, Entry>,
    /// URLs asked for as profile pictures, masked when they arrive; true for a rounded square.
    avatars: HashMap<String, bool>,
    /// The row background that fills the corners when the protocol cannot draw transparency.
    backdrop: [u8; 3],
    clock: u64,
    frame: u64,
}

/// Builds the picker for `want`; must run after raw mode is on and before the event loop.
pub fn picker(want: Want) -> Picker {
    let env = |key: &str| std::env::var(key).ok().filter(|value| !value.is_empty());
    if prints_graphics_as_text(&env) {
        return Picker::halfblocks();
    }
    let protocol = match want {
        Want::Blocks => return Picker::halfblocks(),
        Want::Auto if in_tmux(&env) => return Picker::halfblocks(),
        Want::Auto => None,
        Want::Kitty => Some(ProtocolType::Kitty),
        Want::Iterm => Some(ProtocolType::Iterm2),
        Want::Sixel => Some(ProtocolType::Sixel),
    };
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    match protocol {
        Some(protocol) => picker.set_protocol_type(protocol),
        None => {
            let sixel = picker.capabilities().contains(&Capability::Sixel);
            if auto_choice(&env, sixel) == Choice::Halfblocks {
                picker.set_protocol_type(ProtocolType::Halfblocks);
            }
        }
    }
    picker
}

#[derive(Debug, PartialEq, Eq)]
enum Choice {
    Halfblocks,
    Queried,
}

/** tmux needs `allow-passthrough`, which the query would switch on behind the user's back. */
fn in_tmux(env: &dyn Fn(&str) -> Option<String>) -> bool {
    env("TMUX").is_some_and(|value| !value.is_empty())
}

/**
Apple Terminal shows the kitty graphics query, and a kitty or iTerm picture, as ordinary text.
Half blocks stay on screen. A saved `kitty` mode still means kitty in a terminal that can draw it.
*/
pub(crate) fn prints_graphics_as_text(env: &dyn Fn(&str) -> Option<String>) -> bool {
    env("TERM_PROGRAM").as_deref() == Some("Apple_Terminal")
}

/** VS Code and Cursor draw images only with `terminal.integrated.enableImages`, which answers the sixel query. */
fn auto_choice(env: &dyn Fn(&str) -> Option<String>, sixel: bool) -> Choice {
    let vscode = env("TERM_PROGRAM").as_deref() == Some("vscode");
    if in_tmux(env) || (vscode && !sixel) {
        Choice::Halfblocks
    } else {
        Choice::Queried
    }
}

impl Previews {
    /// Starts the download workers and the encoder thread.
    pub fn new(picker: Picker, fetch: Fetch) -> Self {
        let (previews, encoder) = Self::unstarted(picker, fetch);
        let picker = previews.picker.clone();
        thread::spawn(move || encoder.run(&picker));
        previews
    }

    /// Starts the download workers and leaves the encoder's channel ends to the caller.
    fn unstarted(picker: Picker, fetch: Fetch) -> (Self, Encoder) {
        let queue = Arc::new(Queue {
            jobs: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            closed: AtomicBool::new(false),
        });
        let (sender, results) = channel();
        for _ in 0..WORKERS {
            let queue = Arc::clone(&queue);
            let fetch = Arc::clone(&fetch);
            let sender = sender.clone();
            let picker = picker.clone();
            thread::spawn(move || work(&queue, &fetch, &picker, &sender));
        }
        let (encoder, jobs) = channel();
        let (done, encoded) = channel();
        let previews = Self {
            picker,
            queue,
            results,
            encoder,
            encoded,
            entries: HashMap::new(),
            avatars: HashMap::new(),
            backdrop: [0, 0, 0],
            clock: 0,
            frame: 0,
        };
        (previews, Encoder { jobs, done })
    }

    /// Queues a profile picture, which arrives masked to a circle, or a rounded square when `square`.
    pub fn request_avatar(&mut self, url: &str, square: bool) {
        self.avatars.insert(url.to_string(), square);
        self.request(url);
    }

    /// Sets the color behind avatar corners; pictures already loaded reload when it changes and the protocol has no alpha.
    pub fn set_backdrop(&mut self, rgb: [u8; 3]) {
        if self.backdrop == rgb {
            return;
        }
        self.backdrop = rgb;
        if !self.alpha() {
            let avatars = &self.avatars;
            self.entries.retain(|url, entry| {
                !(matches!(entry, Entry::Ready { .. }) && avatars.contains_key(url))
            });
        }
    }

    fn alpha(&self) -> bool {
        matches!(
            self.picker.protocol_type(),
            ProtocolType::Kitty | ProtocolType::Iterm2
        )
    }

    /// Whether the terminal draws real pictures rather than half blocks.
    pub fn graphics(&self) -> bool {
        self.picker.protocol_type() != ProtocolType::Halfblocks
    }

    /// Whether pictures are placed with the kitty graphics protocol.
    pub fn kitty(&self) -> bool {
        self.picker.protocol_type() == ProtocolType::Kitty
    }

    /// Whether any picture is still downloading, so spinners keep turning.
    pub fn busy(&self) -> bool {
        self.entries
            .values()
            .any(|entry| matches!(entry, Entry::Loading))
    }

    /// Queues `url` unless it is already loading, ready or failed.
    pub fn request(&mut self, url: &str) {
        if self.entries.contains_key(url) {
            return;
        }
        self.entries.insert(url.to_string(), Entry::Loading);
        let job = Download {
            url: url.to_string(),
            avatar: self.avatar(url),
        };
        let mut jobs = self
            .queue
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        jobs.push_back(job);
        self.queue.wake.notify_one();
    }

    /// The cut `url` gets as a profile picture: the largest square in a 5×3 cell box.
    fn avatar(&self, url: &str) -> Option<Avatar> {
        let square = *self.avatars.get(url)?;
        let (cell_w, cell_h) = self.picker.font_size();
        Some(Avatar {
            width: u32::from(cell_w) * 5,
            height: u32::from(cell_h) * 3,
            square,
            backdrop: (!self.alpha()).then_some(self.backdrop),
        })
    }

    /// Collects finished downloads and encodes without blocking; true when anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((url, result)) = self.results.try_recv() {
            changed = true;
            let entry = match result {
                Ok(loaded) if loaded.avatar != self.avatar(&url) => {
                    self.entries.remove(&url);
                    continue;
                }
                Ok(loaded) => {
                    self.clock += 1;
                    Entry::Ready {
                        source: loaded.source,
                        unclaimed: Some(Box::new(loaded.protocol)),
                        variants: Vec::new(),
                        used: self.clock,
                        size: loaded.size,
                    }
                }
                Err(_) => Entry::Failed,
            };
            self.entries.insert(url, entry);
            self.evict();
        }
        while let Ok(done) = self.encoded.try_recv() {
            let Some(Entry::Ready { variants, .. }) = self.entries.get_mut(&done.key.0) else {
                continue;
            };
            let Some(variant) = variants.iter_mut().find(|variant| {
                variant.protocol.key == done.key && variant.protocol.gen == done.gen
            }) else {
                continue;
            };
            changed = true;
            match done.result {
                Ok(protocol) => variant.protocol.inner = Some(protocol),
                Err(()) => {
                    self.entries.insert(done.key.0, Entry::Failed);
                }
            }
        }
        changed
    }

    /// Whether a picture is with the encoder thread.
    pub fn encoding(&self) -> bool {
        self.entries.values().any(|entry| match entry {
            Entry::Ready { variants, .. } => variants
                .iter()
                .any(|variant| variant.protocol.inner.is_none()),
            _ => false,
        })
    }

    /// Starts a frame; a picture's variant drawn in this frame is not handed to another slot size.
    pub fn next_frame(&mut self) {
        self.frame += 1;
    }

    /**
    The picture for `url` encoded for `area`, with the size it fills there, and marks it recently used.
    None means the slot shows its loading frame: the picture isn't ready, its encode is with the
    encoder thread, or two other slot sizes hold its variants in this frame.
    */
    pub fn encoded(&mut self, url: &str, area: Rect) -> Option<(&mut KeyedProtocol, Rect)> {
        self.clock += 1;
        let Some(Entry::Ready {
            source,
            unclaimed,
            variants,
            used,
            ..
        }) = self.entries.get_mut(url)
        else {
            return None;
        };
        *used = self.clock;
        let key = (url.to_string(), area.width, area.height);
        let index = match variants
            .iter()
            .position(|variant| variant.protocol.key == key)
        {
            Some(index) => index,
            None if variants.len() < MAX_VARIANTS => {
                let mut protocol = KeyedProtocol {
                    inner: unclaimed.take().map(|protocol| *protocol),
                    tx: self.encoder.clone(),
                    key,
                    gen: next_generation(),
                };
                if protocol.inner.is_none() {
                    build(&mut protocol, source);
                }
                variants.push(Variant {
                    protocol,
                    drawn: self.frame,
                });
                variants.len() - 1
            }
            None => {
                let (index, oldest) = variants
                    .iter_mut()
                    .enumerate()
                    .min_by_key(|(_, variant)| variant.drawn)?;
                if oldest.drawn == self.frame {
                    return None;
                }
                oldest.protocol.key = key;
                oldest.protocol.gen = next_generation();
                if oldest.protocol.inner.is_none() {
                    build(&mut oldest.protocol, source);
                }
                index
            }
        };
        let variant = &mut variants[index];
        variant.drawn = self.frame;
        let protocol = &mut variant.protocol;
        let fill = protocol.size_for(SCALE, area)?;
        if let Some(rect) = protocol.needs_resize(&SCALE, fill) {
            protocol.resize_encode(&SCALE, rect);
            return None;
        }
        Some((protocol, fill))
    }

    /// Reports the state of `url`.
    pub fn shown(&self, url: &str) -> Shown {
        match self.entries.get(url) {
            None => Shown::Missing,
            Some(Entry::Loading) => Shown::Loading,
            Some(Entry::Failed) => Shown::Failed,
            Some(Entry::Ready { .. }) => Shown::Ready,
        }
    }

    /// The pixel size of a decoded image.
    pub fn size(&self, url: &str) -> Option<(u32, u32)> {
        match self.entries.get(url) {
            Some(Entry::Ready { size, .. }) => Some(*size),
            _ => None,
        }
    }

    /// Cell width divided by cell height; half-blocks count as 0.5.
    pub fn cell_ratio(&self) -> f32 {
        let (width, height) = self.picker.font_size();
        if self.picker.protocol_type() == ProtocolType::Halfblocks || height == 0 {
            0.5
        } else {
            f32::from(width) / f32::from(height)
        }
    }

    fn evict(&mut self) {
        let ready = self
            .entries
            .values()
            .filter(|entry| matches!(entry, Entry::Ready { .. }))
            .count();
        if ready <= MAX_READY {
            return;
        }
        let oldest = self
            .entries
            .iter()
            .filter_map(|(url, entry)| match entry {
                Entry::Ready { used, .. } => Some((*used, url.clone())),
                _ => None,
            })
            .min();
        if let Some((_, url)) = oldest {
            self.entries.remove(&url);
        }
    }
}

/// Crops a picture to the largest square that fits the avatar's box, masks it and centers it.
fn fill_box(image: DynamicImage, avatar: Avatar) -> DynamicImage {
    if avatar.width == 0 || avatar.height == 0 {
        return image;
    }
    let side = avatar.width.min(avatar.height);
    let face = image
        .resize_to_fill(side, side, FilterType::Triangle)
        .to_rgba8();
    let canvas = shaped(
        face,
        avatar.width,
        avatar.height,
        avatar.square,
        avatar.backdrop,
    );
    DynamicImage::ImageRgba8(canvas)
}

/**
Masks a square `face` to a circle, or to a rounded square with corner radius a fifth of the side,
and centers it on a transparent `width`×`height` canvas. With a `backdrop` the canvas is composited
over that color and made opaque, for protocols without transparency.
*/
fn shaped(
    face: RgbaImage,
    width: u32,
    height: u32,
    square: bool,
    backdrop: Option<[u8; 3]>,
) -> RgbaImage {
    const GRID: u32 = 4;
    let side = face.width();
    let extent = side as f32;
    let radius = extent * 0.2;
    let inside = |x: f32, y: f32| {
        if !square {
            let half = extent / 2.0;
            return (x - half).hypot(y - half) <= half;
        }
        if !(0.0..=extent).contains(&x) || !(0.0..=extent).contains(&y) {
            return false;
        }
        let corner = |v: f32| {
            if v < radius {
                Some(radius)
            } else if v > extent - radius {
                Some(extent - radius)
            } else {
                None
            }
        };
        match (corner(x), corner(y)) {
            (Some(cx), Some(cy)) => (x - cx).hypot(y - cy) <= radius,
            _ => true,
        }
    };
    let mut canvas = RgbaImage::new(width, height);
    let (left, top) = ((width - side) / 2, (height - side) / 2);
    for (px, py, pixel) in face.enumerate_pixels() {
        let mut hits = 0;
        for j in 0..GRID {
            for i in 0..GRID {
                let step = |offset: u32, at: u32| at as f32 + (offset as f32 + 0.5) / GRID as f32;
                hits += u32::from(inside(step(i, px), step(j, py)));
            }
        }
        let alpha = (f32::from(pixel[3]) * hits as f32 / (GRID * GRID) as f32).round() as u8;
        canvas.put_pixel(
            left + px,
            top + py,
            Rgba([pixel[0], pixel[1], pixel[2], alpha]),
        );
    }
    if let Some(rgb) = backdrop {
        for pixel in canvas.pixels_mut() {
            let alpha = f32::from(pixel[3]) / 255.0;
            for channel in 0..3 {
                let mixed =
                    f32::from(pixel[channel]) * alpha + f32::from(rgb[channel]) * (1.0 - alpha);
                pixel[channel] = mixed.round() as u8;
            }
            pixel[3] = 255;
        }
    }
    canvas
}

/// Asks the encoder thread for a new protocol at `protocol`'s slot size, built from the prepared picture.
fn build(protocol: &mut KeyedProtocol, source: &Arc<DynamicImage>) {
    let _ = protocol.tx.send(Job::Build {
        key: protocol.key.clone(),
        gen: protocol.gen,
        source: Arc::clone(source),
    });
}

/** Resizes and encodes off the reader's thread; a built protocol is encoded for the area the slot will ask for. */
fn encode(picker: &Picker, job: Job) -> Done {
    let (key, gen, mut protocol, resize, area) = match job {
        Job::Encode {
            key,
            gen,
            protocol,
            resize,
            area,
        } => (key, gen, *protocol, resize, area),
        Job::Build { key, gen, source } => {
            let protocol = picker.new_resize_protocol((*source).clone());
            let fill = protocol.size_for(SCALE, Rect::new(0, 0, key.1, key.2));
            let area = protocol.needs_resize(&SCALE, fill).unwrap_or(fill);
            (key, gen, protocol, SCALE, area)
        }
    };
    protocol.resize_encode(&resize, area);
    let result = match protocol.last_encoding_result() {
        Some(Ok(())) => Ok(protocol),
        _ => Err(()),
    };
    Done { key, gen, result }
}

impl Drop for Previews {
    fn drop(&mut self) {
        self.queue.closed.store(true, Ordering::SeqCst);
        self.queue.wake.notify_all();
    }
}

/// Rows for a preview `cols` wide at `ratio` cell width over height, capped at `max_rows`.
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

/** Downloads, decodes, cuts and wraps each queued picture, so the reader's thread never touches pixels. */
fn work(queue: &Queue, fetch: &Fetch, picker: &Picker, sender: &Sender<Decoded>) {
    loop {
        let job = {
            let mut jobs = queue.jobs.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if queue.closed.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(job) = jobs.pop_front() {
                    break job;
                }
                jobs = queue
                    .wake
                    .wait(jobs)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let result = fetch(&job.url)
            .and_then(|bytes| crate::media::decode(&bytes))
            .map(|image| {
                let image = match job.avatar {
                    Some(avatar) => fill_box(image, avatar),
                    None => image,
                };
                Loaded {
                    size: (image.width(), image.height()),
                    protocol: picker.new_resize_protocol(image.clone()),
                    source: Arc::new(image),
                    avatar: job.avatar,
                }
            });
        if sender.send((job.url, result)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    use image::{ImageFormat, RgbaImage};
    use ratatui::widgets::StatefulWidget;
    use ratatui_image::StatefulImage;

    use super::*;

    const AREA: Rect = Rect::new(0, 0, 8, 4);

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        let written = RgbaImage::new(4, 2).write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png);
        assert!(written.is_ok());
        bytes
    }

    fn previews(fetch: Fetch) -> Previews {
        Previews::new(Picker::halfblocks(), fetch)
    }

    fn ok_fetch() -> Fetch {
        let bytes = png();
        Arc::new(move |_| Ok(bytes.clone()))
    }

    fn wait_for(previews: &mut Previews, url: &str, want: Shown) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while previews.shown(url) != want {
            assert!(Instant::now() < deadline, "{url} never became {want:?}");
            previews.poll();
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Draws frames until `url` is encoded for [`AREA`].
    fn wait_encoded(previews: &mut Previews, url: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            previews.next_frame();
            if previews.encoded(url, AREA).is_some() {
                return;
            }
            assert!(Instant::now() < deadline, "{url} was never encoded");
            previews.poll();
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Plays the encoder thread: answers every queued job and counts them.
    fn answer(encoder: &Encoder, picker: &Picker) -> usize {
        let mut answered = 0;
        while let Ok(job) = encoder.jobs.try_recv() {
            assert!(encoder.done.send(encode(picker, job)).is_ok());
            answered += 1;
        }
        answered
    }

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        move |key| {
            owned
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn avatars_fill_the_five_by_three_box() {
        assert!(!previews(ok_fetch()).graphics());
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Iterm2);
        let mut previews = Previews::new(picker, ok_fetch());
        assert!(previews.graphics());
        previews.request_avatar("a", false);
        assert!(previews.busy());
        wait_for(&mut previews, "a", Shown::Ready);
        assert_eq!(previews.size("a"), Some((50, 60)));
        assert!(!previews.busy());
    }

    fn red_face() -> RgbaImage {
        RgbaImage::from_pixel(60, 60, Rgba([255, 0, 0, 255]))
    }

    #[test]
    fn circle_mask_is_anti_aliased_and_centered() {
        let canvas = shaped(red_face(), 60, 72, false, None);
        assert_eq!(canvas[(5, 11)][3], 0);
        assert_eq!(canvas[(30, 36)][3], 255);
        assert!(canvas.pixels().any(|p| p[3] > 0 && p[3] < 255));
        assert!((0..60).all(|x| canvas[(x, 0)][3] == 0));
    }

    #[test]
    fn rounded_square_keeps_more_of_the_corner_than_the_circle() {
        let canvas = shaped(red_face(), 60, 72, true, None);
        assert_eq!(canvas[(5, 11)][3], 255);
        assert_eq!(canvas[(1, 7)][3], 0);
    }

    #[test]
    fn backdrop_fills_the_transparent_pixels() {
        let canvas = shaped(red_face(), 60, 72, false, Some([10, 20, 30]));
        assert_eq!(canvas[(0, 0)].0, [10, 20, 30, 255]);
        assert_eq!(canvas[(30, 36)].0, [255, 0, 0, 255]);
    }

    #[test]
    fn apple_terminal_prints_graphics_as_text() {
        let env = env_of(&[("TERM_PROGRAM", "Apple_Terminal")]);
        assert!(prints_graphics_as_text(&env));
        assert!(!prints_graphics_as_text(&env_of(&[(
            "TERM_PROGRAM",
            "ghostty"
        )])));
        assert!(!prints_graphics_as_text(&env_of(&[])));
    }

    #[test]
    fn auto_in_tmux_skips_the_query() {
        assert!(in_tmux(&env_of(&[("TMUX", "/tmp/tmux-1/default,1,0")])));
        assert!(!in_tmux(&env_of(&[])));
        let env = env_of(&[("TMUX", "x")]);
        assert_eq!(auto_choice(&env, true), Choice::Halfblocks);
    }

    #[test]
    fn auto_in_vscode_needs_sixel() {
        let env = env_of(&[("TERM_PROGRAM", "vscode")]);
        assert_eq!(auto_choice(&env, false), Choice::Halfblocks);
        assert_eq!(auto_choice(&env, true), Choice::Queried);
    }

    #[test]
    fn auto_elsewhere_keeps_the_queried_protocol() {
        let env = env_of(&[("TERM_PROGRAM", "ghostty")]);
        assert_eq!(auto_choice(&env, false), Choice::Queried);
        assert_eq!(auto_choice(&env_of(&[]), false), Choice::Queried);
    }

    #[test]
    fn rows_for_known_size() {
        let p = previews(ok_fetch());
        assert_eq!(p.cell_ratio(), 0.5);
        assert_eq!(rows_for(0.5, Some(800), Some(400), 40, 12), 10);
        assert_eq!(rows_for(0.5, Some(800), Some(400), 41, 12), 11);
    }

    #[test]
    fn rows_for_is_capped_and_at_least_one() {
        assert_eq!(rows_for(0.5, Some(100), Some(1000), 40, 12), 12);
        assert_eq!(rows_for(0.5, Some(1000), Some(1), 40, 12), 1);
    }

    #[test]
    fn rows_for_unknown_size_uses_the_cap() {
        assert_eq!(rows_for(0.5, None, Some(10), 40, 6), 6);
        assert_eq!(rows_for(0.5, Some(10), None, 40, 6), 6);
        assert_eq!(rows_for(0.5, Some(0), Some(10), 40, 6), 6);
        assert_eq!(rows_for(0.5, Some(10), Some(0), 40, 6), 6);
    }

    #[test]
    fn decoded_image_becomes_ready() {
        let mut p = previews(ok_fetch());
        assert_eq!(p.shown("a"), Shown::Missing);
        p.request("a");
        assert_eq!(p.shown("a"), Shown::Loading);
        wait_for(&mut p, "a", Shown::Ready);
        wait_encoded(&mut p, "a");
        assert!(p.encoded("b", AREA).is_none());
    }

    #[test]
    fn a_picture_keeps_its_loading_frame_until_the_encoder_answers() {
        let picker = Picker::halfblocks();
        let (mut p, encoder) = Previews::unstarted(picker.clone(), ok_fetch());
        p.request("a");
        wait_for(&mut p, "a", Shown::Ready);
        p.next_frame();
        assert!(p.encoded("a", AREA).is_none());
        assert!(p.encoding());
        let Ok(job) = encoder.jobs.try_recv() else {
            panic!("the first draw sent no job");
        };
        for _ in 0..3 {
            assert!(!p.poll());
            p.next_frame();
            assert!(p.encoded("a", AREA).is_none());
        }
        assert!(encoder.jobs.try_recv().is_err());
        assert!(encoder.done.send(encode(&picker, job)).is_ok());
        assert!(p.poll());
        assert!(!p.encoding());
        p.next_frame();
        let Some((protocol, fill)) = p.encoded("a", AREA) else {
            panic!("the answer was not applied");
        };
        let mut buffer = Buffer::empty(fill);
        StatefulImage::default()
            .resize(SCALE)
            .render(fill, &mut buffer, protocol);
        assert_ne!(buffer, Buffer::empty(fill));
        assert_eq!(answer(&encoder, &picker), 0);
    }

    #[test]
    fn one_picture_in_two_slot_sizes_settles_on_two_variants() {
        let picker = Picker::halfblocks();
        let (mut p, encoder) = Previews::unstarted(picker.clone(), ok_fetch());
        p.request("a");
        wait_for(&mut p, "a", Shown::Ready);
        let (small, large, wide) = (
            Rect::new(0, 0, 6, 3),
            Rect::new(0, 0, 12, 6),
            Rect::new(0, 0, 20, 4),
        );
        let mut frames = 0;
        let shown = loop {
            frames += 1;
            p.next_frame();
            let small_shown = p.encoded("a", small).is_some();
            let large_shown = p.encoded("a", large).is_some();
            if (small_shown && large_shown) || frames == 3 {
                break small_shown && large_shown;
            }
            answer(&encoder, &picker);
            p.poll();
        };
        assert!(shown, "both slots still loading after {frames} frames");
        for _ in 0..5 {
            p.next_frame();
            assert!(p.encoded("a", small).is_some());
            assert!(p.encoded("a", large).is_some());
            assert_eq!(answer(&encoder, &picker), 0);
        }
        p.next_frame();
        assert!(p.encoded("a", small).is_some());
        assert!(p.encoded("a", wide).is_none());
        assert_eq!(answer(&encoder, &picker), 1);
        assert!(p.poll());
        p.next_frame();
        assert!(p.encoded("a", small).is_some());
        assert!(p.encoded("a", wide).is_some());
    }

    #[test]
    fn failing_fetch_becomes_failed() {
        let mut p = previews(Arc::new(|_| Err("offline".to_string())));
        p.request("a");
        wait_for(&mut p, "a", Shown::Failed);
        assert!(p.encoded("a", AREA).is_none());
    }

    #[test]
    fn undecodable_bytes_become_failed() {
        let mut p = previews(Arc::new(|_| Ok(b"not an image".to_vec())));
        p.request("a");
        wait_for(&mut p, "a", Shown::Failed);
    }

    #[test]
    fn request_is_idempotent() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let bytes = png();
        let mut p = previews(Arc::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(bytes.clone())
        }));
        p.request("a");
        p.request("a");
        wait_for(&mut p, "a", Shown::Ready);
        p.request("a");
        thread::sleep(Duration::from_millis(50));
        p.poll();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn least_recently_used_is_dropped() {
        let mut p = previews(ok_fetch());
        for index in 0..=MAX_READY {
            let url = format!("u{index}");
            p.request(&url);
            wait_for(&mut p, &url, Shown::Ready);
        }
        assert_eq!(p.shown("u0"), Shown::Missing);
        assert_eq!(p.shown("u1"), Shown::Ready);
        assert_eq!(p.shown(&format!("u{MAX_READY}")), Shown::Ready);
    }

    #[test]
    fn recent_use_protects_an_entry_from_eviction() {
        let mut p = previews(ok_fetch());
        for index in 0..MAX_READY {
            let url = format!("u{index}");
            p.request(&url);
            wait_for(&mut p, &url, Shown::Ready);
        }
        wait_encoded(&mut p, "u0");
        p.request("extra");
        wait_for(&mut p, "extra", Shown::Ready);
        assert_eq!(p.shown("u0"), Shown::Ready);
        assert_eq!(p.shown("u1"), Shown::Missing);
    }
}
