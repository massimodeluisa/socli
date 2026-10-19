//! Link previews: a page's Open Graph tags and image, fetched in the background with size, time and redirect limits, from public addresses only.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
#[cfg(feature = "native")]
use std::thread;
use std::time::Duration;

#[cfg(feature = "native")]
use reqwest::blocking::Client;
#[cfg(feature = "native")]
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
#[cfg(feature = "native")]
use reqwest::header::{ACCEPT, CONTENT_TYPE};
#[cfg(feature = "native")]
use reqwest::redirect::Policy;
use url::Url;

const MAX_BYTES: usize = 512 * 1024;
const CHUNK: usize = 8 * 1024;
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REDIRECTS: usize = 5;
const HEAD_END: &[u8] = b"</head>";
const IMAGE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const WORKERS: usize = 2;

/// What a page's Open Graph tags say about it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Card {
    pub image: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub title: Option<String>,
    pub site: Option<String>,
    pub description: Option<String>,
}

/// Reads the preview fields from the `<head>` of `html`; relative image URLs resolve against `page`.
pub fn parse(html: &str, page: &Url) -> Card {
    let head = head_of(html.as_bytes(), html.len());
    let head = String::from_utf8_lossy(head);
    let meta = metas(&head);
    let get = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| meta.get(*key).map(|value| value.trim()))
            .filter(|value| !value.is_empty())
    };
    let size = |key: &str| get(&[key]).and_then(|value| value.parse().ok());
    let image = get(&[
        "og:image",
        "og:image:url",
        "og:image:secure_url",
        "twitter:image",
        "twitter:image:src",
    ])
    .and_then(|raw| page.join(raw).ok())
    .filter(|url| matches!(url.scheme(), "http" | "https"))
    .map(String::from);
    let title = get(&["og:title", "twitter:title"])
        .map(str::to_string)
        .or_else(|| title_tag(&head));
    let description = get(&["og:description", "twitter:description", "description"]);
    Card {
        image,
        width: size("og:image:width"),
        height: size("og:image:height"),
        title: title
            .map(|text| collapse(&text))
            .filter(|text| !text.is_empty()),
        site: get(&["og:site_name"]).map(str::to_string),
        description: description.map(collapse).filter(|text| !text.is_empty()),
    }
}

/// Downloads the start of `page` and parses its preview fields.
#[cfg(feature = "native")]
pub fn fetch(client: &Client, page: &str) -> Result<Card, String> {
    let url = Url::parse(page)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .ok_or("not a web page")?;
    if !public(&url) {
        return Err("not a public address".to_string());
    }
    let mut response = client
        .get(url)
        .header(ACCEPT, "text/html,application/xhtml+xml")
        .timeout(TIMEOUT)
        .send()
        .map_err(|error| format!("couldn't fetch the page: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("the page answered {}", status.as_u16()));
    }
    let not_html = response.headers().get(CONTENT_TYPE).is_some_and(|value| {
        !String::from_utf8_lossy(value.as_bytes())
            .to_ascii_lowercase()
            .contains("html")
    });
    if not_html {
        return Err("not an html page".to_string());
    }
    let final_url = response.url().clone();
    let mut body = Vec::new();
    let mut chunk = [0u8; CHUNK];
    while body.len() < MAX_BYTES && find_head_end(&body).is_none() {
        let read = response
            .read(&mut chunk)
            .map_err(|error| format!("couldn't read the page: {error}"))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    let html = String::from_utf8_lossy(head_of(&body, MAX_BYTES));
    Ok(parse(&html, &final_url))
}

/// Builds the client for unfurls and their images: every redirect hop and every connection must be public.
#[cfg(feature = "native")]
pub fn client() -> Result<Client, String> {
    let policy = Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if public(attempt.url()) {
            attempt.follow()
        } else {
            attempt.error("not a public address")
        }
    });
    Client::builder()
        .user_agent(concat!("socli/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(TIMEOUT)
        .redirect(policy)
        .dns_resolver(Arc::new(PublicOnly))
        .build()
        .map_err(|error| format!("couldn't start the link reader: {error}"))
}

/// Downloads an image from a public address, at most 8 MiB; `client` refuses every non-public hop.
#[cfg(feature = "native")]
pub fn image(client: &Client, url: &str) -> Result<Vec<u8>, String> {
    let url = Url::parse(url)
        .ok()
        .filter(public)
        .ok_or("not a public address")?;
    let response = client
        .get(url)
        .header(ACCEPT, "image/*")
        .timeout(IMAGE_TIMEOUT)
        .send()
        .map_err(|error| format!("couldn't fetch the image: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("the image answered {}", status.as_u16()));
    }
    let too_large = || "the image is larger than 8 MiB".to_string();
    if response.content_length().unwrap_or(0) > MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("couldn't read the image: {error}"))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

#[cfg(feature = "native")]
thread_local! {
    /** Each preview worker keeps its own client, since the resolver blocks the client's runtime. */
    static CLIENT: Result<Client, String> = client();
}

/// [`image`] through the calling thread's own client.
#[cfg(feature = "native")]
pub fn image_here(url: &str) -> Result<Vec<u8>, String> {
    CLIENT.with(|client| image(client.as_ref().map_err(String::clone)?, url))
}

struct Queue {
    pages: Mutex<VecDeque<String>>,
    wake: Condvar,
    closed: AtomicBool,
}

/// Reads link pages on background threads, each page once per session.
pub struct Unfurls {
    queue: Arc<Queue>,
    results: Receiver<(String, Option<Card>)>,
    asked: HashSet<String>,
}

impl Unfurls {
    /// Starts the workers; each builds its own client.
    #[allow(
        clippy::new_without_default,
        reason = "starting workers is a side effect, not a default value"
    )]
    pub fn new() -> Self {
        let queue = Arc::new(Queue {
            pages: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            closed: AtomicBool::new(false),
        });
        let (sender, results) = channel();
        #[cfg(feature = "native")]
        for _ in 0..WORKERS {
            let queue = Arc::clone(&queue);
            let sender = sender.clone();
            thread::spawn(move || work(&queue, &sender));
        }
        #[cfg(not(feature = "native"))]
        drop(sender);
        Self {
            queue,
            results,
            asked: HashSet::new(),
        }
    }

    /// Queues `page` unless it was asked for before.
    pub fn request(&mut self, page: &str) {
        if !self.asked.insert(page.to_string()) {
            return;
        }
        self.queue
            .pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(page.to_string());
        self.queue.wake.notify_one();
    }

    /// The pages read since the last call, without blocking; pages that failed are left out.
    pub fn poll(&mut self) -> Vec<(String, Card)> {
        let mut cards = Vec::new();
        while let Ok((page, card)) = self.results.try_recv() {
            if let Some(card) = card {
                cards.push((page, card));
            }
        }
        cards
    }
}

impl Drop for Unfurls {
    fn drop(&mut self) {
        self.queue.closed.store(true, Ordering::SeqCst);
        self.queue.wake.notify_all();
    }
}

#[cfg(feature = "native")]
fn work(queue: &Queue, sender: &Sender<(String, Option<Card>)>) {
    let client = client();
    loop {
        let page = {
            let mut pages = queue.pages.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if queue.closed.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(page) = pages.pop_front() {
                    break page;
                }
                pages = queue
                    .wake
                    .wait(pages)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let card = client
            .as_ref()
            .ok()
            .and_then(|client| fetch(client, &page).ok())
            .map(|mut card| {
                card.image = card
                    .image
                    .filter(|image| Url::parse(image).is_ok_and(|url| public(&url)));
                card
            });
        if sender.send((page, card)).is_err() {
            return;
        }
    }
}

/** Resolves names like the system does but drops non-public addresses, so a rebinding answer can't reach the connection. */
#[cfg(feature = "native")]
struct PublicOnly;

#[cfg(feature = "native")]
impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = (name.as_str(), 0)
                .to_socket_addrs()?
                .filter(|addr| global(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err("not a public address".into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// True when `url` is http(s) and its host resolves only to public addresses.
pub fn public(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) => global(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => global(IpAddr::V6(ip)),
        Some(url::Host::Domain(domain)) => {
            let name = domain.trim_end_matches('.').to_ascii_lowercase();
            let local = name == "localhost"
                || [".localhost", ".local", ".internal"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix));
            let port = url.port_or_known_default().unwrap_or(80);
            !local
                && (name.as_str(), port)
                    .to_socket_addrs()
                    .is_ok_and(|mut addrs| addrs.len() > 0 && addrs.all(|addr| global(addr.ip())))
        }
        None => false,
    }
}

fn global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => global_v4(ip),
        IpAddr::V6(ip) => global_v6(ip),
    }
}

fn global_v4(ip: Ipv4Addr) -> bool {
    let [first, second, ..] = ip.octets();
    let shared = first == 100 && second & 0xC0 == 64;
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || shared
        || first == 0)
}

/** IPv4-mapped, IPv4-compatible and NAT64 forms are judged by the IPv4 address they carry. */
fn global_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    let [a, b, c, d, e, f, g, h] = segments;
    let embedded = |high: u16, low: u16| {
        let [h1, h2] = high.to_be_bytes();
        let [l1, l2] = low.to_be_bytes();
        Ipv4Addr::new(h1, h2, l1, l2)
    };
    if let Some(v4) = ip.to_ipv4_mapped() {
        return global_v4(v4);
    }
    if [a, b, c, d, e, f] == [0; 6] && (g, h) != (0, 0) && (g, h) != (0, 1) {
        return global_v4(embedded(g, h));
    }
    if [a, b, c, d, e, f] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return global_v4(embedded(g, h));
    }
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a & 0xffc0 == 0xfe80
        || a & 0xfe00 == 0xfc00)
}

fn find_head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(HEAD_END.len())
        .position(|window| window.eq_ignore_ascii_case(HEAD_END))
        .map(|start| start + HEAD_END.len())
}

/** The bytes up to `</head>` (inclusive), or up to `cap` bytes when it doesn't appear. */
fn head_of(bytes: &[u8], cap: usize) -> &[u8] {
    let capped = &bytes[..bytes.len().min(cap)];
    &capped[..find_head_end(capped).unwrap_or(capped.len())]
}

/** Maps each `<meta property|name=…>` key (lowercase) to its first non-missing content. */
fn metas(head: &str) -> HashMap<String, String> {
    let lower = head.to_ascii_lowercase();
    let mut found = HashMap::new();
    let mut from = 0;
    while let Some(offset) = lower[from..].find("<meta") {
        let start = from + offset + "<meta".len();
        from = start;
        let Some(rest) = head.get(start..) else {
            break;
        };
        if !rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '/') {
            continue;
        }
        let end = tag_end(rest);
        from = start + end;
        let attributes = attributes(&rest[..end]);
        let key = ["property", "name"]
            .iter()
            .find_map(|name| attributes.get(*name));
        if let (Some(key), Some(content)) = (key, attributes.get("content")) {
            found
                .entry(key.trim().to_ascii_lowercase())
                .or_insert_with(|| content.clone());
        }
    }
    found
}

/// Returns the byte index of the `>` that closes a tag, ignoring any inside quotes.
fn tag_end(rest: &str) -> usize {
    let mut quote = None;
    for (index, c) in rest.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, '>') => return index,
            _ => {}
        }
    }
    rest.len()
}

/** Parses `name=value` pairs (quoted or bare); names are lowercase and values are entity-decoded. */
fn attributes(tag: &str) -> HashMap<String, String> {
    let mut found = HashMap::new();
    let mut chars = tag.char_indices().peekable();
    while let Some(&(start, c)) = chars.peek() {
        if c.is_ascii_whitespace() || c == '/' {
            chars.next();
            continue;
        }
        let mut name_end = tag.len();
        while let Some(&(index, c)) = chars.peek() {
            if c == '=' || c.is_ascii_whitespace() || c == '/' {
                name_end = index;
                break;
            }
            chars.next();
        }
        let name = tag[start..name_end].to_ascii_lowercase();
        while chars.next_if(|&(_, c)| c.is_ascii_whitespace()).is_some() {}
        if chars.next_if(|&(_, c)| c == '=').is_none() {
            continue;
        }
        while chars.next_if(|&(_, c)| c.is_ascii_whitespace()).is_some() {}
        let value = match chars.peek().copied() {
            Some((index, quote @ ('"' | '\''))) => {
                chars.next();
                let from = index + 1;
                let mut to = tag.len();
                for (index, c) in chars.by_ref() {
                    if c == quote {
                        to = index;
                        break;
                    }
                }
                &tag[from..to]
            }
            Some((from, _)) => {
                let mut to = tag.len();
                while let Some(&(index, c)) = chars.peek() {
                    if c.is_ascii_whitespace() {
                        to = index;
                        break;
                    }
                    chars.next();
                }
                &tag[from..to]
            }
            None => "",
        };
        found.entry(name).or_insert_with(|| decode(value));
    }
    found
}

fn title_tag(head: &str) -> Option<String> {
    let lower = head.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let text = collapse(&decode(&head[start..end]));
    (!text.is_empty()).then_some(text)
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest
            .find(';')
            .filter(|&end| end <= 10)
            .and_then(|end| entity(&rest[1..end]).map(|c| (c, end)));
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "lt" => Some('<'),
        "gt" => Some('>'),
        _ => {
            let digits = name.strip_prefix('#')?;
            let code = match digits.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => digits.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Url {
        Url::parse("https://example.com/blog/post").expect("valid test url")
    }

    fn card(html: &str) -> Card {
        parse(html, &page())
    }

    #[test]
    fn images_from_private_addresses_are_refused() {
        let client = client().expect("client builds");
        for url in [
            "http://127.0.0.1/a.png",
            "http://[::1]/a.png",
            "http://10.0.0.8/a.png",
            "file:///etc/passwd",
            "not a url",
        ] {
            assert_eq!(
                image(&client, url).unwrap_err(),
                "not a public address",
                "{url}"
            );
        }
    }

    #[test]
    fn each_page_is_asked_once_and_failures_stay_out() {
        let mut unfurls = Unfurls::new();
        unfurls.request("http://127.0.0.1/page");
        unfurls.request("http://127.0.0.1/page");
        assert_eq!(unfurls.asked.len(), 1);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !unfurls.queue.pages.lock().unwrap().is_empty()
            && std::time::Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_millis(50));
        assert!(unfurls.poll().is_empty());
    }

    #[test]
    fn og_tags_in_both_attribute_orders() {
        let html = r#"<head>
            <meta property="og:image" content="https://cdn.example.com/a.png">
            <meta content="https://cdn.example.com/b.png" property="og:image:url">
            <meta content="Hello" property="og:title">
            <meta property="og:site_name" content="Example">
            <meta property="og:description" content="A post">
            <meta property="og:image:width" content="1200">
            <meta content="630" property="og:image:height">
        </head>"#;
        assert_eq!(
            card(html),
            Card {
                image: Some("https://cdn.example.com/a.png".to_string()),
                width: Some(1200),
                height: Some(630),
                title: Some("Hello".to_string()),
                site: Some("Example".to_string()),
                description: Some("A post".to_string()),
            }
        );
    }

    #[test]
    fn name_attribute_works_like_property() {
        let html =
            r#"<meta name="og:title" content="Named"><meta name="description" content="Plain">"#;
        let card = card(html);
        assert_eq!(card.title.as_deref(), Some("Named"));
        assert_eq!(card.description.as_deref(), Some("Plain"));
    }

    #[test]
    fn single_quotes_and_bare_values() {
        let html =
            "<meta property='og:title' content='It\"s'><meta property=og:site_name content=Bare>";
        let card = card(html);
        assert_eq!(card.title.as_deref(), Some("It\"s"));
        assert_eq!(card.site.as_deref(), Some("Bare"));
    }

    #[test]
    fn a_gt_inside_quotes_does_not_end_the_tag() {
        let card = card(r#"<meta property="og:title" content="a > b">"#);
        assert_eq!(card.title.as_deref(), Some("a > b"));
    }

    #[test]
    fn entities_are_decoded() {
        let html = r#"<meta property="og:title" content="Tom &amp; Jerry &quot;x&quot; &#39;y&#39; &lt;z&gt; &#x41;&#66; &bogus; &">"#;
        assert_eq!(
            card(html).title.as_deref(),
            Some("Tom & Jerry \"x\" 'y' <z> AB &bogus; &")
        );
    }

    #[test]
    fn relative_image_is_resolved() {
        let card = card(r#"<meta property="og:image" content="/img/a.png">"#);
        assert_eq!(card.image.as_deref(), Some("https://example.com/img/a.png"));
        let card = self::card(r#"<meta property="og:image" content="a.png">"#);
        assert_eq!(
            card.image.as_deref(),
            Some("https://example.com/blog/a.png")
        );
    }

    #[test]
    fn protocol_relative_image_is_resolved() {
        let card = card(r#"<meta property="og:image" content="//cdn.example.com/a.png">"#);
        assert_eq!(card.image.as_deref(), Some("https://cdn.example.com/a.png"));
    }

    #[test]
    fn non_web_images_are_dropped() {
        for content in [
            "javascript:alert(1)",
            "data:image/png;base64,AAAA",
            "ftp://x/a.png",
        ] {
            let html = format!(r#"<meta property="og:image" content="{content}">"#);
            assert_eq!(card(&html).image, None, "{content}");
        }
    }

    #[test]
    fn twitter_fallbacks() {
        let html = r#"<meta name="twitter:image:src" content="/t.png">
            <meta name="twitter:title" content="Tw title">
            <meta name="twitter:description" content="Tw desc">
            <meta name="description" content="Plain">"#;
        let card = card(html);
        assert_eq!(card.image.as_deref(), Some("https://example.com/t.png"));
        assert_eq!(card.title.as_deref(), Some("Tw title"));
        assert_eq!(card.description.as_deref(), Some("Tw desc"));
    }

    #[test]
    fn og_wins_over_twitter() {
        let html = r#"<meta name="twitter:image" content="/t.png">
            <meta property="og:image" content="/o.png">"#;
        assert_eq!(
            card(html).image.as_deref(),
            Some("https://example.com/o.png")
        );
    }

    #[test]
    fn title_tag_is_the_last_resort() {
        let card = card("<head><TITLE>\n  Spaced \n  out &amp; in </TITLE></head>");
        assert_eq!(card.title.as_deref(), Some("Spaced out & in"));
    }

    #[test]
    fn title_and_description_collapse_whitespace() {
        let html = "<meta property=\"og:title\" content=\"  a \n  b \"><meta property=\"og:description\" content=\"x\t\ty   z\">";
        let card = card(html);
        assert_eq!(card.title.as_deref(), Some("a b"));
        assert_eq!(card.description.as_deref(), Some("x y z"));
    }

    #[test]
    fn empty_fields_are_none() {
        let card =
            card(r#"<meta property="og:title" content="  "><meta property="og:image" content="">"#);
        assert_eq!(card, Card::default());
    }

    #[test]
    fn tags_after_head_are_ignored() {
        let html = r#"<head><meta property="og:title" content="In"></head>
            <body><meta property="og:image" content="/late.png"><meta property="og:site_name" content="Late"></body>"#;
        let card = card(html);
        assert_eq!(card.title.as_deref(), Some("In"));
        assert_eq!(card.image, None);
        assert_eq!(card.site, None);
    }

    #[test]
    fn uppercase_tags_and_attributes() {
        let html = r#"<HEAD><META PROPERTY="og:title" CONTENT="Loud"><META Property="og:image" Content="/L.png"></HEAD>"#;
        let card = card(html);
        assert_eq!(card.title.as_deref(), Some("Loud"));
        assert_eq!(card.image.as_deref(), Some("https://example.com/L.png"));
    }

    #[test]
    fn first_tag_wins() {
        let html =
            r#"<meta property="og:title" content="One"><meta property="og:title" content="Two">"#;
        assert_eq!(card(html).title.as_deref(), Some("One"));
    }

    #[test]
    fn head_of_stops_at_a_late_head_end() {
        let mut body = b"<html><head>".to_vec();
        body.resize(550 * 1024, b' ');
        body.extend_from_slice(b"</HEAD><body>tail");
        let head = head_of(&body, MAX_BYTES);
        assert_eq!(head.len(), MAX_BYTES);
        let head = head_of(&body, 600 * 1024);
        assert!(head.ends_with(b"</HEAD>"));
        assert_eq!(head.len(), 550 * 1024 + HEAD_END.len());
    }

    #[test]
    fn head_of_caps_a_body_without_head_end() {
        let body = vec![b'x'; 600 * 1024];
        assert_eq!(head_of(&body, MAX_BYTES).len(), MAX_BYTES);
        assert_eq!(head_of(b"abc", MAX_BYTES), b"abc");
        assert_eq!(head_of(b"", MAX_BYTES), b"");
    }

    #[test]
    fn meta_past_the_cap_is_not_seen() {
        let mut body = vec![b' '; MAX_BYTES];
        body.extend_from_slice(br#"<meta property="og:title" content="Far">"#);
        let html = String::from_utf8_lossy(head_of(&body, MAX_BYTES)).into_owned();
        assert_eq!(card(&html).title, None);
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("valid test ip")
    }

    #[test]
    fn non_global_addresses_are_rejected() {
        for text in [
            "127.0.0.1",
            "10.1.2.3",
            "172.20.0.1",
            "192.168.1.1",
            "100.64.0.1",
            "169.254.169.254",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fd00::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
        ] {
            assert!(!global(ip(text)), "{text}");
        }
    }

    #[test]
    fn global_addresses_pass() {
        for text in [
            "93.184.216.34",
            "2606:4700::1111",
            "100.128.0.1",
            "172.32.0.1",
            "::ffff:93.184.216.34",
        ] {
            assert!(global(ip(text)), "{text}");
        }
    }

    #[test]
    fn public_rejects_local_names_without_a_lookup() {
        for page in [
            "http://localhost/",
            "http://LOCALHOST:8080/",
            "http://localhost./",
            "http://a.localhost/",
            "http://printer.local/",
            "https://db.internal/",
            "ftp://93.184.216.34/",
            "http://127.0.0.1/",
            "http://[::1]/",
            "http://[::ffff:10.0.0.1]/",
            "http://2130706433/",
        ] {
            let url = Url::parse(page).expect("valid test url");
            assert!(!public(&url), "{page}");
        }
    }

    #[test]
    fn public_accepts_a_public_ip_literal() {
        let url = Url::parse("https://93.184.216.34/x").expect("valid test url");
        assert!(public(&url));
    }

    #[test]
    fn fetch_refuses_private_addresses_without_connecting() {
        let client = Client::new();
        assert_eq!(
            fetch(&client, "http://127.0.0.1:1/").unwrap_err(),
            "not a public address"
        );
    }

    fn resolve(name: &str) -> Result<Vec<SocketAddr>, String> {
        use std::task::{Context, Poll, Waker};

        let name = name.parse::<Name>().map_err(|error| error.to_string())?;
        let mut future = PublicOnly.resolve(name);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => result
                .map(Iterator::collect)
                .map_err(|error| error.to_string()),
            Poll::Pending => Err("pending".to_string()),
        }
    }

    #[test]
    fn resolver_refuses_local_names() {
        assert_eq!(resolve("localhost").unwrap_err(), "not a public address");
        assert_eq!(resolve("127.0.0.1").unwrap_err(), "not a public address");
        assert_eq!(resolve("10.0.0.1").unwrap_err(), "not a public address");
    }

    #[test]
    fn resolver_keeps_public_addresses() {
        let addrs = resolve("93.184.216.34").expect("public literal resolves");
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].ip(), ip("93.184.216.34"));
    }

    #[test]
    fn client_builds() {
        assert!(client().is_ok());
    }

    #[test]
    fn fetch_rejects_non_web_pages() {
        let client = Client::new();
        for page in ["ftp://example.com/", "not a url", "javascript:alert(1)"] {
            assert_eq!(fetch(&client, page).unwrap_err(), "not a web page");
        }
    }
}
