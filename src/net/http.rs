//! The blocking HTTP client every provider shares, and the error text of a failed response.
//!
//! Native builds send with reqwest. The browser build has no reqwest (the wasm graph forbids it)
//! and calls the page's `socli_http` import, which is a synchronous request from the reader worker.

#[cfg(any(test, target_arch = "wasm32"))]
use serde::Serialize;
use serde_json::Value;

use super::NetError;

pub const USER_AGENT: &str = concat!("socli/", env!("CARGO_PKG_VERSION"));

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::time::Duration;

    use super::{NetError, USER_AGENT};

    pub use reqwest::blocking::{Client, RequestBuilder, Response};
    pub use reqwest::header;
    pub use reqwest::{Error, Method, StatusCode};

    pub fn client() -> Result<Client, NetError> {
        Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .use_rustls_tls()
            .build()
            .map_err(|e| NetError::Other(format!("could not start the HTTP client: {e}")))
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use base64::Engine;
    use serde::de::DeserializeOwned;
    use serde::Serialize;

    use super::{pairs_of, NetError, USER_AGENT};

    pub use self::header::HeaderMap;

    const TOO_SMALL: usize = usize::MAX;
    const MAX_RESPONSE: usize = 8 * 1024 * 1024;

    #[derive(Clone, Debug, Default)]
    pub struct Client;

    impl Client {
        pub fn new() -> Self {
            Self
        }

        pub fn get(&self, url: impl AsRef<str>) -> RequestBuilder {
            self.request(Method::GET, url)
        }

        pub fn post(&self, url: impl AsRef<str>) -> RequestBuilder {
            self.request(Method::POST, url)
        }

        pub fn delete(&self, url: impl AsRef<str>) -> RequestBuilder {
            self.request(Method::DELETE, url)
        }

        pub fn request(&self, method: Method, url: impl AsRef<str>) -> RequestBuilder {
            RequestBuilder {
                method,
                url: url.as_ref().to_string(),
                query: Vec::new(),
                headers: vec![("user-agent".to_string(), USER_AGENT.to_string())],
                body: Vec::new(),
            }
        }
    }

    pub fn client() -> Result<Client, NetError> {
        Ok(Client)
    }

    #[derive(Clone, PartialEq, Eq, Debug)]
    pub struct Method(&'static str);

    impl Method {
        pub const GET: Self = Self("GET");
        pub const POST: Self = Self("POST");
        pub const PUT: Self = Self("PUT");
        pub const DELETE: Self = Self("DELETE");

        pub fn as_str(&self) -> &'static str {
            self.0
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub struct StatusCode(u16);

    impl StatusCode {
        pub const FORBIDDEN: Self = Self(403);
        pub const TOO_MANY_REQUESTS: Self = Self(429);

        pub fn from_u16(code: u16) -> Self {
            Self(code)
        }

        pub fn as_u16(self) -> u16 {
            self.0
        }

        pub fn is_success(self) -> bool {
            (200..300).contains(&self.0)
        }
    }

    impl std::fmt::Display for StatusCode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[derive(Debug)]
    pub struct Error {
        message: String,
    }

    impl Error {
        fn new(message: impl Into<String>) -> Self {
            Self {
                message: message.into(),
            }
        }

        pub fn without_url(&self) -> &str {
            &self.message
        }
    }

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.message)
        }
    }

    pub struct RequestBuilder {
        method: Method,
        url: String,
        query: Vec<(String, String)>,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl RequestBuilder {
        pub fn query<T: Serialize + ?Sized>(mut self, query: &T) -> Self {
            self.query.extend(pairs_of(query));
            self
        }

        pub fn json<T: Serialize + ?Sized>(mut self, body: &T) -> Self {
            self.body = serde_json::to_vec(body).unwrap_or_default();
            self.set_header("content-type", "application/json");
            self
        }

        pub fn form<T: Serialize + ?Sized>(mut self, form: &T) -> Self {
            let pairs = pairs_of(form);
            self.body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .finish()
                .into_bytes();
            self.set_header("content-type", "application/x-www-form-urlencoded");
            self
        }

        pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
            self.body = body.into();
            self
        }

        pub fn bearer_auth(self, token: impl std::fmt::Display) -> Self {
            let value = format!("Bearer {token}");
            self.header("authorization", value)
        }

        pub fn basic_auth<U, P>(self, username: U, password: Option<P>) -> Self
        where
            U: std::fmt::Display,
            P: std::fmt::Display,
        {
            let user = username.to_string();
            let raw = match password {
                Some(password) => format!("{user}:{password}"),
                None => format!("{user}:"),
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
            self.header("authorization", format!("Basic {encoded}"))
        }

        pub fn header(mut self, name: impl AsRef<str>, value: impl AsRef<str>) -> Self {
            self.set_header(name.as_ref(), value.as_ref());
            self
        }

        pub fn headers(mut self, headers: HeaderMap) -> Self {
            for (name, value) in headers.iter() {
                if let Ok(text) = value.to_str() {
                    self.set_header(name.as_ref(), text);
                }
            }
            self
        }

        pub fn send(self) -> Result<Response, Error> {
            let url = with_query(&self.url, &self.query);
            let header_json = serde_json::to_vec(&self.headers).unwrap_or_default();
            let bytes = exchange(
                self.method.as_str().as_bytes(),
                url.as_bytes(),
                &header_json,
                &self.body,
            )?;
            decode(&bytes)
        }

        fn set_header(&mut self, name: &str, value: &str) {
            let name = name.to_ascii_lowercase();
            if let Some(slot) = self.headers.iter_mut().find(|(key, _)| key == &name) {
                slot.1 = value.to_string();
            } else {
                self.headers.push((name, value.to_string()));
            }
        }
    }

    pub struct Response {
        status: StatusCode,
        headers: HeaderMap,
        body: Vec<u8>,
    }

    impl Response {
        pub fn status(&self) -> StatusCode {
            self.status
        }

        pub fn headers(&self) -> &HeaderMap {
            &self.headers
        }

        pub fn text(self) -> Result<String, Error> {
            String::from_utf8(self.body).map_err(|_| Error::new("the response was not utf-8"))
        }

        pub fn json<T: DeserializeOwned>(self) -> Result<T, Error> {
            serde_json::from_slice(&self.body).map_err(|err| Error::new(err.to_string()))
        }
    }

    pub mod header {
        use std::borrow::Cow;

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct HeaderName(Cow<'static, str>);

        impl HeaderName {
            pub const fn from_static(name: &'static str) -> Self {
                Self(Cow::Borrowed(name))
            }

            pub fn from_owned(name: String) -> Self {
                Self(Cow::Owned(name.to_ascii_lowercase()))
            }
        }

        impl AsRef<str> for HeaderName {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        #[derive(Clone, Debug)]
        pub struct HeaderValue(String);

        impl HeaderValue {
            #[expect(
                clippy::should_implement_trait,
                reason = "matches reqwest's inherent HeaderValue::from_str API"
            )]
            pub fn from_str(value: &str) -> Result<Self, InvalidHeaderValue> {
                value.parse()
            }

            pub fn to_str(&self) -> Result<&str, InvalidHeaderValue> {
                Ok(&self.0)
            }
        }

        impl std::str::FromStr for HeaderValue {
            type Err = InvalidHeaderValue;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                if value
                    .bytes()
                    .any(|byte| byte == 0 || byte == b'\r' || byte == b'\n')
                {
                    return Err(InvalidHeaderValue);
                }
                Ok(Self(value.to_string()))
            }
        }

        #[derive(Clone, Copy, Debug)]
        pub struct InvalidHeaderValue;

        #[derive(Clone, Debug, Default)]
        pub struct HeaderMap {
            pairs: Vec<(HeaderName, HeaderValue)>,
        }

        impl HeaderMap {
            pub fn new() -> Self {
                Self { pairs: Vec::new() }
            }

            pub fn insert(&mut self, name: HeaderName, value: HeaderValue) -> Option<HeaderValue> {
                if let Some(slot) = self
                    .pairs
                    .iter_mut()
                    .find(|(key, _)| key.as_ref().eq_ignore_ascii_case(name.as_ref()))
                {
                    return Some(std::mem::replace(&mut slot.1, value));
                }
                self.pairs.push((name, value));
                None
            }

            pub fn get(&self, name: impl AsRef<str>) -> Option<&HeaderValue> {
                let name = name.as_ref();
                self.pairs
                    .iter()
                    .find(|(key, _)| key.as_ref().eq_ignore_ascii_case(name))
                    .map(|(_, value)| value)
            }

            pub fn iter(&self) -> impl Iterator<Item = (&HeaderName, &HeaderValue)> {
                self.pairs.iter().map(|(name, value)| (name, value))
            }
        }

        pub const ACCEPT: HeaderName = HeaderName::from_static("accept");
        pub const CONTENT_TYPE: HeaderName = HeaderName::from_static("content-type");
        pub const RETRY_AFTER: HeaderName = HeaderName::from_static("retry-after");
    }

    fn with_query(base: &str, query: &[(String, String)]) -> String {
        if query.is_empty() {
            return base.to_string();
        }
        let Ok(mut url) = url::Url::parse(base) else {
            return base.to_string();
        };
        {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        url.to_string()
    }

    fn exchange(method: &[u8], url: &[u8], headers: &[u8], body: &[u8]) -> Result<Vec<u8>, Error> {
        let mut cap = 65_536usize;
        loop {
            let mut out = vec![0u8; cap];
            let wrote =
                // SAFETY: `method`, `url`, `headers`, and `body` are live slices for this call (null body pointer only when `body.len()` is 0), and `out` is this function's zeroed `Vec<u8>` of length `cap`. `socli_http` is the `env` import with that pointer-and-length signature; the worker reads the inputs, writes at most `out.len()` bytes into `out` or returns `usize::MAX` and writes nothing, and does not keep the pointers.
                unsafe {
                    socli_http(
                        method.as_ptr(),
                        method.len(),
                        url.as_ptr(),
                        url.len(),
                        headers.as_ptr(),
                        headers.len(),
                        if body.is_empty() {
                            std::ptr::null()
                        } else {
                            body.as_ptr()
                        },
                        body.len(),
                        out.as_mut_ptr(),
                        out.len(),
                    )
                };
            if wrote == TOO_SMALL {
                cap = cap.saturating_mul(2);
                if cap > MAX_RESPONSE {
                    return Err(Error::new("the response is larger than 8 MB"));
                }
                continue;
            }
            if wrote > out.len() {
                return Err(Error::new(
                    "the browser returned a response socli could not read",
                ));
            }
            out.truncate(wrote);
            return Ok(out);
        }
    }

    fn decode(bytes: &[u8]) -> Result<Response, Error> {
        if bytes.len() < 4 {
            return Err(Error::new("the browser returned an empty response"));
        }
        let status = u16::from_le_bytes([bytes[0], bytes[1]]);
        let count = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
        let mut at = 4;
        let mut headers = HeaderMap::new();
        for _ in 0..count {
            let (name, next) = read_chunk(bytes, at)?;
            let (value, next) = read_chunk(bytes, next)?;
            at = next;
            if let Ok(value) = header::HeaderValue::from_str(&value) {
                headers.insert(header::HeaderName::from_owned(name), value);
            }
        }
        if at + 4 > bytes.len() {
            return Err(Error::new("the browser response was cut off"));
        }
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&bytes[at..at + 4]);
        let body_len = u32::from_le_bytes(len_bytes) as usize;
        at += 4;
        let end = at
            .checked_add(body_len)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::new("the browser response was cut off"))?;
        let body = bytes[at..end].to_vec();
        if status == 0 {
            let message = String::from_utf8_lossy(&body).trim().to_string();
            return Err(Error::new(if message.is_empty() {
                "the browser could not reach the network".to_string()
            } else {
                message
            }));
        }
        Ok(Response {
            status: StatusCode::from_u16(status),
            headers,
            body,
        })
    }

    fn read_chunk(bytes: &[u8], at: usize) -> Result<(String, usize), Error> {
        if at + 2 > bytes.len() {
            return Err(Error::new("the browser response was cut off"));
        }
        let len = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
        let start = at + 2;
        let end = start + len;
        if end > bytes.len() {
            return Err(Error::new("the browser response was cut off"));
        }
        Ok((
            String::from_utf8_lossy(&bytes[start..end]).into_owned(),
            end,
        ))
    }

    #[link(wasm_import_module = "env")]
    unsafe extern "C" {
        /** The reader worker performs one synchronous request and writes the encoded response. `usize::MAX` means the buffer was too small. */
        fn socli_http(
            method: *const u8,
            method_len: usize,
            url: *const u8,
            url_len: usize,
            headers: *const u8,
            headers_len: usize,
            body: *const u8,
            body_len: usize,
            out: *mut u8,
            out_cap: usize,
        ) -> usize;
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use imp::{client, header, Client, Error, Method, RequestBuilder, Response, StatusCode};
#[cfg(target_arch = "wasm32")]
pub use imp::{client, header, Client, Error, Method, RequestBuilder, Response, StatusCode};

/** Query or form pairs. A slice of 2-tuples serializes as a JSON array of pairs. */
#[cfg(any(test, target_arch = "wasm32"))]
fn pairs_of(value: &(impl Serialize + ?Sized)) -> Vec<(String, String)> {
    let Ok(Value::Array(items)) = serde_json::to_value(value) else {
        return Vec::new();
    };
    items
        .into_iter()
        .filter_map(|item| {
            let Value::Array(pair) = item else {
                return None;
            };
            if pair.len() != 2 {
                return None;
            }
            Some((json_string(&pair[0]), json_string(&pair[1])))
        })
        .collect()
}

#[cfg(any(test, target_arch = "wasm32"))]
fn json_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Preserves uncertain mutation outcomes while removing request URLs from diagnostics.
pub fn transport_error(net: super::Network, error: Error, mutation: bool) -> NetError {
    #[cfg(not(target_arch = "wasm32"))]
    let uncertain = mutation && !error.is_connect() && !error.is_builder();
    #[cfg(target_arch = "wasm32")]
    let uncertain = mutation;
    let message = format!("network: {}", error.without_url());
    if uncertain {
        NetError::OutcomeUnknown { net, message }
    } else {
        NetError::Other(message)
    }
}

const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// Reads at most 8 MiB so a malformed API response cannot exhaust the process memory.
pub fn read_text(response: Response) -> Result<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let mut bytes = Vec::new();
        response
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "could not read the network response".to_string())?;
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            return Err("the response is larger than 8 MB".into());
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let text = response.text().map_err(|error| error.to_string())?;
        if text.len() as u64 > MAX_RESPONSE_BYTES {
            return Err("the response is larger than 8 MB".into());
        }
        Ok(text)
    }
}

/// The human part of a JSON error body (`message`, `error_description`, `detail`, `errors[]`).
pub fn error_text(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    if let Some(errors) = value.get("errors").and_then(Value::as_array) {
        let parts: Vec<&str> = errors
            .iter()
            .filter_map(|e| {
                e.get("message")
                    .or_else(|| e.get("detail"))
                    .and_then(Value::as_str)
            })
            .collect();
        if !parts.is_empty() {
            return Some(parts.join("; "));
        }
    }
    ["message", "error_description", "detail", "title", "error"]
        .iter()
        .find_map(|key| match value.get(*key) {
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Object(inner)) => inner
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text_reads_the_common_shapes() {
        assert_eq!(
            error_text(r#"{"error":"InvalidToken","message":"Token has expired"}"#).as_deref(),
            Some("Token has expired")
        );
        assert_eq!(
            error_text(r#"{"error":{"message":"Invalid OAuth access token","code":190}}"#)
                .as_deref(),
            Some("Invalid OAuth access token")
        );
        assert_eq!(
            error_text(r#"{"errors":[{"message":"a"},{"detail":"b"}]}"#).as_deref(),
            Some("a; b")
        );
        assert_eq!(error_text("<html>"), None);
    }

    #[test]
    fn query_pairs_append_in_call_order() {
        let mut pairs = pairs_of(&[("access_token", "t")]);
        pairs.extend(pairs_of(&[("fields", "id"), ("limit", "20")]));
        assert_eq!(
            pairs,
            vec![
                ("access_token".into(), "t".into()),
                ("fields".into(), "id".into()),
                ("limit".into(), "20".into()),
            ]
        );
        let owned: &[(&str, String)] = &[("q", "a".into())];
        assert_eq!(pairs_of(owned), vec![("q".into(), "a".into())]);
    }
}
