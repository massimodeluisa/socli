use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const CREDENTIALS: &[&str] = &[
    "X_AUTH_TOKEN",
    "X_CT0",
    "X_CLIENT_ID",
    "X_CLIENT_SECRET",
    "X_ACCESS_TOKEN",
    "X_REFRESH_TOKEN",
    "X_USER_ID",
    "X_USERNAME",
    "X_BEARER_TOKEN",
    "BLUESKY_HANDLE",
    "BLUESKY_APP_PASSWORD",
    "BLUESKY_SERVICE",
    "THREADS_ACCESS_TOKEN",
    "THREADS_APP_ID",
    "THREADS_APP_SECRET",
    "MASTODON_ACCESS_TOKEN",
    "MASTODON_SERVER",
];

/// An empty config directory for this test process; no test writes to it.
fn empty_config() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("socli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn socli() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_socli"));
    cmd.env("SOCLI_CONFIG_DIR", empty_config())
        .env("NO_COLOR", "1")
        .env("TERM_PROGRAM", "vscode")
        .env("TERM", "xterm-256color")
        .env("SSH_TTY", "1")
        .env("SOCLI_AGENT", "0")
        .env_remove("SOCLI_ICONS")
        .env_remove("NO_ICONS")
        .env_remove("SOCLI_NET")
        .env_remove("SOCLI_THEME")
        .env_remove("SOCLI_DENSITY")
        .env_remove("SOCLI_DETAIL")
        .env_remove("SOCLI_LAYOUT")
        .env_remove("SOCLI_MOUSE")
        .env_remove("SOCLI_IMAGE")
        .env_remove("SOCLI_TRUECOLOR")
        .env_remove("SOCLI_TRACE_FRAMES")
        .env_remove("COLORFGBG");
    for key in CREDENTIALS {
        cmd.env_remove(key);
    }
    cmd
}

/// A fresh config directory that one test owns and may write to.
fn own_config(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("socli-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn stderr_of(out: Output) -> String {
    assert!(
        !out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8(out.stderr).unwrap()
}

fn stdout_of(out: Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn demo_print(width: &str, extra: &[&str]) -> String {
    let out = socli()
        .args(["--demo", "--print", "--no-prompt", "--width", width])
        .args(extra)
        .output()
        .unwrap();
    stdout_of(out)
}

fn private_use(c: char) -> bool {
    matches!(c as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD)
}

#[test]
fn configuration_errors_stay_failures_and_do_not_print_secrets() {
    let dir = own_config("Broken pipe");
    std::fs::write(
        dir.join("config.toml"),
        "[x]\naccess_token = [\"private-token\"]\n",
    )
    .unwrap();
    let out = socli()
        .env("SOCLI_CONFIG_DIR", &dir)
        .args(["config", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(error["error"].as_str().unwrap().contains("not valid toml"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-token"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn print_lays_a_narrow_width_out_at_the_minimum_and_draws_to_the_edge() {
    for (asked, shown) in [("10", 24), ("24", 24), ("40", 40), ("59", 59)] {
        let text = demo_print(asked, &[]);
        let widest = text.lines().map(|line| line.chars().count()).max().unwrap();
        assert!(widest <= shown, "{asked}: {widest}\n{text}");
        assert!(
            text.lines().any(|line| line.starts_with("SoCLI")),
            "{asked}\n{text}"
        );
    }
    let wide = demo_print("60", &["--detail", "rich"]);
    assert!(
        wide.lines().any(|line| line.starts_with("  SoCLI")),
        "{wide}"
    );
    let medium = demo_print("104", &["--detail", "medium"]);
    let lines: Vec<&str> = medium.lines().collect();
    assert!(lines[0].starts_with("SoCLI › all › home"), "{medium}");
    assert!(lines[0].ends_with("Accounts   ⚑ 2"), "{medium}");
    assert!(
        lines.get(2).is_some_and(|line| line.starts_with("[All]")),
        "{medium}"
    );
    assert!(
        lines.get(3).is_some_and(|line| line.starts_with('─')),
        "{medium}"
    );
    assert!(!medium.contains("│ N │"), "{medium}");
}

#[test]
fn demo_print_merges_every_network() {
    let text = demo_print("72", &[]);
    assert!(text.contains("SoCLI › all › home"), "{text}");
    assert!(text.contains("@mina"));
    assert!(text.contains("@nuno"));
    assert!(text.contains("Show 2 more replies"), "{text}");
    assert!(text.contains("╭"));
    assert!(!text.contains("@lea"), "the tail stays folded: {text}");
    for tag in ["· x", "· bluesky", "· dev/null", "· threads"] {
        assert!(text.contains(tag), "{tag}: {text}");
    }
}

#[test]
fn demo_print_draws_medium_chrome_to_the_edge() {
    let text = demo_print("104", &["--detail", "medium"]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("SoCLI › all › home"), "{text}");
    assert!(lines[0].ends_with("Accounts   ⚑ 2"), "{text}");
    assert!(lines[1].is_empty());
    assert_eq!(
        lines[2],
        "[All]  ● X  ● Bluesky  ● dev/null  ● Threads  ● Mastodon                      │  [Home]  Discover  Saved"
    );
    assert_eq!(lines[3], "─".repeat(104));
    assert!(lines[4].is_empty());
    let fold = lines
        .iter()
        .find(|l| l.contains("Show 2 more replies"))
        .unwrap();
    assert_eq!(fold.chars().position(|c| c == '○'), Some(9), "{fold}");
    assert!(!text.contains("│ N │"));
    assert!(text.contains("├╌╌╌╌╌╎"), "{text}");
    assert!(text.contains("╰╌╌╌╌╌╎"), "{text}");
    assert!(
        text.contains("│ SoCLI A thread-first terminal client for X, Bluesky"),
        "{text}"
    );
    assert!(text.contains("○ Show this thread"), "{text}");
    assert!(
        text.contains("SoCLI› [Home]  Notifications 2  Search"),
        "{text}"
    );
    assert!(text.contains("j/k move  ⏎ open"), "{text}");
}

#[test]
fn an_unset_detail_follows_the_opening_client() {
    let rich = demo_print("104", &[]);
    assert!(
        rich.lines()
            .next()
            .is_some_and(|line| line.starts_with("  SoCLI")),
        "{rich}"
    );
    assert!(!rich.is_ascii(), "{rich}");
    assert_eq!(
        stdout_of(
            socli()
                .args(["config", "get", "ui.detail"])
                .output()
                .unwrap()
        )
        .trim(),
        "rich"
    );

    let plain = stdout_of(
        socli()
            .env("TERM", "dumb")
            .args(["--demo", "--print", "--no-prompt", "--width", "104"])
            .output()
            .unwrap(),
    );
    assert!(plain.contains("socli"), "{plain}");
    assert!(
        !plain.contains(['╭', '›', '│', '▍']),
        "plain drops the rich chrome: {plain}"
    );
    assert_eq!(
        stdout_of(
            socli()
                .env("TERM", "espterm")
                .env("TERM_PROGRAM", "ESPTerm")
                .args(["config", "get", "ui.detail"])
                .output()
                .unwrap()
        )
        .trim(),
        "plain"
    );

    let kept = stdout_of(
        socli()
            .env("TERM", "vt102")
            .env("SOCLI_DETAIL", "medium")
            .args(["--demo", "--print", "--no-prompt", "--width", "104"])
            .output()
            .unwrap(),
    );
    assert!(
        kept.lines()
            .next()
            .is_some_and(|line| line.starts_with("SoCLI ›")),
        "{kept}"
    );

    let dir = own_config("detail-client");
    std::fs::write(dir.join("config.toml"), "[ui]\ndetail = \"rich\"\n").unwrap();
    let saved = stdout_of(
        socli()
            .env("SOCLI_CONFIG_DIR", &dir)
            .env("TERM", "dumb")
            .args(["--demo", "--print", "--no-prompt", "--width", "104"])
            .output()
            .unwrap(),
    );
    assert!(
        saved
            .lines()
            .next()
            .is_some_and(|line| line.starts_with("  SoCLI")),
        "{saved}"
    );
}

#[test]
fn stats_cells_follow_each_network() {
    let text = demo_print("104", &["--detail", "rich"]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.contains(&"     ╎    ↩ 5             ♡ 86"),
        "dev/null leaves the repost column empty and has no bookmark cell: {text}"
    );
    assert!(
        lines.contains(&"          ↩       ⇄"),
        "threads shows reply and repost icons only: {text}"
    );
}

#[test]
fn one_network_scope_shows_its_tabs_and_no_tags() {
    let text = demo_print("104", &["--detail", "rich", "--net", "bluesky"]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("  SoCLI › bluesky › following"),
        "{text}"
    );
    assert_eq!(
        lines[2],
        "  All  ○ X  [● Bluesky]  ○ dev/null  ○ Threads  ○ Mastodon         │  [Following]  Discover  Bookmarks"
    );
    assert!(text.contains("@ada.bsky.social"), "{text}");
    assert!(
        !text.contains("@nuno") && !text.contains("@mina "),
        "{text}"
    );
    assert!(!text.contains("· bluesky"), "{text}");

    let out = socli()
        .env("SOCLI_NET", "bluesky")
        .args([
            "--demo",
            "--print",
            "--no-prompt",
            "--detail",
            "rich",
            "--width",
            "104",
        ])
        .output()
        .unwrap();
    assert!(stdout_of(out).starts_with("  SoCLI › bluesky › following"));
}

#[test]
fn a_network_list_merges_only_those_networks() {
    let text = demo_print("104", &["--detail", "rich", "--net", "x,bluesky"]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("  SoCLI › all › home"), "{text}");
    assert_eq!(
        lines[2],
        "  All  ● X  ● Bluesky  ○ dev/null  ○ Threads  ○ Mastodon                    │  [Home]  Discover  Saved"
    );
    assert!(text.contains("· x") && text.contains("· bluesky"), "{text}");
    assert!(
        !text.contains("· dev/null") && !text.contains("· threads"),
        "{text}"
    );
}

#[test]
fn demo_json_carries_network_and_url() {
    let out = socli()
        .args(["--demo", "--json", "--no-prompt"])
        .output()
        .unwrap();
    let posts: serde_json::Value = serde_json::from_str(&stdout_of(out)).unwrap();
    let posts = posts.as_array().unwrap();
    assert!(posts.len() > 10);
    for post in posts {
        let id = post["id"].as_str().unwrap();
        let net = post["network"].as_str().unwrap();
        assert!(id.starts_with(&format!("{net}:")), "{post}");
        assert!(
            post["url"].as_str().unwrap().starts_with("https://"),
            "{post}"
        );
    }
    assert!(posts.iter().any(|p| p["author"]["username"] == "mina"));
}

#[test]
fn demo_post_reports_each_target_network() {
    let out = socli()
        .args(["--demo", "post", "hi", "--net", "x,bluesky"])
        .output()
        .unwrap();
    assert_eq!(
        stdout_of(out),
        "✓ X        demo · not posted\n✓ Bluesky  demo · not posted\n"
    );
    let out = socli().args(["--demo", "post", "hi"]).output().unwrap();
    assert_eq!(stdout_of(out), "✓ X  demo · not posted\n");
}

#[test]
fn demo_writes_target_the_pinned_post() {
    let out = socli().args(["--demo", "like", "x:p1"]).output().unwrap();
    assert_eq!(stdout_of(out), "✓ X  demo · liked here only\n");
    let out = socli()
        .args(["--demo", "like", "https://dev-null.sh/@priya/commits/p3"])
        .output()
        .unwrap();
    assert_eq!(stdout_of(out), "✓ dev/null  demo · starred here only\n");
    let out = socli()
        .args(["--demo", "like", "p3", "--net", "devnull"])
        .output()
        .unwrap();
    assert_eq!(stdout_of(out), "✓ dev/null  demo · starred here only\n");
}

#[test]
fn x_has_no_trending_feed() {
    let message = "X has no trending feed in SoCLI yet. Try `socli search <words> --net x`";
    for (format, stderr) in [
        ("--print", format!("socli: {message}\n")),
        (
            "--json",
            format!("{}\n", serde_json::json!({ "error": message })),
        ),
    ] {
        let out = socli()
            .args(["--demo", "feed", "trending", "--net", "x", format])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert_eq!(String::from_utf8(out.stderr).unwrap(), stderr);
    }
    let out = socli()
        .args(["--demo", "feed", "trending", "--json"])
        .output()
        .unwrap();
    let posts: serde_json::Value = serde_json::from_str(&stdout_of(out)).unwrap();
    let posts = posts.as_array().unwrap();
    assert!(!posts.is_empty());
    let networks: std::collections::BTreeSet<&str> =
        posts.iter().filter_map(|p| p["network"].as_str()).collect();
    assert_eq!(networks, ["devnull", "mastodon"].into(), "{posts:?}");
}

#[test]
fn a_closed_pipe_ends_output_quietly() {
    for args in [
        &["--demo", "--print", "--no-prompt"][..],
        &["--demo", "--json", "--no-prompt"],
        &["--help"],
        &["completions", "zsh"],
    ] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let out = socli()
            .args(args)
            .stdout(writer)
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), "", "{args:?}");
    }
}

#[test]
fn closing_the_pipe_after_one_line_exits_quietly() {
    let mut child = socli()
        .args(["--demo", "feed", "--print"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    assert!(first.contains("SoCLI › all"), "{first}");
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "");
}

#[test]
fn readers_that_stop_after_one_line_never_see_a_panic() {
    for args in [
        &["--demo", "--print", "--no-prompt"][..],
        &["completions", "zsh"],
    ] {
        let mut child = socli()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut first = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut first)
            .unwrap();
        assert!(!first.is_empty(), "{args:?}");
        let out = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {stderr}");
        assert!(!stderr.contains("panicked"), "{args:?}: {stderr}");
    }
}

#[test]
fn unsupported_actions_fail_without_a_request() {
    let out = socli()
        .args(["--demo", "like", "threads:p10"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("Threads has no likes in its API"), "{err}");
}

#[test]
fn no_account_says_how_to_sign_in() {
    let out = socli().output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains(
            "no accounts yet. `socli login bluesky` (or x, devnull, threads, mastodon), or `socli --demo`."
        ),
        "{err}"
    );
    let out = socli().args(["search", "rust"]).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("socli login"), "{err}");
}

#[test]
fn auth_status_lists_every_network() {
    let text = stdout_of(socli().args(["auth", "status"]).output().unwrap());
    for net in ["x", "bluesky", "dev/null"] {
        assert!(
            text.contains(&format!("\n{net}\n  account        not signed in\n")),
            "{net}: {text}"
        );
    }
    assert!(
        text.contains("\nthreads\n  account        Waiting for Meta approval\n"),
        "{text}"
    );
    assert!(text.contains("default network  not set"), "{text}");
}

#[test]
fn vscode_print_has_no_private_use_glyphs() {
    let text = demo_print("104", &[]);
    assert!(!text.chars().any(private_use), "{text}");
}

#[test]
fn nerd_icons_when_asked() {
    let out = socli()
        .env("SOCLI_ICONS", "nerd")
        .args(["--demo", "--print", "--no-prompt", "--width", "104"])
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.chars().any(private_use), "{text}");
}

#[test]
fn help_lists_commands_and_flags() {
    let text = stdout_of(socli().arg("--help").output().unwrap());
    let version = concat!("SoCLI ", env!("CARGO_PKG_VERSION"));
    assert!(text.contains(version), "{text}");
    assert!(
        text.contains("X, Bluesky, dev/null, Threads and Mastodon in your terminal"),
        "{text}"
    );
    assert!(text.contains("Usage  socli <command> [flags]"), "{text}");
    for word in [
        "feed",
        "thread",
        "login <net>",
        "logout <net>",
        "--net <list>",
        "--icons",
        "--no-icons",
        "config",
        "Read or change settings",
        "drafts",
        "List saved drafts",
        "--theme <name>",
        "--density <name>",
        "--mono",
        "--images[=<mode>]",
        "share",
        "Copy a post's link",
        "--raw",
        "Send <b> and <i> as typed",
        "socli help <command>          for one command's flags and examples",
        "socli help --json             every command, flag and example as JSON",
    ] {
        assert!(text.contains(word), "{word}: {text}");
    }
    assert!(text.contains("socli feed latest"), "{text}");
    assert!(!text.contains("Keys inside the reader"), "{text}");
}

#[test]
fn config_lists_gets_sets_and_rejects() {
    let dir = own_config("config");
    let run = |args: &[&str]| {
        socli()
            .env("SOCLI_CONFIG_DIR", &dir)
            .args(args)
            .output()
            .unwrap()
    };
    let path = stdout_of(run(&["config", "path"]));
    assert_eq!(path.trim(), dir.join("config.toml").display().to_string());

    let list = stdout_of(run(&["config", "list"]));
    for line in [
        "feed.networks = all (default)",
        "ui.density = cozy (default)",
        "ui.theme = auto (default)",
        "bluesky.color = sky (default)",
        "threads.color = violet (default)",
    ] {
        assert!(list.lines().any(|l| l == line), "{line}: {list}");
    }
    assert!(
        list.lines().any(|l| l.starts_with("colors.blue = #")),
        "{list}"
    );

    let saved = stdout_of(run(&["config", "set", "ui.density", "roomy"]));
    assert_eq!(saved.trim(), "saved ui.density = roomy");
    assert_eq!(
        stdout_of(run(&["config", "get", "ui.density"])).trim(),
        "roomy"
    );
    let saved = stdout_of(run(&["config", "set", "bluesky.color", "teal"]));
    assert_eq!(saved.trim(), "saved bluesky.color = teal");
    let list = stdout_of(run(&["config", "list"]));
    assert!(list.lines().any(|l| l == "ui.density = roomy"), "{list}");
    assert!(list.lines().any(|l| l == "bluesky.color = teal"), "{list}");
    let file = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(file.contains("density = \"roomy\""), "{file}");

    let err = stderr_of(run(&["config", "set", "ui.density", "huge"]));
    assert!(
        err.contains("\"huge\" is not a valid ui.density. Use one of: compact, cozy, roomy"),
        "{err}"
    );
    let err = stderr_of(run(&["config", "get", "ui.nope"]));
    assert!(
        err.contains("unknown key \"ui.nope\". Run `socli config list` to see the keys"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn drafts_list_and_remove() {
    let dir = own_config("drafts");
    let run = |args: &[&str]| {
        socli()
            .env("SOCLI_CONFIG_DIR", &dir)
            .args(args)
            .output()
            .unwrap()
    };
    let empty = stdout_of(run(&["drafts"]));
    assert!(empty.starts_with("no drafts"), "{empty}");
    std::fs::write(
        dir.join("drafts.json"),
        r#"[
  {"id": "1", "created_at": "2026-10-20T10:00:00+00:00", "updated_at": "2026-10-20T10:00:00+00:00",
   "networks": ["x"], "text": "an older new post"},
  {"id": "2", "created_at": "2026-10-20T11:00:00+00:00", "updated_at": "2026-10-20T11:00:00+00:00",
   "networks": ["bluesky"], "reply_to": "bluesky:9", "handle": "ada.bsky.social",
   "text": "the newest reply\nsecond line"}
]"#,
    )
    .unwrap();
    let list = stdout_of(run(&["drafts"]));
    let lines: Vec<&str> = list.lines().collect();
    assert_eq!(lines.len(), 2, "{list}");
    assert!(lines[0].starts_with("  1  "), "{list}");
    assert!(lines[0].contains("reply to @ada.bsky."), "{list}");
    assert!(lines[0].ends_with("the newest reply"), "{list}");
    assert!(lines[1].ends_with("an older new post"), "{list}");

    let removed = stdout_of(run(&["drafts", "rm", "1"]));
    assert!(removed.starts_with("removed draft 1"), "{removed}");
    let list = stdout_of(run(&["drafts"]));
    assert_eq!(list.lines().count(), 1, "{list}");
    assert!(list.contains("an older new post"), "{list}");
    let err = stderr_of(run(&["drafts", "rm", "5"]));
    assert!(
        err.contains("no draft 5. Run `socli drafts` to see the 1 saved"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn account_config_and_drafts_commands_print_json() {
    let dir = own_config("json");
    let json = |args: &[&str]| -> serde_json::Value {
        let out = socli()
            .env("SOCLI_CONFIG_DIR", &dir)
            .args(args)
            .arg("--json")
            .output()
            .unwrap();
        serde_json::from_str(&stdout_of(out)).unwrap()
    };
    let nets = ["x", "bluesky", "devnull", "threads", "mastodon"];
    let whoami = json(&["whoami"]);
    assert_eq!(whoami.as_array().unwrap().len(), 5, "{whoami}");
    for (row, net) in whoami.as_array().unwrap().iter().zip(nets) {
        assert_eq!(
            row,
            &serde_json::json!({ "network": net, "signed_in": false })
        );
    }
    assert_eq!(
        json(&["auth", "default", "bluesky"]),
        serde_json::json!({ "ok": true, "default_network": "bluesky" })
    );
    assert_eq!(
        json(&["auth", "x", "bearer", "AAAAAAAAAAAAbearer"]),
        serde_json::json!({ "ok": true, "network": "x", "action": "bearer" })
    );
    let status = json(&["auth", "status"]);
    assert_eq!(status["default_network"], "bluesky");
    assert!(status["config"].as_str().unwrap().ends_with("config.toml"));
    assert_eq!(
        status["networks"][0],
        serde_json::json!({
            "network": "x",
            "signed_in": false,
            "method": "bearer",
            "detail": "app bearer for public reads"
        })
    );
    assert!(!status.to_string().contains("AAAA"), "{status}");
    json(&[
        "auth",
        "x",
        "token",
        "--access-token",
        "tok_1234567890",
        "--username",
        "river",
    ]);
    let status = json(&["auth", "status"]);
    assert_eq!(status["networks"][0]["handle"], "river");
    assert_eq!(status["networks"][0]["method"], "oauth");
    assert!(!status.to_string().contains("tok_"), "{status}");
    assert_eq!(
        json(&["whoami"])[0],
        serde_json::json!({
            "network": "x",
            "signed_in": true,
            "handle": "river",
            "url": "https://x.com/river"
        })
    );
    assert_eq!(
        json(&["auth", "x", "logout"]),
        serde_json::json!({ "ok": true, "network": "x", "action": "logout" })
    );
    let logout = json(&["logout", "all"]);
    for (row, net) in logout.as_array().unwrap().iter().zip(nets) {
        assert_eq!(row, &serde_json::json!({ "network": net, "ok": true }));
    }

    assert!(json(&["config", "path"])["path"]
        .as_str()
        .unwrap()
        .ends_with("config.toml"));
    let list = json(&["config", "list"]);
    assert_eq!(list["feed.networks"], serde_json::json!(["all"]));
    assert_eq!(list["feed.replies"], "oldest");
    assert_eq!(list["default_network"], "bluesky");
    assert_eq!(
        json(&["config", "get", "feed.replies"]),
        serde_json::json!({ "key": "feed.replies", "value": "oldest", "default": true })
    );
    assert_eq!(
        json(&["config", "set", "feed.networks", "x,bluesky"]),
        serde_json::json!({ "ok": true, "key": "feed.networks", "value": ["x", "bluesky"] })
    );

    assert_eq!(json(&["drafts"]), serde_json::json!([]));
    std::fs::write(
        dir.join("drafts.json"),
        r#"[{"id": "2", "created_at": "2026-10-20T11:00:00+00:00", "updated_at": "2026-10-20T11:05:00+00:00",
   "targets": ["bluesky"], "reply_to": "bluesky:9", "handle": "ada.bsky.social", "text": "a reply"}]"#,
    )
    .unwrap();
    assert_eq!(
        json(&["drafts"]),
        serde_json::json!([{
            "index": 1,
            "id": "2",
            "targets": ["bluesky"],
            "text": "a reply",
            "reply_to": "bluesky:9",
            "created_at": "2026-10-20T11:00:00+00:00",
            "updated_at": "2026-10-20T11:05:00+00:00"
        }])
    );
    let mut pending: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("drafts.json")).unwrap()).unwrap();
    pending[0]["sending"] = serde_json::json!(["bluesky"]);
    std::fs::write(
        dir.join("drafts.json"),
        serde_json::to_string(&pending).unwrap(),
    )
    .unwrap();
    let pending = json(&["drafts"]);
    assert_eq!(pending[0]["sending"], serde_json::json!(["bluesky"]));
    assert!(pending[0]["warning"]
        .as_str()
        .unwrap()
        .contains("check before posting again"));
    assert_eq!(
        json(&["drafts", "rm", "1"]),
        serde_json::json!({ "ok": true, "removed": 1, "id": "2" })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn density_follows_flag_then_env_then_config() {
    let dir = own_config("density");
    let print = |env: Option<&str>, extra: &[&str]| {
        let mut cmd = socli();
        cmd.env("SOCLI_CONFIG_DIR", &dir)
            .args(["--demo", "--print", "--no-prompt", "--width", "104"])
            .args(extra);
        if let Some(value) = env {
            cmd.env("SOCLI_DENSITY", value);
        }
        let text = stdout_of(cmd.output().unwrap());
        text.lines().nth(4).unwrap_or_default().is_empty()
    };
    assert!(print(None, &[]), "cozy keeps the row under the rule");
    stdout_of(
        socli()
            .env("SOCLI_CONFIG_DIR", &dir)
            .args(["config", "set", "ui.density", "compact"])
            .output()
            .unwrap(),
    );
    assert!(!print(None, &[]), "compact from the config");
    assert!(print(Some("roomy"), &[]), "the env beats the config");
    assert!(
        !print(Some("roomy"), &["--density", "compact"]),
        "the flag beats the env"
    );
    assert!(print(None, &["--density", "cozy"]));
    let _ = std::fs::remove_dir_all(&dir);
}

const POST_HELP: &str = "
  socli post  Publish a post on your default network, or on the --net networks

  Usage     socli post <text> [flags]
            echo \"<text>\" | socli post -
            socli post      opens the composer when no text is given

  Flags     --net <list>    Networks to post to: x, bluesky, devnull, threads, mastodon, or all
            --raw           Send <b> and <i> as typed instead of bold and italic letters
            --json          Print the result as JSON
            --demo          Try it without posting
            --dry-run       Check sign-in, caps and limits without sending anything
            Global flags: socli help

  Examples  socli post \"hi from a terminal\"
            socli post \"<b>Hello</b> world!\" --net x,bluesky
            git log -1 --format=%s | socli post - --net devnull

";

#[test]
fn command_help_matches_the_mockup() {
    for args in [
        &["help", "post"][..],
        &["post", "--help"],
        &["post", "-h"],
        &["post", "hi", "--net", "x", "-h"],
    ] {
        let text = stdout_of(socli().args(args).output().unwrap());
        assert_eq!(text, POST_HELP, "{args:?}");
    }
    for args in [
        &["reply", "--help"][..],
        &["auth", "-h"],
        &["login", "--help"],
    ] {
        let text = stdout_of(socli().args(args).output().unwrap());
        assert!(
            text.starts_with(&format!("\n  socli {}  ", args[0])),
            "{text}"
        );
    }
    let out = socli().args(["help", "nope"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "socli: no command named nope. Run socli help\n"
    );
}

fn demo_rows(args: &[&str]) -> Vec<serde_json::Value> {
    let out = socli()
        .args(["--demo", "--json"])
        .args(args)
        .output()
        .unwrap();
    let rows: serde_json::Value = serde_json::from_str(&stdout_of(out)).unwrap();
    rows.as_array().unwrap().clone()
}

#[test]
fn demo_post_json_holds_the_converted_text() {
    let rows = demo_rows(&["post", "<b>Hello</b> world!", "--net", "x,bluesky"]);
    assert_eq!(rows.len(), 2);
    for (row, net) in rows.iter().zip(["x", "bluesky"]) {
        assert_eq!(row["network"], net);
        assert_eq!(row["ok"], true);
        assert_eq!(row["demo"], true);
        assert_eq!(row["text"], "𝗛𝗲𝗹𝗹𝗼 world!");
        assert!(row["id"].as_str().unwrap().starts_with(&format!("{net}:")));
    }
    let rows = demo_rows(&["post", "<b>Hello</b> world!", "--raw"]);
    assert_eq!(rows[0]["text"], "<b>Hello</b> world!");
}

fn piped(args: &[&str], input: &str) -> String {
    let mut child = socli()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    stdout_of(child.wait_with_output().unwrap())
}

#[test]
fn posts_read_stdin_for_a_dash_or_a_pipe() {
    for args in [
        &["post", "-", "--demo", "--json"][..],
        &["post", "--demo", "--json"],
    ] {
        let rows: serde_json::Value = serde_json::from_str(&piped(args, "hi\n")).unwrap();
        assert_eq!(rows[0]["text"], "hi", "{args:?}");
    }
    let rows: serde_json::Value = serde_json::from_str(&piped(
        &["reply", "x:p1", "-", "--demo", "--json"],
        "ok\n\n",
    ))
    .unwrap();
    assert_eq!(rows[0]["text"], "ok");
    assert_eq!(rows[0]["network"], "x");
}

#[test]
fn share_only_prints_the_link_in_a_pipe() {
    let url = "https://x.com/mina/status/p1";
    let out = socli().args(["share", "x:p1", "--demo"]).output().unwrap();
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout_of(out), format!("{url}\n"));
    let out = socli()
        .args(["share", "x:p1", "--demo", "--profile"])
        .output()
        .unwrap();
    assert_eq!(stdout_of(out), "https://x.com/mina\n");
    let out = socli()
        .args(["share", "x:p1", "--demo", "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(stdout_of(out).trim()).unwrap();
    assert_eq!(value, serde_json::json!({ "url": url, "copied": false }));
}

#[test]
fn share_only_prints_the_link_for_an_agent() {
    let out = socli()
        .env("SOCLI_AGENT", "1")
        .args(["share", "x:p1", "--demo"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_str(stdout_of(out).trim()).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "url": "https://x.com/mina/status/p1", "copied": false })
    );
}

#[test]
fn images_alone_is_auto_and_leaves_the_command() {
    let text = demo_print("104", &["--images", "feed"]);
    assert!(text.contains("SoCLI › all"), "{text}");
    assert!(text.contains("▣ "), "images printed: {text}");
    let out = socli()
        .args(["--demo", "--print", "--no-prompt", "--images", "blocks"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// Stdout as JSON, after checking the run succeeded and printed no escape byte.
fn json_of(out: Output) -> serde_json::Value {
    assert!(!out.stdout.contains(&0x1b), "{out:?}");
    serde_json::from_str(&stdout_of(out)).unwrap()
}

#[test]
fn every_demo_command_prints_json_without_escapes() {
    for args in [
        &["feed"][..],
        &["feed", "latest"],
        &["thread", "x:p1"],
        &["search", "terminal"],
        &["user", "mina"],
        &["notifications"],
        &["bookmarks"],
        &["ask", "what"],
        &["post", "hi"],
        &["reply", "x:p1", "hi"],
        &["quote", "x:p1", "hi"],
        &["like", "x:p1"],
        &["unlike", "x:p1"],
        &["repost", "x:p1"],
        &["unrepost", "x:p1"],
        &["bookmark", "x:p1"],
        &["unbookmark", "x:p1"],
        &["delete", "x:p1", "--yes"],
        &["follow", "ada", "--net", "x"],
        &["unfollow", "ada", "--net", "x"],
        &["view", "bluesky:p8"],
        &["help"],
        &["help", "post"],
    ] {
        let out = socli()
            .args(["--demo", "--json"])
            .args(args)
            .output()
            .unwrap();
        json_of(out);
    }
}

#[test]
fn agents_get_json_and_never_wait_for_a_key() {
    let mut child = socli()
        .env_remove("SOCLI_AGENT")
        .env("CLAUDECODE", "1")
        .arg("--demo")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let held = child.stdin.take();
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut text).unwrap();
        text
    });
    let start = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > std::time::Duration::from_secs(5) {
            child.kill().unwrap();
            panic!("socli waited for input in agent mode");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    drop(held);
    assert!(status.success());
    let posts: serde_json::Value = serde_json::from_str(&reader.join().unwrap()).unwrap();
    assert!(!posts.as_array().unwrap().is_empty());

    let text = stdout_of(
        socli()
            .env("CLAUDECODE", "1")
            .arg("--demo")
            .output()
            .unwrap(),
    );
    assert!(
        text.contains("SoCLI › all › home"),
        "SOCLI_AGENT=0 keeps text: {text}"
    );
    let text = stdout_of(
        socli()
            .env("SOCLI_AGENT", "1")
            .args(["--demo", "--print"])
            .output()
            .unwrap(),
    );
    assert!(
        text.contains("SoCLI › all › home"),
        "--print asks for text: {text}"
    );
}

#[test]
fn help_json_lists_every_command() {
    let help = json_of(socli().args(["help", "--json"]).output().unwrap());
    assert_eq!(help["name"], "socli");
    assert_eq!(help["version"], env!("CARGO_PKG_VERSION"));
    let names: Vec<&str> = help["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    for name in [
        "feed",
        "thread",
        "search",
        "user",
        "notifications",
        "bookmarks",
        "ask",
        "post",
        "reply",
        "quote",
        "like",
        "unlike",
        "repost",
        "unrepost",
        "bookmark",
        "unbookmark",
        "share",
        "follow",
        "unfollow",
        "delete",
        "view",
        "login",
        "logout",
        "whoami",
        "auth",
        "config",
        "drafts",
        "completions",
        "help",
    ] {
        assert!(names.contains(&name), "{name}");
    }
    let post = &help["commands"][names.iter().position(|n| *n == "post").unwrap()];
    assert_eq!(post["usage"][0], "socli post <text> [flags]");
    assert_eq!(
        post["flags"][0],
        serde_json::json!({
            "flag": "--net",
            "value": "<list>",
            "description": "Networks to post to: x, bluesky, devnull, threads, mastodon, or all",
        })
    );
    assert_eq!(post["examples"][0], "socli post \"hi from a terminal\"");
    assert!(help["global_flags"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["flag"] == "--dry-run"));
    assert_eq!(
        help["exit_codes"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["0", "1", "2"]
    );
    let environment: Vec<&str> = help["environment"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        environment,
        [
            "SOCLI_NET",
            "SOCLI_CONFIG_DIR",
            "SOCLI_AGENT",
            "SOCLI_IMAGE",
            "SOCLI_DETAIL",
            "SOCLI_LAYOUT",
            "SOCLI_MOUSE",
            "SOCLI_HYPERLINKS",
            "SOCLI_TRUECOLOR",
            "SOCLI_TRACE_FRAMES",
            "NO_COLOR",
            "NO_ICONS",
        ]
    );
    let one = json_of(socli().args(["help", "post", "--json"]).output().unwrap());
    assert_eq!(one, *post);
    let agent = json_of(
        socli()
            .env("SOCLI_AGENT", "1")
            .arg("help")
            .output()
            .unwrap(),
    );
    assert_eq!(agent, help);
}

#[test]
fn mouse_is_a_setting_and_a_flag() {
    let dir = own_config("mouse");
    let run = |args: &[&str]| {
        socli()
            .env("SOCLI_CONFIG_DIR", &dir)
            .args(args)
            .output()
            .unwrap()
    };
    let list = stdout_of(run(&["config", "list"]));
    assert!(
        list.lines().any(|l| l == "ui.mouse = on (default)"),
        "{list}"
    );
    let saved = stdout_of(run(&["config", "set", "ui.mouse", "off"]));
    assert_eq!(saved.trim(), "saved ui.mouse = off");
    assert_eq!(stdout_of(run(&["config", "get", "ui.mouse"])).trim(), "off");
    let err = stderr_of(run(&["config", "set", "ui.mouse", "maybe"]));
    assert!(
        err.contains("\"maybe\" is not a valid ui.mouse. Use one of: on, off"),
        "{err}"
    );

    let out = socli()
        .args(["--demo", "--print", "--no-prompt", "--mouse", "maybe"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("--mouse"), "{err}");
    stdout_of(
        socli()
            .args(["--demo", "--print", "--no-prompt", "--mouse", "off"])
            .output()
            .unwrap(),
    );
}

#[test]
fn enter_is_a_setting_without_a_flag() {
    let dir = own_config("enter");
    let run = |args: &[&str]| {
        socli()
            .env("SOCLI_CONFIG_DIR", &dir)
            .args(args)
            .output()
            .unwrap()
    };
    let list = stdout_of(run(&["config", "list"]));
    assert!(
        list.lines().any(|l| l == "ui.enter = post (default)"),
        "{list}"
    );
    let saved = stdout_of(run(&["config", "set", "ui.enter", "newline"]));
    assert_eq!(saved.trim(), "saved ui.enter = newline");
    assert_eq!(
        stdout_of(run(&["config", "get", "ui.enter"])).trim(),
        "newline"
    );
    let err = stderr_of(run(&["config", "set", "ui.enter", "maybe"]));
    assert!(
        err.contains("\"maybe\" is not a valid ui.enter. Use one of: post, newline"),
        "{err}"
    );
}

#[test]
fn usage_errors_exit_2_with_one_json_line() {
    for (args, message) in [
        (
            &["--demo", "delete", "x:p1"][..],
            "delete needs --yes when socli runs without a terminal",
        ),
        (&["help", "nope"], "no command named nope. Run socli help"),
        (
            &["login", "bluesky", "--password-stdin"],
            "--password-stdin needs --handle",
        ),
    ] {
        let out = socli().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert_eq!(
            String::from_utf8(out.stderr).unwrap(),
            format!("socli: {message}\n")
        );
        let out = socli().args(args).arg("--json").output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
        assert_eq!(
            String::from_utf8(out.stderr).unwrap(),
            format!("{}\n", serde_json::json!({ "error": message }))
        );
    }
}

#[test]
fn agents_pass_text_explicitly() {
    let mut child = socli()
        .env("SOCLI_AGENT", "1")
        .args(["post", "--demo"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"hi\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        format!(
            "{}\n",
            serde_json::json!({ "error": "post needs text: socli post \"…\" or socli post - < file" })
        )
    );
    let rows: serde_json::Value =
        serde_json::from_str(&piped(&["post", "-", "--demo", "--json"], "hi\n")).unwrap();
    assert_eq!(rows[0]["text"], "hi");
}

#[test]
fn dry_runs_send_nothing_and_check_each_limit() {
    let out = socli()
        .args(["--demo", "--dry-run", "post", "hi", "--net", "x,bluesky"])
        .output()
        .unwrap();
    assert_eq!(
        stdout_of(out),
        "✓ X        would post · 2/280\n✓ Bluesky  would post · 2/300\n"
    );
    let rows = demo_rows(&["--dry-run", "post", "<b>Hi</b>", "--net", "x,bluesky"]);
    assert_eq!(
        rows[0],
        serde_json::json!({
            "network": "x",
            "ok": true,
            "demo": true,
            "dry_run": true,
            "text": "𝗛𝗶",
            "length": 4,
            "limit": 280,
        })
    );
    let long = "a".repeat(290);
    let out = socli()
        .args([
            "--demo",
            "--dry-run",
            "--json",
            "post",
            &long,
            "--net",
            "x,bluesky",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let rows: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        rows[0],
        serde_json::json!({"network": "x", "ok": false, "dry_run": true, "error": "too long for X: 290/280"})
    );
    assert_eq!(rows[1]["ok"], true);
    let out = socli()
        .args(["--demo", "--dry-run", "delete", "x:p1"])
        .output()
        .unwrap();
    assert_eq!(stdout_of(out), "✓ X  would delete\n");
}

#[test]
fn clap_errors_become_one_json_line() {
    let out = socli().args(["posts", "hi", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    let value: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(value["error"], "unrecognized subcommand 'posts'");
    assert!(value["tip"].as_str().unwrap().contains("'post'"), "{value}");
    assert_eq!(value["usage"], "socli [OPTIONS] [COMMAND]");
    let out = socli().args(["posts", "hi"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .starts_with("error: unrecognized subcommand 'posts'"));
}

#[test]
fn agents_cannot_open_a_browser_sign_in() {
    for (args, message) in [
        (
            &["login", "x"][..],
            "signing in to X needs a person at a browser. Run socli login x in a terminal",
        ),
        (
            &["auth", "devnull", "login"],
            "signing in to dev/null needs a person at a browser. Run socli login devnull in a terminal",
        ),
    ] {
        let out = socli().env("SOCLI_AGENT", "1").args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert_eq!(
            String::from_utf8(out.stderr).unwrap(),
            format!("{}\n", serde_json::json!({ "error": message }))
        );
    }
}

#[test]
fn threads_waits_for_meta_approval() {
    let message = "Waiting for Meta approval";
    for args in [
        &["login", "threads"][..],
        &["auth", "threads", "login"],
        &["auth", "threads", "refresh"],
        &[
            "auth",
            "threads",
            "oauth",
            "--app-id",
            "1",
            "--app-secret",
            "s",
            "--redirect-uri",
            "https://localhost/cb",
        ],
        &["whoami", "--net", "threads"],
        &["post", "hi", "--net", "x,threads"],
    ] {
        let out = socli().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.contains(message), "{args:?}: {err}");
        assert!(!err.contains("http"), "{args:?}: {err}");
    }
    let out = socli()
        .env("SOCLI_AGENT", "1")
        .args(["login", "threads"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        format!("{}\n", serde_json::json!({ "error": message }))
    );
    let out = socli()
        .args(["--demo", "whoami", "--net", "threads"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn sign_in_prompts_need_a_person() {
    let out = socli()
        .args(["login", "bluesky"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.contains("this step needs a person at a terminal."),
        "{err}"
    );
    assert!(err.contains("--password-stdin"), "{err}");
    assert!(out.stdout.is_empty() || !String::from_utf8_lossy(&out.stdout).contains("Handle"));
}

#[test]
fn agents_get_sign_in_errors_as_json() {
    let out = socli()
        .env("SOCLI_AGENT", "1")
        .args(["login", "bluesky"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    let value: serde_json::Value = serde_json::from_str(err.trim()).unwrap();
    assert!(
        value["error"].as_str().unwrap().contains("needs a person"),
        "{err}"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains('\u{1b}'), "{stdout}");
}

#[test]
fn piped_output_uses_unicode_icons_in_ghostty() {
    let out = socli()
        .env("TERM_PROGRAM", "ghostty")
        .env("TERM", "xterm-ghostty")
        .args(["--demo", "--print", "--no-prompt", "--width", "80"])
        .output()
        .unwrap();
    let text = stdout_of(out);
    assert!(!text.chars().any(private_use), "{text}");
}

#[test]
fn view_rejects_a_target_it_cannot_load_in_json_too() {
    let out = socli().args(["view", "nope", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        format!(
            "{}\n",
            serde_json::json!({ "error": "not an image url: nope" })
        )
    );
}

#[test]
fn printed_profiles_skip_the_search_bar_and_the_back_key() {
    let text = demo_print("100", &["--detail", "rich", "user", "@mina", "--net", "x"]);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("  SoCLI › x › @mina"), "{text}");
    assert!(!lines[0].contains("back"), "{text}");
    assert!(lines[1].is_empty(), "{text}");
    assert!(lines[2].contains("╭─ Profile "), "{text}");
    assert!(lines[3].contains("Mina Cho"), "{text}");
    assert!(lines[4].contains("@mina"), "{text}");
    assert!(lines[5].contains("x.com/mina"), "{text}");
    assert!(!text.contains("Search  mina"), "{text}");
}

#[test]
fn printed_threads_never_offer_the_back_key() {
    let text = demo_print("100", &["--detail", "rich", "thread", "x:p1"]);
    let first = text.lines().next().unwrap();
    assert!(
        first.starts_with("  SoCLI › all › home › thread"),
        "{first}"
    );
    assert!(!first.contains("back"), "{first}");
}

#[test]
fn printed_merged_profiles_name_the_account() {
    let text = demo_print("100", &["--detail", "rich", "user", "@nuno"]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[2], "  Posts  @nuno", "{text}");
}

#[test]
fn config_themes_lists_every_theme() {
    let out = stdout_of(socli().args(["config", "themes"]).output().unwrap());
    assert_eq!(out.lines().count(), 31, "{out}");
    assert!(out.lines().any(|l| l.starts_with("native ")), "{out}");
    assert!(out.lines().any(|l| l.starts_with("catppuccin ")), "{out}");
    let json = json_of(
        socli()
            .args(["config", "themes", "--json"])
            .output()
            .unwrap(),
    );
    let themes = json["themes"].as_array().unwrap();
    assert_eq!(themes.len(), 31);
    assert_eq!(themes[1]["name"], "native");
    assert_eq!(themes[1]["variants"], serde_json::json!([]));
}

#[test]
fn an_unknown_theme_is_rejected() {
    let err = stderr_of(socli().args(["--theme", "nope", "feed"]).output().unwrap());
    assert!(
        err.contains("use auto, native, black, white or a theme from socli config themes"),
        "{err}"
    );
}
