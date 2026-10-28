//! One XRPC round trip, and what to do with the answer.

use serde_json::Value;

use crate::net::http::Client;
use crate::net::{http, NetError, Network};

pub struct Raw {
    pub status: u16,
    pub body: String,
    pub retry_after: Option<u64>,
}

pub enum Call<'a> {
    Get(&'a [(&'a str, String)]),
    Post(Option<&'a Value>),
    Write(&'a Value),
}

pub fn send(
    client: &Client,
    url: &str,
    call: &Call<'_>,
    token: Option<&str>,
) -> Result<Raw, NetError> {
    let mut request = match call {
        Call::Get(query) => client.get(url).query(query),
        Call::Post(Some(body)) | Call::Write(body) => client.post(url).json(body),
        Call::Post(None) => client.post(url),
    };
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let mutation = matches!(call, Call::Write(_));
    let response = request
        .send()
        .map_err(|error| http::transport_error(Network::Bluesky, error, mutation))?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get(http::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    let body = http::read_text(response).map_err(|message| {
        if mutation && (200..300).contains(&status) {
            NetError::OutcomeUnknown {
                net: Network::Bluesky,
                message,
            }
        } else {
            NetError::Other(format!("could not read the Bluesky response: {message}"))
        }
    })?;
    Ok(Raw {
        status,
        body,
        retry_after,
    })
}

/// Turns an answer into its JSON body (`Null` when empty), or the matching `NetError`.
pub fn finish(raw: Raw) -> Result<Value, NetError> {
    if (200..300).contains(&raw.status) {
        if raw.body.trim().is_empty() {
            return Ok(Value::Null);
        }
        return serde_json::from_str(&raw.body).map_err(|e| NetError::Parse {
            net: Network::Bluesky,
            message: e.to_string(),
        });
    }
    if raw.status == 429 {
        return Err(NetError::RateLimited {
            net: Network::Bluesky,
            retry_after: raw.retry_after,
        });
    }
    Err(NetError::Http {
        net: Network::Bluesky,
        status: raw.status,
        message: http::error_text(&raw.body)
            .unwrap_or_else(|| format!("Bluesky returned {}", raw.status)),
    })
}

/// A 400 or 401 whose `error` says the access token is spent: refresh once and retry.
pub fn should_refresh(status: u16, body: &str) -> bool {
    if status != 400 && status != 401 {
        return false;
    }
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .is_some_and(|e| e == "ExpiredToken" || e == "InvalidToken")
}

pub fn refreshed(raw: Raw) -> Result<(String, String), NetError> {
    if raw.status == 401 || should_refresh(raw.status, &raw.body) {
        return Err(NetError::SignInRequired(Network::Bluesky));
    }
    let value = finish(raw)?;
    let token = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
    };
    match (token("accessJwt"), token("refreshJwt")) {
        (Some(access), Some(refresh)) => Ok((access, refresh)),
        _ => Err(NetError::Parse {
            net: Network::Bluesky,
            message: "the refreshed session had no access or refresh token".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_preserves_rate_limits_and_server_failures() {
        for status in [429, 503] {
            let error = refreshed(Raw {
                status,
                body: r#"{"message":"try later"}"#.into(),
                retry_after: Some(30),
            })
            .unwrap_err();
            match status {
                429 => assert!(matches!(
                    error,
                    NetError::RateLimited {
                        retry_after: Some(30),
                        ..
                    }
                )),
                _ => assert!(matches!(error, NetError::Http { status: 503, .. })),
            }
        }
    }

    #[test]
    fn refresh_rejects_spent_and_incomplete_credentials() {
        assert!(matches!(
            refreshed(Raw {
                status: 400,
                body: r#"{"error":"ExpiredToken"}"#.into(),
                retry_after: None,
            }),
            Err(NetError::SignInRequired(Network::Bluesky))
        ));
        for body in [
            r#"{"accessJwt":"","refreshJwt":"r"}"#,
            r#"{"accessJwt":"a","refreshJwt":" "}"#,
        ] {
            assert!(matches!(
                refreshed(Raw {
                    status: 200,
                    body: body.into(),
                    retry_after: None
                }),
                Err(NetError::Parse { .. })
            ));
        }
        assert_eq!(
            refreshed(Raw {
                status: 200,
                body: r#"{"accessJwt":"a","refreshJwt":"r"}"#.into(),
                retry_after: None
            })
            .unwrap(),
            ("a".into(), "r".into())
        );
    }

    #[test]
    fn expired_and_invalid_tokens_trigger_a_refresh() {
        let expired = r#"{"error":"ExpiredToken","message":"Token has expired"}"#;
        let invalid = r#"{"error":"InvalidToken","message":"Bad token"}"#;
        assert!(should_refresh(400, expired));
        assert!(should_refresh(401, expired));
        assert!(should_refresh(400, invalid));
        assert!(should_refresh(401, invalid));
    }

    #[test]
    fn other_failures_do_not_trigger_a_refresh() {
        let expired = r#"{"error":"ExpiredToken"}"#;
        assert!(!should_refresh(500, expired));
        assert!(!should_refresh(403, expired));
        assert!(!should_refresh(
            401,
            r#"{"error":"AuthenticationRequired"}"#
        ));
        assert!(!should_refresh(400, r#"{"error":"InvalidRequest"}"#));
        assert!(!should_refresh(401, "<html>"));
    }

    #[test]
    fn answers_become_json_or_typed_errors() {
        let ok = Raw {
            status: 200,
            body: String::new(),
            retry_after: None,
        };
        assert_eq!(finish(ok).expect("empty body"), Value::Null);
        let limited = Raw {
            status: 429,
            body: String::new(),
            retry_after: Some(30),
        };
        assert!(matches!(
            finish(limited),
            Err(NetError::RateLimited {
                retry_after: Some(30),
                ..
            })
        ));
        let failed = Raw {
            status: 400,
            body: r#"{"error":"InvalidRequest","message":"bad uri"}"#.into(),
            retry_after: None,
        };
        match finish(failed) {
            Err(NetError::Http {
                status, message, ..
            }) => {
                assert_eq!(status, 400);
                assert_eq!(message, "bad uri");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
