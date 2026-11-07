//! A cited site's icon, found the way the Tauri link preview found it: the
//! icon the page declares, else `/favicon.ico`. It is fetched through the same
//! public-address guard as every agent web request, shrunk to a small PNG and
//! kept per host, so a sources list costs one request per site, once.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use url::Url;

use super::{fetch, page};

const TIMEOUT: Duration = Duration::from_secs(8);
const MAX_PAGE_BYTES: usize = 1024 * 1024;
const MAX_ICON_BYTES: usize = 512 * 1024;
const SIDE: u32 = 64;
/// A site without a usable icon is not asked again for a day.
const MISS_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// The site's icon as a PNG, or `None` when it has none the app can read.
/// Blocks on the network the first time a host is seen.
pub fn site_icon_png(url: &str) -> Option<Vec<u8>> {
    let cited = fetch::parse_public_url(url).ok()?;
    // The home page names the site's icon more reliably than a deep link.
    let page_url = Url::parse(&format!("{}/", cited.origin().ascii_serialization())).ok()?;
    let host = page_url.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let dir = cache_dir()?;
    let file = dir.join(format!("{}.png", crate::papers::key_of(host)));
    let miss = file.with_extension("none");
    if let Ok(png) = fs::read(&file) {
        return Some(png);
    }
    let recent_miss = fs::metadata(&miss)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_some_and(|age| age < MISS_TTL);
    if recent_miss {
        return None;
    }
    let _ = fs::create_dir_all(&dir);
    match fetch_icon(&page_url) {
        Some(png) => {
            let _ = fs::write(&file, &png);
            Some(png)
        }
        None => {
            let _ = fs::write(&miss, b"");
            None
        }
    }
}

fn cache_dir() -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("com.monocode.desktop")
            .join("site-icons"),
    )
}

fn fetch_icon(page_url: &Url) -> Option<Vec<u8>> {
    let mut candidates = Vec::new();
    if let Ok(html) = fetch::fetch_within(
        page_url.clone(),
        "text/html,application/xhtml+xml;q=0.9,*/*;q=0.5",
        MAX_PAGE_BYTES,
        TIMEOUT,
    ) {
        if fetch::is_text(html.content_type.as_deref(), &html.bytes) {
            let text = String::from_utf8_lossy(&html.bytes);
            let declared = page::read_web_page(&text, html.final_url.as_str(), Some(0), None).icon;
            candidates.push(declared);
        }
    }
    candidates.push(format!(
        "{}/favicon.ico",
        page_url.origin().ascii_serialization()
    ));
    candidates.dedup();
    candidates.into_iter().find_map(|icon| {
        let url = fetch::parse_public_url(&icon).ok()?;
        let fetched =
            fetch::fetch_within(url, "image/*,*/*;q=0.5", MAX_ICON_BYTES, TIMEOUT).ok()?;
        small_png(&fetched.bytes)
    })
}

/// The image as a `SIDE`-pixel PNG; `None` for anything that is not a
/// readable raster image (an SVG or an HTML error page, for instance).
fn small_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = image.thumbnail(SIDE, SIDE);
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_a_raster_icon_to_a_small_png_and_refuses_anything_else() {
        let mut big = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(256, 256, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut big, image::ImageFormat::Png)
            .unwrap();
        let png = small_png(&big.into_inner()).unwrap();
        let shown = image::load_from_memory(&png).unwrap();
        assert_eq!((shown.width(), shown.height()), (SIDE, SIDE));
        assert!(small_png(b"<html>not found</html>").is_none());
        assert!(small_png(b"<svg xmlns='http://www.w3.org/2000/svg'/>").is_none());
    }

    #[test]
    fn refuses_addresses_that_are_not_public() {
        assert!(site_icon_png("http://127.0.0.1/").is_none());
        assert!(site_icon_png("file:///etc/passwd").is_none());
    }
}
