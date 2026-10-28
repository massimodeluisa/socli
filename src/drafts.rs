//! Composer drafts in `<config dir>/drafts.json`, changed in locked transactions and replaced atomically.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config;
use crate::net::Network;
use crate::util;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub id: String,
    #[serde(with = "stamp")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "stamp")]
    pub updated_at: DateTime<Utc>,
    /// Network slugs the post goes to; drafts from before quote targets call it `networks`.
    #[serde(alias = "networks")]
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// The account a reply or quote answers, for the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub text: String,
    /// Network slugs whose send started and has no recorded outcome.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sending: Vec<String>,
}

/// A short id suffix. Native uses the rng; the browser demo uses a counter so it never links `rand`.
fn draft_nonce() -> u16 {
    #[cfg(feature = "native")]
    {
        rand::random::<u16>()
    }
    #[cfg(not(feature = "native"))]
    {
        use std::cell::Cell;
        thread_local! {
            static NEXT: Cell<u16> = const { Cell::new(1) };
        }
        NEXT.with(|next| {
            let value = next.get();
            next.set(value.wrapping_add(1));
            value
        })
    }
}

impl Draft {
    pub fn new(targets: Vec<String>, reply_to: Option<String>, handle: Option<String>) -> Self {
        let now = crate::clock::utc_now();
        Self {
            id: format!("{}-{:04x}", now.timestamp_millis(), draft_nonce()),
            created_at: now,
            updated_at: now,
            targets,
            reply_to,
            quote: None,
            handle,
            text: String::new(),
            sending: Vec::new(),
        }
    }

    /// `new post`, `reply to @handle` or `quote of @handle`.
    pub fn target(&self) -> String {
        let handle = self.handle.as_deref().unwrap_or("a post");
        let at = if self.handle.is_some() { "@" } else { "" };
        match (&self.reply_to, &self.quote) {
            (Some(_), _) => format!("reply to {at}{handle}"),
            (None, Some(_)) => format!("quote of {at}{handle}"),
            (None, None) => "new post".into(),
        }
    }

    /// Whether this draft answers the same post, or is a new post when both are None.
    pub fn same_target(&self, reply_to: Option<&str>, quote: Option<&str>) -> bool {
        self.reply_to.as_deref() == reply_to && self.quote.as_deref() == quote
    }

    /// The status for a draft whose last send has no outcome; None when nothing was in flight.
    pub fn sending_status(&self) -> Option<String> {
        if self.sending.is_empty() {
            return None;
        }
        let titles: Vec<&str> = Network::ALL
            .into_iter()
            .filter(|net| self.sending.iter().any(|slug| slug == net.slug()))
            .map(Network::title)
            .collect();
        Some(format!(
            "last send may have reached {} · check before posting again",
            titles.join(", ")
        ))
    }
}

/// RFC 3339 timestamps; chrono is built without its serde feature.
mod stamp {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(at: &DateTime<Utc>, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(&at.to_rfc3339())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<DateTime<Utc>, D::Error> {
        let text = String::deserialize(input)?;
        DateTime::parse_from_rfc3339(&text)
            .map(|at| at.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

pub fn path() -> PathBuf {
    config::dir().join("drafts.json")
}

/// Every draft, newest first. A missing file has none.
///
/// Unit tests share this process. Unless `SOCLI_CONFIG_DIR` is set, they see no drafts, so a
/// composer test cannot pick up a file in `~/.config/socli`.
pub fn load() -> Result<Vec<Draft>, String> {
    // The browser demo keeps drafts in memory for the session and never reads a file.
    #[cfg(not(feature = "native"))]
    return Ok(Vec::new());
    #[cfg(test)]
    if crate::config::config_dir_var().is_none() {
        return Ok(Vec::new());
    }
    #[cfg(feature = "native")]
    load_from(&path())
}

/// Every draft in `path`, newest first.
pub fn load_from(path: &Path) -> Result<Vec<Draft>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut drafts: Vec<Draft> = serde_json::from_str(&text).map_err(|e| {
        format!(
            "{} is not valid json: {e}. Move it aside to start a new one",
            path.display()
        )
    })?;
    drafts.sort_by_key(|draft| std::cmp::Reverse(draft.updated_at));
    Ok(drafts)
}

/**
One transaction on the drafts in `path`: takes the lock, loads, applies `f`, and writes the list
back when `f` succeeds.
*/
pub fn update_at<T>(
    path: &Path,
    f: impl FnOnce(&mut Vec<Draft>) -> Result<T, String>,
) -> Result<T, String> {
    config::locked(path, || {
        let mut drafts = load_from(path)?;
        let out = f(&mut drafts)?;
        #[cfg(test)]
        injected_failure()?;
        let text = serde_json::to_string_pretty(&drafts).map_err(|e| e.to_string())?;
        config::write_atomic(path, &text)?;
        Ok(out)
    })
}

#[cfg(test)]
thread_local! {
    static FAIL_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Test seam: on this thread, the next `n` draft writes succeed and the one after fails.
#[cfg(test)]
pub fn fail_after(n: usize) {
    FAIL_AFTER.with(|left| left.set(Some(n)));
}

#[cfg(test)]
fn injected_failure() -> Result<(), String> {
    FAIL_AFTER.with(|left| match left.get() {
        Some(0) => {
            left.set(None);
            Err("injected write failure".into())
        }
        Some(n) => {
            left.set(Some(n - 1));
            Ok(())
        }
        None => Ok(()),
    })
}

/// Puts `draft` first in `drafts`, replacing the one with its id.
pub fn put(drafts: &mut Vec<Draft>, draft: Draft) {
    drafts.retain(|d| d.id != draft.id);
    drafts.insert(0, draft);
}

/// Stores `draft`, replacing the one with its id.
pub fn upsert(draft: &Draft) -> Result<(), String> {
    #[cfg(not(feature = "native"))]
    {
        let _ = draft;
        Ok(())
    }
    #[cfg(feature = "native")]
    upsert_at(&path(), draft)
}

pub fn upsert_at(path: &Path, draft: &Draft) -> Result<(), String> {
    #[cfg(not(feature = "native"))]
    {
        let _ = (path, draft);
        Err("the browser demo does not keep drafts".into())
    }
    #[cfg(feature = "native")]
    update_at(path, |drafts| {
        put(drafts, draft.clone());
        Ok(())
    })
}

pub fn remove(id: &str) -> Result<(), String> {
    #[cfg(not(feature = "native"))]
    {
        let _ = id;
        Ok(())
    }
    #[cfg(feature = "native")]
    update_at(&path(), |drafts| {
        drafts.retain(|d| d.id != id);
        Ok(())
    })
}

/// The newest draft for a target, if one was saved.
pub fn find(reply_to: Option<&str>, quote: Option<&str>) -> Option<Draft> {
    load()
        .ok()?
        .into_iter()
        .find(|draft| draft.same_target(reply_to, quote))
}

// MARK: socli drafts

/// `socli drafts`: index, age, target and first line of every draft, newest first.
pub fn list_lines(drafts: &[Draft], now: DateTime<Utc>) -> Vec<String> {
    drafts
        .iter()
        .enumerate()
        .map(|(i, draft)| {
            let text = util::clean(&draft.text);
            let first = text.lines().next().unwrap_or("");
            let target = if draft.sending.is_empty() {
                draft.target()
            } else {
                "may have posted".into()
            };
            format!(
                "{:>3}  {:<10} {:<20} {}",
                i + 1,
                util::ago_long(draft.updated_at, now),
                util::truncate(&util::clean(&target), 20),
                util::truncate(first, 60)
            )
        })
        .collect()
}

/// `socli drafts rm <index>`, counting from 1 in list order.
pub fn remove_index(index: usize) -> Result<Draft, String> {
    #[cfg(not(feature = "native"))]
    {
        let _ = index;
        Err("the browser demo does not keep drafts".into())
    }
    #[cfg(feature = "native")]
    update_at(&path(), |drafts| {
        let i = index
            .checked_sub(1)
            .filter(|i| *i < drafts.len())
            .ok_or_else(|| match drafts.len() {
                0 => "there are no drafts".to_string(),
                n => format!("no draft {index}. Run `socli drafts` to see the {n} saved"),
            })?;
        Ok(drafts.remove(i))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty directory of the test's own, removed by the test.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "socli-drafts-{name}-{}-{:08x}",
            std::process::id(),
            rand::random::<u32>()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unit_tests_do_not_read_the_home_drafts() {
        if crate::config::config_dir_var().is_some() {
            return;
        }
        assert!(load().unwrap().is_empty());
    }

    #[test]
    fn older_files_load_and_sending_is_written_only_when_set() {
        let dir = temp("format");
        let file = dir.join("drafts.json");
        fs::write(
            &file,
            r#"[{"id": "1", "created_at": "2026-10-20T10:00:00+00:00",
                "updated_at": "2026-10-20T10:00:00+00:00", "networks": ["x"], "text": "hi"}]"#,
        )
        .unwrap();
        let drafts = load_from(&file).unwrap();
        assert!(drafts[0].sending.is_empty());
        upsert_at(&file, &drafts[0]).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("sending"));
        let mut sent = drafts[0].clone();
        sent.sending = vec!["x".into(), "bluesky".into()];
        upsert_at(&file, &sent).unwrap();
        let back = load_from(&file).unwrap();
        assert_eq!(back, [sent.clone()]);
        assert_eq!(
            back[0].sending_status().as_deref(),
            Some("last send may have reached X, Bluesky · check before posting again")
        );
        let line = &list_lines(&back, Utc::now())[0];
        assert!(line.contains("may have posted"), "{line}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_upserts_of_distinct_drafts_all_survive() {
        let dir = temp("upserts");
        let file = dir.join("drafts.json");
        std::thread::scope(|scope| {
            for thread in 0..8 {
                let file = &file;
                scope.spawn(move || {
                    for n in 0..25 {
                        let mut draft = Draft::new(vec!["x".into()], None, None);
                        draft.id = format!("{thread}-{n}");
                        upsert_at(file, &draft).unwrap();
                    }
                });
            }
        });
        assert_eq!(load_from(&file).unwrap().len(), 200);
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !names.iter().any(|name| name.ends_with(".tmp")),
            "{names:?}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_transaction_leaves_the_file_as_it_was() {
        let dir = temp("failed");
        let file = dir.join("drafts.json");
        let draft = Draft::new(vec!["x".into()], None, None);
        upsert_at(&file, &draft).unwrap();
        let before = fs::read_to_string(&file).unwrap();
        fail_after(0);
        let other = Draft::new(vec!["bluesky".into()], None, None);
        assert_eq!(
            upsert_at(&file, &other).unwrap_err(),
            "injected write failure"
        );
        let refused: Result<(), String> = update_at(&file, |drafts| {
            drafts.clear();
            Err("no".into())
        });
        assert!(refused.is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), before);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn targets_name_the_answered_account() {
        let mut draft = Draft::new(vec!["x".into()], None, None);
        assert_eq!(draft.target(), "new post");
        assert!(draft.same_target(None, None));
        draft.reply_to = Some("x:1".into());
        draft.handle = Some("nuno".into());
        assert_eq!(draft.target(), "reply to @nuno");
        assert!(draft.same_target(Some("x:1"), None));
        assert!(!draft.same_target(None, None));
        draft.reply_to = None;
        draft.quote = Some("x:2".into());
        assert_eq!(draft.target(), "quote of @nuno");
    }
}
