//! dev/null (dev-null.sh): public reads, the signed v1 API of an approved device, and commits signed with a GitHub SSH key.

mod api;
#[cfg(feature = "native")]
mod auth;
#[cfg(not(feature = "native"))]
mod auth_web;
#[cfg(feature = "native")]
mod browser;
mod flow;
mod map;
mod sign;
#[cfg(feature = "native")]
mod ssh;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use ssh_key::PrivateKey;

use crate::config::{self, DevnullConfig};
use crate::model::{Metrics, Page, Post, User};
use crate::net::http::{Client, Method, Response};
use crate::net::{http, scoped, split, Caps, Draft, FeedKind, NetError, Network, Provider};

#[cfg(feature = "native")]
pub use auth::{login, logout, setup_ssh};
#[cfg(not(feature = "native"))]
pub use auth_web::{finish, login, logout, prepare, setup_ssh};

use map::{Author, Commit, List, Notification, One, Shown};

/// How many parents a thread walks up before it stops.
const MAX_ANCESTORS: usize = 20;

/// How far the stored keys reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Public,
    /// A GitHub SSH key registered for CLI commits: post and reply only.
    SshKey,
    /// An approved device key: the full API.
    Device,
}

pub struct DevnullClient {
    access: Access,
    username: Option<String>,
    device_key: Option<String>,
    ssh_key: Option<String>,
    http: Option<Client>,
    key: Option<PrivateKey>,
}

impl DevnullClient {
    pub fn new(section: &DevnullConfig) -> Self {
        let access = if section.device_key.is_some() && section.device_approved {
            Access::Device
        } else if section.ssh_key.as_deref().is_some_and(|k| !k.is_empty()) {
            Access::SshKey
        } else {
            Access::Public
        };
        DevnullClient {
            access,
            username: section.username.clone(),
            device_key: section.device_key.clone(),
            ssh_key: section.ssh_key.clone().filter(|k| !k.is_empty()),
            http: None,
            key: None,
        }
    }

    /// No key at all: public threads and profiles.
    pub fn public() -> Self {
        DevnullClient {
            access: Access::Public,
            username: None,
            device_key: None,
            ssh_key: None,
            http: None,
            key: None,
        }
    }

    fn client(&mut self) -> Result<Client, NetError> {
        if let Some(client) = &self.http {
            return Ok(client.clone());
        }
        let client = http::client()?;
        self.http = Some(client.clone());
        Ok(client)
    }

    fn require_device(&self) -> Result<(), NetError> {
        match self.access {
            Access::Device => Ok(()),
            _ => Err(NetError::SignInRequired(Network::Devnull)),
        }
    }

    /** A v1 request, signed whenever the device is approved so reads carry `starred`. */
    fn send(
        &mut self,
        method: Method,
        path: &[&str],
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<Response, NetError> {
        let client = self.client()?;
        if self.access == Access::Device && self.key.is_none() {
            self.key = Some(self.load_key()?);
        }
        let segments: Vec<&str> = ["api", "v1"].iter().chain(path).copied().collect();
        let url = api::url(&segments, query)?;
        api::request(&client, self.key.as_ref(), method, url, body)
    }

    fn load_key(&self) -> Result<PrivateKey, NetError> {
        #[cfg(feature = "native")]
        {
            let file = self.device_key.as_deref().unwrap_or(sign::KEY_FILE);
            sign::read_key(&config::dir().join(file)).map_err(NetError::Other)
        }
        #[cfg(not(feature = "native"))]
        {
            let pem = config::load()
                .map_err(NetError::Other)?
                .devnull
                .device_secret
                .filter(|pem| !pem.is_empty())
                .ok_or_else(|| {
                    NetError::Other("the device key is missing. sign in to dev/null again".into())
                })?;
            sign::from_openssh(&pem).map_err(NetError::Other)
        }
    }

    fn get<T: DeserializeOwned>(
        &mut self,
        path: &[&str],
        query: &[(&str, &str)],
    ) -> Result<T, NetError> {
        let response = self.send(Method::GET, path, query, None)?;
        api::json(response)
    }

    fn list(&mut self, path: &[&str], cursor: Option<&str>) -> Result<Page, NetError> {
        let query: Vec<(&str, &str)> = cursor.map(|c| ("cursor", c)).into_iter().collect();
        let list: List<Commit> = self.get(path, &query)?;
        Ok(map::page(list.data, list.meta.next_cursor))
    }

    /// Unsigned reads carry no `ancestors`, so the chain is fetched one parent at a time, root first.
    fn parents(&mut self, mut parent_id: Option<String>) -> Result<Vec<Commit>, NetError> {
        let mut chain = Vec::new();
        while let Some(parent) = parent_id.take() {
            if chain.len() >= MAX_ANCESTORS {
                break;
            }
            let parent: One<Commit> = match self.get(&["commits", &parent], &[]) {
                Ok(parent) => parent,
                Err(NetError::Http { .. }) => break,
                Err(err) => return Err(err),
            };
            parent_id = parent.data.parent_id.clone();
            chain.push(parent.data);
        }
        chain.reverse();
        Ok(chain)
    }

    fn toggle(&mut self, on: bool, put: Method, path: &[&str]) -> Result<(), NetError> {
        self.require_device()?;
        let method = if on { put } else { Method::DELETE };
        self.send(method, path, &[], None).map(drop)
    }

    fn fetch_me(&mut self) -> Result<User, NetError> {
        self.require_device()?;
        let me: One<Author> = self.get(&["me"], &[])?;
        Ok(map::user(&me.data))
    }

    fn post_signed(&mut self, draft: &Draft) -> Result<Post, NetError> {
        let mut body = json!({ "body": draft.text });
        if let Some(parent) = draft.reply_to.as_deref() {
            body["parent_id"] = json!(native(parent)?);
        }
        if let Some(quoted) = draft.quote.as_deref() {
            body["quoted_commit_id"] = json!(native(quoted)?);
        }
        let created: One<Commit> =
            api::json(self.send(Method::POST, &["commits"], &[], Some(&body))?)?;
        let id = scoped(Network::Devnull, &created.data.id);
        map::page(vec![created.data], None)
            .posts
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| NetError::Other("dev/null did not return the new commit".into()))
    }

    fn post_with_ssh(&mut self, draft: &Draft, key_line: &str) -> Result<Post, NetError> {
        #[cfg(not(feature = "native"))]
        {
            let _ = (draft, key_line);
            return Err(NetError::Other(
                "GitHub SSH signing stays in the terminal. run `socli login devnull`.".into(),
            ));
        }
        #[cfg(feature = "native")]
        {
            if draft.quote.is_some() {
                return Err(NetError::SignInRequired(Network::Devnull));
            }
            let reply_to = draft.reply_to.as_deref().map(native).transpose()?;
            let client = self.client()?;
            let answer = ssh::commit(&client, key_line, reply_to.unwrap_or(""), &draft.text)?;
            let (url, id, username) = ssh::committed(&answer).ok_or_else(|| {
                NetError::Other(format!(
                    "dev/null answered without a commit link: {}",
                    answer.trim()
                ))
            })?;
            if self.username.as_deref() != Some(username.as_str()) {
                let saved = username.clone();
                // The commit is already posted, so a config that cannot be written must not turn it into an error.
                let _ = config::update(|cfg| cfg.devnull.username = Some(saved));
                self.username = Some(username);
            }
            let author = self.account().ok_or_else(|| {
                NetError::Other("dev/null answered without a commit author".into())
            })?;
            let id = scoped(Network::Devnull, &id);
            Ok(Post {
                conversation_id: draft.reply_to.clone().unwrap_or_else(|| id.clone()),
                id,
                net: Network::Devnull,
                url: Some(url),
                cid: None,
                author,
                text: draft.text.trim().to_string(),
                created_at: Some(crate::clock::utc_now()),
                in_reply_to: draft.reply_to.clone(),
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
    }
}

fn native(id: &str) -> Result<&str, NetError> {
    match split(id) {
        Some((Network::Devnull, native)) => Ok(native),
        Some((other, _)) => Err(NetError::Other(format!(
            "{id} belongs to {}, not dev/null",
            other.title()
        ))),
        None => Ok(id),
    }
}

pub fn caps(access: Access) -> Caps {
    match access {
        Access::Public => super::caps::devnull_public(),
        Access::SshKey => super::caps::devnull_ssh(),
        Access::Device => super::caps::devnull_device(),
    }
}

impl Provider for DevnullClient {
    fn network(&self) -> Network {
        Network::Devnull
    }

    fn caps(&self) -> Caps {
        caps(self.access)
    }

    fn signed_in(&self) -> bool {
        self.access != Access::Public
    }

    fn account(&self) -> Option<User> {
        let username = self.username.clone().filter(|_| self.signed_in())?;
        Some(User {
            id: scoped(Network::Devnull, &username),
            name: username.clone(),
            username,
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        })
    }

    fn me(&mut self) -> Result<User, NetError> {
        self.fetch_me()
    }

    fn feed(&mut self, kind: FeedKind, cursor: Option<&str>, _count: u8) -> Result<Page, NetError> {
        let path: &[&str] = match kind {
            FeedKind::Following => &["log", "following"],
            FeedKind::Latest => &["log", "latest"],
            FeedKind::Trending => &["log", "trending"],
            FeedKind::Bookmarks => &["starred"],
            other => {
                return Err(NetError::unsupported(
                    Network::Devnull,
                    format!("{} feed", other.word()),
                ))
            }
        };
        self.require_device()?;
        self.list(path, cursor)
    }

    /// Listing notifications marks every one of them read on dev/null, as the web does.
    fn mentions(&mut self, cursor: Option<&str>, _count: u8) -> Result<Page, NetError> {
        self.require_device()?;
        let mut query = vec![("type", "replies,mentions,quotes")];
        query.extend(cursor.map(|c| ("cursor", c)));
        let list: List<Notification> = self.get(&["notifications"], &query)?;
        let commits = list.data.into_iter().filter_map(|n| n.commit).collect();
        Ok(map::page(commits, list.meta.next_cursor))
    }

    fn thread(&mut self, id: &str, _count: u8) -> Result<Page, NetError> {
        let target_id = native(id)?.to_string();
        let shown: Shown = self.get(&["commits", &target_id], &[])?;
        let mut chain = match self.access {
            Access::Device => shown.ancestors,
            _ => self.parents(shown.data.parent_id.clone())?,
        };
        chain.push(shown.data);
        if self.access == Access::Device {
            let replies: List<Commit> = self.get(&["commits", &target_id, "replies"], &[])?;
            chain.extend(replies.data);
        }
        Ok(map::page(chain, None))
    }

    fn user(
        &mut self,
        handle: &str,
        cursor: Option<&str>,
        _count: u8,
    ) -> Result<(User, Page), NetError> {
        let username = handle.trim().trim_start_matches('@').to_string();
        let profile: One<Author> = self.get(&["users", &username], &[])?;
        let page = self.list(&["users", &username, "commits"], cursor)?;
        Ok((map::user(&profile.data), page))
    }

    fn search(&mut self, _: &str, _: Option<&str>, _: u8) -> Result<Page, NetError> {
        Err(NetError::unsupported(Network::Devnull, "search"))
    }

    fn post(&mut self, draft: &Draft) -> Result<Post, NetError> {
        match (self.access, self.ssh_key.clone()) {
            (Access::Device, _) => self.post_signed(draft),
            (_, Some(key_line)) => self.post_with_ssh(draft, &key_line),
            _ => Err(NetError::SignInRequired(Network::Devnull)),
        }
    }

    fn delete(&mut self, id: &str) -> Result<(), NetError> {
        self.require_device()?;
        let id = native(id)?.to_string();
        self.send(Method::DELETE, &["commits", &id], &[], None)
            .map(drop)
    }

    fn set_like(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        let id = native(id)?.to_string();
        self.toggle(on, Method::POST, &["commits", &id, "star"])
    }

    fn set_repost(&mut self, _: &str, _: bool) -> Result<(), NetError> {
        Err(NetError::unsupported(Network::Devnull, "reposts"))
    }

    fn set_bookmark(&mut self, _: &str, _: bool) -> Result<(), NetError> {
        Err(NetError::unsupported(Network::Devnull, "bookmarks"))
    }

    fn set_follow(&mut self, handle: &str, on: bool) -> Result<(), NetError> {
        let username = handle.trim().trim_start_matches('@').to_string();
        self.toggle(on, Method::POST, &["users", &username, "follow"])
    }
}

/// The `devnull` rows of `socli auth status`.
pub fn status(section: &DevnullConfig) -> Vec<(&'static str, String)> {
    let account = match (&section.username, DevnullClient::new(section).access) {
        (_, Access::Public) => "not signed in".to_string(),
        (Some(user), _) => format!("@{user}"),
        (None, _) => "signed in".into(),
    };
    let device = match (&section.device_key, section.device_approved) {
        (Some(file), true) => format!("approved ({file})"),
        (Some(file), false) => format!("waiting for approval ({file})"),
        (None, _) => "missing".into(),
    };
    vec![
        ("account", account),
        ("device key", device),
        ("ssh key", config::redact(section.ssh_key.as_deref())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(device: bool, ssh: bool) -> DevnullConfig {
        DevnullConfig {
            username: Some("nuno".into()),
            device_key: device.then(|| sign::KEY_FILE.to_string()),
            device_secret: None,
            device_approved: device,
            ssh_key: ssh.then(|| "ssh-ed25519 AAAAC3Nza me".to_string()),
        }
    }

    #[test]
    fn access_follows_the_stored_keys() {
        assert_eq!(
            DevnullClient::new(&section(true, true)).access,
            Access::Device
        );
        assert_eq!(
            DevnullClient::new(&section(false, true)).access,
            Access::SshKey
        );
        assert_eq!(
            DevnullClient::new(&section(false, false)).access,
            Access::Public
        );
        let waiting = DevnullConfig {
            device_approved: false,
            ..section(true, false)
        };
        assert_eq!(DevnullClient::new(&waiting).access, Access::Public);
        assert_eq!(DevnullClient::public().account(), None);
        assert_eq!(
            DevnullClient::new(&section(false, true))
                .account()
                .map(|u| u.username),
            Some("nuno".into())
        );
    }

    #[test]
    fn caps_follow_the_access_state() {
        let device = caps(Access::Device);
        assert_eq!(device.primary, Some(FeedKind::Following));
        assert_eq!(device.feeds.len(), 4);
        assert!(device.like && device.quote && device.follow && device.delete);
        assert!(!device.search && !device.repost && !device.bookmark);
        assert!(device.feeds.contains(&(FeedKind::Bookmarks, "Starred")));
        assert_eq!(device.like_verb, "star");
        assert_eq!(device.max_chars, 4000);

        let ssh = caps(Access::SshKey);
        assert!(ssh.post && ssh.thread && ssh.user);
        assert!(!ssh.like && !ssh.quote && !ssh.mentions && ssh.feeds.is_empty());

        let public = caps(Access::Public);
        assert!(!public.post && public.thread && public.user);
    }

    #[test]
    fn actions_outside_the_access_ask_to_sign_in_and_missing_ones_are_unsupported() {
        let mut ssh = DevnullClient::new(&section(false, true));
        let quote = Draft {
            text: "hi".into(),
            reply_to: None,
            quote: Some("devnull:abc".into()),
        };
        assert!(matches!(
            ssh.post(&quote),
            Err(NetError::SignInRequired(Network::Devnull))
        ));
        assert!(matches!(
            ssh.set_like("devnull:abc", true),
            Err(NetError::SignInRequired(_))
        ));
        assert!(matches!(
            ssh.feed(FeedKind::Latest, None, 20),
            Err(NetError::SignInRequired(_))
        ));
        assert!(matches!(
            ssh.mentions(None, 20),
            Err(NetError::SignInRequired(_))
        ));
        assert!(matches!(
            ssh.search("rust", None, 20),
            Err(NetError::Unsupported { .. })
        ));
        assert!(matches!(
            ssh.set_repost("devnull:abc", true),
            Err(NetError::Unsupported { .. })
        ));
        let mut public = DevnullClient::public();
        assert!(matches!(
            public.post(&Draft::default()),
            Err(NetError::SignInRequired(_))
        ));
        assert!(matches!(
            public.feed(FeedKind::ForYou, None, 20),
            Err(NetError::Unsupported { .. })
        ));
    }

    #[test]
    fn ids_from_other_networks_are_refused() {
        assert_eq!(native("devnull:abc").unwrap(), "abc");
        assert_eq!(native("abc").unwrap(), "abc");
        assert!(native("x:123")
            .unwrap_err()
            .to_string()
            .contains("x:123 belongs to X, not dev/null"));
    }
}
