//! Inline images for the rich detail level: background download and decode, drawn as kitty, iTerm2, sixel or half blocks through ratatui-image.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui_image::picker::{Capability, Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, ResizeEncodeRender};

const WORKERS: usize = 3;
const MAX_READY: usize = 64;
const MAX_PENDING: usize = 64;
const MAX_OUTSTANDING: usize = MAX_PENDING + WORKERS;
const MAX_ENTRIES: usize = MAX_READY + MAX_OUTSTANDING;
const RETRY_QUEUE: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(30);
const MAX_VARIANTS: usize = 2;
const MAX_ENCODER: usize = MAX_READY * MAX_VARIANTS;
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

enum Failure {
    Fetch,
    Decode,
}

type Decoded = (String, Result<Loaded, Failure>);

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

struct EncoderPermit(Arc<AtomicUsize>);

impl Drop for EncoderPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

struct PendingJob {
    job: Job,
    permit: EncoderPermit,
}

#[derive(Clone)]
struct EncoderSender {
    jobs: SyncSender<PendingJob>,
    outstanding: Arc<AtomicUsize>,
}

impl EncoderSender {
    fn try_send(&self, job: Job) -> Result<(), Job> {
        let mut used = self.outstanding.load(Ordering::Relaxed);
        let mut reserved = false;
        for _ in 0..MAX_ENCODER {
            if used >= MAX_ENCODER {
                return Err(job);
            }
            match self.outstanding.compare_exchange_weak(
                used,
                used + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    reserved = true;
                    break;
                }
                Err(current) => used = current,
            }
        }
        if !reserved {
            return Err(job);
        }
        let pending = PendingJob {
            job,
            permit: EncoderPermit(Arc::clone(&self.outstanding)),
        };
        match self.jobs.try_send(pending) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(pending) | TrySendError::Disconnected(pending)) => {
                let PendingJob { job, permit } = pending;
                drop(permit);
                Err(job)
            }
        }
    }
}

/// The encoder thread's answer to one [`Job`].
struct Done {
    key: Key,
    gen: u64,
    result: Result<StatefulProtocol, ()>,
    _permit: EncoderPermit,
}

/** A picture at one slot size. Like ratatui-image's `ThreadProtocol`, it hands its protocol to the encoder thread when a resize is due, but answers come back by key. */
pub struct KeyedProtocol {
    inner: Option<StatefulProtocol>,
    tx: EncoderSender,
    pending: bool,
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
            match self.tx.try_send(Job::Encode {
                key: self.key.clone(),
                gen: self.gen,
                protocol: Box::new(protocol),
                resize: resize.clone(),
                area,
            }) {
                Ok(()) => self.pending = true,
                Err(Job::Encode { protocol, .. }) => self.inner = Some(*protocol),
                Err(Job::Build { .. }) => {}
            }
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
    Loading {
        attempts: u8,
        requested: u64,
    },
    Waiting {
        attempts: u8,
        retry_at: Instant,
        requested: u64,
        used: u64,
    },
    Failed {
        used: u64,
    },
    Ready {
        source: Arc<DynamicImage>,
        /// The download worker's protocol, until the first slot that draws the picture claims it.
        unclaimed: Option<Box<StatefulProtocol>>,
        variants: Vec<Variant>,
        used: u64,
        size: (u32, u32),
    },
}

impl Entry {
    fn used(&self) -> Option<u64> {
        match self {
            Self::Loading { .. } => None,
            Self::Waiting { used, .. } | Self::Failed { used } | Self::Ready { used, .. } => {
                Some(*used)
            }
        }
    }
}

fn retry_delay(attempts: u8) -> Duration {
    Duration::from_secs(1 << attempts.saturating_sub(1).min(5)).min(RETRY_MAX)
}

/// The encoder thread's ends of its two channels.
struct Encoder {
    jobs: Receiver<PendingJob>,
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
    encoder: EncoderSender,
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
        let (jobs_sender, jobs) = sync_channel(MAX_ENCODER);
        let encoder = EncoderSender {
            jobs: jobs_sender,
            outstanding: Arc::new(AtomicUsize::new(0)),
        };
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
            .any(|entry| matches!(entry, Entry::Loading { .. }))
    }

    /// Queues `url`, retrying transient failures with backoff while it remains in the viewport.
    pub fn request(&mut self, url: &str) {
        self.request_at(url, Instant::now());
    }

    fn request_at(&mut self, url: &str, now: Instant) {
        self.clock = self.clock.saturating_add(1);
        let attempts = match self.entries.get_mut(url) {
            Some(Entry::Loading { requested, .. }) => {
                *requested = self.frame;
                return;
            }
            Some(Entry::Waiting {
                attempts,
                retry_at,
                requested,
                used,
            }) => {
                *requested = self.frame;
                *used = self.clock;
                if now < *retry_at {
                    return;
                }
                *attempts
            }
            Some(Entry::Ready { used, .. } | Entry::Failed { used }) => {
                *used = self.clock;
                return;
            }
            None => 0,
        };
        let avatar = self.avatar(url);
        let outstanding = self
            .entries
            .values()
            .filter(|entry| matches!(entry, Entry::Loading { .. }))
            .count();
        let mut jobs = self
            .queue
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = if jobs.len() < MAX_PENDING && outstanding < MAX_OUTSTANDING {
            jobs.push_back(Download {
                url: url.to_string(),
                avatar,
            });
            self.queue.wake.notify_one();
            Entry::Loading {
                attempts,
                requested: self.frame,
            }
        } else {
            Entry::Waiting {
                attempts,
                retry_at: now + RETRY_QUEUE,
                requested: self.frame,
                used: self.clock,
            }
        };
        drop(jobs);
        self.entries.insert(url.to_string(), entry);
        self.evict();
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
        self.poll_at(Instant::now())
    }

    fn poll_at(&mut self, now: Instant) -> bool {
        let mut changed = false;
        while let Ok((url, result)) = self.results.try_recv() {
            let Some(Entry::Loading {
                attempts,
                requested,
            }) = self.entries.get(&url)
            else {
                continue;
            };
            let (attempts, requested) = (*attempts, *requested);
            changed = true;
            self.clock = self.clock.saturating_add(1);
            let entry = match result {
                Ok(loaded) if loaded.avatar != self.avatar(&url) => {
                    self.entries.remove(&url);
                    self.avatars.remove(&url);
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
                Err(Failure::Fetch) => {
                    let attempts = attempts.saturating_add(1);
                    Entry::Waiting {
                        attempts,
                        retry_at: now + retry_delay(attempts),
                        requested,
                        used: self.clock,
                    }
                }
                Err(Failure::Decode) => Entry::Failed { used: self.clock },
            };
            self.entries.insert(url, entry);
            self.evict();
        }
        while let Ok(done) = self.encoded.try_recv() {
            changed = true;
            let Some(Entry::Ready { variants, .. }) = self.entries.get_mut(&done.key.0) else {
                continue;
            };
            let Some(variant) = variants.iter_mut().find(|variant| {
                variant.protocol.key == done.key && variant.protocol.gen == done.gen
            }) else {
                continue;
            };
            match done.result {
                Ok(protocol) => {
                    variant.protocol.inner = Some(protocol);
                    variant.protocol.pending = false;
                }
                Err(()) => {
                    self.clock = self.clock.saturating_add(1);
                    self.entries
                        .insert(done.key.0, Entry::Failed { used: self.clock });
                }
            }
        }
        let due: Vec<String> = self
            .entries
            .iter()
            .filter_map(|(url, entry)| match entry {
                Entry::Waiting {
                    retry_at,
                    requested,
                    ..
                } if *requested == self.frame && *retry_at <= now => Some(url.clone()),
                _ => None,
            })
            .collect();
        for url in due {
            self.request_at(&url, now);
            changed = true;
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
                    pending: false,
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
                    .filter(|(_, variant)| !variant.protocol.pending)
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
        if protocol.inner.is_none() && !protocol.pending {
            build(protocol, source);
        }
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
            Some(Entry::Loading { .. } | Entry::Waiting { .. }) => Shown::Loading,
            Some(Entry::Failed { .. }) => Shown::Failed,
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
        while self
            .entries
            .values()
            .filter(|entry| matches!(entry, Entry::Ready { .. }))
            .count()
            > MAX_READY
        {
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
        while self.entries.len() > MAX_ENTRIES {
            let oldest = self
                .entries
                .iter()
                .filter_map(|(url, entry)| entry.used().map(|used| (used, url.clone())))
                .min();
            let Some((_, url)) = oldest else {
                break;
            };
            self.entries.remove(&url);
        }
        self.avatars.retain(|url, _| self.entries.contains_key(url));
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
    protocol.pending = protocol
        .tx
        .try_send(Job::Build {
            key: protocol.key.clone(),
            gen: protocol.gen,
            source: Arc::clone(source),
        })
        .is_ok();
}

/** Resizes and encodes off the reader's thread; a built protocol is encoded for the area the slot will ask for. */
fn encode(picker: &Picker, pending: PendingJob) -> Done {
    let PendingJob { job, permit } = pending;
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
    Done {
        key,
        gen,
        result,
        _permit: permit,
    }
}

impl Drop for Previews {
    fn drop(&mut self) {
        let mut jobs = self
            .queue
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.queue.closed.store(true, Ordering::SeqCst);
        jobs.clear();
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
            .map_err(|_| Failure::Fetch)
            .and_then(|bytes| crate::media::decode(&bytes).map_err(|_| Failure::Decode))
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
    fn dropping_previews_closes_and_empties_pending_downloads() {
        let started = Arc::new(std::sync::Barrier::new(WORKERS + 1));
        let release = Arc::new(std::sync::Barrier::new(WORKERS + 1));
        let fetch: Fetch = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            Arc::new(move |_| {
                started.wait();
                release.wait();
                Err("offline".into())
            })
        };
        let mut previews = previews(fetch);
        for index in 0..WORKERS {
            previews.request(&format!("https://example.com/active-{index}.png"));
        }
        started.wait();
        previews.request("https://example.com/pending.png");
        let queue = Arc::clone(&previews.queue);
        drop(previews);
        let closed = queue.closed.load(Ordering::SeqCst);
        let empty = queue.jobs.lock().expect("queue lock").is_empty();
        release.wait();
        assert!(closed);
        assert!(empty);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Arc::strong_count(&queue) > 1 {
            assert!(Instant::now() < deadline, "download workers stayed alive");
            thread::sleep(Duration::from_millis(5));
        }
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
    fn completed_encoder_results_hold_capacity_until_polled() {
        let picker = Picker::halfblocks();
        let (mut p, encoder) = Previews::unstarted(picker.clone(), ok_fetch());
        let source = Arc::new(DynamicImage::ImageRgba8(RgbaImage::new(4, 2)));
        for index in 0..MAX_ENCODER {
            assert!(p
                .encoder
                .try_send(Job::Build {
                    key: (format!("old-{index}"), 8, 4),
                    gen: index as u64,
                    source: Arc::clone(&source),
                })
                .is_ok());
            let pending = encoder.jobs.try_recv().expect("queued encode");
            assert!(encoder.done.send(encode(&picker, pending)).is_ok());
        }
        assert_eq!(p.encoder.outstanding.load(Ordering::Relaxed), MAX_ENCODER);
        let rejected = p.encoder.try_send(Job::Build {
            key: ("deferred".into(), 8, 4),
            gen: 0,
            source: Arc::clone(&source),
        });
        assert!(matches!(rejected, Err(Job::Build { .. })));
        p.poll();
        assert_eq!(p.encoder.outstanding.load(Ordering::Relaxed), 0);
        assert!(p
            .encoder
            .try_send(Job::Build {
                key: ("retry".into(), 8, 4),
                gen: 1,
                source,
            })
            .is_ok());
    }

    #[test]
    fn a_rejected_resize_keeps_its_protocol_for_the_next_draw() {
        let picker = Picker::halfblocks();
        let (p, encoder) = Previews::unstarted(picker.clone(), ok_fetch());
        let image = DynamicImage::ImageRgba8(RgbaImage::new(4, 2));
        let source = Arc::new(image.clone());
        for index in 0..MAX_ENCODER {
            assert!(p
                .encoder
                .try_send(Job::Build {
                    key: (format!("queued-{index}"), 8, 4),
                    gen: index as u64,
                    source: Arc::clone(&source),
                })
                .is_ok());
        }
        let mut protocol = KeyedProtocol {
            inner: Some(picker.new_resize_protocol(image)),
            tx: p.encoder.clone(),
            pending: false,
            key: ("current".into(), 8, 4),
            gen: 0,
        };
        protocol.resize_encode(&SCALE, AREA);
        assert!(protocol.inner.is_some());
        assert!(!protocol.pending);
        drop(encoder.jobs.try_recv().expect("free one permit"));
        protocol.resize_encode(&SCALE, AREA);
        assert!(protocol.inner.is_none());
        assert!(protocol.pending);
        assert_eq!(p.encoder.outstanding.load(Ordering::Relaxed), MAX_ENCODER);
    }

    #[test]
    fn rapid_resize_does_not_replace_inflight_image_variants() {
        let picker = Picker::halfblocks();
        let (mut p, _encoder) = Previews::unstarted(picker, ok_fetch());
        p.request("a");
        wait_for(&mut p, "a", Shown::Ready);
        p.next_frame();
        assert!(p.encoded("a", Rect::new(0, 0, 6, 3)).is_none());
        p.next_frame();
        assert!(p.encoded("a", Rect::new(0, 0, 12, 6)).is_none());
        let keys = |p: &Previews| match p.entries.get("a") {
            Some(Entry::Ready { variants, .. }) => variants
                .iter()
                .map(|variant| variant.protocol.key.clone())
                .collect::<Vec<_>>(),
            _ => panic!("ready image"),
        };
        let original = keys(&p);
        assert_eq!(p.encoder.outstanding.load(Ordering::Relaxed), MAX_VARIANTS);
        for width in 20..120 {
            p.next_frame();
            assert!(p.encoded("a", Rect::new(0, 0, width, 4)).is_none());
        }
        assert_eq!(keys(&p), original);
        assert_eq!(p.encoder.outstanding.load(Ordering::Relaxed), MAX_VARIANTS);
    }

    fn wait_for_retry(previews: &mut Previews, url: &str, now: Instant) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            previews.poll_at(now);
            if matches!(
                previews.entries.get(url),
                Some(Entry::Waiting { attempts: 1, .. })
            ) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{url} never entered retry backoff"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn an_offline_preview_recovers_without_a_new_reader() {
        let now = Instant::now();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let bytes = png();
        let mut p = previews(Arc::new(move |_| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("offline".into())
            } else {
                Ok(bytes.clone())
            }
        }));
        p.request_at("a", now);
        wait_for_retry(&mut p, "a", now);
        assert_eq!(p.shown("a"), Shown::Loading);
        assert!(!p.busy(), "backoff does not keep the spinner running");
        p.poll_at(now + Duration::from_millis(999));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        p.poll_at(now + Duration::from_secs(1));
        wait_for(&mut p, "a", Shown::Ready);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn offscreen_previews_wait_until_requested_again_before_retrying() {
        let now = Instant::now();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let mut p = previews(Arc::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Err("offline".into())
        }));
        p.next_frame();
        p.request_at("a", now);
        wait_for_retry(&mut p, "a", now);
        p.next_frame();
        p.poll_at(now + RETRY_MAX);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!p.busy());
        p.request_at("a", now + RETRY_MAX);
        assert!(p.busy());
    }

    #[test]
    fn repeated_fetch_failures_have_a_capped_retry_delay() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(2));
        assert_eq!(retry_delay(5), Duration::from_secs(16));
        assert_eq!(retry_delay(6), RETRY_MAX);
        assert_eq!(retry_delay(u8::MAX), RETRY_MAX);
    }

    #[test]
    fn a_full_download_queue_defers_work_and_bounds_avatar_metadata() {
        let now = Instant::now();
        let started = Arc::new(std::sync::Barrier::new(WORKERS + 1));
        let release = Arc::new(std::sync::Barrier::new(WORKERS + 1));
        let calls = Arc::new(AtomicUsize::new(0));
        let fetch: Fetch = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            let count = Arc::clone(&calls);
            let bytes = png();
            Arc::new(move |_| {
                if count.fetch_add(1, Ordering::SeqCst) < WORKERS {
                    started.wait();
                    release.wait();
                }
                Ok(bytes.clone())
            })
        };
        let mut p = previews(fetch);
        for index in 0..WORKERS {
            p.request_at(&format!("active-{index}"), now);
        }
        started.wait();
        for index in 0..(MAX_PENDING + MAX_ENTRIES) {
            p.request_avatar(&format!("queued-{index}"), false);
        }
        let pending = p.queue.jobs.lock().expect("queue").len();
        let remembered = p.entries.len();
        let avatar_count = p.avatars.len();
        let deferred = format!("queued-{}", MAX_PENDING + MAX_ENTRIES - 1);
        let waits = matches!(p.entries.get(&deferred), Some(Entry::Waiting { .. }));
        release.wait();
        assert_eq!(pending, MAX_PENDING);
        assert!(remembered <= MAX_ENTRIES);
        assert!(avatar_count <= MAX_ENTRIES);
        assert!(waits);
        let deadline = Instant::now() + Duration::from_secs(30);
        while p.busy() {
            p.poll_at(now);
            assert!(Instant::now() < deadline, "queued downloads did not finish");
            thread::sleep(Duration::from_millis(5));
        }
        p.poll_at(now + RETRY_MAX);
        let deadline = Instant::now() + Duration::from_secs(30);
        while p.shown(&deferred) != Shown::Ready {
            p.poll_at(now + RETRY_MAX);
            assert!(
                Instant::now() < deadline,
                "deferred downloads did not finish"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(p.avatars.keys().all(|url| p.entries.contains_key(url)));
    }

    #[test]
    fn completed_downloads_count_toward_the_outstanding_bound_until_polled() {
        let (completed, received) = std::sync::mpsc::channel();
        let bytes = png();
        let mut p = previews(Arc::new(move |_| {
            completed.send(()).expect("completion observer");
            Ok(bytes.clone())
        }));
        for index in 0..MAX_OUTSTANDING {
            p.request(&format!("completed-{index}"));
            received
                .recv_timeout(Duration::from_secs(2))
                .expect("worker drained the queue");
        }
        assert!(p.queue.jobs.lock().expect("queue").is_empty());
        for index in 0..MAX_ENTRIES {
            p.request_avatar(&format!("deferred-{index}"), false);
        }
        let outstanding = p
            .entries
            .values()
            .filter(|entry| matches!(entry, Entry::Loading { .. }))
            .count();
        assert_eq!(outstanding, MAX_OUTSTANDING);
        assert!(p.entries.len() <= MAX_ENTRIES);
        assert!(p.avatars.len() <= MAX_ENTRIES);
        assert!(p.queue.jobs.lock().expect("queue").is_empty());
        assert!(matches!(
            p.entries.get(&format!("deferred-{}", MAX_ENTRIES - 1)),
            Some(Entry::Waiting { .. })
        ));
    }

    #[test]
    fn failed_cache_entries_release_their_avatar_metadata_on_eviction() {
        let mut p = previews(ok_fetch());
        for index in 0..=MAX_ENTRIES {
            let url = format!("failed-{index}");
            p.avatars.insert(url.clone(), false);
            p.entries.insert(url, Entry::Failed { used: index as u64 });
            p.evict();
        }
        assert_eq!(p.entries.len(), MAX_ENTRIES);
        assert_eq!(p.avatars.len(), MAX_ENTRIES);
        assert!(!p.entries.contains_key("failed-0"));
        assert!(!p.avatars.contains_key("failed-0"));
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
