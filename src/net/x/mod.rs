//! X behind the provider trait. The client speaks native ids; this layer scopes them.

pub mod auth;
mod client;
mod cookies;
mod transaction;
mod web;

pub use client::XClient;

use client::ApiError;

use super::{scoped, split, Caps, Draft, FeedKind, NetError, Network, Provider};
use crate::model::{Metrics, Page, Post, User};

pub use super::caps::NO_TRENDING;
/// Official API quotes are text plus the quoted post's link. A browser session quotes through the web client.
pub const QUOTE_NEEDS_LINK: &str =
    "can't quote this post on X: its author is unknown. Open it and press s to copy the link";

/// X's capabilities: everything with a user token; public reads with an app bearer alone.
pub fn caps(user: bool) -> Caps {
    super::caps::x(user)
}

impl XClient {
    /// Runs one call, refreshing the user token once when X answers 401.
    fn call<T>(&mut self, f: impl Fn(&mut XClient) -> Result<T, ApiError>) -> Result<T, NetError> {
        match f(self) {
            Ok(value) => Ok(value),
            Err(ApiError::Failed { status: 401, .. })
                if !self.credentials().uses_web() && self.credentials().refresh_token.is_some() =>
            {
                auth::refresh(self.credentials_mut()).map_err(NetError::Other)?;
                f(self).map_err(net_error)
            }
            Err(err) => Err(net_error(err)),
        }
    }

    fn require_user(&self) -> Result<(), NetError> {
        if self.credentials().has_user() {
            Ok(())
        } else {
            Err(NetError::SignInRequired(Network::X))
        }
    }
}

fn net_error(err: ApiError) -> NetError {
    match err {
        ApiError::Failed { status, .. } => NetError::Http {
            net: Network::X,
            status,
            message: err.to_string(),
        },
        ApiError::RateLimited { retry_after } => NetError::RateLimited {
            net: Network::X,
            retry_after,
        },
        ApiError::OutcomeUnknown(message) => NetError::OutcomeUnknown {
            net: Network::X,
            message,
        },
        ApiError::Other(message) => NetError::Other(message),
    }
}

fn native(id: &str) -> Result<String, NetError> {
    match split(id) {
        Some((Network::X, native)) => Ok(native.to_string()),
        Some((other, _)) => Err(NetError::Other(format!(
            "{id} belongs to {}, not X",
            other.title()
        ))),
        None => Ok(id.to_string()),
    }
}

fn scope_user(mut user: User) -> User {
    user.id = scoped(Network::X, &user.id);
    user
}

fn scope_post(mut post: Post) -> Post {
    post.id = scoped(Network::X, &post.id);
    post.conversation_id = scoped(Network::X, &post.conversation_id);
    post.in_reply_to = post.in_reply_to.map(|id| scoped(Network::X, &id));
    post.quoted = post.quoted.map(|q| Box::new(scope_post(*q)));
    post.author = scope_user(post.author);
    post
}

fn scope_page(page: Page) -> Page {
    Page {
        ids: page.ids.iter().map(|id| scoped(Network::X, id)).collect(),
        posts: page.posts.into_iter().map(scope_post).collect(),
        next: page.next,
    }
}

impl Provider for XClient {
    fn network(&self) -> Network {
        Network::X
    }

    fn caps(&self) -> Caps {
        caps(self.credentials().has_user())
    }

    fn signed_in(&self) -> bool {
        self.credentials().has_user()
    }

    fn account(&self) -> Option<User> {
        let creds = self.credentials();
        let username = creds.username.clone().filter(|_| creds.has_user())?;
        Some(User {
            id: scoped(
                Network::X,
                creds.user_id.as_deref().unwrap_or(username.as_str()),
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
        self.require_user()?;
        self.call(|c| XClient::me(c)).map(scope_user)
    }

    fn feed(&mut self, kind: FeedKind, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        let page = match kind {
            FeedKind::ForYou if self.credentials().uses_web() => {
                self.call(|c| c.web_for_you(cursor, count))?
            }
            FeedKind::ForYou | FeedKind::Latest => {
                let query = self.credentials().latest_query().to_string();
                self.call(|c| XClient::search(c, &query, cursor, count))?
            }
            FeedKind::Following => {
                self.require_user()?;
                self.call(|c| c.home(cursor, count))?
            }
            FeedKind::Bookmarks => {
                self.require_user()?;
                self.call(|c| c.bookmarks(cursor, count))?
            }
            FeedKind::Trending => return Err(NetError::Other(NO_TRENDING.into())),
            FeedKind::Discover => return Err(NetError::unsupported(Network::X, "discover feed")),
            FeedKind::Mine => return Err(NetError::unsupported(Network::X, "own-posts feed")),
        };
        Ok(scope_page(page))
    }

    fn mentions(&mut self, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.require_user()?;
        self.call(|c| c.mentions(cursor, count)).map(scope_page)
    }

    fn thread(&mut self, id: &str, count: u8) -> Result<Page, NetError> {
        let id = native(id)?;
        self.call(|c| c.conversation(&id, count)).map(scope_page)
    }

    fn user(
        &mut self,
        handle: &str,
        cursor: Option<&str>,
        count: u8,
    ) -> Result<(User, Page), NetError> {
        let user = self.call(|c| c.user_by_username(handle))?;
        let page = self.call(|c| c.user_tweets(&user.id, cursor, count))?;
        Ok((scope_user(user), scope_page(page)))
    }

    fn search(&mut self, query: &str, cursor: Option<&str>, count: u8) -> Result<Page, NetError> {
        self.call(|c| XClient::search(c, query, cursor, count))
            .map(scope_page)
    }

    fn post(&mut self, draft: &Draft) -> Result<Post, NetError> {
        if draft.quote.is_some() && !self.credentials().uses_web() {
            return Err(NetError::Other(QUOTE_NEEDS_LINK.into()));
        }
        self.require_user()?;
        let reply_to = draft.reply_to.as_deref().map(native).transpose()?;
        let quote = draft.quote.as_deref().map(native).transpose()?;
        let text = draft.text.clone();
        let id = if self.credentials().uses_web() {
            self.call(|c| c.web_create(&text, reply_to.as_deref(), quote.as_deref()))?
        } else {
            self.call(|c| c.create_post(&text, reply_to.as_deref()))?
        };
        let author = self.account().unwrap_or_else(|| User {
            id: scoped(Network::X, "me"),
            name: "You".into(),
            username: "i".into(),
            avatar_url: None,
            verified: false,
            business: false,
            description: None,
        });
        Ok(Post {
            id: scoped(Network::X, &id),
            net: Network::X,
            url: Some(format!("https://x.com/{}/status/{id}", author.username)),
            cid: None,
            author,
            text: draft.text.clone(),
            created_at: Some(crate::clock::utc_now()),
            conversation_id: draft
                .reply_to
                .clone()
                .unwrap_or_else(|| scoped(Network::X, &id)),
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

    fn delete(&mut self, id: &str) -> Result<(), NetError> {
        self.require_user()?;
        let id = native(id)?;
        self.call(|c| c.delete_post(&id))
    }

    fn set_like(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.require_user()?;
        let id = native(id)?;
        if on {
            self.call(|c| c.like(&id))
        } else {
            self.call(|c| c.unlike(&id))
        }
    }

    fn set_repost(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.require_user()?;
        let id = native(id)?;
        if on {
            self.call(|c| c.repost(&id))
        } else {
            self.call(|c| c.unrepost(&id))
        }
    }

    fn set_bookmark(&mut self, id: &str, on: bool) -> Result<(), NetError> {
        self.require_user()?;
        let id = native(id)?;
        if on {
            self.call(|c| c.bookmark(&id))
        } else {
            self.call(|c| c.unbookmark(&id))
        }
    }

    fn set_follow(&mut self, handle: &str, on: bool) -> Result<(), NetError> {
        self.require_user()?;
        let user = self.call(|c| c.user_by_username(handle))?;
        if on {
            self.call(|c| c.follow(&user.id))
        } else {
            self.call(|c| c.unfollow(&user.id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::x::auth::Credentials;

    #[test]
    fn a_rejected_browser_session_never_refreshes_stale_oauth_tokens() {
        let mut client = XClient::new(Credentials {
            auth_token: Some("cookie".into()),
            ct0: Some("csrf".into()),
            access_token: Some("stale-oauth".into()),
            refresh_token: Some("stale-refresh".into()),
            ..Credentials::default()
        })
        .unwrap();
        let calls = std::cell::Cell::new(0);
        let result: Result<(), NetError> = client.call(|_| {
            calls.set(calls.get() + 1);
            Err(ApiError::Failed {
                status: 401,
                detail: "expired cookie".into(),
                hint: String::new(),
            })
        });
        assert!(matches!(result, Err(NetError::Http { status: 401, .. })));
        assert_eq!(calls.get(), 1);
        assert_eq!(
            client.credentials().access_token.as_deref(),
            Some("stale-oauth")
        );
    }

    #[test]
    fn a_quote_draft_fails_before_any_request() {
        let mut client = XClient::new(Credentials::default()).unwrap();
        let draft = Draft {
            text: "hi".into(),
            reply_to: None,
            quote: Some("x:100".into()),
        };
        let err = client.post(&draft).unwrap_err().to_string();
        assert_eq!(err, QUOTE_NEEDS_LINK);
    }
}
