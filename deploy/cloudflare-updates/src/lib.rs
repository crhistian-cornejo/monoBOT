//! monoBOT update Worker.
//!
//! Serves the release bucket (`monobot-releases` in R2) that
//! `cargo xtask release` fills:
//!
//! - `latest.json`: the signed update feed the app checks.
//! - `downloads.json`: which installer each `/download/<key>` serves.
//! - `releases/<version>/<file>`: the installers, update packages and their
//!   signatures. They never change once published.
//!
//! The routing and its rules live here as plain Rust; `cf` binds them to R2
//! when compiled to wasm32.

#[cfg(target_arch = "wasm32")]
mod cf;
#[cfg(test)]
mod tests;

/// The installers people download by hand; the same list as
/// `xtask/src/release.rs` (`DOWNLOAD_KEYS`).
pub const DOWNLOAD_KEYS: [&str; 6] = [
    "windows",
    "macos-arm64",
    "macos-x64",
    "linux-appimage",
    "linux-deb",
    "linux-rpm",
];

pub const FEED_KEY: &str = "latest.json";
pub const DOWNLOADS_KEY: &str = "downloads.json";

/// The feed and the download list change with every release.
pub const NO_STORE: &str = "no-store, max-age=0, must-revalidate";
/// A published file never changes: a new release uses a new folder.
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// `/`: what this Worker serves.
    Index,
    /// `/latest.json`
    Feed,
    /// `/downloads.json`: the latest version and its installers, for the
    /// download page.
    Downloads,
    /// `/download/<key>`: a redirect to the latest installer.
    Download(String),
    /// `/releases/<version>/<file>`
    File {
        key: String,
    },
    NotFound,
    MethodNotAllowed,
}

/// A version folder: digits, dots and a `-preview.N` suffix.
fn version(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 40
        && segment.starts_with(|c: char| c.is_ascii_digit())
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && !segment.contains("..")
}

/// A file the release wrote: letters, digits and `._+-`, never hidden.
fn filename(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 160
        && !segment.starts_with('.')
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
        && !segment.contains("..")
}

/// Which object a request reads. Only `GET` and `HEAD` exist.
pub fn route(method: &str, path: &str) -> Route {
    if method != "GET" && method != "HEAD" {
        return Route::MethodNotAllowed;
    }
    let path = path
        .strip_suffix('/')
        .filter(|p| !p.is_empty())
        .unwrap_or(path);
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        [""] => Route::Index,
        ["latest.json"] => Route::Feed,
        ["downloads.json"] | ["download"] => Route::Downloads,
        ["download", key] if DOWNLOAD_KEYS.contains(key) => Route::Download((*key).to_owned()),
        ["releases", v, file] if version(v) && filename(file) => Route::File {
            key: format!("releases/{v}/{file}"),
        },
        _ => Route::NotFound,
    }
}

/// The path `/download/<key>` redirects to, from `downloads.json`.
pub fn download_path(downloads: &[u8], key: &str) -> Option<String> {
    let listed: serde_json::Value = serde_json::from_slice(downloads).ok()?;
    let release = listed["version"].as_str().filter(|v| version(v))?;
    let file = listed["files"][key].as_str().filter(|f| filename(f))?;
    Some(format!("/releases/{release}/{file}"))
}

/// The `Content-Type` of a published file.
pub fn content_type(key: &str) -> &'static str {
    let lower = key.to_ascii_lowercase();
    let ends = |suffix: &str| lower.ends_with(suffix);
    if ends(".json") {
        "application/json"
    } else if ends(".sig") {
        "text/plain; charset=utf-8"
    } else if ends(".exe") {
        "application/vnd.microsoft.portable-executable"
    } else if ends(".dmg") {
        "application/x-apple-diskimage"
    } else if ends(".tar.gz") {
        "application/gzip"
    } else if ends(".deb") {
        "application/vnd.debian.binary-package"
    } else if ends(".rpm") {
        "application/x-rpm"
    } else {
        "application/octet-stream"
    }
}

/// `Content-Disposition` for an installer, so browsers keep its name.
pub fn disposition(key: &str) -> Option<String> {
    let file = key.rsplit('/').next()?;
    let package = [".exe", ".dmg", ".deb", ".rpm", ".AppImage", ".tar.gz"]
        .iter()
        .any(|suffix| file.ends_with(suffix));
    package.then(|| format!("attachment; filename=\"{file}\""))
}

pub const INDEX: &str = "monoBOT updates\n\n\
GET /latest.json               signed update feed\n\
GET /downloads.json            latest version and its installers\n\
GET /download/<key>            latest installer: windows, macos-arm64, macos-x64,\n\
                               linux-appimage, linux-deb, linux-rpm\n\
GET /releases/<version>/<file> a published file\n";
