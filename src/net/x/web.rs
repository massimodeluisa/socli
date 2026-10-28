//! The X web session.
//!
//! `socli login x` stores the browser's `auth_token` and `ct0` and calls the same GraphQL the
//! site uses (`https://x.com/i/api/graphql/{queryId}/{Operation}`). Query ids rotate. A fresh
//! copy is read from the web client's `main.*.js` and cached in `x-ops.json`. Ids that the
//! current bundle no longer names, but that still answer, stay as fallbacks: HomeTimeline,
//! HomeLatestTimeline and Bookmarks. Bookmarks needs
//! `graphql_timeline_v2_bookmark_timeline` and returns `bookmark_timeline_v2`. An older id of
//! the same name returns an empty timeline.
//!
//! The bearer below is the public client bearer shipped in x.com. It is not a user secret.
//! The `socli` user agent is rejected, so these calls send a browser user agent. The document
//! request that discovers query ids sends only the cookie and that user agent. CreateBookmark
//! also sends `x-client-transaction-id`, computed from the home page. Without it the edge
//! answers 404.

use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, HashMap};
use std::sync::{MutexGuard, OnceLock};

use crate::net::http::header;
use serde_json::{json, Value};

use super::auth::Credentials;
use super::client::{request_failure, retry_after, ApiError, XClient};
use crate::model::{Link, Media, MediaKind, Metrics, Page, Post, User};
use crate::net::Network;

const WEB_BEARER: &str = "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs=1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";
const CHROME_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const CACHE_TTL_SECS: i64 = 6 * 60 * 60;
/// Bumped when a fallback id changes. Older files stored the previous id beside bundle ids,
/// and loading them would keep the empty Bookmarks query for the whole TTL.
const CACHE_VERSION: u32 = 2;

/// Operations missing from the current bundle whose persisted queries still answer.
const FALLBACKS: &[(&str, &str)] = &[
    ("HomeTimeline", "edseUwk9sP5Phz__9TIRnA"),
    ("HomeLatestTimeline", "iOEZpOdfekFsxSlPQCQtPg"),
    ("Bookmarks", "Z9GWmP0kP2dajyckAaDUBw"),
    ("UserByScreenName", "AMIBMjtxEEATh4z8V9GtRg"),
    ("UserByRestId", "mxaYWwt52eAb6OAzYHMVzg"),
    ("UserTweets", "OdBLACRZ0-9NikpJotaT7g"),
    ("SearchTimeline", "BYppgupFFS2yqVmmRNjlsw"),
    ("TweetDetail", "wjV7NuWEsHgTBFfW3zYKzQ"),
    ("FavoriteTweet", "lI07N6Otwv1PhnEgXILM7A"),
    ("UnfavoriteTweet", "ZYKSe-w7KEslx3JhSIk5LA"),
    ("CreateBookmark", "aoDbu3RHznuiSkQ9aNM67Q"),
    ("DeleteBookmark", "Wlmlj2-xzyS1GN3a6cj-mQ"),
    ("CreateRetweet", "mbRO74GrOvSfRcJnlMapnQ"),
    ("DeleteRetweet", "Zy ZigVsNiFO6v1dEks1eWg"),
    ("CreateTweet", "oTolA2WNu-LVMjbarDB2KA"),
    ("DeleteTweet", "nxpZCY2K-I6QoFHAHeojFQ"),
    ("Following", "9l9tzqLGQU9eutAJffqqMw"),
];

const FEATURES_JSON: &str = r#"{
  "rweb_tipjar_consumption_enabled": true,
  "verified_phone_label_enabled": false,
  "creator_subscriptions_tweet_preview_api_enabled": true,
  "responsive_web_graphql_timeline_navigation_enabled": true,
  "responsive_web_graphql_skip_user_profile_image_extensions_enabled": false,
  "communities_web_enable_tweet_community_results_fetch": true,
  "c9s_tweet_anatomy_moderator_badge_enabled": true,
  "articles_preview_enabled": true,
  "responsive_web_edit_tweet_api_enabled": true,
  "graphql_is_translatable_rweb_tweet_is_translatable_enabled": true,
  "view_counts_everywhere_api_enabled": true,
  "longform_notetweets_consumption_enabled": true,
  "responsive_web_twitter_article_tweet_consumption_enabled": true,
  "creator_subscriptions_quote_tweet_preview_enabled": false,
  "freedom_of_speech_not_reach_fetch_enabled": true,
  "standardized_nudges_misinfo": true,
  "tweet_with_visibility_results_prefer_gql_limited_actions_policy_enabled": true,
  "longform_notetweets_rich_text_read_enabled": true,
  "longform_notetweets_inline_media_enabled": true,
  "responsive_web_enhance_cards_enabled": false,
  "rweb_video_screen_enabled": true,
  "responsive_web_graphql_exclude_directive_enabled": true,
  "rweb_video_timestamps_enabled": true,
  "blue_business_profile_image_shape_enabled": true,
  "responsive_web_twitter_blue_verified_badge_is_enabled": true,
  "interactive_text_enabled": true,
  "longform_notetweets_richtext_consumption_enabled": true,
  "hidden_profile_subscriptions_enabled": true,
  "hidden_profile_likes_enabled": true,
  "subscriptions_verification_info_is_identity_verified_enabled": true,
  "subscriptions_verification_info_verified_since_enabled": true,
  "highlights_tweets_tab_ui_enabled": true,
  "responsive_web_twitter_article_notes_tab_enabled": true,
  "subscriptions_feature_can_gift_premium": true,
  "graphql_timeline_v2_bookmark_timeline": true
}"#;

#[derive(Clone, Copy)]
enum Gql {
    Get,
    /// SearchTimeline answers 404 on GET and 200 on POST.
    Post,
}

pub struct OpCache {
    ids: HashMap<String, String>,
    fetched_at: i64,
    loaded: bool,
}

impl OpCache {
    pub fn seed() -> Self {
        let mut ids = HashMap::new();
        for (name, id) in FALLBACKS {
            ids.insert((*name).to_string(), (*id).to_string());
        }
        Self {
            ids,
            fetched_at: 0,
            loaded: false,
        }
    }

    fn fresh_id(&self, name: &str) -> Option<String> {
        if self.fetched_at == 0 || now_secs().saturating_sub(self.fetched_at) >= CACHE_TTL_SECS {
            return None;
        }
        self.ids.get(name).cloned()
    }

    fn id(&self, name: &str) -> Option<String> {
        self.ids.get(name).cloned()
    }

    fn store(&mut self, parsed: HashMap<String, String>) {
        self.ids = parsed;
        for (name, id) in FALLBACKS {
            self.ids
                .entry((*name).to_string())
                .or_insert_with(|| (*id).to_string());
        }
        self.fetched_at = now_secs();
        self.loaded = true;
        let _ = write_disk(self);
    }

    fn load_disk(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(disk) = read_disk() else {
            return;
        };
        if disk.version != CACHE_VERSION
            || now_secs().saturating_sub(disk.fetched_at) >= CACHE_TTL_SECS
        {
            return;
        }
        self.ids.extend(disk.ops);
        for (name, id) in FALLBACKS {
            self.ids
                .entry((*name).to_string())
                .or_insert_with(|| (*id).to_string());
        }
        self.fetched_at = disk.fetched_at;
    }
}

impl XClient {
    pub(super) fn web_for_you(&self, cursor: Option<&str>, count: u8) -> Result<Page, ApiError> {
        let value = self.gql("HomeTimeline", home_vars(count, cursor), Gql::Get, None)?;
        Ok(parse_timeline(&value))
    }

    pub(super) fn web_following(&self, cursor: Option<&str>, count: u8) -> Result<Page, ApiError> {
        let value = self.gql(
            "HomeLatestTimeline",
            home_vars(count, cursor),
            Gql::Get,
            None,
        )?;
        Ok(parse_timeline(&value))
    }

    pub(super) fn web_bookmarks(&self, cursor: Option<&str>, count: u8) -> Result<Page, ApiError> {
        // This persisted query accepts `count` and `cursor` only. The reaction flags
        // belong to an older id that answers with an empty `bookmark_timeline`.
        let mut variables = json!({ "count": clamp_count(count) });
        if let Some(cursor) = cursor {
            variables["cursor"] = json!(cursor);
        }
        let value = self.gql("Bookmarks", variables, Gql::Get, None)?;
        Ok(parse_timeline(&value))
    }

    pub(super) fn web_search(
        &self,
        query: &str,
        cursor: Option<&str>,
        count: u8,
    ) -> Result<Page, ApiError> {
        let mut variables = json!({
            "rawQuery": query,
            "count": clamp_count(count),
            "querySource": "typed_query",
            "product": "Latest",
        });
        if let Some(cursor) = cursor {
            variables["cursor"] = json!(cursor);
        }
        let value = self.gql("SearchTimeline", variables, Gql::Post, None)?;
        Ok(parse_timeline(&value))
    }

    pub(super) fn web_thread(&self, id: &str, _count: u8) -> Result<Page, ApiError> {
        let variables = json!({
            "focalTweetId": id,
            "with_rux_injections": false,
            "rankingMode": "Relevance",
            "includePromotedContent": false,
            "withCommunity": true,
            "withQuickPromoteEligibilityTweetFields": true,
            "withBirdwatchNotes": true,
            "withVoice": true,
        });
        let value = self.gql("TweetDetail", variables, Gql::Get, None)?;
        Ok(parse_timeline(&value))
    }

    pub(super) fn web_user(&self, username: &str) -> Result<User, ApiError> {
        let variables = json!({
            "screen_name": username.trim_start_matches('@'),
            "withSafetyModeUserFields": true,
        });
        let toggles = json!({ "withAuxiliaryUserLabels": false });
        let value = self.gql("UserByScreenName", variables, Gql::Get, Some(toggles))?;
        user_from_payload(&value)
            .ok_or_else(|| ApiError::Other(format!("no public user @{username}")))
    }

    pub(super) fn web_me(&self) -> Result<User, ApiError> {
        // UserByRestId answers 403 from the edge. The signed-in home page names one account.
        // A username saved from an older login is not that account.
        let username = self.signed_in_handle()?;
        self.web_user(&username)
    }

    fn signed_in_handle(&self) -> Result<String, ApiError> {
        if let Some(name) = self.viewer_mut().clone() {
            return Ok(name);
        }
        let name = viewer_handle(self)?;
        *self.viewer_mut() = Some(name.clone());
        Ok(name)
    }

    pub(super) fn web_user_tweets(
        &self,
        id: &str,
        cursor: Option<&str>,
        count: u8,
    ) -> Result<Page, ApiError> {
        let mut variables = json!({
            "userId": id,
            "count": clamp_count(count),
            "includePromotedContent": false,
            "withQuickPromoteEligibilityTweetFields": true,
            "withVoice": true,
        });
        if let Some(cursor) = cursor {
            variables["cursor"] = json!(cursor);
        }
        let toggles = json!({ "withArticlePlainText": false });
        let value = self.gql("UserTweets", variables, Gql::Get, Some(toggles))?;
        Ok(parse_timeline(&value))
    }

    pub(super) fn web_create(
        &self,
        text: &str,
        reply_to: Option<&str>,
        quote: Option<&str>,
    ) -> Result<String, ApiError> {
        let mut variables = json!({
            "tweet_text": text,
            "dark_request": false,
            "media": { "media_entities": [], "possibly_sensitive": false },
            "semantic_annotation_ids": [],
        });
        if let Some(id) = reply_to {
            variables["reply"] = json!({
                "in_reply_to_tweet_id": id,
                "exclude_reply_user_ids": [],
            });
        }
        if let Some(id) = quote {
            variables["attachment_url"] = json!(format!("https://x.com/i/status/{id}"));
        }
        let value = self.gql("CreateTweet", variables, Gql::Post, None)?;
        value
            .pointer("/data/create_tweet/tweet_results/result/rest_id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| {
                ApiError::OutcomeUnknown("X accepted the post but returned no id".into())
            })
    }

    pub(super) fn web_delete(&self, id: &str) -> Result<(), ApiError> {
        let variables = json!({ "tweet_id": id, "dark_request": false });
        self.gql("DeleteTweet", variables, Gql::Post, None)?;
        Ok(())
    }

    pub(super) fn web_like(&self, id: &str, on: bool) -> Result<(), ApiError> {
        let name = if on {
            "FavoriteTweet"
        } else {
            "UnfavoriteTweet"
        };
        let value = self.gql(name, json!({ "tweet_id": id }), Gql::Post, None)?;
        let field = if on {
            "favorite_tweet"
        } else {
            "unfavorite_tweet"
        };
        if value
            .get("data")
            .and_then(|data| data.get(field))
            .and_then(Value::as_str)
            != Some("Done")
        {
            return Err(ApiError::OutcomeUnknown(
                "X did not confirm the like".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn web_repost(&self, id: &str, on: bool) -> Result<(), ApiError> {
        let (name, variables) = if on {
            (
                "CreateRetweet",
                json!({ "tweet_id": id, "dark_request": false }),
            )
        } else {
            (
                "DeleteRetweet",
                json!({ "source_tweet_id": id, "tweet_id": id, "dark_request": false }),
            )
        };
        self.gql(name, variables, Gql::Post, None)?;
        Ok(())
    }

    pub(super) fn web_bookmark(&self, id: &str, on: bool) -> Result<(), ApiError> {
        let name = if on {
            "CreateBookmark"
        } else {
            "DeleteBookmark"
        };
        self.gql(name, json!({ "tweet_id": id }), Gql::Post, None)?;
        Ok(())
    }

    /// Follow uses the REST friendship endpoints. GraphQL no longer ships CreateFriendship.
    pub(super) fn web_follow(&self, user_id: &str, on: bool) -> Result<(), ApiError> {
        let action = if on { "create" } else { "destroy" };
        let url = format!("{}/i/api/1.1/friendships/{action}.json", self.web_origin());
        let response = self
            .http()
            .post(url)
            .headers(web_headers(self.credentials())?)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("user_id={user_id}&skip_status=true"))
            .send()
            .map_err(|error| request_failure(error, true))?;
        let status = response.status();
        let text = crate::net::http::read_text(response).map_err(ApiError::OutcomeUnknown)?;
        if already_following(&text) && on {
            return Ok(());
        }
        if status.is_success() {
            let value: Value = serde_json::from_str(&text).map_err(|_| {
                ApiError::OutcomeUnknown("X did not confirm the follow state".into())
            })?;
            if let Some(message) = graphql_error(&value) {
                return Err(ApiError::Other(message));
            }
            if value.get("id_str").and_then(Value::as_str).is_some() {
                return Ok(());
            }
            return Err(ApiError::OutcomeUnknown(
                "X did not confirm the follow state".into(),
            ));
        }
        Err(ApiError::from_response(status, &text))
    }

    fn gql(
        &self,
        name: &str,
        variables: Value,
        method: Gql,
        field_toggles: Option<Value>,
    ) -> Result<Value, ApiError> {
        let mut refreshed = false;
        loop {
            let qid = self.query_id(name)?;
            match self.gql_once(name, &qid, &variables, method, field_toggles.as_ref()) {
                Err(ApiError::Failed { status: 404, .. }) if !refreshed => {
                    // A 404 is either a rotated query id or a stale transaction id.
                    self.clear_tid();
                    let _ = self.refresh_ops();
                    refreshed = true;
                }
                other => return other,
            }
        }
    }

    fn gql_once(
        &self,
        name: &str,
        qid: &str,
        variables: &Value,
        method: Gql,
        field_toggles: Option<&Value>,
    ) -> Result<Value, ApiError> {
        let path = format!("/i/api/graphql/{}/{}", encode_path(qid), name);
        let url = format!("{}{path}", self.web_origin());
        let features = features().clone();
        let mut pairs = vec![
            ("variables".to_string(), variables.to_string()),
            ("features".to_string(), features.to_string()),
        ];
        if let Some(toggles) = field_toggles {
            pairs.push(("fieldToggles".to_string(), toggles.to_string()));
        }
        let request = match method {
            Gql::Get => self.http().get(url).query(&pairs),
            Gql::Post => {
                let body = json!({
                    "variables": variables,
                    "features": features,
                    "queryId": qid,
                });
                self.http().post(url).query(&pairs).json(&body)
            }
        };
        // `headers` replaces the map, so the transaction id has to be in that map.
        // Reads and likes answer without it. A wrong one on those calls is worse than none,
        // so only the write that the edge rejects gets it.
        let mut headers = web_headers(self.credentials())?;
        if name == "CreateBookmark" {
            let Some(header) = transaction_header(self, "POST", &path) else {
                return Err(ApiError::Other(
                    "X did not give socli a client transaction id. run `socli login x` again."
                        .into(),
                ));
            };
            insert_header(&mut headers, "x-client-transaction-id", &header)?;
        }
        let failed = |message: String| {
            if is_mutation(name) {
                ApiError::OutcomeUnknown(message)
            } else {
                ApiError::Other(message)
            }
        };
        let response = request
            .headers(headers)
            .send()
            .map_err(|error| request_failure(error, is_mutation(name)))?;
        let status = response.status();
        if status.as_u16() == 429 {
            let retry_after = retry_after(response.headers(), crate::clock::unix_secs());
            return Err(ApiError::RateLimited { retry_after });
        }
        let text = crate::net::http::read_text(response)
            .map_err(|error| failed(format!("reading the X response: {error}")))?;
        if !status.is_success() {
            return Err(ApiError::from_response(status, &text));
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| failed("X returned a response socli could not read".into()))?;
        if timeline_present(&value) && !is_mutation(name) {
            return Ok(value);
        }
        if let Some(message) = graphql_error(&value) {
            let status = if graphql_auth_error(&value) { 401 } else { 403 };
            return Err(ApiError::Failed {
                status,
                detail: message,
                hint: if status == 401 {
                    "\nThe X session expired or was rejected. Run `socli login x` again.".into()
                } else {
                    String::new()
                },
            });
        }
        if mutation_ok(&value) {
            Ok(value)
        } else {
            Err(failed("X returned no result".into()))
        }
    }

    fn query_id(&self, name: &str) -> Result<String, ApiError> {
        self.with_ops(|ops| ops.load_disk());
        if let Some(id) = self.with_ops(|ops| ops.fresh_id(name)) {
            return Ok(id);
        }
        if self.refresh_ops().is_err() {
            if let Some(id) = self.with_ops(|ops| ops.id(name)) {
                let now = now_secs();
                self.with_ops(|ops| ops.fetched_at = now);
                return Ok(id);
            }
        }
        self.with_ops(|ops| ops.id(name)).ok_or_else(|| {
            ApiError::Other(format!(
                "X did not publish the {name} operation. run `socli login x` again later."
            ))
        })
    }

    fn refresh_ops(&self) -> Result<(), ApiError> {
        let parsed = download_ops(self)?;
        self.with_ops(|ops| ops.store(parsed));
        Ok(())
    }

    fn with_ops<T>(&self, f: impl FnOnce(&mut OpCache) -> T) -> T {
        let mut guard = lock_ops(self);
        f(&mut guard)
    }
}

fn lock_ops(client: &XClient) -> MutexGuard<'_, OpCache> {
    client.ops().lock().unwrap_or_else(|err| err.into_inner())
}

fn home_vars(count: u8, cursor: Option<&str>) -> Value {
    let mut variables = json!({
        "count": clamp_count(count),
        "includePromotedContent": false,
        "latestControlAvailable": true,
        "requestContext": "launch",
        "withCommunity": true,
    });
    if let Some(cursor) = cursor {
        variables["cursor"] = json!(cursor);
    }
    variables
}

fn clamp_count(count: u8) -> u8 {
    count.clamp(1, 40)
}

fn features() -> &'static Value {
    static FEATURES: OnceLock<Value> = OnceLock::new();
    FEATURES.get_or_init(|| serde_json::from_str(FEATURES_JSON).unwrap_or_else(|_| json!({})))
}

fn transaction_header(client: &XClient, method: &str, path: &str) -> Option<String> {
    let tid = ensure_transaction(client)?;
    Some(tid.header(method, path))
}

fn ensure_transaction(client: &XClient) -> Option<super::transaction::Transaction> {
    if let Some(tid) = {
        let guard = client.tid().lock().unwrap_or_else(|err| err.into_inner());
        guard.clone()
    } {
        return Some(tid);
    }
    let html = home_document(client).ok()?;
    let url = super::transaction::ondemand_url(&html)?;
    let js = script_text(client, &url).ok()?;
    let tid = super::transaction::Transaction::from_page(&html, &js)?;
    let mut guard = client.tid().lock().unwrap_or_else(|err| err.into_inner());
    *guard = Some(tid.clone());
    Some(tid)
}

fn home_document(client: &XClient) -> Result<String, ApiError> {
    let response = client
        .http()
        .get(format!("{}/home", client.web_origin()))
        .headers(document_headers(client.credentials())?)
        .header("user-agent", CHROME_UA)
        .send()
        .map_err(|err| ApiError::Other(format!("could not open x.com to sign a write: {err}")))?;
    let status = response.status();
    let html = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::Other(format!(
            "x.com returned {} while socli was signing a write. run `socli login x` again.",
            status.as_u16()
        )));
    }
    Ok(html)
}

fn script_text(client: &XClient, url: &str) -> Result<String, ApiError> {
    let response = client
        .http()
        .get(url)
        .header("user-agent", CHROME_UA)
        .send()
        .map_err(|err| {
            ApiError::Other(format!("could not download the X animation script: {err}"))
        })?;
    if !response.status().is_success() {
        return Err(ApiError::Other(
            "the X animation script did not download. run `socli login x` again later.".into(),
        ));
    }
    Ok(response.text().unwrap_or_default())
}

fn web_headers(creds: &Credentials) -> Result<header::HeaderMap, ApiError> {
    let (token, ct0) = match (creds.auth_token.as_deref(), creds.ct0.as_deref()) {
        (Some(token), Some(ct0)) if !token.is_empty() && !ct0.is_empty() => (token, ct0),
        _ => {
            return Err(ApiError::Other(
                "the X browser session is missing. run `socli login x`.".into(),
            ))
        }
    };
    let mut headers = header::HeaderMap::new();
    let bearer = format!("Bearer {WEB_BEARER}");
    let cookie = format!("auth_token={token}; ct0={ct0}");
    insert_header(&mut headers, "authorization", &bearer)?;
    insert_header(&mut headers, "x-csrf-token", ct0)?;
    insert_header(&mut headers, "cookie", &cookie)?;
    insert_header(&mut headers, "x-twitter-auth-type", "OAuth2Session")?;
    insert_header(&mut headers, "x-twitter-active-user", "yes")?;
    insert_header(&mut headers, "x-twitter-client-language", "en")?;
    insert_header(&mut headers, "origin", "https://x.com")?;
    insert_header(&mut headers, "referer", "https://x.com/home")?;
    insert_header(&mut headers, "user-agent", CHROME_UA)?;
    insert_header(&mut headers, "accept", "*/*")?;
    Ok(headers)
}

fn insert_header(
    headers: &mut header::HeaderMap,
    name: &'static str,
    value: &str,
) -> Result<(), ApiError> {
    let name = header::HeaderName::from_static(name);
    let value = header::HeaderValue::from_str(value).map_err(|_| {
        ApiError::Other("the X session cookie is not valid text. run `socli login x` again.".into())
    })?;
    headers.insert(name, value);
    Ok(())
}

fn download_ops(client: &XClient) -> Result<HashMap<String, String>, ApiError> {
    let home = client
        .http()
        .get(format!("{}/home", client.web_origin()))
        .headers(document_headers(client.credentials())?)
        .header("user-agent", CHROME_UA)
        .send()
        .map_err(|err| {
            ApiError::Other(format!("could not open x.com to read operation ids: {err}"))
        })?;
    let status = home.status();
    let html = home.text().unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::Other(format!(
            "x.com returned {} while socli was reading operation ids. run `socli login x` again.",
            status.as_u16()
        )));
    }
    let script = main_script(&html).ok_or_else(|| {
        ApiError::Other(
            "x.com did not point at its web client. run `socli login x` again later.".into(),
        )
    })?;
    let bundle = client
        .http()
        .get(script)
        .header("user-agent", CHROME_UA)
        .send()
        .map_err(|err| ApiError::Other(format!("could not download the X web client: {err}")))?;
    if !bundle.status().is_success() {
        return Err(ApiError::Other(
            "the X web client did not download. run `socli login x` again later.".into(),
        ));
    }
    let js = bundle.text().unwrap_or_default();
    let ops = parse_ops(&js);
    if ops.is_empty() {
        return Err(ApiError::Other(
            "the X web client had no operations in it. run `socli login x` again later.".into(),
        ));
    }
    Ok(ops)
}

/// The HTML document takes the cookie and a browser user agent. The GraphQL bearer on this
/// request is rejected.
fn document_headers(creds: &Credentials) -> Result<header::HeaderMap, ApiError> {
    let (token, ct0) = match (creds.auth_token.as_deref(), creds.ct0.as_deref()) {
        (Some(token), Some(ct0)) => (token, ct0),
        _ => {
            return Err(ApiError::Other(
                "the X browser session is missing. run `socli login x`.".into(),
            ))
        }
    };
    let mut headers = header::HeaderMap::new();
    insert_header(
        &mut headers,
        "cookie",
        &format!("auth_token={token}; ct0={ct0}"),
    )?;
    Ok(headers)
}

/// The signed-in home page embeds one `screen_name`. More than one is ambiguous.
fn viewer_handle(client: &XClient) -> Result<String, ApiError> {
    let home = client
        .http()
        .get(format!("{}/home", client.web_origin()))
        .headers(document_headers(client.credentials())?)
        .header("user-agent", CHROME_UA)
        .send()
        .map_err(|err| {
            ApiError::Other(format!("could not open x.com to read the account: {err}"))
        })?;
    let status = home.status();
    let html = home.text().unwrap_or_default();
    if !status.is_success() {
        return Err(ApiError::from_response(status, &html));
    }
    screen_name_from_home(&html).ok_or_else(|| {
        ApiError::Other(
            "x.com did not name the signed-in account. run `socli login x` again.".into(),
        )
    })
}

fn screen_name_from_home(html: &str) -> Option<String> {
    let key = "\"screen_name\":\"";
    let mut rest = html;
    let mut found = None;
    while let Some(index) = rest.find(key) {
        rest = &rest[index + key.len()..];
        let end = rest.find('"')?;
        let name = &rest[..end];
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        if found.as_deref().is_some_and(|have| have != name) {
            return None;
        }
        found = Some(name.to_string());
    }
    found
}

fn main_script(html: &str) -> Option<String> {
    let marker = "https://abs.twimg.com/responsive-web/client-web/main.";
    let start = html.find(marker)?;
    let rest = &html[start..];
    let end = rest.find(['"', '\'', ' ', ')'])?;
    Some(rest[..end].to_string())
}

pub fn parse_ops(js: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = js;
    let needle = "queryId:\"";
    while let Some(index) = rest.find(needle) {
        rest = &rest[index + needle.len()..];
        let Some(end) = rest.find('"') else { break };
        let qid = &rest[..end];
        rest = &rest[end + 1..];
        let name_key = "operationName:\"";
        let Some(name_at) = rest.find(name_key) else {
            break;
        };
        if name_at > 80 {
            continue;
        }
        let after = &rest[name_at + name_key.len()..];
        let Some(name_end) = after.find('"') else {
            break;
        };
        let name = &after[..name_end];
        if !qid.is_empty() && !name.is_empty() {
            out.insert(name.to_string(), qid.to_string());
        }
        rest = &after[name_end..];
    }
    out
}

fn timeline_present(value: &Value) -> bool {
    instructions_of(value).is_some()
}

fn is_mutation(name: &str) -> bool {
    matches!(
        name,
        "CreateTweet"
            | "DeleteTweet"
            | "FavoriteTweet"
            | "UnfavoriteTweet"
            | "CreateRetweet"
            | "DeleteRetweet"
            | "CreateBookmark"
            | "DeleteBookmark"
    )
}

fn mutation_ok(value: &Value) -> bool {
    value
        .get("data")
        .and_then(Value::as_object)
        .is_some_and(|data| {
            data.values()
                .any(|value| !value.is_null() && value != &Value::Bool(false))
        })
        && graphql_error(value).is_none()
}

fn graphql_auth_error(value: &Value) -> bool {
    value
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| {
            errors.iter().any(|error| {
                matches!(
                    error.get("code").and_then(Value::as_u64),
                    Some(32 | 89 | 215 | 239)
                )
            })
        })
}

fn graphql_error(value: &Value) -> Option<String> {
    let errors = value.get("errors")?.as_array()?;
    let parts: Vec<&str> = errors
        .iter()
        .filter_map(|err| err.get("message")?.as_str())
        .collect();
    if errors.is_empty() {
        None
    } else if parts.is_empty() {
        Some("X rejected the request".into())
    } else {
        Some(parts.join("; "))
    }
}

fn already_following(body: &str) -> bool {
    body.contains("\"code\":160") || body.contains("\"code\": 160")
}

pub fn parse_timeline(value: &Value) -> Page {
    let Some(instructions) = instructions_of(value) else {
        return Page {
            ids: Vec::new(),
            posts: Vec::new(),
            next: None,
        };
    };
    let mut acc = Acc::default();
    for instruction in instructions {
        walk_instruction(instruction, &mut acc);
    }
    Page {
        ids: acc.ids,
        posts: acc.posts,
        next: acc.cursor,
    }
}

fn instructions_of(value: &Value) -> Option<&Vec<Value>> {
    const PATHS: &[&str] = &[
        "/data/home/home_timeline_urt/instructions",
        "/data/user/result/timeline/timeline/instructions",
        "/data/search_by_raw_query/search_timeline/timeline/instructions",
        "/data/bookmark_timeline_v2/timeline/instructions",
        "/data/threaded_conversation_with_injections_v2/instructions",
        "/data/threaded_conversation_with_injections_v2/timeline/instructions",
    ];
    for path in PATHS {
        if let Some(items) = value.pointer(path).and_then(Value::as_array) {
            return Some(items);
        }
    }
    find_instructions(value.get("data").unwrap_or(value), 0)
}

fn find_instructions(value: &Value, depth: u8) -> Option<&Vec<Value>> {
    if depth > 7 {
        return None;
    }
    match value {
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_instructions(item, depth + 1)),
        Value::Object(map) => {
            if let Some(items) = map.get("instructions").and_then(Value::as_array) {
                return Some(items);
            }
            map.iter().find_map(|(key, child)| {
                if matches!(
                    key.as_str(),
                    "legacy"
                        | "core"
                        | "tweet"
                        | "quoted_status_result"
                        | "retweeted_status_result"
                ) {
                    return None;
                }
                find_instructions(child, depth + 1)
            })
        }
        _ => None,
    }
}

#[derive(Default)]
struct Acc {
    ids: Vec<String>,
    posts: Vec<Post>,
    cursor: Option<String>,
}

fn walk_instruction(instruction: &Value, acc: &mut Acc) {
    if let Some(entries) = instruction.get("entries").and_then(Value::as_array) {
        for entry in entries {
            walk_entry(entry, acc);
        }
        return;
    }
    if let Some(entry) = instruction.get("entry") {
        walk_entry(entry, acc);
        return;
    }
    if instruction.get("content").is_some() {
        walk_entry(instruction, acc);
    }
}

fn walk_entry(entry: &Value, acc: &mut Acc) {
    if entry.get("promotedMetadata").is_some() {
        return;
    }
    let Some(content) = entry.get("content") else {
        return;
    };
    if content.get("promotedMetadata").is_some() {
        return;
    }
    if let Some(cursor) = cursor_value(content) {
        // The last Bottom cursor is the next page. An earlier Gap or ShowMore cursor
        // only fills one hole, and following it loads a single post.
        acc.cursor = Some(cursor);
        return;
    }
    if let Some(items) = content.get("items").and_then(Value::as_array) {
        for item in items {
            let node = item.get("item").unwrap_or(item);
            push_tweet(node, acc);
        }
        return;
    }
    push_tweet(content, acc);
}

fn push_tweet(node: &Value, acc: &mut Acc) {
    let result = node
        .pointer("/itemContent/tweet_results/result")
        .or_else(|| node.pointer("/tweet_results/result"));
    let Some(result) = result else {
        return;
    };
    if result.pointer("/promotedMetadata").is_some()
        || node.pointer("/itemContent/promotedMetadata").is_some()
    {
        return;
    }
    let Some(post) = parse_tweet(result, 0) else {
        return;
    };
    if let Some(quoted) = post.quoted.clone() {
        if !acc.posts.iter().any(|item| item.id == quoted.id) {
            acc.posts.push(*quoted);
        }
    }
    if !acc.ids.contains(&post.id) {
        acc.ids.push(post.id.clone());
    }
    if let Some(slot) = acc.posts.iter_mut().find(|item| item.id == post.id) {
        *slot = post;
    } else {
        acc.posts.push(post);
    }
}

fn cursor_value(content: &Value) -> Option<String> {
    if content.get("cursorType").and_then(Value::as_str) != Some("Bottom") {
        return None;
    }
    content
        .get("value")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_string)
}

fn parse_tweet(result: &Value, depth: u8) -> Option<Post> {
    let result = unwrap_tweet(result);
    if result.get("__typename").and_then(Value::as_str) == Some("TweetTombstone") {
        return None;
    }
    let legacy = result.get("legacy")?;
    let id = result.get("rest_id").and_then(Value::as_str)?.to_string();
    let author = user_of(result).unwrap_or_else(|| unknown_user(&id));
    if let Some(original) = legacy
        .pointer("/retweeted_status_result/result")
        .or_else(|| result.pointer("/retweeted_status_result/result"))
    {
        if depth < 2 {
            if let Some(mut post) = parse_tweet(original, depth + 1) {
                post.reposted_by = Some(author.username);
                return Some(post);
            }
        }
    }
    let links = links_of(legacy.get("entities"));
    let mut text = result
        .pointer("/note_tweet/note_tweet_results/result/text")
        .and_then(Value::as_str)
        .or_else(|| legacy.get("full_text").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    for link in &links {
        if !link.short.is_empty() {
            text = text.replace(&link.short, &link.expanded);
        }
    }
    let quoted = if depth < 2 {
        legacy
            .pointer("/quoted_status_result/result")
            .or_else(|| result.pointer("/quoted_status_result/result"))
            .and_then(|quoted| parse_tweet(quoted, depth + 1))
            .map(Box::new)
    } else {
        None
    };
    let handle = if author.username.is_empty() {
        "i"
    } else {
        author.username.as_str()
    };
    Some(Post {
        id: id.clone(),
        net: Network::X,
        url: Some(format!("https://x.com/{handle}/status/{id}")),
        cid: None,
        author,
        text,
        created_at: legacy
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(parse_twitter_time),
        conversation_id: legacy
            .get("conversation_id_str")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_string(),
        in_reply_to: legacy
            .get("in_reply_to_status_id_str")
            .and_then(Value::as_str)
            .map(str::to_string),
        quoted,
        reposted_by: None,
        metrics: metrics_of(legacy, result.get("views")),
        links,
        media: media_of(legacy),
        liked: legacy
            .get("favorited")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        bookmarked: legacy
            .get("bookmarked")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        reposted: legacy
            .get("retweeted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn unwrap_tweet(result: &Value) -> &Value {
    if result.get("__typename").and_then(Value::as_str) == Some("TweetWithVisibilityResults") {
        result.get("tweet").unwrap_or(result)
    } else {
        result
    }
}

fn user_of(result: &Value) -> Option<User> {
    let node = result.pointer("/core/user_results/result")?;
    parse_user(node)
}

fn user_from_payload(value: &Value) -> Option<User> {
    value
        .pointer("/data/user/result")
        .or_else(|| value.pointer("/data/user_result/result"))
        .and_then(parse_user)
}

fn parse_user(node: &Value) -> Option<User> {
    if node.get("__typename").and_then(Value::as_str) == Some("UserUnavailable") {
        return None;
    }
    let legacy = node.get("legacy");
    let core = node.get("core");
    let username = text_at(legacy, "screen_name")
        .or_else(|| text_at(core, "screen_name"))
        .or_else(|| text_at(Some(node), "screen_name"))?;
    let name = text_at(legacy, "name")
        .or_else(|| text_at(core, "name"))
        .unwrap_or_else(|| username.clone());
    let id = node
        .get("rest_id")
        .and_then(Value::as_str)
        .unwrap_or(&username)
        .to_string();
    let verified = node
        .get("is_blue_verified")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || legacy
            .and_then(|v| v.get("verified"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let business = legacy
        .and_then(|v| v.get("verified_type"))
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("Business"));
    Some(User {
        id,
        name,
        username,
        avatar_url: text_at(legacy, "profile_image_url_https")
            .or_else(|| text_at(node.pointer("/avatar"), "image_url")),
        verified,
        business,
        description: text_at(legacy, "description").filter(|text| !text.is_empty()),
    })
}

fn text_at(node: Option<&Value>, key: &str) -> Option<String> {
    node?.get(key)?.as_str().map(str::to_string)
}

fn unknown_user(id: &str) -> User {
    User {
        id: id.to_string(),
        name: "unknown".into(),
        username: String::new(),
        avatar_url: None,
        verified: false,
        business: false,
        description: None,
    }
}

fn links_of(entities: Option<&Value>) -> Vec<Link> {
    let Some(urls) = entities
        .and_then(|v| v.get("urls"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    urls.iter()
        .filter_map(|url| {
            let short = url
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let expanded = url
                .get("expanded_url")
                .and_then(Value::as_str)
                .unwrap_or(&short)
                .to_string();
            if short.is_empty() && expanded.is_empty() {
                return None;
            }
            Some(Link {
                display: url
                    .get("display_url")
                    .and_then(Value::as_str)
                    .unwrap_or(&expanded)
                    .to_string(),
                short,
                expanded,
                title: None,
                description: None,
                image: None,
                image_width: None,
                image_height: None,
                site: None,
            })
        })
        .collect()
}

fn media_of(legacy: &Value) -> Vec<Media> {
    let items = legacy
        .pointer("/extended_entities/media")
        .or_else(|| legacy.pointer("/entities/media"))
        .and_then(Value::as_array);
    let Some(items) = items else {
        return Vec::new();
    };
    items.iter().filter_map(parse_media).collect()
}

fn parse_media(item: &Value) -> Option<Media> {
    let kind = match item.get("type").and_then(Value::as_str).unwrap_or("") {
        "photo" => MediaKind::Photo,
        "video" => MediaKind::Video,
        "animated_gif" => MediaKind::Gif,
        _ => MediaKind::Other,
    };
    let thumb = item
        .get("media_url_https")
        .and_then(Value::as_str)
        .map(str::to_string);
    let video = item
        .pointer("/video_info/variants")
        .and_then(Value::as_array)
        .and_then(|variants| {
            variants
                .iter()
                .filter_map(|variant| variant.get("url").and_then(Value::as_str))
                .find(|url| url.contains(".mp4"))
        })
        .map(str::to_string);
    let (url, preview_url) = match kind {
        MediaKind::Photo | MediaKind::Gif => (thumb.clone(), None),
        MediaKind::Video | MediaKind::Other => (video.or(thumb.clone()), thumb),
    };
    Some(Media {
        key: item
            .get("id_str")
            .or_else(|| item.get("media_key"))
            .and_then(Value::as_str)
            .unwrap_or("media")
            .to_string(),
        kind,
        url,
        preview_url,
        alt: item
            .get("ext_alt_text")
            .and_then(Value::as_str)
            .map(str::to_string),
        width: item
            .pointer("/original_info/width")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        height: item
            .pointer("/original_info/height")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
    })
}

fn metrics_of(legacy: &Value, views: Option<&Value>) -> Metrics {
    Metrics {
        replies: num(legacy, "reply_count"),
        reposts: num(legacy, "retweet_count"),
        likes: num(legacy, "favorite_count"),
        quotes: num(legacy, "quote_count"),
        bookmarks: num(legacy, "bookmark_count"),
        views: views.and_then(|v| v.get("count")).map(number).unwrap_or(0),
    }
}

fn num(value: &Value, key: &str) -> u64 {
    value.get(key).map(number).unwrap_or(0)
}

fn number(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .unwrap_or(0)
}

fn parse_twitter_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_str(value, "%a %b %d %H:%M:%S %z %Y")
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

fn encode_path(segment: &str) -> String {
    let mut out = String::new();
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn now_secs() -> i64 {
    crate::clock::unix_secs() as i64
}

#[derive(serde::Deserialize)]
struct DiskOps {
    fetched_at: i64,
    ops: BTreeMap<String, String>,
    #[serde(default)]
    version: u32,
}

fn read_disk() -> Option<DiskOps> {
    let text = std::fs::read_to_string(crate::config::dir().join("x-ops.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_disk(cache: &OpCache) -> Result<(), ()> {
    let path = crate::config::dir().join("x-ops.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ())?;
    }
    let ops: BTreeMap<_, _> = cache
        .ids
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let body = serde_json::to_string(&json!({
        "fetched_at": cache.fetched_at,
        "version": CACHE_VERSION,
        "ops": ops,
    }))
    .map_err(|_| ())?;
    std::fs::write(path, body).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mutation_client(body: Option<&'static str>) -> XClient {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = [0u8; 16384];
            let _ = stream.read(&mut request);
            if let Some(body) = body {
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let client = XClient::for_test(
            Credentials {
                auth_token: Some("test-session".into()),
                ct0: Some("test-csrf".into()),
                ..Credentials::default()
            },
            format!("http://{addr}"),
        )
        .unwrap();
        client.with_ops(|ops| {
            ops.loaded = true;
            ops.fetched_at = now_secs();
        });
        client
    }

    #[test]
    fn browser_likes_require_an_acknowledgement() {
        for body in [
            r#"{"data":null}"#,
            r#"{"data":{"favorite_tweet":null}}"#,
            r#"{"data":{"unrelated":"Done"}}"#,
        ] {
            let client = mutation_client(Some(body));
            assert!(matches!(
                client.web_like("post", true),
                Err(ApiError::OutcomeUnknown(_))
            ));
        }
        mutation_client(Some(r#"{"data":{"favorite_tweet":"Done"}}"#))
            .web_like("post", true)
            .unwrap();
        mutation_client(Some(r#"{"data":{"unfavorite_tweet":"Done"}}"#))
            .web_like("post", false)
            .unwrap();
    }

    #[test]
    fn browser_write_rejections_and_lost_responses_stay_distinct() {
        let client = mutation_client(Some(
            r#"{"data":null,"errors":[{"code":89,"message":"expired"}]}"#,
        ));
        let error = client.web_like("post", true).unwrap_err();
        assert!(matches!(error, ApiError::Failed { status: 401, .. }));
        assert!(error.to_string().contains("socli login x"));
        let client = mutation_client(None);
        assert!(matches!(
            client.web_like("post", true),
            Err(ApiError::OutcomeUnknown(_))
        ));
    }

    #[test]
    fn code_only_graphql_errors_are_rejections() {
        assert!(
            graphql_error(&json!({"data":{"favorite_tweet":"Done"},"errors":[{"code":89}]}))
                .is_some()
        );
        assert!(!mutation_ok(&json!({"data":null})));
        assert!(!mutation_ok(&json!({"data":{"favorite_tweet":false}})));
    }

    #[test]
    fn home_html_names_one_account() {
        let html =
            r#"<script>{"entities":{"users":{"7":{"name":"Ada","screen_name":"ada"}}}}</script>"#;
        assert_eq!(screen_name_from_home(html).as_deref(), Some("ada"));
        let twice = r#"{"screen_name":"ada"}{"screen_name":"bea"}"#;
        assert_eq!(screen_name_from_home(twice), None);
    }

    #[test]
    fn ops_regex_reads_the_bundle_shape() {
        let js =
            r#"e.exports={queryId:"abc_DEF-123",operationName:"UserTweets",operationType:"query"}"#;
        let ops = parse_ops(js);
        assert_eq!(
            ops.get("UserTweets").map(String::as_str),
            Some("abc_DEF-123")
        );
    }

    #[test]
    fn timeline_keeps_order_skips_promoted_and_reads_the_cursor() {
        let value = serde_json::from_str::<Value>(
            r#"{
            "data": {
                "home": {
                    "home_timeline_urt": {
                        "instructions": [{
                            "type": "TimelineAddEntries",
                            "entries": [
                                {
                                    "content": {
                                        "itemContent": {
                                            "promotedMetadata": { "x": 1 },
                                            "tweet_results": { "result": {
                                                "__typename": "Tweet",
                                                "rest_id": "9",
                                                "legacy": {
                                                    "full_text": "ad",
                                                    "created_at": "Wed Oct 10 20:19:24 +0000 2018",
                                                    "conversation_id_str": "9"
                                                },
                                                "core": { "user_results": { "result": {
                                                    "rest_id": "7",
                                                    "legacy": { "screen_name": "ads", "name": "ads" }
                                                } } }
                                            } }
                                        }
                                    }
                                },
                                {
                                    "content": {
                                        "itemContent": {
                                            "tweet_results": {
                                                "result": {
                                                    "__typename": "TweetWithVisibilityResults",
                                                    "tweet": {
                                                        "__typename": "Tweet",
                                                        "rest_id": "100",
                                                        "views": { "count": "5" },
                                                        "legacy": {
                                                            "full_text": "hello https://t.co/abc",
                                                            "created_at": "Wed Oct 10 20:19:24 +0000 2018",
                                                            "conversation_id_str": "100",
                                                            "favorited": true,
                                                            "reply_count": 1,
                                                            "retweet_count": 2,
                                                            "favorite_count": 3,
                                                            "quote_count": 0,
                                                            "bookmark_count": 4,
                                                            "entities": { "urls": [{
                                                                "url": "https://t.co/abc",
                                                                "expanded_url": "https://example.com/a",
                                                                "display_url": "example.com/a"
                                                            }] },
                                                            "extended_entities": { "media": [{
                                                                "id_str": "m1",
                                                                "type": "photo",
                                                                "media_url_https": "https://pbs.twimg.com/media/x.jpg",
                                                                "ext_alt_text": "a cat",
                                                                "original_info": { "width": 10, "height": 20 }
                                                            }] }
                                                        },
                                                        "core": { "user_results": { "result": {
                                                            "rest_id": "7",
                                                            "is_blue_verified": true,
                                                            "legacy": {
                                                                "screen_name": "ada",
                                                                "name": "Ada",
                                                                "description": "hi",
                                                                "profile_image_url_https": "https://pbs.twimg.com/profile.jpg"
                                                            }
                                                        } } }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                },
                                {
                                    "content": {
                                        "entryType": "TimelineTimelineCursor",
                                        "cursorType": "Bottom",
                                        "value": "next-page"
                                    }
                                }
                            ]
                        }]
                    }
                }
            }
        }"#,
        )
        .unwrap();
        let page = parse_timeline(&value);
        assert_eq!(page.ids, vec!["100".to_string()]);
        assert_eq!(page.next.as_deref(), Some("next-page"));
        let post = page.posts.iter().find(|post| post.id == "100").unwrap();
        assert_eq!(post.text, "hello https://example.com/a");
        assert!(post.liked);
        assert_eq!(post.metrics.views, 5);
        assert_eq!(post.metrics.likes, 3);
        assert_eq!(post.author.username, "ada");
        assert!(post.author.verified);
        assert_eq!(
            post.author.avatar_url.as_deref(),
            Some("https://pbs.twimg.com/profile.jpg")
        );
        assert_eq!(post.media.len(), 1);
        assert_eq!(
            post.media[0].view_url(),
            Some("https://pbs.twimg.com/media/x.jpg")
        );
        assert_eq!(post.media[0].alt.as_deref(), Some("a cat"));
        assert!(post.created_at.is_some());
    }

    #[test]
    fn a_gap_cursor_is_not_the_next_page() {
        let value = serde_json::from_str::<Value>(
            r#"{
            "data": { "home": { "home_timeline_urt": { "instructions": [{
                "entries": [
                    {
                        "content": {
                            "entryType": "TimelineTimelineCursor",
                            "cursorType": "Top",
                            "value": "top"
                        }
                    },
                    {
                        "content": {
                            "entryType": "TimelineTimelineCursor",
                            "cursorType": "Gap",
                            "value": "one-post"
                        }
                    },
                    {
                        "content": { "itemContent": { "tweet_results": { "result": {
                            "__typename": "Tweet",
                            "rest_id": "1",
                            "legacy": {
                                "full_text": "first",
                                "created_at": "Wed Oct 10 20:19:24 +0000 2018",
                                "conversation_id_str": "1"
                            },
                            "core": { "user_results": { "result": {
                                "rest_id": "7",
                                "legacy": { "screen_name": "ada", "name": "Ada" }
                            } } }
                        } } } }
                    },
                    {
                        "content": { "itemContent": { "tweet_results": { "result": {
                            "__typename": "Tweet",
                            "rest_id": "2",
                            "legacy": {
                                "full_text": "second",
                                "created_at": "Wed Oct 10 20:19:24 +0000 2018",
                                "conversation_id_str": "2"
                            },
                            "core": { "user_results": { "result": {
                                "rest_id": "7",
                                "legacy": { "screen_name": "ada", "name": "Ada" }
                            } } }
                        } } } }
                    },
                    {
                        "content": {
                            "entryType": "TimelineTimelineCursor",
                            "cursorType": "Bottom",
                            "value": "next-page"
                        }
                    }
                ]
            }] } } }
        }"#,
        )
        .unwrap();
        let page = parse_timeline(&value);
        assert_eq!(page.ids, vec!["1".to_string(), "2".to_string()]);
        assert_eq!(page.next.as_deref(), Some("next-page"));
    }

    #[test]
    fn a_retweet_keeps_the_original_id_and_names_the_reposter() {
        let value = serde_json::from_str::<Value>(
            r#"{
            "data": { "home": { "home_timeline_urt": { "instructions": [{
                "entries": [{
                    "content": { "itemContent": { "tweet_results": { "result": {
                        "__typename": "Tweet",
                        "rest_id": "wrap",
                        "legacy": {
                            "full_text": "RT @ada: hello",
                            "retweeted_status_result": { "result": {
                                "__typename": "Tweet",
                                "rest_id": "100",
                                "legacy": {
                                    "full_text": "hello",
                                    "created_at": "Wed Oct 10 20:19:24 +0000 2018",
                                    "conversation_id_str": "100"
                                },
                                "core": { "user_results": { "result": {
                                    "rest_id": "7",
                                    "legacy": { "screen_name": "ada", "name": "ada" }
                                } } }
                            } }
                        },
                        "core": { "user_results": { "result": {
                            "rest_id": "2",
                            "legacy": { "screen_name": "bea", "name": "Bea" }
                        } } }
                    } } } }
                }]
            }] } } }
        }"#,
        )
        .unwrap();
        let page = parse_timeline(&value);
        assert_eq!(page.ids, vec!["100".to_string()]);
        assert_eq!(page.posts[0].reposted_by.as_deref(), Some("bea"));
        assert_eq!(page.posts[0].author.username, "ada");
    }
}
