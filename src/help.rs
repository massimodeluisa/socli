//! `socli help`: the full page and one page per command, both drawn from one table.

use serde_json::{json, Value};

use crate::render::{Canvas, Kind, StyledLine};
use crate::screens::{masthead, TAGLINE, VERSION};

const LABEL: usize = 2;
const TEXT: usize = 12;
const NOTE: usize = 28;
const DESCRIPTION: usize = 32;
const EXAMPLE: usize = 69;
/// Wide enough that no command page row is cut; `Canvas::finish` trims the rest.
const PAGE_WIDTH: usize = 160;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Read,
    Write,
    Account,
}

/// One command: its row on the full page and its own page.
struct Command {
    name: &'static str,
    /// Dim after the name on the full page, as in `login <net>`.
    arg: &'static str,
    /// Where the full page lists it; commands without a group only have their own page.
    group: Option<Group>,
    /// The full page's description.
    brief: &'static str,
    summary: &'static str,
    /// Command words, dim arguments, and a dim note at column 28.
    usage: &'static [(&'static str, &'static str, &'static str)],
    flags: &'static [(&'static str, &'static str)],
    /// The first one is also the full page's example.
    examples: &'static [&'static str],
}

type Flag = (&'static str, &'static str);

const NET_READ: Flag = (
    "--net <list>",
    "Networks to read: x, bluesky, devnull, threads, mastodon, or all",
);
const NET_POST: Flag = (
    "--net <list>",
    "Networks to post to: x, bluesky, devnull, threads, mastodon, or all",
);
const NET_ID: Flag = ("--net <list>", "The network of a bare id");
const NET_HANDLE: Flag = ("--net <list>", "The network of the account");
const PRINT: Flag = ("--print", "Print into the scrollback instead of the reader");
const JSON_POSTS: Flag = ("--json", "Print the posts as JSON");
const JSON_RESULT: Flag = ("--json", "Print the result as JSON");
const DEMO_READ: Flag = ("--demo", "Read the offline demo feed");
const DEMO_POST: Flag = ("--demo", "Try it without posting");
const DEMO_WRITE: Flag = ("--demo", "Try it without sending anything");
const RAW: Flag = (
    "--raw",
    "Send <b> and <i> as typed instead of bold and italic letters",
);
const DRY_RUN: Flag = (
    "--dry-run",
    "Check sign-in, caps and limits without sending anything",
);

const READ_FLAGS: &[Flag] = &[NET_READ, PRINT, JSON_POSTS, DEMO_READ];
const POST_FLAGS: &[Flag] = &[NET_POST, RAW, JSON_RESULT, DEMO_POST, DRY_RUN];
const REPLY_FLAGS: &[Flag] = &[NET_ID, RAW, JSON_RESULT, DEMO_POST, DRY_RUN];
const WRITE_FLAGS: &[Flag] = &[NET_ID, JSON_RESULT, DEMO_WRITE, DRY_RUN];
const FOLLOW_FLAGS: &[Flag] = &[NET_HANDLE, JSON_RESULT, DEMO_WRITE, DRY_RUN];
const DELETE_FLAGS: &[Flag] = &[
    NET_ID,
    ("--yes", "Delete without asking; needed without a terminal"),
    JSON_RESULT,
    DEMO_WRITE,
    DRY_RUN,
];

const COMMANDS: &[Command] = &[
    Command {
        name: "feed",
        arg: "",
        group: Some(Group::Read),
        brief: "Home timeline, or latest posts",
        summary: "Your home timeline, merged or one network, or one feed by name",
        usage: &[
            ("socli feed", "[<name>] [flags]", ""),
            (
                "socli feed",
                "<name>",
                "following, latest, trending, discover, saved, mine, notifications",
            ),
        ],
        flags: &[
            NET_READ,
            PRINT,
            JSON_POSTS,
            ("--count <n>", "How many posts to ask each network for"),
            DEMO_READ,
        ],
        examples: &[
            "socli feed latest",
            "socli feed saved --net bluesky",
            "socli feed --json | jq '.[0]'",
        ],
    },
    Command {
        name: "thread",
        arg: "",
        group: Some(Group::Read),
        brief: "A post with its replies",
        summary: "One conversation, from a scoped id or a post link",
        usage: &[("socli thread", "<id|url> [flags]", "")],
        flags: &[PRINT, JSON_POSTS, DEMO_READ],
        examples: &[
            "socli thread <id>",
            "socli thread x:1849213377 --print",
            "socli thread https://bsky.app/profile/bsky.app/post/3l…",
        ],
    },
    Command {
        name: "search",
        arg: "",
        group: Some(Group::Read),
        brief: "Recent public search",
        summary: "Search the networks in scope that can search",
        usage: &[("socli search", "<words> [flags]", "")],
        flags: &[
            (
                "--net <list>",
                "Networks to search: x, bluesky, devnull, threads, mastodon, or all",
            ),
            PRINT,
            JSON_POSTS,
            DEMO_READ,
        ],
        examples: &[
            "socli search rust tui",
            "socli search ratatui --net bluesky",
            "socli search rust tui --json",
        ],
    },
    Command {
        name: "user",
        arg: "",
        group: Some(Group::Read),
        brief: "Public posts from one account",
        summary: "Public posts from one account",
        usage: &[("socli user", "<@handle> [flags]", "")],
        flags: &[NET_HANDLE, PRINT, JSON_POSTS, DEMO_READ],
        examples: &["socli user @handle", "socli user @bsky.app --net bluesky"],
    },
    Command {
        name: "notifications",
        arg: "",
        group: Some(Group::Read),
        brief: "Posts that mention you",
        summary: "Posts that mention you, in the notification list",
        usage: &[("socli notifications", "[flags]", "")],
        flags: READ_FLAGS,
        examples: &[
            "socli notifications",
            "socli notifications --net bluesky --print",
        ],
    },
    Command {
        name: "bookmarks",
        arg: "",
        group: Some(Group::Read),
        brief: "Posts you saved",
        summary: "Posts you saved, on the networks that keep bookmarks",
        usage: &[("socli bookmarks", "[flags]", "")],
        flags: READ_FLAGS,
        examples: &["socli bookmarks", "socli bookmarks --json"],
    },
    Command {
        name: "ask",
        arg: "",
        group: Some(Group::Read),
        brief: "Ask about the posts already loaded",
        summary: "Read the posts already loaded. Does not call a model",
        usage: &[("socli ask", "[flags]", "")],
        flags: &[NET_READ, DEMO_READ],
        examples: &["socli ask", "socli ask --demo"],
    },
    Command {
        name: "post",
        arg: "",
        group: Some(Group::Write),
        brief: "Publish a post",
        summary: "Publish a post on your default network, or on the --net networks",
        usage: &[
            ("socli post", "<text> [flags]", ""),
            ("echo \"<text>\" | socli post -", "", ""),
            ("socli post", "", "opens the composer when no text is given"),
        ],
        flags: POST_FLAGS,
        examples: &[
            "socli post \"hi from a terminal\"",
            "socli post \"<b>Hello</b> world!\" --net x,bluesky",
            "git log -1 --format=%s | socli post - --net devnull",
        ],
    },
    Command {
        name: "reply",
        arg: "",
        group: Some(Group::Write),
        brief: "Reply to a post",
        summary: "Reply to a post on its own network",
        usage: &[
            ("socli reply", "<id|url> <text> [flags]", ""),
            ("echo \"<text>\" | socli reply", "<id> -", ""),
            (
                "socli reply",
                "<id>",
                "opens the composer when no text is given",
            ),
        ],
        flags: REPLY_FLAGS,
        examples: &[
            "socli reply <id> \"nice\"",
            "socli reply x:1849213377 \"<i>agreed</i>\"",
        ],
    },
    Command {
        name: "quote",
        arg: "",
        group: Some(Group::Write),
        brief: "Quote a post",
        summary: "Quote a post, or post its link with your text on other networks",
        usage: &[
            ("socli quote", "<id|url> <text> [flags]", ""),
            ("echo \"<text>\" | socli quote", "<id> -", ""),
            (
                "socli quote",
                "<id>",
                "opens the composer when no text is given",
            ),
        ],
        flags: &[NET_POST, RAW, JSON_RESULT, DEMO_POST],
        examples: &[
            "socli quote <id> \"this\"",
            "socli quote x:1849213377 \"this\" --net x,bluesky",
        ],
    },
    Command {
        name: "like",
        arg: "",
        group: Some(Group::Write),
        brief: "Like a post (unlike to undo)",
        summary: "Like a post",
        usage: &[("socli like", "<id|url> [flags]", "")],
        flags: WRITE_FLAGS,
        examples: &["socli like <id>", "socli like p3 --net devnull"],
    },
    Command {
        name: "unlike",
        arg: "",
        group: None,
        brief: "Remove a like",
        summary: "Remove a like",
        usage: &[("socli unlike", "<id|url> [flags]", "")],
        flags: WRITE_FLAGS,
        examples: &["socli unlike <id>"],
    },
    Command {
        name: "repost",
        arg: "",
        group: Some(Group::Write),
        brief: "Repost a post (unrepost to undo)",
        summary: "Repost a post",
        usage: &[("socli repost", "<id|url> [flags]", "")],
        flags: WRITE_FLAGS,
        examples: &["socli repost <id>"],
    },
    Command {
        name: "unrepost",
        arg: "",
        group: None,
        brief: "Undo a repost",
        summary: "Undo a repost",
        usage: &[("socli unrepost", "<id|url> [flags]", "")],
        flags: WRITE_FLAGS,
        examples: &["socli unrepost <id>"],
    },
    Command {
        name: "bookmark",
        arg: "",
        group: Some(Group::Write),
        brief: "Save a post (unbookmark to undo)",
        summary: "Save a post to your bookmarks",
        usage: &[("socli bookmark", "<id|url> [flags]", "")],
        flags: WRITE_FLAGS,
        examples: &["socli bookmark <id>"],
    },
    Command {
        name: "unbookmark",
        arg: "",
        group: None,
        brief: "Remove a bookmark",
        summary: "Remove a bookmark",
        usage: &[("socli unbookmark", "<id|url> [flags]", "")],
        flags: WRITE_FLAGS,
        examples: &["socli unbookmark <id>"],
    },
    Command {
        name: "share",
        arg: "",
        group: Some(Group::Write),
        brief: "Copy a post's link",
        summary: "Copy a post's link, or its author's profile link; in a pipe or for an agent, only print it",
        usage: &[("socli share", "<id|url> [flags]", "")],
        flags: &[
            ("--profile", "Copy the author's profile link instead"),
            NET_ID,
            ("--json", "Print the link as JSON"),
            ("--demo", "Share a post from the offline demo feed"),
        ],
        examples: &["socli share 1848213…", "socli share x:p1 --profile --demo"],
    },
    Command {
        name: "follow",
        arg: "",
        group: Some(Group::Write),
        brief: "Follow an account (unfollow to undo)",
        summary: "Follow an account",
        usage: &[("socli follow", "<@handle> [flags]", "")],
        flags: FOLLOW_FLAGS,
        examples: &[
            "socli follow @handle",
            "socli follow @bsky.app --net bluesky",
        ],
    },
    Command {
        name: "unfollow",
        arg: "",
        group: None,
        brief: "Unfollow an account",
        summary: "Unfollow an account",
        usage: &[("socli unfollow", "<@handle> [flags]", "")],
        flags: FOLLOW_FLAGS,
        examples: &["socli unfollow @handle"],
    },
    Command {
        name: "delete",
        arg: "",
        group: Some(Group::Write),
        brief: "Delete one of your posts",
        summary: "Delete one of your posts",
        usage: &[("socli delete", "<id|url> [flags]", "")],
        flags: DELETE_FLAGS,
        examples: &["socli delete <id>"],
    },
    Command {
        name: "view",
        arg: "",
        group: None,
        brief: "Show a post's images in the terminal",
        summary: "Show a post's images in the terminal",
        usage: &[("socli view", "<post id|post link|image url|file|demo>", "")],
        flags: &[
            ("--images[=<mode>]", "auto, kitty, iterm, sixel, blocks, or off"),
            ("--json", "Print the image URLs as JSON instead"),
            DEMO_READ,
        ],
        examples: &["socli view demo", "socli view x:1848213… --json"],
    },
    Command {
        name: "login",
        arg: "<net>",
        group: Some(Group::Account),
        brief: "Sign in to a network",
        summary: "Sign in to X, Bluesky, dev/null, Threads or Mastodon",
        usage: &[(
            "socli login",
            "<net> [flags]",
            "x, bluesky, devnull, threads or mastodon",
        )],
        flags: &[
            ("--oauth", "X: OAuth 2.0 with a developer app, instead of the browser session"),
            ("--browser <name>", "X: Chrome, Brave, Edge, Arc or Chromium"),
            (
                "--profile <name>",
                "X: the browser profile to use, instead of asking",
            ),
            ("--client-id <id>", "X: your app's OAuth client id"),
            ("--client-secret <secret>", "X: your app's client secret"),
            ("--scopes <list>", "X: the OAuth scopes to ask for"),
            ("--port <n>", "X: the local callback port, 8741 by default"),
            (
                "--handle <handle>",
                "Bluesky: your handle, else BLUESKY_HANDLE or a prompt",
            ),
            (
                "--password-stdin",
                "Bluesky: read the app password from stdin, else BLUESKY_APP_PASSWORD or a prompt",
            ),
            (
                "--service <url>",
                "Bluesky: your server, when it is not bsky.social",
            ),
            (
                "--device-name <name>",
                "dev/null: a name for the new device key",
            ),
            (
                "--token <token>",
                "Threads: a Meta app token. Mastodon: one from Preferences > Development",
            ),
            (
                "--server <host>",
                "Mastodon: your server, else MASTODON_SERVER or a prompt (mastodon.social when left empty)",
            ),
            ("--app-secret <secret>", "Threads: your Meta app's secret"),
        ],
        examples: &[
            "socli login x",
            "socli login bluesky",
            "socli login bluesky --handle <handle> --password-stdin",
            "socli login x --oauth --client-id <id>",
            "socli login threads",
            "socli login threads --token <token>",
            "socli login mastodon --server fosstodon.org",
        ],
    },
    Command {
        name: "logout",
        arg: "<net>",
        group: Some(Group::Account),
        brief: "Sign out of one network, or all",
        summary: "Sign out of one network, or all of them",
        usage: &[("socli logout", "<net|all>", "")],
        flags: &[],
        examples: &["socli logout bluesky", "socli logout all"],
    },
    Command {
        name: "whoami",
        arg: "",
        group: Some(Group::Account),
        brief: "Accounts this process will use",
        summary: "The accounts this process will use",
        usage: &[("socli whoami", "", "")],
        flags: &[],
        examples: &["socli whoami"],
    },
    Command {
        name: "auth",
        arg: "",
        group: Some(Group::Account),
        brief: "Status, default network, tokens",
        summary: "Status, the default network, and each network's sign-in",
        usage: &[
            ("socli auth status", "", ""),
            ("socli auth default", "<net>", ""),
            (
                "socli auth",
                "<net> <command>",
                "login, logout, and each network's tokens",
            ),
        ],
        flags: &[],
        examples: &[
            "socli auth status",
            "socli auth default bluesky",
            "socli auth x bearer <token>",
        ],
    },
    Command {
        name: "config",
        arg: "",
        group: Some(Group::Account),
        brief: "Read or change settings",
        summary: "Read or change the settings in config.toml",
        usage: &[
            ("socli config", "[list]", ""),
            ("socli config get", "<key>", ""),
            ("socli config set", "<key> <value>", ""),
            ("socli config path", "", ""),
            ("socli config themes", "", ""),
        ],
        flags: &[],
        examples: &[
            "socli config set ui.theme dark",
            "socli config get ui.density",
            "socli config themes",
            "socli config path",
        ],
    },
    Command {
        name: "drafts",
        arg: "",
        group: Some(Group::Account),
        brief: "List saved drafts",
        summary: "List saved drafts, or remove one",
        usage: &[("socli drafts", "", ""), ("socli drafts rm", "<n>", "")],
        flags: &[],
        examples: &["socli drafts", "socli drafts rm 2"],
    },
    Command {
        name: "completions",
        arg: "",
        group: None,
        brief: "Shell completions",
        summary: "Shell completions",
        usage: &[(
            "socli completions",
            "<shell>",
            "bash, zsh, fish, elvish or powershell",
        )],
        flags: &[],
        examples: &["socli completions zsh > ~/.zfunc/_socli"],
    },
    Command {
        name: "help",
        arg: "",
        group: None,
        brief: "The full help page, or one command's page",
        summary: "The full help page, or one command's page",
        usage: &[
            ("socli help", "[<command>]", ""),
            ("socli", "<command> --help", ""),
        ],
        flags: &[],
        examples: &["socli help", "socli help post", "socli post -h"],
    },
];

const FLAGS: &[(&str, &str, &str)] = &[
    ("--json", "JSON for a pipe", "socli feed --json"),
    (
        "--net <list>",
        "Networks to read or post to",
        "socli post \"hi\" --net x,bluesky",
    ),
    ("--raw", "Send <b> and <i> as typed", ""),
    ("--print", "Print into the scrollback", "socli --print"),
    ("--demo", "Offline demo feed", "socli --demo"),
    ("--theme <name>", "a name from socli config themes", ""),
    ("--density <name>", "compact, cozy or roomy", ""),
    ("--detail <level>", "plain, minimal, medium or rich", ""),
    ("--layout <name>", "auto, single or split", ""),
    (
        "--mouse <on|off>",
        "Clicks, wheel and hover in the reader",
        "",
    ),
    ("--mono", "Black and white only", ""),
    ("--images[=<mode>]", "auto|kitty|iterm|sixel|blocks|off", ""),
    ("--icons <name>", "auto, nerd or unicode", ""),
    ("--no-icons", "Unicode icons, no Nerd Font needed", ""),
];

/// Flags the JSON help lists besides the full page's.
const MORE_FLAGS: &[Flag] = &[
    DRY_RUN,
    (
        "--no-prompt",
        "Do not open the socli> prompt after printing",
    ),
    (
        "--count <n>",
        "How many posts to ask each network for, 15 by default",
    ),
    (
        "--width <n>",
        "Wrap width for --print, the terminal width by default",
    ),
    ("--help", "The full help page, or one command's page"),
];

const EXIT_CODES: [(&str, &str); 3] = [
    ("0", "success"),
    ("1", "a network or target failed"),
    (
        "2",
        "a usage error, or a step that needs a person at a terminal",
    ),
];

const ENVIRONMENT: &[(&str, &str)] = &[
    (
        "SOCLI_NET",
        "Networks to read or post to when --net is absent",
    ),
    ("SOCLI_CONFIG_DIR", "The directory that holds config.toml"),
    (
        "SOCLI_AGENT",
        "1 forces agent mode (JSON output, no prompts), 0 turns it off",
    ),
    (
        "SOCLI_IMAGE",
        "Images when --images is absent: auto, kitty, iterm, sixel, blocks, or off",
    ),
    (
        "SOCLI_DETAIL",
        "Detail level when --detail is absent: plain, minimal, medium or rich (standard is the old name for medium). Unset, a normal terminal uses rich and an ESP or other low-resource terminal uses plain",
    ),
    (
        "SOCLI_LAYOUT",
        "Layout when --layout is absent: auto, single or split",
    ),
    ("SOCLI_MOUSE", "Mouse when --mouse is absent: on or off"),
    ("SOCLI_HYPERLINKS", "1 turns terminal hyperlinks on, 0 off"),
    ("SOCLI_TRUECOLOR", "1 turns 24-bit color on, 0 off"),
    (
        "SOCLI_TRACE_FRAMES",
        "Append one timing line per drawn frame to this file",
    ),
    ("NO_COLOR", "Any value turns color off"),
    (
        "NO_ICONS",
        "Any value picks Unicode icons over Nerd Font glyphs",
    ),
];

/// A flag's name and its value, split at the first space or `[`.
fn split_flag(name: &str) -> (&str, &str) {
    name.split_at(name.find([' ', '[']).unwrap_or(name.len()))
}

/// A flag at `col`: the name in Fg and its value in Dim.
fn flag(canvas: &mut Canvas, y: usize, col: usize, name: &str) -> usize {
    let (name, value) = split_flag(name);
    let x = canvas.put(y, col, Kind::Fg, name);
    canvas.put(y, x, Kind::Dim, value)
}

/// A full page row's description after the name ending at `end`, and its example at column 69.
fn entry(canvas: &mut Canvas, y: usize, end: usize, description: &str, example: Option<&str>) {
    canvas.put(y, end.max(DESCRIPTION - 2) + 2, Kind::Dim, description);
    if let Some(example) = example.filter(|e| !e.is_empty()) {
        canvas.put(y, EXAMPLE, Kind::Dim, example);
    }
}

/// `socli help` and `socli --help`. Below 100 columns the example column is left out.
pub fn help_lines(width: usize) -> Vec<StyledLine> {
    let examples = width >= 100;
    let mut canvas = Canvas::new(width.max(40));
    let blank: &[(Kind, &str)] = &[];
    masthead(
        &mut canvas,
        &[
            blank,
            &[(Kind::Bold, "socli"), (Kind::Fg, " "), (Kind::Dim, VERSION)],
            &[(Kind::Dim, TAGLINE)],
            &[
                (Kind::Bold, "Usage"),
                (Kind::Fg, "  socli "),
                (Kind::Dim, "<command> [flags]"),
            ],
            blank,
        ],
    );
    canvas.push(false);
    for (label, group) in [
        ("Read", Group::Read),
        ("Write", Group::Write),
        ("Account", Group::Account),
    ] {
        let listed = COMMANDS.iter().filter(|c| c.group == Some(group));
        for (i, command) in listed.enumerate() {
            let y = canvas.push(false);
            let mut end = canvas.put(y, TEXT, Kind::Fg, command.name);
            if !command.arg.is_empty() {
                end = canvas.put(y, end + 1, Kind::Dim, command.arg);
            }
            if i == 0 {
                canvas.put(y, LABEL, Kind::Bold, label);
            }
            let example = command.examples.first().copied().unwrap_or("");
            entry(
                &mut canvas,
                y,
                end,
                command.brief,
                examples.then_some(example),
            );
        }
        canvas.push(false);
    }
    for (i, (name, description, example)) in FLAGS.iter().enumerate() {
        let y = canvas.push(false);
        if i == 0 {
            canvas.put(y, LABEL, Kind::Bold, "Flags");
        }
        let end = flag(&mut canvas, y, TEXT, name);
        entry(
            &mut canvas,
            y,
            end,
            description,
            examples.then_some(*example),
        );
    }
    for (arg, description) in [
        ("<command>", "for one command's flags and examples"),
        ("--json", "every command, flag and example as JSON"),
    ] {
        let y = canvas.push(false);
        let x = canvas.put(y, LABEL, Kind::Fg, "socli help");
        canvas.put(y, x + 1, Kind::Dim, arg);
        canvas.put(y, DESCRIPTION, Kind::Dim, description);
    }
    canvas.finish()
}

// MARK: JSON

/// `socli help --json`: every command, flag and example, the exit codes and the environment.
pub fn help_json() -> Value {
    let global = FLAGS
        .iter()
        .map(|(name, description, _)| (*name, *description))
        .chain(MORE_FLAGS.iter().copied());
    json!({
        "name": "socli",
        "version": VERSION,
        "summary": TAGLINE,
        "commands": COMMANDS.iter().map(command_value).collect::<Vec<_>>(),
        "global_flags": global.map(flag_value).collect::<Vec<_>>(),
        "exit_codes": EXIT_CODES
            .iter()
            .map(|(code, meaning)| (code.to_string(), json!(meaning)))
            .collect::<serde_json::Map<_, _>>(),
        "environment": ENVIRONMENT
            .iter()
            .map(|(name, description)| json!({ "name": name, "description": description }))
            .collect::<Vec<_>>(),
    })
}

/// `socli help <command> --json`, or None when no command has that name.
pub fn command_json(name: &str) -> Option<Value> {
    COMMANDS.iter().find(|c| c.name == name).map(command_value)
}

fn command_value(command: &Command) -> Value {
    let usage: Vec<String> = command
        .usage
        .iter()
        .map(|(words, args, _)| [*words, *args].join(" ").trim_end().to_string())
        .collect();
    json!({
        "name": command.name,
        "summary": command.summary,
        "usage": usage,
        "flags": command.flags.iter().copied().map(flag_value).collect::<Vec<_>>(),
        "examples": command.examples,
    })
}

fn flag_value((name, description): Flag) -> Value {
    let (name, value) = split_flag(name);
    let value = Some(value.trim()).filter(|value| !value.is_empty());
    json!({ "flag": name, "value": value, "description": description })
}

/// `socli help <command>` and `socli <command> --help`, or None when no command has that name.
pub fn command_lines(name: &str) -> Option<Vec<StyledLine>> {
    let command = COMMANDS.iter().find(|c| c.name == name)?;
    let mut canvas = Canvas::new(PAGE_WIDTH);
    canvas.push(false);
    let y = canvas.push(false);
    let x = canvas.put(y, LABEL, Kind::Bold, &format!("socli {}", command.name));
    canvas.put(y, x + 2, Kind::Dim, command.summary);
    canvas.push(false);
    for (i, (words, args, note)) in command.usage.iter().enumerate() {
        let y = canvas.push(false);
        if i == 0 {
            canvas.put(y, LABEL, Kind::Bold, "Usage");
        }
        let mut x = canvas.put(y, TEXT, Kind::Fg, words);
        if !args.is_empty() {
            x = canvas.put(y, x + 1, Kind::Dim, args);
        }
        if !note.is_empty() {
            canvas.put(y, x.max(NOTE - 2) + 2, Kind::Dim, note);
        }
    }
    canvas.push(false);
    for (i, (name, description)) in command.flags.iter().enumerate() {
        let y = canvas.push(false);
        if i == 0 {
            canvas.put(y, LABEL, Kind::Bold, "Flags");
        }
        let x = flag(&mut canvas, y, TEXT, name);
        canvas.put(y, x.max(NOTE - 2) + 2, Kind::Dim, description);
    }
    let y = canvas.push(false);
    if command.flags.is_empty() {
        canvas.put(y, LABEL, Kind::Bold, "Flags");
    }
    canvas.put(y, TEXT, Kind::Dim, "Global flags: socli help");
    canvas.push(false);
    for (i, example) in command.examples.iter().enumerate() {
        let y = canvas.push(false);
        if i == 0 {
            canvas.put(y, LABEL, Kind::Bold, "Examples");
        }
        canvas.put(y, TEXT, Kind::Fg, example);
    }
    canvas.push(false);
    Some(canvas.finish())
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::*;

    fn plain(lines: &[StyledLine]) -> Vec<String> {
        lines.iter().map(StyledLine::plain).collect()
    }

    #[test]
    fn help_drops_examples_below_100_columns() {
        let wide = plain(&help_lines(100));
        let narrow = plain(&help_lines(80));
        assert!(wide.iter().any(|l| l.contains("socli feed latest")));
        assert!(!narrow.iter().any(|l| l.contains("socli feed latest")));
        let feed = wide.iter().find(|l| l.contains("Read")).unwrap();
        assert_eq!(feed.find("feed"), Some(12));
        assert!(wide.iter().all(|l| l.width() <= 100));
        assert!(wide
            .iter()
            .any(|l| l.ends_with("socli post \"hi from a terminal\"")));
        let rows = wide
            .iter()
            .filter(|l| !l.starts_with("  socli help") && !l.contains(TAGLINE));
        for line in rows.filter(|l| l.chars().count() > 69) {
            assert_eq!(line.chars().nth(68), Some(' '), "{line}");
        }
    }

    #[test]
    fn full_page_lists_share_raw_and_the_command_footer() {
        let wide = plain(&help_lines(100));
        let share = wide
            .iter()
            .find(|l| l.contains("Copy a post's link"))
            .unwrap();
        assert_eq!(share.find("share"), Some(12));
        assert_eq!(share.find("Copy"), Some(32));
        assert!(share.ends_with("socli share 1848213…"));
        assert!(wide
            .iter()
            .any(|l| l.contains("--raw") && l.contains("Send <b> and <i> as typed")));
        assert!(wide.iter().any(|l| l.contains("--images[=<mode>]")));
        let footer: Vec<&String> = wide
            .iter()
            .filter(|l| l.starts_with("  socli help"))
            .collect();
        assert_eq!(
            footer,
            [
                "  socli help <command>          for one command's flags and examples",
                "  socli help --json             every command, flag and example as JSON",
            ]
        );
        assert_eq!(wide.last(), Some(footer[1]));
    }

    #[test]
    fn every_command_page_keeps_the_grid() {
        for command in COMMANDS {
            let page = plain(&command_lines(command.name).unwrap());
            assert_eq!(page[1].find(&format!("socli {}", command.name)), Some(2));
            for label in ["Usage", "Flags", "Examples"] {
                let row = page.iter().find(|l| l.starts_with(&format!("  {label}")));
                let row = row.unwrap_or_else(|| panic!("{} has no {label}", command.name));
                assert_eq!(row.chars().nth(TEXT - 1), Some(' '), "{row}");
                assert_ne!(row.chars().nth(TEXT), Some(' '), "{row}");
            }
            assert!(page
                .iter()
                .any(|l| l.find("Global flags: socli help") == Some(TEXT)));
        }
        assert!(command_lines("nope").is_none());
    }
}
