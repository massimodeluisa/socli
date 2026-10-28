/*!
The provider layer: one `Provider` per network behind a common trait, the capability table
the UI reads, and the scoped ids (`<slug>:<native>`) every post carries inside the app.
*/

pub mod bluesky;
pub mod caps;
pub mod devnull;
pub mod http;
pub mod mastodon;
pub mod pkce;
pub mod threads;
pub mod x;

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::config::Config;
use crate::model::{Page, Post, User};
use crate::theme::Kind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Network {
    X,
    Bluesky,
    Devnull,
    Threads,
    Mastodon,
}

impl Network {
    pub const ALL: [Network; 5] = [
        Network::X,
        Network::Bluesky,
        Network::Devnull,
        Network::Threads,
        Network::Mastodon,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Network::X => "x",
            Network::Bluesky => "bluesky",
            Network::Devnull => "devnull",
            Network::Threads => "threads",
            Network::Mastodon => "mastodon",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Network::X => "x",
            Network::Bluesky => "bluesky",
            Network::Devnull => "dev/null",
            Network::Threads => "threads",
            Network::Mastodon => "mastodon",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Network::X => "X",
            Network::Bluesky => "Bluesky",
            Network::Devnull => "dev/null",
            Network::Threads => "Threads",
            Network::Mastodon => "Mastodon",
        }
    }

    /// The network's color, `[<net>] color`, resolved by the theme.
    pub fn accent(self) -> Kind {
        Kind::Net(self)
    }

    /// The web profile of `handle`; a leading `@` is dropped.
    pub fn profile_url(&self, handle: &str) -> String {
        let handle = handle.trim_start_matches('@');
        match self {
            Network::X => format!("https://x.com/{handle}"),
            Network::Bluesky => format!("https://bsky.app/profile/{handle}"),
            Network::Devnull => format!("https://dev-null.sh/@{handle}"),
            Network::Threads => format!("https://www.threads.com/@{handle}"),
            Network::Mastodon => caps::mastodon_profile_url(handle),
        }
    }

    /// Characters `text` uses against `max_chars`, counted the way this network counts them.
    pub fn text_len(&self, text: &str) -> usize {
        match self {
            Network::X => crate::count::x_len(text),
            Network::Threads => caps::threads_text_len(text),
            Network::Mastodon => caps::mastodon_text_len(text),
            Network::Bluesky | Network::Devnull => self.caps().used(text),
        }
    }

    /// Live pause. The wording is the product message, capitals included.
    /// `--demo` ignores it. Removing the string turns the network back on.
    pub fn hold(self) -> Option<&'static str> {
        match self {
            Network::Threads => Some("Waiting for Meta approval"),
            _ => None,
        }
    }

    /// What a signed-in account can do here: the signed-in column of the capability table.
    pub fn caps(self) -> Caps {
        match self {
            Network::X => caps::x(true),
            Network::Bluesky => caps::bluesky(true),
            Network::Devnull => caps::devnull_device(),
            Network::Threads => caps::threads(),
            Network::Mastodon => caps::mastodon(true),
        }
    }
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl FromStr for Network {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "x" | "twitter" => Ok(Network::X),
            "bluesky" | "bsky" => Ok(Network::Bluesky),
            "devnull" | "dev/null" | "dev-null" => Ok(Network::Devnull),
            "threads" => Ok(Network::Threads),
            "mastodon" | "masto" => Ok(Network::Mastodon),
            other => Err(format!(
                "unknown network {other}. use x, bluesky, devnull, threads or mastodon"
            )),
        }
    }
}

/// Parses a `--net` value: a comma list of slugs, or `all` for every network.
pub fn parse_list(value: &str) -> Result<Vec<Network>, String> {
    let mut nets = Vec::new();
    for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if part.eq_ignore_ascii_case("all") {
            return Ok(Network::ALL.to_vec());
        }
        let net: Network = part.parse()?;
        if !nets.contains(&net) {
            nets.push(net);
        }
    }
    if nets.is_empty() {
        return Err("--net needs x, bluesky, devnull, threads, mastodon or all".into());
    }
    nets.sort();
    Ok(nets)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FeedKind {
    Following,
    ForYou,
    Latest,
    Trending,
    Discover,
    Bookmarks,
    Mine,
}

impl FeedKind {
    /// The name `socli feed` and the prompt use for this kind.
    pub fn word(self) -> &'static str {
        match self {
            FeedKind::Following => "following",
            FeedKind::ForYou => "foryou",
            FeedKind::Latest => "latest",
            FeedKind::Trending => "trending",
            FeedKind::Discover => "discover",
            FeedKind::Bookmarks => "saved",
            FeedKind::Mine => "mine",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Feed tabs in display order, with their labels.
    pub feeds: Vec<(FeedKind, &'static str)>,
    /// The feed merged into the All timeline.
    pub primary: Option<FeedKind>,
    pub mentions: bool,
    pub search: bool,
    pub like: bool,
    pub repost: bool,
    /// Whether a repost can be undone; read only where `repost` holds. False only on Threads.
    pub unrepost: bool,
    pub quote: bool,
    pub bookmark: bool,
    pub follow: bool,
    pub delete: bool,
    pub thread: bool,
    pub user: bool,
    pub post: bool,
    pub counts: bool,
    pub views: bool,
    pub like_verb: &'static str,
    pub max_chars: usize,
    pub count_graphemes: bool,
}

impl Caps {
    pub fn default_feed(&self, net: Network) -> Option<FeedKind> {
        self.resolve_feed(net, FeedKind::Discover)
            .or(self.primary)
            .or_else(|| self.feeds.first().map(|(kind, _)| *kind))
    }

    /// A client that can read public threads and profiles and nothing else.
    pub fn public(max_chars: usize) -> Self {
        Caps {
            feeds: Vec::new(),
            primary: None,
            mentions: false,
            search: false,
            like: false,
            repost: false,
            unrepost: true,
            quote: false,
            bookmark: false,
            follow: false,
            delete: false,
            thread: true,
            user: true,
            post: false,
            counts: true,
            views: false,
            like_verb: "like",
            max_chars,
            count_graphemes: false,
        }
    }

    pub fn feed_label(&self, kind: FeedKind) -> Option<&'static str> {
        self.feeds
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, label)| *label)
    }

    pub fn has_feed(&self, kind: FeedKind) -> bool {
        self.feed_label(kind).is_some()
    }

    /**
    The tab a feed name opens on `net`: the kind itself, else For you for `latest` and `discover`
    on X, and Trending for `discover` on dev/null and Mastodon.
    */
    pub fn resolve_feed(&self, net: Network, kind: FeedKind) -> Option<FeedKind> {
        if self.has_feed(kind) {
            return Some(kind);
        }
        let alias = match (net, kind) {
            (Network::X, FeedKind::Latest | FeedKind::Discover) => FeedKind::ForYou,
            (Network::Devnull, FeedKind::Discover) => FeedKind::Trending,
            (Network::Mastodon, FeedKind::Discover) => FeedKind::Trending,
            _ => return None,
        };
        self.has_feed(alias).then_some(alias)
    }

    /// Past tense of the like verb for status lines: `liked`, or `starred` on dev/null.
    pub fn liked_word(&self, on: bool) -> String {
        let verb = self.like_verb;
        let done = if verb.ends_with('e') {
            format!("{verb}d")
        } else if verb.ends_with('r') {
            format!("{verb}red")
        } else {
            format!("{verb}ed")
        };
        if on {
            done
        } else {
            format!("un{done}")
        }
    }

    /// Characters a draft uses against `max_chars`: graphemes where the network counts them.
    pub fn used(&self, text: &str) -> usize {
        if self.count_graphemes {
            unicode_segmentation::UnicodeSegmentation::graphemes(text, true).count()
        } else {
            text.chars().count()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub text: String,
    pub reply_to: Option<String>,
    pub quote: Option<String>,
}

#[derive(Debug, Error)]
pub enum NetError {
    #[error("sign in first: `socli login {}`", .0.slug())]
    SignInRequired(Network),
    #[error("{} has no {what} in its API", .net.title())]
    Unsupported { net: Network, what: String },
    #[error("{message}")]
    Http {
        net: Network,
        status: u16,
        message: String,
    },
    #[error("{} rate limit reached; {}", .net.title(), retry_text(*.retry_after))]
    RateLimited {
        net: Network,
        retry_after: Option<u64>,
    },
    #[error("could not read the {} response: {message}", .net.title())]
    Parse { net: Network, message: String },
    #[error("{message}; the request may have reached {}; check before retrying", .net.title())]
    OutcomeUnknown { net: Network, message: String },
    #[error("{0}")]
    Other(String),
}

impl NetError {
    pub fn outcome_unknown(&self) -> bool {
        matches!(self, Self::OutcomeUnknown { .. })
    }

    pub fn unsupported(net: Network, what: impl Into<String>) -> Self {
        NetError::Unsupported {
            net,
            what: what.into(),
        }
    }
}

fn retry_text(after: Option<u64>) -> String {
    match after {
        Some(secs) if secs >= 120 => format!("try again in {} minutes", secs.div_ceil(60)),
        Some(secs) => format!("try again in {secs}s"),
        None => "try again in a minute".into(),
    }
}

impl From<NetError> for String {
    fn from(err: NetError) -> Self {
        err.to_string()
    }
}

/** Every id argument and every id in a returned `Page`, `Post` or `User` is scoped. */
pub trait Provider: Send {
    fn network(&self) -> Network;
    fn caps(&self) -> Caps;
    /// User-level auth. An X app bearer is not signed in.
    fn signed_in(&self) -> bool;
    /// The cached signed-in account, if known.
    fn account(&self) -> Option<User>;
    fn me(&mut self) -> Result<User, NetError>;
    fn feed(&mut self, kind: FeedKind, cursor: Option<&str>, count: u8) -> Result<Page, NetError>;
    fn mentions(&mut self, cursor: Option<&str>, count: u8) -> Result<Page, NetError>;
    fn thread(&mut self, id: &str, count: u8) -> Result<Page, NetError>;
    fn user(
        &mut self,
        handle: &str,
        cursor: Option<&str>,
        count: u8,
    ) -> Result<(User, Page), NetError>;
    fn search(&mut self, query: &str, cursor: Option<&str>, count: u8) -> Result<Page, NetError>;
    /// Returns the created post; it may hold only the id, url, text and author.
    fn post(&mut self, draft: &Draft) -> Result<Post, NetError>;
    fn delete(&mut self, id: &str) -> Result<(), NetError>;
    fn set_like(&mut self, id: &str, on: bool) -> Result<(), NetError>;
    fn set_repost(&mut self, id: &str, on: bool) -> Result<(), NetError>;
    fn set_bookmark(&mut self, id: &str, on: bool) -> Result<(), NetError>;
    fn set_follow(&mut self, handle: &str, on: bool) -> Result<(), NetError>;
}

/// What the reader knows about one network's client, without borrowing it.
#[derive(Clone, Debug)]
pub struct NetInfo {
    pub net: Network,
    pub signed_in: bool,
    /// What the client can do in its current auth state.
    pub caps: Caps,
    pub handle: Option<String>,
}

/** One client per network. Bluesky, dev/null and Mastodon always exist so public reads work signed out. */
pub struct Clients {
    pub x: Option<x::XClient>,
    pub bluesky: bluesky::BlueskyClient,
    pub devnull: devnull::DevnullClient,
    pub threads: Option<threads::ThreadsClient>,
    pub mastodon: mastodon::MastodonClient,
    /// Replaces the real client for its network. Tests use this so a fetch never leaves the process.
    #[cfg(test)]
    stub: Option<Box<dyn Provider + Send>>,
}

impl Clients {
    /// True when the X client is a browser session. The browser build has no X client.
    pub fn uses_x_web(&self) -> bool {
        self.x
            .as_ref()
            .is_some_and(|client| client.credentials().uses_web())
    }

    pub fn load(config: &Config) -> Result<Clients, String> {
        let creds = x::auth::Credentials::from_config(&config.x);
        let x = if creds.has_any_read() {
            Some(x::XClient::new(creds).map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok(Clients {
            x,
            bluesky: bluesky::BlueskyClient::new(&config.bluesky),
            devnull: devnull::DevnullClient::new(&config.devnull),
            // The token stays in the file. A live session does not build the client while Threads is on hold,
            // so a stored token cannot refresh or post. `socli logout threads` still clears the section.
            threads: if Network::Threads.hold().is_some() {
                None
            } else {
                threads::ThreadsClient::from_config(&config.threads)
            },
            mastodon: mastodon::MastodonClient::new(&config.mastodon),
            #[cfg(test)]
            stub: None,
        })
    }

    /// No accounts at all; demo mode uses this and never calls it.
    pub fn empty() -> Clients {
        Clients {
            x: None,
            bluesky: bluesky::BlueskyClient::public(),
            devnull: devnull::DevnullClient::public(),
            threads: None,
            mastodon: mastodon::MastodonClient::public(),
            #[cfg(test)]
            stub: None,
        }
    }

    /// Installs a provider that stands in for its own network.
    #[cfg(test)]
    pub(crate) fn stub(&mut self, provider: impl Provider + 'static) {
        self.stub = Some(Box::new(provider));
    }

    #[cfg(test)]
    fn stub_net(&self) -> Option<Network> {
        self.stub.as_ref().map(|provider| provider.network())
    }

    #[cfg(not(test))]
    fn stub_net(&self) -> Option<Network> {
        None
    }

    pub fn get(&mut self, net: Network) -> Option<&mut (dyn Provider + '_)> {
        #[cfg(test)]
        if let Some(provider) = self.stub.as_mut() {
            if provider.network() == net {
                return Some(provider.as_mut());
            }
        }
        match net {
            Network::X => self.x.as_mut().map(|c| c as &mut dyn Provider),
            Network::Bluesky => Some(&mut self.bluesky),
            Network::Devnull => Some(&mut self.devnull),
            Network::Threads => self.threads.as_mut().map(|c| c as &mut dyn Provider),
            Network::Mastodon => Some(&mut self.mastodon),
        }
    }

    fn providers(&self) -> Vec<&dyn Provider> {
        let skip = self.stub_net();
        let mut out: Vec<&dyn Provider> = Vec::new();
        if skip != Some(Network::X) {
            if let Some(c) = &self.x {
                out.push(c);
            }
        }
        if skip != Some(Network::Bluesky) {
            out.push(&self.bluesky);
        }
        if skip != Some(Network::Devnull) {
            out.push(&self.devnull);
        }
        if skip != Some(Network::Threads) {
            if let Some(c) = &self.threads {
                out.push(c);
            }
        }
        if skip != Some(Network::Mastodon) {
            out.push(&self.mastodon);
        }
        #[cfg(test)]
        if let Some(provider) = &self.stub {
            out.push(provider.as_ref());
        }
        out
    }

    /// Disjoint mutable borrows, so a merged load can run every network on its own thread.
    pub fn providers_mut(&mut self) -> Vec<&mut dyn Provider> {
        let skip = self.stub_net();
        let mut out: Vec<&mut dyn Provider> = Vec::new();
        if skip != Some(Network::X) {
            if let Some(c) = self.x.as_mut() {
                out.push(c);
            }
        }
        if skip != Some(Network::Bluesky) {
            out.push(&mut self.bluesky);
        }
        if skip != Some(Network::Devnull) {
            out.push(&mut self.devnull);
        }
        if skip != Some(Network::Threads) {
            if let Some(c) = self.threads.as_mut() {
                out.push(c);
            }
        }
        if skip != Some(Network::Mastodon) {
            out.push(&mut self.mastodon);
        }
        #[cfg(test)]
        if let Some(provider) = self.stub.as_mut() {
            out.push(provider.as_mut());
        }
        out
    }

    pub fn infos(&self) -> Vec<NetInfo> {
        self.providers()
            .iter()
            .map(|p| NetInfo {
                net: p.network(),
                signed_in: p.signed_in(),
                caps: p.caps(),
                handle: p.account().map(|user| user.username),
            })
            .collect()
    }

    pub fn signed_in(&self) -> Vec<Network> {
        self.providers()
            .iter()
            .filter(|p| p.signed_in())
            .map(|p| p.network())
            .collect()
    }
}

pub fn scoped(net: Network, native: &str) -> String {
    format!("{}:{native}", net.slug())
}

pub fn split(id: &str) -> Option<(Network, &str)> {
    let (slug, native) = id.split_once(':')?;
    let net = Network::ALL.into_iter().find(|n| n.slug() == slug)?;
    Some((net, native))
}

/**
Turns a CLI target into a scoped post id: a scoped id as is, a web link from a network that
has public post URLs, or a bare id on the `fallback` network (the one `--net`, or the default).
*/
pub fn parse_target(input: &str, fallback: Option<Network>) -> Result<String, String> {
    let input = input.trim();
    if split(input).is_some() {
        return Ok(input.to_string());
    }
    if input.starts_with("http://") || input.starts_with("https://") {
        return parse_link(input);
    }
    match fallback {
        Some(net) if !input.is_empty() => Ok(scoped(net, input)),
        _ => Err(
            "which network? add --net x|bluesky|devnull|threads|mastodon, or use a full id like bluesky:at://…"
                .into(),
        ),
    }
}

fn parse_link(input: &str) -> Result<String, String> {
    let url = url::Url::parse(input).map_err(|e| format!("{input} is not a link: {e}"))?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let server = url.host_str().unwrap_or("").to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("mobile."))
        .unwrap_or(&host);
    let parts: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let unknown = || {
        format!("SoCLI cannot read {input}. use a post id, or a link from x.com, bsky.app, dev-null.sh or a Mastodon server")
    };
    match host {
        "x.com" | "twitter.com" => match parts.as_slice() {
            [_, "status", id, ..] if id.chars().all(|c| c.is_ascii_digit()) => {
                Ok(scoped(Network::X, id))
            }
            _ => Err(unknown()),
        },
        "bsky.app" => match parts.as_slice() {
            ["profile", actor, "post", rkey, ..] => Ok(scoped(
                Network::Bluesky,
                &format!("at://{actor}/app.bsky.feed.post/{rkey}"),
            )),
            _ => Err(unknown()),
        },
        "dev-null.sh" => match parts.iter().position(|p| *p == "commits") {
            Some(i) if i + 1 < parts.len() => Ok(scoped(Network::Devnull, parts[i + 1])),
            _ => Err(unknown()),
        },
        "threads.net" | "threads.com" => Err(
            "Threads post links cannot be resolved through the API; use the id from `socli --json`"
                .into(),
        ),
        _ => match parts.as_slice() {
            [user, id] if user.starts_with('@') && is_digits(id) => Ok(scoped(
                Network::Mastodon,
                &format!("https://{server}/{user}/{id}"),
            )),
            ["users", user, "statuses", id] if is_digits(id) => Ok(scoped(
                Network::Mastodon,
                &format!("https://{server}/@{user}/{id}"),
            )),
            _ => Err(unknown()),
        },
    }
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_waits_read_in_seconds_or_minutes() {
        assert_eq!(retry_text(Some(30)), "try again in 30s");
        assert_eq!(retry_text(Some(119)), "try again in 119s");
        assert_eq!(retry_text(Some(121)), "try again in 3 minutes");
        assert_eq!(retry_text(Some(900)), "try again in 15 minutes");
        assert_eq!(retry_text(None), "try again in a minute");
    }

    #[test]
    fn scoped_ids_round_trip() {
        let id = scoped(Network::Bluesky, "at://did:plc:abc/app.bsky.feed.post/3m");
        assert_eq!(id, "bluesky:at://did:plc:abc/app.bsky.feed.post/3m");
        assert_eq!(
            split(&id),
            Some((Network::Bluesky, "at://did:plc:abc/app.bsky.feed.post/3m"))
        );
        assert_eq!(split("x:1846"), Some((Network::X, "1846")));
        assert_eq!(
            split("devnull:0199-uuid"),
            Some((Network::Devnull, "0199-uuid"))
        );
        assert_eq!(split("1846"), None);
        assert_eq!(split("at://did:plc:abc"), None);
        assert_eq!(split("reddit:abc"), None);
    }

    #[test]
    fn targets_accept_ids_links_and_a_fallback_network() {
        assert_eq!(parse_target("threads:1789", None).unwrap(), "threads:1789");
        assert_eq!(
            parse_target("https://x.com/mina/status/1846213", None).unwrap(),
            "x:1846213"
        );
        assert_eq!(
            parse_target("https://twitter.com/mina/status/1846213?s=20", None).unwrap(),
            "x:1846213"
        );
        assert_eq!(
            parse_target(
                "https://bsky.app/profile/ada.bsky.social/post/3myafofo4ik2m",
                None
            )
            .unwrap(),
            "bluesky:at://ada.bsky.social/app.bsky.feed.post/3myafofo4ik2m"
        );
        assert_eq!(
            parse_target(
                "https://dev-null.sh/@nunomaduro/commits/01a14fbd-d036-7257-8f10-0c1fb436a2a0",
                None
            )
            .unwrap(),
            "devnull:01a14fbd-d036-7257-8f10-0c1fb436a2a0"
        );
        assert_eq!(
            parse_target("1846213", Some(Network::X)).unwrap(),
            "x:1846213"
        );
    }

    #[test]
    fn mastodon_links_keep_their_url_as_the_id() {
        assert_eq!(
            parse_target("https://mastodon.social/@ines/115000000000000001", None).unwrap(),
            "mastodon:https://mastodon.social/@ines/115000000000000001"
        );
        assert_eq!(
            parse_target(
                "https://mastodon.social/@theo@fosstodon.org/115000000000000002?x=1",
                None
            )
            .unwrap(),
            "mastodon:https://mastodon.social/@theo@fosstodon.org/115000000000000002"
        );
        assert_eq!(
            parse_target(
                "https://fosstodon.org/users/theo/statuses/115000000000000010",
                None
            )
            .unwrap(),
            "mastodon:https://fosstodon.org/@theo/115000000000000010"
        );
        assert_eq!(
            parse_target("mastodon:115000000000000001", None).unwrap(),
            "mastodon:115000000000000001"
        );
        assert!(parse_target("https://example.com/@ines/about", None).is_err());
        assert_eq!("masto".parse::<Network>(), Ok(Network::Mastodon));
        assert_eq!(Network::Mastodon.slug(), "mastodon");
        assert_eq!(Network::Mastodon.title(), "Mastodon");
    }

    #[test]
    fn targets_explain_what_they_cannot_resolve() {
        let threads = parse_target("https://www.threads.com/@kal/post/C9xyz", None).unwrap_err();
        assert_eq!(
            threads,
            "Threads post links cannot be resolved through the API; use the id from `socli --json`"
        );
        let bare = parse_target("1846213", None).unwrap_err();
        assert!(bare.starts_with("which network? add --net x|bluesky|devnull|threads"));
        assert!(parse_target("https://example.com/post/1", None).is_err());
    }

    #[test]
    fn net_lists_parse_slugs_and_all() {
        assert_eq!(
            parse_list("bluesky,x").unwrap(),
            vec![Network::X, Network::Bluesky]
        );
        assert_eq!(parse_list("all").unwrap(), Network::ALL.to_vec());
        assert_eq!(parse_list("threads").unwrap(), vec![Network::Threads]);
        assert_eq!(Network::Threads.hold(), Some("Waiting for Meta approval"));
        assert_eq!(Network::X.hold(), None);
        assert!(parse_list("reddit").is_err());
    }

    #[test]
    fn composer_counts_graphemes_only_where_the_network_does() {
        let family = "👩‍👩‍👧";
        assert_eq!(Network::Bluesky.caps().used(family), 1);
        assert_eq!(Network::X.caps().used(family), 5);
        assert_eq!(Network::X.caps().max_chars, 280);
        assert_eq!(Network::Bluesky.caps().max_chars, 300);
        assert_eq!(Network::Devnull.caps().max_chars, 4000);
        assert_eq!(Network::Threads.caps().max_chars, 500);
    }

    #[test]
    fn profile_urls_follow_each_network() {
        assert_eq!(Network::X.profile_url("kal"), "https://x.com/kal");
        assert_eq!(
            Network::Bluesky.profile_url("ada.bsky.social"),
            "https://bsky.app/profile/ada.bsky.social"
        );
        assert_eq!(
            Network::Devnull.profile_url("nuno"),
            "https://dev-null.sh/@nuno"
        );
        assert_eq!(
            Network::Threads.profile_url("@kal"),
            "https://www.threads.com/@kal"
        );
    }

    #[test]
    fn text_len_counts_the_way_each_network_does() {
        let family = "👩‍👩‍👧";
        assert_eq!(
            Network::X.text_len("hi https://example.com/a/long/path"),
            26
        );
        assert_eq!(Network::X.text_len(family), 2);
        assert_eq!(Network::X.text_len("𝗛i"), 3);
        assert_eq!(Network::Threads.text_len("hi ☀"), 6);
        assert_eq!(Network::Bluesky.text_len(family), 1);
        assert_eq!(Network::Devnull.text_len("héllo"), 5);
    }

    #[test]
    fn errors_read_as_next_steps() {
        assert_eq!(
            NetError::SignInRequired(Network::Bluesky).to_string(),
            "sign in first: `socli login bluesky`"
        );
        assert_eq!(
            NetError::unsupported(Network::Threads, "likes").to_string(),
            "Threads has no likes in its API"
        );
        assert_eq!(Network::Devnull.caps().liked_word(true), "starred");
        assert_eq!(Network::X.caps().liked_word(false), "unliked");
    }
    #[test]
    fn default_feeds_prefer_discovery_and_fall_back_only_when_unsupported() {
        assert_eq!(
            Network::X.caps().default_feed(Network::X),
            Some(FeedKind::ForYou)
        );
        assert_eq!(
            Network::Bluesky.caps().default_feed(Network::Bluesky),
            Some(FeedKind::Discover)
        );
        assert_eq!(
            Network::Devnull.caps().default_feed(Network::Devnull),
            Some(FeedKind::Trending)
        );
        assert_eq!(
            Network::Mastodon.caps().default_feed(Network::Mastodon),
            Some(FeedKind::Trending)
        );
        assert_eq!(
            Network::Threads.caps().default_feed(Network::Threads),
            Some(FeedKind::Mine)
        );
    }
}
