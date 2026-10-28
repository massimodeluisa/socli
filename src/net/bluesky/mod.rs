//! Bluesky (AT Protocol) behind the provider trait: app-password sessions over XRPC.

mod auth;
mod facets;
mod map;
mod rpc;

pub use auth::{login, login_with_password, logout};

use chrono::SecondsFormat;

use crate::net::http::Client;
use serde_json::{json, Value};

use self::rpc::Call;
use crate::config::{self, BlueskyConfig};
use crate::model::{Metrics, Page, Post, User};
use crate::net::{http, scoped, split, Caps, Draft, FeedKind, NetError, Network, Provider};

pub const DEFAULT_SERVICE: &str = "https://bsky.social";

const PUBLIC_BASE: &str = "https://public.api.bsky.app";
const DISCOVER_FEED: &str =
    "at://did:plc:z72i7hdynmk6r22z27h6tvur/app.bsky.feed.generator/whats-hot";
const POST: &str = "app.bsky.feed.post";
const LIKE: &str = "app.bsky.feed.like";
const REPOST: &str = "app.bsky.feed.repost";
const FOLLOW: &str = "app.bsky.graph.follow";

/// A signed-in app-password session from the `[bluesky]` section.
#[derive(Clone)]
struct Session {
    handle: String,
    did: String,
    service: String,
    access_jwt: String,
    refresh_jwt: Option<String>,
}

pub struct BlueskyClient {
    session: Option<Session>,
    http: Option<Client>,
}

impl BlueskyClient {
    pub fn new(section: &BlueskyConfig) -> Self {
        let session = match (&section.handle, &section.did, &section.access_jwt) {
            (Some(handle), Some(did), Some(jwt)) if !jwt.is_empty() => Some(Session {
                handle: handle.clone(),
                did: did.clone(),
                service: section
                    .service
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(DEFAULT_SERVICE)
                    .trim_end_matches('/')
                    .to_string(),
                access_jwt: jwt.clone(),
                refresh_jwt: section.refresh_jwt.clone().filter(|t| !t.is_empty()),
            }),
            _ => None,
        };
        BlueskyClient {
            session,
            http: None,
        }
    }

    /// Signed out: public threads and profiles through the public AppView.
    pub fn public() -> Self {
        BlueskyClient {
            session: None,
            http: None,
        }
    }

    fn session(&self) -> Result<&Session, NetError> {
        self.session
            .as_ref()
            .ok_or(NetError::SignInRequired(Network::Bluesky))
    }

    fn http(&mut self) -> Result<Client, NetError> {
        if let Some(client) = &self.http {
            return Ok(client.clone());
        }
        let client = http::client()?;
        self.http = Some(client.clone());
        Ok(client)
    }

    // MARK: transport

    fn xrpc(&mut self, nsid: &str, call: &Call<'_>) -> Result<Value, NetError> {
        let client = self.http()?;
        let base = self
            .session
            .as_ref()
            .map_or(PUBLIC_BASE, |s| s.service.as_str());
        let url = format!("{base}/xrpc/{nsid}");
        let token = self.session.as_ref().map(|s| s.access_jwt.clone());
        let mut raw = rpc::send(&client, &url, call, token.as_deref())?;
        if self.session.is_some() && rpc::should_refresh(raw.status, &raw.body) {
            self.refresh(&client)?;
            let token = self.session.as_ref().map(|s| s.access_jwt.clone());
            raw = rpc::send(&client, &url, call, token.as_deref())?;
        }
        let result = rpc::finish(raw);
        if matches!(call, Call::Write(_)) {
            result.map_err(|error| match error {
                NetError::Parse { .. }
                | NetError::Http {
                    status: 500..=599, ..
                } => NetError::OutcomeUnknown {
                    net: Network::Bluesky,
                    message: error.to_string(),
                },
                other => other,
            })
        } else {
            result
        }
    }

    fn get(&mut self, nsid: &str, query: &[(&str, String)]) -> Result<Value, NetError> {
        self.xrpc(nsid, &Call::Get(query))
    }

    fn send_json(&mut self, nsid: &str, body: &Value) -> Result<Value, NetError> {
        self.xrpc(nsid, &Call::Write(body))
    }

    /// Trades the refresh token for a new pair, saves it, and keeps it in memory.
    fn refresh(&mut self, client: &Client) -> Result<(), NetError> {
        let signed_out = || NetError::SignInRequired(Network::Bluesky);
        let session = self.session.as_ref().ok_or_else(signed_out)?;
        let refresh_jwt = session.refresh_jwt.clone().ok_or_else(signed_out)?;
        let url = format!("{}/xrpc/com.atproto.server.refreshSession", session.service);
        let raw = rpc::send(client, &url, &Call::Post(None), Some(&refresh_jwt))?;
        let (access, refresh) = rpc::refreshed(raw)?;
        let mut saved = false;
        config::update(|cfg| {
            saved = save_refresh(&mut cfg.bluesky, session, &access, &refresh);
        })
        .map_err(|e| NetError::Other(format!("could not save the refreshed session: {e}")))?;
        if !saved {
            return Err(NetError::Other(
                "the Bluesky sign-in changed while refreshing. run the command again".into(),
            ));
        }
        if let Some(session) = self.session.as_mut() {
            session.access_jwt = access;
            session.refresh_jwt = Some(refresh);
        }
        Ok(())
    }

    // MARK: lookups

    fn did_for(&mut self, actor: &str) -> Result<String, NetError> {
        if actor.starts_with("did:") {
            return Ok(actor.to_string());
        }
        let value = self.get(
            "com.atproto.identity.resolveHandle",
            &[("handle", actor.to_string())],
        )?;
        value
            .get("did")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| NetError::Other(format!("Bluesky could not resolve @{actor}")))
    }

    /// The uri with a DID authority, since a link may carry a handle.
    fn canonical_uri(&mut self, uri: &str) -> Result<String, NetError> {
        match map::parse_uri(uri) {
            Some((authority, collection, rkey)) if !authority.starts_with("did:") => {
                let did = self.did_for(authority)?;
                Ok(format!("at://{did}/{collection}/{rkey}"))
            }
            _ => Ok(uri.to_string()),
        }
    }

    fn posts(&mut self, uris: &[String]) -> Result<Vec<Value>, NetError> {
        let query: Vec<(&str, String)> = uris.iter().map(|u| ("uris", u.clone())).collect();
        let value = self.get("app.bsky.feed.getPosts", &query)?;
        Ok(value
            .get("posts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    fn post_view(&mut self, uri: &str) -> Result<Value, NetError> {
        let uri = self.canonical_uri(uri)?;
        self.posts(&[uri])?
            .into_iter()
            .next()
            .ok_or_else(|| NetError::Other("Bluesky could not find that post".into()))
    }

    fn profile(&mut self, actor: &str) -> Result<Value, NetError> {
        self.get(
            "app.bsky.actor.getProfile",
            &[("actor", actor.trim_start_matches('@').to_string())],
        )
    }

    fn create_record(&mut self, collection: &str, mut record: Value) -> Result<Value, NetError> {
        let repo = self.session()?.did.clone();
        record["$type"] = json!(collection);
        let body = json!({"repo": repo, "collection": collection, "record": record});
        let created = self.send_json("com.atproto.repo.createRecord", &body)?;
        if created.get("uri").and_then(Value::as_str).is_none()
            || created.get("cid").and_then(Value::as_str).is_none()
        {
            return Err(NetError::OutcomeUnknown {
                net: Network::Bluesky,
                message: "Bluesky did not confirm the new record".into(),
            });
        }
        Ok(created)
    }

    fn delete_record(&mut self, collection: &str, rkey: &str) -> Result<(), NetError> {
        let repo = self.session()?.did.clone();
        let body = json!({"repo": repo, "collection": collection, "rkey": rkey});
        self.send_json("com.atproto.repo.deleteRecord", &body)
            .map(|_| ())
    }

    /// Likes and reposts: create a record on, delete the viewer's record off.
    fn toggle(
        &mut self,
        id: &str,
        on: bool,
        collection: &str,
        viewer_key: &str,
    ) -> Result<(), NetError> {
        self.session()?;
        let view = self.post_view(native(id)?)?;
        let existing = view
            .pointer(&format!("/viewer/{viewer_key}"))
            .and_then(Value::as_str)
            .map(str::to_string);
        match (on, existing) {
            (true, None) => {
                let record = json!({"subject": strong_ref(&view)?, "createdAt": now()});
                self.create_record(collection, record).map(|_| ())
            }
            (false, Some(uri)) => self.delete_record(collection, map::rkey(&uri)),
            _ => Ok(()),
        }
    }

    fn feed_get(
        &mut self,
        nsid: &str,
        mut query: Vec<(&str, String)>,
        cursor: Option<&str>,
        count: u8,
    ) -> Result<Value, NetError> {
        query.push(("limit", count.clamp(1, 100).to_string()));
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor.to_string()));
        }
        self.get(nsid, &query)
    }
}

fn cursor_of(value: &Value) -> Option<String> {
    value
        .get("cursor")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_string)
}

fn save_refresh(
    section: &mut BlueskyConfig,
    session: &Session,
    access: &str,
    refresh: &str,
) -> bool {
    if section.did.as_deref() != Some(session.did.as_str())
        || section.refresh_jwt != session.refresh_jwt
        || section
            .service
            .as_deref()
            .unwrap_or(DEFAULT_SERVICE)
            .trim_end_matches('/')
            != session.service
    {
        return false;
    }
    section.access_jwt = Some(access.to_string());
    section.refresh_jwt = Some(refresh.to_string());
    true
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn now() -> String {
    crate::clock::utc_now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn native(id: &str) -> Result<&str, NetError> {
    match split(id) {
        Some((Network::Bluesky, native)) => Ok(native),
        Some((other, _)) => Err(NetError::Other(format!(
            "{id} belongs to {}, not Bluesky",
            other.title()
        ))),
        None => Ok(id),
    }
}

fn strong_ref(view: &Value) -> Result<Value, NetError> {
    match (
        view.get("uri").and_then(Value::as_str),
        view.get("cid").and_then(Value::as_str),
    ) {
        (Some(uri), Some(cid)) => Ok(json!({"uri": uri, "cid": cid})),
        _ => Err(NetError::Other(
            "Bluesky returned a post without a uri or cid".into(),
        )),
    }
}

/// `{root, parent}` for a reply: the parent's own root when it is itself a reply, else the parent.
fn reply_refs(parent_view: &Value) -> Result<Value, NetError> {
    let parent = strong_ref(parent_view)?;
    let root = parent_view
        .pointer("/record/reply/root")
        .filter(|r| r.get("uri").is_some() && r.get("cid").is_some())
        .map(|r| json!({"uri": r["uri"], "cid": r["cid"]}))
        .unwrap_or_else(|| parent.clone());
    Ok(json!({"root": root, "parent": parent}))
}

fn post_record(
    text: &str,
    created_at: &str,
    facets: Vec<Value>,
    reply: Option<Value>,
    quote: Option<Value>,
) -> Value {
    let mut record = json!({"$type": POST, "text": text, "createdAt": created_at});
    if !facets.is_empty() {
        record["facets"] = Value::Array(facets);
    }
    if let Some(reply) = reply {
        record["reply"] = reply;
    }
    if let Some(quote) = quote {
        record["embed"] = json!({"$type": "app.bsky.embed.record", "record": quote});
    }
    record
}

/// Bluesky's capabilities with an app-password session, or signed out.
pub fn caps(session: bool) -> Caps {
    super::caps::bluesky(session)
}

impl Provider for BlueskyClient {
    fn network(&self) -> Network {
        Network::Bluesky
    }

    fn caps(&self) -> Caps {
        caps(self.session.is_some())
    }

    fn signed_in(&self) -> bool {
        self.session.is_some()
    }

    fn account(&self) -> Option<User> {
        self.session.as_ref().map(|s| User {
            id: scoped(Network::Bluesky, &s.did),
            name: s.handle.clone(),
            username: s.handle.clone(),
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        })
    }

    fn me(&mut self) -> Result<User, NetError> {
        let did = self.session()?.did.clone();
        let profile = self.profile(&did)?;
        map::user(&profile)
            .ok_or_else(|| NetError::Other("Bluesky returned an empty profile".into()))
    }

    fn feed(&mut self, kind: FeedKind, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        match kind {
            FeedKind::Following => {
                self.session()?;
                let value =
                    self.feed_get("app.bsky.feed.getTimeline", Vec::new(), cursor, count)?;
                Ok(map::feed_page(items(&value, "feed"), cursor_of(&value)))
            }
            FeedKind::Discover => {
                self.session()?;
                let query = vec![("feed", DISCOVER_FEED.to_string())];
                let value = self.feed_get("app.bsky.feed.getFeed", query, cursor, count)?;
                Ok(map::feed_page(items(&value, "feed"), cursor_of(&value)))
            }
            FeedKind::Bookmarks => {
                self.session()?;
                let value =
                    self.feed_get("app.bsky.bookmark.getBookmarks", Vec::new(), cursor, count)?;
                Ok(map::bookmarks_page(
                    items(&value, "bookmarks"),
                    cursor_of(&value),
                ))
            }
            other => Err(NetError::unsupported(
                Network::Bluesky,
                format!("{} feed", other.word()),
            )),
        }
    }

    fn mentions(&mut self, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.session()?;
        let query = ["mention", "reply", "quote"]
            .iter()
            .map(|reason| ("reasons", reason.to_string()))
            .collect();
        let value = self.feed_get(
            "app.bsky.notification.listNotifications",
            query,
            cursor,
            count,
        )?;
        let uris = map::notification_uris(&value);
        let views = map::hydrate(&uris, |batch| self.posts(batch))?;
        Ok(map::posts_page(&views, cursor_of(&value)))
    }

    fn thread(&mut self, id: &str, _count: u8) -> Result<Page, NetError> {
        let requested = native(id)?;
        let uri = self.canonical_uri(requested)?;
        let value = self.get(
            "app.bsky.feed.getPostThread",
            &[
                ("uri", uri.clone()),
                ("depth", "10".into()),
                ("parentHeight", "80".into()),
            ],
        )?;
        let mut page =
            map::thread_page(value.get("thread").unwrap_or(&Value::Null)).ok_or_else(|| {
                NetError::Other("Bluesky could not find that post, or it is blocked".into())
            })?;
        if uri != requested {
            map::rename(
                &mut page,
                &scoped(Network::Bluesky, &uri),
                &scoped(Network::Bluesky, requested),
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
        let actor = handle.trim().trim_start_matches('@').to_string();
        let profile = self.profile(&actor)?;
        let user = map::user(&profile)
            .ok_or_else(|| NetError::Other(format!("Bluesky has no @{actor}")))?;
        let query = vec![
            ("actor", actor),
            ("filter", "posts_and_author_threads".to_string()),
        ];
        let value = self.feed_get("app.bsky.feed.getAuthorFeed", query, cursor, count)?;
        Ok((
            user,
            map::feed_page(items(&value, "feed"), cursor_of(&value)),
        ))
    }

    fn search(&mut self, query: &str, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.session()?;
        let params = vec![("q", query.to_string()), ("sort", "latest".to_string())];
        let value = self.feed_get("app.bsky.feed.searchPosts", params, cursor, count)?;
        Ok(map::posts_page(items(&value, "posts"), cursor_of(&value)))
    }

    fn post(&mut self, draft: &Draft) -> Result<Post, NetError> {
        let author = self
            .account()
            .ok_or(NetError::SignInRequired(Network::Bluesky))?;
        facets::check_length(&draft.text)?;
        let found = facets::build(&draft.text, |handle| self.did_for(handle).ok());
        let reply = match draft.reply_to.as_deref() {
            Some(id) => Some(reply_refs(&self.post_view(native(id)?)?)?),
            None => None,
        };
        let quote = match draft.quote.as_deref() {
            Some(id) => Some(strong_ref(&self.post_view(native(id)?)?)?),
            None => None,
        };
        let created_at = now();
        let in_reply_to = reply
            .as_ref()
            .and_then(|r| r.pointer("/parent/uri"))
            .and_then(Value::as_str)
            .map(|uri| scoped(Network::Bluesky, uri));
        let conversation = reply
            .as_ref()
            .and_then(|r| r.pointer("/root/uri"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let record = post_record(&draft.text, &created_at, found, reply, quote);
        let created = self.create_record(POST, record)?;
        let (Some(uri), Some(cid)) = (
            created.get("uri").and_then(Value::as_str),
            created.get("cid").and_then(Value::as_str),
        ) else {
            return Err(NetError::Other(
                "Bluesky did not return the new post".into(),
            ));
        };
        let id = scoped(Network::Bluesky, uri);
        Ok(Post {
            url: Some(map::post_url(&author.username, uri)),
            cid: Some(cid.to_string()),
            conversation_id: conversation
                .map_or_else(|| id.clone(), |root| scoped(Network::Bluesky, &root)),
            id,
            net: Network::Bluesky,
            author,
            text: draft.text.clone(),
            created_at: Some(crate::clock::utc_now()),
            in_reply_to,
            quoted: None,
            reposted_by: None,
            metrics: Metrics::default(),
            links: Vec::new(),
            media: Vec::new(),
            liked: false,
            bookmarked: false,
            reposted: false,
        })
    }

    fn delete(&mut self, id: &str) -> Result<(), NetError> {
        let session = self.session()?;
        let (did, handle) = (session.did.clone(), session.handle.clone());
        let uri = native(id)?;
        match map::parse_uri(uri) {
            Some((authority, POST, rkey)) if authority == did || authority == handle => {
                self.delete_record(POST, rkey)
            }
            _ => Err(NetError::Other(
                "Bluesky can only delete your own posts".into(),
            )),
        }
    }

    fn set_like(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.toggle(id, on, LIKE, "like")
    }

    fn set_repost(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.toggle(id, on, REPOST, "repost")
    }

    fn set_bookmark(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.session()?;
        let uri = self.canonical_uri(native(id)?)?;
        if on {
            let body = strong_ref(&self.post_view(&uri)?)?;
            self.send_json("app.bsky.bookmark.createBookmark", &body)
                .map(|_| ())
        } else {
            self.send_json("app.bsky.bookmark.deleteBookmark", &json!({"uri": uri}))
                .map(|_| ())
        }
    }

    fn set_follow(&mut self, handle: &str, on: bool) -> Result<(), NetError> {
        self.session()?;
        let profile = self.profile(handle)?;
        let did = profile
            .get("did")
            .and_then(Value::as_str)
            .ok_or_else(|| NetError::Other(format!("Bluesky has no {handle}")))?
            .to_string();
        let following = profile
            .pointer("/viewer/following")
            .and_then(Value::as_str)
            .map(str::to_string);
        match (on, following) {
            (true, None) => self
                .create_record(FOLLOW, json!({"subject": did, "createdAt": now()}))
                .map(|_| ()),
            (false, Some(uri)) => self.delete_record(FOLLOW, map::rkey(&uri)),
            _ => Ok(()),
        }
    }
}

/// The `bluesky` rows of `socli auth status`, secrets redacted.
pub fn status(section: &BlueskyConfig) -> Vec<(&'static str, String)> {
    let account = match (&section.handle, &section.access_jwt) {
        (Some(handle), Some(_)) => format!("@{handle}"),
        _ => "not signed in".into(),
    };
    vec![
        ("account", account),
        ("session", config::redact(section.access_jwt.as_deref())),
        (
            "service",
            section
                .service
                .clone()
                .unwrap_or_else(|| DEFAULT_SERVICE.into()),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_expired_write_keeps_the_refresh_rate_limit_and_is_not_replayed() {
        use std::io::{BufRead, Read, Write};
        use std::time::{Duration, Instant};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let service = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in [
                ("400 Bad Request", r#"{"error":"ExpiredToken"}"#),
                ("429 Too Many Requests", r#"{"message":"try later"}"#),
            ] {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(2))
                        }
                        other => panic!("missing expected request: {other:?}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(&mut stream);
                let mut request = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                    request.push_str(&line);
                }
                let mut bytes = vec![0; length];
                reader.read_exact(&mut bytes).unwrap();
                request.push_str(&String::from_utf8(bytes).unwrap());
                requests.push(request);
                write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nRetry-After: 30\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let mut client = BlueskyClient::new(&BlueskyConfig {
            service: Some(service),
            handle: Some("ada.test".into()),
            did: Some("did:plc:ada".into()),
            access_jwt: Some("old-access".into()),
            refresh_jwt: Some("refresh".into()),
        });
        client.http = Some(
            Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
        );
        let result = client.post(&Draft {
            text: "hello".into(),
            ..Draft::default()
        });
        assert!(matches!(
            result,
            Err(NetError::RateLimited {
                net: Network::Bluesky,
                retry_after: Some(30)
            })
        ));
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("POST /xrpc/com.atproto.repo.createRecord "));
        assert!(requests[0].contains(r#""$type":"app.bsky.feed.post""#));
        assert!(requests[1].starts_with("POST /xrpc/com.atproto.server.refreshSession "));
        assert!(requests[1].contains("Bearer refresh"));
        assert_eq!(client.session().unwrap().access_jwt, "old-access");
    }

    #[test]
    fn a_refresh_cannot_restore_a_logged_out_or_replaced_account() {
        let original = BlueskyConfig {
            did: Some("did:plc:ada".into()),
            handle: Some("ada.test".into()),
            access_jwt: Some("old-access".into()),
            refresh_jwt: Some("old-refresh".into()),
            ..BlueskyConfig::default()
        };
        let client = BlueskyClient::new(&original);
        let session = client.session().unwrap();
        let mut current = original.clone();
        assert!(save_refresh(
            &mut current,
            session,
            "new-access",
            "new-refresh"
        ));
        assert_eq!(current.refresh_jwt.as_deref(), Some("new-refresh"));
        assert!(!save_refresh(
            &mut current,
            session,
            "stale-access",
            "stale-refresh"
        ));
        assert_eq!(current.access_jwt.as_deref(), Some("new-access"));
        let mut logged_out = BlueskyConfig::default();
        assert!(!save_refresh(
            &mut logged_out,
            session,
            "new-access",
            "new-refresh"
        ));
        assert!(logged_out.access_jwt.is_none());
        let mut switched = BlueskyConfig {
            did: Some("did:plc:other".into()),
            ..original
        };
        assert!(!save_refresh(
            &mut switched,
            session,
            "new-access",
            "new-refresh"
        ));
        assert_eq!(switched.did.as_deref(), Some("did:plc:other"));
    }

    #[test]
    fn caps_follow_the_session_and_public_columns() {
        let signed = caps(true);
        assert_eq!(
            signed.feeds,
            vec![
                (FeedKind::Following, "Following"),
                (FeedKind::Discover, "Discover"),
                (FeedKind::Bookmarks, "Bookmarks"),
            ]
        );
        assert_eq!(signed.primary, Some(FeedKind::Following));
        assert!(signed.mentions && signed.search && signed.like && signed.repost);
        assert!(signed.quote && signed.bookmark && signed.follow && signed.delete);
        assert!(signed.post && signed.thread && signed.user && signed.counts);
        assert!(!signed.views);
        assert_eq!((signed.max_chars, signed.count_graphemes), (300, true));

        let public = caps(false);
        assert!(public.feeds.is_empty() && public.primary.is_none());
        assert!(public.thread && public.user && public.counts);
        assert!(!public.mentions && !public.search && !public.like && !public.repost);
        assert!(!public.quote && !public.bookmark && !public.follow);
        assert!(!public.delete && !public.post && !public.views);
    }

    #[test]
    fn a_session_needs_a_handle_a_did_and_a_token() {
        let full = BlueskyConfig {
            service: Some("https://pds.test/".into()),
            handle: Some("ada.test".into()),
            did: Some("did:plc:ada".into()),
            access_jwt: Some("a".into()),
            refresh_jwt: Some("r".into()),
        };
        let client = BlueskyClient::new(&full);
        assert!(client.signed_in());
        assert_eq!(
            client.session.as_ref().map(|s| s.service.as_str()),
            Some("https://pds.test")
        );
        assert_eq!(
            client.account().map(|u| u.id),
            Some("bluesky:did:plc:ada".to_string())
        );
        let missing = BlueskyConfig {
            access_jwt: None,
            ..full
        };
        assert!(!BlueskyClient::new(&missing).signed_in());
        assert!(!BlueskyClient::public().signed_in());
    }

    #[test]
    fn a_reply_to_a_root_post_uses_it_as_root_and_parent() {
        let parent = json!({"uri": "at://did:plc:a/app.bsky.feed.post/1", "cid": "c1", "record": {"text": "x"}});
        let refs = reply_refs(&parent).expect("refs");
        assert_eq!(refs["root"], refs["parent"]);
        assert_eq!(refs["parent"]["cid"], "c1");
    }

    #[test]
    fn a_reply_to_a_reply_keeps_the_thread_root() {
        let parent = json!({
            "uri": "at://did:plc:a/app.bsky.feed.post/2", "cid": "c2",
            "record": {"reply": {
                "root": {"uri": "at://did:plc:a/app.bsky.feed.post/1", "cid": "c1"},
                "parent": {"uri": "at://did:plc:a/app.bsky.feed.post/1", "cid": "c1"},
            }},
        });
        let refs = reply_refs(&parent).expect("refs");
        assert_eq!(refs["root"]["uri"], "at://did:plc:a/app.bsky.feed.post/1");
        assert_eq!(refs["root"]["cid"], "c1");
        assert_eq!(refs["parent"]["uri"], "at://did:plc:a/app.bsky.feed.post/2");
    }

    #[test]
    fn the_post_record_carries_facets_reply_and_quote_only_when_present() {
        let plain = post_record("hi", "2026-01-01T00:00:00.000Z", Vec::new(), None, None);
        assert_eq!(plain["$type"], "app.bsky.feed.post");
        assert_eq!(plain["createdAt"], "2026-01-01T00:00:00.000Z");
        assert!(
            plain.get("facets").is_none()
                && plain.get("reply").is_none()
                && plain.get("embed").is_none()
        );
        let full = post_record(
            "hi #tag",
            "2026-01-01T00:00:00.000Z",
            facets::build("hi #tag", |_| None),
            Some(json!({"root": {}, "parent": {}})),
            Some(json!({"uri": "u", "cid": "c"})),
        );
        assert_eq!(full["facets"][0]["features"][0]["tag"], "tag");
        assert!(full["reply"].is_object());
        assert_eq!(full["embed"]["$type"], "app.bsky.embed.record");
        assert_eq!(full["embed"]["record"]["cid"], "c");
    }

    #[test]
    fn the_timestamp_has_milliseconds_and_a_z() {
        let stamp = now();
        assert!(stamp.ends_with('Z'));
        assert_eq!(stamp.len(), "2026-01-01T00:00:00.000Z".len());
    }

    #[test]
    fn an_over_long_post_fails_before_any_request() {
        let mut client = BlueskyClient::new(&BlueskyConfig {
            handle: Some("ada.test".into()),
            did: Some("did:plc:ada".into()),
            access_jwt: Some("a".into()),
            ..BlueskyConfig::default()
        });
        let draft = Draft {
            text: "a".repeat(301),
            ..Draft::default()
        };
        let err = client.post(&draft).expect_err("too long");
        assert_eq!(
            err.to_string(),
            "Bluesky allows 300 characters; this post has 301"
        );
    }

    #[test]
    fn a_public_client_asks_for_sign_in() {
        let mut client = BlueskyClient::public();
        assert!(matches!(
            client.feed(FeedKind::Following, None, 10),
            Err(NetError::SignInRequired(_))
        ));
        assert!(matches!(
            client.post(&Draft::default()),
            Err(NetError::SignInRequired(_))
        ));
        assert!(matches!(
            client.search("x", None, 10),
            Err(NetError::SignInRequired(_))
        ));
        assert!(matches!(
            client.feed(FeedKind::Trending, None, 10),
            Err(NetError::Unsupported { .. })
        ));
    }

    #[test]
    fn ids_from_another_network_are_refused() {
        assert_eq!(
            native("bluesky:at://did:plc:a/app.bsky.feed.post/1").ok(),
            Some("at://did:plc:a/app.bsky.feed.post/1")
        );
        assert!(native("x:123").is_err());
    }
}
