//! The clap command line, dispatch, JSON output and exit codes.

use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::Serialize;

use crate::command::Cmd;
use crate::config::{Config, Enter};
use crate::intent::Intent;
use crate::media::{ImageMode, Origin, Viewer};
use crate::model::{FeedId, Post, Scope, Section};
use crate::net::{Clients, FeedKind, Network};
use crate::plain::PrintOpts;
use crate::render::{Kind, StyledLine};
use crate::send::Done;
use crate::state::State;
use crate::theme::{Choice, ColorMode, Density, IconMode, Icons, Layout, Scheme, Switch, Theme};
use crate::{
    agent, clip, command, config, drafts, help, net, plain, rich, screens, send, state, theme, tui,
    util,
};

const ABOUT: &str = "\
A thread-first terminal client for X, Bluesky, dev/null, Threads and Mastodon.

Signed in, it merges the home timelines of every network into one reader, or
reads one network with --net. `socli --demo` opens an offline feed from all four.

Images paint in the terminal through viuer. `socli --print` leaves the feed in
the scrollback.
";

#[derive(Parser)]
// bin_name stays `socli` on Windows, where argv0 is `socli.exe`.
#[command(name = "socli", bin_name = "socli", version, about = "A thread-first terminal client for X, Bluesky, dev/null, Threads and Mastodon", long_about = ABOUT, disable_help_flag = true, disable_help_subcommand = true)]
struct Cli {
    /// Use the offline demo even if accounts are signed in.
    #[arg(long, global = true)]
    demo: bool,
    /// Print into the scrollback instead of the full-screen reader.
    #[arg(long, global = true)]
    print: bool,
    /// Emit JSON for a pipe.
    #[arg(long, global = true)]
    json: bool,
    /// Check a write against sign-in, caps and limits without sending it.
    #[arg(long, global = true)]
    dry_run: bool,
    /// Do not open the `SoCLI>` prompt after printing.
    #[arg(long, global = true)]
    no_prompt: bool,
    /// Paint post images inline: auto, kitty, iterm, sixel, blocks, or off, as `--images=blocks`. Auto when given alone.
    #[arg(
        long,
        global = true,
        value_name = "mode",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "auto",
        value_parser = ["auto", "kitty", "iterm", "sixel", "blocks", "off"]
    )]
    images: Option<String>,
    /// How many posts to ask each network for. Each returned X post is billed.
    #[arg(long, global = true, default_value_t = 15)]
    count: u8,
    /// Wrap width for --print. Defaults to the terminal width.
    #[arg(long, global = true)]
    width: Option<usize>,
    /// Networks to read or post to (x, bluesky, devnull, threads, mastodon, a comma list, or all). Defaults to SOCLI_NET.
    #[arg(long, global = true, value_name = "list")]
    net: Option<String>,
    /// auto, native, black, white, or a named theme from socli config themes. Auto follows the terminal background.
    #[arg(long, global = true, value_name = "name", value_parser = parse_theme)]
    theme: Option<String>,
    /// compact, cozy or roomy: the rows between posts and sections.
    #[arg(long, global = true, value_name = "name", value_parser = ["compact", "cozy", "roomy"])]
    density: Option<String>,
    /// plain, minimal, medium or rich. `standard` is the old name for medium. Unset, rich, or plain on an ESP or other low-resource terminal.
    #[arg(long, global = true, value_name = "level", value_parser = ["plain", "minimal", "medium", "standard", "rich"])]
    detail: Option<String>,
    /// auto, single or split: side by side panes on wide terminals.
    #[arg(long, global = true, value_name = "name", value_parser = ["auto", "single", "split"])]
    layout: Option<String>,
    /// on or off: clicks, the wheel and hover in the reader.
    #[arg(long, global = true, value_name = "on|off", value_parser = ["on", "off"])]
    mouse: Option<String>,
    /// Black and white only.
    #[arg(long, global = true)]
    mono: bool,
    /// Unicode icons, no Nerd Font needed.
    #[arg(long, global = true)]
    no_icons: bool,
    /// Icon set: auto, nerd, or unicode. Auto picks Nerd Font glyphs on Ghostty and WezTerm.
    #[arg(long, global = true, value_enum, conflicts_with = "no_icons")]
    icons: Option<IconsArg>,
    /// Show the designed help page.
    #[arg(long, short = 'h', global = true, action = clap::ArgAction::SetTrue)]
    help: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Following, latest, trending, discover, saved, mine, or notifications.
    Feed { which: Option<FeedName> },
    /// Search the networks in scope that can search.
    Search { query: Vec<String> },
    /// Public posts from one account.
    User { handle: String },
    /// One conversation, from a scoped id or a post link.
    Thread { id: String },
    /// Publish a post on the --net networks, or on the default one.
    Post {
        text: Vec<String>,
        /// Send <b> and <i> as typed.
        #[arg(long)]
        raw: bool,
    },
    /// Reply to a post id.
    Reply {
        id: String,
        text: Vec<String>,
        /// Send <b> and <i> as typed.
        #[arg(long)]
        raw: bool,
    },
    /// Quote a post id.
    Quote {
        id: String,
        text: Vec<String>,
        /// Send <b> and <i> as typed.
        #[arg(long)]
        raw: bool,
    },
    /// Copy a post's link.
    Share {
        target: String,
        /// Copy the author's profile link instead.
        #[arg(long)]
        profile: bool,
    },
    /// Repost.
    Repost { id: String },
    /// Undo a repost.
    Unrepost { id: String },
    /// Bookmark.
    Bookmark { id: String },
    /// Remove a bookmark.
    Unbookmark { id: String },
    /// Like.
    Like { id: String },
    /// Remove a like.
    Unlike { id: String },
    /// Follow an account.
    Follow { handle: String },
    /// Unfollow an account.
    Unfollow { handle: String },
    /// Delete one of your posts.
    Delete {
        id: String,
        /// Delete without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Show a post's images in the terminal.
    View { target: String },
    /// Status, the default network, and each network's sign-in.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Shell completions.
    Completions { shell: Shell },
    /// The designed help page, or one command's page.
    Help { command: Option<String> },
    /// Sign in to a network.
    Login {
        #[command(subcommand)]
        net: LoginNet,
    },
    /// Sign out of one network, or all of them.
    Logout {
        /// x, bluesky, devnull, threads, mastodon, or all.
        net: String,
    },
    /// The accounts this process will use.
    Whoami,
    /// Read posts already loaded. Does not call a model.
    Ask { prompt: Vec<String> },
    /// Mentions, in the notification list.
    Notifications,
    /// Posts you saved.
    Bookmarks,
    /// Read or change settings: list, get, set, path.
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
    /// List saved drafts, or remove one.
    Drafts {
        #[command(subcommand)]
        command: Option<DraftsCommand>,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Every key and its value; unset keys show their default.
    List,
    /// One key's value.
    Get { key: String },
    /// Check a value and save it.
    Set {
        key: String,
        #[arg(required = true, num_args = 1..)]
        value: Vec<String>,
    },
    /// The config file path.
    Path,
    /// Every theme --theme and ui.theme accept.
    Themes,
}

#[derive(Subcommand)]
enum DraftsCommand {
    /// Remove a draft by its number in `socli drafts`.
    Rm { index: usize },
}

#[derive(Clone, Copy, ValueEnum)]
enum IconsArg {
    Auto,
    Nerd,
    Unicode,
}

impl From<IconsArg> for IconMode {
    fn from(arg: IconsArg) -> Self {
        match arg {
            IconsArg::Auto => IconMode::Auto,
            IconsArg::Nerd => IconMode::Nerd,
            IconsArg::Unicode => IconMode::Unicode,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum FeedName {
    Following,
    #[value(alias = "foryou")]
    Latest,
    Trending,
    Discover,
    Saved,
    Mine,
    Notifications,
}

impl FeedName {
    fn kind(self) -> Option<FeedKind> {
        match self {
            FeedName::Following => Some(FeedKind::Following),
            FeedName::Latest => Some(FeedKind::Latest),
            FeedName::Trending => Some(FeedKind::Trending),
            FeedName::Discover => Some(FeedKind::Discover),
            FeedName::Saved => Some(FeedKind::Bookmarks),
            FeedName::Mine => Some(FeedKind::Mine),
            FeedName::Notifications => None,
        }
    }
}

#[derive(Subcommand)]
enum LoginNet {
    /// The browser session at x.com, or OAuth with --oauth.
    X(XLogin),
    /// An app password from bsky.app/settings/app-passwords.
    Bluesky(BlueskyLogin),
    /// A new device key, approved in the browser.
    Devnull(DevnullLogin),
    /// An access token from your Meta app.
    Threads(ThreadsLogin),
    /// Your Mastodon server: approve in the browser, or pass a token.
    Mastodon(MastodonLogin),
}

#[derive(Args)]
struct XLogin {
    /// Use a developer app (OAuth 2.0 PKCE) instead of the browser session.
    #[arg(long)]
    oauth: bool,
    #[arg(long)]
    client_id: Option<String>,
    #[arg(long)]
    client_secret: Option<String>,
    #[arg(long)]
    scopes: Option<String>,
    #[arg(long, default_value_t = 8741)]
    port: u16,
    /// Chrome, Brave, Edge, Arc or Chromium. With --profile, skips the account question when one session matches.
    #[arg(long)]
    browser: Option<String>,
    /// Profile directory or display name, for example "Profile 7". With --browser, skips the account question when one session matches.
    #[arg(long)]
    profile: Option<String>,
}

#[derive(Args)]
struct BlueskyLogin {
    #[arg(long)]
    handle: Option<String>,
    #[arg(long)]
    service: Option<String>,
    #[arg(long)]
    password_stdin: bool,
}

#[derive(Args)]
struct DevnullLogin {
    #[arg(long)]
    device_name: Option<String>,
}

#[derive(Args)]
struct ThreadsLogin {
    #[arg(long)]
    token: Option<String>,
    #[arg(long)]
    app_secret: Option<String>,
}

#[derive(Args)]
struct MastodonLogin {
    #[arg(long)]
    server: Option<String>,
    #[arg(long)]
    token: Option<String>,
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Show what is configured for every network, without printing secrets.
    Status,
    /// Set the network `socli post` uses without --net.
    Default { net: String },
    /// X: OAuth login, a user token, or an app bearer.
    X {
        #[command(subcommand)]
        command: AuthX,
    },
    /// Bluesky: app password login.
    Bluesky {
        #[command(subcommand)]
        command: AuthBluesky,
    },
    /// dev/null: device key login, or the SSH key used for CLI posting.
    Devnull {
        #[command(subcommand)]
        command: AuthDevnull,
    },
    /// Threads: access token, OAuth, refresh.
    Threads {
        #[command(subcommand)]
        command: AuthThreads,
    },
    /// Mastodon: browser or token login.
    Mastodon {
        #[command(subcommand)]
        command: AuthMastodon,
    },
}

#[derive(Subcommand)]
enum AuthX {
    /// Browser session, or OAuth 2.0 PKCE with --oauth.
    Login(XLogin),
    /// Store a user access token you already have.
    Token {
        #[arg(long)]
        access_token: String,
        #[arg(long)]
        user_id: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        refresh_token: Option<String>,
    },
    /// Store an app-only bearer for public reads.
    Bearer { token: String },
    /// Forget the user token and the app bearer. Keeps the client id.
    Logout,
}

#[derive(Subcommand)]
enum AuthBluesky {
    /// Sign in with an app password.
    Login(BlueskyLogin),
    /// Forget the session.
    Logout,
}

#[derive(Subcommand)]
enum AuthDevnull {
    /// Create a device key and approve it in the browser.
    Login(DevnullLogin),
    /// Pick the GitHub SSH key used for CLI posting.
    Ssh {
        #[arg(long)]
        key: Option<PathBuf>,
    },
    /// Forget the device key.
    Logout,
}

#[derive(Subcommand)]
enum AuthMastodon {
    /// Sign in on your server, in the browser or with --token.
    Login(MastodonLogin),
    /// Forget the token and revoke it when socli registered the app.
    Logout,
}

#[derive(Subcommand)]
enum AuthThreads {
    /// Store an access token, pasted or from THREADS_ACCESS_TOKEN.
    Login(ThreadsLogin),
    /// Authorize in the browser and paste the redirect.
    Oauth {
        #[arg(long)]
        app_id: String,
        #[arg(long)]
        app_secret: String,
        #[arg(long)]
        redirect_uri: String,
        #[arg(long)]
        scopes: Option<String>,
    },
    /// Refresh the long-lived token.
    Refresh,
    /// Forget the token.
    Logout,
}

pub fn main() {
    if let Err(err) = run() {
        if err.contains("Broken pipe") {
            return;
        }
        if err.is_empty() {
            std::process::exit(1);
        }
        if err.starts_with(util::NEEDS_PERSON) {
            fail(&err, 2);
        }
        fail(&err, 1);
    }
}

/// Whether `flag` comes before any `--`, for the paths that run before or without clap.
fn flag_given(flag: &str) -> bool {
    std::env::args_os()
        .skip(1)
        .take_while(|arg| arg != "--")
        .any(|arg| arg == flag)
}

/// `--json`, or agent mode: errors print as one JSON line.
fn json_errors() -> bool {
    agent::active() || flag_given("--json")
}

/// `--json`, or agent mode without `--print`: output is JSON.
fn json_output() -> bool {
    flag_given("--json") || (agent::active() && !flag_given("--print"))
}

/// A person at a terminal: stdin and stdout are terminals and agent mode is off.
fn terminal() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal() && !agent::active()
}

/// Prints `message` on stderr, as `{"error":…}` when errors are JSON, and exits with `code`.
fn fail(message: &str, code: i32) -> ! {
    if json_errors() {
        eprintln!("{}", serde_json::json!({ "error": message }));
    } else {
        eprintln!("socli: {}", util::clean(message));
    }
    std::process::exit(code)
}

/**
Clap's own errors: help and version print as clap prints them, and usage errors exit 2. When
errors are JSON, the rendered message becomes one line, `{"error","tip"?,"usage"?}`.
*/
fn clap_error(err: clap::Error) -> ! {
    use clap::error::ErrorKind;
    let shown = matches!(
        err.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    );
    if shown || !json_errors() {
        err.exit();
    }
    let text = err.render().to_string();
    let mut blocks = text.split("\n\n").map(str::trim);
    let first = blocks.next().unwrap_or_default();
    let message = first.strip_prefix("error: ").unwrap_or(first);
    let words = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut value = serde_json::json!({ "error": words(message) });
    for block in blocks {
        if let Some(usage) = block.strip_prefix("Usage: ") {
            value["usage"] = words(usage).into();
        } else if block.starts_with("tip: ") {
            let tips: Vec<&str> = block
                .lines()
                .map(|line| line.trim().trim_start_matches("tip: "))
                .collect();
            value["tip"] = tips.join(" ").into();
        }
    }
    eprintln!("{value}");
    std::process::exit(2)
}

/// Exits 2: a usage error, or a step that needs a person at a terminal.
fn usage_error(message: &str) -> ! {
    fail(message, 2)
}

fn run() -> Result<(), String> {
    screens::to_stderr(json_output());
    if let Some(command) = help_asked() {
        return show_help(command.as_deref());
    }
    let mut cli = Cli::try_parse().unwrap_or_else(|err| clap_error(err));
    match cli.command.take() {
        Some(Command::Completions { shell }) => {
            let mut cmd = <Cli as clap::CommandFactory>::command();
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut cmd, "socli", &mut script);
            plain::emit(&String::from_utf8_lossy(&script));
            Ok(())
        }
        Some(Command::Auth { command }) => auth(command),
        Some(Command::Help { command }) => show_help(command.as_deref()),
        Some(Command::Login { net }) => login(net),
        Some(Command::Logout { net }) => logout(&net),
        Some(Command::Whoami) => whoami(),
        Some(Command::View { target }) => view(&cli, &target),
        Some(Command::Config { command }) => config_command(command),
        Some(Command::Drafts { command }) => drafts_command(command),
        command => dispatch(&cli, command),
    }
}

/**
`--help` or `-h` before any `--`: Some with the command it names, or Some(None) for the full page.
Clap ignores its errors here, so a missing argument or subcommand still finds the command.
*/
fn help_asked() -> Option<Option<String>> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let asked = args
        .iter()
        .skip(1)
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help" || arg == "-h");
    if !asked {
        return None;
    }
    let matches = <Cli as clap::CommandFactory>::command()
        .ignore_errors(true)
        .try_get_matches_from(args)
        .ok();
    Some(matches.and_then(|m| m.subcommand_name().map(str::to_string)))
}

/// The full help page, or one command's page; an unknown command exits 2 like a usage error.
fn show_help(command: Option<&str>) -> Result<(), String> {
    let json = json_output();
    let Some(name) = command else {
        return if json {
            say_json(&help::help_json())
        } else {
            screens::print(help::help_lines(help_width()))
        };
    };
    match (json, help::command_json(name), help::command_lines(name)) {
        (true, Some(value), _) => say_json(&value),
        (false, _, Some(lines)) => screens::print(lines),
        _ => usage_error(&format!("no command named {name}. Run socli help")),
    }
}

/// One JSON value, pretty-printed, on stdout, its fields in declaration order.
fn say_json(value: &impl Serialize) -> Result<(), String> {
    plain::emit_line(&serde_json::to_string_pretty(value).map_err(|e| e.to_string())?);
    Ok(())
}

/// The terminal width for `--help`, or 100 columns when stdout is not a terminal.
fn help_width() -> usize {
    if io::stdout().is_terminal() {
        util::terminal_width(100)
    } else {
        100
    }
}

fn login(net: LoginNet) -> Result<(), String> {
    match net {
        LoginNet::X(args) => x_login(args),
        LoginNet::Bluesky(args) => {
            bluesky_login(args)?;
            signed_in(Network::Bluesky)
        }
        LoginNet::Devnull(args) => {
            needs_browser(Network::Devnull, "socli login devnull");
            net::devnull::login(args.device_name)?;
            signed_in(Network::Devnull)
        }
        LoginNet::Threads(args) => {
            net::threads::login(args.token, args.app_secret)?;
            signed_in(Network::Threads)
        }
        LoginNet::Mastodon(args) => mastodon_login(args),
    }
}

/// Mastodon's sign-in: a token (flag or MASTODON_ACCESS_TOKEN) needs no browser.
fn mastodon_login(args: MastodonLogin) -> Result<(), String> {
    let env_token = std::env::var("MASTODON_ACCESS_TOKEN").is_ok_and(|t| !t.trim().is_empty());
    if args.token.is_none() && !env_token {
        needs_browser(Network::Mastodon, "socli login mastodon");
    }
    net::mastodon::login(args.server, args.token)?;
    signed_in(Network::Mastodon)
}

/// The JSON line after a sign-in: the account just saved and its profile link.
#[derive(Serialize)]
struct SignedIn {
    ok: bool,
    network: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

/// In JSON output, prints the account a sign-in just saved; the text output is the sign-in's own.
fn signed_in(net: Network) -> Result<(), String> {
    if !json_output() {
        return Ok(());
    }
    let handle = account_handle(net)?;
    say_json(&SignedIn {
        ok: true,
        network: net.slug(),
        url: handle.as_deref().map(|handle| net.profile_url(handle)),
        handle,
    })
}

/// The signed-in handle on `net`, read back from the saved config.
fn account_handle(net: Network) -> Result<Option<String>, String> {
    Ok(Clients::load(&config::load()?)?
        .infos()
        .into_iter()
        .find(|info| info.net == net && info.signed_in)
        .and_then(|info| info.handle))
}

/// Imports the signed-in browser session, or runs OAuth when `--oauth` or `--client-id` is set.
fn x_login(args: XLogin) -> Result<(), String> {
    needs_browser(Network::X, "socli login x");
    if args.oauth || args.client_id.is_some() || args.client_secret.is_some() {
        net::x::auth::login(args.client_id, args.client_secret, args.scopes, args.port)?;
    } else {
        match net::x::auth::login_browser(args.browser.as_deref(), args.profile.as_deref()) {
            Err(err) if err == "cancelled" => return Ok(()),
            other => other?,
        }
    }
    signed_in(Network::X)
}

/// X, dev/null, Threads OAuth and Mastodon without a token sign in through a browser, so agent mode refuses them with exit 2.
fn needs_browser(net: Network, run: &str) {
    if agent::active() {
        usage_error(&format!(
            "signing in to {} needs a person at a browser. Run {run} in a terminal",
            net.title()
        ));
    }
}

/// Bluesky's sign-in; `--password-stdin` reads the app password from one stdin line and never prompts.
fn bluesky_login(args: BlueskyLogin) -> Result<(), String> {
    if !args.password_stdin {
        return net::bluesky::login(args.handle, args.service);
    }
    let Some(handle) = args.handle else {
        usage_error("--password-stdin needs --handle");
    };
    let mut password = String::new();
    io::stdin()
        .read_line(&mut password)
        .map_err(|e| format!("couldn't read the app password from stdin: {e}"))?;
    net::bluesky::login_with_password(handle, args.service, password.trim().to_string())
}

fn logout_one(net: Network) -> Result<Option<String>, String> {
    match net {
        Network::X => net::x::auth::logout().map(|()| None),
        Network::Bluesky => net::bluesky::logout().map(|()| None),
        Network::Devnull => net::devnull::logout(),
        Network::Threads => net::threads::logout().map(|()| None),
        Network::Mastodon => net::mastodon::logout().map(|()| None),
    }
}

/// One network in `socli logout --json`.
#[derive(Serialize)]
struct LoggedOut {
    network: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// `socli logout <slug|all>`; with `all`, every network is tried and the failures listed.
fn logout(target: &str) -> Result<(), String> {
    let nets = if target.eq_ignore_ascii_case("all") {
        Network::ALL.to_vec()
    } else {
        vec![target.parse::<Network>()?]
    };
    if json_output() {
        let rows: Vec<LoggedOut> = nets
            .into_iter()
            .map(|net| {
                let error = logout_one(net).err();
                LoggedOut {
                    network: net.slug(),
                    ok: error.is_none(),
                    error,
                }
            })
            .collect();
        say_json(&rows)?;
        return if rows.iter().all(|row| row.ok) {
            Ok(())
        } else {
            Err(String::new())
        };
    }
    let mut failed = Vec::new();
    for net in nets {
        match logout_one(net) {
            Ok(note) => {
                screens::print(vec![StyledLine::text(
                    Kind::Green,
                    &format!("  Signed out of {}", net.title()),
                )])?;
                if let Some(note) = note {
                    util::say(format!("  {note}"))?;
                }
            }
            Err(err) => failed.push(format!("{}: {err}", net.label())),
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

/// One network in `socli whoami --json`; no network stores a display name, so none is printed.
#[derive(Serialize)]
struct Account {
    network: &'static str,
    signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

fn whoami() -> Result<(), String> {
    let clients = Clients::load(&config::load()?)?;
    let signed: Vec<_> = clients
        .infos()
        .into_iter()
        .filter(|info| info.signed_in)
        .collect();
    if json_output() {
        let accounts: Vec<Account> = Network::ALL
            .into_iter()
            .map(|net| {
                let info = signed.iter().find(|info| info.net == net);
                let handle = info.and_then(|info| info.handle.clone());
                Account {
                    network: net.slug(),
                    signed_in: info.is_some(),
                    url: handle.as_deref().map(|handle| net.profile_url(handle)),
                    handle,
                }
            })
            .collect();
        return say_json(&accounts);
    }
    if signed.is_empty() {
        return Err(state::NO_ACCOUNTS.into());
    }
    for info in signed {
        let handle = info.handle.unwrap_or_else(|| "signed in".into());
        screens::print(screens::profile_lines(
            &handle,
            Some(&handle),
            info.net.label(),
        ))?;
    }
    Ok(())
}

fn auth(command: AuthCommand) -> Result<(), String> {
    let json = json_output();
    match command {
        AuthCommand::Status if json => say_json(&status_json(&config::load()?)),
        AuthCommand::Status => {
            plain::emit(&util::clean(&status_report(&config::load()?)));
            Ok(())
        }
        AuthCommand::Default { net } => {
            let net: Network = net.parse()?;
            config::update(|cfg| cfg.default_network = Some(net.slug().into()))?;
            if json {
                say_json(&serde_json::json!({ "ok": true, "default_network": net.slug() }))
            } else {
                plain::emit_line(&format!("default network · {}", net.label()));
                Ok(())
            }
        }
        AuthCommand::X { command } => match command {
            AuthX::Login(args) => x_login(args),
            AuthX::Token {
                access_token,
                user_id,
                username,
                refresh_token,
            } => {
                net::x::auth::save_token(access_token, user_id, username, refresh_token)?;
                auth_done(
                    Network::X,
                    "token",
                    format!("X user token saved · {}", config::path().display()),
                )
            }
            AuthX::Bearer { token } => {
                net::x::auth::save_bearer(token)?;
                auth_done(
                    Network::X,
                    "bearer",
                    format!("X bearer saved · {}", config::path().display()),
                )
            }
            AuthX::Logout => auth_logout(Network::X),
        },
        AuthCommand::Bluesky { command } => match command {
            AuthBluesky::Login(args) => {
                bluesky_login(args)?;
                signed_in(Network::Bluesky)
            }
            AuthBluesky::Logout => auth_logout(Network::Bluesky),
        },
        AuthCommand::Devnull { command } => match command {
            AuthDevnull::Login(args) => {
                needs_browser(Network::Devnull, "socli login devnull");
                net::devnull::login(args.device_name)?;
                signed_in(Network::Devnull)
            }
            AuthDevnull::Ssh { key } => {
                net::devnull::setup_ssh(key)?;
                auth_done(Network::Devnull, "ssh", String::new())
            }
            AuthDevnull::Logout => auth_logout(Network::Devnull),
        },
        AuthCommand::Threads { command } => match command {
            AuthThreads::Login(args) => {
                net::threads::login(args.token, args.app_secret)?;
                signed_in(Network::Threads)
            }
            AuthThreads::Oauth {
                app_id,
                app_secret,
                redirect_uri,
                scopes,
            } => {
                needs_browser(Network::Threads, "socli auth threads oauth");
                net::threads::oauth(app_id, app_secret, redirect_uri, scopes)?;
                auth_done(Network::Threads, "oauth", String::new())
            }
            AuthThreads::Refresh => {
                net::threads::refresh()?;
                auth_done(Network::Threads, "refresh", String::new())
            }
            AuthThreads::Logout => auth_logout(Network::Threads),
        },
        AuthCommand::Mastodon { command } => match command {
            AuthMastodon::Login(args) => mastodon_login(args),
            AuthMastodon::Logout => auth_logout(Network::Mastodon),
        },
    }
}

/// After an auth subcommand saved or forgot something: its JSON line, or `text` when there is any.
fn auth_done(net: Network, action: &str, text: String) -> Result<(), String> {
    if json_output() {
        say_json(&serde_json::json!({ "ok": true, "network": net.slug(), "action": action }))
    } else {
        if !text.is_empty() {
            plain::emit_line(&util::clean(&text));
        }
        Ok(())
    }
}

/// `socli auth <net> logout`, which prints the auth JSON line rather than `socli logout`'s list.
fn auth_logout(net: Network) -> Result<(), String> {
    if json_output() {
        logout_one(net)?;
        auth_done(net, "logout", String::new())
    } else {
        logout(net.slug())
    }
}

/// A `--theme` value in its canonical form.
fn parse_theme(value: &str) -> Result<String, String> {
    Scheme::parse(value).map(Scheme::word).ok_or_else(|| {
        "use auto, native, black, white or a theme from socli config themes".to_string()
    })
}

/// `socli config list|get|set|path|themes`; no subcommand lists.
fn config_command(command: Option<ConfigCommand>) -> Result<(), String> {
    let json = json_output();
    let path = config::path().display().to_string();
    match command.unwrap_or(ConfigCommand::List) {
        ConfigCommand::Path if json => say_json(&serde_json::json!({ "path": path })),
        ConfigCommand::Path => {
            plain::emit_line(&util::clean(&path));
            Ok(())
        }
        ConfigCommand::Themes if json => {
            let themes: Vec<_> = Scheme::list()
                .into_iter()
                .map(|scheme| {
                    serde_json::json!({
                        "name": scheme.word(),
                        "description": scheme.describe(),
                        "variants": scheme.variants(),
                    })
                })
                .collect();
            say_json(&serde_json::json!({ "themes": themes }))
        }
        ConfigCommand::Themes => {
            let list = Scheme::list();
            let width = list.iter().map(|s| s.word().len()).max().unwrap_or(0) + 2;
            let mut out = String::new();
            for scheme in list {
                out.push_str(&format!("{:width$}{}\n", scheme.word(), scheme.describe()));
            }
            plain::emit(&out);
            Ok(())
        }
        ConfigCommand::List if json => {
            let cfg = config::load()?;
            let mut values = serde_json::Map::new();
            for key in config::keys() {
                let value = config::get(&cfg, &key)?.unwrap_or_else(|| default_value(&cfg, &key));
                values.insert(key.clone(), config_json(&key, &value));
            }
            values.insert("path".into(), serde_json::json!(path));
            say_json(&serde_json::Value::Object(values))
        }
        ConfigCommand::List => {
            let cfg = config::load()?;
            let mut out = String::new();
            for key in config::keys() {
                out.push_str(&match config::get(&cfg, &key)? {
                    Some(value) => format!("{key} = {value}\n"),
                    None => format!("{key} = {} (default)\n", default_value(&cfg, &key)),
                });
            }
            plain::emit(&util::clean(&out));
            Ok(())
        }
        ConfigCommand::Get { key } => {
            let cfg = config::load()?;
            let value = config::get(&cfg, &key)?;
            let default = value.is_none();
            let value = value.unwrap_or_else(|| default_value(&cfg, &key));
            if json {
                let value = config_json(&key, &value);
                say_json(&serde_json::json!({ "key": key, "value": value, "default": default }))
            } else {
                plain::emit_line(&util::clean(&value));
                Ok(())
            }
        }
        ConfigCommand::Set { key, value } => {
            let value = value.join(" ");
            let saved = config::set(&mut config::load()?, &key, &value)?;
            config::update(|cfg| {
                let _ = config::set(cfg, &key, &value);
            })?;
            if json {
                let value = config_json(&key, &saved);
                say_json(&serde_json::json!({ "ok": true, "key": key, "value": value }))
            } else {
                plain::emit_line(&util::clean(&format!("saved {key} = {saved}")));
                Ok(())
            }
        }
    }
}

/// A config value in JSON: `feed.networks` as an array of slugs, every other key as its string.
fn config_json(key: &str, value: &str) -> serde_json::Value {
    if key == "feed.networks" {
        serde_json::json!(value.split(',').map(str::trim).collect::<Vec<_>>())
    } else {
        serde_json::json!(value)
    }
}

/// What an unset key means; palette colors follow the theme `--print` would use.
fn default_value(cfg: &Config, key: &str) -> String {
    config::default_of(key)
        .or_else(|| {
            let name = key.strip_prefix("colors.")?;
            let scheme = cfg
                .ui
                .theme
                .as_deref()
                .and_then(Scheme::parse)
                .unwrap_or(Scheme::Auto);
            theme::palette_value(name, scheme)
        })
        .unwrap_or_default()
}

/// `socli drafts` lists; `socli drafts rm <index>` removes one.
fn drafts_command(command: Option<DraftsCommand>) -> Result<(), String> {
    match command {
        None => {
            let list = drafts::load()?;
            if json_output() {
                let rows: Vec<DraftRow> = list
                    .iter()
                    .enumerate()
                    .map(|(i, draft)| DraftRow::of(i + 1, draft))
                    .collect();
                return say_json(&rows);
            }
            if list.is_empty() {
                plain::emit_line("no drafts. The composer keeps one when you leave it with text");
                return Ok(());
            }
            plain::emit_line(&drafts::list_lines(&list, chrono::Utc::now()).join("\n"));
            Ok(())
        }
        Some(DraftsCommand::Rm { index }) => {
            let draft = drafts::remove_index(index)?;
            if json_output() {
                say_json(&serde_json::json!({ "ok": true, "removed": index, "id": draft.id }))
            } else {
                plain::emit_line(&util::clean(&format!(
                    "removed draft {index} · {}",
                    draft.target()
                )));
                Ok(())
            }
        }
    }
}

/// One draft in `socli drafts --json`; `index` is the number `socli drafts rm` takes.
#[derive(Serialize)]
struct DraftRow<'a> {
    index: usize,
    id: &'a str,
    targets: &'a [String],
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote: Option<&'a str>,
    created_at: String,
    updated_at: String,
}

impl<'a> DraftRow<'a> {
    fn of(index: usize, draft: &'a drafts::Draft) -> Self {
        DraftRow {
            index,
            id: &draft.id,
            targets: &draft.targets,
            text: &draft.text,
            reply_to: draft.reply_to.as_deref(),
            quote: draft.quote.as_deref(),
            created_at: draft.created_at.to_rfc3339(),
            updated_at: draft.updated_at.to_rfc3339(),
        }
    }
}

/// `socli auth status --json`: the facts of the text report, without the secrets' redacted tails.
#[derive(Serialize)]
struct Status {
    config: String,
    default_network: Option<&'static str>,
    images: &'static str,
    networks: Vec<NetStatus>,
}

#[derive(Serialize)]
struct NetStatus {
    network: &'static str,
    signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    method: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn status_json(config: &Config) -> Status {
    Status {
        config: config::path().display().to_string(),
        default_network: config
            .default_network
            .as_deref()
            .and_then(|slug| slug.parse::<Network>().ok())
            .map(Network::slug),
        images: ImageMode::resolve(None, config).word(),
        networks: Network::ALL
            .into_iter()
            .map(|net| net_status(net, &status_rows(net, config)))
            .collect(),
    }
}

/**
One network's status from its text rows: the account row gives the handle, the first stored
credential the method, and the rest of the account row, the device key state or the service the
detail.
*/
fn net_status(net: Network, rows: &[(&'static str, String)]) -> NetStatus {
    let row = |key: &str| {
        rows.iter()
            .find(|(name, _)| *name == key)
            .map_or("", |(_, value)| value.as_str())
    };
    let stored = |key: &str| !matches!(row(key), "" | "missing");
    let account = row("account");
    let handle = account.strip_prefix('@').map(str::to_string);
    let signed_in = handle.is_some() || account == "signed in";
    let (method, detail) = match net {
        Network::X if stored("browser session") => (Some("browser"), None),
        Network::X if stored("user token") => (Some("oauth"), None),
        Network::X if stored("app bearer") => (
            Some("bearer"),
            account.split_once(" · ").map(|(_, rest)| rest.to_string()),
        ),
        Network::Bluesky if signed_in => (Some("app password"), Some(row("service").to_string())),
        Network::Devnull if stored("device key") => {
            (Some("device key"), Some(row("device key").to_string()))
        }
        Network::Devnull if stored("ssh key") => (Some("ssh key"), None),
        Network::Threads if stored("token") => (Some("token"), None),
        Network::Mastodon if stored("token") => (
            Some(if row("app") == "registered" {
                "oauth"
            } else {
                "token"
            }),
            Some(row("server").to_string()),
        ),
        _ => (None, None),
    };
    NetStatus {
        network: net.slug(),
        signed_in,
        handle,
        method,
        detail,
    }
}

fn status_rows(net: Network, config: &Config) -> Vec<(&'static str, String)> {
    match net {
        Network::X => net::x::auth::status(&config.x),
        Network::Bluesky => net::bluesky::status(&config.bluesky),
        Network::Devnull => net::devnull::status(&config.devnull),
        Network::Threads => net::threads::status(&config.threads),
        Network::Mastodon => net::mastodon::status(&config.mastodon),
    }
}

/// `socli auth status`: where the config lives, then each network's rows with secrets redacted.
fn status_report(config: &Config) -> String {
    let default = config
        .default_network
        .as_deref()
        .and_then(|slug| slug.parse::<Network>().ok())
        .map_or("not set", Network::label);
    let mut out = format!(
        "{:<17}{}\n{:<17}{}\n{:<17}{}\n",
        "config",
        config::path().display(),
        "default network",
        default,
        "images",
        ImageMode::resolve(None, config).word()
    );
    for net in Network::ALL {
        let rows = status_rows(net, config);
        out.push_str(&format!("\n{}\n", net.label()));
        for (key, value) in rows {
            out.push_str(&format!("  {key:<15}{value}\n"));
        }
    }
    out
}

/**
`socli view`: `demo`, a post id or permalink (its images in order, a blank line apart), or any
other image URL or file. JSON output paints nothing and prints the image URLs.
*/
fn view(cli: &Cli, target: &str) -> Result<(), String> {
    let post = (target != "demo")
        .then(|| net::parse_target(target, None).ok())
        .flatten();
    let origin = if post.is_some() {
        Origin::Post
    } else {
        Origin::Typed
    };
    let images = match &post {
        Some(id) => post_images(cli, id)?,
        None if target == "demo" => vec!["demo://spark".to_string()],
        None => {
            crate::media::check_source(target)?;
            vec![target.to_string()]
        }
    };
    if cli.json || (agent::active() && !cli.print) {
        return say_json(&match post {
            Some(id) => serde_json::json!({ "id": id, "images": images }),
            None => serde_json::json!({ "images": images }),
        });
    }
    let viewer = Viewer::new(ImageMode::resolve(
        cli.images.as_deref().and_then(ImageMode::parse),
        &config::load().unwrap_or_default(),
    ));
    let mut shown = None;
    for (i, url) in images.iter().enumerate() {
        if i > 0 {
            plain::emit_line("");
        }
        let path = crate::media::materialize(url, origin)?;
        shown = Some(crate::media::show_with(&viewer, &path, 60, false)?);
    }
    if let Some(kind) = shown {
        plain::emit_line(kind.label());
    }
    Ok(())
}

/// The image URLs of post `id`, loaded like `socli thread` when the demo doesn't hold it.
fn post_images(cli: &Cli, id: &str) -> Result<Vec<String>, String> {
    let config = config::load()?;
    let mut clients = if cli.demo {
        Clients::empty()
    } else {
        Clients::load(&config)?
    };
    let mut state = State::boot(cli.demo, cli.count, &config, &clients, None);
    if !state.posts.contains_key(id) {
        state.apply(Cmd::Thread(id.to_string()), &mut clients)?;
    }
    let post = state
        .posts
        .get(id)
        .ok_or_else(|| format!("no post {id}. Check the id, or open it with socli thread {id}"))?;
    let images: Vec<String> = post
        .media
        .iter()
        .filter_map(|media| media.view_url())
        .map(str::to_string)
        .collect();
    if images.is_empty() {
        return Err("this post has no images".into());
    }
    Ok(images)
}

/// The `--net` networks, or `SOCLI_NET` when the flag is absent.
fn net_list(cli: &Cli) -> Result<Option<Vec<Network>>, String> {
    net_value(cli).as_deref().map(net::parse_list).transpose()
}

fn net_value(cli: &Cli) -> Option<String> {
    cli.net.clone().or_else(|| {
        std::env::var("SOCLI_NET")
            .ok()
            .filter(|v| !v.trim().is_empty())
    })
}

/// `--net all`, or `all` in a list: every signed-in network that can do the write.
fn wants_all(cli: &Cli) -> bool {
    net_value(cli).is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("all"))
    })
}

fn dispatch(cli: &Cli, command: Option<Command>) -> Result<(), String> {
    let print = cli.print;
    let json = cli.json || (agent::active() && !print);
    let dry_run = cli.dry_run;
    let config = config::load()?;
    let nets = net_list(cli)?;
    let demo = cli.demo;
    let mut clients = if demo {
        Clients::empty()
    } else {
        Clients::load(&config)?
    };
    let open_gate = terminal() && !print && !json && command.is_none();
    if !demo && clients.signed_in().is_empty() && !public_read(&command) && !open_gate {
        return Err(state::NO_ACCOUNTS.into());
    }
    let mut state = State::boot(demo, cli.count, &config, &clients, nets.as_deref());
    if let (Some(list), true) = (&nets, public_read(&command)) {
        // Public reads go to the named networks whether or not they are signed in.
        state.set_on(list.clone());
    }
    let named = match nets.as_deref() {
        Some([net]) => Some(*net),
        _ => None,
    };
    let interactive = terminal() && !print && !json;
    let mut notices = Vec::new();
    appearance(&mut state, cli, &config, interactive, &mut notices);
    prepare(&mut state, &command)?;

    let fallback = named.or(state.scope_net()).or(state.default_network);
    let all = wants_all(cli);
    match &command {
        Some(Command::Post { text, raw }) => {
            let targets = if all {
                all_targets(&state)?
            } else {
                default_targets(&state, nets.as_deref())?
            };
            let publish = Publish {
                usage: "post",
                words: text,
                raw: *raw,
                dry_run,
                targets,
                reply_to: None,
                quote: None,
            };
            return publish.run(&mut state, &mut clients, json);
        }
        Some(Command::Reply { id, text, raw }) => {
            let id = net::parse_target(id, fallback)?;
            state.pin(&id);
            let publish = Publish {
                usage: "reply <id>",
                words: text,
                raw: *raw,
                dry_run,
                targets: vec![net_of(&id)?],
                reply_to: Some(id),
                quote: None,
            };
            return publish.run(&mut state, &mut clients, json);
        }
        Some(Command::Quote { id: arg, text, raw }) => {
            let id = net::parse_target(arg, fallback)?;
            state.pin(&id);
            let targets = match &nets {
                _ if all => all_targets(&state)?,
                Some(list) => list.clone(),
                None => vec![net_of(&id)?],
            };
            let link = arg.starts_with("http://") || arg.starts_with("https://");
            let publish = Publish {
                usage: "quote <id>",
                words: text,
                raw: *raw,
                dry_run,
                targets,
                reply_to: None,
                quote: Some(if link { arg.clone() } else { id }),
            };
            return publish.run(&mut state, &mut clients, json);
        }
        Some(Command::Share { target, profile }) => {
            let id = net::parse_target(target, fallback)?;
            return share(&mut state, &mut clients, &id, *profile, json);
        }
        _ => {}
    }
    if let Some((pin, action)) = action_of(&command) {
        let (net, id) = match pin {
            Some(raw) => {
                let id = net::parse_target(&raw, fallback)?;
                state.pin(&id);
                (net_of(&id)?, Some(id))
            }
            None => (state.handle_network()?, None),
        };
        let unconfirmed = matches!(command, Some(Command::Delete { yes: false, .. }));
        if unconfirmed && !dry_run && !confirm_delete(net, json)? {
            plain::emit_line("nothing deleted");
            return Ok(());
        }
        return write_action(&mut state, &mut clients, net, id, action, json, dry_run);
    }

    let first = match &command {
        Some(Command::Search { query }) => state
            .apply(Cmd::Search(query.join(" ")), &mut clients)
            .map(|_| ()),
        Some(Command::User { handle }) => state
            .apply(Cmd::User(command::clean_handle(handle)), &mut clients)
            .map(|_| ()),
        Some(Command::Thread { id }) => state
            .apply(Cmd::Thread(id.clone()), &mut clients)
            .map(|_| ()),
        _ => state.ensure_loaded(&mut clients),
    };
    if let Err(err) = first {
        if !interactive {
            return Err(err);
        }
        state.status = err;
    }

    if interactive {
        if let Some(first) = notices.first() {
            state.status = match notices.len() {
                1 => first.clone(),
                n => format!("{first} (+{} more)", n - 1),
            };
        }
        tui::run(state, clients)
    } else {
        for notice in &notices {
            eprintln!("socli: {}", util::clean(notice));
        }
        let opts = PrintOpts::from_flags(json, cli.images.is_some(), !cli.no_prompt, cli.width);
        if opts.prompt {
            plain::repl(&mut state, &mut clients, &opts)
        } else {
            plain::print_state(&state, &opts)
        }
    }
}

/**
Theme, colors, density, icons and images, each from its flag, then its environment variable, then
the config, then the default. Detail's default follows the opening client: rich, or plain on an
ESP or other low-resource terminal. Invalid colors add notices. NO_COLOR makes the reader mono.
*/
fn appearance(
    state: &mut State,
    cli: &Cli,
    config: &Config,
    interactive: bool,
    notices: &mut Vec<String>,
) {
    let env = |key: &str| std::env::var(key).ok().filter(|v| !v.trim().is_empty());
    fn pick<T: Choice>(flag: Option<&str>, env: Option<String>, config: Option<&str>) -> Option<T> {
        flag.and_then(T::parse)
            .or_else(|| env.as_deref().and_then(T::parse))
            .or_else(|| config.and_then(T::parse))
    }
    state.scheme = cli
        .theme
        .as_deref()
        .and_then(Scheme::parse)
        .or_else(|| env("SOCLI_THEME").as_deref().and_then(Scheme::parse))
        .or_else(|| config.ui.theme.as_deref().and_then(Scheme::parse))
        .unwrap_or(Scheme::Auto);
    state.density = pick(
        cli.density.as_deref(),
        env("SOCLI_DENSITY"),
        config.ui.density.as_deref(),
    )
    .unwrap_or(Density::Cozy);
    state.detail = pick(
        cli.detail.as_deref(),
        env("SOCLI_DETAIL"),
        config.ui.detail.as_deref(),
    )
    .unwrap_or_else(|| theme::default_detail(env));
    state.layout = pick(
        cli.layout.as_deref(),
        env("SOCLI_LAYOUT"),
        config.ui.layout.as_deref(),
    )
    .unwrap_or(Layout::Auto);
    state.unfurl =
        pick(None, None, config.ui.unfurl.as_deref()).unwrap_or(Switch::On) == Switch::On;
    state.enter = pick(None, None, config.ui.enter.as_deref()).unwrap_or(Enter::Post);
    state.mouse = pick(
        cli.mouse.as_deref(),
        env("SOCLI_MOUSE"),
        config.ui.mouse.as_deref(),
    )
    .unwrap_or(Switch::On)
        == Switch::On;
    state.color_mode = if cli.mono {
        ColorMode::Mono
    } else {
        pick(None, None, config.ui.colors.as_deref()).unwrap_or(ColorMode::Full)
    };
    let icons_flag = cli.icons.map(IconMode::from);
    let icons_config = pick(None, None, config.ui.icons.as_deref());
    state.icon_mode = icons_flag
        .or_else(|| pick(None, env("SOCLI_ICONS"), None))
        .or(icons_config)
        .unwrap_or(IconMode::Auto);
    state.icons = theme::resolve_icons(
        cli.no_icons,
        icons_flag,
        icons_config,
        theme::icon_env(io::stdout().is_terminal() && !agent::active()),
    );
    state.viewer = Viewer::new(ImageMode::resolve(
        cli.images.as_deref().and_then(ImageMode::parse),
        config,
    ));
    state.auto_dark = env("COLORFGBG")
        .as_deref()
        .and_then(theme::colorfgbg_dark)
        .unwrap_or(true);
    state.forced_mono = interactive && env("NO_COLOR").is_some();
    state.theme = Theme::from_env().with_colors(
        config
            .colors
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
        Network::ALL.map(|net| (net, config.net_color(net))),
        notices,
    );
    state.retheme();
}

/// Reads that work without an account, through public clients or an X app bearer.
fn public_read(command: &Option<Command>) -> bool {
    matches!(
        command,
        Some(Command::Search { .. }) | Some(Command::User { .. }) | Some(Command::Thread { .. })
    )
}

fn prepare(state: &mut State, command: &Option<Command>) -> Result<(), String> {
    let kind = match command {
        Some(Command::Feed { which: Some(which) }) => match which.kind() {
            Some(kind) => Some(kind),
            None => {
                state.section = Section::Notifications;
                None
            }
        },
        Some(Command::Bookmarks) => Some(FeedKind::Bookmarks),
        Some(Command::Ask { .. }) => {
            state.start_ask = true;
            None
        }
        Some(Command::Notifications) => {
            state.section = Section::Notifications;
            None
        }
        _ => None,
    };
    if let Some(kind) = kind {
        match state.scope() {
            Scope::One(_) => state.pick_tab(kind)?,
            Scope::All => state.focus = Some(FeedId::Kind(kind)),
        }
    }
    Ok(())
}

/// The `--net` list, else `default_network`, else the only network that can post (X in the demo).
fn default_targets(state: &State, listed: Option<&[Network]>) -> Result<Vec<Network>, String> {
    if let Some(list) = listed {
        return Ok(list.to_vec());
    }
    let can_post = state.post_targets();
    match (state.default_network, can_post.as_slice()) {
        (Some(net), _) => Ok(vec![net]),
        (None, [only]) => Ok(vec![*only]),
        (None, _) if state.demo => Ok(vec![Network::X]),
        _ => {
            Err("pick networks with --net x,bluesky (or set default_network in config.toml)".into())
        }
    }
}

fn all_targets(state: &State) -> Result<Vec<Network>, String> {
    let targets = state.post_targets();
    if targets.is_empty() {
        return Err(
            "no signed-in network can post. `socli login bluesky` (or x, devnull, threads, mastodon)".into(),
        );
    }
    Ok(targets)
}

fn net_of(id: &str) -> Result<Network, String> {
    net::split(id)
        .map(|(net, _)| net)
        .ok_or_else(|| format!("{id} has no network. use a full id like bluesky:at://…"))
}

/// Post, reply and quote, before their text is read.
struct Publish<'a> {
    /// The command and its id argument, as the missing-text error shows them.
    usage: &'static str,
    words: &'a [String],
    raw: bool,
    dry_run: bool,
    targets: Vec<Network>,
    reply_to: Option<String>,
    quote: Option<String>,
}

impl Publish<'_> {
    /// Reads the text from the words, stdin or the composer, then prints one result line per network.
    fn run(self, state: &mut State, clients: &mut Clients, json: bool) -> Result<(), String> {
        let text = match text_input(self.words)? {
            Some(text) if self.raw => text,
            Some(text) => rich::convert(&text),
            None if terminal() => {
                let composed = tui::compose(
                    state,
                    self.targets.clone(),
                    self.reply_to.clone(),
                    self.quote.clone(),
                    self.raw,
                )?;
                return match composed {
                    tui::Composed::Sent(intent) => {
                        publish(state, clients, &intent, self.dry_run, json, &drafts::path())
                    }
                    tui::Composed::Saved => {
                        plain::emit_line("draft saved");
                        Ok(())
                    }
                    tui::Composed::Discarded => {
                        plain::emit_line("nothing posted");
                        Ok(())
                    }
                };
            }
            None => {
                let name = self.usage.split(' ').next().unwrap_or(self.usage);
                let usage = self.usage;
                usage_error(&format!(
                    "{name} needs text: socli {usage} \"…\" or socli {usage} - < file"
                ))
            }
        };
        let results = if self.dry_run {
            send::check(
                state,
                &text,
                &self.targets,
                self.reply_to.as_deref(),
                self.quote.as_deref(),
            )
        } else {
            let sent = send::send(
                state,
                clients,
                &text,
                &self.targets,
                self.reply_to,
                self.quote,
            );
            send::posted(state.demo, sent)
        };
        send::report(&results, json, self.dry_run, Icons::new(state.icons))
    }
}

/**
Checks or sends exactly what the composer returned, with drafts in `store`. A dry run keeps the
previewed intent as its draft. A send goes through `send::send_intent`, which keeps the text as a
draft until every network took it; stderr says what became of that draft, and when the draft can't
be written before the send, the error carries the typed text so it is never lost.
*/
fn publish(
    state: &mut State,
    clients: &mut Clients,
    intent: &Intent,
    dry_run: bool,
    json: bool,
    store: &Path,
) -> Result<(), String> {
    intent.check()?;
    if dry_run {
        let results = send::check(
            state,
            &intent.text,
            &intent.targets,
            intent.reply_to.as_deref(),
            intent.quote.as_deref(),
        );
        let kept = send::keep(state, intent, store);
        let reported = send::report(&results, json, true, Icons::new(state.icons));
        match kept {
            Ok(()) => eprintln!("draft kept · socli drafts"),
            Err(err) => eprintln!(
                "{}",
                util::clean(&format!("couldn't keep the draft: {err}\n{}", intent.typed))
            ),
        }
        return reported;
    }
    let delivery = send::send_intent(state, clients, intent, store, send::ONE_SHOT_HINT)
        .map_err(|err| format!("{err}\n{}", intent.typed))?;
    let failed = delivery.failed();
    let names: Vec<&str> = failed.iter().map(|net| net.title()).collect();
    match (&delivery.kept, failed.is_empty()) {
        (Ok(()), true) => {}
        (Ok(()), false) => eprintln!(
            "draft saved for {} · socli drafts lists it",
            names.join(", ")
        ),
        (Err(err), true) => eprintln!("couldn't update the draft: {}", util::clean(err)),
        (Err(err), false) => eprintln!(
            "{}",
            util::clean(&format!(
                "couldn't update the draft: {err}\nnot posted to {}:\n{}",
                names.join(", "),
                intent.typed
            ))
        ),
    }
    let results = send::posted(state.demo, delivery.results);
    send::report(&results, json, false, Icons::new(state.icons))
}

/**
The words joined, or stdin for `-`. Outside agent mode stdin is also read when it is not a
terminal. None means no text: the composer opens at a terminal.
*/
fn text_input(words: &[String]) -> Result<Option<String>, String> {
    match words {
        [dash] if dash == "-" => read_stdin().map(Some),
        [] if !agent::active() && !io::stdin().is_terminal() => read_stdin().map(Some),
        [] => Ok(None),
        words => Ok(Some(words.join(" "))),
    }
}

/// All of stdin as UTF-8, less one trailing newline.
fn read_stdin() -> Result<String, String> {
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| format!("couldn't read the text from stdin: {e}"))?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    Ok(text)
}

/**
`delete this post on <Net>? y/N` for a person at a terminal. Anywhere else, and with `--json`,
`--yes` is required.
*/
fn confirm_delete(net: Network, json: bool) -> Result<bool, String> {
    if json || !terminal() {
        usage_error("delete needs --yes when socli runs without a terminal");
    }
    let answer = util::prompt_line(
        &format!("delete this post on {}? y/N ", net.title()),
        "Pass --yes to delete without asking.",
    )?;
    Ok(matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes"))
}

/**
A write on one post or account, reported through the same result lines as a post: the post's id
and permalink, or the account's handle and profile link.
*/
fn write_action(
    state: &mut State,
    clients: &mut Clients,
    net: Network,
    id: Option<String>,
    action: Cmd,
    json: bool,
    dry_run: bool,
) -> Result<(), String> {
    let handle = match &action {
        Cmd::Follow(handle) | Cmd::Unfollow(handle) => Some(handle.clone()),
        _ => None,
    };
    let url = match &handle {
        Some(handle) => Some(net.profile_url(handle)),
        None => id
            .as_ref()
            .and_then(|id| state.posts.get(id))
            .map(Post::permalink)
            .filter(|url| !url.is_empty()),
    };
    let what = if dry_run {
        send::check_write(state, net, &action)
    } else {
        state.apply(action, clients).map(|_| state.status.clone())
    };
    let result = what.map(|what| Done {
        id,
        handle,
        url,
        what,
        demo: state.demo,
        dry_run,
        ..Done::default()
    });
    send::report(&[(net, result)], json, dry_run, Icons::new(state.icons))
}

/**
`socli share`: copies the post's link, or its author's profile link with `--profile`, when a person
runs it in a terminal. Prints the link either way; a pipe or an agent gets it without a copy, so
the person's clipboard never changes behind their back.
*/
fn share(
    state: &mut State,
    clients: &mut Clients,
    id: &str,
    profile: bool,
    json: bool,
) -> Result<(), String> {
    if !state.demo && !state.posts.contains_key(id) {
        state.apply(Cmd::Thread(id.to_string()), clients)?;
    }
    let post = state
        .posts
        .get(id)
        .ok_or_else(|| format!("no post {id}. Check the id, or open it with socli thread {id}"))?;
    let (url, what) = if profile {
        let url = post.net.profile_url(&post.author.username);
        (url, "copied profile link")
    } else {
        let url = post
            .url
            .clone()
            .filter(|url| !url.is_empty())
            .ok_or_else(|| format!("{id} has no link to copy"))?;
        (url, "copied link")
    };
    if !io::stdout().is_terminal() || agent::active() {
        if json {
            plain::emit_line(&serde_json::json!({ "url": url, "copied": false }).to_string());
        } else {
            plain::emit_line(&util::clean(&url));
        }
        return Ok(());
    }
    match clip::copy(&url) {
        Ok(_) if json => {
            plain::emit_line(&serde_json::json!({ "url": url, "copied": true }).to_string());
            Ok(())
        }
        Ok(via) => {
            let suffix = match via {
                clip::Via::Terminal => " (terminal clipboard)",
                clip::Via::System => "",
            };
            plain::emit_line(&util::clean(&format!("{what}  {url}{suffix}")));
            Ok(())
        }
        Err(err) => {
            if json {
                plain::emit_line(&serde_json::json!({ "url": url, "copied": false }).to_string());
            } else {
                plain::emit_line(&util::clean(&url));
                eprintln!("socli: {}", util::clean(&err));
            }
            Ok(())
        }
    }
}

/// Writes return `(post id or link to pin, command)`. Reads return None.
fn action_of(command: &Option<Command>) -> Option<(Option<String>, Cmd)> {
    match command {
        Some(Command::Repost { id }) => Some((Some(id.clone()), Cmd::Repost { n: None })),
        Some(Command::Unrepost { id }) => Some((Some(id.clone()), Cmd::Unrepost { n: None })),
        Some(Command::Bookmark { id }) => Some((Some(id.clone()), Cmd::Bookmark { n: None })),
        Some(Command::Unbookmark { id }) => Some((Some(id.clone()), Cmd::Unbookmark { n: None })),
        Some(Command::Like { id }) => Some((Some(id.clone()), Cmd::Like { n: None })),
        Some(Command::Unlike { id }) => Some((Some(id.clone()), Cmd::Unlike { n: None })),
        Some(Command::Delete { id, .. }) => Some((Some(id.clone()), Cmd::Delete { n: None })),
        Some(Command::Follow { handle }) => {
            Some((None, Cmd::Follow(command::clean_handle(handle))))
        }
        Some(Command::Unfollow { handle }) => {
            Some((None, Cmd::Unfollow(command::clean_handle(handle))))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty directory of the test's own, removed by the test.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "socli-main-{name}-{}-{:08x}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_composed_dry_run_checks_the_intent() {
        let dir = temp("dry-run");
        let store = dir.join("drafts.json");
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let posts = state.posts.len();
        let mut intent = Intent {
            text: "a".repeat(290),
            typed: "a".repeat(290),
            targets: vec![Network::Bluesky],
            reply_to: None,
            quote: None,
            draft_id: None,
        };
        let mut clients = Clients::empty();
        assert_eq!(
            publish(&mut state, &mut clients, &intent, true, true, &store),
            Ok(())
        );
        intent.targets = vec![Network::X];
        assert!(publish(&mut state, &mut clients, &intent, true, true, &store).is_err());
        intent.targets.clear();
        assert_eq!(
            publish(&mut state, &mut clients, &intent, true, true, &store),
            Err("no network to post to".into())
        );
        assert_eq!(state.posts.len(), posts);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_dry_run_whose_draft_fails_keeps_its_own_result() {
        let dir = temp("dry-run-unkept");
        let store = dir.join("drafts.json");
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let intent = Intent {
            text: "hi".into(),
            typed: "as typed".into(),
            targets: vec![Network::Bluesky],
            reply_to: None,
            quote: None,
            draft_id: None,
        };
        drafts::fail_after(0);
        assert_eq!(
            publish(
                &mut state,
                &mut Clients::empty(),
                &intent,
                true,
                true,
                &store
            ),
            Ok(())
        );
        assert!(drafts::load_from(&store).unwrap_or_default().is_empty());
        publish(
            &mut state,
            &mut Clients::empty(),
            &intent,
            true,
            true,
            &store,
        )
        .unwrap();
        let kept = drafts::load_from(&store).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].text, "as typed");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_draft_that_fails_before_sending_keeps_the_typed_text_in_the_error() {
        let dir = temp("unrecorded");
        let store = dir.join("drafts.json");
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let posts = state.posts.len();
        let intent = Intent {
            text: "hi".into(),
            typed: "as typed".into(),
            targets: vec![Network::X],
            reply_to: None,
            quote: None,
            draft_id: None,
        };
        drafts::fail_after(0);
        let err = publish(
            &mut state,
            &mut Clients::empty(),
            &intent,
            false,
            true,
            &store,
        )
        .unwrap_err();
        assert!(err.contains("nothing was sent"), "{err}");
        assert!(err.ends_with("\nas typed"), "{err}");
        assert_eq!(state.posts.len(), posts);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn help_json_lists_every_subcommand() {
        let help = help::help_json();
        let listed: Vec<&str> = help["commands"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|command| command["name"].as_str())
            .collect();
        for sub in <Cli as clap::CommandFactory>::command().get_subcommands() {
            assert!(listed.contains(&sub.get_name()), "{}", sub.get_name());
        }
    }
}
