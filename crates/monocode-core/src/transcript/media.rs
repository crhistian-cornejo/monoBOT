//! Images and videos a reply points at on this computer: `![alt](path)` and
//! a lone link to a local image or video (`DocumentImage.tsx`,
//! `LocalMedia.tsx`). Remote images are never fetched; their alt text stands
//! in for them, as in the Tauri app.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Video,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    pub kind: Kind,
    pub path: PathBuf,
}

/// What a file name's extension says it is.
pub fn kind_of(name: &str) -> Option<Kind> {
    let name = name.split(['?', '#']).next().unwrap_or(name);
    let extension = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => Some(Kind::Image),
        "mp4" | "webm" | "mov" | "m4v" => Some(Kind::Video),
        _ => None,
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Some(byte) = text
                .get(at + 1..at + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn drive_path(src: &str) -> bool {
    let bytes = src.as_bytes();
    bytes.len() > 2
        && bytes[1] == b':'
        && bytes[0].is_ascii_alphabetic()
        && matches!(bytes[2], b'\\' | b'/')
}

/// Whether `src` names something on this computer rather than the web.
pub fn is_local(src: &str) -> bool {
    let src = src.trim();
    if src.is_empty() || src.starts_with("//") {
        return false;
    }
    if src.starts_with("file://") {
        return true;
    }
    // A Windows drive letter is a path, not a scheme.
    drive_path(src)
        || !src.split_once(':').is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        })
}

/// The local image or video `src` names, resolved against the session's
/// folder when relative. Nothing for web addresses or other files.
pub fn resolve(src: &str, cwd: Option<&str>, home: Option<&Path>) -> Option<Media> {
    let src = src.trim();
    if !is_local(src) {
        return None;
    }
    let raw = src.strip_prefix("file://").map(percent_decode);
    let src = raw.as_deref().unwrap_or(src);
    let src = src.split(['?', '#']).next().unwrap_or(src);
    let kind = kind_of(src)?;
    let path = if let Some(rest) = src.strip_prefix("~/") {
        home?.join(rest)
    } else if Path::new(src).is_absolute() || drive_path(src) {
        PathBuf::from(src)
    } else {
        Path::new(cwd?).join(src.strip_prefix("./").unwrap_or(src))
    };
    Some(Media { kind, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_images_and_videos_resolve_against_the_session() {
        let home = Path::new("/Users/me");
        assert_eq!(
            resolve("./shots/a.png", Some("/repo"), Some(home)),
            Some(Media {
                kind: Kind::Image,
                path: "/repo/shots/a.png".into()
            })
        );
        assert_eq!(
            resolve("/tmp/clip.MP4", None, None).map(|m| m.kind),
            Some(Kind::Video)
        );
        assert_eq!(
            resolve("~/Desktop/x.webp", None, Some(home)).unwrap().path,
            PathBuf::from("/Users/me/Desktop/x.webp")
        );
        assert_eq!(
            resolve("file:///tmp/my%20shot.png", None, None)
                .unwrap()
                .path,
            PathBuf::from("/tmp/my shot.png")
        );
        assert_eq!(
            resolve("C:\\shots\\a.jpg", None, None).map(|m| m.kind),
            Some(Kind::Image)
        );
    }

    #[test]
    fn web_addresses_and_other_files_are_not_media() {
        assert_eq!(
            resolve("https://example.com/a.png", Some("/repo"), None),
            None
        );
        assert_eq!(
            resolve("//cdn.example.com/a.png", Some("/repo"), None),
            None
        );
        assert_eq!(
            resolve("data:image/png;base64,AAAA", Some("/repo"), None),
            None
        );
        assert_eq!(resolve("src/main.rs", Some("/repo"), None), None);
        assert_eq!(resolve("a.png", None, None), None);
        assert!(!is_local("mailto:me@example.com"));
        assert!(is_local("docs/a.png"));
    }
}
