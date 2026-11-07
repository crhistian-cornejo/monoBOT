//! The guarded fetch behind the agents' web tools: every request, redirects
//! included, goes only to public addresses, with limits on size and time.
//! Ports src-tauri/src/link_preview.rs (`fetch_resource_within`) and the PDF
//! hunt in src-tauri/src/papers.rs (`download_pdf`).

use std::collections::{HashSet, VecDeque};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::time::Duration;

use scraper::{Html, Selector};
use url::{Host, Url};

pub(super) const PAGE_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
pub(super) const FILE_TIMEOUT: Duration = Duration::from_secs(120);
pub(super) const MAX_FILE_BYTES: usize = 200 * 1024 * 1024;
const PDF_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_PDF_BYTES: usize = 40 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const USER_AGENT: &str = "Mozilla/5.0 (compatible; MonoBOT-LinkPreview/1.0)";

/// What a guarded request got back.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub final_url: Url,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

/// The public http(s) address in `raw`, or why it is refused.
pub fn parse_public_url(raw: &str) -> Result<Url, String> {
    let value = raw.trim();
    if value.is_empty() || value.len() > 8192 {
        return Err("The web address is invalid".into());
    }
    let url = Url::parse(value).map_err(|_| "The web address is invalid".to_string())?;
    if !is_http_url(&url)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return Err("The web address is invalid".into());
    }
    validate_public_target(&url)?;
    Ok(url)
}

/// Fetches `url`, following up to five redirects, each checked as a public
/// address before a request is made. Errors carry the HTTP status in
/// parentheses, as `is_refusal` expects.
pub fn fetch_within(
    mut url: Url,
    accept: &str,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Fetched, String> {
    let agent = monobot_http::agent().timeout(timeout).redirects(0).build();

    for _ in 0..=MAX_REDIRECTS {
        validate_public_target(&url)?;
        let response = match agent
            .get(url.as_str())
            .set("Accept", accept)
            .set("User-Agent", USER_AGENT)
            .call()
        {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) if is_redirect(status) => response,
            Err(ureq::Error::Status(status, _)) => {
                return Err(format!("The web request failed ({status})"));
            }
            Err(_) => return Err("Could not load the web address".into()),
        };

        if is_redirect(response.status()) {
            let location = response
                .header("Location")
                .ok_or_else(|| "A redirect has no destination".to_string())?;
            url = url
                .join(location)
                .map_err(|_| "A redirect is invalid".to_string())?;
            if !is_http_url(&url) {
                return Err("A redirect is not HTTP".into());
            }
            continue;
        }

        if !(200..300).contains(&response.status()) {
            return Err(format!("The web request failed ({})", response.status()));
        }
        if let Some(length) = response
            .header("Content-Length")
            .and_then(|value| value.parse::<usize>().ok())
        {
            if length > max_bytes {
                return Err("The response is too large".into());
            }
        }
        let content_type = response.header("Content-Type").map(str::to_string);
        let mut reader = response.into_reader().take(max_bytes as u64 + 1);
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read the response".to_string())?;
        if bytes.len() > max_bytes {
            return Err("The response is too large".into());
        }
        return Ok(Fetched {
            final_url: url,
            content_type,
            bytes,
        });
    }

    Err("Too many redirects".into())
}

fn is_http_url(url: &Url) -> bool {
    url.scheme() == "https" || url.scheme() == "http"
}

fn validate_public_target(url: &Url) -> Result<(), String> {
    if !is_http_url(url) || !url.username().is_empty() || url.password().is_some() {
        return Err("The web address is invalid".into());
    }
    let host = match url
        .host()
        .ok_or_else(|| "The web address is invalid".to_string())?
    {
        Host::Ipv4(ip) => return public_ip_result(IpAddr::V4(ip)),
        Host::Ipv6(ip) => return public_ip_result(IpAddr::V6(ip)),
        Host::Domain(host) => host.trim_end_matches('.').to_ascii_lowercase(),
    };
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return Err("Local addresses cannot be read".into());
    }

    let port = url
        .port_or_known_default()
        .ok_or_else(|| "The web address has no port".to_string())?;
    let addresses: Vec<_> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| "Could not resolve the host".to_string())?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err("Private addresses cannot be read".into());
    }
    Ok(())
}

fn public_ip_result(ip: IpAddr) -> Result<(), String> {
    if is_public_ip(ip) {
        Ok(())
    } else {
        Err("Private addresses cannot be read".into())
    }
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = ip.to_ipv4() {
        return is_public_ipv4(ipv4);
    }
    let segments = ip.segments();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || segments[0] & 0xfe00 == 0xfc00
        || segments[0] & 0xffc0 == 0xfe80
        || segments[0] & 0xffc0 == 0xfec0
        || (segments[0] == 0x2001 && segments[1] == 0x0db8))
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Text the app can read: HTML, XML, JSON, plain text. A NUL byte early on
/// means binary whatever the server claims.
pub fn is_text(content_type: Option<&str>, bytes: &[u8]) -> bool {
    if bytes.iter().take(1024).any(|&byte| byte == 0) {
        return false;
    }
    match content_type.map(|kind| kind.to_ascii_lowercase()) {
        Some(kind) => {
            kind.starts_with("text/")
                || kind.contains("html")
                || kind.contains("xml")
                || kind.contains("json")
        }
        None => looks_like_html(bytes),
    }
}

pub fn looks_like_html(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    head.contains("<!doctype html") || head.contains("<html") || head.contains("<head")
}

/// What a site says when it will not hand a file to a program: a refusal, or
/// a page that asks for a human check. MonoBOT does not try to pass these; it
/// says so, and the user can fetch the file from the page themselves.
fn is_refusal(message: &str) -> bool {
    ["(401)", "(403)", "(429)", "(451)", "(503)"]
        .iter()
        .any(|code| message.contains(code))
}

fn looks_like_challenge(html: &str) -> bool {
    let page = html.to_ascii_lowercase();
    [
        "not a bot",
        "anubis_challenge",
        "just a moment...",
        "cf-browser-verification",
        "challenge-platform",
        "attention required! | cloudflare",
        "g-recaptcha",
        "h-captcha",
        "are you a robot",
        "verify you are human",
    ]
    .iter()
    .any(|sign| page.contains(sign))
}

/// The values of `attribute` on the elements `selector` finds that `keep`
/// accepts, in document order.
fn attribute_values(
    doc: &Html,
    selector: &str,
    attribute: &str,
    keep: impl Fn(&scraper::node::Element) -> bool,
) -> Vec<String> {
    let Ok(selector) = Selector::parse(selector) else {
        return Vec::new();
    };
    doc.select(&selector)
        .filter(|element| keep(element.value()))
        .filter_map(|element| element.value().attr(attribute))
        .map(|value| value.trim().to_string())
        .collect()
}

/// The PDFs an HTML page points to, best first: the `citation_pdf_url` that
/// repositories and publishers put in their pages for readers' tools, a
/// `link rel=alternate type=application/pdf`, then links to a `.pdf` file.
pub fn pdf_links(html: &str, base: &Url) -> Vec<Url> {
    let doc = Html::parse_document(html);
    let metas = attribute_values(&doc, "meta[name][content]", "content", |meta| {
        meta.attr("name")
            .is_some_and(|name| name.eq_ignore_ascii_case("citation_pdf_url"))
    });
    let alternates = attribute_values(&doc, "link[type][href]", "href", |link| {
        link.attr("type")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("application/pdf"))
    });
    let anchors = attribute_values(&doc, "a[href]", "href", |anchor| {
        let path = anchor
            .attr("href")
            .unwrap_or("")
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        path.ends_with(".pdf") || path.contains("/bitstream/") && path.contains(".pdf")
    });
    let mut out: Vec<Url> = Vec::new();
    for href in metas.into_iter().chain(alternates).chain(anchors) {
        if let Ok(url) = base.join(&href) {
            if matches!(url.scheme(), "http" | "https") && !out.contains(&url) {
                out.push(url);
            }
        }
    }
    out.truncate(4);
    out
}

/// Gets the PDF a link leads to: the file itself, or the PDF its page points
/// to (up to two pages deep). Errors starting `blocked:` name a host that
/// refuses programs; nothing here tries to get past that.
pub fn download_pdf(start: Url) -> Result<(Vec<u8>, Url), String> {
    let mut queue = VecDeque::from([(start, 0u8)]);
    let mut seen = HashSet::new();
    let mut blocked: Option<String> = None;
    let mut last = "That link is a web page, not the PDF".to_string();
    while let Some((url, depth)) = queue.pop_front() {
        if !seen.insert(url.as_str().to_owned()) || seen.len() > 8 {
            continue;
        }
        let host = url.host_str().unwrap_or("that site").to_string();
        match fetch_within(
            url.clone(),
            "application/pdf,text/html;q=0.8,*/*;q=0.1",
            MAX_PDF_BYTES,
            PDF_TIMEOUT,
        ) {
            Ok(file) if file.bytes.starts_with(b"%PDF") => return Ok((file.bytes, file.final_url)),
            Ok(page) => {
                let html = String::from_utf8_lossy(&page.bytes);
                if looks_like_challenge(&html) {
                    blocked.get_or_insert(page.final_url.host_str().unwrap_or(&host).to_string());
                    continue;
                }
                if depth < 2 {
                    for link in pdf_links(&html, &page.final_url).into_iter().take(3) {
                        queue.push_back((link, depth + 1));
                    }
                }
            }
            Err(message) if is_refusal(&message) => {
                blocked.get_or_insert(host);
            }
            Err(message) => last = message,
        }
    }
    Err(match blocked {
        Some(host) => format!("blocked:{host}"),
        None => last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_text_from_binary() {
        assert!(is_text(Some("text/html; charset=utf-8"), b"<html>"));
        assert!(is_text(Some("application/json"), b"{}"));
        assert!(!is_text(Some("application/pdf"), b"%PDF-1.7"));
        assert!(!is_text(Some("text/plain"), b"PK\x03\x04\0\0"));
        assert!(is_text(None, b"<!DOCTYPE html><html>"));
    }

    #[test]
    fn refuses_private_addresses() {
        for url in [
            "http://127.0.0.1/",
            "http://localhost:8080/",
            "http://192.168.1.1/",
            "http://10.0.0.5/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://printer.local/",
            "file:///etc/passwd",
            "ftp://example.org/",
            "https://user:secret@example.org/",
            "",
        ] {
            assert!(parse_public_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn a_hop_to_a_private_address_is_refused_before_any_request() {
        let url = Url::parse("http://127.0.0.1:9/").unwrap();
        let error = fetch_within(url, "*/*", 1024, Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("Private"), "{error}");
    }

    #[test]
    fn finds_the_pdfs_a_page_points_to_best_first() {
        let base = Url::parse("https://repo.example/handle/1").unwrap();
        let html = r#"<html><head>
            <link rel="alternate" type="application/pdf" href="/alt/paper.pdf">
            <meta name="citation_pdf_url" content="/content/pond.pdf">
          </head><body>
            <a href="/files/guide.PDF?download=1">guide</a>
            <a href="/bitstream/handle/1/x.pdf?seq=1">bitstream</a>
            <a href="/about">about</a>
            <a href="mailto:a@b.c">mail</a>
          </body></html>"#;
        let links: Vec<String> = pdf_links(html, &base)
            .into_iter()
            .map(|url| url.to_string())
            .collect();
        assert_eq!(
            links,
            [
                "https://repo.example/content/pond.pdf",
                "https://repo.example/alt/paper.pdf",
                "https://repo.example/files/guide.PDF?download=1",
                "https://repo.example/bitstream/handle/1/x.pdf?seq=1",
            ]
        );
    }

    #[test]
    fn spots_pages_that_ask_for_a_human() {
        assert!(looks_like_challenge("<title>Just a moment...</title>"));
        assert!(looks_like_challenge("Please verify you are human"));
        assert!(!looks_like_challenge("<p>A paper about settling ponds</p>"));
        assert!(is_refusal("The web request failed (403)"));
        assert!(!is_refusal("The web request failed (404)"));
    }
}
