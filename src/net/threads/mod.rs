/*!
Threads (Meta Graph API at `graph.threads.com`). Reads, writes, and the sign-in flows live here.

The token travels as the `access_token` parameter, so no error text may carry a request URL.
Docs: https://developers.facebook.com/documentation/threads
*/

mod auth;
mod map;

use std::time::Duration;

use crate::net::http::{Client, RequestBuilder};
use serde_json::Value;

use crate::config::{self, ThreadsConfig};
use crate::model::{Page, Post, User};
use crate::net::{http, scoped, split, Caps, Draft, FeedKind, NetError, Network, Provider};

pub use auth::{begin_page, finish_page, login, logout, oauth, page_copy, refresh};

const API: &str = "https://graph.threads.com/v1.0";
const MAX_CONVERSATION_PAGES: usize = 5;
const MAX_EXPANSIONS: usize = 10;
const CONTAINER_POLLS: usize = 5;

pub struct ThreadsClient {
    http: Client,
    token: String,
    user_id: Option<String>,
    username: Option<String>,
}

impl ThreadsClient {
    /// A client when the `[threads]` section or `THREADS_ACCESS_TOKEN` holds a token.
    pub fn from_config(section: &ThreadsConfig) -> Option<Self> {
        let http = http::client().ok()?;
        let env = std::env::var("THREADS_ACCESS_TOKEN")
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let token = match env {
            Some(token) => token,
            None => {
                let stored = section.access_token.clone().filter(|t| !t.is_empty())?;
                auth::auto_refresh(&http, section, stored)
            }
        };
        Some(ThreadsClient {
            http,
            token,
            user_id: section.user_id.clone(),
            username: section.username.clone(),
        })
    }

    fn with_token(http: Client, token: String) -> Self {
        ThreadsClient {
            http,
            token,
            user_id: None,
            username: None,
        }
    }

    fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, NetError> {
        send(
            self.http
                .get(format!("{API}{path}"))
                .query(&[("access_token", self.token.as_str())])
                .query(query),
        )
    }

    fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<Value, NetError> {
        let mut fields = form.to_vec();
        fields.push(("access_token", self.token.as_str()));
        send(self.http.post(format!("{API}{path}")).form(&fields))
    }

    /// The signed-in profile with a scoped id.
    fn profile(&self) -> Result<User, NetError> {
        let value = self.get("/me", &[("fields", map::FIELDS_PROFILE)])?;
        map::profile(&value).ok_or_else(|| NetError::Parse {
            net: Network::Threads,
            message: "the profile had no id".into(),
        })
    }

    fn list(
        &self,
        path: &str,
        fields: &str,
        cursor: Option<&str>,
        count: u8,
        extra: &[(&str, &str)],
    ) -> Result<Page, NetError> {
        let limit = usize::from(count).clamp(1, 100);
        let limit_text = limit.to_string();
        let mut query = vec![("fields", fields), ("limit", limit_text.as_str())];
        query.extend_from_slice(extra);
        if let Some(after) = cursor {
            query.push(("after", after));
        }
        let body = self.get(path, &query)?;
        let mut page = map::page(&body, limit);
        self.expand(&mut page);
        self.claim_own(&mut page.posts);
        Ok(page)
    }

    /// Fetches repost originals and quote targets that arrived as bare ids. A failed fetch leaves the stub.
    fn expand(&self, page: &mut Page) {
        let mut budget = MAX_EXPANSIONS;
        for post in &mut page.posts {
            if budget > 0 && map::is_stub(post) {
                budget -= 1;
                if let Some(mut full) = self.fetch_post(&post.id) {
                    full.reposted_by = post.reposted_by.take();
                    *post = full;
                }
            }
            if let Some(quoted) = post.quoted.as_mut().filter(|q| map::is_stub(q)) {
                if budget > 0 {
                    budget -= 1;
                    if let Some(full) = self.fetch_post(&quoted.id) {
                        **quoted = full;
                    }
                }
            }
        }
    }

    fn fetch_post(&self, scoped_id: &str) -> Option<Post> {
        let (_, native) = split(scoped_id)?;
        let value = self
            .get(&format!("/{native}"), &[("fields", map::FIELDS_PUBLIC)])
            .ok()?;
        map::post(&value)
    }

    fn claim_own(&self, posts: &mut [Post]) {
        if let Some(username) = &self.username {
            map::claim_own(posts, username, self.user_id.as_deref().unwrap_or("me"));
        }
    }

    /// Text containers are normally ready at once; this only waits when Meta reports one still processing.
    fn wait_for_container(&self, creation_id: &str) -> Result<(), NetError> {
        for attempt in 0..CONTAINER_POLLS {
            let Ok(status) = self.get(
                &format!("/{creation_id}"),
                &[("fields", "status,error_message")],
            ) else {
                return Ok(());
            };
            match map::container(&status) {
                map::Container::Ready => return Ok(()),
                map::Container::Failed(reason) => {
                    return Err(NetError::Http {
                        net: Network::Threads,
                        status: 400,
                        message: format!("Threads could not prepare the post: {reason}. check the text and try again"),
                    })
                }
                map::Container::Pending if attempt + 1 < CONTAINER_POLLS => {
                    crate::clock::pause(Duration::from_secs(2));
                }
                map::Container::Pending => {}
            }
        }
        Ok(())
    }
}

/// Sends a request and decodes the body, with the request URL kept out of every error.
fn send(request: RequestBuilder) -> Result<Value, NetError> {
    let response = request.send().map_err(transport)?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get(http::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    let body = response.text().map_err(transport)?;
    map::decode(status, retry_after, &body)
}

fn transport(err: http::Error) -> NetError {
    NetError::Other(format!(
        "could not reach threads: {}. check your connection and try again",
        err.without_url()
    ))
}

fn native(id: &str) -> Result<String, NetError> {
    match split(id) {
        Some((Network::Threads, native)) => Ok(native.to_string()),
        Some((other, _)) => Err(NetError::Other(format!(
            "{id} belongs to {}, not Threads",
            other.title()
        ))),
        None => Ok(id.to_string()),
    }
}

fn created_id(value: &Value) -> Result<String, NetError> {
    map::text_of(value, "id")
        .map(String::from)
        .ok_or_else(|| NetError::Parse {
            net: Network::Threads,
            message: "the response had no id".into(),
        })
}

/** Threads counts emoji and symbol characters as their UTF-8 bytes against the 500 limit. */
pub fn text_len(text: &str) -> usize {
    super::caps::threads_text_len(text)
}

/// Unique http and https URLs in the text; Threads counts each as a link.
#[cfg(test)]
fn link_count(text: &str) -> usize {
    super::caps::threads_link_count(text)
}

/// Rejects empty text, text over the 500 limit and more than 5 links before any request.
pub fn check_text(text: &str) -> Result<(), NetError> {
    super::caps::threads_check_text(text)
}

pub fn caps() -> Caps {
    super::caps::threads()
}

impl Provider for ThreadsClient {
    fn network(&self) -> Network {
        Network::Threads
    }

    fn caps(&self) -> Caps {
        caps()
    }

    fn signed_in(&self) -> bool {
        true
    }

    fn account(&self) -> Option<User> {
        let username = self.username.clone()?;
        Some(User {
            id: scoped(
                Network::Threads,
                self.user_id.as_deref().unwrap_or(username.as_str()),
            ),
            name: username.clone(),
            username,
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        })
    }

    fn me(&mut self) -> Result<User, NetError> {
        let user = self.profile()?;
        self.username = Some(user.username.clone());
        self.user_id = split(&user.id).map(|(_, native)| native.to_string());
        Ok(user)
    }

    fn feed(&mut self, kind: FeedKind, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        match kind {
            FeedKind::Mine => self.list("/me/threads", map::FIELDS_OWN, cursor, count, &[]),
            other => Err(NetError::unsupported(
                Network::Threads,
                format!("{} feed", other.word()),
            )),
        }
    }

    fn mentions(&mut self, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.list("/me/mentions", map::FIELDS_PUBLIC, cursor, count, &[])
    }

    fn thread(&mut self, id: &str, _count: u8) -> Result<Page, NetError> {
        let id = native(id)?;
        let fields = [("fields", map::FIELDS_PUBLIC)];
        let target = self.get(&format!("/{id}"), &fields)?;
        let root_id = map::ref_id(target.get("root_post"))
            .unwrap_or(id.as_str())
            .to_string();
        let root = if root_id == id {
            target
        } else {
            self.get(&format!("/{root_id}"), &fields)?
        };
        let parse = || NetError::Parse {
            net: Network::Threads,
            message: "the post had no id".into(),
        };
        let mut posts = vec![map::post(&root).ok_or_else(parse)?];
        let has_replies = root.get("has_replies").and_then(Value::as_bool) != Some(false);
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_CONVERSATION_PAGES {
            if !has_replies {
                break;
            }
            let mut query = vec![
                ("fields", map::FIELDS_CONVERSATION),
                ("reverse", "false"),
                ("limit", "100"),
            ];
            if let Some(after) = cursor.as_deref() {
                query.push(("after", after));
            }
            let body = self.get(&format!("/{root_id}/conversation"), &query)?;
            let replies = map::page(&body, 100);
            posts.extend(replies.posts);
            cursor = replies.next;
            if cursor.is_none() {
                break;
            }
        }
        let mut seen = Vec::new();
        posts.retain(|post| {
            let fresh = !seen.contains(&post.id);
            seen.push(post.id.clone());
            fresh
        });
        self.claim_own(&mut posts);
        Ok(Page {
            ids: posts.iter().map(|p| p.id.clone()).collect(),
            posts,
            next: None,
        })
    }

    fn user(
        &mut self,
        handle: &str,
        cursor: Option<&str>,
        count: u8,
    ) -> Result<(User, Page), NetError> {
        let wanted = handle.trim().trim_start_matches('@');
        let mine = self
            .username
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(wanted));
        if !(wanted.is_empty() || wanted.eq_ignore_ascii_case("me") || mine) {
            return Err(NetError::unsupported(
                Network::Threads,
                "profiles of other accounts",
            ));
        }
        let user = self.me()?;
        let page = self.feed(FeedKind::Mine, cursor, count)?;
        Ok((user, page))
    }

    fn search(&mut self, query: &str, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.list(
            "/keyword_search",
            map::FIELDS_PUBLIC,
            cursor,
            count,
            &[("q", query), ("search_type", "RECENT")],
        )
    }

    fn post(&mut self, draft: &Draft) -> Result<Post, NetError> {
        check_text(&draft.text)?;
        let reply_to = draft.reply_to.as_deref().map(native).transpose()?;
        let quote = draft.quote.as_deref().map(native).transpose()?;
        let mut form = vec![("media_type", "TEXT"), ("text", draft.text.as_str())];
        if let Some(target) = reply_to.as_deref() {
            form.push(("reply_to_id", target));
        }
        if let Some(target) = quote.as_deref() {
            form.push(("quote_post_id", target));
        }
        let creation_id = created_id(&self.post_form("/me/threads", &form)?)?;
        self.wait_for_container(&creation_id)?;
        let published = self.post_form("/me/threads_publish", &[("creation_id", &creation_id)])?;
        let id = created_id(&published)?;
        let mut detail = self
            .get(
                &format!("/{id}"),
                &[("fields", "permalink,text,timestamp,username")],
            )
            .unwrap_or_else(|_| serde_json::json!({ "text": draft.text }));
        detail["id"] = Value::String(id);
        let mut post = map::post(&detail).ok_or_else(|| NetError::Parse {
            net: Network::Threads,
            message: "the published post had no id".into(),
        })?;
        post.in_reply_to = draft.reply_to.clone();
        self.claim_own(std::slice::from_mut(&mut post));
        Ok(post)
    }

    fn delete(&mut self, id: &str) -> Result<(), NetError> {
        let id = native(id)?;
        let body = send(
            self.http
                .delete(format!("{API}/{id}"))
                .query(&[("access_token", self.token.as_str())]),
        )?;
        if body.get("success").and_then(Value::as_bool) == Some(false) {
            return Err(NetError::Other(
                "Threads did not delete the post. check that it is yours and try again".into(),
            ));
        }
        Ok(())
    }

    fn set_like(&mut self, _: &str, _: bool) -> Result<(), NetError> {
        Err(NetError::unsupported(Network::Threads, "likes"))
    }

    fn set_repost(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        if !on {
            return Err(NetError::unsupported(Network::Threads, "repost undo"));
        }
        let id = native(id)?;
        self.post_form(&format!("/{id}/repost"), &[]).map(|_| ())
    }

    fn set_bookmark(&mut self, _: &str, _: bool) -> Result<(), NetError> {
        Err(NetError::unsupported(Network::Threads, "bookmarks"))
    }

    fn set_follow(&mut self, _: &str, _: bool) -> Result<(), NetError> {
        Err(NetError::unsupported(Network::Threads, "follows"))
    }
}

/// The `threads` rows of `socli auth status`, the token redacted.
pub fn status(section: &ThreadsConfig) -> Vec<(&'static str, String)> {
    let env = std::env::var("THREADS_ACCESS_TOKEN").ok();
    let token = env.as_deref().or(section.access_token.as_deref());
    let account = match (token, &section.username) {
        (Some(_), Some(user)) => format!("@{user}"),
        (Some(_), None) => "signed in".into(),
        (None, _) => "not signed in".into(),
    };
    vec![("account", account), ("token", config::redact(token))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_match_the_threads_feature_set() {
        let caps = caps();
        assert_eq!(caps.feeds, vec![(FeedKind::Mine, "Your threads")]);
        assert!(caps.repost && !caps.unrepost);
        assert!(caps.quote && caps.delete && caps.thread && caps.post);
        assert!(!caps.like && !caps.bookmark && !caps.follow);
        assert_eq!(caps.max_chars, 500);
    }

    #[test]
    fn five_hundred_characters_are_allowed() {
        assert!(check_text(&"a".repeat(500)).is_ok());
    }

    #[test]
    fn five_hundred_and_one_characters_are_rejected() {
        let err = check_text(&"a".repeat(501)).unwrap_err().to_string();
        assert!(err.contains("500"));
        assert!(err.contains("501"));
        assert!(err.contains("shorten"));
    }

    #[test]
    fn empty_text_is_rejected() {
        assert!(check_text("  \n").is_err());
    }

    #[test]
    fn native_ids_accept_bare_and_scoped_threads_ids() {
        assert_eq!(native("123").ok().as_deref(), Some("123"));
        assert_eq!(
            native(&scoped(Network::Threads, "123")).ok().as_deref(),
            Some("123")
        );
        assert!(native(&scoped(Network::X, "123")).is_err());
    }

    #[test]
    fn unsupported_actions_name_the_missing_feature() {
        let mut client = ThreadsClient::with_token(Client::new(), "t".into());
        assert!(matches!(
            client.set_like("1", true),
            Err(NetError::Unsupported { .. })
        ));
        assert!(matches!(
            client.set_bookmark("1", true),
            Err(NetError::Unsupported { .. })
        ));
        assert!(matches!(
            client.set_follow("1", true),
            Err(NetError::Unsupported { .. })
        ));
        assert!(matches!(
            client.set_repost("1", false),
            Err(NetError::Unsupported { .. })
        ));
        assert!(matches!(
            client.feed(FeedKind::Latest, None, 10),
            Err(NetError::Unsupported { .. })
        ));
    }

    #[test]
    fn over_long_posts_fail_before_any_request() {
        let mut client = ThreadsClient::with_token(Client::new(), "t".into());
        let draft = Draft {
            text: "a".repeat(501),
            reply_to: None,
            quote: None,
        };
        assert!(matches!(client.post(&draft), Err(NetError::Other(_))));
    }

    #[test]
    fn status_redacts_the_token() {
        let section = ThreadsConfig {
            username: Some("ada".into()),
            access_token: Some("THAA-secret-token-value".into()),
            ..ThreadsConfig::default()
        };
        let rows = status(&section);
        assert!(rows.iter().all(|(_, v)| !v.contains("secret-token-value")));
    }

    #[test]
    fn text_len_counts_ascii_and_accents_per_char() {
        assert_eq!(text_len("hello"), 5);
        assert_eq!(text_len("perché città"), 12);
    }

    #[test]
    fn text_len_counts_emoji_as_bytes() {
        assert_eq!(text_len("\u{1F44D}"), 4);
        assert_eq!(text_len("a\u{1F44D}"), 5);
    }

    #[test]
    fn text_len_sums_a_zwj_family() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        assert_eq!(text_len(family), 4 + 3 + 4 + 3 + 4);
    }

    #[test]
    fn text_len_counts_a_flag() {
        assert_eq!(text_len("\u{1F1EE}\u{1F1F9}"), 8);
    }

    #[test]
    fn emoji_bytes_count_toward_the_limit() {
        assert!(check_text(&"\u{1F44D}".repeat(125)).is_ok());
        assert!(check_text(&"\u{1F44D}".repeat(126)).is_err());
    }

    fn links(n: usize) -> String {
        (0..n)
            .map(|i| format!("https://example.com/{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn five_links_are_allowed() {
        assert!(check_text(&links(5)).is_ok());
    }

    #[test]
    fn six_links_are_rejected() {
        let err = check_text(&links(6)).unwrap_err().to_string();
        assert_eq!(err, "Threads allows at most 5 links in a post. Remove 1");
    }

    #[test]
    fn repeated_links_count_once() {
        let text = format!("{} {}", links(5), "https://example.com/0");
        assert_eq!(link_count(&text), 5);
        assert!(check_text(&text).is_ok());
    }
}
