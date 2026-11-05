//! What a message's text is cleaned of before the owner reads it: the entities mail clients leave undecoded
//! (`&zwnj;`, `&#8204;`), the invisible characters newsletters use as padding, the `[image: …]` notes of a text-only
//! rendering and the links. A link is never shown as one (nothing here can be clicked or followed): a link in angle
//! brackets goes, and any other says where it points, so the owner knows what a message wants them to open.

/// Characters that take no room: zero-width spaces and joiners, direction marks, the soft hyphen and the byte order mark.
fn invisible(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{feff}' | '\u{34f}' | '\u{ad}' | '\u{202a}'..='\u{202e}')
}

/// The character an entity stands for (`zwnj`, `amp`, `#8204`, `#x1F4A1`), or `None` when it is not one Echo knows.
fn entity(name: &str) -> Option<String> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code).map(String::from);
    }
    Some(match name {
        "nbsp" => " ", "amp" => "&", "lt" => "<", "gt" => ">", "quot" => "\"", "apos" => "'", "zwnj" | "zwj" | "shy" => "",
        "aacute" => "á", "eacute" => "é", "iacute" => "í", "oacute" => "ó", "uacute" => "ú", "ntilde" => "ñ", "uuml" => "ü",
        "Aacute" => "Á", "Eacute" => "É", "Iacute" => "Í", "Oacute" => "Ó", "Uacute" => "Ú", "Ntilde" => "Ñ",
        "iquest" => "¿", "iexcl" => "¡", "hellip" => "…", "ndash" => "–", "mdash" => "—", "lsquo" | "rsquo" => "'", "ldquo" | "rdquo" => "\"",
        "copy" => "©", "reg" => "®", "euro" => "€", "middot" => "·", "bull" => "•",
        _ => return None,
    }.to_string())
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        // An entity is short: «&» alone, or one that never ends, stays as it is.
        match after.find(';').filter(|end| *end <= 10).and_then(|end| entity(&after[..end]).map(|plain| (end, plain))) {
            Some((end, plain)) => {
                out.push_str(&plain);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// «[image: Logo de la tienda]» is how a text-only rendering says there was a picture: it tells the owner nothing.
fn without_images(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("[image:") {
        out.push_str(&rest[..at]);
        match rest[at..].find(']').filter(|end| *end <= 300) {
            Some(end) => rest = &rest[at + end + 1..],
            None => {
                out.push_str("[image:");
                rest = &rest[at + "[image:".len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One word of the text: a link goes (in angle brackets) or is told by where it points; anything else stays.
fn word(token: &str) -> String {
    let core = token.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | '[' | ']' | '"' | '\'' | ',' | '.' | ';' | ':'));
    let lower = core.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return token.to_string();
    }
    if token.starts_with('<') {
        return String::new();
    }
    let host = core.split("://").nth(1).unwrap_or_default().split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or(host).trim_start_matches("www.");
    let tail: String = token.chars().rev().take_while(|c| matches!(c, ',' | '.' | ';' | ':' | ')' | '>')).collect::<Vec<_>>().into_iter().rev().collect();
    let head: String = token.chars().take_while(|c| matches!(c, '(' | '[' | '"')).collect();
    if host.is_empty() { tail } else { format!("{head}[enlace: {host}]{tail}") }
}

fn without_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    for c in text.chars() {
        if c.is_whitespace() {
            out.push_str(&word(&token));
            token.clear();
            out.push(c);
        } else {
            token.push(c);
        }
    }
    out.push_str(&word(&token));
    out
}

/// The text as the owner reads it. Paragraph breaks are kept; the rest is `mime::one_line`'s and `paragraphs`'.
pub fn tidy(text: &str) -> String {
    let text: String = decode_entities(text).chars().filter(|c| !invisible(*c)).collect();
    without_links(&without_images(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_a_client_left_undecoded_are_decoded_or_gone() {
        assert_eq!(tidy("avisanos &zwnj; &#8204; &amp; m&aacute;s &#x41; &nbsp;fin"), "avisanos   & más A  fin");
        assert_eq!(tidy("a & b &unknown; c &"), "a & b &unknown; c &");
        assert_eq!(tidy("&#99999999999;"), "&#99999999999;");
    }

    #[test]
    fn padding_made_of_invisible_characters_is_removed() {
        assert_eq!(tidy("a\u{200c}\u{200b}\u{feff}b\u{ad}c"), "abc");
    }

    #[test]
    fn the_notes_of_pictures_are_removed() {
        assert_eq!(tidy("[image: Google] Se creó una contraseña [image: sin cerrar"), " Se creó una contraseña [image: sin cerrar");
    }

    #[test]
    fn links_in_angle_brackets_go_and_the_others_say_where_they_point() {
        assert_eq!(tidy("Hola <https://tienda.test/x?y=1> adiós"), "Hola  adiós");
        assert_eq!(tidy("Mira https://www.ejemplo.test/a/b?c=d, ahora."), "Mira [enlace: ejemplo.test], ahora.");
        assert_eq!(tidy("(https://usuario@banco.test/login)"), "([enlace: banco.test])");
        assert_eq!(tidy("sin enlaces ni http aquí"), "sin enlaces ni http aquí");
    }

    #[test]
    fn paragraph_breaks_survive() {
        assert_eq!(tidy("uno\n\ndos &amp; tres"), "uno\n\ndos & tres");
    }
}
