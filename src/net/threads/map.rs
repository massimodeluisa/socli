//! Threads response shapes into the reader's model, and Meta's error bodies into `NetError`.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::{Link, Media, MediaKind, Metrics, Page, Post, User};
use crate::net::{scoped, NetError, Network};

const NET: Network = Network::Threads;

/** Fields of `GET /me/threads`; `owner` appears only on top-level posts the token owns. */
pub const FIELDS_OWN: &str = "id,media_product_type,media_type,media_url,permalink,owner,username,text,timestamp,shortcode,thumbnail_url,children,is_quote_post,quoted_post,reposted_post,alt_text,link_attachment_url,has_replies,root_post,replied_to,is_reply";

/** Fields for single posts, mentions and keyword search, where `owner` is not listed. */
pub const FIELDS_PUBLIC: &str = "id,media_product_type,media_type,media_url,permalink,username,text,timestamp,shortcode,thumbnail_url,children,is_quote_post,quoted_post,reposted_post,alt_text,link_attachment_url,has_replies,root_post,replied_to,is_reply";

/** Fields the `/{id}/conversation` endpoint lists; it has no `owner`, `reposted_post`, `alt_text` or `link_attachment_url`. */
pub const FIELDS_CONVERSATION: &str = "id,media_product_type,media_type,media_url,permalink,username,text,timestamp,shortcode,thumbnail_url,children,is_quote_post,quoted_post,has_replies,root_post,replied_to,is_reply";

pub const FIELDS_PROFILE: &str =
    "id,username,name,threads_profile_picture_url,threads_biography,is_verified";

// MARK: errors

/** Turns a failed response into `NetError`: Meta's `error.code`, or the OAuth endpoint's `error_message`. */
pub fn error_from_body(status: u16, retry_after: Option<u64>, body: &str) -> NetError {
    let rate_limited = NetError::RateLimited {
        net: NET,
        retry_after,
    };
    if status == 429 {
        return rate_limited;
    }
    let value: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if let Some(error) = value.get("error").filter(|e| e.is_object()) {
        let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the request failed");
        return match code {
            190 => NetError::SignInRequired(NET),
            4 | 17 | 32 | 613 => rate_limited,
            _ => {
                let hint = if code == 10 || (200..300).contains(&code) {
                    ". grant the missing permission, then run `socli login threads` again"
                } else {
                    ""
                };
                NetError::Http {
                    net: NET,
                    status,
                    message: format!("threads: {message} (code {code}){hint}"),
                }
            }
        };
    }
    if let Some(text) = value
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return NetError::Http {
            net: NET,
            status,
            message: format!("threads: {text}"),
        };
    }
    let message = match value.get("error_message").and_then(Value::as_str) {
        Some(text) => format!("threads: {text}"),
        None => format!("Threads returned {status}"),
    };
    NetError::Http {
        net: NET,
        status,
        message,
    }
}

/// Parses a 2xx body, or maps a failed response through `error_from_body`.
pub fn decode(status: u16, retry_after: Option<u64>, body: &str) -> Result<Value, NetError> {
    if !(200..300).contains(&status) {
        return Err(error_from_body(status, retry_after, body));
    }
    serde_json::from_str(body).map_err(|e| NetError::Parse {
        net: NET,
        message: e.to_string(),
    })
}

// MARK: posts

pub fn timestamp(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%z")
        .or_else(|_| DateTime::parse_from_rfc3339(text))
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

pub fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
}

/// The id inside a `{ "id": … }` reference, or a bare id string.
pub fn ref_id(value: Option<&Value>) -> Option<&str> {
    match value? {
        Value::String(id) => Some(id.as_str()),
        other => other.get("id").and_then(Value::as_str),
    }
}

fn unknown_user(id: &str) -> User {
    User {
        id: scoped(NET, id),
        name: "unknown".into(),
        username: "unknown".into(),
        avatar_url: None,
        verified: false,
        business: false,
        description: None,
    }
}

fn author_of(value: &Value) -> User {
    let owner = ref_id(value.get("owner"));
    match text_of(value, "username") {
        Some(username) => User {
            id: scoped(NET, owner.unwrap_or(username)),
            name: username.into(),
            username: username.into(),
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        },
        None => unknown_user(owner.unwrap_or("unknown")),
    }
}

fn display_url(url: &str) -> String {
    let bare = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let bare = bare.strip_prefix("www.").unwrap_or(bare);
    bare.trim_end_matches('/').to_string()
}

fn media_item(item: &Value) -> Option<Media> {
    let kind = match text_of(item, "media_type") {
        Some("IMAGE") => MediaKind::Photo,
        Some("VIDEO") => MediaKind::Video,
        _ => MediaKind::Other,
    };
    Some(Media {
        key: text_of(item, "id")?.to_string(),
        kind,
        url: text_of(item, "media_url").map(String::from),
        preview_url: text_of(item, "thumbnail_url").map(String::from),
        alt: text_of(item, "alt_text").map(String::from),
        width: None,
        height: None,
    })
}

fn media_of(value: &Value) -> Vec<Media> {
    match text_of(value, "media_type") {
        Some("IMAGE" | "VIDEO" | "AUDIO") => media_item(value).into_iter().collect(),
        Some("CAROUSEL_ALBUM" | "CAROUSEL") => value
            .pointer("/children/data")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(media_item).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// A post known only by id: a quote target or a repost original not fetched yet.
pub fn is_stub(post: &Post) -> bool {
    post.url.is_none() && post.text.is_empty() && post.created_at.is_none()
}

fn embedded(value: Option<&Value>) -> Option<Post> {
    match value? {
        Value::String(id) => post(&serde_json::json!({ "id": id })),
        object @ Value::Object(_) => post(object),
        _ => None,
    }
}

/** A post from one media object. A `REPOST_FACADE` becomes its original, marked `reposted_by`. */
pub fn post(value: &Value) -> Option<Post> {
    let id = text_of(value, "id")?;
    if text_of(value, "media_type") == Some("REPOST_FACADE") {
        if let Some(mut original) = embedded(value.get("reposted_post")) {
            let reposter = text_of(value, "username").unwrap_or("unknown");
            original.reposted_by = Some(reposter.to_string());
            return Some(original);
        }
    }
    let own = scoped(NET, id);
    let links = text_of(value, "link_attachment_url")
        .map(|url| Link {
            short: url.into(),
            expanded: url.into(),
            display: display_url(url),
            title: None,
            description: None,
            ..Default::default()
        })
        .into_iter()
        .collect();
    Some(Post {
        id: own.clone(),
        net: NET,
        url: text_of(value, "permalink").map(String::from),
        cid: None,
        author: author_of(value),
        text: text_of(value, "text").unwrap_or_default().to_string(),
        created_at: text_of(value, "timestamp").and_then(timestamp),
        conversation_id: ref_id(value.get("root_post"))
            .map(|root| scoped(NET, root))
            .unwrap_or(own),
        in_reply_to: ref_id(value.get("replied_to")).map(|parent| scoped(NET, parent)),
        quoted: embedded(value.get("quoted_post")).map(Box::new),
        reposted_by: None,
        metrics: Metrics::default(),
        links,
        media: media_of(value),
        liked: false,
        bookmarked: false,
        reposted: false,
    })
}

/** The cursor for the next page. Meta lists `paging.next` only when more follows, but the documented examples omit it, so a full page also counts as more. */
fn next_cursor(value: &Value, returned: usize, limit: usize) -> Option<String> {
    let after = value.pointer("/paging/cursors/after")?.as_str()?;
    let more = value.pointer("/paging/next").is_some() || returned >= limit;
    more.then(|| after.to_string())
}

pub fn page(value: &Value, limit: usize) -> Page {
    let items = value
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    let mut posts: Vec<Post> = Vec::new();
    for item in items {
        if let Some(parsed) = post(item) {
            if !ids.contains(&parsed.id) {
                ids.push(parsed.id.clone());
                posts.push(parsed);
            }
        }
    }
    Page {
        ids,
        posts,
        next: next_cursor(value, items.len(), limit),
    }
}

/** Points the author id of every post (and quote) by `username` at the signed-in account id. */
pub fn claim_own(posts: &mut [Post], username: &str, user_id: &str) {
    let mine = scoped(NET, user_id);
    let owned = |user: &User| user.username.eq_ignore_ascii_case(username);
    for post in posts {
        if owned(&post.author) {
            post.author.id = mine.clone();
        }
        if let Some(quoted) = post.quoted.as_mut().filter(|q| owned(&q.author)) {
            quoted.author.id = mine.clone();
        }
    }
}

pub fn profile(value: &Value) -> Option<User> {
    let id = text_of(value, "id")?;
    let username = text_of(value, "username").unwrap_or(id);
    Some(User {
        id: scoped(NET, id),
        name: text_of(value, "name").unwrap_or(username).to_string(),
        username: username.to_string(),
        avatar_url: text_of(value, "threads_profile_picture_url").map(String::from),
        verified: value
            .get("is_verified")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        business: false,
        description: text_of(value, "threads_biography").map(String::from),
    })
}

// MARK: publishing

#[derive(Debug, PartialEq, Eq)]
pub enum Container {
    Ready,
    Pending,
    Failed(String),
}

pub fn container(value: &Value) -> Container {
    match text_of(value, "status") {
        Some("FINISHED" | "PUBLISHED") => Container::Ready,
        Some(status @ ("ERROR" | "EXPIRED")) => Container::Failed(
            text_of(value, "error_message")
                .unwrap_or(status)
                .to_string(),
        ),
        _ => Container::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    fn fixture(text: &str) -> Value {
        serde_json::from_str(text).expect("fixture is valid json")
    }

    fn first_page() -> Page {
        page(&fixture(include_str!("fixtures/media_page1.json")), 3)
    }

    #[test]
    fn media_list_maps_ids_urls_and_text() {
        let page = first_page();
        assert_eq!(page.ids, ["threads:1001", "threads:1002", "threads:1003"]);
        let first = &page.posts[0];
        assert_eq!(
            first.url.as_deref(),
            Some("https://www.threads.com/@ada/post/AAA1")
        );
        assert_eq!(first.text, "first thread, quoting another");
        assert_eq!(first.author.username, "ada");
        assert_eq!(first.author.id, "threads:42");
        assert_eq!(first.conversation_id, "threads:1001");
        assert!(first.in_reply_to.is_none());
    }

    #[test]
    fn media_list_pages_on_next_and_ends_without_it() {
        assert_eq!(first_page().next.as_deref(), Some("QVFA"));
        let last = page(&fixture(include_str!("fixtures/media_page_last.json")), 25);
        assert_eq!(last.next, None);
    }

    #[test]
    fn a_full_page_without_next_still_offers_a_cursor() {
        let body = fixture(include_str!("fixtures/media_page_last.json"));
        assert_eq!(page(&body, 1).next.as_deref(), Some("QVFA2"));
    }

    #[test]
    fn carousel_children_become_media_and_link_is_kept() {
        let page = first_page();
        let album = &page.posts[1];
        assert_eq!(album.media.len(), 2);
        assert_eq!(album.media[0].kind, MediaKind::Photo);
        assert_eq!(album.media[1].kind, MediaKind::Video);
        assert_eq!(
            album.media[1].preview_url.as_deref(),
            Some("https://cdn.example.com/2.jpg")
        );
        assert_eq!(album.links[0].display, "example.com/read/more");
    }

    #[test]
    fn image_post_keeps_alt_text() {
        let page = first_page();
        assert_eq!(
            page.posts[2].media[0].alt.as_deref(),
            Some("a cat on a keyboard")
        );
        assert_eq!(page.posts[2].text, "");
    }

    #[test]
    fn quote_target_is_a_stub_until_fetched() {
        let page = first_page();
        let quoted = page.posts[0].quoted.as_ref().expect("quote target");
        assert_eq!(quoted.id, "threads:900");
        assert!(is_stub(quoted));
        assert!(!is_stub(&page.posts[0]));
    }

    #[test]
    fn repost_facade_becomes_the_original_marked_reposted_by() {
        let last = page(&fixture(include_str!("fixtures/media_page_last.json")), 25);
        let post = &last.posts[0];
        assert_eq!(post.id, "threads:777");
        assert_eq!(post.reposted_by.as_deref(), Some("ada"));
        assert!(is_stub(post));
    }

    #[test]
    fn conversation_rebuilds_reply_links() {
        let body = fixture(include_str!("fixtures/conversation.json"));
        let replies = page(&body, 100);
        assert_eq!(
            replies.posts[0].in_reply_to.as_deref(),
            Some("threads:1001")
        );
        assert_eq!(
            replies.posts[1].in_reply_to.as_deref(),
            Some("threads:2001")
        );
        assert!(replies
            .posts
            .iter()
            .all(|p| p.conversation_id == "threads:1001"));
        assert_eq!(replies.posts[0].author.id, "threads:bob");
    }

    #[test]
    fn claim_own_points_the_account_posts_at_the_user_id() {
        let body = fixture(include_str!("fixtures/conversation.json"));
        let mut posts = page(&body, 100).posts;
        claim_own(&mut posts, "Ada", "42");
        assert_eq!(posts[0].author.id, "threads:bob");
        assert_eq!(posts[1].author.id, "threads:42");
    }

    #[test]
    fn mentions_map_with_a_timestamp() {
        let body = fixture(include_str!("fixtures/mentions.json"));
        let mentions = page(&body, 25);
        let post = &mentions.posts[0];
        assert_eq!(post.text, "hello @ada");
        assert_eq!(post.author.username, "cy");
        let at = post.created_at.expect("timestamp");
        assert_eq!((at.year(), at.month(), at.day()), (2026, 3, 2));
        assert_eq!((at.hour(), at.minute(), at.second()), (9, 15, 30));
    }

    #[test]
    fn timestamps_parse_meta_offsets_and_rfc3339() {
        let meta = timestamp("2024-01-01T18:20:00+0000").expect("meta format");
        let rfc = timestamp("2024-01-01T18:20:00+00:00").expect("rfc3339");
        assert_eq!(meta, rfc);
        let shifted = timestamp("2024-01-01T20:20:00+0200").expect("offset");
        assert_eq!(shifted, meta);
        assert!(timestamp("yesterday").is_none());
    }

    #[test]
    fn profile_maps_fields() {
        let user = profile(&fixture(
            r#"{"id":"42","username":"ada","name":"Ada L","threads_profile_picture_url":"https://x/p.jpg","threads_biography":"hi","is_verified":true}"#,
        ))
        .expect("profile");
        assert_eq!(user.id, "threads:42");
        assert_eq!(user.name, "Ada L");
        assert!(user.verified);
        assert_eq!(user.description.as_deref(), Some("hi"));
    }

    #[test]
    fn expired_token_asks_to_sign_in() {
        let body = include_str!("fixtures/error_token.json");
        assert!(matches!(
            error_from_body(400, None, body),
            NetError::SignInRequired(Network::Threads)
        ));
    }

    #[test]
    fn rate_limit_codes_map_to_rate_limited() {
        for code in [4, 17, 32, 613] {
            let body = format!(r#"{{"error":{{"message":"slow down","code":{code}}}}}"#);
            assert!(
                matches!(
                    error_from_body(400, None, &body),
                    NetError::RateLimited { .. }
                ),
                "code {code}"
            );
        }
        let body = include_str!("fixtures/error_rate.json");
        assert!(matches!(
            error_from_body(400, None, body),
            NetError::RateLimited { .. }
        ));
    }

    #[test]
    fn http_429_keeps_retry_after() {
        match error_from_body(429, Some(30), "") {
            NetError::RateLimited { retry_after, .. } => assert_eq!(retry_after, Some(30)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn permission_errors_say_what_to_run() {
        let body = include_str!("fixtures/error_permission.json");
        match error_from_body(403, None, body) {
            NetError::Http {
                status, message, ..
            } => {
                assert_eq!(status, 403);
                assert!(message.contains("code 10"));
                assert!(message.contains("socli login threads"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn other_codes_keep_message_and_code() {
        let body = r#"{"error":{"message":"Unsupported get request","code":100}}"#;
        match error_from_body(400, None, body) {
            NetError::Http { message, .. } => {
                assert!(message.contains("Unsupported get request"));
                assert!(message.contains("code 100"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn oauth_endpoint_errors_use_error_message() {
        let body = include_str!("fixtures/error_oauth.json");
        match error_from_body(400, None, body) {
            NetError::Http { message, .. } => {
                assert!(message.contains("Invalid authorization code"))
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn exchange_errors_keep_the_worker_message() {
        let body = r#"{"error":"Invalid verification code"}"#;
        match error_from_body(502, None, body) {
            NetError::Http {
                status, message, ..
            } => {
                assert_eq!(status, 502);
                assert!(message.contains("Invalid verification code"), "{message}");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn non_json_failures_name_the_status() {
        match error_from_body(502, None, "<html>bad gateway</html>") {
            NetError::Http {
                status, message, ..
            } => {
                assert_eq!(status, 502);
                assert_eq!(message, "Threads returned 502");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_a_non_json_success() {
        assert!(matches!(
            decode(200, None, "nope"),
            Err(NetError::Parse { .. })
        ));
        assert!(decode(200, None, "{}").is_ok());
    }

    #[test]
    fn container_status_maps() {
        assert_eq!(
            container(&fixture(r#"{"status":"FINISHED"}"#)),
            Container::Ready
        );
        assert_eq!(
            container(&fixture(r#"{"status":"IN_PROGRESS"}"#)),
            Container::Pending
        );
        assert_eq!(
            container(&fixture(
                r#"{"status":"ERROR","error_message":"bad media"}"#
            )),
            Container::Failed("bad media".into())
        );
    }
}
