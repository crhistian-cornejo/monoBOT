//! Scholarly papers for the research agents: the open metadata APIs and the
//! open-access PDFs, fetched here because the webview's CSP is narrow, and kept
//! in the app's cache folder so a paper is read once and summarised from disk.
//! Nothing is written to the user's Downloads.
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use url::Url;

use crate::link_preview::{fetch_resource_within, parse_public_url};

const API_TIMEOUT: Duration = Duration::from_secs(15);
const PDF_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_API_BYTES: usize = 4 * 1024 * 1024;
const MAX_PDF_BYTES: usize = 40 * 1024 * 1024;
/// Open scholarly APIs that need no key. Anything else goes through the
/// agent's own web tools, not through this command.
const API_HOSTS: [&str; 6] = [
    "api.openalex.org",
    "api.crossref.org",
    "export.arxiv.org",
    "api.archives-ouvertes.fr",
    "api.semanticscholar.org",
    "www.ebi.ac.uk",
];

fn cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("papers");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn key_of(key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    key.trim().to_ascii_lowercase().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// What a site says when it will not hand a file to a program: a refusal, or
/// a page that asks for a human check. MonoBOT does not try to pass these; it
/// says so, and the user can fetch the paper from the page themselves.
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

/// The value of an attribute in one HTML tag (quoted values only).
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        let before_ok = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric() && lower.as_bytes()[at - 1] != b'-';
        let rest = &tag[at + name.len()..];
        let trimmed = rest.trim_start();
        if before_ok && trimmed.starts_with('=') {
            let value = trimmed[1..].trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &value[1..];
                let end = inner.find(quote)?;
                return Some(inner[..end].replace("&amp;", "&").trim().to_string());
            }
        }
        from = at + name.len();
    }
    None
}

/// The PDFs an HTML page points to, best first: the `citation_pdf_url` that
/// repositories and publishers put in their pages for readers' tools, a
/// `link rel=alternate type=application/pdf`, then links to a `.pdf` file.
fn pdf_links(html: &str, base: &Url) -> Vec<Url> {
    let lower = html.to_ascii_lowercase();
    let mut metas = Vec::new();
    let mut alternates = Vec::new();
    let mut anchors = Vec::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find('<') {
        let start = from + found;
        let Some(len) = lower[start..].find('>') else { break };
        let tag = &html[start..start + len + 1];
        let name = lower[start + 1..].split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or("");
        match name {
            "meta" => {
                if attr(tag, "name").is_some_and(|n| n.eq_ignore_ascii_case("citation_pdf_url")) {
                    if let Some(content) = attr(tag, "content") {
                        metas.push(content);
                    }
                }
            }
            "link" => {
                if attr(tag, "type").is_some_and(|t| t.eq_ignore_ascii_case("application/pdf")) {
                    if let Some(href) = attr(tag, "href") {
                        alternates.push(href);
                    }
                }
            }
            "a" => {
                if let Some(href) = attr(tag, "href") {
                    let path = href.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
                    if path.ends_with(".pdf") || path.contains("/bitstream/") && path.contains(".pdf") {
                        anchors.push(href);
                    }
                }
            }
            _ => {}
        }
        from = start + len + 1;
    }
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
pub(crate) fn download_pdf(start: Url) -> Result<(Vec<u8>, Url), String> {
    let mut queue = std::collections::VecDeque::from([(start, 0u8)]);
    let mut seen = std::collections::HashSet::new();
    let mut blocked: Option<String> = None;
    let mut last = "That link is a web page, not the PDF".to_string();
    while let Some((url, depth)) = queue.pop_front() {
        if !seen.insert(url.as_str().to_owned()) || seen.len() > 8 {
            continue;
        }
        let host = url.host_str().unwrap_or("that site").to_string();
        match fetch_resource_within(
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

/// GET one of the open scholarly APIs and return its body as text.
#[tauri::command]
pub async fn paper_api(url: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let parsed = Url::parse(url.trim()).map_err(|_| "Invalid paper API URL".to_string())?;
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if parsed.scheme() != "https" || !API_HOSTS.contains(&host.as_str()) {
            return Err("That host is not an open scholarly API".into());
        }
        let page = fetch_resource_within(
            parsed,
            "application/json,application/atom+xml;q=0.9",
            MAX_API_BYTES,
            API_TIMEOUT,
        )?;
        String::from_utf8(page.bytes).map_err(|_| "The API did not answer with text".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedPdf {
    /// Where the PDF sits in the cache.
    path: String,
    bytes: usize,
    /// It was already there, so nothing was downloaded.
    cached: bool,
}

/// Downloads a paper's open-access PDF into the cache (once per `key`, which
/// is the DOI or the work id) after checking it really is a PDF.
#[tauri::command]
pub async fn paper_pdf(app: AppHandle, key: String, url: String) -> Result<CachedPdf, String> {
    let target = cache_dir(&app)?.join(format!("{}.pdf", key_of(&key)));
    tauri::async_runtime::spawn_blocking(move || {
        if let Ok(meta) = fs::metadata(&target) {
            if meta.len() > 0 {
                return Ok(CachedPdf {
                    path: target.to_string_lossy().into_owned(),
                    bytes: meta.len() as usize,
                    cached: true,
                });
            }
        }
        let parsed = parse_public_url(&url)?;
        let (bytes, _) = download_pdf(parsed)?;
        let partial = target.with_extension("part");
        fs::write(&partial, &bytes).map_err(|e| e.to_string())?;
        fs::rename(&partial, &target).map_err(|e| e.to_string())?;
        Ok(CachedPdf {
            path: target.to_string_lossy().into_owned(),
            bytes: bytes.len(),
            cached: false,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The text extracted from a cached paper, written once by the app.
#[tauri::command]
pub fn paper_text_read(app: AppHandle, key: String) -> Result<Option<String>, String> {
    let path = cache_dir(&app)?.join(format!("{}.json", key_of(&key)));
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub fn paper_text_write(app: AppHandle, key: String, text: String) -> Result<(), String> {
    if text.len() > 16 * 1024 * 1024 {
        return Err("The paper's text is too large to keep".into());
    }
    fs::write(cache_dir(&app)?.join(format!("{}.json", key_of(&key))), text)
        .map_err(|e| e.to_string())
}

/// Copies a PDF the user chose (one they got through their library, say) into
/// the cache as this paper's copy, after checking it really is a PDF.
fn read_pdf_file(source: &std::path::Path) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(source).map_err(|_| "That file could not be read".to_string())?;
    if !meta.is_file() {
        return Err("That is not a file".into());
    }
    if meta.len() as usize > MAX_PDF_BYTES {
        return Err("That PDF is too large (over 40 MB)".into());
    }
    let bytes = fs::read(source).map_err(|_| "That file could not be read".to_string())?;
    if !bytes.starts_with(b"%PDF") {
        return Err("That file is not a PDF".into());
    }
    Ok(bytes)
}

fn store_pdf_from(dir: &std::path::Path, key: &str, source: &std::path::Path) -> Result<CachedPdf, String> {
    let bytes = read_pdf_file(source)?;
    let target = dir.join(format!("{}.pdf", key_of(key)));
    let partial = target.with_extension("part");
    fs::write(&partial, &bytes).map_err(|e| e.to_string())?;
    fs::rename(&partial, &target).map_err(|e| e.to_string())?;
    Ok(CachedPdf {
        path: target.to_string_lossy().into_owned(),
        bytes: bytes.len(),
        cached: false,
    })
}

/// A PDF on this computer, checked, for the app to read before it is kept.
#[tauri::command]
pub async fn paper_pdf_file(path: String) -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(move || read_pdf_file(std::path::Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
        .map(tauri::ipc::Response::new)
}

#[tauri::command]
pub async fn paper_pdf_import(app: AppHandle, key: String, path: String) -> Result<CachedPdf, String> {
    let dir = cache_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || store_pdf_from(&dir, &key, std::path::Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// Writes a rendered page of a paper (a PNG, in base64) to the cache and
/// returns its path, so an agent can show it in the chat as a local image.
fn store_page_image(
    dir: &std::path::Path,
    key: &str,
    page: u32,
    png_base64: &str,
) -> Result<PathBuf, String> {
    use base64::Engine;
    if page == 0 || page > 2000 {
        return Err("That is not a page number".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.trim())
        .map_err(|_| "The image is not valid base64".to_string())?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("The image is not a PNG".into());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("The page image is too large".into());
    }
    let images = dir.join("images");
    fs::create_dir_all(&images).map_err(|e| e.to_string())?;
    let path = images.join(format!("{}-p{page}.png", key_of(key)));
    fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

#[tauri::command]
pub fn paper_image_write(
    app: AppHandle,
    key: String,
    page: u32,
    png_base64: String,
) -> Result<String, String> {
    store_page_image(&cache_dir(&app)?, &key, page, &png_base64)
        .map(|path| path.to_string_lossy().into_owned())
}

/// A folder or file name that is safe on macOS and Windows: no separators or
/// characters Windows refuses, no control characters, no trailing dots or
/// spaces, no reserved device names, and not too long.
pub(crate) fn safe_name(raw: &str, fallback: &str, max: usize) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { ' ' } else { c })
        .collect();
    let mut name = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    name = name.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if name.chars().count() > max {
        name = name.chars().take(max).collect::<String>().trim_end().to_string();
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.chars().last().is_some_and(|c| c.is_ascii_digit() && c != '0'));
    if name.is_empty() {
        return fallback.to_string();
    }
    if reserved {
        return format!("_{name}");
    }
    name
}

/// Copies a cached paper into the user's Documents, in a folder for the agent
/// that read it: `Documents/MonoBOT/<agent>/<name>.pdf`. The same paper read
/// again lands on the same file; a different one that shares its name gets a
/// number.
fn export_pdf(
    cached: &std::path::Path,
    documents: &std::path::Path,
    agent: &str,
    name: &str,
) -> Result<PathBuf, String> {
    let bytes = fs::read(cached).map_err(|_| "That paper is not in the cache".to_string())?;
    let folder = documents.join("MonoBOT").join(safe_name(agent, "General", 60));
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let stem = safe_name(name, "Paper", 120);
    for attempt in 1..=50 {
        let file = if attempt == 1 {
            format!("{stem}.pdf")
        } else {
            format!("{stem} ({attempt}).pdf")
        };
        let target = folder.join(file);
        match fs::read(&target) {
            Ok(existing) if existing == bytes => return Ok(target),
            Ok(_) => continue,
            Err(_) => {
                fs::write(&target, &bytes).map_err(|e| e.to_string())?;
                return Ok(target);
            }
        }
    }
    Err("Too many files with that name".into())
}

/// Puts a copy of a paper's PDF where the user can see it: Documents, in a
/// folder named for the agent that fetched it.
#[tauri::command]
pub fn paper_export(app: AppHandle, key: String, agent: String, name: String) -> Result<String, String> {
    let cached = cache_dir(&app)?.join(format!("{}.pdf", key_of(&key)));
    let documents = app.path().document_dir().map_err(|e| e.to_string())?;
    export_pdf(&cached, &documents, &agent, &name).map(|path| path.to_string_lossy().into_owned())
}

/// The cached PDF's bytes, for the app's own viewer.
#[tauri::command]
pub fn paper_pdf_bytes(app: AppHandle, key: String) -> Result<tauri::ipc::Response, String> {
    let path = cache_dir(&app)?.join(format!("{}.pdf", key_of(&key)));
    fs::read(path)
        .map(tauri::ipc::Response::new)
        .map_err(|_| "That paper is not in the cache".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;


    /// Real open-access papers through the real network, by their DOI links.
    /// Run by hand: `cargo test -p monocode live_open_papers -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_open_papers() {
        for link in [
            "https://arxiv.org/abs/1706.03762",
            "https://doi.org/10.1371/journal.pone.0000308",
            "https://doi.org/10.5194/hess-25-1-2021",
            "https://doi.org/10.3390/w13020125",
            "https://doi.org/10.1038/s41467-020-19160-7",
            "https://doi.org/10.1016/j.ejrh.2023.101381",
        ] {
            let url = Url::parse(link).unwrap();
            match download_pdf(url) {
                Ok((bytes, at)) => eprintln!("OK      {link} -> {} ({} bytes)", at, bytes.len()),
                Err(why) => eprintln!("NO      {link}: {why}"),
            }
        }
    }

    #[test]
    fn finds_the_pdf_a_repository_page_points_to() {
        let base = Url::parse("https://repo.example.org/handle/1/2894").unwrap();
        let page = r#"<html><head>
            <meta name="DC.title" content="x">
            <meta name='citation_pdf_url' content='/bitstream/handle/1/2894/paper.pdf?sequence=1&amp;isAllowed=y'>
            <link rel="alternate" type="application/pdf" href="https://cdn.example.org/alt.pdf">
          </head><body><a href="docs/extra.PDF">Extra</a><a href="/about">About</a></body></html>"#;
        let links: Vec<String> = pdf_links(page, &base).iter().map(|u| u.to_string()).collect();
        assert_eq!(
            links,
            [
                "https://repo.example.org/bitstream/handle/1/2894/paper.pdf?sequence=1&isAllowed=y",
                "https://cdn.example.org/alt.pdf",
                "https://repo.example.org/handle/1/docs/extra.PDF",
            ]
        );
        assert!(pdf_links("<html><a href=\"/about\">x</a></html>", &base).is_empty());
        assert!(pdf_links("<meta name=\"citation_pdf_url\" content=\"javascript:alert(1)\">", &base).is_empty());
    }

    #[test]
    fn knows_a_page_that_asks_for_a_human_and_a_refusal() {
        assert!(looks_like_challenge("<title>Making sure you&#39;re not a bot!</title>"));
        assert!(looks_like_challenge("<title>Just a moment...</title>"));
        assert!(!looks_like_challenge("<html><body>An article about robots.</body></html>"));
        assert!(is_refusal("Link preview request failed (403)"));
        assert!(!is_refusal("Link preview request failed (404)"));
    }

    #[test]
    fn reads_quoted_attributes_without_confusing_similar_names() {
        let tag = r#"<meta data-name="no" name="citation_pdf_url" content="a&amp;b">"#;
        assert_eq!(attr(tag, "name").as_deref(), Some("citation_pdf_url"));
        assert_eq!(attr(tag, "content").as_deref(), Some("a&b"));
        assert_eq!(attr(tag, "href"), None);
    }

    #[test]
    fn a_chosen_pdf_is_copied_into_the_cache_only_when_it_is_a_pdf() {
        let dir = std::env::temp_dir().join(format!("monobot-papers-import-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let good = dir.join("paper.pdf");
        fs::write(&good, b"%PDF-1.7 body").unwrap();
        let stored = store_pdf_from(&dir, "10.1234/x", &good).unwrap();
        assert_eq!(fs::read(&stored.path).unwrap(), b"%PDF-1.7 body");
        assert!(stored.path.ends_with(&format!("{}.pdf", key_of("10.1234/x"))));
        let page = dir.join("page.pdf");
        fs::write(&page, b"<html>login</html>").unwrap();
        assert_eq!(store_pdf_from(&dir, "10.1234/y", &page).unwrap_err(), "That file is not a PDF");
        assert!(store_pdf_from(&dir, "10.1234/y", &dir.join("missing.pdf")).is_err());
        assert!(store_pdf_from(&dir, "10.1234/y", &dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_page_image_is_kept_only_when_it_is_a_png() {
        use base64::Engine;
        let dir = std::env::temp_dir().join(format!("monobot-papers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nrest");
        let path = store_page_image(&dir, "10.1234/x", 3, &png).unwrap();
        assert!(path.ends_with(format!("{}-p3.png", key_of("10.1234/x"))));
        assert_eq!(fs::read(&path).unwrap(), b"\x89PNG\r\n\x1a\nrest");
        let html = base64::engine::general_purpose::STANDARD.encode(b"<html>");
        assert!(store_page_image(&dir, "10.1234/x", 3, &html).is_err());
        assert!(store_page_image(&dir, "10.1234/x", 0, &png).is_err());
        assert!(store_page_image(&dir, "10.1234/x", 1, "not base64!").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_are_safe_on_macos_and_windows() {
        assert_eq!(safe_name("Research", "x", 60), "Research");
        assert_eq!(safe_name("a/b\\c:d*e?f\"g<h>i|j", "x", 60), "a b c d e f g h i j");
        assert_eq!(safe_name("  ..Name.. ", "x", 60), "Name");
        assert_eq!(safe_name("???", "Agent", 60), "Agent");
        assert_eq!(safe_name("CON", "x", 60), "_CON");
        assert_eq!(safe_name("com1.txt", "x", 60), "_com1.txt");
        assert_eq!(safe_name("COM0", "x", 60), "COM0");
        assert_eq!(safe_name(&"a".repeat(200), "x", 60).len(), 60);
        assert_eq!(safe_name("Ünïcode — título", "x", 60), "Ünïcode — título");
    }

    #[test]
    fn a_paper_is_copied_into_documents_under_its_agent() {
        let root = std::env::temp_dir().join(format!("monobot-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let cached = root.join("cached.pdf");
        fs::write(&cached, b"%PDF one").unwrap();
        let docs = root.join("Documents");
        let first = export_pdf(&cached, &docs, "Research", "Coron et al. (2017) - The suite").unwrap();
        assert_eq!(first, docs.join("MonoBOT").join("Research").join("Coron et al. (2017) - The suite.pdf"));
        assert_eq!(fs::read(&first).unwrap(), b"%PDF one");
        // The same paper read again is the same file.
        assert_eq!(export_pdf(&cached, &docs, "Research", "Coron et al. (2017) - The suite").unwrap(), first);
        // Another paper with the same name does not overwrite it.
        let other = root.join("other.pdf");
        fs::write(&other, b"%PDF two").unwrap();
        let second = export_pdf(&other, &docs, "Research", "Coron et al. (2017) - The suite").unwrap();
        assert!(second.ends_with("Coron et al. (2017) - The suite (2).pdf"));
        assert_eq!(fs::read(&first).unwrap(), b"%PDF one");
        // Each agent has its own folder.
        let banana = export_pdf(&cached, &docs, "Banana", "Coron et al. (2017) - The suite").unwrap();
        assert!(banana.starts_with(docs.join("MonoBOT").join("Banana")));
        assert!(export_pdf(&root.join("missing.pdf"), &docs, "Research", "x").is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_key_is_the_same_whatever_its_case_or_spacing() {
        assert_eq!(key_of(" 10.1016/J.ENVSOFT.2017.05.002 "), key_of("10.1016/j.envsoft.2017.05.002"));
        assert_ne!(key_of("a"), key_of("b"));
    }
}
