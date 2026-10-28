//! The browser session: the reader, a viewport, and a way in.
//!
//! The page paints the cells. This module does not open a terminal. A live session reads the
//! config the page stored and talks to the networks through the worker's HTTP import. The demo
//! session is the same world the native binary installs, and a key runs the same reducer.

use chrono::{DateTime, Utc};
use ratatui::layout::Rect;
use ratatui::Terminal;

use crate::clock::Instant;
use crate::config::Config;
use crate::grid::{self, Grid};
use crate::key::{self, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::media::{ImageMode, Viewer};
use crate::net::Clients;
use crate::state::{self, State};
use crate::theme::{Choice, Detail, IconMode, Kind, Scheme};
use crate::tui;

/// The demo reader for one page. Nothing here is saved, and nothing is sent.
pub struct Session {
    state: State,
    clients: Clients,
    ui: tui::Ui,
    terminal: Terminal<Grid>,
    frame: Vec<u8>,
    /// Bytes the page reads until the next call. Native tests never touch them.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    text: Vec<u8>,
    /// Cell width over height, measured by the page. Native layout measures its own font.
    #[cfg_attr(feature = "native", allow(dead_code))]
    cell_ratio: f32,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    copied: Vec<u8>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    opened: Vec<u8>,
    /// Picture boxes from the last draw, as JSON. The page overlays the bytes.
    #[cfg_attr(feature = "native", allow(dead_code))]
    picture_list: Vec<u8>,
    #[cfg_attr(feature = "native", allow(dead_code))]
    picture_bytes: Vec<u8>,
}

impl Session {
    /// A dark or light demo at the visitor's current time.
    pub fn new(cols: u16, rows: u16, dark: bool) -> Self {
        Self::at(cols, rows, dark, crate::clock::utc_now())
    }

    /// `now` is frozen, so ages on the posts stay put for the rest of the visit.
    pub fn at(cols: u16, rows: u16, dark: bool, now: DateTime<Utc>) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut session = Self {
            state: demo_state(dark, now),
            clients: Clients::empty(),
            ui: tui::demo_ui(),
            terminal: Terminal::new(Grid::new(cols, rows)).expect("the grid reports its size"),
            frame: Vec::new(),
            text: Vec::new(),
            copied: Vec::new(),
            opened: Vec::new(),
            picture_list: Vec::new(),
            picture_bytes: Vec::new(),
            cell_ratio: 0.5,
        };
        session.redraw();
        session
    }

    /// A signed-out or signed-in reader. `config` and `clients` are explicit, so a test never reads the user's file.
    pub fn live(cols: u16, rows: u16, dark: bool, config: Config, mut clients: Clients) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut state = State::boot(false, state::page_count(15), &config, &clients, None);
        state.unfurl = false;
        state.icons = page_icons(config.ui.icons.as_deref());
        state.icon_mode = if state.icons {
            IconMode::Nerd
        } else {
            IconMode::Unicode
        };
        state.scheme = config
            .ui
            .theme
            .as_deref()
            .and_then(Scheme::parse)
            .unwrap_or(if dark { Scheme::Black } else { Scheme::White });
        state.auto_dark = dark;
        state.detail = config
            .ui
            .detail
            .as_deref()
            .and_then(Detail::parse)
            .unwrap_or(Detail::Rich);
        state.retheme();
        if !clients.signed_in().is_empty() {
            if let Err(err) = state.ensure_loaded(&mut clients) {
                state.fail(err);
            }
        }
        let mut session = Self {
            state,
            clients,
            ui: tui::demo_ui(),
            terminal: Terminal::new(Grid::new(cols, rows)).expect("the grid reports its size"),
            frame: Vec::new(),
            text: Vec::new(),
            copied: Vec::new(),
            opened: Vec::new(),
            picture_list: Vec::new(),
            picture_bytes: Vec::new(),
            cell_ratio: 0.5,
        };
        session.settle();
        session
    }

    /// One key, in the browser's byte encoding.
    pub fn key(&mut self, bytes: &[u8]) {
        self.state.expire(Instant::now());
        if let Some(event) = key::decode(bytes) {
            if let Err(err) =
                tui::handle_key(&mut self.state, &mut self.clients, &mut self.ui, event)
            {
                self.state.fail(err);
            }
        }
        self.settle();
    }

    pub fn paste(&mut self, text: &str) {
        self.state.expire(Instant::now());
        tui::paste(&mut self.ui, text);
        self.redraw();
    }

    /**
    A pointer event. `kind` is 0 down, 1 up, 2 moved, 3 scroll down, 4 scroll up.
    `button` is 0 left, 1 right, 2 middle.
    */
    pub fn pointer(&mut self, kind: u8, col: u16, row: u16, button: u8) {
        self.state.expire(Instant::now());
        if let Some(mouse) = pointer_event(kind, col, row, button) {
            if let Err(err) =
                tui::handle_mouse(&mut self.state, &mut self.clients, &mut self.ui, mouse)
            {
                self.state.fail(err);
            }
        }
        self.settle();
    }

    pub fn resize(&mut self, cols: u16, rows: u16, ratio: f32) {
        if ratio.is_finite() && (0.2..2.0).contains(&ratio) {
            self.cell_ratio = ratio;
        }
        let cols = cols.max(1);
        let rows = rows.max(1);
        self.terminal.backend_mut().resize(cols, rows);
        if let Err(err) = self.terminal.resize(Rect::new(0, 0, cols, rows)) {
            self.state.fail(err.to_string());
            return;
        }
        self.redraw();
        if self.pull_preview() {
            self.redraw();
        }
    }

    /// Updates the page's appearance while keeping its reader state and named theme.
    pub fn set_dark(&mut self, dark: bool) {
        self.state.auto_dark = dark;
        if matches!(self.state.scheme, Scheme::Black | Scheme::White) {
            self.state.scheme = if dark { Scheme::Black } else { Scheme::White };
        }
        self.state.retheme();
        self.redraw();
    }

    /// Drops a confirmation once its four seconds are up, and keeps the welcome water moving with no pointer event.
    pub fn tick(&mut self) {
        let expired = self.state.expire(Instant::now());
        let (width, height) = self
            .terminal
            .size()
            .map(|size| (usize::from(size.width), usize::from(size.height)))
            .unwrap_or((0, 0));
        if expired || tui::clock_runs(&self.state, &self.ui, width, height) {
            self.redraw();
        }
    }

    /// The cells that changed since the last call.
    pub fn frame(&mut self) -> &[u8] {
        self.frame = self.terminal.backend_mut().take_frame();
        &self.frame
    }

    /// The same screen as plain text, for the live region.
    pub fn text(&self) -> String {
        grid::plain_text(self.terminal.backend().buffer())
    }

    /// Column and row when the cursor is shown.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.terminal.backend().cursor()
    }

    #[cfg(test)]
    pub(crate) fn buffer(&self) -> &ratatui::buffer::Buffer {
        self.terminal.backend().buffer()
    }

    /// Draws, then fetches a due page. At most eight pages, so one key cannot walk the whole timeline.
    fn settle(&mut self) {
        self.redraw();
        for _ in 0..8 {
            let due = self.state.page_ask || self.state.show_page_card;
            if !due {
                break;
            }
            if !self.pull_page() {
                self.redraw();
                break;
            }
            self.redraw();
        }
        if self.pull_preview() {
            self.redraw();
        }
    }

    /// One conversation for the right pane. The feed's page fetch is separate, and this does not mark it paging.
    fn pull_preview(&mut self) -> bool {
        if !self.ui.is_split() {
            return false;
        }
        let plan = match self.state.plan_preview() {
            Ok(Some(plan)) => plan,
            Ok(None) => return false,
            Err(err) => {
                self.state.fail(err);
                return true;
            }
        };
        let fetched = state::fetch_page(&mut self.clients, &plan);
        if let Err(err) = self.state.store_page(plan, fetched) {
            self.state.fail(err);
        }
        true
    }

    fn pull_page(&mut self) -> bool {
        let plan = match self.state.plan_more() {
            Ok(Some(plan)) => plan,
            Ok(None) => {
                if self.state.page_ask {
                    self.state.fail("no further page");
                }
                self.stop_page();
                return false;
            }
            Err(err) => {
                self.state.fail(err);
                self.stop_page();
                return false;
            }
        };
        self.state.paging = true;
        self.state.page_ask = false;
        if let Some(id) = self.state.current_feed_id() {
            self.state.page_gate = Some((id, self.state.selected_post_id()));
        }
        let fetched = state::fetch_page(&mut self.clients, &plan);
        self.state.paging = false;
        if let Err(err) = self.state.store_page(plan, fetched) {
            self.state.fail(err);
        }
        true
    }

    fn stop_page(&mut self) {
        self.state.page_ask = false;
        if let Some(id) = self.state.current_feed_id() {
            self.state.page_gate = Some((id, self.state.selected_post_id()));
        }
    }

    fn redraw(&mut self) {
        #[cfg(not(feature = "native"))]
        self.sync_page_pictures();
        let fg = self.state.theme.rgb(Kind::Fg);
        let bg = self.state.theme.row_bg(false);
        {
            let Self {
                terminal,
                state,
                ui,
                ..
            } = self;
            terminal.backend_mut().set_fallback(fg, bg);
            if let Err(err) = terminal.draw(|frame| tui::render_frame(frame, state, ui)) {
                state.fail(err.to_string());
            }
        }
        #[cfg(not(feature = "native"))]
        self.stash_pictures();
    }

    /// Rich mode paints pictures on the page. `ui.images = off` keeps the image card.
    ///
    /// The demo always does. A signed-in timeline does unless images are off. An `http(s)` URL is
    /// drawn by the browser; `demo://` still comes from generated PNG bytes.
    #[cfg(not(feature = "native"))]
    fn sync_page_pictures(&mut self) {
        let on = self.state.demo || self.state.viewer.mode != ImageMode::Off;
        if on {
            self.state.graphics = true;
            self.state.preview_ratio = Some(self.cell_ratio);
            self.state.pictures = Some("this page");
            self.ui.use_page_pictures(true);
        } else if self.ui.use_page_pictures(false) {
            self.state.graphics = false;
            self.state.preview_ratio = None;
            self.state.pictures = None;
        }
    }

    #[cfg(not(feature = "native"))]
    fn stash_pictures(&mut self) {
        self.picture_list = self.ui.places_json().into_bytes();
    }

    #[cfg(not(feature = "native"))]
    fn take_picture(&mut self, url: &str) {
        self.picture_bytes = self.ui.picture_png(url);
    }

    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fn stash(slot: &mut Vec<u8>, text: Option<String>) -> (*const u8, usize) {
        slot.clear();
        let Some(text) = text else {
            return (std::ptr::null(), 0);
        };
        slot.extend_from_slice(text.as_bytes());
        if slot.is_empty() {
            return (std::ptr::null(), 0);
        }
        (slot.as_ptr(), slot.len())
    }
}

/// The page ships the Nerd Font glyphs Ghostty draws. `ui.icons = unicode` keeps the other set.
fn page_icons(icons: Option<&str>) -> bool {
    !matches!(icons.and_then(IconMode::parse), Some(IconMode::Unicode))
}

/// Demo mode, the frozen clock, rich detail, Nerd Font icons, images off, and a black or white theme.
fn demo_state(dark: bool, now: DateTime<Utc>) -> State {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.frozen_now = Some(now);
    state.install_demo();
    state.status = state::DEMO_NOTICE.into();
    state.detail = Detail::Rich;
    state.icons = true;
    state.icon_mode = IconMode::Nerd;
    state.unfurl = false;
    state.viewer = Viewer::new(ImageMode::Off);
    state.scheme = if dark { Scheme::Black } else { Scheme::White };
    state.auto_dark = dark;
    state.retheme();
    state
}

fn pointer_event(kind: u8, col: u16, row: u16, button: u8) -> Option<MouseEvent> {
    let button = match button {
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        _ => MouseButton::Left,
    };
    let kind = match kind {
        0 => MouseEventKind::Down(button),
        1 => MouseEventKind::Up(button),
        2 => MouseEventKind::Moved,
        3 => MouseEventKind::ScrollDown,
        4 => MouseEventKind::ScrollUp,
        _ => return None,
    };
    Some(MouseEvent {
        kind,
        column: col,
        row,
        modifiers: KeyModifiers::empty(),
    })
}

#[cfg(target_arch = "wasm32")]
mod ffi {
    use super::Session;

    /// # Safety
    /// `millis` is `Date.now()`. The pointer is owned by the caller until [`socli_free`].
    #[no_mangle]
    pub unsafe extern "C" fn socli_new(
        cols: u16,
        rows: u16,
        dark: u8,
        millis: f64,
    ) -> *mut Session {
        let _ = millis;
        let (config, failed) = match crate::config::load() {
            Ok(config) => (config, None),
            Err(err) => (crate::config::Config::default(), Some(err)),
        };
        let (clients, failed) = match crate::net::Clients::load(&config) {
            Ok(clients) => (clients, failed),
            Err(err) => (crate::net::Clients::empty(), Some(err)),
        };
        let mut session = Session::live(cols, rows, dark != 0, config, clients);
        if let Some(err) = failed {
            session.state.fail(err);
            session.redraw();
        }
        Box::into_raw(Box::new(session))
    }

    /**
    # Safety
    `ptr` is null or an aligned, live session from [`socli_new`]. The caller exclusively
    owns the session for this call and has not called [`socli_free`].
    */
    #[no_mangle]
    pub unsafe extern "C" fn socli_theme(ptr: *mut Session, dark: u8) {
        if let Some(session) =
            // SAFETY: `ptr` is null or the aligned `Box<Session>` from `socli_new`, and the host has exclusive access until `socli_free`.
            unsafe { ptr.as_mut() }
        {
            session.set_dark(dark != 0);
        }
    }

    /// # Safety
    /// `ptr` is null or a session from [`socli_new`] that has not been freed.
    #[no_mangle]
    pub unsafe extern "C" fn socli_free(ptr: *mut Session) {
        if !ptr.is_null() {
            drop(Box::from_raw(ptr));
        }
    }

    /// # Safety
    /// `ptr` is a live session. `bytes` points at `len` bytes, or is null when `len` is 0.
    #[no_mangle]
    pub unsafe extern "C" fn socli_key(ptr: *mut Session, bytes: *const u8, len: usize) {
        let Some(session) = ptr.as_mut() else {
            return;
        };
        let bytes = if bytes.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(bytes, len)
        };
        session.key(bytes);
    }

    /// # Safety
    /// `ptr` is a live session. `bytes` points at `len` bytes, or is null when `len` is 0.
    #[no_mangle]
    pub unsafe extern "C" fn socli_paste(ptr: *mut Session, bytes: *const u8, len: usize) {
        let Some(session) = ptr.as_mut() else {
            return;
        };
        let text = if bytes.is_null() || len == 0 {
            String::new()
        } else {
            String::from_utf8_lossy(std::slice::from_raw_parts(bytes, len)).into_owned()
        };
        session.paste(&text);
    }

    /// # Safety
    /// `ptr` is a live session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_pointer(
        ptr: *mut Session,
        kind: u8,
        col: u16,
        row: u16,
        button: u8,
    ) {
        if let Some(session) = ptr.as_mut() {
            session.pointer(kind, col, row, button);
        }
    }

    /// # Safety
    /// `ptr` is a live session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_resize(ptr: *mut Session, cols: u16, rows: u16, ratio: f32) {
        if let Some(session) = ptr.as_mut() {
            session.resize(cols, rows, ratio);
        }
    }

    /// # Safety
    /// `ptr` is a live session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_tick(ptr: *mut Session) {
        if let Some(session) = ptr.as_mut() {
            session.tick();
        }
    }

    /// # Safety
    /// `ptr` is a live session. `out_len` is null or points at a `usize`.
    /// The returned bytes live until the next call on this session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_frame(ptr: *mut Session, out_len: *mut usize) -> *const u8 {
        let Some(session) = ptr.as_mut() else {
            write_len(out_len, 0);
            return std::ptr::null();
        };
        let len = session.frame().len();
        write_len(out_len, len);
        if len == 0 {
            std::ptr::null()
        } else {
            session.frame.as_ptr()
        }
    }

    /// # Safety
    /// `ptr` is a live session. `out_len` is null or points at a `usize`.
    /// The returned bytes live until the next call on this session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_text(ptr: *mut Session, out_len: *mut usize) -> *const u8 {
        let Some(session) = ptr.as_mut() else {
            write_len(out_len, 0);
            return std::ptr::null();
        };
        let text = session.text();
        session.text = text.into_bytes();
        write_len(out_len, session.text.len());
        if session.text.is_empty() {
            std::ptr::null()
        } else {
            session.text.as_ptr()
        }
    }

    /// # Safety
    /// `ptr` is a live session. The returned bytes live until the next call on this session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_take_clipboard(
        ptr: *mut Session,
        out_len: *mut usize,
    ) -> *const u8 {
        let Some(session) = ptr.as_mut() else {
            write_len(out_len, 0);
            return std::ptr::null();
        };
        let (bytes, len) = Session::stash(&mut session.copied, crate::clip::take_copied());
        write_len(out_len, len);
        bytes
    }

    /// # Safety
    /// `ptr` is a live session. The returned bytes live until the next call on this session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_take_open(ptr: *mut Session, out_len: *mut usize) -> *const u8 {
        let Some(session) = ptr.as_mut() else {
            write_len(out_len, 0);
            return std::ptr::null();
        };
        let (bytes, len) = Session::stash(&mut session.opened, crate::clip::take_open());
        write_len(out_len, len);
        bytes
    }

    /// Column in the high 16 bits and row in the low 16, or `u32::MAX` when the cursor is hidden.
    ///
    /// # Safety
    /// `ptr` is null or a live session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_cursor(ptr: *const Session) -> u32 {
        let Some(session) = ptr.as_ref() else {
            return u32::MAX;
        };
        match session.cursor() {
            Some((col, row)) => (u32::from(col) << 16) | u32::from(row),
            None => u32::MAX,
        }
    }

    /// Picture boxes from the last draw, as JSON. The bytes live until the next call on this session.
    ///
    /// # Safety
    /// `ptr` is a live session. `out_len` is null or points at a `usize`.
    #[no_mangle]
    pub unsafe extern "C" fn socli_pictures(ptr: *mut Session, out_len: *mut usize) -> *const u8 {
        let Some(session) = ptr.as_mut() else {
            write_len(out_len, 0);
            return std::ptr::null();
        };
        write_len(out_len, session.picture_list.len());
        if session.picture_list.is_empty() {
            std::ptr::null()
        } else {
            session.picture_list.as_ptr()
        }
    }

    /// PNG bytes for a `demo://` picture already drawn. Empty when the page has not loaded that name.
    ///
    /// # Safety
    /// `ptr` is a live session. `url` points at `len` bytes, or is null when `len` is 0.
    /// The returned bytes live until the next call on this session.
    #[no_mangle]
    pub unsafe extern "C" fn socli_picture(
        ptr: *mut Session,
        url: *const u8,
        len: usize,
        out_len: *mut usize,
    ) -> *const u8 {
        let Some(session) = ptr.as_mut() else {
            write_len(out_len, 0);
            return std::ptr::null();
        };
        let name = if url.is_null() || len == 0 {
            ""
        } else {
            std::str::from_utf8(std::slice::from_raw_parts(url, len)).unwrap_or("")
        };
        session.take_picture(name);
        write_len(out_len, session.picture_bytes.len());
        if session.picture_bytes.is_empty() {
            std::ptr::null()
        } else {
            session.picture_bytes.as_ptr()
        }
    }

    /// Finishes a page sign-in before the session boots. `0` is success. `n` is the error length.
    /// [`usize::MAX`] means `out_cap` was too small.
    ///
    /// # Safety
    /// `code_ptr` and `state_ptr` point at their lengths, or are null when the length is 0.
    /// `out_ptr` has room for `out_cap` bytes.
    #[no_mangle]
    pub unsafe extern "C" fn socli_finish_oauth(
        code_ptr: *const u8,
        code_len: usize,
        state_ptr: *const u8,
        state_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> usize {
        let code = text_at(code_ptr, code_len);
        let state = text_at(state_ptr, state_len);
        match finish_oauth(&code, &state) {
            Ok(()) => 0,
            Err(err) => write_text(out_ptr, out_cap, &err),
        }
    }

    /// Shows `text` in the error footer until the next key clears it.
    ///
    /// # Safety
    /// `ptr` is a live session. `bytes` points at `len` bytes, or is null when `len` is 0.
    #[no_mangle]
    pub unsafe extern "C" fn socli_notice(ptr: *mut Session, bytes: *const u8, len: usize) {
        let Some(session) = ptr.as_mut() else {
            return;
        };
        let text = text_at(bytes, len);
        if text.is_empty() {
            return;
        }
        session.state.fail(text);
        session.redraw();
    }

    unsafe fn text_at(ptr: *const u8, len: usize) -> String {
        if ptr.is_null() || len == 0 {
            String::new()
        } else {
            String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
        }
    }

    unsafe fn write_text(out: *mut u8, cap: usize, text: &str) -> usize {
        let bytes = text.as_bytes();
        if out.is_null() || bytes.len() > cap {
            return usize::MAX;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
        bytes.len()
    }

    fn finish_oauth(code: &str, state: &str) -> Result<(), String> {
        let cfg = crate::config::load()?;
        if !state.is_empty() && cfg.x.oauth_state.as_deref() == Some(state) {
            return crate::net::x::auth::finish_page(code, state);
        }
        if !state.is_empty() && cfg.mastodon.oauth_state.as_deref() == Some(state) {
            return crate::net::mastodon::finish_page(code, state);
        }
        if !state.is_empty() && cfg.threads.oauth_state.as_deref() == Some(state) {
            if let Some(message) = crate::net::Network::Threads.hold() {
                let _ = crate::config::update(|cfg| {
                    cfg.threads.oauth_state = None;
                    cfg.threads.oauth_redirect = None;
                });
                return Err(message.into());
            }
            return crate::net::threads::finish_page(code, state);
        }
        let _ = crate::config::update(|cfg| {
            cfg.x.oauth_state = None;
            cfg.x.oauth_verifier = None;
            cfg.x.oauth_redirect = None;
            cfg.mastodon.oauth_state = None;
            cfg.mastodon.oauth_verifier = None;
            cfg.mastodon.oauth_redirect = None;
            cfg.threads.oauth_state = None;
            cfg.threads.oauth_redirect = None;
        });
        Err("this sign-in has expired. start it again.".into())
    }

    /// A zeroed buffer of `len` bytes. Pair it with [`socli_free_bytes`].
    #[no_mangle]
    pub extern "C" fn socli_alloc(len: usize) -> *mut u8 {
        let mut buf = vec![0u8; len];
        let ptr = buf.as_mut_ptr();
        std::mem::forget(buf);
        ptr
    }

    /// # Safety
    /// `ptr` came from [`socli_alloc`] with the same `len`, and is freed once.
    #[no_mangle]
    pub unsafe extern "C" fn socli_free_bytes(ptr: *mut u8, len: usize) {
        if ptr.is_null() || len == 0 {
            return;
        }
        drop(Vec::from_raw_parts(ptr, len, len));
    }

    fn write_len(out: *mut usize, len: usize) {
        if !out.is_null() {
            // SAFETY: `out` is non-null and, by the export contract, points at one aligned writable `usize`; the worker allocates those 4 bytes with `socli_alloc` and reads them back as a little-endian u32.
            unsafe { *out = len };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use crate::fixtures;
    use crate::model::FeedId;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    const SCRIPT: &[&[u8]] = &[
        b"j",
        b"j",
        b"k",
        &[0x0D],
        &[0x1B],
        b"d",
        b"?",
        &[0x1B],
        b"f",
        &[0x1B],
        b"n",
        &[0x1B],
    ];

    struct Mirror {
        state: State,
        clients: Clients,
        ui: tui::Ui,
        terminal: Terminal<TestBackend>,
    }

    impl Mirror {
        fn at(cols: u16, rows: u16, dark: bool, now: DateTime<Utc>) -> Self {
            let mut mirror = Self {
                state: demo_state(dark, now),
                clients: Clients::empty(),
                ui: tui::demo_ui(),
                terminal: Terminal::new(TestBackend::new(cols, rows)).expect("test backend"),
            };
            mirror.redraw();
            mirror
        }

        fn key(&mut self, bytes: &[u8]) {
            self.state.expire(Instant::now());
            if let Some(event) = key::decode(bytes) {
                if let Err(err) =
                    tui::handle_key(&mut self.state, &mut self.clients, &mut self.ui, event)
                {
                    self.state.fail(err);
                }
            }
            self.redraw();
        }

        fn redraw(&mut self) {
            let Self {
                terminal,
                state,
                ui,
                ..
            } = self;
            terminal
                .draw(|frame| tui::render_frame(frame, state, ui))
                .expect("draw");
        }
    }

    fn assert_same(session: &Session, mirror: &Mirror) {
        let got = session.buffer();
        let want = mirror.terminal.backend().buffer();
        assert_eq!(got.area, want.area);
        for y in 0..got.area.height {
            for x in 0..got.area.width {
                assert_eq!(
                    &got[(x, y)],
                    &want[(x, y)],
                    "cell {x},{y}\n{}",
                    grid::plain_text(got)
                );
            }
        }
        assert_eq!(session.text(), grid::plain_text(want));
    }

    #[test]
    fn theme_changes_preserve_the_reader_session() {
        let mut session = Session::at(100, 40, true, fixtures::frozen_now());
        session.key(b"j");
        session.key(b"?");
        let text = session.text();
        session.frame();
        session.set_dark(false);
        assert_eq!(session.text(), text);
        assert_eq!(session.state.scheme, Scheme::White);
        assert!(session.state.demo);
        assert!(!session.frame().is_empty());
        session.state.scheme = Scheme::parse("catppuccin").expect("known theme");
        let named = session.state.scheme;
        session.set_dark(true);
        assert_eq!(session.state.scheme, named);
        assert!(session.state.auto_dark);
    }

    #[test]
    fn the_demo_world_has_every_network_and_mina() {
        let world = demo::world(fixtures::frozen_now());
        assert_eq!(world.posts.len(), 18);
        assert_eq!(world.feed_ids(&FeedId::Merged).len(), 17);
        assert!(world
            .posts
            .values()
            .any(|post| post.author.name == "Mina Cho"));
        let session = Session::at(100, 40, true, fixtures::frozen_now());
        assert!(session.text().contains("Nuno Park"), "{}", session.text());
        assert!(session.text().contains("demo"));
        assert!(!session.state.overview);
    }

    #[test]
    fn the_session_matches_the_terminal_cell_for_cell() {
        let now = fixtures::frozen_now();
        let mut session = Session::at(100, 40, true, now);
        let mut mirror = Mirror::at(100, 40, true, now);
        assert_same(&session, &mirror);
        for key in SCRIPT {
            session.key(key);
            mirror.key(key);
            assert_same(&session, &mirror);
        }
    }

    #[test]
    fn a_like_changes_the_screen_and_stays_here() {
        let mut session = Session::at(100, 40, true, fixtures::frozen_now());
        session.key(b"l");
        assert!(
            session.state.status.contains("here only"),
            "{}",
            session.state.status
        );
        assert!(session.state.posts.values().any(|post| post.liked));
    }

    #[test]
    fn a_wide_screen_splits_the_same_way() {
        let now = fixtures::frozen_now();
        let session = Session::at(160, 44, false, now);
        let mirror = Mirror::at(160, 44, false, now);
        assert_same(&session, &mirror);
        assert!(session.state.overview);
    }

    #[test]
    fn refresh_stays_on_the_demo() {
        let mut session = Session::at(100, 40, true, fixtures::frozen_now());
        session.key(&[0x12]);
        assert_eq!(session.state.status, "demo · already local");
    }

    #[test]
    fn a_live_session_without_accounts_opens_the_welcome() {
        let session = Session::live(80, 24, true, Config::default(), Clients::empty());
        let text = session.text();
        assert!(!text.contains("Nuno"), "{text}");
        assert!(text.contains("Follow the conversation."), "{text}");
    }

    #[test]
    fn the_welcome_keeps_moving_without_a_key() {
        let mut session = Session::live(100, 32, true, Config::default(), Clients::empty());
        let before = session.text();
        let mut moved = false;
        for _ in 0..8 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            session.tick();
            if session.text() != before {
                moved = true;
                break;
            }
        }
        assert!(moved, "the welcome frame stayed still:\n{before}");
    }

    #[test]
    fn the_first_frame_is_a_diff_and_the_next_is_empty() {
        let mut session = Session::at(80, 24, true, fixtures::frozen_now());
        let frame = session.frame().to_vec();
        assert!(!frame.is_empty());
        let mut at = 0;
        let mut cells = 0;
        while at + grid::RECORD <= frame.len() {
            let col = u16::from_le_bytes([frame[at], frame[at + 1]]);
            let row = u16::from_le_bytes([frame[at + 2], frame[at + 3]]);
            let scalar = u32::from_le_bytes(frame[at + 4..at + 8].try_into().unwrap());
            let mods = frame[at + 14];
            at += grid::RECORD;
            if mods & 0x80 != 0 {
                let len = usize::from(frame[at]);
                at += 1 + len;
            }
            let symbol = session.buffer()[(col, row)].symbol();
            let expect = symbol.chars().next().map(u32::from).unwrap_or(0);
            assert_eq!(scalar, expect, "{col},{row} {symbol:?}");
            cells += 1;
        }
        assert_eq!(at, frame.len());
        assert!(cells > 0);
        assert!(session.frame().is_empty());
    }

    #[test]
    fn the_page_uses_nerd_icons_unless_unicode_is_configured() {
        assert!(page_icons(None));
        assert!(page_icons(Some("auto")));
        assert!(page_icons(Some("nerd")));
        assert!(!page_icons(Some("unicode")));
    }
}
