/*!
The config file, one section per network plus `[feed]`, `[ui]` and `[colors]`. It lives at
`$SOCLI_CONFIG_DIR/config.toml`, `$XDG_CONFIG_HOME/socli/config.toml` or
`~/.config/socli/config.toml`, with mode 0600 in a 0700 directory. Every write replaces the file
atomically under a lock on `config.toml.lock`. `socli config` reads and writes the keys listed in
`KEYS`.
*/

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::media::ImageMode;
use crate::net::Network;
use crate::theme::{
    self, choice, Choice, ColorBy, ColorMode, Density, Detail, IconMode, Layout, Replies, Scheme,
    Switch,
};

choice!(Enter { Post => "post", Newline => "newline" });

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_network: Option<String>,
    /// Read from files written before `[ui] images`, then moved there on load.
    #[serde(skip_serializing)]
    pub image_viewer: Option<String>,
    #[serde(skip_serializing_if = "FeedConfig::is_empty")]
    pub feed: FeedConfig,
    #[serde(skip_serializing_if = "UiConfig::is_empty")]
    pub ui: UiConfig,
    /// `[colors]`: palette overrides, a name and a `#rrggbb` value.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "XConfig::is_empty")]
    pub x: XConfig,
    #[serde(skip_serializing_if = "BlueskyConfig::is_empty")]
    pub bluesky: BlueskyConfig,
    #[serde(skip_serializing_if = "DevnullConfig::is_empty")]
    pub devnull: DevnullConfig,
    #[serde(skip_serializing_if = "ThreadsConfig::is_empty")]
    pub threads: ThreadsConfig,
    #[serde(skip_serializing_if = "MastodonConfig::is_empty")]
    pub mastodon: MastodonConfig,
    /**
    `color` in each network's section, by slug. It is kept apart from the section structs so a
    sign-in or sign-out that replaces a whole section keeps the color.
    */
    #[serde(skip)]
    pub net_colors: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FeedConfig {
    /// The networks the feed starts with: slugs, or `all`.
    pub networks: Option<Vec<String>>,
    pub color_by: Option<String>,
    pub replies: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub theme: Option<String>,
    pub colors: Option<String>,
    pub density: Option<String>,
    pub icons: Option<String>,
    pub images: Option<String>,
    pub detail: Option<String>,
    pub layout: Option<String>,
    pub unfurl: Option<String>,
    pub mouse: Option<String>,
    pub enter: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct XConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub bearer_token: Option<String>,
    pub latest_query: Option<String>,
    /// `auth_token` cookie from a signed-in browser.
    pub auth_token: Option<String>,
    /// `ct0` cookie paired with `auth_token`.
    pub ct0: Option<String>,
    /// PKCE values for a sign-in that left this page. Cleared when it finishes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_verifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_redirect: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlueskyConfig {
    pub service: Option<String>,
    pub handle: Option<String>,
    pub did: Option<String>,
    pub access_jwt: Option<String>,
    pub refresh_jwt: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DevnullConfig {
    pub username: Option<String>,
    /// File name of the OpenSSH private key inside the config directory.
    pub device_key: Option<String>,
    /// The page has no key file, so it keeps the OpenSSH private key here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_secret: Option<String>,
    pub device_approved: bool,
    pub ssh_key: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadsConfig {
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub access_token: Option<String>,
    pub issued_at: Option<i64>,
    pub expires_at: Option<i64>,
    /// PKCE-less OAuth state for a sign-in that left this page. Cleared when it finishes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_redirect: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MastodonConfig {
    /// The server's host name, such as `mastodon.social`.
    pub server: Option<String>,
    /// The app SoCLI registered on that server, reused by the next sign-in.
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub access_token: Option<String>,
    pub user_id: Option<String>,
    /// The full handle, `user@server`.
    pub username: Option<String>,
    /// The server's post length limit, read at sign-in.
    pub max_chars: Option<usize>,
    /// PKCE values for a sign-in that left this page. Cleared when it finishes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_verifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_redirect: Option<String>,
}

impl Config {
    pub fn net_color(&self, net: Network) -> Option<&str> {
        self.net_colors.get(net.slug()).map(String::as_str)
    }

    /// Moves a legacy `image_viewer` into `[ui] images` when that is not set yet.
    fn migrate(&mut self) {
        if let Some(old) = self.image_viewer.take() {
            if self.ui.images.is_none() {
                let mode = ImageMode::from_setting(&old).unwrap_or(ImageMode::Auto);
                self.ui.images = Some(mode.word().to_string());
            }
        }
    }
}

/// Only the `color` of each network section, read beside the full config.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ColorsFile {
    x: ColorOnly,
    bluesky: ColorOnly,
    devnull: ColorOnly,
    threads: ColorOnly,
    mastodon: ColorOnly,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ColorOnly {
    color: Option<String>,
}

impl ColorsFile {
    fn into_map(self) -> BTreeMap<String, String> {
        [
            (Network::X, self.x),
            (Network::Bluesky, self.bluesky),
            (Network::Devnull, self.devnull),
            (Network::Threads, self.threads),
            (Network::Mastodon, self.mastodon),
        ]
        .into_iter()
        .filter_map(|(net, section)| section.color.map(|c| (net.slug().to_string(), c)))
        .collect()
    }
}

/// What a write puts on disk: the config with each network's `color` back inside its section.
#[derive(Serialize)]
struct FileOut<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    default_network: &'a Option<String>,
    #[serde(skip_serializing_if = "FeedConfig::is_empty")]
    feed: &'a FeedConfig,
    #[serde(skip_serializing_if = "UiConfig::is_empty")]
    ui: &'a UiConfig,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    colors: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Section::is_empty")]
    x: Section<'a, XConfig>,
    #[serde(skip_serializing_if = "Section::is_empty")]
    bluesky: Section<'a, BlueskyConfig>,
    #[serde(skip_serializing_if = "Section::is_empty")]
    devnull: Section<'a, DevnullConfig>,
    #[serde(skip_serializing_if = "Section::is_empty")]
    threads: Section<'a, ThreadsConfig>,
    #[serde(skip_serializing_if = "Section::is_empty")]
    mastodon: Section<'a, MastodonConfig>,
}

#[derive(Serialize)]
struct Section<'a, T: Serialize> {
    #[serde(flatten)]
    fields: &'a T,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<&'a str>,
    #[serde(skip)]
    empty: bool,
}

impl<T: Serialize> Section<'_, T> {
    fn is_empty(&self) -> bool {
        self.empty && self.color.is_none()
    }
}

impl<'a> FileOut<'a> {
    fn new(config: &'a Config) -> Self {
        let color = |net: Network| config.net_color(net);
        FileOut {
            default_network: &config.default_network,
            feed: &config.feed,
            ui: &config.ui,
            colors: &config.colors,
            x: Section {
                fields: &config.x,
                color: color(Network::X),
                empty: config.x.is_empty(),
            },
            bluesky: Section {
                fields: &config.bluesky,
                color: color(Network::Bluesky),
                empty: config.bluesky.is_empty(),
            },
            devnull: Section {
                fields: &config.devnull,
                color: color(Network::Devnull),
                empty: config.devnull.is_empty(),
            },
            threads: Section {
                fields: &config.threads,
                color: color(Network::Threads),
                empty: config.threads.is_empty(),
            },
            mastodon: Section {
                fields: &config.mastodon,
                color: color(Network::Mastodon),
                empty: config.mastodon.is_empty(),
            },
        }
    }
}

macro_rules! empty_when_default {
    ($($section:ty),*) => {
        $(impl $section {
            fn is_empty(&self) -> bool {
                *self == Self::default()
            }
        })*
    };
}

empty_when_default!(
    FeedConfig,
    UiConfig,
    XConfig,
    BlueskyConfig,
    DevnullConfig,
    ThreadsConfig,
    MastodonConfig
);

/// The flat file xcli wrote before the networks had sections.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyConfig {
    client_id: Option<String>,
    client_secret: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    user_id: Option<String>,
    username: Option<String>,
    bearer_token: Option<String>,
    latest_query: Option<String>,
    image_viewer: Option<String>,
}

impl From<LegacyConfig> for Config {
    fn from(old: LegacyConfig) -> Self {
        Config {
            image_viewer: old.image_viewer,
            x: XConfig {
                client_id: old.client_id,
                client_secret: old.client_secret,
                access_token: old.access_token,
                refresh_token: old.refresh_token,
                expires_at: old.expires_at,
                user_id: old.user_id,
                username: old.username,
                bearer_token: old.bearer_token,
                latest_query: old.latest_query,
                auth_token: None,
                ct0: None,
                oauth_state: None,
                oauth_verifier: None,
                oauth_redirect: None,
            },
            ..Config::default()
        }
    }
}

/// Where the config lives, and the pre-rename file to migrate from when it does not exist yet.
struct Paths {
    file: PathBuf,
    #[cfg(any(test, not(target_arch = "wasm32")))]
    legacy: Option<PathBuf>,
}

fn paths(env: impl Fn(&str) -> Option<OsString>) -> Paths {
    let env = |key: &str| env(key).filter(|value| !value.is_empty());
    if let Some(dir) = env("SOCLI_CONFIG_DIR") {
        return Paths {
            file: PathBuf::from(dir).join("config.toml"),
            #[cfg(any(test, not(target_arch = "wasm32")))]
            legacy: None,
        };
    }
    let base = env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".config")));
    match base {
        Some(base) => Paths {
            file: base.join("socli").join("config.toml"),
            #[cfg(any(test, not(target_arch = "wasm32")))]
            legacy: Some(base.join("xcli").join("config.toml")),
        },
        None => Paths {
            file: PathBuf::from("socli-config.toml"),
            #[cfg(any(test, not(target_arch = "wasm32")))]
            legacy: None,
        },
    }
}

fn process_paths() -> Paths {
    // Login tests retarget `SOCLI_CONFIG_DIR` for this whole process. Hold that override still
    // until the read has copied the path.
    #[cfg(test)]
    let _guard = lock_config_env();
    paths(|key| std::env::var_os(key))
}

/// `SOCLI_CONFIG_DIR` as a test sees it, waiting out any login test that is borrowing the variable.
#[cfg(test)]
pub(crate) fn config_dir_var() -> Option<OsString> {
    let _guard = lock_config_env();
    std::env::var_os("SOCLI_CONFIG_DIR")
}

/**
Unit tests share one process. A test that points `SOCLI_CONFIG_DIR` at a temporary directory holds
this for the whole override, and every read of the variable takes it too. Re-entrant on the thread
that already holds it, so a config write inside that test does not deadlock.
*/
#[cfg(test)]
pub(crate) fn lock_config_env() -> ConfigEnvGuard {
    ConfigEnvGuard::acquire()
}

#[cfg(test)]
mod config_env {
    use std::cell::Cell;
    use std::sync::{Mutex, MutexGuard};

    pub(super) static LOCK: Mutex<()> = Mutex::new(());
    thread_local! {
        pub(super) static HELD: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) type HeldGuard = MutexGuard<'static, ()>;
}

#[cfg(test)]
pub(crate) struct ConfigEnvGuard {
    locked: Option<config_env::HeldGuard>,
}

#[cfg(test)]
impl ConfigEnvGuard {
    fn acquire() -> Self {
        if config_env::HELD.with(std::cell::Cell::get) {
            return Self { locked: None };
        }
        let locked = config_env::LOCK
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        config_env::HELD.with(|held| held.set(true));
        Self {
            locked: Some(locked),
        }
    }
}

#[cfg(test)]
impl Drop for ConfigEnvGuard {
    fn drop(&mut self) {
        if self.locked.take().is_some() {
            config_env::HELD.with(|held| held.set(false));
        }
    }
}

pub fn path() -> PathBuf {
    process_paths().file
}

/// The directory holding `config.toml`, where key files live too.
pub fn dir() -> PathBuf {
    let file = path();
    file.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Reads the file, migrating the old xcli file on the first run. A missing file is an empty config.
///
/// The browser build reads the text the page kept for this origin. It does not migrate an old file.
pub fn load() -> Result<Config, String> {
    #[cfg(target_arch = "wasm32")]
    {
        load_browser()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        load_at(&process_paths())
    }
}

/// Loads, applies `change`, and writes the result back atomically, all under the file's lock.
///
/// The browser build writes the same TOML back to the page.
pub fn update(change: impl FnOnce(&mut Config)) -> Result<Config, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let mut config = load_browser()?;
        change(&mut config);
        store_browser(&config)?;
        Ok(config)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        update_at(&process_paths(), change)
    }
}

#[cfg(any(test, not(target_arch = "wasm32")))]
fn update_at(paths: &Paths, change: impl FnOnce(&mut Config)) -> Result<Config, String> {
    locked(&paths.file, || {
        let mut config = load_at(paths)?;
        change(&mut config);
        write(&paths.file, &config)?;
        Ok(config)
    })
}

// MARK: socli config

/// The keys `socli config` reads and writes, in `list` order.
pub fn keys() -> Vec<String> {
    let mut keys: Vec<String> = [
        "feed.networks",
        "feed.color_by",
        "feed.replies",
        "default_network",
        "x.color",
        "bluesky.color",
        "devnull.color",
        "threads.color",
        "mastodon.color",
        "ui.detail",
        "ui.layout",
        "ui.theme",
        "ui.colors",
        "ui.density",
        "ui.icons",
        "ui.images",
        "ui.unfurl",
        "ui.mouse",
        "ui.enter",
    ]
    .map(String::from)
    .to_vec();
    keys.extend(theme::PALETTE_KEYS.map(|name| format!("colors.{name}")));
    keys
}

fn unknown_key(key: &str) -> String {
    format!("unknown key \"{key}\". Run `socli config list` to see the keys")
}

fn bad_value(value: &str, key: &str, values: &str) -> String {
    format!("\"{value}\" is not a valid {key}. Use one of: {values}")
}

/// The network whose section a `<slug>.color` key names.
fn color_key(key: &str) -> Option<Network> {
    let slug = key.strip_suffix(".color")?;
    Network::ALL.into_iter().find(|net| net.slug() == slug)
}

/// The palette entry a `colors.<name>` key names.
fn palette_key(key: &str) -> Option<&str> {
    key.strip_prefix("colors.")
        .filter(|name| theme::PALETTE_KEYS.contains(name))
}

/// The value the file stores for `key`, or None when it leaves the key unset.
pub fn get(config: &Config, key: &str) -> Result<Option<String>, String> {
    Ok(match key {
        "feed.networks" => config.feed.networks.as_ref().map(|list| list.join(",")),
        "feed.color_by" => config.feed.color_by.clone(),
        "feed.replies" => config.feed.replies.clone(),
        "default_network" => config.default_network.clone(),
        "ui.detail" => config.ui.detail.clone(),
        "ui.layout" => config.ui.layout.clone(),
        "ui.theme" => config.ui.theme.clone(),
        "ui.colors" => config.ui.colors.clone(),
        "ui.density" => config.ui.density.clone(),
        "ui.icons" => config.ui.icons.clone(),
        "ui.images" => config.ui.images.clone(),
        "ui.unfurl" => config.ui.unfurl.clone(),
        "ui.mouse" => config.ui.mouse.clone(),
        "ui.enter" => config.ui.enter.clone(),
        _ => match (color_key(key), palette_key(key)) {
            (Some(net), _) => config.net_color(net).map(str::to_string),
            (_, Some(name)) => config.colors.get(name).cloned(),
            _ => return Err(unknown_key(key)),
        },
    })
}

/// What an unset key means. `colors.<name>` depends on the theme, so it has none here.
pub fn default_of(key: &str) -> Option<String> {
    let value = match key {
        "feed.networks" => "all",
        "feed.color_by" => ColorBy::Auto.word(),
        "feed.replies" => Replies::Oldest.word(),
        "default_network" => "none",
        "ui.detail" => theme::default_detail(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .word(),
        "ui.layout" => Layout::Auto.word(),
        "ui.theme" => "auto",
        "ui.colors" => ColorMode::Full.word(),
        "ui.density" => Density::Cozy.word(),
        "ui.icons" => IconMode::Auto.word(),
        "ui.images" => ImageMode::Auto.word(),
        "ui.unfurl" => Switch::On.word(),
        "ui.mouse" => Switch::On.word(),
        "ui.enter" => Enter::Post.word(),
        _ => return color_key(key).map(|net| theme::default_net_color(net).to_string()),
    };
    Some(value.to_string())
}

fn choice<T: Choice>(value: &str, key: &str) -> Result<String, String> {
    T::parse(value)
        .map(|c| c.word().to_string())
        .ok_or_else(|| bad_value(value, key, &T::words()))
}

/// Validates `value` for `key` and stores it. Returns the value as saved.
pub fn set(config: &mut Config, key: &str, value: &str) -> Result<String, String> {
    let nets = "all, x, bluesky, devnull, threads, mastodon";
    let saved = match key {
        "feed.networks" => {
            let list = if value.trim().eq_ignore_ascii_case("all") {
                vec!["all".to_string()]
            } else {
                let parsed =
                    crate::net::parse_list(value).map_err(|_| bad_value(value, key, nets))?;
                if let Some(message) = parsed.iter().find_map(|net| net.hold()) {
                    return Err(message.into());
                }
                parsed.iter().map(|net| net.slug().to_string()).collect()
            };
            let saved = list.join(",");
            config.feed.networks = Some(list);
            return Ok(saved);
        }
        "feed.color_by" => choice::<ColorBy>(value, key)?,
        "feed.replies" => choice::<Replies>(value.trim().trim_end_matches(" first"), key)
            .map_err(|_| bad_value(value, key, &Replies::words()))?,
        "default_network" => {
            if value.trim().eq_ignore_ascii_case("none") {
                config.default_network = None;
                return Ok("none".into());
            }
            let net: Network = value.parse().map_err(|_| {
                bad_value(value, key, "none, x, bluesky, devnull, threads, mastodon")
            })?;
            if let Some(message) = net.hold() {
                return Err(message.into());
            }
            net.slug().to_string()
        }
        "ui.detail" => choice::<Detail>(value, key)?,
        "ui.layout" => choice::<Layout>(value, key)?,
        "ui.theme" => Scheme::parse(value)
            .map(|scheme| scheme.word())
            .ok_or_else(|| bad_value(value, key, &Scheme::words()))?,
        "ui.colors" => choice::<ColorMode>(value, key)?,
        "ui.density" => choice::<Density>(value, key)?,
        "ui.icons" => choice::<IconMode>(value, key)?,
        "ui.images" => choice::<ImageMode>(value, key)?,
        "ui.unfurl" | "ui.mouse" => choice::<Switch>(value, key)?,
        "ui.enter" => choice::<Enter>(value, key)?,
        _ => match (color_key(key), palette_key(key)) {
            (Some(net), _) => {
                let names = format!("{}, #rrggbb", theme::COLOR_NAMES.join(", "));
                let color =
                    theme::valid_color(value).ok_or_else(|| bad_value(value, key, &names))?;
                config
                    .net_colors
                    .insert(net.slug().to_string(), color.clone());
                return Ok(color);
            }
            (_, Some(name)) => {
                let rgb = theme::Rgb::parse(value.trim())
                    .ok_or_else(|| bad_value(value, key, "#rrggbb"))?;
                config.colors.insert(name.to_string(), rgb.hex());
                return Ok(rgb.hex());
            }
            _ => return Err(unknown_key(key)),
        },
    };
    let slot = match key {
        "feed.color_by" => &mut config.feed.color_by,
        "feed.replies" => &mut config.feed.replies,
        "default_network" => &mut config.default_network,
        "ui.detail" => &mut config.ui.detail,
        "ui.layout" => &mut config.ui.layout,
        "ui.theme" => &mut config.ui.theme,
        "ui.colors" => &mut config.ui.colors,
        "ui.density" => &mut config.ui.density,
        "ui.icons" => &mut config.ui.icons,
        "ui.unfurl" => &mut config.ui.unfurl,
        "ui.mouse" => &mut config.ui.mouse,
        "ui.enter" => &mut config.ui.enter,
        _ => &mut config.ui.images,
    };
    *slot = Some(saved.clone());
    Ok(saved)
}

/// `missing`, `set`, or `set (…abcd)` when the secret is long enough to show its tail.
pub fn redact(secret: Option<&str>) -> String {
    match secret.filter(|s| !s.is_empty()) {
        None => "missing".into(),
        Some(secret) if secret.chars().count() <= 6 => "set".into(),
        Some(secret) => {
            let tail: String = secret
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("set (…{tail})")
        }
    }
}

#[cfg(any(test, not(target_arch = "wasm32")))]
fn load_at(paths: &Paths) -> Result<Config, String> {
    if paths.file.exists() {
        let text = fs::read_to_string(&paths.file)
            .map_err(|e| format!("reading {}: {e}", paths.file.display()))?;
        let invalid = |e: toml::de::Error| {
            format!(
                "{} is not valid toml{}; check its syntax and field types",
                paths.file.display(),
                error_location(&e, &text)
            )
        };
        let mut config: Config = toml::from_str(&text).map_err(invalid)?;
        config.net_colors = toml::from_str::<ColorsFile>(&text)
            .map_err(invalid)?
            .into_map();
        config.migrate();
        return Ok(config);
    }
    match paths.legacy.as_deref().filter(|old| old.is_file()) {
        Some(old) => {
            let text =
                fs::read_to_string(old).map_err(|e| format!("reading {}: {e}", old.display()))?;
            let legacy: LegacyConfig = toml::from_str(&text).map_err(|e| {
                format!(
                    "{} is not valid toml{}, so it was not migrated; check its syntax and field types",
                    old.display(), error_location(&e, &text)
                )
            })?;
            let mut config = Config::from(legacy);
            config.migrate();
            write(&paths.file, &config)?;
            Ok(config)
        }
        None => Ok(Config::default()),
    }
}

/** Parser diagnostics can contain entire credential values, even without a source excerpt. */
fn error_location(error: &toml::de::Error, text: &str) -> String {
    let Some(span) = error.span() else {
        return String::new();
    };
    let before = &text.as_bytes()[..span.start.min(text.len())];
    let line = before.iter().filter(|byte| **byte == b'\n').count() + 1;
    let column = before
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(before.len() + 1, |newline| before.len() - newline);
    format!(" at line {line}, column {column}")
}

#[cfg(any(test, not(target_arch = "wasm32")))]
fn write(path: &Path, config: &Config) -> Result<(), String> {
    let text = toml::to_string_pretty(&FileOut::new(config)).map_err(|e| e.to_string())?;
    write_atomic(path, &text)
}

/**
Runs `f` while this process holds the exclusive lock on `<target>.lock`, so a read, change and write
of `target` is one transaction across socli processes. The lock goes with the process, so a crash
leaves no stale lock behind, and the lock file stays in place for the next writer.
*/
pub fn locked<T>(target: &Path, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let mut name = target.as_os_str().to_owned();
    name.push(".lock");
    let path = PathBuf::from(name);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let mut dir = fs::DirBuilder::new();
        dir.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut dir, 0o700);
        dir.create(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options
        .open(&path)
        .map_err(|e| format!("opening {}: {e}", path.display()))?;
    file.lock()
        .map_err(|e| format!("locking {}: {e}", path.display()))?;
    f()
}

#[cfg(target_arch = "wasm32")]
fn load_browser() -> Result<Config, String> {
    let text = read_browser()?;
    if text.trim().is_empty() {
        return Ok(Config::default());
    }
    let invalid = |err: toml::de::Error| {
        format!(
            "the saved config is not valid toml{}; check its syntax and field types",
            error_location(&err, &text)
        )
    };
    let mut config: Config = toml::from_str(&text).map_err(invalid)?;
    config.net_colors = toml::from_str::<ColorsFile>(&text)
        .map_err(invalid)?
        .into_map();
    config.migrate();
    Ok(config)
}

#[cfg(target_arch = "wasm32")]
fn store_browser(config: &Config) -> Result<(), String> {
    let text = toml::to_string_pretty(&FileOut::new(config)).map_err(|err| err.to_string())?;
    let bytes = text.as_bytes();
    // SAFETY: `bytes` is the live UTF-8 of this `text`; `socli_config_write` is the `env` import `(*const u8, usize)`, and the worker only reads that slice before returning.
    unsafe {
        socli_config_write(bytes.as_ptr(), bytes.len());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn read_browser() -> Result<String, String> {
    const MAX: usize = 1024 * 1024;
    let mut cap = 4096usize;
    loop {
        let mut buf = vec![0u8; cap];
        let wrote =
            // SAFETY: `buf` is this function's zeroed `Vec<u8>` of length `cap`; `socli_config_read` is the `env` import `(*mut u8, usize) -> usize`, and the worker writes at most `cap` bytes or returns `usize::MAX` and writes nothing.
            unsafe { socli_config_read(buf.as_mut_ptr(), buf.len()) };
        if wrote == usize::MAX {
            if cap >= MAX {
                return Err("the saved config is larger than 1 MB".into());
            }
            cap = cap.saturating_mul(2).min(MAX);
            continue;
        }
        if wrote > buf.len() {
            return Err("the saved config could not be read".into());
        }
        buf.truncate(wrote);
        return String::from_utf8(buf).map_err(|_| "the saved config is not utf-8".into());
    }
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
unsafe extern "C" {
    fn socli_config_read(buf: *mut u8, cap: usize) -> usize;
    fn socli_config_write(ptr: *const u8, len: usize);
}

/// A suffix for a temporary file. Native draws it from the rng. The browser demo never writes one.
fn temp_nonce() -> u32 {
    #[cfg(feature = "native")]
    {
        rand::random::<u32>()
    }
    #[cfg(not(feature = "native"))]
    {
        use std::cell::Cell;
        thread_local! {
            static NEXT: Cell<u32> = const { Cell::new(1) };
        }
        NEXT.with(|next| {
            let value = next.get();
            next.set(value.wrapping_add(1));
            value
        })
    }
}

/**
Replaces `path` with `text` through a temporary file of its own in the same directory, created
exclusively with mode 0600 and synced before the rename, in a 0700 directory.
*/
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    let name = path
        .file_name()
        .map_or_else(|| "socli".into(), |name| name.to_string_lossy());
    let tmp = path.with_file_name(format!(
        "{name}.{}.{:08x}.tmp",
        std::process::id(),
        temp_nonce()
    ));
    write_private(&tmp, text).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("writing {}: {e}", path.display()));
    }
    #[cfg(unix)]
    if let Some(parent) = parent {
        // The file is already replaced; a file system that cannot fsync a directory leaves it so.
        let _ = fs::File::open(parent).and_then(|dir| dir.sync_all());
    }
    Ok(())
}

/// Creates `path`, which must not exist yet, and removes it again when the write fails.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    let written = fill(&mut file, text);
    if written.is_err() {
        let _ = fs::remove_file(path);
    }
    written
}

fn fill(file: &mut fs::File, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("socli-config-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn invalid_configuration_reports_location_without_secret_values() {
        let dir = temp("private-errors");
        let paths = Paths {
            file: dir.join("config.toml"),
            legacy: None,
        };
        for text in [
            "[x]\naccess_token = \"private-token\" broken\n",
            "[x]\naccess_token = [\"private-token\"]\n",
            "[x]\ncolor = [\"private-token\"]\n",
        ] {
            fs::write(&paths.file, text).unwrap();
            let error = load_at(&paths).unwrap_err();
            assert!(error.contains("at line 2, column"), "{error}");
            assert!(!error.contains("private-token"), "{error}");
            assert_eq!(fs::read_to_string(&paths.file).unwrap(), text);
        }
        let legacy = dir.join("legacy.toml");
        fs::write(&legacy, "access_token = [\"private-token\"]\n").unwrap();
        let paths = Paths {
            file: dir.join("unmigrated.toml"),
            legacy: Some(legacy),
        };
        let error = load_at(&paths).unwrap_err();
        assert!(error.contains("not migrated"), "{error}");
        assert!(!error.contains("private-token"), "{error}");
        assert!(!paths.file.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn migrates_the_flat_xcli_file_into_an_x_section() {
        let xdg = temp("migrate");
        let old = xdg.join("xcli").join("config.toml");
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        let before = "access_token = \"tok-123456\"\nusername = \"mina\"\nimage_viewer = \"viu\"\n";
        fs::write(&old, before).unwrap();
        let home = xdg.join("home");
        let paths = paths(|key| match key {
            "XDG_CONFIG_HOME" => Some(xdg.clone().into_os_string()),
            "HOME" => Some(home.clone().into_os_string()),
            _ => None,
        });
        assert_eq!(paths.file, xdg.join("socli").join("config.toml"));

        let config = load_at(&paths).unwrap();
        assert_eq!(config.x.access_token.as_deref(), Some("tok-123456"));
        assert_eq!(config.x.username.as_deref(), Some("mina"));
        assert_eq!(config.image_viewer, None);
        assert_eq!(config.ui.images.as_deref(), Some("auto"));

        let written = fs::read_to_string(&paths.file).unwrap();
        assert!(written.contains("[x]"), "{written}");
        assert!(written.contains("[ui]\nimages = \"auto\""), "{written}");
        assert!(!written.contains("image_viewer"), "{written}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&paths.file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert_eq!(fs::read_to_string(&old).unwrap(), before);
        assert!(!home.exists());
        let _ = fs::remove_dir_all(&xdg);
    }

    #[test]
    fn an_explicit_dir_never_migrates() {
        let dir = temp("explicit");
        let paths = paths(|key| (key == "SOCLI_CONFIG_DIR").then(|| dir.clone().into_os_string()));
        assert!(paths.legacy.is_none());
        assert_eq!(load_at(&paths).unwrap(), Config::default());
        assert!(!paths.file.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn secrets_are_redacted() {
        assert_eq!(redact(None), "missing");
        assert_eq!(redact(Some("abc")), "set");
        assert_eq!(redact(Some("token-123456")), "set (…3456)");
    }

    #[test]
    fn sections_round_trip_and_unknown_keys_are_ignored() {
        let text = "default_network = \"bluesky\"\nsurprise = 1\n[feed]\nnetworks = [\"x\", \"bluesky\"]\n[bluesky]\nhandle = \"ada.bsky.social\"\n[devnull]\ndevice_approved = true\n";
        let config: Config = toml::from_str(text).unwrap();
        assert_eq!(config.default_network.as_deref(), Some("bluesky"));
        assert_eq!(
            config.feed.networks,
            Some(vec!["x".to_string(), "bluesky".to_string()])
        );
        assert_eq!(config.bluesky.handle.as_deref(), Some("ada.bsky.social"));
        assert!(config.devnull.device_approved);
        let out = toml::to_string_pretty(&config).unwrap();
        assert!(!out.contains("[x]") && !out.contains("[threads]"), "{out}");
        assert_eq!(toml::from_str::<Config>(&out).unwrap(), config);
    }

    #[test]
    fn keys_validate_and_store_their_values() {
        let mut config = Config::default();
        assert_eq!(set(&mut config, "ui.theme", "Light").unwrap(), "white");
        assert_eq!(get(&config, "ui.theme").unwrap().as_deref(), Some("white"));
        assert_eq!(
            set(&mut config, "ui.theme", "Nord-Dark").unwrap(),
            "nord-dark"
        );
        let err = set(&mut config, "ui.theme", "nope").unwrap_err();
        assert!(err.contains("native, black, white, aura"), "{err}");
        assert_eq!(get(&config, "ui.density").unwrap(), None);
        assert_eq!(default_of("ui.density").as_deref(), Some("cozy"));
        assert_eq!(
            set(&mut config, "feed.networks", "bluesky, x").unwrap(),
            "x,bluesky"
        );
        assert_eq!(
            set(&mut config, "feed.replies", "newest first").unwrap(),
            "newest"
        );
        assert_eq!(set(&mut config, "default_network", "none").unwrap(), "none");
        assert_eq!(
            set(&mut config, "feed.networks", "threads").unwrap_err(),
            "Waiting for Meta approval"
        );
        assert_eq!(
            set(&mut config, "feed.networks", "x,threads").unwrap_err(),
            "Waiting for Meta approval"
        );
        assert_eq!(
            set(&mut config, "default_network", "threads").unwrap_err(),
            "Waiting for Meta approval"
        );
        assert_eq!(set(&mut config, "feed.networks", "all").unwrap(), "all");
        assert_eq!(
            set(&mut config, "bluesky.color", "#AABBCC").unwrap(),
            "#aabbcc"
        );
        assert_eq!(config.net_color(Network::Bluesky), Some("#aabbcc"));
        assert_eq!(default_of("threads.color").as_deref(), Some("violet"));
        assert_eq!(
            set(&mut config, "colors.blue", "#102030").unwrap(),
            "#102030"
        );
        assert_eq!(default_of("colors.blue"), None);
        assert_eq!(
            set(&mut config, "ui.density", "huge").unwrap_err(),
            "\"huge\" is not a valid ui.density. Use one of: compact, cozy, roomy"
        );
        assert!(set(&mut config, "x.color", "purple")
            .unwrap_err()
            .starts_with("\"purple\" is not a valid x.color. Use one of: default, blue"));
        assert_eq!(
            get(&config, "ui.nope").unwrap_err(),
            "unknown key \"ui.nope\". Run `socli config list` to see the keys"
        );
        assert_eq!(keys().len(), 19 + 17);
    }

    #[test]
    fn detail_is_a_ui_key() {
        let mut config = Config::default();
        assert_eq!(
            default_of("ui.detail").as_deref(),
            Some(
                theme::default_detail(|key| {
                    std::env::var(key)
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                })
                .word()
            )
        );
        assert_eq!(set(&mut config, "ui.detail", "standard").unwrap(), "medium");
        assert_eq!(
            get(&config, "ui.detail").unwrap().as_deref(),
            Some("medium")
        );
        assert_eq!(set(&mut config, "ui.detail", "Rich").unwrap(), "rich");
        assert_eq!(get(&config, "ui.detail").unwrap().as_deref(), Some("rich"));
        assert_eq!(
            set(&mut config, "ui.detail", "huge").unwrap_err(),
            "\"huge\" is not a valid ui.detail. Use one of: plain, minimal, medium, rich"
        );
    }

    #[test]
    fn unfurl_is_a_ui_key() {
        let mut config = Config::default();
        assert_eq!(default_of("ui.unfurl").as_deref(), Some("on"));
        assert_eq!(set(&mut config, "ui.unfurl", "OFF").unwrap(), "off");
        assert_eq!(get(&config, "ui.unfurl").unwrap().as_deref(), Some("off"));
        assert_eq!(
            set(&mut config, "ui.unfurl", "maybe").unwrap_err(),
            "\"maybe\" is not a valid ui.unfurl. Use one of: on, off"
        );
    }

    #[test]
    fn mouse_is_a_ui_key() {
        let mut config = Config::default();
        assert_eq!(default_of("ui.mouse").as_deref(), Some("on"));
        assert_eq!(set(&mut config, "ui.mouse", "off").unwrap(), "off");
        assert_eq!(get(&config, "ui.mouse").unwrap().as_deref(), Some("off"));
        assert_eq!(
            set(&mut config, "ui.mouse", "maybe").unwrap_err(),
            "\"maybe\" is not a valid ui.mouse. Use one of: on, off"
        );
    }

    #[test]
    fn enter_is_a_ui_key() {
        let mut config = Config::default();
        assert_eq!(default_of("ui.enter").as_deref(), Some("post"));
        assert_eq!(set(&mut config, "ui.enter", "Newline").unwrap(), "newline");
        assert_eq!(
            get(&config, "ui.enter").unwrap().as_deref(),
            Some("newline")
        );
        assert_eq!(
            set(&mut config, "ui.enter", "maybe").unwrap_err(),
            "\"maybe\" is not a valid ui.enter. Use one of: post, newline"
        );
    }

    #[test]
    fn concurrent_locked_updates_keep_every_key() {
        let dir = temp("locked");
        let paths = paths(|key| (key == "SOCLI_CONFIG_DIR").then(|| dir.clone().into_os_string()));
        std::thread::scope(|scope| {
            for thread in 0..8 {
                let paths = &paths;
                scope.spawn(move || {
                    for n in 0..25 {
                        update_at(paths, |cfg| {
                            cfg.colors
                                .insert(format!("k{thread}-{n}"), "#102030".into());
                        })
                        .unwrap();
                    }
                });
            }
        });
        let config = load_at(&paths).unwrap();
        assert_eq!(config.colors.len(), 200);
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !names.iter().any(|name| name.ends_with(".tmp")),
            "{names:?}"
        );
        assert!(names.contains(&"config.toml.lock".to_string()), "{names:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in ["config.toml", "config.toml.lock"] {
                let mode = fs::metadata(dir.join(file)).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600, "{file}");
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_rename_leaves_no_temporary_file() {
        let dir = temp("rename");
        let target = dir.join("taken");
        fs::create_dir_all(target.join("inside")).unwrap();
        assert!(write_atomic(&target, "text").is_err());
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["taken"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn layout_is_a_ui_key() {
        let mut config = Config::default();
        assert_eq!(default_of("ui.layout").as_deref(), Some("auto"));
        assert_eq!(set(&mut config, "ui.layout", "Split").unwrap(), "split");
        assert_eq!(get(&config, "ui.layout").unwrap().as_deref(), Some("split"));
        assert_eq!(
            set(&mut config, "ui.layout", "wide").unwrap_err(),
            "\"wide\" is not a valid ui.layout. Use one of: auto, single, split"
        );
    }

    #[test]
    fn network_colors_live_inside_their_sections() {
        let dir = temp("colors");
        let paths = paths(|key| (key == "SOCLI_CONFIG_DIR").then(|| dir.clone().into_os_string()));
        let mut config = Config::default();
        set(&mut config, "threads.color", "rose").unwrap();
        write(&paths.file, &config).unwrap();
        let out = fs::read_to_string(&paths.file).unwrap();
        assert!(out.contains("[threads]\ncolor = \"rose\""), "{out}");
        let back = load_at(&paths).unwrap();
        assert_eq!(back.net_color(Network::Threads), Some("rose"));
        let _ = fs::remove_dir_all(&dir);
    }
}
