//! What any agent can do on the web through the app: read a page, or download a
//! file. Every request, redirects included, goes only to public addresses, with
//! limits on size and time, like link previews and papers.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use url::Url;

use crate::link_preview::{fetch_resource_within, parse_public_url};
use crate::papers::{download_pdf, safe_name};

const PAGE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
const FILE_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_FILE_BYTES: usize = 200 * 1024 * 1024;

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WebPage {
    /// Where the redirects ended.
    final_url: String,
    content_type: Option<String>,
    /// The page's HTML or text; absent for a PDF or another binary file.
    text: Option<String>,
    is_pdf: bool,
    bytes: usize,
}

/// The page at `url`, for the app to turn into readable text and metadata.
#[tauri::command]
pub async fn web_page(url: String) -> Result<WebPage, String> {
    tauri::async_runtime::spawn_blocking(move || fetch_page(&url))
        .await
        .map_err(|error| error.to_string())?
}

fn fetch_page(url: &str) -> Result<WebPage, String> {
    let url = parse_public_url(url)?;
    let page = fetch_resource_within(
        url,
        "text/html,application/xhtml+xml,text/plain;q=0.9,application/pdf;q=0.8,*/*;q=0.1",
        MAX_PAGE_BYTES,
        PAGE_TIMEOUT,
    )?;
    let is_pdf = page.bytes.starts_with(b"%PDF");
    let text = (!is_pdf && is_text(page.content_type.as_deref(), &page.bytes))
        .then(|| String::from_utf8_lossy(&page.bytes).into_owned());
    Ok(WebPage {
        final_url: page.final_url.to_string(),
        content_type: page.content_type,
        text,
        is_pdf,
        bytes: page.bytes.len(),
    })
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Downloaded {
    path: String,
    final_url: String,
    content_type: Option<String>,
    bytes: usize,
    is_pdf: bool,
}

/// Downloads `url`. A PDF lands in the app's cache, for the app to register
/// as a paper (which puts a well-named copy in Documents). Any other file
/// goes straight to `Documents/MonoBOT/<agent>/`. A link to a page that leads
/// to a PDF (an article's landing page) downloads that PDF.
#[tauri::command]
pub async fn web_download(
    app: AppHandle,
    url: String,
    agent: String,
    name: Option<String>,
) -> Result<Downloaded, String> {
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let documents = app.path().document_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let url = parse_public_url(&url)?;
        let file = fetch_resource_within(url.clone(), "*/*", MAX_FILE_BYTES, FILE_TIMEOUT)?;
        let (bytes, final_url, content_type) =
            if is_text(file.content_type.as_deref(), &file.bytes) && looks_like_html(&file.bytes) {
                // An article's page: follow it to its PDF, or say it is a page.
                match download_pdf(url) {
                    Ok((pdf, at)) => (pdf, at, Some("application/pdf".to_string())),
                    Err(error) if error.starts_with("blocked:") => {
                        return Err(format!(
                            "{} blocks programs from downloading; ask the user to download it and give you the file",
                            &error["blocked:".len()..]
                        ))
                    }
                    Err(_) => {
                        return Err(
                            "That link is a web page, not a file: read it with web.read".into(),
                        )
                    }
                }
            } else {
                (file.bytes, file.final_url, file.content_type)
            };
        let is_pdf = bytes.starts_with(b"%PDF");
        let stem = file_stem(name.as_deref(), &final_url);
        let extension = if is_pdf {
            "pdf".to_string()
        } else {
            extension_of(&final_url, content_type.as_deref())
        };
        let folder = if is_pdf {
            cache.join("papers").join("downloads")
        } else {
            documents.join("MonoBOT").join(safe_name(&agent, "General", 60))
        };
        let path = save_unique(&folder, &stem, &extension, &bytes)?;
        Ok(Downloaded {
            path: path.to_string_lossy().into_owned(),
            final_url: final_url.to_string(),
            content_type,
            bytes: bytes.len(),
            is_pdf,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Copies a downloaded file into `Documents/MonoBOT/<agent>/`, for a PDF the
/// app could not read as a paper (a scan) and so did not put there itself.
#[tauri::command]
pub fn web_keep(app: AppHandle, path: String, agent: String) -> Result<String, String> {
    let source = PathBuf::from(&path);
    let downloads = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("papers")
        .join("downloads");
    // Only files this module downloaded, never an arbitrary path.
    if source.parent() != Some(downloads.as_path()) {
        return Err("Only a downloaded file can be kept".into());
    }
    let bytes = fs::read(&source).map_err(|_| "That download is gone".to_string())?;
    let documents = app.path().document_dir().map_err(|e| e.to_string())?;
    let folder = documents
        .join("MonoBOT")
        .join(safe_name(&agent, "General", 60));
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Download".into());
    let extension = source
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned())
        .unwrap_or_default();
    save_unique(&folder, &stem, &extension, &bytes).map(|path| path.to_string_lossy().into_owned())
}

/// Text the app can read: HTML, XML, JSON, plain text. A NUL byte early on
/// means binary whatever the server claims.
fn is_text(content_type: Option<&str>, bytes: &[u8]) -> bool {
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

fn looks_like_html(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    head.contains("<!doctype html") || head.contains("<html") || head.contains("<head")
}

/// The name to save under: the one asked for, else the URL's last segment.
fn file_stem(name: Option<&str>, url: &Url) -> String {
    let wanted = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            // An extension the agent typed is dropped; the content decides it.
            match name.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 => stem.to_string(),
                _ => name.to_string(),
            }
        });
    let from_url = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(|segment| {
            let decoded = percent_decode(segment);
            match decoded.rsplit_once('.') {
                Some((stem, _)) if !stem.is_empty() => stem.to_string(),
                _ => decoded,
            }
        })
        .filter(|segment| !segment.is_empty());
    safe_name(
        &wanted
            .or(from_url)
            .unwrap_or_else(|| url.host_str().unwrap_or("Download").to_string()),
        "Download",
        120,
    )
}

fn extension_of(url: &Url, content_type: Option<&str>) -> String {
    let from_type = content_type.map(|kind| {
        kind.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    });
    let known = match from_type.as_deref() {
        Some("text/html") => Some("html"),
        Some("text/plain") => Some("txt"),
        Some("text/csv") => Some("csv"),
        Some("application/json") => Some("json"),
        Some("application/zip") => Some("zip"),
        Some("image/png") => Some("png"),
        Some("image/jpeg") => Some("jpg"),
        Some("image/gif") => Some("gif"),
        Some("image/webp") => Some("webp"),
        Some("image/svg+xml") => Some("svg"),
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document") => {
            Some("docx")
        }
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet") => Some("xlsx"),
        Some("application/vnd.openxmlformats-officedocument.presentationml.presentation") => {
            Some("pptx")
        }
        _ => None,
    };
    if let Some(extension) = known {
        return extension.to_string();
    }
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .and_then(|segment| {
            segment
                .rsplit_once('.')
                .map(|(_, ext)| ext.to_ascii_lowercase())
        })
        .filter(|ext| {
            (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| "bin".to_string())
}

fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&segment[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Saves under `stem.ext`; the same bytes reuse the file, other bytes with
/// that name get a number.
fn save_unique(
    folder: &Path,
    stem: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let dot = if extension.is_empty() { "" } else { "." };
    for attempt in 1..=50 {
        let file = if attempt == 1 {
            format!("{stem}{dot}{extension}")
        } else {
            format!("{stem} ({attempt}){dot}{extension}")
        };
        let target = folder.join(file);
        match fs::read(&target) {
            Ok(existing) if existing == bytes => return Ok(target),
            Ok(_) => continue,
            Err(_) => {
                fs::write(&target, bytes).map_err(|e| e.to_string())?;
                return Ok(target);
            }
        }
    }
    Err("Too many files with that name".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).unwrap()
    }

    #[test]
    fn tells_text_from_binary() {
        assert!(is_text(Some("text/html; charset=utf-8"), b"<html>"));
        assert!(is_text(Some("application/json"), b"{}"));
        assert!(!is_text(Some("application/pdf"), b"%PDF-1.7"));
        assert!(!is_text(Some("text/plain"), b"PK\x03\x04\0\0"));
        assert!(is_text(None, b"<!DOCTYPE html><html>"));
    }

    #[test]
    fn names_files_from_the_request_or_the_url() {
        let pdf = url("https://example.org/files/Pond%20Design%20Guide.pdf");
        assert_eq!(file_stem(None, &pdf), "Pond Design Guide");
        assert_eq!(file_stem(Some("Guía de pozas.pdf"), &pdf), "Guía de pozas");
        assert_eq!(
            file_stem(Some("  "), &url("https://example.org/")),
            "example.org"
        );
        assert_eq!(file_stem(Some("a/b:c"), &pdf), "a b c");
    }

    #[test]
    fn picks_the_extension_from_the_content_then_the_url() {
        let page = url("https://example.org/data/table");
        assert_eq!(extension_of(&page, Some("text/csv; charset=utf-8")), "csv");
        assert_eq!(
            extension_of(&url("https://x.org/a/report.XLSX"), None),
            "xlsx"
        );
        assert_eq!(extension_of(&page, Some("application/octet-stream")), "bin");
    }

    #[test]
    fn keeps_one_file_per_content() {
        let dir = std::env::temp_dir().join(format!("monobot-web-{}", std::process::id()));
        let first = save_unique(&dir, "guide", "pdf", b"one").unwrap();
        assert_eq!(save_unique(&dir, "guide", "pdf", b"one").unwrap(), first);
        let second = save_unique(&dir, "guide", "pdf", b"two").unwrap();
        assert!(second.ends_with("guide (2).pdf"));
        let _ = fs::remove_dir_all(&dir);
    }

    // Online: `cargo test -p monocode --lib web:: -- --ignored`.
    #[test]
    #[ignore]
    fn reads_a_real_page() {
        let page = fetch_page("https://example.com/").unwrap();
        assert!(!page.is_pdf);
        assert!(page.text.unwrap().contains("Example Domain"));
    }

    #[test]
    #[ignore]
    fn recognises_a_real_pdf() {
        let page = fetch_page("https://arxiv.org/pdf/1706.03762").unwrap();
        assert!(page.is_pdf);
        assert!(page.text.is_none());
        assert!(page.final_url.starts_with("https://arxiv.org/pdf/"));
    }

    #[test]
    fn refuses_private_addresses() {
        for url in [
            "http://127.0.0.1/",
            "http://localhost:8080/",
            "http://192.168.1.1/",
            "file:///etc/passwd",
        ] {
            assert!(fetch_page(url).is_err(), "{url}");
        }
    }
}
