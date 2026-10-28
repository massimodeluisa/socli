//! Requests to dev-null.sh: public, signed by the device key, or plain text to the CLI endpoints.

use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

use super::sign;
use crate::net::http::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER};
use crate::net::http::{self, Client, Method, Response};
use crate::net::{NetError, Network};

pub const ORIGIN: &str = "https://dev-null.sh";

const NET: Network = Network::Devnull;

/// What reaches the server as the request target: the path and its query, exactly as sent.
pub fn target(url: &Url) -> &str {
    &url[url::Position::BeforePath..url::Position::AfterQuery]
}

/// A dev-null.sh URL; each segment is percent-encoded, so a handle or id cannot change the path.
pub fn url(segments: &[&str], query: &[(&str, &str)]) -> Result<Url, NetError> {
    let mut url = Url::parse(ORIGIN).map_err(|e| NetError::Other(e.to_string()))?;
    url.path_segments_mut()
        .map_err(|()| NetError::Other(format!("{ORIGIN} cannot hold a path")))?
        .clear()
        .extend(segments);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    Ok(url)
}

pub fn now() -> u64 {
    crate::clock::unix_secs()
}

/** A JSON request to the v1 API. With a key, it carries the three signature headers over the canonical message. */
pub fn request(
    http: &Client,
    key: Option<&ssh_key::PrivateKey>,
    method: Method,
    url: Url,
    body: Option<&Value>,
) -> Result<Response, NetError> {
    let bytes = match body {
        Some(value) => serde_json::to_vec(value)
            .map_err(|e| NetError::Other(format!("could not encode the request: {e}")))?,
        None => Vec::new(),
    };
    let mutation = method != Method::GET;
    let mut builder = http
        .request(method.clone(), url.clone())
        .header(ACCEPT, "application/json");
    if let Some(key) = key {
        let timestamp = now();
        let nonce = sign::nonce();
        let message = sign::canonical(timestamp, &nonce, method.as_str(), target(&url), &bytes);
        let signature =
            sign::signature(key, sign::API_NAMESPACE, &message).map_err(NetError::Other)?;
        builder = builder
            .header("X-Devnull-Timestamp", timestamp.to_string())
            .header("X-Devnull-Nonce", nonce)
            .header("X-SSH-Signature", signature);
    }
    if body.is_some() {
        builder = builder.header(CONTENT_TYPE, "application/json").body(bytes);
    }
    let response = builder
        .send()
        .map_err(|error| http::transport_error(NET, error, mutation))?;
    check(response, mutation)
}

/// A plain-text POST to one of the `/api/cli` endpoints the commit script uses.
#[cfg(feature = "native")]
pub fn post_text(
    http: &Client,
    segments: &[&str],
    body: Vec<u8>,
    signature: Option<&str>,
) -> Result<String, NetError> {
    let mut builder = http
        .post(url(segments, &[])?)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(body);
    if let Some(signature) = signature {
        builder = builder.header("X-SSH-Signature", signature);
    }
    let response = builder
        .send()
        .map_err(|error| http::transport_error(NET, error, true))?;
    http::read_text(check(response, true)?)
        .map_err(|message| NetError::OutcomeUnknown { net: NET, message })
}

pub fn json<T: DeserializeOwned>(response: Response) -> Result<T, NetError> {
    let body = text(response)?;
    serde_json::from_str(&body).map_err(|e| NetError::Parse {
        net: NET,
        message: e.to_string(),
    })
}

pub fn text(response: Response) -> Result<String, NetError> {
    http::read_text(response).map_err(|message| NetError::Parse { net: NET, message })
}

fn check(response: Response, mutation: bool) -> Result<Response, NetError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    let body = http::read_text(response).unwrap_or_default();
    if mutation && status.as_u16() >= 500 {
        return Err(NetError::OutcomeUnknown {
            net: NET,
            message: error_for(status.as_u16(), retry_after, &body).to_string(),
        });
    }
    Err(error_for(status.as_u16(), retry_after, &body))
}

/** The error a failed response means: `device_not_approved` asks for a new sign-in, `rate_limited` or a 429 waits, anything else shows the server's message. */
pub fn error_for(status: u16, retry_after: Option<u64>, body: &str) -> NetError {
    let value: Option<Value> = serde_json::from_str(body).ok();
    let field = |key: &str| {
        value
            .as_ref()
            .and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    match field("code").as_deref() {
        Some("device_not_approved") => return NetError::SignInRequired(NET),
        Some("rate_limited") => {
            return NetError::RateLimited {
                net: NET,
                retry_after,
            }
        }
        _ if status == 429 => {
            return NetError::RateLimited {
                net: NET,
                retry_after,
            }
        }
        _ => {}
    }
    let text = body.trim();
    let message = field("message")
        .filter(|m| !m.is_empty())
        .or_else(|| {
            (value.is_none() && !text.is_empty() && !text.starts_with('<'))
                .then(|| text.to_string())
        })
        .unwrap_or_else(|| format!("dev/null returned {status}"));
    NetError::Http {
        net: NET,
        status,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signed_target_is_the_path_and_query_as_sent() {
        let notifications = url(
            &["api", "v1", "notifications"],
            &[("type", "replies,mentions,quotes")],
        )
        .unwrap();
        assert_eq!(
            target(&notifications),
            "/api/v1/notifications?type=replies%2Cmentions%2Cquotes"
        );
        let me = url(&["api", "v1", "me"], &[]).unwrap();
        assert_eq!(target(&me), "/api/v1/me");
        let user = url(&["api", "v1", "users", "a/b?c"], &[]).unwrap();
        assert_eq!(target(&user), "/api/v1/users/a%2Fb%3Fc");
        let latest = url(&["api", "v1", "log", "latest"], &[("cursor", "eyJ0IjoxfQ")]).unwrap();
        assert_eq!(
            latest.as_str(),
            "https://dev-null.sh/api/v1/log/latest?cursor=eyJ0IjoxfQ"
        );
        assert_eq!(target(&latest), "/api/v1/log/latest?cursor=eyJ0IjoxfQ");
    }

    #[test]
    fn error_codes_map_to_next_steps() {
        let body = r#"{"message":"This key isn't approved for the /dev/null app. Sign in again in the app.","code":"device_not_approved"}"#;
        assert!(matches!(
            error_for(401, None, body),
            NetError::SignInRequired(Network::Devnull)
        ));
        assert!(matches!(
            error_for(
                429,
                Some(12),
                r#"{"message":"Too Many Attempts.","code":"rate_limited"}"#
            ),
            NetError::RateLimited {
                retry_after: Some(12),
                ..
            }
        ));
        assert!(matches!(
            error_for(429, None, "Too Many Attempts.\n"),
            NetError::RateLimited {
                retry_after: None,
                ..
            }
        ));
        match error_for(
            401,
            None,
            r#"{"message":"Signature expired. Check that your clock is right.","code":"invalid_signature"}"#,
        ) {
            NetError::Http {
                status, message, ..
            } => {
                assert_eq!(status, 401);
                assert_eq!(
                    message,
                    "Signature expired. Check that your clock is right."
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn plain_text_errors_keep_their_message_and_html_does_not() {
        match error_for(
            404,
            None,
            "None of your SSH keys are on the GitHub account of a /dev/null user.\n",
        ) {
            NetError::Http { message, .. } => {
                assert_eq!(
                    message,
                    "None of your SSH keys are on the GitHub account of a /dev/null user."
                )
            }
            other => panic!("unexpected {other:?}"),
        }
        match error_for(502, None, "<html>bad gateway</html>") {
            NetError::Http { message, .. } => assert_eq!(message, "dev/null returned 502"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
