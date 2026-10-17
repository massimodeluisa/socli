/*!
Official X API v2. Reads are billed per resource returned; this client asks
for small pages and never auto-paginates. Ids here are X's own; the provider
in `mod.rs` scopes them.
*/

use std::collections::HashMap;
use std::sync::Mutex;

use crate::net::http::{header, Client, RequestBuilder, StatusCode};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use thiserror::Error;

use super::auth::Credentials;
use crate::model::{Link, Media, MediaKind, Metrics, Page, Post, User};
use crate::net::Network;

const TWEET_FIELDS: &str = "created_at,public_metrics,entities,conversation_id,referenced_tweets,attachments,author_id,note_tweet,lang";
const EXPANSIONS: &str =
    "author_id,attachments.media_keys,referenced_tweets.id,referenced_tweets.id.author_id";
const MEDIA_FIELDS: &str = "url,preview_image_url,type,width,height,alt_text";
const USER_FIELDS: &str = "name,username,profile_image_url,verified,verified_type,description";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{detail}{hint}")]
    Failed {
        status: u16,
        detail: String,
        hint: String,
    },
    #[error("X rate limit reached")]
    RateLimited { retry_after: Option<u64> },
    #[error("{0}")]
    Other(String),
}

impl ApiError {
    pub(super) fn from_response(status: StatusCode, body: &str) -> Self {
        Self::http(status, body, Call::OtherWrite)
    }

    fn http(status: StatusCode, body: &str, call: Call) -> Self {
        let detail = detail_from(body).unwrap_or_else(|| {
            if body.trim().is_empty() {
                format!("X API returned {}", status.as_u16())
            } else {
                format!(
                    "X API returned {}: {}",
                    status.as_u16(),
                    truncate(body, 280)
                )
            }
        });
        let hint = hint_for(status, call, &detail);
        Self::Failed {
            status: status.as_u16(),
            detail,
            hint,
        }
    }
}

#[derive(Clone, Copy)]
enum Call {
    Read,
    Post,
    Reply,
    Like,
    Follow,
    Repost,
    Bookmark,
    OtherWrite,
}

pub struct XClient {
    http: Client,
    creds: Credentials,
    ops: Mutex<super::web::OpCache>,
    /// Home-page material for `x-client-transaction-id`. Filled on the first write that needs it.
    tid: Mutex<Option<super::transaction::Transaction>>,
    /// `None` is `https://x.com`. Tests point this at a local stub.
    web_origin: Option<String>,
    /// Screen name read from the signed-in home page. The stored config username is not this.
    viewer: Mutex<Option<String>>,
}

impl XClient {
    pub fn new(creds: Credentials) -> Result<Self, ApiError> {
        let http = crate::net::http::client().map_err(|e| ApiError::Other(e.to_string()))?;
        Ok(Self::from_http(creds, http, None))
    }

    #[cfg(test)]
    pub(crate) fn for_test(creds: Credentials, origin: String) -> Result<Self, ApiError> {
        let http = crate::net::http::client().map_err(|e| ApiError::Other(e.to_string()))?;
        Ok(Self::from_http(creds, http, Some(origin)))
    }

    fn from_http(creds: Credentials, http: Client, web_origin: Option<String>) -> Self {
        Self {
            http,
            creds,
            ops: Mutex::new(super::web::OpCache::seed()),
            tid: Mutex::new(None),
            web_origin,
            viewer: Mutex::new(None),
        }
    }

    pub(super) fn web_origin(&self) -> &str {
        self.web_origin.as_deref().unwrap_or("https://x.com")
    }

    pub(super) fn viewer_mut(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.viewer.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub(super) fn http(&self) -> &Client {
        &self.http
    }

    pub(super) fn ops(&self) -> &Mutex<super::web::OpCache> {
        &self.ops
    }

    pub(super) fn tid(&self) -> &Mutex<Option<super::transaction::Transaction>> {
        &self.tid
    }

    pub(super) fn clear_tid(&self) {
        let mut guard = self.tid().lock().unwrap_or_else(|err| err.into_inner());
        *guard = None;
    }

    pub fn credentials(&self) -> &Credentials {
        &self.creds
    }

    pub fn credentials_mut(&mut self) -> &mut Credentials {
        &mut self.creds
    }

    pub fn me(&self) -> Result<User, ApiError> {
        if self.creds.uses_web() {
            return self.web_me();
        }
        let value = self.get_user("/2/users/me", &[("user.fields", USER_FIELDS)])?;
        let user = parse_user(value.get("data").unwrap_or(&Value::Null))
            .ok_or_else(|| ApiError::Other("X did not return a user".into()))?;
        Ok(user)
    }

    pub fn user_by_username(&self, username: &str) -> Result<User, ApiError> {
        if self.creds.uses_web() {
            return self.web_user(username);
        }
        let path = format!("/2/users/by/username/{username}");
        let value = self.get_user(&path, &[("user.fields", USER_FIELDS)])?;
        parse_user(value.get("data").unwrap_or(&Value::Null))
            .ok_or_else(|| ApiError::Other(format!("no public user @{username}")))
    }

    pub fn home(&mut self, page: Option<&str>, count: u8) -> Result<Page, ApiError> {
        if self.creds.uses_web() {
            return self.web_following(page, count);
        }
        let id = self.user_id()?;
        let count = count.clamp(1, 100);
        let mut pairs = vec![
            ("max_results", count.to_string()),
            ("tweet.fields", TWEET_FIELDS.into()),
            ("expansions", EXPANSIONS.into()),
            ("media.fields", MEDIA_FIELDS.into()),
            ("user.fields", USER_FIELDS.into()),
        ];
        if let Some(token) = page {
            pairs.push(("pagination_token", token.to_string()));
        }
        let path = format!("/2/users/{id}/timelines/reverse_chronological");
        let refs: Vec<(&str, String)> = pairs.iter().map(|(k, v)| (*k, v.clone())).collect();
        let query: Vec<(&str, &str)> = refs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.get(&path, &query, Call::Read).map(parse_page)
    }

    pub fn user_tweets(&self, id: &str, page: Option<&str>, count: u8) -> Result<Page, ApiError> {
        if self.creds.uses_web() {
            return self.web_user_tweets(id, page, count);
        }
        let count = count.clamp(5, 100);
        self.timeline(
            &format!("/2/users/{id}/tweets"),
            page,
            count,
            "pagination_token",
        )
    }

    pub fn bookmarks(&mut self, page: Option<&str>, count: u8) -> Result<Page, ApiError> {
        if self.creds.uses_web() {
            return self.web_bookmarks(page, count);
        }
        let id = self.user_id()?;
        let count = count.clamp(1, 100);
        self.timeline(
            &format!("/2/users/{id}/bookmarks"),
            page,
            count,
            "pagination_token",
        )
    }

    pub fn mentions(&mut self, page: Option<&str>, count: u8) -> Result<Page, ApiError> {
        if self.creds.uses_web() {
            let username = if let Some(name) = self.creds.username.clone().filter(|s| !s.is_empty())
            {
                name
            } else {
                self.web_me()?.username
            };
            return self.web_search(&format!("to:{username}"), page, count);
        }
        let id = self.user_id()?;
        let count = count.clamp(5, 100);
        self.timeline(
            &format!("/2/users/{id}/mentions"),
            page,
            count,
            "pagination_token",
        )
    }

    pub fn search(&self, query: &str, page: Option<&str>, count: u8) -> Result<Page, ApiError> {
        if self.creds.uses_web() {
            return self.web_search(query, page, count);
        }
        let count = count.clamp(10, 100);
        let mut pairs = vec![
            ("query", query.to_string()),
            ("max_results", count.to_string()),
            ("sort_order", "recency".into()),
            ("tweet.fields", TWEET_FIELDS.into()),
            ("expansions", EXPANSIONS.into()),
            ("media.fields", MEDIA_FIELDS.into()),
            ("user.fields", USER_FIELDS.into()),
        ];
        if let Some(token) = page {
            pairs.push(("next_token", token.to_string()));
        }
        self.get_owned("/2/tweets/search/recent", &pairs, Call::Read)
            .map(parse_page)
    }

    pub fn conversation(&self, id: &str, count: u8) -> Result<Page, ApiError> {
        if self.creds.uses_web() {
            return self.web_thread(id, count);
        }
        let root = self.tweet(id);
        let replies = self.search(&format!("conversation_id:{id}"), None, count.max(10));
        match (root, replies) {
            (Ok(post), Ok(mut page)) => {
                if !page.ids.contains(&post.id) {
                    page.ids.insert(0, post.id.clone());
                }
                if !page.posts.iter().any(|p| p.id == post.id) {
                    page.posts.insert(0, post);
                }
                Ok(page)
            }
            (Ok(post), Err(_)) => Ok(Page {
                ids: vec![post.id.clone()],
                posts: vec![post],
                next: None,
            }),
            (Err(err), Ok(page)) => {
                if page.ids.is_empty() {
                    Err(err)
                } else {
                    Ok(page)
                }
            }
            (Err(err), Err(_)) => Err(err),
        }
    }

    pub fn tweet(&self, id: &str) -> Result<Post, ApiError> {
        let pairs = [
            ("tweet.fields", TWEET_FIELDS),
            ("expansions", EXPANSIONS),
            ("media.fields", MEDIA_FIELDS),
            ("user.fields", USER_FIELDS),
        ];
        let value = self.get(&format!("/2/tweets/{id}"), &pairs, Call::Read)?;
        let page = parse_page(value);
        page.posts
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| ApiError::Other(format!("post {id} was not in the response")))
    }

    /// Quotes post as text with the quoted link: `quote_tweet_id` needs an Enterprise plan.
    pub fn create_post(&self, text: &str, reply_to: Option<&str>) -> Result<String, ApiError> {
        let call = if reply_to.is_some() {
            Call::Reply
        } else {
            Call::Post
        };
        let value = self.post("/2/tweets", post_body(text, reply_to), call)?;
        value
            .pointer("/data/id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| ApiError::Other("X accepted the post but returned no id".into()))
    }

    pub fn delete_post(&self, id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_delete(id);
        }
        self.delete(&format!("/2/tweets/{id}"), Call::OtherWrite)?;
        Ok(())
    }

    pub fn repost(&mut self, tweet_id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_repost(tweet_id, true);
        }
        let id = self.user_id()?;
        self.post(
            &format!("/2/users/{id}/retweets"),
            json!({ "tweet_id": tweet_id }),
            Call::Repost,
        )?;
        Ok(())
    }

    pub fn unrepost(&mut self, tweet_id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_repost(tweet_id, false);
        }
        let id = self.user_id()?;
        self.delete(&format!("/2/users/{id}/retweets/{tweet_id}"), Call::Repost)?;
        Ok(())
    }

    pub fn bookmark(&mut self, tweet_id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_bookmark(tweet_id, true);
        }
        let id = self.user_id()?;
        self.post(
            &format!("/2/users/{id}/bookmarks"),
            json!({ "tweet_id": tweet_id }),
            Call::Bookmark,
        )?;
        Ok(())
    }

    pub fn unbookmark(&mut self, tweet_id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_bookmark(tweet_id, false);
        }
        let id = self.user_id()?;
        self.delete(
            &format!("/2/users/{id}/bookmarks/{tweet_id}"),
            Call::Bookmark,
        )?;
        Ok(())
    }

    pub fn like(&mut self, tweet_id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_like(tweet_id, true);
        }
        let id = self.user_id()?;
        self.post(
            &format!("/2/users/{id}/likes"),
            json!({ "tweet_id": tweet_id }),
            Call::Like,
        )?;
        Ok(())
    }

    pub fn unlike(&mut self, tweet_id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_like(tweet_id, false);
        }
        let id = self.user_id()?;
        self.delete(&format!("/2/users/{id}/likes/{tweet_id}"), Call::Like)?;
        Ok(())
    }

    pub fn follow(&mut self, target_user_id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_follow(target_user_id, true);
        }
        let id = self.user_id()?;
        self.post(
            &format!("/2/users/{id}/following"),
            json!({ "target_user_id": target_user_id }),
            Call::Follow,
        )?;
        Ok(())
    }

    pub fn unfollow(&mut self, target_user_id: &str) -> Result<(), ApiError> {
        if self.creds.uses_web() {
            return self.web_follow(target_user_id, false);
        }
        let id = self.user_id()?;
        self.delete(
            &format!("/2/users/{id}/following/{target_user_id}"),
            Call::Follow,
        )?;
        Ok(())
    }

    fn user_id(&mut self) -> Result<String, ApiError> {
        if let Some(id) = self.creds.user_id.clone() {
            return Ok(id);
        }
        if !self.creds.has_user() {
            return Err(ApiError::Other("sign in first: `socli login x`".into()));
        }
        let me = self.me()?;
        self.creds.user_id = Some(me.id.clone());
        self.creds.username = Some(me.username.clone());
        Ok(me.id)
    }

    fn timeline(
        &self,
        path: &str,
        page: Option<&str>,
        count: u8,
        page_param: &str,
    ) -> Result<Page, ApiError> {
        let mut pairs = vec![
            ("max_results", count.to_string()),
            ("tweet.fields", TWEET_FIELDS.into()),
            ("expansions", EXPANSIONS.into()),
            ("media.fields", MEDIA_FIELDS.into()),
            ("user.fields", USER_FIELDS.into()),
        ];
        if let Some(token) = page {
            pairs.push((page_param, token.to_string()));
        }
        self.get_owned(path, &pairs, Call::Read).map(parse_page)
    }

    fn get(&self, path: &str, query: &[(&str, &str)], call: Call) -> Result<Value, ApiError> {
        self.send(
            self.http
                .get(format!("https://api.x.com{path}"))
                .query(query),
            call,
        )
    }

    fn get_user(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, ApiError> {
        self.get(path, query, Call::Read)
    }

    fn get_owned(
        &self,
        path: &str,
        pairs: &[(&str, String)],
        call: Call,
    ) -> Result<Value, ApiError> {
        let query: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.get(path, &query, call)
    }

    fn post(&self, path: &str, body: Value, call: Call) -> Result<Value, ApiError> {
        self.send(
            self.http
                .post(format!("https://api.x.com{path}"))
                .json(&body),
            call,
        )
    }

    fn delete(&self, path: &str, call: Call) -> Result<Value, ApiError> {
        self.send(self.http.delete(format!("https://api.x.com{path}")), call)
    }

    fn send(&self, req: RequestBuilder, call: Call) -> Result<Value, ApiError> {
        let token = self.token_for(call)?;
        let response = req
            .bearer_auth(token)
            .send()
            .map_err(|e| ApiError::Other(format!("network: {e}")))?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(ApiError::RateLimited {
                retry_after: retry_after(response.headers(), unix_now()),
            });
        }
        let text = response
            .text()
            .map_err(|e| ApiError::Other(format!("reading the X response: {e}")))?;
        if !status.is_success() {
            return Err(ApiError::http(status, &text, call));
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|e| ApiError::Other(format!("X returned non-JSON: {e}")))
    }

    fn token_for(&self, call: Call) -> Result<&str, ApiError> {
        let user_only = matches!(
            call,
            Call::Post
                | Call::Reply
                | Call::Like
                | Call::Follow
                | Call::Repost
                | Call::Bookmark
                | Call::OtherWrite
        );
        if let Some(token) = self.creds.access_token.as_deref() {
            return Ok(token);
        }
        if user_only {
            return Err(ApiError::Other("sign in first: `socli login x`".into()));
        }
        self.creds.bearer_token.as_deref().ok_or_else(|| {
            ApiError::Other(
                "sign in first: `socli login x`, or `socli auth x bearer <token>` for public reads"
                    .into(),
            )
        })
    }
}

/** Seconds until X's window resets: `x-rate-limit-reset` is a Unix time, and `retry-after` is the fallback. */
fn retry_after(headers: &header::HeaderMap, now: u64) -> Option<u64> {
    let number = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    number("x-rate-limit-reset")
        .map(|reset| reset.saturating_sub(now).max(1))
        .or_else(|| number("retry-after"))
}

fn unix_now() -> u64 {
    crate::clock::unix_secs()
}

/// The `POST /2/tweets` body: the text, and the parent when it is a reply.
fn post_body(text: &str, reply_to: Option<&str>) -> Value {
    let mut body = json!({ "text": text });
    if let Some(id) = reply_to {
        body["reply"] = json!({ "in_reply_to_tweet_id": id });
    }
    body
}

fn hint_for(status: StatusCode, call: Call, detail: &str) -> String {
    let policy = match call {
        Call::Like | Call::Follow => Some(
            "Like and follow writes were pulled from self-serve tiers (announced for 20 Apr 2026) and need Enterprise. Repost, bookmark, post, and reply are still on pay-per-use.",
        ),
        Call::Reply if status.as_u16() == 403 => Some(
            "Self-serve replies are often rejected unless that author has @mentioned or quoted you.",
        ),
        _ => None,
    };
    let generic = match status.as_u16() {
        401 => Some("The token was rejected. Run `socli login x`, or check X_ACCESS_TOKEN / X_BEARER_TOKEN."),
        402 => Some("The developer account is out of credits. Reads are billed per post returned."),
        403 if policy.is_none() => Some("X refused this call. Check the app's scopes and that the token belongs to this user."),
        _ => None,
    };
    let mut hint = String::new();
    if let Some(line) = policy.or(generic) {
        if !detail.contains(line) {
            hint.push('\n');
            hint.push_str(line);
        }
    }
    hint
}

fn detail_from(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    if let Some(errors) = value.get("errors").and_then(|v| v.as_array()) {
        let parts: Vec<&str> = errors
            .iter()
            .filter_map(|e| {
                e.get("message")
                    .or_else(|| e.get("detail"))
                    .and_then(|v| v.as_str())
            })
            .collect();
        if !parts.is_empty() {
            return Some(parts.join("; "));
        }
    }
    value
        .get("detail")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            value
                .get("title")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
}

fn truncate(text: &str, max: usize) -> String {
    let mut s: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        s.push('…');
    }
    s
}

pub fn parse_page(value: Value) -> Page {
    let includes = value.get("includes").cloned().unwrap_or(Value::Null);
    let users = index_users(includes.get("users"));
    let media = index_media(includes.get("media"));
    let extras = tweets_of(includes.get("tweets"), &users, &media);
    let primary_raw = match value.get("data") {
        Some(Value::Array(items)) => items.clone(),
        Some(other) if other.is_object() => vec![other.clone()],
        _ => Vec::new(),
    };
    let mut posts_by_id: HashMap<String, Post> = HashMap::new();
    for post in extras {
        posts_by_id.insert(post.id.clone(), post);
    }
    let mut ids = Vec::new();
    for raw in &primary_raw {
        if let Some(post) = hydrate_tweet(raw, &users, &media, &posts_by_id) {
            ids.push(post.id.clone());
            posts_by_id.insert(post.id.clone(), post);
        }
    }
    // Second pass so quotes and reposts can see siblings parsed in this page.
    let snapshot = posts_by_id.clone();
    for raw in &primary_raw {
        if let Some(post) = hydrate_tweet(raw, &users, &media, &snapshot) {
            posts_by_id.insert(post.id.clone(), post);
        }
    }
    let next = value
        .pointer("/meta/next_token")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Page {
        ids,
        posts: posts_by_id.into_values().collect(),
        next,
    }
}

fn tweets_of(
    value: Option<&Value>,
    users: &HashMap<String, User>,
    media: &HashMap<String, Media>,
) -> Vec<Post> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|raw| hydrate_tweet(raw, users, media, &HashMap::new()))
        .collect()
}

fn hydrate_tweet(
    raw: &Value,
    users: &HashMap<String, User>,
    media_index: &HashMap<String, Media>,
    known: &HashMap<String, Post>,
) -> Option<Post> {
    let id = raw.get("id")?.as_str()?.to_string();
    let author_id = raw.get("author_id").and_then(|v| v.as_str()).unwrap_or("");
    let author = users.get(author_id).cloned().unwrap_or_else(|| User {
        id: author_id.to_string(),
        name: "unknown".into(),
        username: author_id.to_string(),
        avatar_url: None,
        verified: false,
        business: false,
        description: None,
    });
    let links = links_of(raw.get("entities"));
    let mut text = raw
        .get("note_tweet")
        .and_then(|n| n.get("text"))
        .and_then(|t| t.as_str())
        .or_else(|| raw.get("text").and_then(|t| t.as_str()))
        .unwrap_or("")
        .to_string();
    for link in &links {
        if !link.short.is_empty() {
            text = text.replace(&link.short, &link.expanded);
        }
    }
    let created_at = raw
        .get("created_at")
        .and_then(|v| v.as_str())
        .and_then(parse_time);
    let conversation_id = raw
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_string();
    let refs = raw
        .get("referenced_tweets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut in_reply_to = None;
    let mut quoted = None;
    let mut reposted_id = None;
    for reference in &refs {
        let kind = reference.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let rid = reference.get("id").and_then(|v| v.as_str()).unwrap_or("");
        match kind {
            "replied_to" => in_reply_to = Some(rid.to_string()),
            "quoted" => quoted = known.get(rid).cloned(),
            "retweeted" => reposted_id = Some(rid.to_string()),
            _ => {}
        }
    }
    let metrics = metrics_of(raw.get("public_metrics"));
    let media = raw
        .pointer("/attachments/media_keys")
        .and_then(|v| v.as_array())
        .map(|keys| {
            keys.iter()
                .filter_map(|k| k.as_str())
                .filter_map(|k| media_index.get(k).cloned())
                .collect()
        })
        .unwrap_or_default();
    let url = Some(format!(
        "https://x.com/{}/status/{id}",
        if author.username.is_empty() {
            "i"
        } else {
            &author.username
        }
    ));
    let mut post = Post {
        id: id.clone(),
        net: Network::X,
        url,
        cid: None,
        author,
        text,
        created_at,
        conversation_id,
        in_reply_to,
        quoted: quoted.map(Box::new),
        reposted_by: None,
        metrics,
        links,
        media,
        liked: false,
        bookmarked: false,
        reposted: false,
    };
    if let Some(original_id) = reposted_id {
        if let Some(original) = known.get(&original_id) {
            let reposter = post.author.username.clone();
            post = original.clone();
            post.reposted_by = Some(reposter);
            // Keep the original id. Likes, unreposts, and replies all target it.
        }
    }
    Some(post)
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn metrics_of(value: Option<&Value>) -> Metrics {
    let Some(value) = value else {
        return Metrics::default();
    };
    Metrics {
        replies: num(value, "reply_count"),
        reposts: num(value, "retweet_count"),
        likes: num(value, "like_count"),
        quotes: num(value, "quote_count"),
        bookmarks: num(value, "bookmark_count"),
        views: num(value, "impression_count"),
    }
}

fn num(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

fn links_of(entities: Option<&Value>) -> Vec<Link> {
    let Some(urls) = entities
        .and_then(|e| e.get("urls"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    urls.iter()
        .filter_map(|url| {
            let short = url.get("url")?.as_str()?.to_string();
            let expanded = url
                .get("expanded_url")
                .and_then(|v| v.as_str())
                .unwrap_or(&short)
                .to_string();
            let display = url
                .get("display_url")
                .and_then(|v| v.as_str())
                .unwrap_or(&expanded)
                .to_string();
            let text = |key: &str| url.get(key).and_then(|v| v.as_str()).map(str::to_string);
            let (image, image_width, image_height) = match link_image(url) {
                Some((image, width, height)) => (Some(image), width, height),
                None => (None, None, None),
            };
            Some(Link {
                short,
                expanded,
                display,
                title: text("title"),
                description: text("description"),
                image,
                image_width,
                image_height,
                site: None,
            })
        })
        .collect()
}

/// The widest http(s) preview image of one `entities.urls` entry, or the first when none has a width.
fn link_image(url: &Value) -> Option<(String, Option<u32>, Option<u32>)> {
    let size = |image: &Value, key: &str| {
        image
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    url.get("images")?
        .as_array()?
        .iter()
        .filter_map(|image| {
            let src = image.get("url")?.as_str()?;
            let scheme = url::Url::parse(src).ok()?.scheme().to_string();
            matches!(scheme.as_str(), "http" | "https")
                .then(|| (src.to_string(), size(image, "width"), size(image, "height")))
        })
        .reduce(|widest, next| if next.1 > widest.1 { next } else { widest })
}

fn index_users(value: Option<&Value>) -> HashMap<String, User> {
    let mut out = HashMap::new();
    let Some(Value::Array(items)) = value else {
        return out;
    };
    for item in items {
        if let Some(user) = parse_user(item) {
            out.insert(user.id.clone(), user);
        }
    }
    out
}

fn parse_user(value: &Value) -> Option<User> {
    let id = value.get("id")?.as_str()?.to_string();
    Some(User {
        id,
        name: value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into(),
        username: value
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .into(),
        avatar_url: value
            .get("profile_image_url")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        verified: value
            .get("verified")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        business: value.get("verified_type").and_then(|v| v.as_str()) == Some("business"),
        description: value
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

fn index_media(value: Option<&Value>) -> HashMap<String, Media> {
    let mut out = HashMap::new();
    let Some(Value::Array(items)) = value else {
        return out;
    };
    for item in items {
        let key = item
            .get("media_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if key.is_empty() {
            continue;
        }
        let kind = match item.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "photo" => MediaKind::Photo,
            "video" => MediaKind::Video,
            "animated_gif" => MediaKind::Gif,
            _ => MediaKind::Other,
        };
        out.insert(
            key.clone(),
            Media {
                key,
                kind,
                url: item.get("url").and_then(|v| v.as_str()).map(str::to_string),
                preview_url: item
                    .get("preview_image_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                alt: item
                    .get("alt_text")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                width: item.get("width").and_then(|v| v.as_u64()).map(|n| n as u32),
                height: item
                    .get("height")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32),
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn business_follows_verified_type() {
        let user = |verified_type: &str| {
            parse_user(&serde_json::json!({
                "id": "3",
                "name": "Tern Labs",
                "username": "ternlabs",
                "verified": true,
                "verified_type": verified_type,
            }))
            .map(|user| user.business)
        };
        assert_eq!(user("business"), Some(true));
        assert_eq!(user("blue"), Some(false));
    }

    #[test]
    fn rate_limits_wait_for_the_reset_time() {
        let mut headers = HeaderMap::new();
        headers.insert("x-rate-limit-reset", HeaderValue::from_static("1000"));
        assert_eq!(retry_after(&headers, 940), Some(60));
        assert_eq!(retry_after(&headers, 1200), Some(1));
        let mut fallback = HeaderMap::new();
        fallback.insert("retry-after", HeaderValue::from_static("30"));
        assert_eq!(retry_after(&fallback, 940), Some(30));
        assert_eq!(retry_after(&HeaderMap::new(), 940), None);
    }

    #[test]
    fn link_images_pick_the_widest_web_image() {
        let entry = json!({ "images": [
            { "url": "https://pbs.twimg.com/news_img/1?name=150x150", "width": 150, "height": 150 },
            { "url": "https://pbs.twimg.com/news_img/1?name=orig", "width": 1200, "height": 630 }
        ]});
        assert_eq!(
            link_image(&entry),
            Some((
                "https://pbs.twimg.com/news_img/1?name=orig".into(),
                Some(1200),
                Some(630)
            ))
        );
        let no_widths = json!({ "images": [
            { "url": "https://example.com/a.png" },
            { "url": "https://example.com/b.png" }
        ]});
        assert_eq!(
            link_image(&no_widths),
            Some(("https://example.com/a.png".into(), None, None))
        );
        let local = json!({ "images": [
            { "url": "file:///etc/passwd", "width": 2000, "height": 1000 },
            { "url": "http://example.com/c.png", "width": 400, "height": 200 }
        ]});
        assert_eq!(
            link_image(&local),
            Some(("http://example.com/c.png".into(), Some(400), Some(200)))
        );
        assert_eq!(
            link_image(&json!({ "images": [{ "url": "data:image/png;base64,AA" }] })),
            None
        );
        assert_eq!(link_image(&json!({ "images": [] })), None);
        assert_eq!(link_image(&json!({ "url": "https://t.co/abc" })), None);
    }

    #[test]
    fn x_links_carry_the_widest_preview_image() {
        let entities = json!({"urls": [{
            "url": "https://t.co/a",
            "expanded_url": "https://example.com/a",
            "display_url": "example.com/a",
            "images": [
                {"url": "https://pbs.twimg.com/small.jpg", "width": 150, "height": 150},
                {"url": "https://pbs.twimg.com/large.jpg", "width": 1200, "height": 630}
            ]
        }]});
        let links = links_of(Some(&entities));
        assert_eq!(
            links[0].image.as_deref(),
            Some("https://pbs.twimg.com/large.jpg")
        );
        assert_eq!(links[0].image_width, Some(1200));
        assert_eq!(links[0].image_height, Some(630));
    }

    #[test]
    fn post_bodies_never_carry_a_quote_id() {
        assert_eq!(post_body("hi", None), json!({ "text": "hi" }));
        assert_eq!(
            post_body("hi", Some("100")),
            json!({ "text": "hi", "reply": { "in_reply_to_tweet_id": "100" } })
        );
        assert!(!post_body("hi", Some("100"))
            .to_string()
            .contains("quote_tweet_id"));
    }

    #[test]
    fn parses_a_thread_with_media_and_a_parent() {
        let raw = serde_json::json!({
            "data": [{
                "id": "200",
                "text": "a reply https://t.co/abc",
                "author_id": "2",
                "created_at": "2026-10-20T11:00:00.000Z",
                "conversation_id": "100",
                "public_metrics": {"reply_count": 0, "retweet_count": 1, "like_count": 4, "quote_count": 0, "bookmark_count": 2, "impression_count": 310},
                "referenced_tweets": [{"type": "replied_to", "id": "100"}],
                "entities": {"urls": [{"url": "https://t.co/abc", "expanded_url": "https://example.com/x", "display_url": "example.com/x", "title": "Example", "description": "An example page"}]},
                "attachments": {"media_keys": ["3_1"]}
            }],
            "includes": {
                "users": [
                    {"id": "2", "name": "Mina Cho", "username": "mina", "verified": false},
                    {"id": "1", "name": "Nuno Park", "username": "nuno", "verified": true}
                ],
                "tweets": [{
                    "id": "100",
                    "text": "the root",
                    "author_id": "1",
                    "created_at": "2026-10-20T10:00:00.000Z",
                    "conversation_id": "100"
                }],
                "media": [{
                    "media_key": "3_1",
                    "type": "photo",
                    "url": "https://pbs.twimg.com/media/demo.jpg",
                    "width": 100,
                    "height": 80,
                    "alt_text": "a chart"
                }]
            },
            "meta": {"next_token": "NEXT", "result_count": 1}
        });
        let page = parse_page(raw);
        assert_eq!(page.ids, vec!["200".to_string()]);
        assert_eq!(page.next.as_deref(), Some("NEXT"));
        let reply = page.posts.iter().find(|p| p.id == "200").unwrap();
        assert_eq!(reply.in_reply_to.as_deref(), Some("100"));
        assert!(reply.text.contains("https://example.com/x"));
        assert_eq!(reply.metrics.likes, 4);
        assert_eq!(reply.metrics.views, 310);
        assert_eq!(reply.links[0].title.as_deref(), Some("Example"));
        assert_eq!(
            reply.links[0].description.as_deref(),
            Some("An example page")
        );
        assert_eq!(reply.media[0].alt.as_deref(), Some("a chart"));
        assert!(page.posts.iter().any(|p| p.id == "100"));
    }

    #[test]
    fn like_forbidden_mentions_the_self_serve_removal() {
        let err = ApiError::http(
            StatusCode::FORBIDDEN,
            r#"{"detail":"You are not permitted to perform this action"}"#,
            Call::Like,
        );
        let text = err.to_string();
        assert!(text.contains("self-serve"));
        assert!(text.contains("not permitted"));
    }
}
