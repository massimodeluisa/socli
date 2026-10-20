//! Writes from the command line: posting to several networks, and the result lines every write prints.

use std::collections::HashSet;
use std::io::{self, IsTerminal};
use std::path::Path;

use serde::Serialize;
use unicode_width::UnicodeWidthStr;

use crate::command::Cmd;
use crate::drafts::{self, Draft};
use crate::intent::Intent;
use crate::model::Post;
use crate::net::{self, Caps, Clients, NetError, Network};
use crate::plain;
use crate::render::{self, Canvas, Kind, StyledLine};
use crate::state::State;
use crate::theme::Icons;

/// The third column of a demo post, reply or quote.
pub const DEMO: &str = "demo · not posted";
/// How the composer tells the user to fix a quote that has no link.
pub const TUI_HINT: &str = "Open it and press s to copy the link";
/// How `socli quote` tells the user to fix a quote that has no link.
pub const ONE_SHOT_HINT: &str = "Pass its link: socli quote <url> \"…\"";

/// One target's new post.
pub struct Sent {
    pub id: Option<String>,
    pub url: Option<String>,
    /// What `State::write_post` answered: the URL, or the demo note.
    pub note: String,
    /// The text as sent, with the quoted link when there is one.
    pub text: String,
}

/// One network's answer to a write.
#[derive(Default)]
pub struct Done {
    pub id: Option<String>,
    /// The account a follow or unfollow named, without the `@`.
    pub handle: Option<String>,
    pub url: Option<String>,
    /// The third column of the result line: a URL or what changed.
    pub what: String,
    pub demo: bool,
    /// Nothing was sent: `--dry-run`.
    pub dry_run: bool,
    /// The text a demo or dry-run post would have sent.
    pub text: Option<String>,
    /// A dry-run post's length, counted the network's way.
    pub length: Option<usize>,
    /// The network's character limit, for a dry-run post.
    pub limit: Option<usize>,
}

/// Each network's answer, in network order.
pub type Results = Vec<(Network, Result<Done, String>)>;

/**
Posts `text` to each target in network order and returns the new post's id, URL and sent text per
network. Replies go only to the parent's network. A quote is native only on the quoted post's own
network, when that network can quote and is not X; elsewhere the text gets a blank line and the
quoted link.
*/
pub fn send(
    state: &mut State,
    clients: &mut Clients,
    text: &str,
    targets: &[Network],
    reply_to: Option<String>,
    quote: Option<String>,
) -> Vec<(Network, Result<Sent, String>)> {
    send_with(
        state,
        clients,
        text,
        targets,
        reply_to,
        quote,
        ONE_SHOT_HINT,
    )
}

/// What `send_intent` did: the draft id it kept, each network's answer, and the draft update after.
pub struct Delivery {
    pub id: String,
    pub results: Vec<(Network, Result<Sent, String>)>,
    /// The draft transaction after the send; when it fails the record from before the send stays.
    pub kept: Result<(), String>,
}

impl Delivery {
    /// The networks that refused the post, in network order.
    pub fn failed(&self) -> Vec<Network> {
        refused(&self.results)
    }
}

fn refused(results: &[(Network, Result<Sent, String>)]) -> Vec<Network> {
    results
        .iter()
        .filter(|(_, result)| result.is_err())
        .map(|(net, _)| *net)
        .collect()
}

/**
Sends `intent` with a durable record in the drafts file `store`. Before any network call the
intent is written as a draft (under `intent.draft_id` when it has one) with `sending` set to every
addressed network, and nothing is sent when that write fails. After the send, one transaction
removes the draft when every network took the post, or rewrites the same draft with only the
networks that failed.
*/
pub fn send_intent(
    state: &mut State,
    clients: &mut Clients,
    intent: &Intent,
    store: &Path,
    hint: &str,
) -> Result<Delivery, String> {
    intent.check()?;
    let nets = nets_for(&intent.targets, intent.reply_to.as_deref());
    let id = draft_id(intent);
    let handle = handle_of(state, intent);
    let record = |drafts: &mut Vec<Draft>, targets: Vec<String>, sending: Vec<String>| {
        put_draft(drafts, &id, intent, handle.clone(), targets, sending);
    };
    let slugs = |nets: &[Network]| nets.iter().map(|net| net.slug().to_string()).collect();
    drafts::update_at(store, |drafts| {
        record(drafts, intent.slugs(), slugs(&nets));
        Ok(())
    })
    .map_err(|err| format!("couldn't keep a draft before sending, so nothing was sent: {err}"))?;
    let results = send_with(
        state,
        clients,
        &intent.text,
        &intent.targets,
        intent.reply_to.clone(),
        intent.quote.clone(),
        hint,
    );
    let failed = refused(&results);
    let kept = drafts::update_at(store, |drafts| {
        if failed.is_empty() {
            drafts.retain(|draft| draft.id != id);
        } else {
            record(drafts, slugs(&failed), Vec::new());
        }
        Ok(())
    });
    Ok(Delivery { id, results, kept })
}

/**
Writes `intent` to the drafts file `store` as its draft (under `intent.draft_id`, or a new id) with
nothing in flight, so the draft holds exactly what a composer dry run previewed.
*/
pub fn keep(state: &State, intent: &Intent, store: &Path) -> Result<(), String> {
    let id = draft_id(intent);
    let handle = handle_of(state, intent);
    drafts::update_at(store, |drafts| {
        put_draft(drafts, &id, intent, handle, intent.slugs(), Vec::new());
        Ok(())
    })
}

fn draft_id(intent: &Intent) -> String {
    intent
        .draft_id
        .clone()
        .unwrap_or_else(|| Draft::new(Vec::new(), None, None).id)
}

/// The author of the post a reply or quote answers, for the drafts list.
fn handle_of(state: &State, intent: &Intent) -> Option<String> {
    intent
        .reply_to
        .as_ref()
        .or(intent.quote.as_ref())
        .and_then(|id| state.posts.get(id))
        .map(|post| post.author.username.clone())
}

/// Puts `intent` in `drafts` as draft `id` with `targets` and `sending`, keeping the draft's creation time.
fn put_draft(
    drafts: &mut Vec<Draft>,
    id: &str,
    intent: &Intent,
    handle: Option<String>,
    targets: Vec<String>,
    sending: Vec<String>,
) {
    let mut draft = drafts
        .iter()
        .find(|draft| draft.id == id)
        .cloned()
        .unwrap_or_else(|| Draft::new(Vec::new(), None, None));
    draft.id = id.to_string();
    draft.updated_at = crate::clock::utc_now();
    draft.targets = targets;
    draft.reply_to = intent.reply_to.clone();
    draft.quote = intent.quote.clone();
    draft.handle = handle.or(draft.handle);
    draft.text = intent.typed.clone();
    draft.sending = sending;
    drafts::put(drafts, draft);
}

fn send_with(
    state: &mut State,
    clients: &mut Clients,
    text: &str,
    targets: &[Network],
    reply_to: Option<String>,
    quote: Option<String>,
    hint: &str,
) -> Vec<(Network, Result<Sent, String>)> {
    nets_for(targets, reply_to.as_deref())
        .into_iter()
        .map(|net| {
            let result =
                draft_for(state, net, text, quote.as_deref(), hint).and_then(|(text, native)| {
                    fits(net, text.trim())?;
                    write(state, clients, &text, reply_to.clone(), native, net)
                });
            (net, result)
        })
        .collect()
}

/**
The text checks posting and `--dry-run` share: the network's character limit, then Threads' own
rules. Returns the length and the limit.
*/
fn fits(net: Network, text: &str) -> Result<(usize, usize), String> {
    let (length, limit) = (net.text_len(text), net.caps().max_chars);
    if length > limit {
        return Err(format!("too long for {}: {length}/{limit}", net.title()));
    }
    if net == Network::Threads {
        net::caps::threads_check_text(text)?;
    }
    Ok((length, limit))
}

/// The parent's network for a reply, else the targets in network order.
fn nets_for(targets: &[Network], reply_to: Option<&str>) -> Vec<Network> {
    match reply_to.and_then(net::split) {
        Some((net, _)) => vec![net],
        None => Network::ALL
            .into_iter()
            .filter(|net| targets.contains(net))
            .collect(),
    }
}

/**
What `net` publishes for `text`, and the post it quotes natively. A link quote appends the quote
argument when it is a URL, else the stored post's permalink; with neither the target fails.
*/
fn draft_for(
    state: &State,
    net: Network,
    text: &str,
    quote: Option<&str>,
    hint: &str,
) -> Result<(String, Option<String>), String> {
    let Some(arg) = quote else {
        return Ok((text.to_string(), None));
    };
    let quoted = net::parse_target(arg, None).ok();
    let own = quoted
        .as_deref()
        .and_then(net::split)
        .is_some_and(|(of, _)| of == net);
    if own && net.caps().quote && net != Network::X {
        return Ok((text.to_string(), quoted));
    }
    let link = if arg.starts_with("http://") || arg.starts_with("https://") {
        Some(arg.to_string())
    } else {
        quoted
            .as_deref()
            .and_then(|id| state.posts.get(id))
            .map(Post::permalink)
            .filter(|url| !url.is_empty())
    };
    let link = link.ok_or_else(|| {
        format!(
            "can't quote this post on {}: its author is unknown. {hint}",
            net.title()
        )
    })?;
    Ok((format!("{}\n\n{link}", text.trim_end()), None))
}

fn write(
    state: &mut State,
    clients: &mut Clients,
    text: &str,
    reply_to: Option<String>,
    quote: Option<String>,
    net: Network,
) -> Result<Sent, String> {
    let before: HashSet<String> = state.posts.keys().cloned().collect();
    let note = state.write_post(text, reply_to, quote, net, clients)?;
    let sent = text.trim();
    let post = state
        .posts
        .values()
        .find(|post| post.net == net && post.text == sent && !before.contains(&post.id));
    Ok(Sent {
        id: post.map(|post| post.id.clone()),
        url: post.and_then(|post| post.url.clone()),
        note,
        text: sent.to_string(),
    })
}

/// Posting results as result lines: each new post's URL, or the demo note with the text it would send.
pub fn posted(demo: bool, results: Vec<(Network, Result<Sent, String>)>) -> Results {
    results
        .into_iter()
        .map(|(net, result)| {
            let done = result.map(|sent| Done {
                what: if demo {
                    DEMO.to_string()
                } else {
                    sent.url.clone().unwrap_or(sent.note)
                },
                id: sent.id,
                url: sent.url,
                demo,
                text: demo.then_some(sent.text),
                ..Done::default()
            });
            (net, done)
        })
        .collect()
}

// MARK: dry run

/// What a write needs, as `State` names it in errors, and the capability that allows it.
type Check = (&'static str, fn(&Caps) -> bool);

/**
`--dry-run` for post, reply and quote: what each target would send, checked against its sign-in,
its caps, its character limit and the quoted link. Sends nothing.
*/
pub fn check(
    state: &State,
    text: &str,
    targets: &[Network],
    reply_to: Option<&str>,
    quote: Option<&str>,
) -> Results {
    nets_for(targets, reply_to)
        .into_iter()
        .map(|net| {
            let done =
                draft_for(state, net, text, quote, ONE_SHOT_HINT).and_then(|(text, native)| {
                    let verb = match (native.is_some(), reply_to.is_some(), quote.is_some()) {
                        (true, _, _) => {
                            allowed(state, net, &[("quotes", |caps| caps.quote)])?;
                            "would quote"
                        }
                        (_, true, _) => {
                            allowed(state, net, &[("replies", |caps| caps.post)])?;
                            "would reply"
                        }
                        (_, _, quoted) => {
                            allowed(state, net, &[("posts", |caps| caps.post)])?;
                            if quoted {
                                "would quote"
                            } else {
                                "would post"
                            }
                        }
                    };
                    let text = text.trim().to_string();
                    if text.is_empty() {
                        return Err("empty post".into());
                    }
                    let (length, limit) = fits(net, &text)?;
                    Ok(Done {
                        what: format!("{verb} · {length}/{limit}"),
                        demo: state.demo,
                        dry_run: true,
                        text: Some(text),
                        length: Some(length),
                        limit: Some(limit),
                        ..Done::default()
                    })
                });
            (net, done)
        })
        .collect()
}

/// `--dry-run` for a write on one post or account: the result line's third column, or why it would fail.
pub fn check_write(state: &State, net: Network, cmd: &Cmd) -> Result<String, String> {
    const LIKE: Check = ("likes", |caps| caps.like);
    const BOOKMARK: Check = ("bookmarks", |caps| caps.bookmark);
    const REPOST: Check = ("reposts", |caps| caps.repost);
    const FOLLOW: Check = ("follows", |caps| caps.follow);
    let verb = net.caps().like_verb;
    let (checks, what): (&[Check], String) = match cmd {
        Cmd::Like { .. } => (&[LIKE], format!("would {verb}")),
        Cmd::Unlike { .. } => (&[LIKE], format!("would un{verb}")),
        Cmd::Bookmark { .. } => (&[BOOKMARK], "would bookmark".into()),
        Cmd::Unbookmark { .. } => (&[BOOKMARK], "would unbookmark".into()),
        Cmd::Repost { .. } => (&[REPOST], "would repost".into()),
        Cmd::Unrepost { .. } => (
            &[REPOST, ("repost undo", |caps| caps.unrepost)],
            "would unrepost".into(),
        ),
        Cmd::Delete { .. } => (&[("deletes", |caps| caps.delete)], "would delete".into()),
        Cmd::Follow(handle) => (&[FOLLOW], format!("would follow @{handle}")),
        Cmd::Unfollow(handle) => (&[FOLLOW], format!("would unfollow @{handle}")),
        _ => return Err("nothing to check".into()),
    };
    allowed(state, net, checks)?;
    Ok(what)
}

/// The checks `State` makes before a write: the network can do it, and this account can.
fn allowed(state: &State, net: Network, checks: &[Check]) -> Result<(), String> {
    for (what, has) in checks {
        if !has(&net.caps()) {
            return Err(NetError::unsupported(net, *what).into());
        }
        if !state
            .nets
            .iter()
            .any(|info| info.net == net && has(&info.caps))
        {
            return Err(NetError::SignInRequired(net).into());
        }
    }
    Ok(())
}

// MARK: result lines

/**
Prints one line per network, successes on stdout and failures on stderr, or one JSON array on
stdout. Fails with no message of its own when any target failed, since every line already says why.
*/
pub fn report(
    results: &[(Network, Result<Done, String>)],
    json: bool,
    dry_run: bool,
    icons: Icons,
) -> Result<(), String> {
    if json {
        let rows = json_rows(results, dry_run);
        plain::emit_line(&serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?);
    } else {
        let out = color(io::stdout().is_terminal());
        let err = color(io::stderr().is_terminal());
        for ((_, result), line) in results.iter().zip(lines(results, icons)) {
            match result {
                Ok(_) => plain::emit_line(&render::paint(&line, out)),
                Err(_) => eprintln!("{}", render::paint(&line, err)),
            }
        }
    }
    if failed(results) {
        Err(String::new())
    } else {
        Ok(())
    }
}

fn color(tty: bool) -> bool {
    tty && std::env::var_os("NO_COLOR").is_none() && !crate::agent::active()
}

fn failed(results: &[(Network, Result<Done, String>)]) -> bool {
    results.iter().any(|(_, result)| result.is_err())
}

/// `✓ <Net>  <url or change>` or `✗ <Net>  <error>`, the labels padded so the third column aligns.
fn lines(results: &[(Network, Result<Done, String>)], icons: Icons) -> Vec<StyledLine> {
    let width = results
        .iter()
        .map(|(net, _)| net.title().width())
        .max()
        .unwrap_or(0);
    results
        .iter()
        .map(|(net, result)| {
            let (icon, kind, what, tail) = match result {
                Ok(done) => (icons.check, Kind::Green, done.what.as_str(), Kind::Dim),
                Err(err) => (icons.cross, Kind::Red, err.as_str(), Kind::Fg),
            };
            let mut canvas = Canvas::new(icon.width() + width + what.width() + 4);
            let y = canvas.push(false);
            let x = canvas.put(y, 0, kind, icon) + 1;
            canvas.put(y, x, Kind::Bold, net.title());
            canvas.put(y, x + width + 2, tail, what);
            canvas.finish_line()
        })
        .collect()
}

#[derive(Serialize)]
struct Row<'a> {
    network: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    handle: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<&'a str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    demo: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

fn json_rows(results: &[(Network, Result<Done, String>)], dry_run: bool) -> Vec<Row<'_>> {
    results
        .iter()
        .map(|(net, result)| match result {
            Ok(done) => Row {
                network: net.slug(),
                ok: true,
                id: done.id.as_deref(),
                handle: done.handle.as_deref(),
                url: done.url.as_deref(),
                demo: done.demo,
                dry_run: done.dry_run || dry_run,
                text: done.text.as_deref(),
                length: done.length,
                limit: done.limit,
                error: None,
            },
            Err(err) => Row {
                network: net.slug(),
                ok: false,
                id: None,
                handle: None,
                url: None,
                demo: false,
                dry_run,
                text: None,
                length: None,
                limit: None,
                error: Some(err),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn done(what: &str) -> Done {
        Done {
            id: Some("x:1".into()),
            url: Some("https://x.com/you/status/1".into()),
            what: what.into(),
            ..Done::default()
        }
    }

    fn demo() -> State {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        state.ensure_loaded(&mut Clients::empty()).unwrap();
        state
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "socli-send-{name}-{}-{:08x}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn intent(text: &str, targets: &[Network]) -> Intent {
        Intent {
            text: text.into(),
            typed: text.into(),
            targets: targets.to_vec(),
            reply_to: None,
            quote: None,
            draft_id: None,
        }
    }

    fn deliver(state: &mut State, intent: &Intent, store: &Path) -> Result<Delivery, String> {
        send_intent(state, &mut Clients::empty(), intent, store, TUI_HINT)
    }

    #[test]
    fn a_send_keeps_its_draft_until_every_network_took_it() {
        let dir = temp("record");
        let store = dir.join("drafts.json");
        let mut state = demo();
        let mut long = intent(&"a".repeat(290), &[Network::X, Network::Bluesky]);
        long.typed = "as typed".into();
        drafts::fail_after(1);
        let delivery = deliver(&mut state, &long, &store).unwrap();
        assert!(delivery.kept.is_err());
        let kept = drafts::load_from(&store).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, delivery.id);
        assert_eq!(kept[0].text, "as typed");
        assert_eq!(kept[0].targets, ["x", "bluesky"]);
        assert_eq!(kept[0].sending, ["x", "bluesky"]);

        long.draft_id = Some(delivery.id.clone());
        let delivery = deliver(&mut state, &long, &store).unwrap();
        assert_eq!(delivery.kept, Ok(()));
        assert_eq!(delivery.failed(), [Network::X]);
        let kept = drafts::load_from(&store).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(Some(&kept[0].id), long.draft_id.as_ref());
        assert_eq!(kept[0].targets, ["x"]);
        assert!(kept[0].sending.is_empty());

        let short = Intent {
            draft_id: long.draft_id.clone(),
            ..intent("hi", &[Network::X])
        };
        let delivery = deliver(&mut state, &short, &store).unwrap();
        assert_eq!(delivery.kept, Ok(()));
        assert!(delivery.failed().is_empty());
        assert!(drafts::load_from(&store).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_kept_dry_run_replaces_the_older_autosave() {
        let dir = temp("kept");
        let store = dir.join("drafts.json");
        let state = demo();
        let mut autosave = Draft::new(vec!["bluesky".into()], None, None);
        autosave.text = "older autosave".into();
        drafts::upsert_at(&store, &autosave).unwrap();
        let previewed = Intent {
            draft_id: Some(autosave.id.clone()),
            typed: "as previewed".into(),
            ..intent("as previewed", &[Network::X, Network::Bluesky])
        };
        keep(&state, &previewed, &store).unwrap();
        let kept = drafts::load_from(&store).unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, autosave.id);
        assert_eq!(kept[0].text, "as previewed");
        assert_eq!(kept[0].targets, ["x", "bluesky"]);
        assert!(kept[0].sending.is_empty());
        keep(&state, &intent("new", &[Network::X]), &store).unwrap();
        assert_eq!(drafts::load_from(&store).unwrap().len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nothing_is_sent_without_a_record() {
        let dir = temp("unrecorded");
        let store = dir.join("drafts.json");
        let mut state = demo();
        let posts = state.posts.len();
        drafts::fail_after(0);
        let err = deliver(&mut state, &intent("hi", &[Network::X]), &store)
            .err()
            .unwrap();
        assert!(err.contains("nothing was sent"), "{err}");
        assert_eq!(state.posts.len(), posts);
        assert!(!store.exists());
        let err = deliver(&mut state, &intent("hi", &[]), &store)
            .err()
            .unwrap();
        assert_eq!(err, "no network to post to");
        assert_eq!(state.posts.len(), posts);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reply_and_quote_records_keep_their_fields() {
        let dir = temp("fields");
        let store = dir.join("drafts.json");
        let mut state = demo();
        let author = state.posts.get("x:p1").unwrap().author.username.clone();
        let reply = Intent {
            reply_to: Some("x:p1".into()),
            ..intent("hi", &[Network::X])
        };
        drafts::fail_after(1);
        let delivery = deliver(&mut state, &reply, &store).unwrap();
        let kept = drafts::load_from(&store)
            .unwrap()
            .into_iter()
            .find(|draft| draft.id == delivery.id)
            .unwrap();
        assert_eq!(kept.reply_to.as_deref(), Some("x:p1"));
        assert_eq!(kept.quote, None);
        assert_eq!(kept.handle.as_deref(), Some(author.as_str()));
        let quote = Intent {
            quote: Some("x:p1".into()),
            ..intent("hi", &[Network::Bluesky])
        };
        drafts::fail_after(1);
        let delivery = deliver(&mut state, &quote, &store).unwrap();
        let kept = drafts::load_from(&store)
            .unwrap()
            .into_iter()
            .find(|draft| draft.id == delivery.id)
            .unwrap();
        assert_eq!(kept.quote.as_deref(), Some("x:p1"));
        assert_eq!(kept.reply_to, None);
        assert_eq!(kept.sending, ["bluesky"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failure_fails_the_run_and_keeps_the_successes() {
        let results: Results = vec![
            (Network::X, Ok(done("https://x.com/you/status/1"))),
            (Network::Bluesky, Err("bluesky said no".into())),
        ];
        assert!(failed(&results));
        let lines: Vec<String> = lines(&results, Icons::new(false))
            .iter()
            .map(StyledLine::plain)
            .collect();
        assert_eq!(
            lines,
            [
                "✓ X        https://x.com/you/status/1",
                "✗ Bluesky  bluesky said no",
            ]
        );
        let rows = serde_json::to_value(json_rows(&results, false)).unwrap();
        assert_eq!(
            rows,
            serde_json::json!([
                {"network": "x", "ok": true, "id": "x:1", "url": "https://x.com/you/status/1"},
                {"network": "bluesky", "ok": false, "error": "bluesky said no"},
            ])
        );
        assert!(!failed(&results[..1]));
    }

    #[test]
    fn demo_rows_carry_the_text() {
        let results: Results = vec![(
            Network::Devnull,
            Ok(Done {
                demo: true,
                text: Some("hi".into()),
                ..done(DEMO)
            }),
        )];
        let rows = serde_json::to_value(json_rows(&results, false)).unwrap();
        assert_eq!(rows[0]["demo"], true);
        assert_eq!(rows[0]["text"], "hi");
        let line = lines(&results, Icons::new(false))[0].plain();
        assert_eq!(line, "✓ dev/null  demo · not posted");
    }

    #[test]
    fn follow_rows_name_the_account() {
        let results: Results = vec![(
            Network::Bluesky,
            Ok(Done {
                handle: Some("ada.bsky.social".into()),
                url: Some(Network::Bluesky.profile_url("ada.bsky.social")),
                what: "followed @ada.bsky.social on bluesky".into(),
                ..Done::default()
            }),
        )];
        let rows = serde_json::to_value(json_rows(&results, false)).unwrap();
        assert_eq!(
            rows,
            serde_json::json!([{
                "network": "bluesky",
                "ok": true,
                "handle": "ada.bsky.social",
                "url": "https://bsky.app/profile/ada.bsky.social",
            }])
        );
    }

    #[test]
    fn quotes_link_the_post_except_on_its_own_network() {
        let state = demo();
        let x_post = state.posts.get("x:p1").unwrap().clone();
        let (text, native) =
            draft_for(&state, Network::X, "t ", Some("x:p1"), ONE_SHOT_HINT).unwrap();
        assert_eq!(text, format!("t\n\n{}", x_post.permalink()));
        assert_eq!(native, None);

        let bsky = state
            .posts
            .values()
            .find(|post| post.net == Network::Bluesky)
            .unwrap()
            .clone();
        let native = draft_for(&state, Network::Bluesky, "t", Some(&bsky.id), TUI_HINT).unwrap();
        assert_eq!(native, ("t".to_string(), Some(bsky.id.clone())));
        let (text, _) = draft_for(&state, Network::X, "t", Some(&bsky.id), TUI_HINT).unwrap();
        assert_eq!(text, format!("t\n\n{}", bsky.permalink()));

        let link = "https://x.com/mina/status/42";
        let (text, native) =
            draft_for(&state, Network::Bluesky, "t", Some(link), TUI_HINT).unwrap();
        assert_eq!(
            (text.as_str(), native),
            ("t\n\nhttps://x.com/mina/status/42", None)
        );
    }

    #[test]
    fn a_quote_without_a_link_fails_with_the_fix() {
        let state = demo();
        let err =
            draft_for(&state, Network::Bluesky, "t", Some("x:404"), ONE_SHOT_HINT).unwrap_err();
        assert_eq!(
            err,
            "can't quote this post on Bluesky: its author is unknown. Pass its link: socli quote <url> \"…\""
        );
        let err = draft_for(&state, Network::X, "t", Some("x:404"), TUI_HINT).unwrap_err();
        assert_eq!(
            err,
            "can't quote this post on X: its author is unknown. Open it and press s to copy the link"
        );
    }

    #[test]
    fn dry_runs_count_against_each_limit() {
        let state = demo();
        let results = check(&state, "hello", &[Network::X, Network::Bluesky], None, None);
        let lines: Vec<String> = lines(&results, Icons::new(false))
            .iter()
            .map(StyledLine::plain)
            .collect();
        assert_eq!(
            lines,
            [
                "✓ X        would post · 5/280",
                "✓ Bluesky  would post · 5/300"
            ]
        );
        let long = "a".repeat(290);
        let results = check(&state, &long, &[Network::X, Network::Bluesky], None, None);
        assert!(failed(&results));
        assert_eq!(
            results[0].1.as_ref().err().map(String::as_str),
            Some("too long for X: 290/280")
        );
        let rows = serde_json::to_value(json_rows(&results, true)).unwrap();
        assert_eq!(
            rows[0],
            serde_json::json!({"network": "x", "ok": false, "dry_run": true, "error": "too long for X: 290/280"})
        );
        assert_eq!(rows[1]["dry_run"], true);
        assert_eq!(rows[1]["demo"], true);
        assert_eq!(rows[1]["length"], 290);
        assert_eq!(rows[1]["limit"], 300);
    }

    #[test]
    fn posting_refuses_text_over_a_limit() {
        let mut state = demo();
        let long = "a".repeat(290);
        let targets = [Network::X, Network::Bluesky];
        let sent = send(
            &mut state,
            &mut Clients::empty(),
            &long,
            &targets,
            None,
            None,
        );
        assert_eq!(
            sent[0].1.as_ref().err().map(String::as_str),
            Some("too long for X: 290/280")
        );
        assert!(sent[1].1.is_ok());
        let links: Vec<String> = (0..6).map(|i| format!("https://e.com/{i}")).collect();
        let results = check(&state, &links.join(" "), &[Network::Threads], None, None);
        assert_eq!(
            results[0].1.as_ref().err().map(String::as_str),
            Some("Threads allows at most 5 links in a post. Remove 1")
        );
    }

    #[test]
    fn dry_run_writes_check_the_caps() {
        let state = demo();
        let like = check_write(&state, Network::Devnull, &Cmd::Like { n: None });
        assert_eq!(like.as_deref(), Ok("would star"));
        let follow = check_write(&state, Network::X, &Cmd::Follow("ada".into()));
        assert_eq!(follow.as_deref(), Ok("would follow @ada"));
        let like = check_write(&state, Network::Threads, &Cmd::Like { n: None });
        assert!(like.is_err());
    }
}
