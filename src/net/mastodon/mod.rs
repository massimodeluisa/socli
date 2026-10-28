/*!
Mastodon through its REST API on one server: the signed-in account's server, or mastodon.social for
public reads. A status id is only valid on the server that issued it, so a link from another server
is resolved to a local id through search before use. Docs: https://docs.joinmastodon.org/methods/
*/

mod auth;
mod html;
mod map;

pub use auth::{begin_page, finish_page, login, logout};

use crate::net::http::{Client, Method};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use url::Url;

use crate::config::{self, MastodonConfig};
use crate::model::{Page, Post, User};
use crate::net::pkce::code_verifier;
use crate::net::{
    http, is_digits, scoped, split, Caps, Draft, FeedKind, NetError, Network, Provider,
};

pub use super::caps::MASTODON_DEFAULT_SERVER as DEFAULT_SERVER;

pub fn max_chars() -> usize {
    super::caps::mastodon_limit()
}

pub fn caps(session: bool) -> Caps {
    super::caps::mastodon(session)
}

pub fn caps_with(session: bool, max_chars: usize) -> Caps {
    super::caps::mastodon_with(session, max_chars)
}

pub fn text_len(text: &str) -> usize {
    super::caps::mastodon_text_len(text)
}

pub fn profile_url(handle: &str) -> String {
    super::caps::mastodon_profile_url(handle)
}

// MARK: cursors

/** The `rel="next"` URL of a `Link` header as a cursor: its `max_id` or `offset` pair, as `key=value`. */
fn next_cursor(link: Option<&str>) -> Option<String> {
    let part = link?.split(',').find(|p| p.contains("rel=\"next\""))?;
    let target = part.split_once('<')?.1.split_once('>')?.0;
    Url::parse(target)
        .ok()?
        .query_pairs()
        .find(|(key, value)| (key == "max_id" || key == "offset") && is_digits(value))
        .map(|(key, value)| format!("{key}={value}"))
}

/// The query pair a cursor stands for; a bare number is a `max_id`.
fn cursor_pair(cursor: Option<&str>) -> Option<(&'static str, String)> {
    let cursor = cursor?;
    match cursor.split_once('=') {
        Some(("max_id", value)) if is_digits(value) => Some(("max_id", value.to_string())),
        Some(("offset", value)) if is_digits(value) => Some(("offset", value.to_string())),
        Some(_) => None,
        None if is_digits(cursor) => Some(("max_id", cursor.to_string())),
        None => None,
    }
}

fn page_cursor(cursor: Option<&str>) -> Result<Option<(&'static str, String)>, NetError> {
    cursor
        .map(|value| {
            cursor_pair(Some(value)).ok_or_else(|| {
                NetError::Other(
                    "invalid Mastodon page cursor. refresh the feed and try again".into(),
                )
            })
        })
        .transpose()
}

fn search_offset(cursor: Option<&str>, count: u8) -> Result<usize, NetError> {
    let offset = match page_cursor(cursor)? {
        Some(("offset", value)) => value.parse::<usize>().ok(),
        None => Some(0),
        _ => None,
    };
    offset
        .filter(|offset| {
            offset
                .checked_add(usize::from(count.clamp(1, 40)))
                .is_some()
        })
        .ok_or_else(|| {
            NetError::Other("invalid Mastodon search cursor. run the search again".into())
        })
}

fn tag_path(word: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(word.as_bytes()).collect();
    format!("/api/v1/timelines/tag/{encoded}")
}

fn confirm_action(value: &Value, field: &str, on: bool) -> Result<(), NetError> {
    if value.get(field).and_then(Value::as_bool) == Some(on) {
        Ok(())
    } else {
        Err(NetError::OutcomeUnknown {
            net: Network::Mastodon,
            message: "Mastodon did not confirm the requested change".into(),
        })
    }
}

/// Seconds until an ISO 8601 `X-RateLimit-Reset`, at least 1.
fn reset_wait(reset: Option<&str>, now: DateTime<Utc>) -> Option<u64> {
    let at = DateTime::parse_from_rfc3339(reset?).ok()?;
    let secs = (at.with_timezone(&Utc) - now).num_seconds().max(1);
    u64::try_from(secs).ok()
}

// MARK: client

pub struct MastodonClient {
    /// Host name, never a URL.
    server: String,
    token: Option<String>,
    user_id: Option<String>,
    /// The full handle, `user@server`.
    username: Option<String>,
    http: Option<Client>,
}

enum Body<'a> {
    None,
    Json(&'a Value),
}

struct Answer {
    value: Value,
    /// The `rel="next"` page as a cursor.
    next: Option<String>,
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn resolved_section(
    section: &MastodonConfig,
    env: impl Fn(&str) -> Option<String>,
) -> MastodonConfig {
    let stored_server = clean(section.server.clone()).unwrap_or_else(|| DEFAULT_SERVER.into());
    let selected_server = clean(env("MASTODON_SERVER")).unwrap_or_else(|| stored_server.clone());
    let server = auth::normalize_server(&selected_server).unwrap_or(selected_server);
    let same_server = auth::normalize_server(&stored_server).as_deref() == Ok(server.as_str());
    let stored_token = clean(section.access_token.clone());
    let token = clean(env("MASTODON_ACCESS_TOKEN"))
        .or_else(|| same_server.then(|| stored_token.clone()).flatten());
    if same_server && token == stored_token {
        MastodonConfig {
            server: Some(server),
            access_token: token,
            ..section.clone()
        }
    } else {
        MastodonConfig {
            server: Some(server),
            access_token: token,
            max_chars: same_server.then_some(section.max_chars).flatten(),
            ..MastodonConfig::default()
        }
    }
}

fn native(id: &str) -> Result<&str, NetError> {
    match split(id) {
        Some((Network::Mastodon, native)) => Ok(native),
        Some((other, _)) => Err(NetError::Other(format!(
            "{id} belongs to {}, not Mastodon",
            other.title()
        ))),
        None => Ok(id),
    }
}

fn limit(count: u8) -> String {
    count.clamp(1, 40).to_string()
}

impl MastodonClient {
    pub fn new(section: &MastodonConfig) -> Self {
        Self::from_section(section, |key| std::env::var(key).ok())
    }

    fn from_section(section: &MastodonConfig, env: impl Fn(&str) -> Option<String>) -> Self {
        let section = resolved_section(section, env);
        super::caps::set_mastodon_limit(
            section
                .max_chars
                .filter(|n| *n > 0)
                .unwrap_or(super::caps::MASTODON_DEFAULT_MAX),
        );
        MastodonClient {
            server: section.server.unwrap_or_else(|| DEFAULT_SERVER.into()),
            token: section.access_token,
            user_id: section.user_id,
            username: section.username,
            http: None,
        }
    }

    /// Signed out: public threads and profiles on mastodon.social.
    pub fn public() -> Self {
        MastodonClient {
            server: DEFAULT_SERVER.to_string(),
            token: None,
            user_id: None,
            username: None,
            http: None,
        }
    }

    fn session(&self) -> Result<&str, NetError> {
        self.token
            .as_deref()
            .ok_or(NetError::SignInRequired(Network::Mastodon))
    }

    fn http(&mut self) -> Result<Client, NetError> {
        if let Some(client) = &self.http {
            return Ok(client.clone());
        }
        let client = http::client()?;
        self.http = Some(client.clone());
        Ok(client)
    }

    fn base(&self) -> String {
        format!("https://{}", self.server)
    }

    // MARK: transport

    fn call(
        &mut self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Body<'_>,
    ) -> Result<Answer, NetError> {
        let client = self.http()?;
        let url = format!("{}{path}", self.base());
        let creates = method == Method::POST && path == "/api/v1/statuses";
        let mutation = method != Method::GET;
        let mut request = client.request(method, &url).query(query);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        if let Body::Json(value) = body {
            request = request.json(value);
        }
        if creates {
            request = request.header("Idempotency-Key", code_verifier());
        }
        let response = request
            .send()
            .map_err(|error| http::transport_error(Network::Mastodon, error, mutation))?;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let (link, reset, retry) = (
            header("link"),
            header("x-ratelimit-reset"),
            header("retry-after"),
        );
        let body = http::read_text(response).map_err(|message| {
            if mutation && (200..300).contains(&status) {
                NetError::OutcomeUnknown {
                    net: Network::Mastodon,
                    message,
                }
            } else {
                NetError::Other(format!("could not read the Mastodon response: {message}"))
            }
        })?;
        if (200..300).contains(&status) {
            let value = if body.trim().is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&body).map_err(|error| {
                    if mutation {
                        NetError::OutcomeUnknown {
                            net: Network::Mastodon,
                            message: format!("could not read the Mastodon response: {error}"),
                        }
                    } else {
                        NetError::Parse {
                            net: Network::Mastodon,
                            message: error.to_string(),
                        }
                    }
                })?
            };
            return Ok(Answer {
                value,
                next: next_cursor(link.as_deref()),
            });
        }
        if status == 429 {
            return Err(NetError::RateLimited {
                net: Network::Mastodon,
                retry_after: reset_wait(reset.as_deref(), crate::clock::utc_now())
                    .or_else(|| retry.and_then(|v| v.trim().parse().ok())),
            });
        }
        if status == 401 && self.token.is_some() {
            return Err(NetError::Http {
                net: Network::Mastodon,
                status,
                message: format!(
                    "{} no longer accepts the saved token. run `socli login mastodon` again",
                    self.server
                ),
            });
        }
        if mutation && status >= 500 {
            return Err(NetError::OutcomeUnknown {
                net: Network::Mastodon,
                message: http::error_text(&body)
                    .unwrap_or_else(|| format!("Mastodon returned {status}")),
            });
        }
        Err(NetError::Http {
            net: Network::Mastodon,
            status,
            message: http::error_text(&body)
                .unwrap_or_else(|| format!("Mastodon returned {status}")),
        })
    }

    fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Answer, NetError> {
        self.call(Method::GET, path, query, Body::None)
    }

    fn post_to(&mut self, path: &str, body: Body<'_>) -> Result<Answer, NetError> {
        self.call(Method::POST, path, &[], body)
    }

    /// A page of statuses (or notifications): `limit` and the cursor are added to `query`.
    fn list(
        &mut self,
        path: &str,
        mut query: Vec<(&str, String)>,
        cursor: Option<&str>,
        count: u8,
    ) -> Result<(Vec<Value>, Option<String>), NetError> {
        query.push(("limit", limit(count)));
        if let Some((key, value)) = page_cursor(cursor)? {
            query.push((key, value));
        }
        let answer = self.get(path, &query)?;
        let items = answer.value.as_array().cloned().unwrap_or_default();
        Ok((items, answer.next))
    }

    // MARK: lookups

    fn status_id(&mut self, id: &str) -> Result<String, NetError> {
        let native = native(id)?;
        if is_digits(native) {
            return Ok(native.to_string());
        }
        let not_a_post = || NetError::Other(format!("{id} is not a Mastodon post id or link"));
        if !(native.starts_with("https://") || native.starts_with("http://")) {
            return Err(not_a_post());
        }
        let url = Url::parse(native).map_err(|_| not_a_post())?;
        let host = url.host_str().unwrap_or("").to_string();
        if host.eq_ignore_ascii_case(&self.server) {
            let last = url
                .path_segments()
                .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
                .filter(|segment| is_digits(segment));
            if let Some(last) = last {
                return Ok(last.to_string());
            }
        }
        if self.token.is_none() {
            return Err(NetError::Other(format!(
                "sign in to open posts from {host}: `socli login mastodon`"
            )));
        }
        let query = [
            ("q", native.to_string()),
            ("type", "statuses".to_string()),
            ("resolve", "true".to_string()),
            ("limit", "1".to_string()),
        ];
        let found = self.get("/api/v2/search", &query)?;
        found
            .value
            .pointer("/statuses/0/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| NetError::Other(format!("Mastodon could not find {native}")))
    }

    fn account_id(&mut self, handle: &str) -> Result<Value, NetError> {
        let handle: String = handle.chars().filter(|c| !c.is_whitespace()).collect();
        let handle = handle.strip_prefix('@').unwrap_or(&handle).to_string();
        let missing = || NetError::Other(format!("Mastodon has no @{handle}"));
        match self.get("/api/v1/accounts/lookup", &[("acct", handle.clone())]) {
            Ok(answer) => return Ok(answer.value),
            Err(NetError::Http { status: 404, .. }) if self.token.is_some() => {}
            Err(NetError::Http { status: 404, .. }) => return Err(missing()),
            Err(other) => return Err(other),
        }
        let query = [
            ("q", format!("@{handle}")),
            ("type", "accounts".to_string()),
            ("resolve", "true".to_string()),
            ("limit", "1".to_string()),
        ];
        let found = self.get("/api/v2/search", &query)?;
        found
            .value
            .pointer("/accounts/0")
            .cloned()
            .ok_or_else(missing)
    }

    fn status(&mut self, id: &str) -> Result<Value, NetError> {
        let local = self.status_id(id)?;
        Ok(self.get(&format!("/api/v1/statuses/{local}"), &[])?.value)
    }

    /// Favourite, reblog and bookmark: `verb` is the endpoint's last segment.
    fn act(&mut self, id: &str, verb: &str) -> Result<(), NetError> {
        self.session()?;
        let local = self.status_id(id)?;
        let answer = self.post_to(&format!("/api/v1/statuses/{local}/{verb}"), Body::None)?;
        let (field, on) = match verb {
            "favourite" => ("favourited", true),
            "unfavourite" => ("favourited", false),
            "reblog" => ("reblogged", true),
            "unreblog" => ("reblogged", false),
            "bookmark" => ("bookmarked", true),
            "unbookmark" => ("bookmarked", false),
            _ => return Err(NetError::Other("unknown Mastodon action".into())),
        };
        confirm_action(&answer.value, field, on)
    }
}

// MARK: provider

impl Provider for MastodonClient {
    fn network(&self) -> Network {
        Network::Mastodon
    }

    fn caps(&self) -> Caps {
        caps(self.token.is_some())
    }

    fn signed_in(&self) -> bool {
        self.token.is_some()
    }

    fn account(&self) -> Option<User> {
        let username = self.username.as_ref().filter(|_| self.token.is_some())?;
        Some(User {
            id: scoped(Network::Mastodon, self.user_id.as_deref().unwrap_or("")),
            name: username.clone(),
            username: username.clone(),
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        })
    }

    fn me(&mut self) -> Result<User, NetError> {
        self.session()?;
        let answer = self.get("/api/v1/accounts/verify_credentials", &[])?;
        let user = map::user(&answer.value, &self.server)
            .ok_or_else(|| NetError::Other("Mastodon returned an empty profile".into()))?;
        self.user_id = split(&user.id).map(|(_, id)| id.to_string());
        self.username = Some(user.username.clone());
        Ok(user)
    }

    fn feed(&mut self, kind: FeedKind, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.session()?;
        let path = match kind {
            FeedKind::Following => "/api/v1/timelines/home",
            FeedKind::Trending => "/api/v1/trends/statuses",
            FeedKind::Bookmarks => "/api/v1/bookmarks",
            other => {
                return Err(NetError::unsupported(
                    Network::Mastodon,
                    format!("{} feed", other.word()),
                ))
            }
        };
        let (items, next) = self.list(path, Vec::new(), cursor, count)?;
        let mut page = map::statuses_page(&items, next, &self.server);
        if kind == FeedKind::Bookmarks {
            page.posts.iter_mut().for_each(|p| p.bookmarked = true);
        }
        Ok(page)
    }

    fn mentions(&mut self, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.session()?;
        let query = vec![("types[]", "mention".to_string())];
        let (items, next) = self.list("/api/v1/notifications", query, cursor, count)?;
        Ok(map::notifications_page(&items, next, &self.server))
    }

    fn thread(&mut self, id: &str, _count: u8) -> Result<Page, NetError> {
        let requested = native(id)?;
        let local = self.status_id(id)?;
        let status = self.get(&format!("/api/v1/statuses/{local}"), &[])?.value;
        let context = self
            .get(&format!("/api/v1/statuses/{local}/context"), &[])?
            .value;
        let mut page = map::thread_page(&status, &context, &self.server)
            .ok_or_else(|| NetError::Other("Mastodon could not find that post".into()))?;
        if requested != local {
            map::rename(
                &mut page,
                &scoped(Network::Mastodon, &local),
                &scoped(Network::Mastodon, requested),
            );
        }
        Ok(page)
    }

    fn user(
        &mut self,
        handle: &str,
        cursor: Option<&str>,
        count: u8,
    ) -> Result<(User, Page), NetError> {
        let account = self.account_id(handle)?;
        let user = map::user(&account, &self.server).ok_or_else(|| {
            NetError::Other(format!(
                "Mastodon has no @{}",
                handle.trim_start_matches('@')
            ))
        })?;
        let id = account
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let query = vec![("exclude_replies", "true".to_string())];
        let (items, next) = self.list(
            &format!("/api/v1/accounts/{id}/statuses"),
            query,
            cursor,
            count,
        )?;
        Ok((user, map::statuses_page(&items, next, &self.server)))
    }

    fn search(&mut self, query: &str, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.session()?;
        let query = query.trim();
        let tag = query.strip_prefix('#').filter(|word| {
            !word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || c == '_')
        });
        if let Some(word) = tag {
            let (items, next) = self.list(&tag_path(word), Vec::new(), cursor, count)?;
            return Ok(map::statuses_page(&items, next, &self.server));
        }
        let asked = limit(count);
        let offset = search_offset(cursor, count)?;
        let params = [
            ("q", query.to_string()),
            ("type", "statuses".to_string()),
            ("limit", asked.clone()),
            ("offset", offset.to_string()),
        ];
        let answer = self.get("/api/v2/search", &params)?;
        let items = answer
            .value
            .get("statuses")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let next = if asked.parse::<usize>() == Ok(items.len()) {
            offset
                .checked_add(items.len())
                .map(|next| format!("offset={next}"))
        } else {
            None
        };
        Ok(map::statuses_page(&items, next, &self.server))
    }

    fn post(&mut self, draft: &Draft) -> Result<Post, NetError> {
        self.session()?;
        if self.account().is_none() {
            self.me()?;
        }
        let mut text = draft.text.clone();
        let mut body = json!({});
        if let Some(reply_to) = draft.reply_to.as_deref() {
            let parent = self.status(reply_to)?;
            let parent_id = parent
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| {
                    NetError::Other(
                        "Mastodon returned a parent without an id. the reply was not sent".into(),
                    )
                })?;
            body["in_reply_to_id"] = json!(parent_id);
            let visibility = parent.get("visibility").and_then(Value::as_str);
            if let Some(visibility) =
                visibility.filter(|v| ["unlisted", "private", "direct"].contains(v))
            {
                body["visibility"] = json!(visibility);
            }
            let acct = parent.pointer("/account/acct").and_then(Value::as_str);
            let own =
                parent.pointer("/account/id").and_then(Value::as_str) == self.user_id.as_deref();
            if let Some(acct) = acct.filter(|_| !own) {
                let mention = format!("@{acct}");
                if !text
                    .to_ascii_lowercase()
                    .contains(&mention.to_ascii_lowercase())
                {
                    text = format!("{mention} {text}");
                }
            }
        }
        if let Some(quote) = draft.quote.as_deref() {
            let quoted = self.status(quote)?;
            if quoted.get("quote_approval").is_none() {
                let link = quoted
                    .get("url")
                    .or_else(|| quoted.get("uri"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                text = format!("{text}\n\n{link}");
            } else if quoted
                .pointer("/quote_approval/current_user")
                .and_then(Value::as_str)
                == Some("denied")
            {
                return Err(NetError::Other(
                    "the author of that post does not allow quotes. share its link instead".into(),
                ));
            } else if let Some(quoted_id) = quoted.get("id") {
                body["quoted_status_id"] = quoted_id.clone();
            }
        }
        let used = text_len(&text);
        if used > max_chars() {
            return Err(NetError::Other(format!(
                "Mastodon allows {} characters on {}; this post has {used}",
                max_chars(),
                self.server
            )));
        }
        body["status"] = json!(text);
        let answer = self.post_to("/api/v1/statuses", Body::Json(&body))?;
        map::post(&answer.value, &self.server).ok_or_else(|| NetError::OutcomeUnknown {
            net: Network::Mastodon,
            message: "Mastodon did not return the new post".into(),
        })
    }

    fn delete(&mut self, id: &str) -> Result<(), NetError> {
        self.session()?;
        let local = self.status_id(id)?;
        self.call(
            Method::DELETE,
            &format!("/api/v1/statuses/{local}"),
            &[],
            Body::None,
        )
        .map(|_| ())
    }

    fn set_like(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.act(id, if on { "favourite" } else { "unfavourite" })
    }

    fn set_repost(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.act(id, if on { "reblog" } else { "unreblog" })
    }

    fn set_bookmark(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.act(id, if on { "bookmark" } else { "unbookmark" })
    }

    fn set_follow(&mut self, handle: &str, on: bool) -> Result<(), NetError> {
        self.session()?;
        let account = self.account_id(handle)?;
        let id = account
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| NetError::Other(format!("Mastodon has no {handle}")))?;
        let verb = if on { "follow" } else { "unfollow" };
        self.post_to(&format!("/api/v1/accounts/{id}/{verb}"), Body::None)
            .map(|_| ())
    }
}

/// The `mastodon` rows of `socli auth status`, secrets redacted.
pub fn status(section: &MastodonConfig) -> Vec<(&'static str, String)> {
    let section = resolved_section(section, |key| std::env::var(key).ok());
    let token = section.access_token.as_deref();
    let server = section
        .server
        .clone()
        .unwrap_or_else(|| DEFAULT_SERVER.into());
    let account = match (token, &section.username) {
        (Some(_), Some(user)) => format!("@{user}"),
        (Some(_), None) => "signed in".into(),
        (None, _) => "not signed in".into(),
    };
    let app = if section.client_id.is_some() && section.client_secret.is_some() {
        "registered"
    } else {
        "none"
    };
    vec![
        ("account", account),
        ("token", config::redact(token)),
        ("server", server),
        ("app", app.into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_servers_never_reuses_the_saved_token_or_identity() {
        let section = MastodonConfig {
            server: Some("a.example".into()),
            access_token: Some("token-a".into()),
            user_id: Some("account-a".into()),
            username: Some("ada@a.example".into()),
            client_id: Some("client-a".into()),
            client_secret: Some("secret-a".into()),
            max_chars: Some(1000),
            ..MastodonConfig::default()
        };
        let changed = resolved_section(&section, |key| {
            (key == "MASTODON_SERVER").then(|| "b.example".into())
        });
        assert_eq!(changed.server.as_deref(), Some("b.example"));
        assert!(changed.access_token.is_none());
        assert!(changed.user_id.is_none() && changed.username.is_none());
        assert!(changed.client_id.is_none() && changed.client_secret.is_none());
        assert!(changed.max_chars.is_none());
        let client = MastodonClient::from_section(&section, |key| {
            (key == "MASTODON_SERVER").then(|| "b.example".into())
        });
        assert!(!client.signed_in());
        assert!(client.account().is_none());
        assert_eq!(client.base(), "https://b.example");
        assert_eq!(section.access_token.as_deref(), Some("token-a"));
    }

    #[test]
    fn an_environment_token_keeps_metadata_only_for_the_same_account_and_server() {
        let section = MastodonConfig {
            server: Some("a.example".into()),
            access_token: Some("token-a".into()),
            user_id: Some("account-a".into()),
            username: Some("ada@a.example".into()),
            ..MastodonConfig::default()
        };
        let changed = MastodonClient::from_section(&section, |key| match key {
            "MASTODON_ACCESS_TOKEN" => Some("token-b".into()),
            _ => None,
        });
        assert_eq!(changed.token.as_deref(), Some("token-b"));
        assert!(changed.signed_in());
        assert!(changed.account().is_none());
        assert!(changed.user_id.is_none());
        let same = MastodonClient::from_section(&section, |key| match key {
            "MASTODON_ACCESS_TOKEN" => Some(" token-a ".into()),
            "MASTODON_SERVER" => Some("https://A.EXAMPLE/about".into()),
            _ => None,
        });
        assert_eq!(same.user_id.as_deref(), Some("account-a"));
        assert_eq!(same.account().unwrap().username, "ada@a.example");
        let changed_both = resolved_section(&section, |key| match key {
            "MASTODON_ACCESS_TOKEN" => Some("token-b".into()),
            "MASTODON_SERVER" => Some("b.example".into()),
            _ => None,
        });
        assert_eq!(changed_both.access_token.as_deref(), Some("token-b"));
        assert!(changed_both.username.is_none());
    }

    #[test]
    fn toggles_require_confirmation_of_the_requested_state() {
        for field in ["favourited", "reblogged", "bookmarked"] {
            assert!(confirm_action(&json!({ field: true }), field, true).is_ok());
            assert!(confirm_action(&json!({ field: false }), field, false).is_ok());
            assert!(matches!(
                confirm_action(&json!({ field: false }), field, true),
                Err(NetError::OutcomeUnknown { .. })
            ));
            assert!(matches!(
                confirm_action(&json!({}), field, false),
                Err(NetError::OutcomeUnknown { .. })
            ));
        }
    }

    #[test]
    fn malformed_or_overflowing_cursors_fail_before_a_request() {
        for cursor in [
            "",
            "offset=",
            "offset=-1",
            "offset=no",
            "max_id=1&local=true",
            "since_id=1",
        ] {
            assert!(page_cursor(Some(cursor)).is_err());
        }
        let overflow = format!("offset={}", usize::MAX);
        assert!(search_offset(Some(&overflow), 10).is_err());
        let too_large = format!("offset={}0", usize::MAX);
        assert!(search_offset(Some(&too_large), 10).is_err());
        assert_eq!(search_offset(Some("offset=40"), 20).unwrap(), 40);
        assert_eq!(search_offset(None, 20).unwrap(), 0);
        assert!(search_offset(Some("max_id=1"), 20).is_err());
    }

    #[test]
    fn hashtag_text_cannot_change_the_request_path_or_query() {
        let path = tag_path("rust/other?local=true#fragment");
        let url = Url::parse(&format!("https://mastodon.social{path}")).unwrap();
        assert_eq!(
            url.path(),
            "/api/v1/timelines/tag/rust%2Fother%3Flocal%3Dtrue%23fragment"
        );
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
    }

    #[test]
    fn length_counts_links_as_23_and_mentions_without_their_server() {
        assert_eq!(text_len("hello"), 5);
        assert_eq!(
            text_len("see https://example.org/a/very/long/path/that/goes/on"),
            4 + 23
        );
        assert_eq!(text_len("@theo@fosstodon.org hi"), 5 + 3);
        assert_eq!(text_len("👩‍👩‍👧 ok"), 4);
    }

    #[test]
    fn profile_links_go_to_the_handle_server() {
        assert_eq!(
            profile_url("@theo@fosstodon.org"),
            "https://fosstodon.org/@theo"
        );
        assert_eq!(profile_url("ines"), "https://mastodon.social/@ines");
    }

    #[test]
    fn link_headers_become_cursors() {
        let statuses = r#"<https://mastodon.social/api/v1/accounts/1/statuses?limit=2&max_id=117473973972580682>; rel="next", <https://mastodon.social/api/v1/accounts/1/statuses?limit=2&min_id=117474031242292744>; rel="prev""#;
        assert_eq!(
            next_cursor(Some(statuses)).as_deref(),
            Some("max_id=117473973972580682")
        );
        let trends =
            r#"<https://mastodon.social/api/v1/trends/statuses?limit=2&offset=2>; rel="next""#;
        assert_eq!(next_cursor(Some(trends)).as_deref(), Some("offset=2"));
        assert_eq!(
            next_cursor(Some(r#"<https://a.example/x?min_id=1>; rel="prev""#)),
            None
        );
        assert_eq!(next_cursor(None), None);
        assert_eq!(
            cursor_pair(Some("offset=40")),
            Some(("offset", "40".to_string()))
        );
        assert_eq!(
            cursor_pair(Some("1179")),
            Some(("max_id", "1179".to_string()))
        );
        assert_eq!(cursor_pair(Some("since_id=1")), None);
    }

    #[test]
    fn rate_limits_wait_until_the_reset_time() {
        let now = DateTime::parse_from_rfc3339("2026-10-20T19:40:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            reset_wait(Some("2026-10-20T19:45:00.816578Z"), now),
            Some(300)
        );
        assert_eq!(reset_wait(Some("2026-10-20T19:39:00Z"), now), Some(1));
        assert_eq!(reset_wait(Some("soon"), now), None);
    }

    #[test]
    fn caps_follow_the_session() {
        let signed_in = caps_with(true, 500);
        assert_eq!(signed_in.feeds.len(), 3);
        assert_eq!(signed_in.feed_label(FeedKind::Following), Some("Home"));
        assert_eq!(signed_in.like_verb, "favorite");
        assert_eq!(signed_in.liked_word(true), "favorited");
        assert!(signed_in.quote && signed_in.bookmark && signed_in.unrepost && !signed_in.views);
        let public = caps_with(false, 11000);
        assert!(public.feeds.is_empty() && public.thread && public.user && !public.post);
        assert_eq!(public.max_chars, 11000);
        assert_eq!(
            signed_in.resolve_feed(Network::Mastodon, FeedKind::Discover),
            Some(FeedKind::Trending)
        );
    }

    #[test]
    fn a_public_client_asks_for_sign_in() {
        let mut client = MastodonClient::public();
        assert!(!client.signed_in());
        assert!(matches!(
            client.feed(FeedKind::Following, None, 20),
            Err(NetError::SignInRequired(Network::Mastodon))
        ));
        assert!(matches!(
            client.status_id("mastodon:https://fosstodon.org/@theo/115000000000000010"),
            Err(NetError::Other(message)) if message == "sign in to open posts from fosstodon.org: `socli login mastodon`"
        ));
        assert_eq!(
            client
                .status_id("mastodon:https://mastodon.social/@ines/115000000000000001")
                .unwrap(),
            "115000000000000001"
        );
        assert_eq!(
            client.status_id("115000000000000001").unwrap(),
            "115000000000000001"
        );
    }

    #[test]
    fn ids_from_another_network_are_refused() {
        let mut client = MastodonClient::public();
        assert!(matches!(
            client.status_id("x:1846"),
            Err(NetError::Other(message)) if message == "x:1846 belongs to X, not Mastodon"
        ));
    }

    #[test]
    fn the_section_and_the_environment_pick_the_server_and_token() {
        let section = MastodonConfig {
            server: Some("fosstodon.org".into()),
            access_token: Some("tok".into()),
            username: Some("theo@fosstodon.org".into()),
            user_id: Some("109002".into()),
            ..MastodonConfig::default()
        };
        let client = MastodonClient::from_section(&section, |_| None);
        assert!(client.signed_in());
        assert_eq!(client.base(), "https://fosstodon.org");
        assert_eq!(client.account().unwrap().username, "theo@fosstodon.org");
        let env = MastodonClient::from_section(&MastodonConfig::default(), |key| {
            (key == "MASTODON_ACCESS_TOKEN").then(|| "env".to_string())
        });
        assert!(env.signed_in());
        assert_eq!(env.base(), "https://mastodon.social");
    }
}
