//! Reader state. Demo mode and every live network share one model so the keys never change.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use crate::clock::Instant;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::command::Cmd;
use crate::config::{Config, Enter};
use crate::demo;
use crate::layout::{self, Target};
use crate::media::Viewer;
use crate::model::{Feed, FeedId, Link, Metrics, Page, Post, Scope, Section, User};
use crate::net::{self, Caps, Clients, Draft, FeedKind, NetError, NetInfo, Network, Provider};
use crate::render::{self, View, ViewOpts};
use crate::theme::{
    Choice, ColorBy, ColorMode, Density, Detail, IconMode, Layout, Replies, Scheme, Theme,
};
use crate::unfurl::Card;

type ProfilePage = Result<(User, Page), NetError>;

pub const NO_ACCOUNTS: &str =
    "no accounts yet. `socli login bluesky` (or x, devnull, threads, mastodon), or `socli --demo`.";
/// The `SoCLI>` prompt's answer to a tab word in All scope.
pub const PICK_NETWORK_FIRST: &str = "pick a network first: net <slug>";
/// The status at demo startup; the reader clears it on the first key press.
pub const DEMO_NOTICE: &str = "demo feed · socli login <network> for your timelines";
/// How many views back navigation remembers.
const HISTORY: usize = 64;
/// How long a confirmation stays in the status.
const CONFIRM_FOR: Duration = Duration::from_secs(4);
/// Posts the reader asks from each signed-in network on one page.
pub(crate) const PAGE_POSTS: u8 = 20;
/// The next page starts when the selection reaches this post of the current page, counting from 1.
pub(crate) const PREFETCH_AT: usize = 10;

/// The reader never asks a network for fewer than [`PAGE_POSTS`], even when `--count` is smaller.
pub(crate) fn page_count(count: u8) -> u8 {
    count.max(PAGE_POSTS)
}

/**
Index within the current page where a prefetch is due. A page shorter than [`PREFETCH_AT`]
is due on its last post, so a short answer does not stall.
*/
pub(crate) fn prefetch_from(len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some((PREFETCH_AT - 1).min(len - 1))
}

/// Ids that belong to the latest fetch. An empty `page_ids` means the whole list is that page.
fn page_members(feed: &Feed) -> HashSet<&str> {
    if feed.page_ids.is_empty() {
        feed.ids.iter().map(String::as_str).collect()
    } else {
        feed.page_ids.iter().map(String::as_str).collect()
    }
}

fn page_len(feed: &Feed) -> usize {
    let members = page_members(feed);
    feed.ids
        .iter()
        .filter(|id| members.contains(id.as_str()))
        .count()
}

/// The tabs of the All view, each a feed kind across the networks that are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergedTab {
    #[default]
    Home,
    Discover,
    Saved,
}

impl MergedTab {
    pub const ALL: [MergedTab; 3] = [MergedTab::Home, MergedTab::Discover, MergedTab::Saved];

    /// The crumb word, also what the `SoCLI>` prompt types.
    pub fn word(self) -> &'static str {
        match self {
            MergedTab::Home => "home",
            MergedTab::Discover => "discover",
            MergedTab::Saved => "saved",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            MergedTab::Home => "Home",
            MergedTab::Discover => "Discover",
            MergedTab::Saved => "Saved",
        }
    }

    /// The feed kind each network resolves, or None for every network's primary feed.
    fn kind(self) -> Option<FeedKind> {
        match self {
            MergedTab::Home => None,
            MergedTab::Discover => Some(FeedKind::Discover),
            MergedTab::Saved => Some(FeedKind::Bookmarks),
        }
    }

    fn of_kind(kind: FeedKind) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.kind() == Some(kind))
    }

    pub fn feed_id(self) -> FeedId {
        self.kind().map_or(FeedId::Merged, FeedId::Kind)
    }
}

pub struct State {
    pub reveal_shortcuts: bool,
    pub demo: bool,
    pub posts: HashMap<String, Post>,
    /// Every loaded post's author by network and lowercase handle, for profile headers.
    users: HashMap<(Network, String), User>,
    pub feeds: HashMap<FeedId, Feed>,
    pub section: Section,
    /// The networks the feed shows, in network order. The scope follows from it.
    pub on: Vec<Network>,
    /// The open feed tab of every network, kept while the scope moves elsewhere.
    pub tabs: HashMap<Network, FeedKind>,
    /// The All view's tab; it survives scope and filter changes and is never saved.
    pub merged_tab: MergedTab,
    /** What Home shows instead of the scope's own feed: one feed kind across networks, an opened thread, or a pinned post. */
    pub focus: Option<FeedId>,
    /// What the Search section lists: a search, or one account's posts.
    pub remote: Option<FeedId>,
    pub nets: Vec<NetInfo>,
    pub default_network: Option<Network>,
    pub expanded: HashSet<String>,
    /// List posts whose text is open past the three-line cap. Separate from [`Self::expanded`].
    pub opened: HashSet<String>,
    pub thread_focus: Option<String>,
    pub selected: HashMap<String, usize>,
    /// The split is on, so the Home feed lays out as `layout::overview` (see `overview_on`).
    pub overview: bool,
    pub remote_query: String,
    pub filter: Option<String>,
    pub status: String,
    /// When the confirmation the status shows expires, and its text.
    pub status_expiry: Option<(Instant, String)>,
    /// The error the status shows, which the next press clears.
    pub status_error: Option<String>,
    pub following: HashSet<String>,
    pub count: u8,
    pub viewer: Viewer,
    /// Cell width over height of the reader's inline previews; none while they are off and in `--print`.
    pub preview_ratio: Option<f32>,
    /// Whether the reader's terminal draws real pictures (kitty, iTerm2 or sixel), not half blocks.
    pub graphics: bool,
    /// What the reader draws pictures with (`kitty`, `iTerm2`, `sixel` or `blocks in this terminal`); none while images are off.
    pub pictures: Option<&'static str>,
    pub theme: Theme,
    pub scheme: Scheme,
    pub color_mode: ColorMode,
    /// What `auto` resolves to: the terminal background, read once at startup.
    pub auto_dark: bool,
    /// NO_COLOR forces mono whatever `[ui] colors` says.
    pub forced_mono: bool,
    pub density: Density,
    pub detail: Detail,
    pub layout: Layout,
    /// Rich link cards may read the page for a preview image the network left out.
    pub unfurl: bool,
    /// The reader takes clicks, the wheel and hover from the terminal.
    pub mouse: bool,
    /// What ⏎ does in the composer.
    pub enter: Enter,
    /// Link images that failed to load; an unfurl does not bring them back.
    pub dropped_images: HashSet<String>,
    pub color_by: ColorBy,
    pub replies: Replies,
    pub icon_mode: IconMode,
    pub icons: bool,
    pub start_ask: bool,
    /// Settings is open; `settings_row` is its cursor.
    pub settings: bool,
    pub settings_page: SettingsPage,
    pub settings_row: usize,
    history: Vec<Place>,
    /// The clock views and the demo world read in place of the wall clock; tests set it.
    pub(crate) frozen_now: Option<DateTime<Utc>>,
    /// X was signed in from the browser session, so For you is the ranked home timeline.
    pub x_web: bool,
    /// The frame under the list draws a loading card while the next page is due or already in flight.
    pub show_page_card: bool,
    /// `m` asked for the next page. The reader fetches it off the input thread.
    pub(crate) page_ask: bool,
    /// The next page is in flight, so the loading card stays and navigation does not wait on it.
    pub paging: bool,
    /// The welcome mark's cursor: a network, then the local demo.
    pub gate_row: usize,
    /**
    The feed and selected post an automatic next page already started for. Cleared when the
    selection leaves the prefetch point, so a cursor that adds nothing cannot spin, and coming back tries again.
    */
    pub(crate) page_gate: Option<(FeedId, Option<String>)>,
}

/// Which Settings screen is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsPage {
    #[default]
    Main,
    Accounts,
}

/// One level of back navigation: everything a view change replaces.
#[derive(Clone)]
struct Place {
    section: Section,
    focus: Option<FeedId>,
    remote: Option<FeedId>,
    remote_query: String,
    thread_focus: Option<String>,
    filter: Option<String>,
    settings: bool,
}

pub enum Outcome {
    None,
    Quit,
    View(Vec<String>),
    Open(String),
    /// A link to copy: the post's, or the open profile's when `profile` is true.
    Share {
        url: String,
        profile: bool,
    },
    Json(Value),
    Help,
}

/// All five networks signed in, so the demo exercises every chip, tab and composer target.
pub fn demo_networks() -> Vec<NetInfo> {
    Network::ALL
        .into_iter()
        .map(|net| NetInfo {
            net,
            signed_in: true,
            caps: net.caps(),
            handle: Some(demo::you(net).username),
        })
        .collect()
}

impl State {
    /**
    `start` is the `--net` or `SOCLI_NET` list. The first list present (`start`, then `[feed]
    networks`) is kept to the signed-in networks; when that leaves none, every signed-in network is on.
    */
    pub fn boot(
        demo: bool,
        count: u8,
        config: &Config,
        clients: &Clients,
        start: Option<&[Network]>,
    ) -> Self {
        let viewer = Viewer::new(crate::media::ImageMode::resolve(None, config));
        let nets = if demo {
            demo_networks()
        } else {
            clients.infos()
        };
        let available: Vec<Network> = nets
            .iter()
            .filter(|info| info.signed_in && (demo || info.net.hold().is_none()))
            .map(|info| info.net)
            .collect();
        let configured = config
            .feed
            .networks
            .as_ref()
            .and_then(|slugs| net::parse_list(&slugs.join(",")).ok());
        let on = start
            .map(<[Network]>::to_vec)
            .or(configured)
            .map(|list| {
                list.into_iter()
                    .filter(|net| available.contains(net))
                    .collect::<Vec<_>>()
            })
            .filter(|list| !list.is_empty())
            .unwrap_or_else(|| available.clone());
        let tabs = nets
            .iter()
            .filter_map(|info| {
                let kind = if demo {
                    info.caps.feeds.first().map(|(kind, _)| *kind)
                } else {
                    info.caps.default_feed(info.net)
                };
                kind.map(|kind| (info.net, kind))
            })
            .collect();
        let mut state = Self {
            reveal_shortcuts: false,
            demo,
            posts: HashMap::new(),
            users: HashMap::new(),
            feeds: HashMap::new(),
            section: Section::Home,
            on: Vec::new(),
            tabs,
            merged_tab: if demo {
                MergedTab::Home
            } else {
                MergedTab::Discover
            },
            focus: None,
            remote: None,
            nets,
            default_network: config
                .default_network
                .as_deref()
                .and_then(|slug| slug.parse().ok()),
            expanded: HashSet::new(),
            opened: HashSet::new(),
            thread_focus: None,
            selected: HashMap::new(),
            overview: false,
            remote_query: String::new(),
            filter: None,
            status: String::new(),
            status_expiry: None,
            status_error: None,
            following: HashSet::new(),
            count: count.clamp(1, 100),
            viewer,
            preview_ratio: None,
            graphics: false,
            pictures: None,
            theme: Theme::default(),
            scheme: Scheme::Auto,
            color_mode: ColorMode::Full,
            auto_dark: true,
            forced_mono: false,
            density: Density::Cozy,
            detail: Detail::Standard,
            layout: Layout::Auto,
            unfurl: true,
            mouse: true,
            enter: Enter::Post,
            dropped_images: HashSet::new(),
            color_by: config
                .feed
                .color_by
                .as_deref()
                .and_then(ColorBy::parse)
                .unwrap_or(ColorBy::Auto),
            replies: config
                .feed
                .replies
                .as_deref()
                .and_then(Replies::parse)
                .unwrap_or(Replies::Oldest),
            icon_mode: IconMode::Auto,
            icons: true,
            start_ask: false,
            settings: false,
            settings_page: SettingsPage::Main,
            settings_row: 0,
            history: Vec::new(),
            frozen_now: None,
            x_web: clients.uses_x_web(),
            show_page_card: false,
            page_ask: false,
            paging: false,
            gate_row: 0,
            page_gate: None,
        };
        state.set_on(on);
        if demo {
            state.install_demo();
            state.status = DEMO_NOTICE.into();
        } else {
            state.status = format!("{} · loading", state.account_label());
        }
        state
    }

    /// The frozen clock when one is set, otherwise the wall clock.
    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.frozen_now.unwrap_or_else(crate::clock::utc_now)
    }

    pub(crate) fn install_demo(&mut self) {
        let world = demo::world(self.now());
        let mut ids = vec![FeedId::Merged, FeedId::Mentions(Scope::All)];
        for kind in [
            FeedKind::Following,
            FeedKind::ForYou,
            FeedKind::Latest,
            FeedKind::Trending,
            FeedKind::Discover,
            FeedKind::Bookmarks,
            FeedKind::Mine,
        ] {
            ids.push(FeedId::Kind(kind));
        }
        for info in &self.nets {
            ids.push(FeedId::Mentions(Scope::One(info.net)));
            for (kind, _) in &info.caps.feeds {
                ids.push(FeedId::Feed(info.net, *kind));
            }
        }
        for id in ids {
            let list = world.feed_ids(&id);
            self.feeds.insert(
                id,
                Feed {
                    ids: list,
                    ready: true,
                    ..Feed::default()
                },
            );
        }
        self.posts = world.posts;
        for post in self.posts.values() {
            remember(&mut self.users, post);
        }
    }

    /// Stores a post and remembers its author.
    fn keep(&mut self, post: Post) {
        remember(&mut self.users, &post);
        self.posts.insert(post.id.clone(), post);
    }

    /// The account a profile screen lists, from its loaded posts or else from the handle alone.
    pub fn profile(&self) -> Option<(Network, User)> {
        if self.section != Section::Search || self.thread_focus.is_some() || self.settings {
            return None;
        }
        let Some(FeedId::User(Scope::One(net), handle)) = &self.remote else {
            return None;
        };
        let handle = handle.trim_start_matches('@');
        let user = self
            .users
            .get(&(*net, handle.to_lowercase()))
            .cloned()
            .unwrap_or_else(|| User {
                id: String::new(),
                name: handle.to_string(),
                username: handle.to_string(),
                avatar_url: None,
                verified: false,
                business: false,
                description: None,
            });
        Some((*net, user))
    }

    // MARK: networks

    fn info(&self, net: Network) -> Option<&NetInfo> {
        self.nets.iter().find(|info| info.net == net)
    }

    pub fn handle_of(&self, net: Network) -> Option<String> {
        self.info(net).and_then(|info| info.handle.clone())
    }

    /// One network on gives its own scope; two or more merge.
    pub fn scope(&self) -> Scope {
        match self.on.as_slice() {
            [net] => Scope::One(*net),
            _ => Scope::All,
        }
    }

    pub fn scope_net(&self) -> Option<Network> {
        match self.scope() {
            Scope::All => None,
            Scope::One(net) => Some(net),
        }
    }

    /// The live pause for `net`. Demo mode never pauses a network.
    pub fn held(&self, net: Network) -> Option<&'static str> {
        if self.demo {
            None
        } else {
            net.hold()
        }
    }

    /// Signed-in networks, in network order; the ones a chip can turn on.
    pub fn available(&self) -> Vec<Network> {
        self.nets
            .iter()
            .filter(|info| info.signed_in && self.held(info.net).is_none())
            .map(|info| info.net)
            .collect()
    }

    /// True when every signed-in network is on.
    pub fn all_on(&self) -> bool {
        self.available().iter().all(|net| self.on.contains(net))
    }

    /// Every supported network for the chip row: whether it is on, and whether it is signed in.
    pub fn chips(&self) -> Vec<(Network, bool, bool)> {
        let available = self.available();
        Network::ALL
            .into_iter()
            .filter(|net| self.held(*net).is_none())
            .map(|net| (net, self.on.contains(&net), available.contains(&net)))
            .collect()
    }

    /**
    The network `<` or `>` moves to: the previous or next signed-in one, wrapping. From a merged
    view, `>` starts at the first network that is on and `<` at the last. None with fewer than two
    signed-in networks, where the keys do nothing.
    */
    pub fn cycle_scope(&self, dir: i32) -> Option<Scope> {
        let available = self.available();
        if available.len() < 2 {
            return None;
        }
        let net = match self.scope_net() {
            Some(current) => {
                let i = available
                    .iter()
                    .position(|net| *net == current)
                    .map_or(if dir > 0 { -1 } else { 0 }, |i| i as i32);
                available[(i + dir).rem_euclid(available.len() as i32) as usize]
            }
            None => {
                let mut on = available.iter().filter(|net| self.on.contains(net));
                let pick = if dir > 0 { on.next() } else { on.next_back() };
                pick.copied().unwrap_or(available[0])
            }
        };
        Some(Scope::One(net))
    }

    /** The scope `a` moves to: All, then each signed-in network in order, then All again. A partial
    on-set goes to All. None with fewer than two signed-in networks, where the key does nothing. */
    pub fn next_scope(&self) -> Option<Scope> {
        let available = self.available();
        if available.len() < 2 {
            return None;
        }
        match self.scope() {
            Scope::One(net) => match available.iter().position(|n| *n == net) {
                Some(i) if i + 1 < available.len() => Some(Scope::One(available[i + 1])),
                _ => Some(Scope::All),
            },
            Scope::All if self.all_on() => Some(Scope::One(available[0])),
            Scope::All => Some(Scope::All),
        }
    }

    /// Replaces the on-set. Merged feeds a live client fetched for the old set load again.
    pub fn set_on(&mut self, mut on: Vec<Network>) {
        if !self.demo {
            on.retain(|net| net.hold().is_none());
        }
        on.sort();
        on.dedup();
        if on != self.on && !self.demo {
            self.feeds.retain(|id, _| !id.merged());
        }
        self.on = on;
    }

    /// The feed tabs of the scoped network; empty in All scope.
    pub fn scope_tabs(&self) -> Vec<(FeedKind, &'static str)> {
        self.scope_net()
            .and_then(|net| self.info(net))
            .map(|info| {
                let discover = info.caps.resolve_feed(info.net, FeedKind::Discover);
                info.caps
                    .feeds
                    .iter()
                    .map(|(kind, label)| {
                        (
                            *kind,
                            if !self.demo && Some(*kind) == discover {
                                "Discover"
                            } else {
                                *label
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn tab_of(&self, net: Network) -> Option<FeedKind> {
        self.tabs.get(&net).copied()
    }

    /// The All view's tabs that at least one signed-in network that is on supplies.
    pub fn merged_tabs(&self) -> Vec<MergedTab> {
        let nets: Vec<&NetInfo> = self
            .nets
            .iter()
            .filter(|info| info.signed_in && self.on.contains(&info.net))
            .collect();
        MergedTab::ALL
            .into_iter()
            .filter(|tab| {
                nets.iter().any(|info| match tab.kind() {
                    None => info.caps.primary.is_some(),
                    Some(kind) => info.caps.resolve_feed(info.net, kind).is_some(),
                })
            })
            .collect()
    }

    /// The All view's tab on screen: the stored one, or Home when no network that is on supplies it.
    pub fn shown_merged_tab(&self) -> MergedTab {
        if self.merged_tabs().contains(&self.merged_tab) {
            self.merged_tab
        } else {
            MergedTab::Home
        }
    }

    /// Opens an All view tab and loads it; the `SoCLI>` prompt's `home`, `discover` and `saved`.
    pub fn open_merged_tab(&mut self, tab: MergedTab, clients: &mut Clients) -> Result<(), String> {
        if !self.merged_tabs().contains(&tab) {
            return Err(match tab.kind() {
                Some(kind) => missing_kind(kind),
                None => NO_ACCOUNTS.into(),
            });
        }
        self.merged_tab = tab;
        self.section = Section::Home;
        self.focus = None;
        self.thread_focus = None;
        self.history.clear();
        self.note_selection();
        self.ensure_loaded(clients)
    }

    /// The next or previous All view tab, wrapping.
    pub fn cycle_merged_tab(&self, dir: i32) -> Option<MergedTab> {
        let tabs = self.merged_tabs();
        let i = tabs
            .iter()
            .position(|tab| *tab == self.shown_merged_tab())
            .unwrap_or(0) as i32;
        tabs.get((i + dir).rem_euclid(tabs.len().max(1) as i32) as usize)
            .copied()
    }

    /// All view tab `index` (0-based) among the ones shown.
    pub fn merged_tab_at(&self, index: usize) -> Option<MergedTab> {
        self.merged_tabs().get(index).copied()
    }

    /// The next or previous feed tab of the scoped network.
    pub fn cycle_tab(&mut self, dir: i32) -> Option<Cmd> {
        let tabs = self.scope_tabs();
        let net = self.scope_net().filter(|_| !tabs.is_empty())?;
        let i = self
            .tab_of(net)
            .and_then(|kind| tabs.iter().position(|(k, _)| *k == kind))
            .unwrap_or(0) as i32;
        let (kind, _) = tabs[(i + dir).rem_euclid(tabs.len() as i32) as usize];
        Some(Cmd::Tab(kind))
    }

    /// Tab `index` (0-based) of the scoped network.
    pub fn tab_at(&mut self, index: usize) -> Option<Cmd> {
        self.scope_net()?;
        self.scope_tabs()
            .get(index)
            .map(|(kind, _)| Cmd::Tab(*kind))
    }

    /// Signed-in networks that can publish, in network order.
    pub fn post_targets(&self) -> Vec<Network> {
        self.nets
            .iter()
            .filter(|info| info.signed_in && info.caps.post && self.held(info.net).is_none())
            .map(|info| info.net)
            .collect()
    }

    /// The scoped network, else `default_network`, else the first network that can post.
    pub fn initial_target(&self) -> Option<Network> {
        let targets = self.post_targets();
        self.scope_net()
            .filter(|net| targets.contains(net))
            .or_else(|| self.default_network.filter(|net| targets.contains(net)))
            .or_else(|| targets.first().copied())
    }

    /// The network a bare handle belongs to: the scoped one, the only signed-in one, then the default.
    pub fn handle_network(&self) -> Result<Network, String> {
        if let Some(net) = self.scope_net() {
            return Ok(net);
        }
        let signed: Vec<Network> = self
            .nets
            .iter()
            .filter(|info| info.signed_in && self.held(info.net).is_none())
            .map(|info| info.net)
            .collect();
        if let [only] = signed.as_slice() {
            return Ok(*only);
        }
        self.default_network
            .ok_or_else(|| "which network? add --net x|bluesky|devnull|threads|mastodon".into())
    }

    /// Unsupported when the network itself lacks the action, sign-in required when only this client does.
    fn check(&self, net: Network, what: &str, has: impl Fn(&Caps) -> bool) -> Result<(), String> {
        if let Some(message) = self.held(net) {
            return Err(message.into());
        }
        if !has(&net.caps()) {
            return Err(NetError::unsupported(net, what).into());
        }
        if self.info(net).is_some_and(|info| has(&info.caps)) {
            Ok(())
        } else {
            Err(NetError::SignInRequired(net).into())
        }
    }

    /// Opens feed tab `kind` of the scoped network; names like `latest` resolve per network.
    pub fn pick_tab(&mut self, kind: FeedKind) -> Result<(), String> {
        let Some(net) = self.scope_net() else {
            self.fail(PICK_NETWORK_FIRST);
            return Ok(());
        };
        let caps = self
            .info(net)
            .map(|info| info.caps.clone())
            .unwrap_or_else(|| Caps::public(0));
        let Some(tab) = caps.resolve_feed(net, kind) else {
            return Err(if net == Network::X && kind == FeedKind::Trending {
                net::caps::NO_TRENDING.into()
            } else if net.caps().resolve_feed(net, kind).is_some() {
                NetError::SignInRequired(net).into()
            } else {
                NetError::unsupported(net, format!("{} feed", kind.word())).into()
            });
        };
        self.section = Section::Home;
        self.tabs.insert(net, tab);
        self.focus = None;
        self.thread_focus = None;
        Ok(())
    }

    // MARK: status

    /// Shows `text` as a confirmation, which [`State::expire`] clears 4 seconds from now.
    pub fn confirm(&mut self, text: impl Into<String>) {
        self.confirm_at(text, Instant::now());
    }

    /// [`State::confirm`] as of `now`.
    pub fn confirm_at(&mut self, text: impl Into<String>, now: Instant) {
        self.status = text.into();
        self.status_error = None;
        self.status_expiry = Some((now + CONFIRM_FOR, self.status.clone()));
    }

    /// Shows `text` as an error, which stays until [`State::clear_error`].
    pub fn fail(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_expiry = None;
        self.status_error = Some(self.status.clone());
    }

    /// A confirmation or an error, which replaces the footer hints until it clears.
    pub fn temporary_status(&self) -> bool {
        !self.status.is_empty() && (self.status_expiry.is_some() || self.status_error.is_some())
    }

    /// Clears a confirmation that is due by `now` while the status still shows it; true when it did.
    pub fn expire(&mut self, now: Instant) -> bool {
        match self.status_expiry.take_if(|(deadline, _)| *deadline <= now) {
            Some((_, text)) if self.status == text => {
                self.status.clear();
                true
            }
            _ => false,
        }
    }

    /// Clears the error the status still shows; a key, a click or a paste calls it before it runs.
    pub fn clear_error(&mut self) {
        if self
            .status_error
            .take()
            .is_some_and(|text| text == self.status)
        {
            self.status.clear();
        }
    }

    // MARK: back navigation

    fn place(&self) -> Place {
        Place {
            section: self.section,
            focus: self.focus.clone(),
            remote: self.remote.clone(),
            remote_query: self.remote_query.clone(),
            thread_focus: self.thread_focus.clone(),
            filter: self.filter.clone(),
            settings: self.settings,
        }
    }

    /// Remembers the current view so back (`⌫`, `←`, `esc`) returns to it.
    pub fn push_place(&mut self) {
        if self.history.len() == HISTORY {
            self.history.remove(0);
        }
        let place = self.place();
        self.history.push(place);
    }

    fn restore(&mut self, place: Place) {
        self.section = place.section;
        self.focus = place.focus;
        self.remote = place.remote;
        self.remote_query = place.remote_query;
        self.thread_focus = place.thread_focus;
        self.filter = place.filter;
        self.settings = place.settings;
    }

    /// Opens Settings on top of the view stack. A second press leaves the open page where it is.
    pub fn open_settings(&mut self) {
        if !self.settings {
            self.push_place();
            self.settings = true;
            self.settings_page = SettingsPage::Main;
            self.settings_row = 0;
        }
    }

    /// Switches this reader to the local demo. Nothing is written and nothing is fetched.
    pub fn enter_demo(&mut self) {
        self.demo = true;
        self.nets = demo_networks();
        self.posts.clear();
        self.users.clear();
        self.feeds.clear();
        self.install_demo();
        let on = self.available();
        self.set_on(on);
        self.section = Section::Home;
        self.focus = None;
        self.thread_focus = None;
        self.remote = None;
        self.settings = false;
        self.gate_row = 0;
        self.show_page_card = false;
        self.page_ask = false;
        self.paging = false;
        self.page_gate = None;
        self.status = DEMO_NOTICE.into();
        self.status_expiry = None;
        self.status_error = None;
    }

    /// Opens the accounts page. From the reader it remembers the view; from Settings it stays on the stack.
    pub fn open_accounts(&mut self) {
        if !self.settings {
            self.push_place();
        }
        self.settings = true;
        self.settings_page = SettingsPage::Accounts;
        self.settings_row = 0;
    }

    /**
    Reloads the signed-in accounts from `clients`. `also_on` joins the feed when that sign-in just
    succeeded. Demo mode keeps the demo accounts.
    */
    pub fn adopt_accounts(&mut self, clients: &Clients, also_on: Option<Network>) {
        if !self.demo {
            self.nets = clients.infos();
            self.x_web = clients.uses_x_web();
        }
        let available = self.available();
        let mut on: Vec<Network> = self
            .on
            .iter()
            .copied()
            .filter(|net| available.contains(net))
            .collect();
        if on.is_empty() {
            on.clone_from(&available);
        }
        if let Some(net) = also_on {
            if available.contains(&net) && !on.contains(&net) {
                on.push(net);
            }
            if let Some(kind) = self.info(net).and_then(|info| info.caps.default_feed(net)) {
                self.tabs.insert(net, kind);
            }
            self.merged_tab = MergedTab::Discover;
            self.focus = None;
            self.thread_focus = None;
        }
        self.set_on(on);
    }

    /// Applies the scheme and color mode to the palette in use.
    pub fn retheme(&mut self) {
        let (palette, dark) = self.scheme.resolve(self.auto_dark);
        self.theme.set_scheme(palette, dark);
        let ascii = self.detail == Detail::Plain;
        self.theme.ascii = ascii;
        self.theme.mono = self.forced_mono || self.color_mode == ColorMode::Mono || ascii;
        self.theme.plain = self.forced_mono || ascii;
    }

    /// Whether avatars and rails take the network's color: `auto` does when the view mixes networks.
    fn net_colors(&self) -> bool {
        match self.color_by {
            ColorBy::Network => true,
            ColorBy::Author => false,
            ColorBy::Auto => self.scope() == Scope::All && self.thread_focus.is_none(),
        }
    }

    /// Thread replies newest first; feeds always list replies oldest first.
    fn newest(&self) -> bool {
        self.thread_focus.is_some() && self.replies == Replies::Newest
    }

    /// The rows the current screen lays out, in the order it draws them.
    pub fn lay(&self, posts: &[Post]) -> Vec<layout::LaidRow> {
        let rows = layout::lay_for(posts, &self.expanded, self.newest(), self.overview_on());
        if self.profile().is_some() {
            layout::profiled(rows)
        } else {
            rows
        }
    }

    // MARK: what is on screen

    pub fn selected_post_id(&self) -> Option<String> {
        self.resolve(None).ok()
    }

    /// The post under the selection, when the selected row is one.
    pub fn selected_post(&self) -> Option<&Post> {
        self.posts.get(&self.selected_post_id()?)
    }

    /// Make one post id the current selection so a one-shot write can target it.
    pub fn pin(&mut self, id: &str) {
        let net = net::split(id).map_or(Network::X, |(net, _)| net);
        self.posts.entry(id.to_string()).or_insert_with(|| Post {
            id: id.to_string(),
            net,
            url: None,
            cid: None,
            author: User {
                id: String::new(),
                name: "post".into(),
                username: "post".into(),
                avatar_url: None,
                verified: false,
                business: false,
                description: None,
            },
            text: String::new(),
            created_at: None,
            conversation_id: id.to_string(),
            in_reply_to: None,
            quoted: None,
            reposted_by: None,
            metrics: Metrics::default(),
            links: Vec::new(),
            media: Vec::new(),
            liked: false,
            bookmarked: false,
            reposted: false,
        });
        let pinned = FeedId::Thread(id.to_string());
        self.feeds.insert(
            pinned.clone(),
            Feed {
                ids: vec![id.to_string()],
                ready: true,
                ..Feed::default()
            },
        );
        self.section = Section::Home;
        self.focus = Some(pinned);
        self.thread_focus = None;
        self.filter = None;
        let key = self.selection_key();
        self.selected.insert(key, 0);
    }

    /// The scoped network's `@handle`, `not signed in`, or `Accounts` for the merged view.
    pub fn account_label(&self) -> String {
        match self.scope() {
            Scope::One(net) => self
                .handle_of(net)
                .map_or_else(|| "not signed in".into(), |handle| format!("@{handle}")),
            Scope::All => "Accounts".into(),
        }
    }

    /// The Home tab name in the crumb: the All view's tab, else the open feed tab.
    fn tab_name(&self) -> String {
        if let Some(FeedId::Kind(kind)) = &self.focus {
            return kind.word().into();
        }
        if self.scope_net().is_none() {
            return self.shown_merged_tab().word().into();
        }
        self.scope_net()
            .and_then(|net| {
                let kind = self.tab_of(net)?;
                self.scope_tabs()
                    .into_iter()
                    .find(|(tab, _)| *tab == kind)
                    .map(|(_, label)| label)
            })
            .map_or_else(|| "home".into(), str::to_lowercase)
    }

    pub fn crumb(&self) -> String {
        if self.settings {
            return if self.settings_page == SettingsPage::Accounts {
                "settings › accounts".into()
            } else {
                "settings".into()
            };
        }
        if let (Section::Search, None, Some(FeedId::User(scope, handle))) =
            (self.section, &self.thread_focus, &self.remote)
        {
            return format!("{} › @{}", scope.label(), handle.trim_start_matches('@'));
        }
        let scope = self.scope().label();
        let tab = match self.section {
            Section::Home => self.tab_name(),
            Section::Notifications => "mentions".into(),
            Section::Search if self.remote_query.is_empty() || self.thread_focus.is_some() => {
                "search".into()
            }
            Section::Search => format!("search › {}", self.remote_query),
        };
        if self.thread_focus.is_some() {
            format!("{scope} › {tab} › thread")
        } else {
            format!("{scope} › {tab}")
        }
    }

    /// Mentions loaded so far from the networks that are on; zero until that feed has been fetched.
    pub fn mention_count(&self) -> usize {
        let id = FeedId::Mentions(self.scope());
        self.feeds
            .get(&id)
            .filter(|feed| feed.ready)
            .map_or(0, |feed| {
                feed.ids
                    .iter()
                    .filter(|post| {
                        !id.merged()
                            || self
                                .posts
                                .get(*post)
                                .is_some_and(|p| self.on.contains(&p.net))
                    })
                    .count()
            })
    }

    /// The account the ask box shows.
    pub fn you(&self) -> User {
        let net = self
            .scope_net()
            .or_else(|| self.nets.iter().find(|info| info.signed_in).map(|i| i.net));
        let username = net
            .and_then(|net| self.handle_of(net))
            .unwrap_or_else(|| "you".into());
        User {
            id: String::new(),
            name: "You".into(),
            username,
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        }
    }

    /// A loaded conversation for `thread_focus`, when one is on screen.
    ///
    /// An empty or failed thread does not count, so the pane falls back to the list.
    fn thread_on_screen(&self) -> Option<FeedId> {
        let conversation = self.thread_focus.as_deref()?;
        if let Some(id @ FeedId::Thread(target)) = &self.focus {
            let ready = self
                .feeds
                .get(id)
                .is_some_and(|feed| feed.ready && !feed.ids.is_empty());
            let same = target == conversation
                || self
                    .posts
                    .get(target)
                    .is_some_and(|post| post.conversation_id == conversation);
            if ready && same {
                return Some(id.clone());
            }
        }
        self.thread_feed_for(conversation)
    }

    /// The fullest ready thread whose posts belong to `conversation`.
    fn thread_feed_for(&self, conversation: &str) -> Option<FeedId> {
        let mut best: Option<(&FeedId, usize, &str)> = None;
        for (id, feed) in &self.feeds {
            let FeedId::Thread(target) = id else {
                continue;
            };
            if !feed.ready || feed.ids.is_empty() {
                continue;
            }
            let same = target == conversation
                || self
                    .posts
                    .get(target)
                    .is_some_and(|post| post.conversation_id == conversation)
                || feed.ids.iter().any(|post_id| {
                    post_id == conversation
                        || self
                            .posts
                            .get(post_id)
                            .is_some_and(|post| post.conversation_id == conversation)
                });
            if !same {
                continue;
            }
            let better = best.is_none_or(|(_, len, prev)| {
                feed.ids.len() > len || (feed.ids.len() == len && target.as_str() < prev)
            });
            if better {
                best = Some((id, feed.ids.len(), target));
            }
        }
        best.map(|(id, _, _)| id.clone())
    }

    pub fn current_feed_id(&self) -> Option<FeedId> {
        // The right pane, and a narrow thread that still has the keys, read the loaded
        // conversation. The feed pane draws with `thread_focus` cleared, so it keeps the list.
        if let Some(id) = self.thread_on_screen() {
            return Some(id);
        }
        match self.section {
            Section::Notifications => Some(FeedId::Mentions(self.scope())),
            Section::Search => self.remote.clone(),
            Section::Home => {
                if let Some(focus) = &self.focus {
                    return Some(focus.clone());
                }
                match self.scope() {
                    Scope::All => Some(self.shown_merged_tab().feed_id()),
                    Scope::One(net) => {
                        let kind = self.tab_of(net)?;
                        Some(FeedId::Feed(net, kind))
                    }
                }
            }
        }
    }

    pub fn selection_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.crumb(),
            self.filter.as_deref().unwrap_or(""),
            self.thread_focus.as_deref().unwrap_or("")
        )
    }

    fn links_mut(&mut self) -> impl Iterator<Item = &mut Link> {
        self.posts
            .values_mut()
            .flat_map(|post| post.links.iter_mut())
    }

    /// Fills what an unfurl of `page` found into its loaded links, keeping what the network sent.
    pub fn fill_link(&mut self, page: &str, card: &Card) {
        let image = card
            .image
            .clone()
            .filter(|image| !self.dropped_images.contains(image));
        for link in self.links_mut().filter(|link| link.expanded == page) {
            if link.image.is_none() && image.is_some() {
                link.image.clone_from(&image);
                link.image_width = card.width;
                link.image_height = card.height;
            }
            if link.title.is_none() {
                link.title.clone_from(&card.title);
            }
            if link.description.is_none() {
                link.description.clone_from(&card.description);
            }
            if link.site.is_none() {
                link.site.clone_from(&card.site);
            }
        }
    }

    /// Records a link image's decoded size, so its card takes the real shape.
    pub fn size_link_image(&mut self, image: &str, width: u32, height: u32) {
        for link in self
            .links_mut()
            .filter(|link| link.image.as_deref() == Some(image))
        {
            link.image_width = Some(width);
            link.image_height = Some(height);
        }
    }

    /// Forgets a link image that failed to load, so its card falls back to the plain one.
    pub fn drop_link_image(&mut self, image: &str) {
        self.dropped_images.insert(image.to_string());
        for link in self
            .links_mut()
            .filter(|link| link.image.as_deref() == Some(image))
        {
            link.image = None;
            link.image_width = None;
            link.image_height = None;
        }
    }

    pub fn current_posts(&self) -> Vec<Post> {
        let Some(id) = self.current_feed_id() else {
            return Vec::new();
        };
        self.posts_for(&id)
    }

    fn posts_for(&self, id: &FeedId) -> Vec<Post> {
        let Some(feed) = self.feeds.get(id) else {
            return Vec::new();
        };
        // Walk `feed.ids` in order. A HashSet's iteration order changes every call, and a redraw
        // then swaps posts that share a timestamp.
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        for id in &feed.ids {
            if seen.insert(id.clone()) {
                order.push(id.clone());
            }
        }
        let mut i = 0;
        while i < order.len() {
            if let Some(parent) = self
                .posts
                .get(&order[i])
                .and_then(|post| post.in_reply_to.clone())
            {
                if self.posts.contains_key(&parent) && seen.insert(parent.clone()) {
                    order.push(parent);
                }
            }
            i += 1;
        }
        let mut posts: Vec<Post> = order
            .iter()
            .filter_map(|id| self.posts.get(id).cloned())
            .collect();
        if id.merged() {
            posts.retain(|p| self.on.contains(&p.net));
        }
        if let Some(focus) = &self.thread_focus {
            posts.retain(|p| &p.conversation_id == focus || &p.id == focus);
        }
        if matches!(id, FeedId::Thread(_)) || self.thread_focus.is_some() {
            // Reposts are feed events; their canonical post appears only once in a conversation.
            posts.sort_by(|a, b| {
                (a.reposted_by.is_some(), &a.id).cmp(&(b.reposted_by.is_some(), &b.id))
            });
            let mut canonical: HashMap<(Network, String), String> = HashMap::new();
            let mut aliases = HashMap::new();
            posts.retain(|post| {
                let Some(url) = post.url.as_ref().filter(|url| !url.is_empty()) else {
                    return true;
                };
                let identity = (post.net, url.clone());
                match canonical.get(&identity) {
                    Some(original) => {
                        aliases.insert(post.id.clone(), original.clone());
                        false
                    }
                    None => {
                        canonical.insert(identity, post.id.clone());
                        true
                    }
                }
            });
            for post in &mut posts {
                if let Some(original) = post
                    .in_reply_to
                    .as_ref()
                    .and_then(|parent| aliases.get(parent))
                {
                    post.in_reply_to = Some(original.clone());
                }
            }
        }

        if let Some(filter) = &self.filter {
            let needle = filter.to_lowercase();
            let mut keep: HashSet<String> = posts
                .iter()
                .filter(|p| {
                    p.text.to_lowercase().contains(&needle)
                        || p.author.username.to_lowercase().contains(&needle)
                        || p.author.name.to_lowercase().contains(&needle)
                })
                .map(|p| p.id.clone())
                .collect();
            loop {
                let extra: Vec<String> = keep
                    .iter()
                    .filter_map(|id| {
                        posts
                            .iter()
                            .find(|p| &p.id == id)
                            .and_then(|p| p.in_reply_to.clone())
                    })
                    .filter(|parent| !keep.contains(parent))
                    .collect();
                if extra.is_empty() {
                    break;
                }
                keep.extend(extra);
            }
            posts.retain(|p| keep.contains(&p.id));
        }
        posts
    }

    /// The body for the current screen; `selection` is off for `--print`.
    pub fn view(&self, width: usize, selection: bool) -> View {
        self.view_sized(width, selection, None)
    }

    pub fn view_sized(&self, width: usize, selection: bool, height: Option<usize>) -> View {
        let posts = self.current_posts();
        let opts = ViewOpts {
            width,
            height,
            now: self.now(),
            selected: if selection {
                self.selected_row(&posts)
            } else {
                None
            },
            icons: self.icons,
            thread: self.thread_focus.clone(),
            tags: self.scope() == Scope::All && self.thread_focus.is_none(),
            density: self.density,
            net_colors: self.net_colors(),
            newest: self.newest(),
            profile: self.profile(),
            previews: self.preview_ratio.filter(|_| self.detail == Detail::Rich),
            page: self.pictures == Some("this page"),
            images: self.viewer.mode != crate::media::ImageMode::Off,
            level: self.detail,
            shortcuts: self.reveal_shortcuts,
            avatars: self.detail == Detail::Rich && self.graphics,
            overview: self.overview_on(),
            opened: self.opened.clone(),
        };
        if self.section == Section::Notifications && self.thread_focus.is_none() {
            render::notifications(&posts, &self.expanded, &opts)
        } else {
            render::render(&posts, &self.expanded, &opts)
        }
    }

    fn selected_row(&self, posts: &[Post]) -> Option<usize> {
        let rows = self.lay(posts);
        let selectable = layout::selectable(&rows);
        let key = self.selection_key();
        let wanted = self.selected.get(&key).copied();
        wanted
            .filter(|row| selectable.contains(row))
            .or_else(|| selectable.first().copied())
    }

    pub fn notice(&self) -> Option<String> {
        self.current_feed_id()
            .and_then(|id| self.feeds.get(&id))
            .and_then(|f| f.notice.clone())
    }

    pub fn feed_json(&self) -> Value {
        let posts = self.current_posts();
        let rows = self.lay(&posts);
        let mut out = Vec::new();
        for row in rows {
            if let Some(id) = layout::post_id(&row) {
                if let Some(post) = self.posts.get(id) {
                    if !out.iter().any(|p: &Post| p.id == post.id) {
                        out.push(post.clone());
                    }
                }
            }
        }
        Value::Array(out.into_iter().map(|p| p.to_json()).collect())
    }

    pub fn ensure_loaded(&mut self, clients: &mut Clients) -> Result<(), String> {
        if self.demo {
            if let Some(id) = self.current_feed_id() {
                if !self.feeds.contains_key(&id) {
                    let feed = self.demo_feed(&id);
                    self.feeds.insert(id, feed);
                    self.note_selection();
                }
            }
            return Ok(());
        }
        let Some(id) = self.current_feed_id() else {
            return Ok(());
        };
        let ready = self.feeds.get(&id).is_some_and(|f| f.ready);
        if ready {
            return Ok(());
        }
        self.load_feed(id, clients, false)
    }

    /// Puts `on` in the feed, as `<`, `>`, `0` and the filter panel do.
    pub fn switch_on(&mut self, on: Vec<Network>, clients: &mut Clients) -> Result<(), String> {
        self.set_on(on);
        if self.section == Section::Home {
            self.history.clear();
        }
        let scope = self.scope();
        self.focus = None;
        self.thread_focus = None;
        if let Some(FeedId::Search(_, query)) = &self.remote {
            self.remote = Some(FeedId::Search(scope, query.clone()));
        }
        self.note_selection();
        self.ensure_loaded(clients)
    }

    // MARK: commands

    pub fn apply(&mut self, cmd: Cmd, clients: &mut Clients) -> Result<Outcome, String> {
        match cmd {
            Cmd::Help => Ok(Outcome::Help),
            Cmd::Quit => Ok(Outcome::Quit),
            Cmd::Refresh => {
                if let Some(id) = self.current_feed_id() {
                    self.load_feed(id, clients, false)?;
                }
                Ok(Outcome::None)
            }
            Cmd::More => {
                if let Some(id) = self.current_feed_id() {
                    let has_next = self.feeds.get(&id).is_some_and(|f| !f.next.is_empty());
                    if has_next {
                        let kept = self.selected_post_id();
                        self.load_feed(id, clients, true)?;
                        if let Some(post) = kept {
                            self.select_post(&post, false);
                        }
                    } else {
                        self.fail("no further page");
                    }
                }
                Ok(Outcome::None)
            }
            Cmd::Section(section) => {
                if section == Section::Home {
                    self.history.clear();
                } else if section != self.section || self.thread_focus.is_some() {
                    self.push_place();
                }
                self.section = section;
                self.thread_focus = None;
                self.note_selection();
                self.ensure_loaded(clients)?;
                Ok(Outcome::None)
            }
            Cmd::Tab(kind) => {
                if self.scope_net().is_none() {
                    match MergedTab::of_kind(kind) {
                        Some(tab) => self.open_merged_tab(tab, clients)?,
                        None => self.fail(PICK_NETWORK_FIRST),
                    }
                    return Ok(Outcome::None);
                }
                self.pick_tab(kind)?;
                self.history.clear();
                self.note_selection();
                self.ensure_loaded(clients)?;
                Ok(Outcome::None)
            }
            Cmd::Scope(scope) => {
                let available = self.available();
                let on = match scope {
                    Scope::All => available,
                    Scope::One(net) => {
                        if let Some(message) = self.held(net) {
                            self.fail(message);
                            return Ok(Outcome::None);
                        }
                        if !available.contains(&net) {
                            self.fail(NetError::SignInRequired(net).to_string());
                            return Ok(Outcome::None);
                        }
                        vec![net]
                    }
                };
                self.switch_on(on, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Search(query) => {
                let fresh = self.section == Section::Search
                    && self.thread_focus.is_none()
                    && self.remote_query.is_empty();
                if !fresh {
                    self.push_place();
                }
                self.section = Section::Search;
                self.remote_query = query.clone();
                self.thread_focus = None;
                let id = FeedId::Search(self.scope(), query.clone());
                self.remote = Some(id.clone());
                if self.demo {
                    let feed = self.demo_feed(&id);
                    let n = feed.ids.len();
                    self.feeds.insert(id, feed);
                    self.confirm(format!("demo · {n} posts match {query}"));
                } else {
                    self.load_feed(id, clients, false)?;
                }
                Ok(Outcome::None)
            }
            Cmd::User(handle) => {
                let scope = match self.handle_network() {
                    Ok(net) => Scope::One(net),
                    Err(_) if self.demo => Scope::All,
                    Err(err) => return Err(err),
                };
                self.open_user(scope, handle, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Thread(target) => {
                let id = net::parse_target(&target, self.scope_net().or(self.default_network))?;
                self.open_thread(&id, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Open { n } => {
                if let Some(url) = self.selected_profile_url().filter(|_| n.is_none()) {
                    return Ok(Outcome::Open(url));
                }
                let id = self.resolve(n)?;
                let url = self
                    .posts
                    .get(&id)
                    .map(|p| {
                        p.links
                            .first()
                            .map(|l| l.expanded.clone())
                            .unwrap_or_else(|| p.permalink())
                    })
                    .unwrap_or_default();
                if url.is_empty() {
                    self.fail("no link on this post");
                    Ok(Outcome::None)
                } else {
                    Ok(Outcome::Open(url))
                }
            }
            Cmd::Share { n } => {
                if let Some(url) = self.selected_profile_url().filter(|_| n.is_none()) {
                    return Ok(Outcome::Share { url, profile: true });
                }
                let id = self.resolve(n)?;
                let url = self
                    .posts
                    .get(&id)
                    .map(Post::permalink)
                    .filter(|url| !url.is_empty())
                    .ok_or_else(|| format!("{id} has no link to copy"))?;
                Ok(Outcome::Share {
                    url,
                    profile: false,
                })
            }
            Cmd::View { n } => {
                let id = self.resolve(n)?;
                let urls = self
                    .posts
                    .get(&id)
                    .map(|p| {
                        p.media
                            .iter()
                            .filter_map(|m| m.view_url().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if urls.is_empty() {
                    self.fail("no image on this post · V views the avatar");
                    Ok(Outcome::None)
                } else {
                    Ok(Outcome::View(urls))
                }
            }
            Cmd::Avatar { n } => {
                let id = self.resolve(n)?;
                match self
                    .posts
                    .get(&id)
                    .and_then(|p| p.author.avatar_url.clone())
                {
                    Some(url) => Ok(Outcome::View(vec![url])),
                    None => {
                        self.fail("no avatar url on this post");
                        Ok(Outcome::None)
                    }
                }
            }
            Cmd::Json { n } => {
                let id = self.resolve(n)?;
                let value = self
                    .posts
                    .get(&id)
                    .map(|p| p.to_json())
                    .unwrap_or(Value::Null);
                Ok(Outcome::Json(value))
            }
            Cmd::Post(text) => {
                let net = self.initial_target().ok_or_else(no_poster)?;
                self.write_post(&text, None, None, net, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Reply { n, text } => {
                let id = self.resolve(n)?;
                let net = net_of(&id)?;
                self.write_post(&text, Some(id), None, net, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Quote { n, text } => {
                let id = self.resolve(n)?;
                let net = net_of(&id)?;
                self.write_post(&text, None, Some(id), net, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Repost { n } => {
                self.toggle_repost(n, true, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Unrepost { n } => {
                self.toggle_repost(n, false, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Bookmark { n } => {
                self.toggle_flag(n, Flag::Bookmark, true, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Unbookmark { n } => {
                self.toggle_flag(n, Flag::Bookmark, false, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Like { n } => {
                self.toggle_flag(n, Flag::Like, true, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Unlike { n } => {
                self.toggle_flag(n, Flag::Like, false, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Follow(handle) => {
                self.follow_user(&handle, true, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Unfollow(handle) => {
                self.follow_user(&handle, false, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Delete { n } => {
                self.delete_post(n, clients)?;
                Ok(Outcome::None)
            }
            Cmd::Expand => self.expand(clients),
            Cmd::Back => {
                if self.settings && self.history.is_empty() {
                    self.settings = false;
                } else if self.settings {
                    if let Some(place) = self.history.pop() {
                        self.restore(place);
                        self.note_selection();
                        self.ensure_loaded(clients)?;
                    }
                } else if self.thread_focus.is_some()
                    && !matches!(self.focus, Some(FeedId::Thread(_)))
                {
                    // Split: the right pane had the keys. Hand them back. The feed stays put.
                    self.thread_focus = None;
                    self.note_selection();
                } else if let Some(place) = self.history.pop() {
                    self.restore(place);
                    self.note_selection();
                    self.ensure_loaded(clients)?;
                } else if self.thread_focus.take().is_some() {
                    self.note_selection();
                } else if self.filter.take().is_some() {
                    self.confirm("filter cleared");
                }
                Ok(Outcome::None)
            }
            Cmd::SelectDelta(delta) => {
                self.step_selection(delta);
                Ok(Outcome::None)
            }
            Cmd::SelectFirst => {
                self.selection_edge(false);
                Ok(Outcome::None)
            }
            Cmd::SelectLast => {
                self.selection_edge(true);
                Ok(Outcome::None)
            }
            Cmd::Theme(Some(scheme)) => {
                self.scheme = scheme;
                self.retheme();
                self.confirm(format!("theme {}", scheme.word()));
                Ok(Outcome::None)
            }
            Cmd::Theme(None) => Err("name a theme: socli config themes lists them".into()),
            Cmd::Filter(filter) => {
                self.filter = filter;
                self.selected.remove(&self.selection_key());
                self.note_selection();
                Ok(Outcome::None)
            }
        }
    }

    /**
    Opens the selected post so its replies load. A fold that only uncovers replies already in
    memory stays a toggle, and a thread that is already the open feed is left where it is.
    */
    fn expand(&mut self, clients: &mut Clients) -> Result<Outcome, String> {
        let posts = self.current_posts();
        let rows = self.lay(&posts);
        let Some(target) = self
            .selected_row(&posts)
            .and_then(|index| rows.get(index))
            .map(|row| row.target.clone())
        else {
            return Ok(Outcome::None);
        };
        match target {
            Target::Fold { parent, hidden } if hidden > 0 => {
                if !self.expanded.insert(parent.clone()) {
                    self.expanded.remove(&parent);
                }
                self.note_selection();
                Ok(Outcome::None)
            }
            Target::Fold { parent, .. } => self.open_unless_current(&parent, clients),
            Target::Post(id) => self.open_unless_current(&id, clients),
            Target::Gap | Target::Profile => Ok(Outcome::None),
        }
    }

    /**
    In the split, `⏎` and `→` hand the keys to the right pane and leave the feed where it is.
    A narrow reader still fetches the thread and opens it on the one pane.
    */
    fn open_unless_current(&mut self, id: &str, clients: &mut Clients) -> Result<Outcome, String> {
        if self.focus.as_ref() == Some(&FeedId::Thread(id.to_string())) {
            return Ok(Outcome::None);
        }
        if self.overview {
            self.hand_to_thread(id);
            return Ok(Outcome::None);
        }
        self.open_thread(id, clients)?;
        Ok(Outcome::None)
    }

    /// The right pane takes the keys. The feed underneath is not replaced.
    fn hand_to_thread(&mut self, id: &str) {
        self.ensure_demo_thread(id);
        let focus = self
            .posts
            .get(id)
            .map(|post| post.conversation_id.clone())
            .unwrap_or_else(|| id.to_string());
        self.thread_focus = Some(focus);
        self.note_selection();
    }

    /**
    Moves the selection a page: to the first selectable row whose top is at least `rows` body rows
    below the selected row's top (above for a negative count), else to the last (first) one.
    */
    pub fn page(&mut self, width: usize, rows: i64) {
        let posts = self.current_posts();
        let laid = self.lay(&posts);
        let selectable = layout::selectable(&laid);
        let Some(current) = self.selected_row(&posts) else {
            return;
        };
        let view = self.view(width, true);
        let top = |row: usize| view.rows.get(row).map_or(0, |span| span.start as i64);
        let from = top(current);
        let target = if rows >= 0 {
            selectable
                .iter()
                .copied()
                .find(|&row| top(row) >= from + rows)
                .or(selectable.last().copied())
        } else {
            selectable
                .iter()
                .rev()
                .copied()
                .find(|&row| top(row) <= from + rows)
                .or(selectable.first().copied())
        };
        if let Some(row) = target {
            self.selected.insert(self.selection_key(), row);
            self.note_selection();
        }
    }

    /**
    The open list has another page and the selection has reached the prefetch post of that page.
    Threads and the demo never page. A fetch already started for this feed stays quiet until the
    selection moves back before that post.
    */
    pub(crate) fn page_due(&self) -> bool {
        if !self.past_prefetch() {
            return false;
        }
        let Some(id) = self.current_feed_id() else {
            return false;
        };
        !self
            .page_gate
            .as_ref()
            .is_some_and(|(gated, _)| gated == &id)
    }

    /// The selection is at or past the post where the next page should start.
    pub(crate) fn past_prefetch(&self) -> bool {
        if self.demo || self.settings || self.thread_focus.is_some() {
            return false;
        }
        let Some(id) = self.current_feed_id() else {
            return false;
        };
        if matches!(id, FeedId::Thread(_)) {
            return false;
        }
        let Some(feed) = self.feeds.get(&id) else {
            return false;
        };
        if !feed.ready || feed.next.is_empty() {
            return false;
        }
        let Some(at) = self.page_index(feed) else {
            return false;
        };
        prefetch_from(page_len(feed)).is_some_and(|from| at >= from)
    }

    /**
    Where the selected post sits in the current page, in feed order. A reply that the page did
    not list counts as its ancestor, so opening one does not look like a new page.
    */
    fn page_index(&self, feed: &Feed) -> Option<usize> {
        let members = page_members(feed);
        let mut id = self.selected_post_id()?;
        for _ in 0..=self.posts.len() {
            if members.contains(id.as_str()) {
                return feed
                    .ids
                    .iter()
                    .filter(|candidate| members.contains(candidate.as_str()))
                    .position(|candidate| candidate == &id);
            }
            match self
                .posts
                .get(&id)
                .and_then(|post| post.in_reply_to.clone())
            {
                Some(parent) if parent != id => id = parent,
                _ => return None,
            }
        }
        None
    }

    /// The laid row of the current view that holds the selection.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_row(&self.current_posts())
    }

    /// Selects laid row `row` of the current view; false when that row can't hold the selection.
    pub fn select_row(&mut self, row: usize) -> bool {
        let posts = self.current_posts();
        if !layout::selectable(&self.lay(&posts)).contains(&row) {
            return false;
        }
        self.selected.insert(self.selection_key(), row);
        self.note_selection();
        true
    }

    /// Selects post `id` in the current view, or with `fold` the fold row under it.
    pub fn select_post(&mut self, id: &str, fold: bool) -> bool {
        let posts = self.current_posts();
        let row = self.lay(&posts).iter().position(|row| match &row.target {
            Target::Post(post) => !fold && post == id,
            Target::Fold { parent, .. } => fold && parent == id,
            _ => false,
        });
        row.is_some_and(|row| self.select_row(row))
    }

    /// The Home feed's rows are the split's overview: the split is on and no thread is open.
    fn overview_on(&self) -> bool {
        self.overview && self.thread_focus.is_none() && self.section == Section::Home
    }

    /**
    Turns the split's overview on or off, keeping the feed's selected post. A reply the overview
    hides hands the selection to its nearest ancestor that it shows.
    */
    pub fn set_overview(&mut self, on: bool) {
        if self.overview == on {
            return;
        }
        let open = self.thread_focus.take();
        let kept = self.selected_post_id();
        self.overview = on;
        if let Some(mut id) = kept {
            for _ in 0..=self.posts.len() {
                if self.select_post(&id, false) {
                    break;
                }
                match self
                    .posts
                    .get(&id)
                    .and_then(|post| post.in_reply_to.clone())
                {
                    Some(parent) if parent != id => id = parent,
                    _ => break,
                }
            }
        }
        self.thread_focus = open;
    }

    /// Shows or hides every loaded reply under the selected post, keeping the selection on it.
    pub fn toggle_replies(&mut self) {
        let Some(id) = self.selected_post_id() else {
            return;
        };
        if !self.expanded.insert(id.clone()) {
            self.expanded.remove(&id);
        }
        self.select_post(&id, false);
        self.note_selection();
    }

    pub(crate) fn step_selection(&mut self, delta: i32) {
        self.move_sel(delta);
    }

    /// Selects the first selectable row, or the last when `last` is set.
    pub(crate) fn selection_edge(&mut self, last: bool) {
        if !last {
            self.selected.insert(self.selection_key(), 0);
            self.note_selection();
            return;
        }
        let posts = self.current_posts();
        let rows = self.lay(&posts);
        if let Some(row) = layout::selectable(&rows).last().copied() {
            self.selected.insert(self.selection_key(), row);
        }
        self.note_selection();
    }

    fn move_sel(&mut self, delta: i32) {
        let posts = self.current_posts();
        let rows = self.lay(&posts);
        let selectable = layout::selectable(&rows);
        if selectable.is_empty() {
            return;
        }
        let current = self
            .selected_row(&posts)
            .and_then(|row| selectable.iter().position(|r| *r == row))
            .unwrap_or(0);
        let next = if delta >= 0 {
            selectable[(current + delta as usize).min(selectable.len() - 1)]
        } else {
            selectable[current.saturating_sub(delta.unsigned_abs() as usize)]
        };
        self.selected.insert(self.selection_key(), next);
        self.note_selection();
    }

    fn note_selection(&mut self) {
        let posts = self.current_posts();
        if posts.is_empty() {
            self.status = self
                .notice()
                .and_then(|notice| notice.lines().next().map(str::to_string))
                .unwrap_or_else(|| "nothing on this page".into());
            return;
        }
        let rows = self.lay(&posts);
        let Some(row) = self.selected_row(&posts).and_then(|i| rows.get(i)) else {
            return;
        };
        self.status = match &row.target {
            Target::Fold { hidden, .. } => format!("show the last {hidden} · enter"),
            Target::Post(_) => String::new(),
            Target::Gap => self.status.clone(),
            Target::Profile => match self.profile() {
                Some((_, user)) if self.following.contains(&user.username) => "following".into(),
                _ => String::new(),
            },
        };
    }

    pub fn open_user(
        &mut self,
        scope: Scope,
        handle: String,
        clients: &mut Clients,
    ) -> Result<(), String> {
        self.push_place();
        self.section = Section::Search;
        self.remote_query = handle.clone();
        self.thread_focus = None;
        let id = FeedId::User(scope, handle.clone());
        self.remote = Some(id.clone());
        if self.demo {
            let feed = self.demo_feed(&id);
            self.feeds.insert(id, feed);
            self.confirm(format!("demo · @{handle} on {}", scope.label()));
            self.note_selection();
            Ok(())
        } else {
            self.load_feed(id, clients, false)
        }
    }

    /// Opens the profile of the selected post's author on that post's network; a repost opens
    /// the original author.
    pub fn open_author(&mut self, clients: &mut Clients) -> Result<(), String> {
        let id = self.resolve(None)?;
        let post = self
            .posts
            .get(&id)
            .ok_or("that post is gone · r refreshes")?;
        let (net, handle) = (post.net, post.author.username.clone());
        self.open_user(Scope::One(net), handle, clients)
    }

    /// Post `n`, or the selected post; the error says why the selected row isn't one.
    pub fn resolve(&self, n: Option<usize>) -> Result<String, String> {
        let posts = self.current_posts();
        let rows = self.lay(&posts);
        if let Some(n) = n {
            return rows
                .iter()
                .find(|r| r.number == Some(n))
                .and_then(layout::post_id)
                .map(str::to_string)
                .ok_or_else(|| format!("no post {n}"));
        }
        let row = self
            .selected_row(&posts)
            .and_then(|i| rows.get(i))
            .ok_or_else(|| "nothing selected".to_string())?;
        match &row.target {
            Target::Post(id) => Ok(id.clone()),
            Target::Fold { .. } => Err("that row is a fold · enter opens it".into()),
            Target::Gap => Err("nothing selected".into()),
            Target::Profile => {
                Err("that row is the profile · s shares it, o opens it in the browser".into())
            }
        }
    }

    /// The profile link when the selected row is the profile header.
    pub fn selected_profile_url(&self) -> Option<String> {
        let posts = self.current_posts();
        let rows = self.lay(&posts);
        let row = self.selected_row(&posts).and_then(|i| rows.get(i))?;
        let (net, user) = self.profile().filter(|_| row.target == Target::Profile)?;
        Some(net.profile_url(&user.username))
    }

    // MARK: demo

    /// A demo search, account or thread, read from the posts already in memory.
    fn demo_feed(&self, id: &FeedId) -> Feed {
        let matches = |p: &Post, needle: &str| {
            p.text.to_lowercase().contains(needle)
                || p.author.username.to_lowercase().contains(needle)
                || p.author.name.to_lowercase().contains(needle)
        };
        let search = |scope: Scope, query: &str| {
            let needle = query.to_lowercase();
            (
                self.posts
                    .values()
                    .filter(|p| scope.includes(p.net) && matches(p, &needle))
                    .map(|p| p.id.clone())
                    .collect(),
                format!("demo · nothing matches {query}"),
            )
        };
        let (ids, empty): (Vec<String>, String) = match id {
            FeedId::Search(scope, query) => search(*scope, query),
            FeedId::User(scope, handle) => (
                self.posts
                    .values()
                    .filter(|p| {
                        scope.includes(p.net)
                            && p.reposted_by
                                .as_deref()
                                .unwrap_or(&p.author.username)
                                .eq_ignore_ascii_case(handle.trim_start_matches('@'))
                    })
                    .map(|p| p.id.clone())
                    .collect(),
                format!("demo · no posts from @{handle} on {}", scope.label()),
            ),
            FeedId::Thread(target) => {
                let conversation = self.posts.get(target).map(|p| p.conversation_id.clone());
                (
                    self.posts
                        .values()
                        .filter(|p| Some(&p.conversation_id) == conversation.as_ref())
                        .map(|p| p.id.clone())
                        .collect(),
                    format!("demo · no post {target}"),
                )
            }
            _ => (Vec::new(), "demo · nothing on this page".into()),
        };
        Feed {
            notice: ids.is_empty().then_some(empty),
            ids,
            ready: true,
            ..Feed::default()
        }
    }

    /// Installs the in-memory conversation so the demo's right pane can show it before a key.
    pub(crate) fn ensure_demo_thread(&mut self, post_id: &str) {
        if !self.demo {
            return;
        }
        let id = FeedId::Thread(post_id.to_string());
        if self.feeds.get(&id).is_some_and(|feed| feed.ready) {
            return;
        }
        let feed = self.demo_feed(&id);
        self.feeds.insert(id, feed);
    }

    // MARK: reads

    fn open_thread(&mut self, id: &str, clients: &mut Clients) -> Result<(), String> {
        let feed_id = FeedId::Thread(id.to_string());
        if self.demo {
            if !self.posts.contains_key(id) {
                return Err(format!("no post {id} in the demo"));
            }
            let feed = self.demo_feed(&feed_id);
            self.feeds.insert(feed_id.clone(), feed);
        } else {
            self.load_feed(feed_id.clone(), clients, false)?;
        }
        // A failed fetch must not add a back step or leave Home selected over the feed that failed.
        self.push_place();
        self.section = Section::Home;
        let focus = self
            .posts
            .get(id)
            .map(|p| p.conversation_id.clone())
            .unwrap_or_else(|| id.to_string());
        self.focus = Some(feed_id);
        self.thread_focus = Some(focus);
        self.note_selection();
        Ok(())
    }

    /** Fetches a feed from every network it covers, in parallel, and merges the pages. */
    fn load_feed(&mut self, id: FeedId, clients: &mut Clients, append: bool) -> Result<(), String> {
        let Some(plan) = self.plan_page(id, append)? else {
            return Ok(());
        };
        let fetched = fetch_page(clients, &plan);
        self.store_page(plan, fetched)
    }

    /**
    The next page of the feed on screen, for the reader to fetch off the input thread.
    `Ok(None)` when this view does not page (the demo, a thread, or a list with no cursor).
    */
    pub(crate) fn plan_more(&mut self) -> Result<Option<PagePlan>, String> {
        if self.demo {
            return Ok(None);
        }
        let Some(id) = self.current_feed_id() else {
            return Ok(None);
        };
        if matches!(id, FeedId::Thread(_)) {
            return Ok(None);
        }
        let has_next = self
            .feeds
            .get(&id)
            .is_some_and(|feed| !feed.next.is_empty());
        if !has_next {
            return Ok(None);
        }
        self.plan_page(id, true)
    }

    /**
    The conversation the right pane should show, when it is not loaded yet.
    `Ok(None)` for the demo, Settings, nothing selected, or a thread already stored.
    A failed plan is recorded as a ready empty thread so the next frame does not ask again.
    */
    pub(crate) fn plan_preview(&mut self) -> Result<Option<PagePlan>, String> {
        if self.demo || self.settings {
            return Ok(None);
        }
        let Some(post_id) = self.preview_post_id() else {
            return Ok(None);
        };
        let conversation = self
            .posts
            .get(&post_id)
            .map(|post| post.conversation_id.clone())
            .unwrap_or_else(|| post_id.clone());
        if self.thread_feed_for(&conversation).is_some() {
            return Ok(None);
        }
        // Bluesky's root id is the conversation. X's conversation id is a post id only when that root is loaded.
        let target = if self.posts.contains_key(&conversation) {
            conversation
        } else {
            post_id
        };
        let id = FeedId::Thread(target);
        if self.feeds.get(&id).is_some_and(|feed| feed.ready) {
            return Ok(None);
        }
        match self.plan_page(id.clone(), false) {
            Ok(plan) => Ok(plan),
            Err(err) => {
                self.feeds.insert(
                    id,
                    Feed {
                        ready: true,
                        notice: Some(err.clone()),
                        ..Feed::default()
                    },
                );
                Err(err)
            }
        }
    }

    /// The post whose conversation the right pane is showing.
    fn preview_post_id(&self) -> Option<String> {
        if let Some(focus) = &self.thread_focus {
            if self.posts.contains_key(focus) {
                return Some(focus.clone());
            }
        }
        self.selected_post_id()
    }

    /// What one feed load will ask the networks for. `Ok(None)` is the welcome screen, not a failure.
    fn plan_page(&mut self, id: FeedId, append: bool) -> Result<Option<PagePlan>, String> {
        if self.demo {
            self.status = "demo · already local".into();
            return Ok(None);
        }
        let mut cursors = if append {
            self.feeds
                .get(&id)
                .map(|feed| feed.next.clone())
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let count = self.count;
        let work = match &id {
            FeedId::Merged | FeedId::Kind(_) => {
                let kinds: HashMap<Network, FeedKind> = self
                    .nets
                    .iter()
                    .filter(|info| {
                        info.signed_in
                            && self.held(info.net).is_none()
                            && self.on.contains(&info.net)
                    })
                    .filter_map(|info| {
                        let kind = match &id {
                            FeedId::Kind(kind) => info.caps.resolve_feed(info.net, *kind),
                            _ => info.caps.primary,
                        };
                        kind.map(|kind| (info.net, kind))
                    })
                    .collect();
                if kinds.is_empty() {
                    // Nobody is signed in. Home draws the welcome mark; this is not a failed fetch.
                    if matches!(id, FeedId::Merged) && self.available().is_empty() {
                        self.feeds.insert(
                            id.clone(),
                            Feed {
                                ready: true,
                                ..Feed::default()
                            },
                        );
                        if self.status.ends_with("· loading") {
                            self.status.clear();
                        }
                        return Ok(None);
                    }
                    return Err(match &id {
                        FeedId::Kind(kind) => missing_kind(*kind),
                        _ => NO_ACCOUNTS.into(),
                    });
                }
                let mut nets: Vec<Network> = kinds.keys().copied().collect();
                nets.sort();
                cursors.retain(|net, _| nets.contains(net));
                PageWork::Feeds(kinds.into_iter().collect())
            }
            FeedId::Feed(net, kind) => {
                let kind = *kind;
                self.check(*net, &format!("{} feed", kind.word()), |caps| {
                    caps.has_feed(kind)
                })?;
                cursors.retain(|candidate, _| candidate == net);
                PageWork::Feeds(vec![(*net, kind)])
            }
            FeedId::Mentions(scope) => {
                let nets = match scope {
                    Scope::All => self
                        .nets
                        .iter()
                        .filter(|info| {
                            info.signed_in
                                && self.held(info.net).is_none()
                                && info.caps.mentions
                                && self.on.contains(&info.net)
                        })
                        .map(|info| info.net)
                        .collect(),
                    Scope::One(net) => {
                        self.check(*net, "mentions", |caps| caps.mentions)?;
                        vec![*net]
                    }
                };
                if nets.is_empty() {
                    return Err(
                        "no signed-in network has mentions. `socli login bluesky` (or x, devnull, threads, mastodon)"
                            .into(),
                    );
                }
                cursors.retain(|net, _| nets.contains(net));
                PageWork::Mentions(nets)
            }
            FeedId::Search(scope, query) => {
                let nets = match scope {
                    Scope::All => self
                        .nets
                        .iter()
                        .filter(|info| {
                            info.signed_in
                                && self.held(info.net).is_none()
                                && info.caps.search
                                && self.on.contains(&info.net)
                        })
                        .map(|info| info.net)
                        .collect(),
                    Scope::One(net) => {
                        self.check(*net, "search", |caps| caps.search)?;
                        vec![*net]
                    }
                };
                if nets.is_empty() {
                    return Err(
                        "no network here can search. `socli login bluesky` (or x, threads, mastodon)".into(),
                    );
                }
                cursors.retain(|net, _| nets.contains(net));
                PageWork::Search {
                    nets,
                    query: query.clone(),
                }
            }
            FeedId::User(scope, handle) => {
                let Scope::One(net) = scope else {
                    return Err(
                        "which network? add --net x|bluesky|devnull|threads|mastodon".into(),
                    );
                };
                self.check(*net, "profiles", |caps| caps.user)?;
                cursors.retain(|candidate, _| candidate == net);
                PageWork::User {
                    net: *net,
                    handle: handle.clone(),
                }
            }
            FeedId::Thread(target) => {
                let net = net_of(target)?;
                self.check(net, "threads", |caps| caps.thread)?;
                cursors.clear();
                PageWork::Thread {
                    net,
                    target: target.clone(),
                }
            }
        };
        Ok(Some(PagePlan {
            id,
            append,
            cursors,
            count,
            work,
        }))
    }

    /**
    Merges `fetched` into the feed `plan` names. An append whose cursors no longer match the feed
    is dropped, so a refresh that won the race does not mix the old page back in. When the reader
    has not moved, an append keeps the selection on the same post after a merged feed sorts.
    */
    pub(crate) fn store_page(&mut self, plan: PagePlan, fetched: Fetched) -> Result<(), String> {
        if plan.append && page_stale(self, &plan) {
            return Ok(());
        }
        let kept = plan.append.then(|| {
            (
                self.selection_key(),
                self.selected_index(),
                self.selected_post_id(),
            )
        });
        let result = match fetched {
            Fetched::Pages(results) => self.merge(plan.id, results, plan.append),
            Fetched::Profile(results) => self.merge_profile(plan.id, results, plan.append),
        };
        if let Some((key, row, Some(post))) = kept {
            if self.selection_key() == key && self.selected_index() == row {
                self.select_post(&post, false);
            }
        }
        result
    }

    fn merge_profile(
        &mut self,
        id: FeedId,
        results: Vec<(Network, ProfilePage)>,
        append: bool,
    ) -> Result<(), String> {
        let mut profiles = Vec::new();
        let pages = results
            .into_iter()
            .map(|(net, result)| {
                (
                    net,
                    result.map(|(user, page)| {
                        profiles.push((net, user));
                        page
                    }),
                )
            })
            .collect();
        self.merge(id, pages, append)?;
        for (net, user) in profiles {
            self.users.insert((net, user.username.to_lowercase()), user);
        }
        Ok(())
    }

    /**
    Folds per-network pages into one feed. Failed networks become the notice; the load fails
    only when nothing came back, with the first error.
    */
    fn merge(
        &mut self,
        id: FeedId,
        results: Vec<(Network, Result<Page, NetError>)>,
        append: bool,
    ) -> Result<(), String> {
        let several = results.len() > 1;
        let merged = id.merged();
        let single = results.first().map(|(net, _)| *net);
        let mut failed = HashSet::new();
        let mut errors = Vec::new();
        let mut pages = Vec::new();
        for (net, result) in results {
            match result {
                Ok(page) => pages.push((net, page)),
                Err(err) => {
                    failed.insert(net);
                    errors.push(if several {
                        format!("{}: {err}", net.label())
                    } else {
                        err.to_string()
                    });
                }
            }
        }
        if pages.is_empty() {
            let feed = self.feeds.entry(id).or_default();
            feed.ready = true;
            feed.notice = Some(errors.join("\n"));
            return Err(errors
                .into_iter()
                .next()
                .unwrap_or_else(|| "nothing to load".into()));
        }
        let feed = self.feeds.entry(id).or_default();
        if !append {
            feed.ids.retain(|id| {
                self.posts
                    .get(id)
                    .is_some_and(|post| failed.contains(&post.net))
            });
            feed.next.retain(|net, _| failed.contains(net));
        }
        let mut n = 0;
        let mut fresh = Vec::new();
        for (net, page) in pages {
            n += page.ids.len();
            for post in page.posts {
                remember(&mut self.users, &post);
                self.posts.insert(post.id.clone(), post);
            }
            for id in page.ids {
                if !feed.ids.contains(&id) {
                    feed.ids.push(id.clone());
                    fresh.push(id);
                }
            }
            match page.next {
                Some(next) if next.trim().is_empty() => feed.next.remove(&net),
                Some(next) if append && feed.next.get(&net) == Some(&next) => {
                    errors.push(format!(
                        "{} pagination did not advance; stopped loading more",
                        net.label()
                    ));
                    feed.next.remove(&net)
                }
                Some(next) => feed.next.insert(net, next),
                None => feed.next.remove(&net),
            };
        }
        if several || merged {
            feed.ids
                .sort_by_key(|id| Reverse(self.posts.get(id).and_then(|p| p.created_at)));
        }
        // After the sort, so a merged page is the ids in the order the list draws them.
        if !append {
            feed.page_ids = feed.ids.clone();
        } else if !fresh.is_empty() {
            feed.page_ids = fresh;
        }
        feed.ready = true;
        feed.notice = if !errors.is_empty() {
            Some(errors.join("\n"))
        } else if feed.ids.is_empty() {
            Some(match single.filter(|_| !several) {
                Some(net) => format!("{} returned an empty page.", net.title()),
                None => "Every network returned an empty page.".into(),
            })
        } else {
            None
        };
        self.status = format!("{n} posts");
        self.note_selection();
        if let Some(first) = errors.into_iter().next() {
            self.fail(first);
        }
        Ok(())
    }

    // MARK: writes

    /// Publishes on `net`; returns the new post's URL, or the demo note.
    pub fn write_post(
        &mut self,
        text: &str,
        reply_to: Option<String>,
        quote: Option<String>,
        net: Network,
        clients: &mut Clients,
    ) -> Result<String, String> {
        self.write_post_checked(text, reply_to, quote, net, clients)
            .map_err(|err| err.to_string())
    }

    pub(crate) fn write_post_checked(
        &mut self,
        text: &str,
        reply_to: Option<String>,
        quote: Option<String>,
        net: Network,
        clients: &mut Clients,
    ) -> Result<String, NetError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(NetError::Other("empty post".into()));
        }
        if quote.is_some() {
            self.check(net, "quotes", |caps| caps.quote)
                .map_err(NetError::Other)?;
        } else if reply_to.is_some() {
            self.check(net, "replies", |caps| caps.post)
                .map_err(NetError::Other)?;
        } else {
            self.check(net, "posts", |caps| caps.post)
                .map_err(NetError::Other)?;
        }
        if self.demo {
            let native = format!("local-{}", self.posts.len() + 1);
            let id = net::scoped(net, &native);
            let author = demo::you(net);
            let conversation = reply_to
                .as_ref()
                .and_then(|parent| self.posts.get(parent))
                .map(|p| p.conversation_id.clone())
                .unwrap_or_else(|| id.clone());
            let post = Post {
                id: id.clone(),
                net,
                url: Some(demo::permalink(net, &author.username, &native)),
                cid: (net == Network::Bluesky).then(|| format!("bafyreidemo{native}")),
                author,
                text: text.to_string(),
                created_at: Some(self.now()),
                conversation_id: conversation,
                in_reply_to: reply_to.clone(),
                quoted: quote
                    .as_ref()
                    .and_then(|id| self.posts.get(id).cloned())
                    .map(Box::new),
                reposted_by: None,
                metrics: Metrics::default(),
                links: Vec::new(),
                media: Vec::new(),
                liked: false,
                bookmarked: false,
                reposted: false,
            };
            self.keep(post);
            let feed_id = self.current_feed_id().unwrap_or(FeedId::Merged);
            self.feeds
                .entry(feed_id)
                .or_insert_with(|| Feed {
                    ready: true,
                    ..Feed::default()
                })
                .ids
                .insert(0, id);
            self.expanded.insert(reply_to.unwrap_or_default());
            self.confirm("demo · kept on this screen only");
            return Ok(self.status.clone());
        }
        let draft = Draft {
            text: text.to_string(),
            reply_to,
            quote,
        };
        #[cfg(feature = "native")]
        let task = crate::program_status::task(net, crate::program_status::TaskKind::Post);
        let posted = provider(clients, net)
            .map_err(NetError::Other)
            .and_then(|provider| provider.post(&draft));
        #[cfg(feature = "native")]
        let posted = task.finish(posted);
        let post = posted?;
        let url = post.permalink();
        self.keep(post);
        if let Some(id) = self.current_feed_id() {
            let _ = self.load_feed(id, clients, false);
        }
        self.confirm(format!("posted {url}"));
        Ok(url)
    }

    fn toggle_repost(
        &mut self,
        n: Option<usize>,
        on: bool,
        clients: &mut Clients,
    ) -> Result<(), String> {
        let id = self.resolve(n)?;
        let net = net_of(&id)?;
        self.check(net, "reposts", |caps| caps.repost)?;
        if !on {
            self.check(net, "repost undo", |caps| caps.unrepost)?;
        }
        if !self.demo {
            #[cfg(feature = "native")]
            let task = crate::program_status::task(net, crate::program_status::TaskKind::Action);
            let result = provider(clients, net)
                .and_then(|provider| provider.set_repost(&id, on).map_err(String::from));
            #[cfg(feature = "native")]
            let result = task.finish(result);
            result?;
        }
        if let Some(post) = self.posts.get_mut(&id) {
            set_counted_flag(&mut post.reposted, &mut post.metrics.reposts, on);
        }
        if self.demo {
            self.confirm(if on {
                "demo · reposted here only"
            } else {
                "demo · unreposted here only"
            });
            return Ok(());
        }
        self.confirm(if on { "reposted" } else { "unreposted" });
        Ok(())
    }

    fn toggle_flag(
        &mut self,
        n: Option<usize>,
        flag: Flag,
        on: bool,
        clients: &mut Clients,
    ) -> Result<(), String> {
        let id = self.resolve(n)?;
        let net = net_of(&id)?;
        match flag {
            Flag::Like => self.check(net, "likes", |caps| caps.like)?,
            Flag::Bookmark => self.check(net, "bookmarks", |caps| caps.bookmark)?,
        }
        let word = flag.word(net, on);
        if !self.demo {
            #[cfg(feature = "native")]
            let task = crate::program_status::task(net, crate::program_status::TaskKind::Action);
            let result = provider(clients, net).and_then(|provider| {
                match flag {
                    Flag::Like => provider.set_like(&id, on),
                    Flag::Bookmark => provider.set_bookmark(&id, on),
                }
                .map_err(String::from)
            });
            #[cfg(feature = "native")]
            let result = task.finish(result);
            result?;
        }
        if let Some(post) = self.posts.get_mut(&id) {
            match flag {
                Flag::Like => set_counted_flag(&mut post.liked, &mut post.metrics.likes, on),
                Flag::Bookmark => {
                    set_counted_flag(&mut post.bookmarked, &mut post.metrics.bookmarks, on)
                }
            }
        }
        if self.demo {
            self.confirm(format!("demo · {word} here only"));
            return Ok(());
        }
        self.confirm(word);
        Ok(())
    }

    fn follow_user(&mut self, handle: &str, on: bool, clients: &mut Clients) -> Result<(), String> {
        let net = self.handle_network()?;
        self.check(net, "follows", |caps| caps.follow)?;
        let verb = if on { "followed" } else { "unfollowed" };
        if self.demo {
            if on {
                self.following.insert(handle.to_string());
            } else {
                self.following.remove(handle);
            }
            self.confirm(format!("demo · {verb} @{handle} here only"));
            return Ok(());
        }
        #[cfg(feature = "native")]
        let task = crate::program_status::task(net, crate::program_status::TaskKind::Action);
        let result = provider(clients, net)
            .and_then(|provider| provider.set_follow(handle, on).map_err(String::from));
        #[cfg(feature = "native")]
        let result = task.finish(result);
        result?;
        if on {
            self.following.insert(handle.to_string());
        } else {
            self.following.remove(handle);
        }
        self.confirm(format!("{verb} @{handle} on {}", net.label()));
        Ok(())
    }

    fn delete_post(&mut self, n: Option<usize>, clients: &mut Clients) -> Result<(), String> {
        let id = self.resolve(n)?;
        let net = net_of(&id)?;
        self.check(net, "deletes", |caps| caps.delete)?;
        if !self.demo {
            #[cfg(feature = "native")]
            let task = crate::program_status::task(net, crate::program_status::TaskKind::Action);
            let result = provider(clients, net)
                .and_then(|provider| provider.delete(&id).map_err(String::from));
            #[cfg(feature = "native")]
            let result = task.finish(result);
            result?;
        }
        self.posts.remove(&id);
        for feed in self.feeds.values_mut() {
            feed.ids.retain(|x| x != &id);
        }
        self.confirm(if self.demo {
            "demo · deleted here only"
        } else {
            "deleted"
        });
        Ok(())
    }
}

/// One reader fetch: which feed, whether it appends, and the cursors that fetch was planned with.
pub(crate) struct PagePlan {
    id: FeedId,
    append: bool,
    cursors: BTreeMap<Network, String>,
    count: u8,
    work: PageWork,
}

impl PagePlan {
    /// On an append, networks that already returned their last page are left out.
    fn paged(&self, nets: Vec<Network>) -> Vec<Network> {
        nets.into_iter()
            .filter(|net| !self.append || self.cursors.contains_key(net))
            .collect()
    }
}

enum PageWork {
    Feeds(Vec<(Network, FeedKind)>),
    Mentions(Vec<Network>),
    Search { nets: Vec<Network>, query: String },
    User { net: Network, handle: String },
    Thread { net: Network, target: String },
}

pub(crate) enum Fetched {
    Pages(Vec<(Network, Result<Page, NetError>)>),
    Profile(Vec<(Network, ProfilePage)>),
}

/// The cursors on `plan` are no longer the feed's, so this append belongs to a page that was replaced.
fn page_stale(state: &State, plan: &PagePlan) -> bool {
    let Some(feed) = state.feeds.get(&plan.id) else {
        return true;
    };
    plan.cursors
        .iter()
        .any(|(net, cursor)| feed.next.get(net) != Some(cursor))
}

fn missing_kind(kind: FeedKind) -> String {
    let slug = Network::ALL
        .into_iter()
        .find(|net| net.caps().resolve_feed(*net, kind).is_some())
        .map_or("<slug>", Network::slug);
    format!(
        "no signed-in network has a {} feed. Try --net {slug}",
        kind.word()
    )
}

fn no_poster() -> String {
    "no signed-in network can post. `socli login bluesky` (or x, devnull, threads, mastodon)".into()
}

fn net_of(id: &str) -> Result<Network, String> {
    net::split(id)
        .map(|(net, _)| net)
        .ok_or_else(|| format!("{id} has no network. use a full id like bluesky:at://…"))
}

fn provider(clients: &mut Clients, net: Network) -> Result<&mut dyn Provider, String> {
    if let Some(message) = net.hold() {
        return Err(message.into());
    }
    clients
        .get(net)
        .ok_or_else(|| NetError::SignInRequired(net).into())
}

/**
Runs `job` once per network, each on its own thread when there are several. A network without
a client answers sign-in required. Results come back in network order.
*/
fn fan_out<T: Send>(
    clients: &mut Clients,
    nets: &[Network],
    job: impl Fn(&mut dyn Provider) -> Result<T, NetError> + Sync,
) -> Vec<(Network, Result<T, NetError>)> {
    #[cfg(feature = "native")]
    let progress = crate::program_status::batch(nets.len());
    let mut providers: Vec<&mut dyn Provider> = clients
        .providers_mut()
        .into_iter()
        .filter(|p| nets.contains(&p.network()))
        .collect();
    let mut out: Vec<(Network, Result<T, NetError>)> = nets
        .iter()
        .filter(|net| !providers.iter().any(|p| p.network() == **net))
        .map(|net| (*net, Err(NetError::SignInRequired(*net))))
        .collect();
    #[cfg(feature = "native")]
    for (net, result) in &out {
        let task = crate::program_status::task(*net, crate::program_status::TaskKind::Read);
        let _ = task.finish(result.as_ref());
        progress.advance();
    }
    #[cfg(feature = "native")]
    let perform = |provider: &mut dyn Provider| {
        let task =
            crate::program_status::task(provider.network(), crate::program_status::TaskKind::Read);
        let result = task.finish(job(provider));
        progress.advance();
        result
    };
    #[cfg(not(feature = "native"))]
    let perform = job;
    if providers.len() <= 1 {
        if let Some(p) = providers.pop() {
            out.push((p.network(), perform(p)));
        }
    } else {
        // The browser build has no threads. Several providers run one after another.
        #[cfg(not(feature = "native"))]
        {
            for p in providers {
                let net = p.network();
                out.push((net, perform(p)));
            }
        }
        #[cfg(feature = "native")]
        std::thread::scope(|scope| {
            let perform = &perform;
            let handles: Vec<_> = providers
                .into_iter()
                .map(|p| (p.network(), scope.spawn(move || perform(p))))
                .collect();
            for (net, handle) in handles {
                let result = handle.join().unwrap_or_else(|_| {
                    progress.advance();
                    Err(NetError::Other(format!(
                        "the {} request crashed",
                        net.label()
                    )))
                });
                out.push((net, result));
            }
        });
    }
    out.sort_by_key(|(net, _)| *net);
    #[cfg(feature = "native")]
    progress.finish();
    out
}

/// Runs `plan` against `clients`. The caller can do this off the input thread.
pub(crate) fn fetch_page(clients: &mut Clients, plan: &PagePlan) -> Fetched {
    match &plan.work {
        PageWork::Feeds(kinds) => {
            let kinds: HashMap<Network, FeedKind> = kinds.iter().copied().collect();
            let mut nets: Vec<Network> = kinds.keys().copied().collect();
            nets.sort();
            let nets = plan.paged(nets);
            Fetched::Pages(fan_out(clients, &nets, |provider| {
                let cursor = plan.cursors.get(&provider.network()).map(String::as_str);
                provider.feed(kinds[&provider.network()], cursor, plan.count)
            }))
        }
        PageWork::Mentions(nets) => {
            let nets = plan.paged(nets.clone());
            Fetched::Pages(fan_out(clients, &nets, |provider| {
                let cursor = plan.cursors.get(&provider.network()).map(String::as_str);
                provider.mentions(cursor, plan.count)
            }))
        }
        PageWork::Search { nets, query } => {
            let nets = plan.paged(nets.clone());
            Fetched::Pages(fan_out(clients, &nets, |provider| {
                let cursor = plan.cursors.get(&provider.network()).map(String::as_str);
                provider.search(query, cursor, plan.count)
            }))
        }
        PageWork::User { net, handle } => {
            let nets = plan.paged(vec![*net]);
            Fetched::Profile(fan_out(clients, &nets, |provider| {
                let cursor = plan.cursors.get(&provider.network()).map(String::as_str);
                provider.user(handle, cursor, plan.count)
            }))
        }
        PageWork::Thread { net, target } => Fetched::Pages(fan_out(clients, &[*net], |provider| {
            provider.thread(target, plan.count)
        })),
    }
}

fn set_counted_flag(flag: &mut bool, count: &mut u64, on: bool) {
    if *flag != on {
        *count = if on {
            count.saturating_add(1)
        } else {
            count.saturating_sub(1)
        };
        *flag = on;
    }
}

#[derive(Clone, Copy)]
enum Flag {
    Like,
    Bookmark,
}

impl Flag {
    /// The status verb; likes use the network's own word (`starred` on dev/null).
    fn word(self, net: Network, on: bool) -> String {
        match (self, on) {
            (Flag::Like, on) => net.caps().liked_word(on),
            (Flag::Bookmark, true) => "bookmarked".into(),
            (Flag::Bookmark, false) => "unbookmarked".into(),
        }
    }
}

/// Records the authors of a post and of the post it quotes, for profile headers.
fn remember(users: &mut HashMap<(Network, String), User>, post: &Post) {
    for one in std::iter::once(post).chain(post.quoted.as_deref()) {
        let key = (one.net, one.author.username.to_lowercase());
        let mut user = one.author.clone();
        if user.description.is_none() {
            user.description = users.get(&key).and_then(|known| known.description.clone());
        }
        users.insert(key, user);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::command::Cmd;

    /// Records `thread` calls. `None` is a failed fetch. Every other call fails.
    struct Scripted {
        calls: Arc<Mutex<Vec<String>>>,
        page: Option<Page>,
        writes_succeed: bool,
    }

    impl Provider for Scripted {
        fn network(&self) -> Network {
            Network::X
        }

        fn caps(&self) -> Caps {
            Network::X.caps()
        }

        fn signed_in(&self) -> bool {
            true
        }

        fn account(&self) -> Option<User> {
            None
        }

        fn me(&mut self) -> Result<User, NetError> {
            Err(NetError::Other("unused".into()))
        }

        fn feed(&mut self, _: FeedKind, _: Option<&str>, _: u8) -> Result<Page, NetError> {
            Err(NetError::Other("unused".into()))
        }

        fn mentions(&mut self, _: Option<&str>, _: u8) -> Result<Page, NetError> {
            Err(NetError::Other("unused".into()))
        }

        fn thread(&mut self, id: &str, _: u8) -> Result<Page, NetError> {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(id.to_string());
            self.page
                .clone()
                .ok_or_else(|| NetError::Other("thread failed".into()))
        }

        fn user(&mut self, _: &str, _: Option<&str>, _: u8) -> Result<(User, Page), NetError> {
            Err(NetError::Other("unused".into()))
        }

        fn search(&mut self, _: &str, _: Option<&str>, _: u8) -> Result<Page, NetError> {
            Err(NetError::Other("unused".into()))
        }

        fn post(&mut self, _: &Draft) -> Result<Post, NetError> {
            Err(NetError::Other("unused".into()))
        }

        fn delete(&mut self, _: &str) -> Result<(), NetError> {
            Err(NetError::Other("unused".into()))
        }

        fn set_like(&mut self, _: &str, _: bool) -> Result<(), NetError> {
            self.writes_succeed
                .then_some(())
                .ok_or_else(|| NetError::Other("write rejected".into()))
        }

        fn set_repost(&mut self, _: &str, _: bool) -> Result<(), NetError> {
            self.writes_succeed
                .then_some(())
                .ok_or_else(|| NetError::Other("write rejected".into()))
        }

        fn set_bookmark(&mut self, _: &str, _: bool) -> Result<(), NetError> {
            self.writes_succeed
                .then_some(())
                .ok_or_else(|| NetError::Other("write rejected".into()))
        }

        fn set_follow(&mut self, _: &str, _: bool) -> Result<(), NetError> {
            Err(NetError::Other("unused".into()))
        }
    }

    fn sample(id: &str, parent: Option<&str>, conversation: &str) -> Post {
        Post {
            id: id.to_string(),
            net: Network::X,
            url: None,
            cid: None,
            author: User {
                id: "u".into(),
                name: "Ada".into(),
                username: "ada".into(),
                avatar_url: None,
                verified: false,
                business: false,
                description: None,
            },
            text: id.to_string(),
            created_at: None,
            conversation_id: conversation.to_string(),
            in_reply_to: parent.map(str::to_string),
            quoted: None,
            reposted_by: None,
            metrics: Metrics::default(),
            links: Vec::new(),
            media: Vec::new(),
            liked: false,
            bookmarked: false,
            reposted: false,
        }
    }

    /// A live reader whose home page is exactly `posts`, in that order.
    fn live(posts: Vec<Post>) -> State {
        let mut state = demo();
        state.demo = false;
        let ids: Vec<String> = posts.iter().map(|post| post.id.clone()).collect();
        state.posts = posts
            .into_iter()
            .map(|post| (post.id.clone(), post))
            .collect();
        state.feeds.insert(
            FeedId::Merged,
            Feed {
                ids,
                ready: true,
                ..Feed::default()
            },
        );
        state.focus = None;
        state.thread_focus = None;
        state.section = Section::Home;
        state.history.clear();
        state.selected.clear();
        state.expanded.clear();
        state
    }

    fn scripted(page: Option<Page>) -> (Clients, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut clients = Clients::empty();
        clients.stub(Scripted {
            calls: Arc::clone(&calls),
            page,
            writes_succeed: false,
        });
        (clients, calls)
    }

    fn calls_of(calls: &Mutex<Vec<String>>) -> Vec<String> {
        calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn demo() -> State {
        State::boot(true, 15, &Config::default(), &Clients::empty(), None)
    }

    #[test]
    fn a_junk_delete_leaves_the_selected_post() {
        let mut state = demo();
        let mut clients = Clients::empty();
        let selected = state.selected_post_id().expect("a selected post");
        let before = state.posts.len();
        if let Ok(cmd) = crate::command::parse("delete xyz") {
            state.apply(cmd, &mut clients).expect("apply");
        }
        assert!(state.posts.contains_key(&selected));
        assert_eq!(state.posts.len(), before);
    }

    fn at_page_end() -> State {
        let mut state = crate::fixtures::demo_state(Detail::Standard);
        state.demo = false;
        let id = state.current_feed_id().expect("home");
        state
            .feeds
            .get_mut(&id)
            .expect("feed")
            .next
            .insert(Network::X, "cursor".into());
        let posts = state.current_posts();
        let last = layout::selectable(&state.lay(&posts))
            .last()
            .copied()
            .expect("a row");
        assert!(state.select_row(last));
        state
    }

    #[test]
    fn the_last_row_of_a_paged_feed_is_due_once() {
        let mut state = at_page_end();
        let id = state.current_feed_id().expect("home");
        assert_eq!(id, FeedId::Merged);
        assert!(state.page_due());

        state.page_gate = Some((id.clone(), state.selected_post_id()));
        assert!(!state.page_due());
        state.page_gate = None;

        let posts = state.current_posts();
        let first = layout::selectable(&state.lay(&posts))[0];
        assert!(state.select_row(first));
        assert!(!state.page_due());

        let last = layout::selectable(&state.lay(&state.current_posts()))
            .last()
            .copied()
            .expect("a row");
        assert!(state.select_row(last));
        assert!(state.page_due());

        state.demo = true;
        assert!(!state.page_due());
        state.demo = false;
        state.settings = true;
        assert!(!state.page_due());
        state.settings = false;
        state.thread_focus = Some("x:c1".into());
        assert!(!state.page_due());
        state.thread_focus = None;
        state.focus = Some(FeedId::Thread("x:1".into()));
        assert!(!state.page_due());
        state.focus = None;

        state.feeds.get_mut(&id).expect("feed").next.clear();
        assert!(!state.page_due());
        state
            .feeds
            .get_mut(&id)
            .expect("feed")
            .next
            .insert(Network::X, "cursor".into());
        state.feeds.get_mut(&id).expect("feed").ready = false;
        assert!(!state.page_due());
    }

    #[test]
    fn a_reader_page_holds_at_least_twenty_posts() {
        assert_eq!(page_count(1), 20);
        assert_eq!(page_count(15), 20);
        assert_eq!(page_count(20), 20);
        assert_eq!(page_count(50), 50);
        assert_eq!(prefetch_from(0), None);
        assert_eq!(prefetch_from(1), Some(0));
        assert_eq!(prefetch_from(10), Some(9));
        assert_eq!(prefetch_from(20), Some(9));
    }

    fn paged(n: usize) -> State {
        let posts = (0..n)
            .map(|i| crate::fixtures::post(&format!("x:{i}"), None, i as i64))
            .collect();
        let mut state = live(posts);
        state
            .feeds
            .get_mut(&FeedId::Merged)
            .expect("feed")
            .next
            .insert(Network::X, "cursor".into());
        state
    }

    #[test]
    fn the_tenth_post_of_the_current_page_is_due() {
        let mut state = paged(20);
        assert!(state.select_post("x:8", false));
        assert!(!state.page_due());
        assert!(state.select_post("x:9", false));
        assert!(state.page_due());

        state.page_gate = Some((FeedId::Merged, state.selected_post_id()));
        assert!(state.select_post("x:10", false));
        assert!(
            !state.page_due(),
            "a later post of the same page does not fetch again"
        );
        state.page_gate = None;
        assert!(state.page_due());

        state.posts.insert(
            "x:reply".into(),
            crate::fixtures::post("x:reply", Some("x:9"), 1),
        );
        state
            .feeds
            .get_mut(&FeedId::Merged)
            .expect("feed")
            .ids
            .push("x:reply".into());
        state.feeds.get_mut(&FeedId::Merged).expect("feed").page_ids =
            (0..20).map(|i| format!("x:{i}")).collect();
        assert!(
            state.select_post("x:reply", false),
            "the reply is selectable"
        );
        assert!(state.page_due(), "a reply counts as its ancestor");
        state.posts.get_mut("x:reply").expect("reply").in_reply_to = Some("x:8".into());
        assert!(!state.page_due());

        let mut state = paged(40);
        {
            let feed = state.feeds.get_mut(&FeedId::Merged).expect("feed");
            feed.page_ids = feed.ids[20..].to_vec();
        }
        assert!(state.select_post("x:28", false));
        assert!(!state.page_due());
        assert!(state.select_post("x:29", false));
        assert!(state.page_due());
    }

    #[test]
    fn an_append_keeps_only_the_ids_that_fetch_added() {
        let mut state = live(Vec::new());
        let page = |ids: &[&str], next: &str| Page {
            ids: ids.iter().map(|id| (*id).to_string()).collect(),
            posts: ids
                .iter()
                .map(|id| crate::fixtures::post(id, None, 1))
                .collect(),
            next: Some(next.into()),
        };
        state
            .merge(
                FeedId::Merged,
                vec![(Network::X, Ok(page(&["x:0", "x:1"], "one")))],
                false,
            )
            .expect("first page");
        let feed = state.feeds.get(&FeedId::Merged).expect("feed");
        assert_eq!(feed.page_ids, vec!["x:0".to_string(), "x:1".to_string()]);

        state
            .merge(
                FeedId::Merged,
                vec![(Network::X, Ok(page(&["x:1", "x:2"], "two")))],
                true,
            )
            .expect("append");
        let feed = state.feeds.get(&FeedId::Merged).expect("feed");
        assert_eq!(
            feed.ids,
            vec!["x:0".to_string(), "x:1".to_string(), "x:2".to_string()]
        );
        assert_eq!(feed.page_ids, vec!["x:2".to_string()]);

        let plan = PagePlan {
            id: FeedId::Merged,
            append: true,
            cursors: BTreeMap::from([(Network::X, "one".into())]),
            count: 20,
            work: PageWork::Feeds(vec![(Network::X, FeedKind::Following)]),
        };
        let stale = Fetched::Pages(vec![(Network::X, Ok(page(&["x:9"], "nine")))]);
        state
            .store_page(plan, stale)
            .expect("stale append is dropped");
        let feed = state.feeds.get(&FeedId::Merged).expect("feed");
        assert_eq!(feed.ids.len(), 3);
        assert_eq!(feed.next.get(&Network::X).map(String::as_str), Some("two"));
    }

    #[test]
    fn a_partial_refresh_keeps_the_failed_networks_posts_and_cursor() {
        let mut old = crate::fixtures::post("bluesky:old", None, 3);
        old.net = Network::Bluesky;
        let mut state = live(vec![old, crate::fixtures::post("x:old", None, 4)]);
        let feed = state.feeds.get_mut(&FeedId::Merged).expect("feed");
        feed.next.insert(Network::Bluesky, "retry-bsky".into());
        feed.next.insert(Network::X, "old-x".into());
        state
            .merge(
                FeedId::Merged,
                vec![
                    (
                        Network::X,
                        Ok(Page {
                            ids: vec!["x:new".into()],
                            posts: vec![crate::fixtures::post("x:new", None, 1)],
                            next: Some("new-x".into()),
                        }),
                    ),
                    (Network::Bluesky, Err(NetError::Other("offline".into()))),
                ],
                false,
            )
            .expect("the successful network refreshes");
        let feed = state.feeds.get(&FeedId::Merged).expect("feed");
        assert_eq!(feed.ids, ["x:new", "bluesky:old"]);
        assert_eq!(
            feed.next.get(&Network::Bluesky).map(String::as_str),
            Some("retry-bsky")
        );
        assert_eq!(
            feed.next.get(&Network::X).map(String::as_str),
            Some("new-x")
        );
        assert!(feed
            .notice
            .as_deref()
            .is_some_and(|notice| notice.contains("offline")));
    }

    #[test]
    fn a_failed_append_preserves_the_page_for_an_explicit_retry() {
        let mut state = paged(20);
        let before = state.feeds.get(&FeedId::Merged).expect("feed").ids.clone();
        let plan = state.plan_more().expect("plan").expect("next page");
        state
            .store_page(
                plan,
                Fetched::Pages(vec![(Network::X, Err(NetError::Other("offline".into())))]),
            )
            .expect_err("offline");
        let feed = state.feeds.get(&FeedId::Merged).expect("feed");
        assert_eq!(feed.ids, before);
        assert_eq!(
            feed.next.get(&Network::X).map(String::as_str),
            Some("cursor")
        );
        assert!(state.plan_more().expect("retry plan").is_some());
    }

    #[test]
    fn a_repeated_cursor_stops_pagination_without_discarding_posts() {
        let mut state = paged(20);
        let plan = state.plan_more().expect("page plan").expect("next page");
        state
            .store_page(
                plan,
                Fetched::Pages(vec![(
                    Network::X,
                    Ok(Page {
                        ids: vec!["x:0".into(), "x:20".into()],
                        posts: vec![
                            crate::fixtures::post("x:0", None, 0),
                            crate::fixtures::post("x:20", None, 20),
                        ],
                        next: Some("cursor".into()),
                    }),
                )]),
            )
            .expect("returned posts remain useful");
        let feed = &state.feeds[&FeedId::Merged];
        assert_eq!(feed.ids.len(), 21);
        assert!(feed.ids.iter().any(|id| id == "x:20"));
        assert!(feed
            .notice
            .as_deref()
            .is_some_and(|notice| notice.contains("did not advance")));
        assert!(state.plan_more().expect("plan").is_none());
    }

    #[test]
    fn empty_cursors_are_normalized_to_the_last_page() {
        for cursor in ["", "  "] {
            let mut state = live(Vec::new());
            state
                .merge(
                    FeedId::Merged,
                    vec![(
                        Network::X,
                        Ok(Page {
                            ids: vec!["x:post".into()],
                            posts: vec![sample("x:post", None, "x:post")],
                            next: Some(cursor.into()),
                        }),
                    )],
                    false,
                )
                .expect("page");
            assert!(state.feeds[&FeedId::Merged].next.is_empty());
            assert!(state.plan_more().expect("plan").is_none());
        }
    }

    #[test]
    fn a_merged_append_sorts_when_only_one_network_has_more_pages() {
        let mut old = crate::fixtures::post("bluesky:old", None, 3);
        old.net = Network::Bluesky;
        let mut state = live(vec![crate::fixtures::post("x:first", None, 1), old]);
        state
            .merge(
                FeedId::Merged,
                vec![(
                    Network::X,
                    Ok(Page {
                        ids: vec!["x:next".into()],
                        posts: vec![crate::fixtures::post("x:next", None, 2)],
                        next: None,
                    }),
                )],
                true,
            )
            .expect("append");
        assert_eq!(
            state.feeds[&FeedId::Merged].ids,
            ["x:first", "x:next", "bluesky:old"]
        );
    }

    #[test]
    fn repeated_flag_commands_are_idempotent_in_demo_and_live_readers() {
        for demo in [true, false] {
            let mut state = live(vec![sample("x:post", None, "x:post")]);
            state.demo = demo;
            let mut clients = Clients::empty();
            clients.stub(Scripted {
                calls: Arc::new(Mutex::new(Vec::new())),
                page: None,
                writes_succeed: true,
            });
            for command in [
                Cmd::Like { n: None },
                Cmd::Bookmark { n: None },
                Cmd::Repost { n: None },
            ] {
                state
                    .apply(command.clone(), &mut clients)
                    .expect("first write");
                state.apply(command, &mut clients).expect("repeated write");
            }
            let post = &state.posts["x:post"];
            assert!(post.liked && post.bookmarked && post.reposted);
            assert_eq!(
                (
                    post.metrics.likes,
                    post.metrics.bookmarks,
                    post.metrics.reposts
                ),
                (1, 1, 1)
            );
            for command in [
                Cmd::Unlike { n: None },
                Cmd::Unbookmark { n: None },
                Cmd::Unrepost { n: None },
            ] {
                state.apply(command.clone(), &mut clients).expect("undo");
                state.apply(command, &mut clients).expect("repeated undo");
            }
            let post = &state.posts["x:post"];
            assert!(!post.liked && !post.bookmarked && !post.reposted);
            assert_eq!(
                (
                    post.metrics.likes,
                    post.metrics.bookmarks,
                    post.metrics.reposts
                ),
                (0, 0, 0)
            );
        }
    }

    #[test]
    fn a_rejected_flag_write_preserves_flags_and_counts() {
        let mut state = live(vec![sample("x:post", None, "x:post")]);
        let before = state.posts["x:post"].clone();
        let (mut clients, _) = scripted(None);
        for command in [
            Cmd::Like { n: None },
            Cmd::Bookmark { n: None },
            Cmd::Repost { n: None },
        ] {
            let error = state.apply(command, &mut clients).err().expect("rejected");
            assert!(error.contains("write rejected"));
            assert_eq!(state.posts["x:post"], before);
        }
    }

    #[test]
    fn flag_counts_saturate_at_their_bounds() {
        let mut flag = false;
        let mut count = u64::MAX;
        set_counted_flag(&mut flag, &mut count, true);
        assert!(flag);
        assert_eq!(count, u64::MAX);
        count = 0;
        set_counted_flag(&mut flag, &mut count, false);
        assert!(!flag);
        assert_eq!(count, 0);
    }

    #[test]
    fn search_order_follows_the_feed_and_stays_put() {
        let mut state = demo();
        let id = FeedId::Search(Scope::One(Network::X), "elonmusk".into());
        state.section = Section::Search;
        state.remote = Some(id.clone());
        for (pid, parent) in [("x:b", None), ("x:a", Some("x:p"))] {
            state
                .posts
                .insert(pid.into(), crate::fixtures::post(pid, parent, 1));
        }
        state
            .posts
            .insert("x:p".into(), crate::fixtures::post("x:p", None, 2));
        state.feeds.insert(
            id,
            Feed {
                ids: vec!["x:b".into(), "x:a".into()],
                ready: true,
                ..Feed::default()
            },
        );
        let ids = || -> Vec<String> {
            state
                .current_posts()
                .into_iter()
                .map(|post| post.id)
                .collect()
        };
        let once = ids();
        assert_eq!(once, ["x:b", "x:a", "x:p"]);
        for _ in 0..8 {
            assert_eq!(ids(), once);
        }
    }

    #[test]
    fn a_confirmation_expires_four_seconds_after_it_is_set() {
        let mut state = demo();
        let now = Instant::now();
        state.confirm_at("liked", now);
        assert!(!state.expire(now + Duration::from_millis(3900)));
        assert_eq!(state.status, "liked");
        assert!(state.expire(now + Duration::from_secs(4)));
        assert_eq!(state.status, "");
        assert_eq!(state.status_expiry, None);
    }

    #[test]
    fn a_plain_status_after_a_confirmation_never_expires() {
        let mut state = demo();
        let now = Instant::now();
        state.confirm_at("liked", now);
        state.status = "menu · ? lists keys".into();
        assert!(!state.expire(now + Duration::from_secs(5)));
        assert_eq!(state.status, "menu · ? lists keys");
        assert_eq!(state.status_expiry, None);
    }

    #[test]
    fn demo_reply_joins_the_thread() {
        let mut state = demo();
        state
            .apply(
                Cmd::Reply {
                    n: Some(1),
                    text: "nice thread".into(),
                },
                &mut Clients::empty(),
            )
            .unwrap();
        let posts = state.posts_for(&FeedId::Merged);
        let reply = posts.iter().find(|p| p.text == "nice thread").unwrap();
        assert_eq!(reply.in_reply_to.as_deref(), Some("devnull:p2"));
        assert_eq!(reply.net, Network::Devnull);
        assert_eq!(reply.author.username, "you");
    }

    #[test]
    fn scope_keys_step_through_one_network_at_a_time() {
        let mut state = demo();
        let mut seen = vec![state.scope()];
        for _ in 0..6 {
            let next = state.cycle_scope(1).unwrap();
            state
                .apply(Cmd::Scope(next), &mut Clients::empty())
                .unwrap();
            seen.push(state.scope());
        }
        assert_eq!(seen[0], Scope::All);
        assert_eq!(seen[1..6], Network::ALL.map(Scope::One));
        assert_eq!(seen[6], Scope::One(Network::X), "> wraps among networks");
        state
            .apply(Cmd::Scope(Scope::All), &mut Clients::empty())
            .unwrap();
        assert_eq!(state.crumb(), "all › home");
        assert_eq!(state.on, Network::ALL.to_vec());
        assert_eq!(state.cycle_scope(-1), Some(Scope::One(Network::Mastodon)));
        state
            .apply(
                Cmd::Scope(Scope::One(Network::Bluesky)),
                &mut Clients::empty(),
            )
            .unwrap();
        assert_eq!(state.crumb(), "bluesky › following");
        let next = state.cycle_tab(1).unwrap();
        state.apply(next, &mut Clients::empty()).unwrap();
        assert_eq!(state.crumb(), "bluesky › discover");
    }

    #[test]
    fn a_steps_through_all_then_each_network_then_all() {
        let mut state = demo();
        let mut seen = vec![state.scope()];
        for _ in 0..6 {
            let next = state.next_scope().unwrap();
            state
                .apply(Cmd::Scope(next), &mut Clients::empty())
                .unwrap();
            seen.push(state.scope());
        }
        assert_eq!(seen[0], Scope::All);
        assert_eq!(seen[1..6], Network::ALL.map(Scope::One));
        assert_eq!(seen[6], Scope::All);

        state.set_on(vec![Network::X, Network::Threads]);
        assert_eq!(state.next_scope(), Some(Scope::All));

        state.nets.retain(|info| info.net == Network::Bluesky);
        assert_eq!(state.next_scope(), None);
    }

    #[test]
    fn the_starting_on_set_is_kept_to_signed_in_networks() {
        let start = |list: Option<&[Network]>, config: &Config| {
            State::boot(true, 15, config, &Clients::empty(), list).on
        };
        let none = Config::default();
        assert_eq!(start(None, &none), Network::ALL.to_vec());
        let pair = [Network::Bluesky, Network::X];
        assert_eq!(
            start(Some(&pair), &none),
            vec![Network::X, Network::Bluesky]
        );
        let mut saved = Config::default();
        saved.feed.networks = Some(vec!["threads".into()]);
        assert_eq!(start(None, &saved), vec![Network::Threads]);
        assert_eq!(start(Some(&pair[..1]), &saved), vec![Network::Bluesky]);

        let live = State::boot(false, 15, &none, &Clients::empty(), Some(&pair));
        assert!(live.on.is_empty(), "nothing is signed in, so nothing is on");
    }

    #[test]
    fn no_signed_in_account_keeps_an_empty_home() {
        let mut clients = Clients::empty();
        let mut state = State::boot(false, 15, &Config::default(), &clients, None);
        state.status = format!("{} · loading", state.account_label());
        state.ensure_loaded(&mut clients).unwrap();
        assert!(state.current_posts().is_empty());
        assert!(crate::onboard::showing(&state));
        assert!(!state.status.contains("no accounts"));
    }

    #[test]
    fn a_search_plan_leaves_out_a_network_that_is_not_signed_in() {
        let mut state = live(Vec::new());
        let mut signed_out = Network::X.caps();
        signed_out.search = true;
        let mut signed_in = Network::Bluesky.caps();
        signed_in.search = true;
        state.nets = vec![
            NetInfo {
                net: Network::X,
                signed_in: false,
                caps: signed_out,
                handle: None,
            },
            NetInfo {
                net: Network::Bluesky,
                signed_in: true,
                caps: signed_in,
                handle: None,
            },
        ];
        state.on = vec![Network::X, Network::Bluesky];
        let Ok(Some(PagePlan {
            work: PageWork::Search { nets, .. },
            ..
        })) = state.plan_page(FeedId::Search(Scope::All, "rust".into()), false)
        else {
            panic!("search plan");
        };
        assert!(!nets.contains(&Network::X));
        assert!(nets.contains(&Network::Bluesky));
    }

    #[test]
    fn two_networks_on_merge_only_their_posts() {
        let mut state = demo();
        state.set_on(vec![Network::X, Network::Threads]);
        assert_eq!(state.scope(), Scope::All);
        assert_eq!(state.crumb(), "all › home");
        let posts = state.current_posts();
        assert!(!posts.is_empty());
        assert!(posts
            .iter()
            .all(|p| matches!(p.net, Network::X | Network::Threads)));
        assert!(posts.iter().any(|p| p.net == Network::Threads));
        assert!(!state.all_on());
    }

    #[test]
    fn signed_out_networks_stay_off() {
        let mut state = demo();
        state.nets.retain(|info| info.net != Network::Threads);
        state
            .apply(
                Cmd::Scope(Scope::One(Network::Threads)),
                &mut Clients::empty(),
            )
            .unwrap();
        assert_eq!(state.status, "sign in first: `socli login threads`");
        assert_eq!(state.on, Network::ALL.to_vec());
        state
            .apply(Cmd::Scope(Scope::All), &mut Clients::empty())
            .unwrap();
        assert_eq!(
            state.on,
            vec![
                Network::X,
                Network::Bluesky,
                Network::Devnull,
                Network::Mastodon
            ]
        );

        state.nets.retain(|info| info.net == Network::Bluesky);
        state
            .apply(Cmd::Scope(Scope::All), &mut Clients::empty())
            .unwrap();
        assert_eq!(
            state.cycle_scope(1),
            None,
            "one network leaves < and > idle"
        );
        assert_eq!(state.cycle_scope(-1), None);
    }

    #[test]
    fn opening_a_post_fetches_the_missing_reply() {
        let root = sample("x:root", None, "x:root");
        let mut state = live(vec![root.clone()]);
        assert!(state.select_post("x:root", false));
        let selected = state.selected_post_id();
        let reply = sample("x:reply", Some("x:root"), "x:root");
        let (mut clients, calls) = scripted(Some(Page {
            ids: vec!["x:root".into(), "x:reply".into()],
            posts: vec![root, reply],
            next: None,
        }));

        state.apply(Cmd::Expand, &mut clients).unwrap();
        assert_eq!(calls_of(&calls), vec!["x:root".to_string()]);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.focus, Some(FeedId::Thread("x:root".into())));
        assert!(state
            .current_posts()
            .iter()
            .any(|post| post.id == "x:reply"));

        assert!(state.select_post("x:root", false));
        state.apply(Cmd::Expand, &mut clients).unwrap();
        assert_eq!(calls_of(&calls), vec!["x:root".to_string()]);
        assert_eq!(
            state.history.len(),
            1,
            "the open thread is not pushed again"
        );

        state.apply(Cmd::Back, &mut clients).unwrap();
        assert_eq!(state.focus, None);
        assert!(state.thread_focus.is_none());
        assert_eq!(state.selected_post_id(), selected);
        assert!(state
            .current_posts()
            .iter()
            .all(|post| post.id != "x:reply"));
    }

    #[test]
    fn a_filtered_feed_still_fetches_the_thread() {
        let root = sample("x:root", None, "x:root");
        let mut state = live(vec![root.clone()]);
        state.thread_focus = Some("x:root".into());
        assert_eq!(state.focus, None);
        let reply = sample("x:reply", Some("x:root"), "x:root");
        let (mut clients, calls) = scripted(Some(Page {
            ids: vec!["x:root".into(), "x:reply".into()],
            posts: vec![root, reply],
            next: None,
        }));
        state.apply(Cmd::Expand, &mut clients).unwrap();
        assert_eq!(calls_of(&calls), vec!["x:root".to_string()]);
        assert!(state
            .current_posts()
            .iter()
            .any(|post| post.id == "x:reply"));
    }

    #[test]
    fn split_loads_replies_without_replacing_the_feed() {
        let a = sample("x:a", None, "x:a");
        let b = sample("x:b", None, "x:b");
        let mut state = live(vec![a.clone(), b]);
        state.overview = true;
        assert!(state.select_post("x:a", false));
        let plan = state.plan_preview().unwrap().expect("the thread to load");
        assert_eq!(plan.id, FeedId::Thread("x:a".into()));
        let reply = sample("x:areply", Some("x:a"), "x:a");
        state
            .store_page(
                plan,
                Fetched::Pages(vec![(
                    Network::X,
                    Ok(Page {
                        ids: vec!["x:a".into(), "x:areply".into()],
                        posts: vec![a, reply],
                        next: None,
                    }),
                )]),
            )
            .unwrap();

        assert!(state.thread_focus.is_none());
        let listed: Vec<String> = state
            .current_posts()
            .iter()
            .map(|post| post.id.clone())
            .collect();
        assert!(listed.contains(&"x:a".to_string()));
        assert!(listed.contains(&"x:b".to_string()));
        assert!(!listed.iter().any(|id| id == "x:areply"));

        state.thread_focus = Some("x:a".into());
        let shown = state.current_posts();
        let open: Vec<&str> = shown.iter().map(|post| post.id.as_str()).collect();
        assert!(open.contains(&"x:areply"));
        assert!(!open.contains(&"x:b"));

        state.thread_focus = None;
        state.apply(Cmd::Expand, &mut Clients::empty()).unwrap();
        assert_eq!(state.thread_focus.as_deref(), Some("x:a"));
        assert!(!matches!(state.focus, Some(FeedId::Thread(_))));
        assert!(state.history.is_empty());

        let held = state.thread_focus.take();
        let left: Vec<String> = state
            .current_posts()
            .iter()
            .map(|post| post.id.clone())
            .collect();
        assert!(left.contains(&"x:a".to_string()));
        assert!(left.contains(&"x:b".to_string()));
        state.thread_focus = held;
        assert!(state
            .current_posts()
            .iter()
            .any(|post| post.id == "x:areply"));

        state.apply(Cmd::Back, &mut Clients::empty()).unwrap();
        assert!(state.thread_focus.is_none());
        let back: Vec<String> = state
            .current_posts()
            .iter()
            .map(|post| post.id.clone())
            .collect();
        assert!(back.contains(&"x:a".to_string()));
        assert!(back.contains(&"x:b".to_string()));
    }

    #[test]
    fn a_failed_thread_fetch_leaves_the_feed() {
        let mut state = live(vec![sample("x:root", None, "x:root")]);
        assert!(state.select_post("x:root", false));
        let (mut clients, calls) = scripted(None);
        let Err(err) = state.apply(Cmd::Expand, &mut clients) else {
            panic!("a failed fetch must be an error");
        };
        assert!(err.contains("thread failed"), "{err}");
        assert!(state.history.is_empty());
        assert!(state.thread_focus.is_none());
        assert_eq!(state.focus, None);
        assert_eq!(calls_of(&calls), vec!["x:root".to_string()]);
    }

    #[test]
    fn a_loaded_fold_does_not_fetch() {
        let posts = vec![
            sample("x:root", None, "x:root"),
            sample("x:r1", Some("x:root"), "x:root"),
            sample("x:r2", Some("x:root"), "x:root"),
            sample("x:r3", Some("x:root"), "x:root"),
            sample("x:r4", Some("x:root"), "x:root"),
        ];
        let mut state = live(posts);
        assert!(state.select_post("x:root", true));
        let (mut clients, calls) = scripted(None);
        state.apply(Cmd::Expand, &mut clients).unwrap();
        assert!(calls_of(&calls).is_empty());
        assert!(state.history.is_empty());
        assert!(state.expanded.contains("x:root"));
        assert_eq!(state.focus, None);
    }

    #[test]
    fn demo_expand_does_not_ask_the_provider() {
        let mut state = demo();
        let (mut clients, calls) = scripted(None);
        state.apply(Cmd::Expand, &mut clients).unwrap();
        assert!(calls_of(&calls).is_empty());
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.crumb(), "all › home › thread");
    }

    #[test]
    fn back_pops_one_view_and_stops_at_the_feed() {
        let mut state = demo();
        let clients = &mut Clients::empty();
        state.apply(Cmd::Expand, clients).unwrap();
        assert_eq!(state.crumb(), "all › home › thread");
        state.apply(Cmd::User("mina".into()), clients).unwrap();
        assert_eq!(state.section, Section::Search);
        state.apply(Cmd::Back, clients).unwrap();
        assert_eq!(state.crumb(), "all › home › thread");
        state.apply(Cmd::Back, clients).unwrap();
        assert_eq!(state.crumb(), "all › home");
        let status = state.status.clone();
        state.apply(Cmd::Back, clients).unwrap();
        assert_eq!(state.crumb(), "all › home");
        assert_eq!(state.status, status, "back on the root view does nothing");

        state
            .apply(Cmd::Section(Section::Notifications), clients)
            .unwrap();
        state.apply(Cmd::Search("reader".into()), clients).unwrap();
        state.apply(Cmd::Back, clients).unwrap();
        assert_eq!(state.section, Section::Notifications);
        state.apply(Cmd::Back, clients).unwrap();
        assert_eq!(state.section, Section::Home);
    }

    #[test]
    fn demo_user_without_a_network_lists_every_network() {
        let mut state = demo();
        state
            .apply(Cmd::User("mina".into()), &mut Clients::empty())
            .unwrap();
        let posts = state.current_posts();
        assert!(!posts.is_empty());
        assert!(posts.iter().all(|p| p.author.username == "mina"));
    }

    #[test]
    fn demo_user_timelines_belong_to_the_poster_or_reposter() {
        let mut state = demo();
        let clients = &mut Clients::empty();
        state
            .open_user(Scope::One(Network::X), "river".into(), clients)
            .unwrap();
        let posts = state.current_posts();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].id, "x:l1");
        state
            .open_user(Scope::One(Network::X), "moss".into(), clients)
            .unwrap();
        let posts = state.current_posts();
        assert!(posts
            .iter()
            .any(|post| post.id == "x:l3" && post.reposted_by.as_deref() == Some("moss")));
        assert!(posts
            .iter()
            .any(|post| post.id == "x:l2" && post.author.username == "moss"));
        assert!(posts.iter().all(|post| post.id != "x:l1"));
    }

    #[test]
    fn an_empty_profile_keeps_the_returned_user_and_later_posts_keep_its_bio() {
        let mut state = demo();
        let net = Network::X;
        let mut user = state.posts["x:l1"].author.clone();
        user.description =
            Some("Complete profile bio.\nSecond line from the profile response.".into());
        let id = FeedId::User(Scope::One(net), user.username.clone());
        state.section = Section::Search;
        state.remote = Some(id.clone());
        state
            .merge_profile(
                id,
                vec![(
                    net,
                    Ok((
                        user.clone(),
                        Page {
                            ids: Vec::new(),
                            posts: Vec::new(),
                            next: None,
                        },
                    )),
                )],
                false,
            )
            .unwrap();
        assert!(state.current_posts().is_empty());
        assert_eq!(state.profile(), Some((net, user.clone())));
        let mut post = state.posts["x:l1"].clone();
        post.author.description = None;
        state.keep(post);
        assert_eq!(state.profile().unwrap().1.description, user.description);
        let text = state
            .view(80, true)
            .lines
            .iter()
            .map(|line| line.plain())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Second line from the profile response."));
        assert!(text.contains("0 posts loaded"));
        let body = crate::screens::linked_body_sized(&state, 80, true, Some(18));
        assert!(body
            .lines
            .iter()
            .any(|line| line.plain().contains("Complete profile bio.")));
        assert!(body
            .targets
            .iter()
            .any(|target| matches!(target.act, render::Act::ShareProfile(..))));
    }

    #[test]
    fn threads_reposts_cannot_be_undone() {
        let mut state = demo();
        state.pin("threads:p10");
        let clients = &mut Clients::empty();
        state.apply(Cmd::Repost { n: None }, clients).unwrap();
        let err = state.apply(Cmd::Unrepost { n: None }, clients).err();
        assert_eq!(
            err.as_deref(),
            Some("Threads has no repost undo in its API")
        );
        assert!(state.posts["threads:p10"].reposted);
    }

    #[test]
    fn u_opens_the_original_authors_profile_under_a_selectable_header() {
        let mut state = demo();
        let clients = &mut Clients::empty();
        let posts = state.current_posts();
        let reposted = state
            .lay(&posts)
            .iter()
            .position(|row| {
                layout::post_id(row)
                    .and_then(|id| state.posts.get(id))
                    .is_some_and(|post| post.reposted_by.is_some())
            })
            .unwrap();
        state.selected.insert(state.selection_key(), reposted);
        let post = state.selected_post().unwrap();
        let (net, author) = (post.net, post.author.username.clone());
        state.open_author(clients).unwrap();
        assert_eq!(
            state.remote,
            Some(FeedId::User(Scope::One(net), author.clone()))
        );
        assert_eq!(state.crumb(), format!("{} › @{author}", net.label()));
        let posts = state.current_posts();
        assert!(posts
            .iter()
            .all(|p| p.net == net && p.author.username == author));
        assert_eq!(state.lay(&posts)[0].target, Target::Profile);
        let url = net.profile_url(&author);
        assert_eq!(state.selected_profile_url(), Some(url.clone()));
        assert!(matches!(
            state.apply(Cmd::Open { n: None }, clients),
            Ok(Outcome::Open(open)) if open == url
        ));
        assert!(state.resolve(None).unwrap_err().contains("profile"));
        state.apply(Cmd::Back, clients).unwrap();
        assert_eq!(state.crumb(), "all › home");
        assert!(state.profile().is_none());
        assert_eq!(
            state.selected_post().map(|p| p.author.username.clone()),
            Some(author)
        );
    }

    #[test]
    fn all_scope_cycles_its_own_tabs() {
        let mut state = demo();
        let clients = &mut Clients::empty();
        assert_eq!(state.merged_tabs(), MergedTab::ALL.to_vec());
        assert!(state.cycle_tab(1).is_none());
        assert!(state.tab_at(0).is_none());
        assert_eq!(state.cycle_merged_tab(1), Some(MergedTab::Discover));
        assert_eq!(state.cycle_merged_tab(-1), Some(MergedTab::Saved));
        assert_eq!(state.merged_tab_at(2), Some(MergedTab::Saved));
        state.open_merged_tab(MergedTab::Discover, clients).unwrap();
        assert_eq!(state.crumb(), "all › discover");
        assert_eq!(
            state.current_feed_id(),
            Some(FeedId::Kind(FeedKind::Discover))
        );
        assert!(!state.current_posts().is_empty());
        assert_eq!(state.cycle_merged_tab(1), Some(MergedTab::Saved));
        state
            .apply(Cmd::Scope(Scope::One(Network::X)), clients)
            .unwrap();
        state.apply(Cmd::Scope(Scope::All), clients).unwrap();
        assert_eq!(state.shown_merged_tab(), MergedTab::Discover);
        state.apply(Cmd::Tab(FeedKind::Bookmarks), clients).unwrap();
        assert_eq!(state.crumb(), "all › saved");
        state.apply(Cmd::Tab(FeedKind::Following), clients).unwrap();
        assert_eq!(state.status, PICK_NETWORK_FIRST);
    }

    #[test]
    fn unsupported_actions_never_reach_a_client() {
        let mut state = demo();
        state.pin("threads:p10");
        let err = state
            .apply(Cmd::Like { n: None }, &mut Clients::empty())
            .err();
        assert_eq!(err.as_deref(), Some("Threads has no likes in its API"));
        state.pin("devnull:p2");
        state
            .apply(Cmd::Like { n: None }, &mut Clients::empty())
            .unwrap();
        assert_eq!(state.status, "demo · starred here only");
        let err = state
            .apply(Cmd::Repost { n: None }, &mut Clients::empty())
            .err();
        assert_eq!(err.as_deref(), Some("dev/null has no reposts in its API"));
    }

    #[test]
    fn share_gives_the_selected_posts_link() {
        let mut state = demo();
        let mut clients = Clients::empty();
        match state.apply(Cmd::Share { n: None }, &mut clients).unwrap() {
            Outcome::Share { url, profile } => {
                assert!(!profile);
                assert_eq!(url, state.selected_post().unwrap().permalink());
            }
            _ => panic!("share must give a link"),
        }
    }

    #[test]
    fn composer_targets_follow_scope_then_default() {
        let mut state = demo();
        assert_eq!(state.initial_target(), Some(Network::X));
        state.default_network = Some(Network::Threads);
        assert_eq!(state.initial_target(), Some(Network::Threads));
        state.set_on(vec![Network::Bluesky]);
        assert_eq!(state.initial_target(), Some(Network::Bluesky));
        assert_eq!(state.post_targets(), Network::ALL.to_vec());
    }

    #[test]
    fn a_live_session_leaves_threads_out() {
        let mut saved = Config::default();
        saved.feed.networks = Some(vec!["threads".into(), "x".into()]);
        saved.default_network = Some("threads".into());
        let mut state = State::boot(false, 15, &saved, &Clients::empty(), None);
        state.nets.push(NetInfo {
            net: Network::Threads,
            signed_in: true,
            caps: Network::Threads.caps(),
            handle: Some("ada".into()),
        });
        assert_eq!(
            state.held(Network::Threads),
            Some("Waiting for Meta approval")
        );
        assert!(!state.available().contains(&Network::Threads));
        assert!(!state
            .chips()
            .iter()
            .any(|(net, _, _)| *net == Network::Threads));
        assert!(!state.post_targets().contains(&Network::Threads));
        assert!(state.on.iter().all(|net| *net != Network::Threads));
        state
            .apply(
                Cmd::Scope(Scope::One(Network::Threads)),
                &mut Clients::empty(),
            )
            .unwrap();
        assert_eq!(state.status, "Waiting for Meta approval");
        assert_eq!(
            state
                .check(Network::Threads, "search", |caps| caps.search)
                .unwrap_err(),
            "Waiting for Meta approval"
        );
    }
    #[test]
    fn demo_thread_shows_the_original_once_when_opened_from_either_record() {
        for target in ["x:l1", "x:l3"] {
            let mut state = demo();
            state.open_thread(target, &mut Clients::empty()).unwrap();
            let posts = state.current_posts();
            assert_eq!(posts.len(), 1);
            assert_eq!(posts[0].id, "x:l1");
            for detail in [
                Detail::Plain,
                Detail::Minimal,
                Detail::Standard,
                Detail::Rich,
            ] {
                state.detail = detail;
                let text = state
                    .view(100, false)
                    .lines
                    .iter()
                    .map(|line| line.plain())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(
                    text.matches("Half-block graphics still look like magic")
                        .count(),
                    1,
                    "{target}, {detail:?}"
                );
            }
        }
    }

    #[test]
    fn canonical_thread_posts_keep_replies_and_distinct_identical_text() {
        let mut state = demo();
        let mut reply = state.posts["x:l1"].clone();
        reply.id = "x:thread-reply-test".into();
        reply.url = Some("https://x.com/river/status/thread-reply-test".into());
        reply.in_reply_to = Some("x:l3".into());
        reply.media.clear();
        state.posts.insert(reply.id.clone(), reply);
        state.open_thread("x:l3", &mut Clients::empty()).unwrap();
        let posts = state.current_posts();
        assert_eq!(posts.len(), 2);
        let reply = posts
            .iter()
            .find(|post| post.id == "x:thread-reply-test")
            .unwrap();
        assert_eq!(reply.in_reply_to.as_deref(), Some("x:l1"));
        assert_eq!(
            posts.iter().filter(|post| post.text == reply.text).count(),
            2
        );
    }
    #[test]
    fn thread_posts_without_urls_are_not_collapsed() {
        let mut state = demo();
        state.posts.get_mut("x:l1").unwrap().url = None;
        state.posts.get_mut("x:l3").unwrap().url = None;
        state.open_thread("x:l1", &mut Clients::empty()).unwrap();
        assert_eq!(state.current_posts().len(), 2);
    }
    #[test]
    fn first_login_replaces_the_missing_tab_and_selects_discover() {
        let config = crate::config::BlueskyConfig {
            service: Some("http://127.0.0.1:1".into()),
            handle: Some("reader.test".into()),
            did: Some("did:plc:localtest".into()),
            access_jwt: Some("test-access".into()),
            ..Default::default()
        };
        let mut clients = Clients::empty();
        let mut state = State::boot(false, 15, &Config::default(), &clients, None);
        assert_eq!(state.tab_of(Network::Bluesky), None);
        clients.bluesky = crate::net::bluesky::BlueskyClient::new(&config);
        state.adopt_accounts(&clients, Some(Network::Bluesky));
        assert_eq!(state.tab_of(Network::Bluesky), Some(FeedKind::Discover));
        assert_eq!(
            state.current_feed_id(),
            Some(FeedId::Feed(Network::Bluesky, FeedKind::Discover))
        );
        let booted = State::boot(false, 15, &Config::default(), &clients, None);
        assert_eq!(booted.current_feed_id(), state.current_feed_id());
    }
}
