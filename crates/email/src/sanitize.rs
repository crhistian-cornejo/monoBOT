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
    "background",
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
/// Parts of a style that reach out (`image-set(`), run (`expression(`, `javascript:`) or import; a backslash
/// could hide any of them, so it is refused too. A `url(` is not here: it is resolved like a picture's address.
const CSS_REFUSED: &[&str] = &["image-set(", "expression(", "@import", "javascript:", "\\"];

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
        ("img", "src") | (_, "background") => picture(value, pictures, remote)?,
        ("a", "href") => link(value)?,
        (_, "style") => style(value, pictures, remote)?,
        _ => value.to_string(),
    };
    Some(Cow::Owned(kept))
}

/// A picture the message carries (`cid:`) becomes a data URL. One on the web stays as it is and is listed, so the
/// frame (which loads nothing) gets it from the app (`remote.rs`); one on plain `http` is asked for over `https`, the
/// only way the app fetches. A `data:` picture of a known type stays; anything else goes.
fn picture(value: &str, pictures: &Pictures, remote: &Mutex<Vec<String>>) -> Option<String> {
    let value = value.trim();
    if value
        .get(..4)
        .is_some_and(|head| head.eq_ignore_ascii_case("cid:"))
    {
        let (kind, bytes) = cid_lookup(pictures, &value[4..])?;
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

/// The picture a `cid:` address names, however the sender wrote it: percent-encoded, in another
/// case, or padded with the angle brackets or space the `Content-ID` header lost.
fn cid_lookup<'a>(pictures: &'a Pictures, id: &str) -> Option<&'a (String, Vec<u8>)> {
    if let Some(hit) = pictures.get(id) {
        return Some(hit);
    }
    let decoded = percent_decode(id);
    if let Some(hit) = pictures.get(decoded.as_str()) {
        return Some(hit);
    }
    let wanted = decoded.trim_matches(|c: char| c == '<' || c == '>' || c.is_whitespace());
    pictures
        .iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(wanted))
        .map(|(_, picture)| picture)
}

/// `%40` → `@`, and so on: some senders encode the `cid:` address.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = (bytes[i] == b'%')
            .then(|| {
                std::str::from_utf8(bytes.get(i + 1..i + 3)?)
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            })
            .flatten();
        match decoded {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

/// The CSS a message sets on its own elements, without what runs or covers the page. A `url()` is
/// resolved like a picture's address: what the message carries shows at once, what lives on the web
/// waits for the owner, and anything else drops the whole declaration.
fn style(value: &str, pictures: &Pictures, remote: &Mutex<Vec<String>>) -> Option<String> {
    let mut kept: Vec<String> = Vec::new();
    for declaration in declarations(value) {
        let declaration = declaration.trim();
        let Some((property, _)) = declaration.split_once(':') else {
            continue;
        };
        let lower = declaration.to_ascii_lowercase();
        if CSS_BANNED.contains(&property.trim().to_ascii_lowercase().as_str())
            || CSS_REFUSED.iter().any(|bad| lower.contains(bad))
        {
            continue;
        }
        if lower.contains("url(") {
            match style_urls(declaration, pictures, remote) {
                Some(resolved) => kept.push(resolved),
                None => continue,
            }
        } else {
            kept.push(declaration.to_string());
        }
    }
    (!kept.is_empty()).then(|| kept.join("; "))
}

/// A `url(...)` address the declaration is allowed to keep (`cid:`, `https`, `data:` of a known
/// picture type), replaced by what it resolves to. Any address that is none of those refuses the
/// whole declaration, like before.
fn style_urls(
    declaration: &str,
    pictures: &Pictures,
    remote: &Mutex<Vec<String>>,
) -> Option<String> {
    let mut out = String::with_capacity(declaration.len());
    let mut rest = declaration;
    while let Some(at) = rest.to_ascii_lowercase().find("url(") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 4..];
        let close = url_end(after)?;
        let target = after[..close]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .trim();
        let resolved = picture(target, pictures, remote)?;
        // It is written inside quotes: nothing may break out of them.
        if resolved.contains(['"', '\'']) || resolved.chars().any(char::is_whitespace) {
            return None;
        }
        out.push_str("url(\"");
        out.push_str(&resolved);
        out.push_str("\")");
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The `)` that closes a `url(`, skipping over a quoted interior; `None` if it never ends.
fn url_end(after: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in after.char_indices() {
        match c {
            '"' | '\'' => {
                if quote == Some(c) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(c);
                }
            }
            ')' if quote.is_none() => return Some(i),
            _ => {}
        }
    }
    None
}

/// The `;` that ends a declaration, never one inside `url(...)` (a data picture carries `;base64`)
/// or inside quotes.
fn declarations(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0usize, None, 0usize);
    for (i, c) in value.char_indices() {
        match c {
            '(' if quote.is_none() => depth += 1,
            ')' if quote.is_none() => depth = depth.saturating_sub(1),
            '"' | '\'' => {
                if quote == Some(c) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(c);
                }
            }
            ';' if depth == 0 && quote.is_none() => {
                out.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&value[start..]);
    out
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
        // A picture on the web is kept for the owner to allow, like an `img` address.
        assert!(
            cleaned
                .html
                .contains(r##"background: url(&quot;https://ejemplo.test/fondo.png&quot;)"##),
            "{}",
            cleaned.html
        );
        assert_eq!(cleaned.remote, ["https://ejemplo.test/fondo.png"]);
        assert!(!cleaned.html.contains("position"), "{}", cleaned.html);
    }

    #[test]
    fn a_style_picture_the_message_carries_or_a_bad_one() {
        let cleaned = clean(
            r##"<div style="background: url( 'cid:logo@echo.test' ); color: #fff">a</div><div style="background: url(data:image/png;base64,AAAA); background-image: url('https://ejemplo.test/b.png')">b</div><div style="background: url(javascript:alert(1))">c</div><div style="background: url(https://ejemplo.test/x.png">d</div>"##,
            pictures(),
        );
        assert!(
            cleaned
                .html
                .contains(r##"url(&quot;data:image/png;base64,AQID&quot;)"##),
            "{}",
            cleaned.html
        );
        // A data picture's `;base64` inside `url()` does not split the declaration.
        assert!(
            cleaned
                .html
                .contains(r##"url(&quot;data:image/png;base64,AAAA&quot;)"##),
            "{}",
            cleaned.html
        );
        assert_eq!(cleaned.remote, ["https://ejemplo.test/b.png"]);
        assert!(
            !cleaned.html.contains("javascript:") && !cleaned.html.contains("x.png"),
            "{}",
            cleaned.html
        );
    }

    #[test]
    fn the_pictures_a_cid_names_are_found_however_the_sender_wrote_them() {
        let mut pictures = pictures();
        pictures.insert(
            "qr@banco.test".to_string(),
            ("image/png".to_string(), vec![4, 5, 6]),
        );
        let cleaned = clean(
            r##"<img src="CID:logo@echo.test"><img src="cid:qr%40banco.test"><table background="cid:logo@echo.test"><tr><td>x</td></tr></table>"##,
            pictures,
        );
        assert_eq!(
            cleaned.html.matches("data:image/png;base64,AQID").count(),
            2
        );
        assert!(cleaned.html.contains("data:image/png;base64,BAUG"));
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
