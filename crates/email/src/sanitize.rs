//! A message's HTML made safe to show. What lays out and writes the text is kept (tables, colours, sizes, links, the
//! pictures the message carries); what runs, loads on its own, covers the page or leaves the message is dropped. It is
//! an allow list: a tag, attribute or style that is not named here is never guessed at. The page shows the result in a
//! frame that runs no script and loads nothing but the pictures the owner allows (`mail-body` in the desktop app).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ammonia::{Builder, UrlRelative};
use base64::Engine;

/// The pictures a message may show, by media type. Never SVG: it can carry script.
const PICTURE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
/// The most pictures on the web one message may name; the rest are dropped.
const MAX_REMOTE: usize = 50;
const TAGS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "b",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "dfn",
    "div",
    "dl",
    "dt",
    "em",
    "font",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "mark",
    "ol",
    "p",
    "pre",
    "q",
    "s",
    "samp",
    "small",
    "span",
    "strike",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "tt",
    "u",
    "ul",
];
/// Content dropped together with its tags, never shown as text.
const DROPPED: &[&str] = &[
    "head", "script", "style", "title", "iframe", "object", "embed", "svg", "math", "noscript",
    "template", "textarea", "select",
];
/// Attributes any kept element may have. `src` (pictures) and `href` (links) are checked where they are set.
const ATTRIBUTES: &[&str] = &[
    "align",
    "alt",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "color",
    "colspan",
    "dir",
    "face",
    "height",
    "lang",
    "rowspan",
    "size",
    "style",
    "title",
    "valign",
    "width",
];
/// Properties a message may not set: they place its content over the page, or run code.
const CSS_BANNED: &[&str] = &["position", "behavior", "-moz-binding"];
/// Parts of a style that reach out (`url(`, `image-set(`), run (`expression(`, `javascript:`) or import; a backslash
/// could hide any of them, so it is refused too.
const CSS_REFUSED: &[&str] = &[
    "url(",
    "image-set(",
    "expression(",
    "@import",
    "javascript:",
    "\\",
];

/// The message's pictures: `Content-ID` → (media type, bytes).
pub type Pictures = HashMap<String, (String, Vec<u8>)>;

/// The HTML a message shows, and the pictures on the web it names (which the owner allows before they load).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cleaned {
    pub html: String,
    pub remote: Vec<String>,
}

pub fn clean(html: &str, pictures: Pictures) -> Cleaned {
    let remote = Arc::new(Mutex::new(Vec::new()));
    let cleaned = {
        let mut builder = Builder::default();
        builder
            .tags(TAGS.iter().copied().collect())
            .clean_content_tags(DROPPED.iter().copied().collect())
            .generic_attributes(ATTRIBUTES.iter().copied().collect())
            .add_tag_attributes("img", ["src"])
            .add_tag_attributes("a", ["href"])
            // `http` only reaches the filter, which asks for a picture over https and drops a link.
            .url_schemes(
                ["https", "http", "mailto", "data", "cid"]
                    .into_iter()
                    .collect(),
            )
            .url_relative(UrlRelative::Deny)
            .link_rel(Some("noopener noreferrer nofollow"))
            .attribute_filter({
                let remote = Arc::clone(&remote);
                move |element, attribute, value| {
                    filter(element, attribute, value, &pictures, &remote)
                }
            });
        builder.clean(html).to_string()
    };
    let remote = remote.lock().unwrap_or_else(|p| p.into_inner()).clone();
    Cleaned {
        html: cleaned,
        remote,
    }
}

fn filter<'a>(
    element: &str,
    attribute: &str,
    value: &str,
    pictures: &Pictures,
    remote: &Mutex<Vec<String>>,
) -> Option<Cow<'a, str>> {
    let kept = match (element, attribute) {
        ("img", "src") => picture(value, pictures, remote)?,
        ("a", "href") => link(value)?,
        (_, "style") => style(value)?,
        _ => value.to_string(),
    };
    Some(Cow::Owned(kept))
}

/// A picture the message carries (`cid:`) becomes a data URL. One on the web stays as it is and is listed, so the
/// frame (which loads nothing) gets it from the app (`remote.rs`); one on plain `http` is asked for over `https`, the
/// only way the app fetches. A `data:` picture of a known type stays; anything else goes.
fn picture(value: &str, pictures: &Pictures, remote: &Mutex<Vec<String>>) -> Option<String> {
    let value = value.trim();
    if let Some(id) = value.strip_prefix("cid:") {
        let (kind, bytes) = pictures.get(id)?;
        return PICTURE_TYPES.contains(&kind.as_str()).then(|| {
            format!(
                "data:{kind};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        });
    }
    let secured;
    let value = match value.get(..7) {
        Some(scheme) if scheme.eq_ignore_ascii_case("http://") => {
            secured = format!("https://{}", &value[7..]);
            secured.as_str()
        }
        _ => value,
    };
    if is_https(value) {
        let mut listed = remote.lock().unwrap_or_else(|p| p.into_inner());
        if !listed.iter().any(|url| url == value) {
            if listed.len() >= MAX_REMOTE {
                return None;
            }
            listed.push(value.to_string());
        }
        return Some(value.to_string());
    }
    if let Some(rest) = value.strip_prefix("data:") {
        let kind = rest.split(';').next().unwrap_or_default();
        return (PICTURE_TYPES.contains(&kind) && rest.contains(";base64,"))
            .then(|| value.to_string());
    }
    None
}

/// A link goes to the web or to a mail address, never to a page of its own (a `data:` address could be one).
fn link(value: &str) -> Option<String> {
    let value = value.trim();
    (is_https(value)
        || value
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mailto:")))
    .then(|| value.to_string())
}

/// The CSS a message sets on its own elements, without what reaches out, runs or covers the page.
fn style(value: &str) -> Option<String> {
    let kept: Vec<&str> = value
        .split(';')
        .map(str::trim)
        .filter(|declaration| {
            let Some((property, _)) = declaration.split_once(':') else {
                return false;
            };
            let lower = declaration.to_ascii_lowercase();
            !CSS_BANNED.contains(&property.trim().to_ascii_lowercase().as_str())
                && !CSS_REFUSED.iter().any(|bad| lower.contains(bad))
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

fn is_https(value: &str) -> bool {
    value
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pictures() -> Pictures {
        HashMap::from([(
            "logo@echo.test".to_string(),
            ("image/png".to_string(), vec![1, 2, 3]),
        )])
    }

    #[test]
    fn scripts_handlers_and_forms_do_not_survive() {
        let cleaned = clean(
            r##"<p onclick="robar()">Hola<script>alert(1)</script></p><form action="https://ejemplo.test/x"><input name="clave"></form>"##,
            Pictures::new(),
        );
        assert!(cleaned.html.contains("Hola"), "{}", cleaned.html);
        for gone in ["script", "onclick", "<form", "action", "<input", "alert"] {
            assert!(
                !cleaned.html.contains(gone),
                "{gone} survived: {}",
                cleaned.html
            );
        }
    }

    #[test]
    fn links_go_to_the_web_or_a_mail_address_only() {
        let cleaned = clean(
            r##"<a href="javascript:alert(1)">mal</a> <a href="https://ejemplo.test/factura">bien</a> <a href="data:text/html,<script>x</script>">datos</a> <a href="mailto:ayuda@ejemplo.test">correo</a>"##,
            Pictures::new(),
        );
        assert!(
            cleaned
                .html
                .contains(r##"href="https://ejemplo.test/factura""##),
            "{}",
            cleaned.html
        );
        assert!(
            cleaned.html.contains("mailto:ayuda@ejemplo.test"),
            "{}",
            cleaned.html
        );
        assert!(
            !cleaned.html.contains("javascript:") && !cleaned.html.contains("data:text"),
            "{}",
            cleaned.html
        );
        let plain = clean(
            r##"<a href="http://ejemplo.test/x">sin cifrar</a>"##,
            Pictures::new(),
        );
        assert!(!plain.html.contains("href"), "{}", plain.html);
    }

    #[test]
    fn the_message_s_pictures_become_data_and_pictures_on_the_web_wait_for_the_owner() {
        let cleaned = clean(
            r##"<img src="cid:logo@echo.test" alt="Logo"><img src="https://imagenes.ejemplo.test/a.png"><img src="cid:otra"><img src="http://ejemplo.test/espia.png">"##,
            pictures(),
        );
        assert!(
            cleaned.html.contains("data:image/png;base64,AQID"),
            "{}",
            cleaned.html
        );
        assert!(
            cleaned.html.contains("https://imagenes.ejemplo.test/a.png"),
            "{}",
            cleaned.html
        );
        assert_eq!(
            cleaned.remote,
            vec![
                "https://imagenes.ejemplo.test/a.png",
                "https://ejemplo.test/espia.png"
            ],
            "plain http is asked for over https"
        );
        for gone in ["cid:", "http://ejemplo.test"] {
            assert!(
                !cleaned.html.contains(gone),
                "{gone} survived: {}",
                cleaned.html
            );
        }
    }

    #[test]
    fn a_picture_that_could_be_a_page_is_not_kept() {
        let mut svg = Pictures::new();
        svg.insert(
            "x".to_string(),
            (
                "image/svg+xml".to_string(),
                b"<svg onload=alert(1)>".to_vec(),
            ),
        );
        let cleaned = clean(
            r##"<img src="cid:x"><img src="data:image/svg+xml;base64,PHN2Zz4=">"##,
            svg,
        );
        assert!(
            !cleaned.html.contains("svg") && !cleaned.html.contains("data:"),
            "{}",
            cleaned.html
        );
    }

    #[test]
    fn styles_keep_the_layout_and_drop_what_reaches_out_or_covers_the_page() {
        let cleaned = clean(
            r##"<table><tr><td style="width: 50%; background: url(https://ejemplo.test/fondo.png); position: fixed; color: #333">x</td></tr></table>"##,
            Pictures::new(),
        );
        assert!(
            cleaned.html.contains("width: 50%") && cleaned.html.contains("color: #333"),
            "{}",
            cleaned.html
        );
        assert!(
            !cleaned.html.contains("url(") && !cleaned.html.contains("position"),
            "{}",
            cleaned.html
        );
        assert!(cleaned.remote.is_empty());
    }

    #[test]
    fn tables_colours_and_text_styles_stay() {
        let cleaned = clean(
            r##"<table width="600" bgcolor="#f4f4f4"><tr><td align="center" style="font-weight: bold">Total</td></tr></table>"##,
            Pictures::new(),
        );
        for kept in [
            "<table",
            "width=\"600\"",
            "bgcolor=\"#f4f4f4\"",
            "align=\"center\"",
            "font-weight: bold",
            "Total",
        ] {
            assert!(
                cleaned.html.contains(kept),
                "{kept} missing: {}",
                cleaned.html
            );
        }
    }

    #[test]
    fn what_is_dropped_takes_its_text_with_it() {
        let cleaned = clean(
            "<style>p{color:red}</style><title>Aviso</title><p>hola</p>",
            Pictures::new(),
        );
        assert!(
            cleaned.html.contains("hola")
                && !cleaned.html.contains("color")
                && !cleaned.html.contains("Aviso"),
            "{}",
            cleaned.html
        );
    }
}
