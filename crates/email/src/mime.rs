//! What a message says, as text: its headers read and its body decoded (quoted-printable, base64, charsets, HTML
//! without its tags). Nothing in it is ever run or opened: a mail's content is data.

use std::collections::HashMap;

use crate::model::Attachment;

/// The first bytes of a message's text that are read.
pub const TEXT_BYTES: usize = 12_000;
/// The paragraphs of a message the page gets, and their longest total.
const MAX_PARAGRAPHS: usize = 16;
const MAX_CHARS: usize = 4_000;

/// A message as the page and the classifier need it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub from_name: String,
    pub from_address: String,
    /// The first recipient, for what the owner sent or is writing.
    pub to_name: String,
    pub to_address: String,
    /// Where the sender asks replies to go (`Reply-To`), when it says, as (name, address).
    pub reply_to: Vec<(String, String)>,
    /// Everyone else it went to, in `To` and then `Cc`, as (name, address): who a reply to all copies.
    pub others: Vec<(String, String)>,
    pub subject: String,
    pub message_id: String,
    pub text: String,
    /// It has a `List-Unsubscribe`: a newsletter, a promotion, a notice sent to many.
    pub unsubscribe: bool,
    /// Where to unsubscribe: the `https` link the sender named, else its `mailto`; and whether one request does it.
    pub unsubscribe_link: Option<(String, bool)>,
    /// `Precedence: bulk | list | junk` or an automatic reply.
    pub bulk: bool,
    /// The receiving server says the sender failed SPF, DKIM or DMARC.
    pub auth_failed: bool,
}

pub fn parse(header: &[u8], body: &[u8]) -> Parsed {
    let header = String::from_utf8_lossy(header)
        .replace("\r\n ", " ")
        .replace("\r\n\t", " ")
        .replace("\n ", " ")
        .replace("\n\t", " ");
    let field = |name: &str| header_field(&header, name);
    let (from_name, from_address) = split_from(&decode_words(&field("from")));
    let to = decode_words(&field("to"));
    let (to_name, to_address) = split_from(to.split(',').next().unwrap_or_default());
    let precedence = field("precedence").to_ascii_lowercase();
    let auto = field("auto-submitted").to_ascii_lowercase();
    let auth = header_fields(&header, "authentication-results").to_ascii_lowercase();
    let mut others = address_list(&to);
    others.extend(address_list(&decode_words(&field("cc"))));
    Parsed {
        from_name,
        from_address,
        to_name,
        to_address,
        reply_to: address_list(&decode_words(&field("reply-to"))),
        others,
        subject: one_line(&decode_words(&field("subject")), 240),
        message_id: field("message-id")
            .chars()
            .filter(|c| !c.is_whitespace() && !c.is_control())
            .take(300)
            .collect(),
        text: part_text(
            &field("content-type"),
            &field("content-transfer-encoding"),
            body,
            0,
        ),
        unsubscribe: !field("list-unsubscribe").is_empty(),
        unsubscribe_link: unsubscribe_link(
            &field("list-unsubscribe"),
            &field("list-unsubscribe-post"),
        ),
        bulk: matches!(precedence.as_str(), "bulk" | "list" | "junk")
            || (!auto.is_empty() && auto != "no"),
        auth_failed: ["spf=fail", "dkim=fail", "dmarc=fail"]
            .iter()
            .any(|bad| auth.contains(bad)),
    }
}

/// The link of a `List-Unsubscribe` («<https://…>, <mailto:…>»): its `https` one first, else its `mailto`; and
/// whether `List-Unsubscribe-Post` says one request does it. Anything else (plain `http`, a script) is not a link.
pub fn unsubscribe_link(value: &str, post: &str) -> Option<(String, bool)> {
    let links: Vec<&str> = value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            part.strip_prefix('<')?.strip_suffix('>').map(str::trim)
        })
        .filter(|link| !link.chars().any(|c| c.is_whitespace() || c.is_control()))
        .filter(|link| link.len() <= 2_000)
        .collect();
    let one_click = post
        .to_ascii_lowercase()
        .replace(' ', "")
        .contains("list-unsubscribe=one-click");
    if let Some(https) = links
        .iter()
        .find(|l| l.to_ascii_lowercase().starts_with("https://"))
    {
        return Some(((*https).to_owned(), one_click));
    }
    links
        .iter()
        .find(|l| l.to_ascii_lowercase().starts_with("mailto:"))
        .map(|mailto| ((*mailto).to_owned(), false))
}

fn header_field(header: &str, name: &str) -> String {
    header
        .lines()
        .find_map(|l| named(l, name))
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

/// The value of a header line when it is the `name` one. (`get`: a line may hold a letter with an accent where the
/// name would end, and cutting through it would panic.)
fn named<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let (head, rest) = (line.get(..name.len())?, line.get(name.len()..)?);
    head.eq_ignore_ascii_case(name)
        .then(|| rest.strip_prefix(':'))
        .flatten()
}

/// Every line of a header that appears more than once (a server may add several `Authentication-Results`).
fn header_fields(header: &str, name: &str) -> String {
    header
        .lines()
        .filter_map(|l| named(l, name))
        .collect::<Vec<_>>()
        .join(" ")
}

/// «Ana Pérez <ana@ejemplo.test>», «"Ana" <ana@…>» or a bare address, as (name, address).
fn split_from(from: &str) -> (String, String) {
    let from = from.trim();
    if let (Some(open), Some(close)) = (from.rfind('<'), from.rfind('>'))
        && open < close
    {
        let address = from[open + 1..close].trim().to_lowercase();
        let name = one_line(from[..open].trim().trim_matches('"'), 120);
        return (
            if name.is_empty() {
                address.clone()
            } else {
                name
            },
            address,
        );
    }
    let address = from.to_lowercase();
    (address.clone(), address)
}

/// The most people kept of one address list.
const MAX_PEOPLE: usize = 50;

/// The people of an address list, «Ana <a@x.test>, "Prado, Luis" <l@x.test>; b@x.test», as (name, address). A comma
/// inside quotes or angle brackets does not split; a piece with no address (a group's «undisclosed-recipients:;») is
/// left out.
pub fn address_list(list: &str) -> Vec<(String, String)> {
    let mut people = Vec::new();
    let mut add = |piece: &str| {
        let (name, address) = split_from(piece);
        if address.contains('@') && people.len() < MAX_PEOPLE {
            people.push((name, address));
        }
    };
    let (mut piece, mut quoted, mut angled) = (String::new(), false, false);
    for c in list.chars() {
        match c {
            '"' if !angled => quoted = !quoted,
            '<' if !quoted => angled = true,
            '>' if !quoted => angled = false,
            ',' | ';' if !quoted && !angled => {
                add(&piece);
                piece.clear();
                continue;
            }
            _ => {}
        }
        piece.push(c);
    }
    add(&piece);
    people
}

/// The readable text of a MIME part: `text/plain` first, else `text/html` without its tags.
fn part_text(content_type: &str, encoding: &str, body: &[u8], depth: u8) -> String {
    let kind = content_type.to_ascii_lowercase();
    if kind.starts_with("multipart/") && depth < 4 {
        let Some(boundary) = parameter(content_type, "boundary") else {
            return String::new();
        };
        let marker = format!("--{boundary}");
        let raw = String::from_utf8_lossy(body);
        let mut html = String::new();
        for part in raw.split(marker.as_str()).skip(1) {
            let part = part.trim_start_matches(['\r', '\n']);
            let (head, content) = part
                .split_once("\r\n\r\n")
                .or_else(|| part.split_once("\n\n"))
                .unwrap_or((part, ""));
            let head = head.replace("\r\n ", " ").replace("\r\n\t", " ");
            let kind = header_field(&head, "content-type");
            let text = part_text(
                &kind,
                &header_field(&head, "content-transfer-encoding"),
                content.as_bytes(),
                depth + 1,
            );
            if text.trim().is_empty() {
                continue;
            }
            if kind.to_ascii_lowercase().starts_with("text/html") {
                if html.is_empty() {
                    html = text;
                }
            } else {
                return text;
            }
        }
        return html;
    }
    if !(kind.is_empty() || kind.starts_with("text/")) {
        return String::new();
    }
    let text = to_text(
        &parameter(content_type, "charset").unwrap_or_default(),
        &decode_transfer(encoding, body),
    );
    crate::clean::tidy(&if kind.starts_with("text/html")
        || text.contains("<html")
        || text.contains("<table")
        || text.contains("<div")
    {
        strip_html(&text)
    } else {
        text
    })
}

/// The bytes of a part as its transfer encoding wrote them: base64, quoted-printable, or as they are.
fn decode_transfer(encoding: &str, body: &[u8]) -> Vec<u8> {
    match encoding.trim().to_ascii_lowercase().as_str() {
        "base64" => {
            use base64::Engine;
            let compact: Vec<u8> = body
                .iter()
                .copied()
                .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
                .collect();
            // The fetch may cut the text in the middle of a group: one loose character cannot be decoded.
            let whole = &compact[..compact.len() - usize::from(compact.len() % 4 == 1)];
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(whole)
                .unwrap_or_default()
        }
        "quoted-printable" => quoted_printable(body),
        _ => body.to_vec(),
    }
}

/// Text in its charset: the Latin ones byte by byte, the rest as UTF-8.
fn to_text(charset: &str, bytes: &[u8]) -> String {
    let charset = charset.to_ascii_lowercase();
    if charset.starts_with("iso-8859") || charset.starts_with("windows-125") || charset == "latin1"
    {
        bytes.iter().map(|b| *b as char).collect()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// The parts of a message read whole: its HTML and its text as they were written, and the pictures an HTML part can
/// show inline (`cid:`), by their `Content-ID` without the angle brackets.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Whole {
    /// The HTML part as it was written: `sanitize` makes it safe before anything shows it.
    pub html: String,
    /// The text part; without one, the text of the HTML.
    pub text: String,
    /// `Content-ID` → (media type, bytes).
    pub pictures: HashMap<String, (String, Vec<u8>)>,
    /// The files that came with it, listed (their bytes are not kept: `attachment_at` reads one when the owner asks).
    pub attachments: Vec<Attachment>,
}

/// How deep multipart parts are followed, and how many leaf parts are read.
const MAX_DEPTH: u8 = 6;
const MAX_LEAVES: usize = 200;

/// A part with nothing inside it: its media type, charset, `Content-ID`, whether it is an attachment, and its bytes.
struct Leaf {
    kind: String,
    charset: String,
    content_id: String,
    attachment: bool,
    name: String,
    bytes: Vec<u8>,
}

/// A whole message as the server gives it (`BODY[]`): its HTML, its text and its pictures.
pub fn parse_whole(message: &[u8]) -> Whole {
    let (head, body) = split_head(message);
    let mut leaves = Vec::new();
    collect_leaves(&unfold(head), body, 0, &mut leaves);
    let mut whole = Whole {
        attachments: attachments_of(&leaves).0,
        ..Whole::default()
    };
    let mut plain = None;
    for leaf in leaves {
        match leaf.kind.as_str() {
            "text/html" if whole.html.is_empty() && !leaf.attachment => {
                whole.html = to_text(&leaf.charset, &leaf.bytes)
            }
            "text/plain" if plain.is_none() && !leaf.attachment => {
                plain = Some(to_text(&leaf.charset, &leaf.bytes))
            }
            kind if kind.starts_with("image/") && !leaf.content_id.is_empty() => {
                whole
                    .pictures
                    .entry(leaf.content_id)
                    .or_insert((kind.to_string(), leaf.bytes));
            }
            _ => {}
        }
    }
    whole.text = plain.unwrap_or_else(|| crate::clean::tidy(&strip_html(&whole.html)));
    whole
}

/// One file of a whole message (`BODY[]`), by its place among its attachments: what the reader listed of it and its bytes.
pub fn attachment_at(message: &[u8], index: usize) -> Option<(Attachment, Vec<u8>)> {
    let (head, body) = split_head(message);
    let mut leaves = Vec::new();
    collect_leaves(&unfold(head), body, 0, &mut leaves);
    let (list, places) = attachments_of(&leaves);
    let info = list.into_iter().nth(index)?;
    let place = *places.get(index)?;
    Some((info, leaves.swap_remove(place).bytes))
}

/// A leaf is a file the owner would want when it says so (`Content-Disposition: attachment`), when it has a name and is not a
/// picture the message shows inline, or when it is an application's data (a PDF, a spreadsheet, an archive, a signature).
fn is_file(leaf: &Leaf) -> bool {
    leaf.attachment
        || (!leaf.name.is_empty() && leaf.content_id.is_empty())
        || leaf.kind.starts_with("application/")
        || leaf.kind == "message/rfc822"
}

/// The biggest picture shown in place, and the most bytes all of one message's pictures may take.
const MAX_PREVIEW: usize = 4 * 1024 * 1024;
const MAX_PREVIEWS: usize = 12 * 1024 * 1024;

/// The attachments listed, each looked at (`inspect.rs`) and a picture shown in place, and the place of each among the
/// leaves (so `attachment_at` finds its bytes). Everything stays in memory.
fn attachments_of(leaves: &[Leaf]) -> (Vec<Attachment>, Vec<usize>) {
    use base64::Engine;
    let mut list = Vec::new();
    let mut places = Vec::new();
    let mut shown = 0;
    for (place, leaf) in leaves.iter().enumerate().filter(|(_, leaf)| is_file(leaf)) {
        let name = if leaf.name.is_empty() {
            format!("adjunto{}", extension_of(&leaf.kind))
        } else {
            leaf.name.clone()
        };
        let risk = crate::inspect::inspect(&name, &leaf.kind, &leaf.bytes);
        // Only a picture its bytes prove (never SVG), with nothing found against it.
        let preview = crate::inspect::picture_type(&leaf.bytes)
            .filter(|_| {
                risk.level == crate::inspect::Level::Safe
                    && leaf.bytes.len() <= MAX_PREVIEW
                    && shown + leaf.bytes.len() <= MAX_PREVIEWS
            })
            .map(|kind| {
                shown += leaf.bytes.len();
                format!(
                    "data:{kind};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(&leaf.bytes)
                )
            });
        list.push(Attachment {
            index: list.len(),
            name,
            kind: leaf.kind.clone(),
            size: leaf.bytes.len(),
            risk,
            preview,
        });
        places.push(place);
    }
    (list, places)
}

/// The ending a file of this media type usually has, for a part that came without a name.
fn extension_of(kind: &str) -> &'static str {
    match kind {
        "application/pdf" => ".pdf",
        "application/zip" => ".zip",
        "application/pkcs7-mime" => ".p7m",
        "application/pgp-encrypted" => ".pgp",
        "message/rfc822" => ".eml",
        _ => "",
    }
}

/// A name that is safe to give a file: no folder, no character a system refuses, no control character, not too long.
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    cleaned
        .trim()
        .trim_matches('.')
        .trim()
        .chars()
        .take(120)
        .collect()
}

/// The name a part says it has: `filename` of its disposition (also in its RFC 2231 form `filename*=UTF-8''…`), else `name` of
/// its type. Encoded words are read; the result is safe to give a file.
fn file_name(head: &str, content_type: &str) -> String {
    let disposition = header_field(head, "content-disposition");
    let raw = parameter(&disposition, "filename")
        .or_else(|| parameter(&disposition, "filename*").map(|value| decode_extended(&value)))
        .or_else(|| parameter(content_type, "name"))
        .unwrap_or_default();
    safe_name(&decode_words(&raw))
}

/// The value of an RFC 2231 parameter: `charset'language'%percent-encoded`.
fn decode_extended(value: &str) -> String {
    let encoded = value.splitn(3, '\'').nth(2).unwrap_or(value);
    let raw = encoded.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = (raw[i] == b'%')
            .then(|| raw.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                bytes.push(byte);
                i += 3;
            }
            None => {
                bytes.push(raw[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The leaf parts of a body, in order: its headers (`head`, unfolded) and its bytes. A multipart goes into its parts.
fn collect_leaves(head: &str, body: &[u8], depth: u8, out: &mut Vec<Leaf>) {
    let content_type = header_field(head, "content-type");
    let kind = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if kind.starts_with("multipart/") {
        if depth < MAX_DEPTH
            && let Some(boundary) = parameter(&content_type, "boundary")
        {
            for part in parts_of(body, &boundary) {
                let (head, content) = split_head(part);
                collect_leaves(&unfold(head), content, depth + 1, out);
            }
        }
        return;
    }
    if out.len() < MAX_LEAVES {
        out.push(Leaf {
            kind: if kind.is_empty() {
                "text/plain".to_string()
            } else {
                kind
            },
            charset: parameter(&content_type, "charset").unwrap_or_default(),
            content_id: header_field(head, "content-id")
                .trim_matches(|c: char| c == '<' || c == '>' || c.is_whitespace())
                .to_string(),
            attachment: header_field(head, "content-disposition")
                .to_ascii_lowercase()
                .starts_with("attachment"),
            name: file_name(head, &content_type),
            bytes: decode_transfer(&header_field(head, "content-transfer-encoding"), body),
        });
    }
}

/// The parts of a multipart body: what lies between its `--boundary` lines, without the line break before the next one.
fn parts_of<'a>(body: &'a [u8], boundary: &str) -> Vec<&'a [u8]> {
    let marker = format!("--{boundary}");
    let marker = marker.as_bytes();
    let mut parts = Vec::new();
    let Some(mut at) = find(body, marker, 0) else {
        return parts;
    };
    while parts.len() < MAX_LEAVES {
        let line_end = find(body, b"\n", at + marker.len()).unwrap_or(body.len());
        // The closing boundary is `--boundary--`: no part follows it.
        if body[at + marker.len()..line_end].starts_with(b"--") {
            break;
        }
        let start = (line_end + 1).min(body.len());
        let next = find(body, marker, start);
        let mut end = next.unwrap_or(body.len());
        if end > start && body[end - 1] == b'\n' {
            end -= 1;
            if end > start && body[end - 1] == b'\r' {
                end -= 1;
            }
        }
        parts.push(&body[start..end]);
        match next {
            Some(next) => at = next,
            None => break,
        }
    }
    parts
}

/// The headers of a message or a part, and its body (after the first blank line).
fn split_head(message: &[u8]) -> (&[u8], &[u8]) {
    match find(message, b"\r\n\r\n", 0) {
        Some(at) => (&message[..at], &message[at + 4..]),
        None => match find(message, b"\n\n", 0) {
            Some(at) => (&message[..at], &message[at + 2..]),
            None => (message, &[]),
        },
    }
}

/// Headers as text, a line that starts with a space or a tab going on the one before it.
fn unfold(head: &[u8]) -> String {
    String::from_utf8_lossy(head)
        .replace("\r\n ", " ")
        .replace("\r\n\t", " ")
        .replace("\n ", " ")
        .replace("\n\t", " ")
}

/// Where `needle` first appears in `haystack`, from `from` on.
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| from + at)
}

fn parameter(header: &str, name: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let at = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &header[at..];
    Some(match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().unwrap_or_default().to_string(),
        None => rest
            .split([';', ' ', '\r', '\n'])
            .next()
            .unwrap_or_default()
            .to_string(),
    })
}

fn quoted_printable(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] == b'=' {
            if body[i + 1..].starts_with(b"\r\n") {
                i += 3;
                continue;
            }
            if body[i + 1..].starts_with(b"\n") {
                i += 2;
                continue;
            }
            let hex = body
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(byte) = hex {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(body[i]);
        i += 1;
    }
    out
}

/// RFC 2047 words in a header («=?UTF-8?B?…?=», «=?iso-8859-1?Q?…?=»).
pub fn decode_words(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("=?") {
        let after = &rest[start + 2..];
        let mut parts = after.splitn(3, '?');
        let (Some(charset), Some(kind), Some(tail)) = (parts.next(), parts.next(), parts.next())
        else {
            break;
        };
        let Some(end) = tail.find("?=") else { break };
        let bytes = if kind.eq_ignore_ascii_case("b") {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(tail[..end].trim_end_matches('='))
                .unwrap_or_default()
        } else {
            quoted_printable(tail[..end].replace('_', " ").as_bytes())
        };
        let word: String = if charset.to_ascii_lowercase().starts_with("utf") {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            bytes.iter().map(|b| *b as char).collect()
        };
        // White space between two encoded words is not part of the text.
        let before = &rest[..start];
        if !(before.trim().is_empty() && !out.is_empty()) {
            out.push_str(before);
        }
        out.push_str(&word);
        rest = &tail[end + 2..];
    }
    out.push_str(rest);
    out
}

/// A header word encoded for a message Echo writes (a subject with accents).
pub fn encode_word(text: &str) -> String {
    if text.is_ascii() {
        return text.to_string();
    }
    use base64::Engine;
    format!(
        "=?UTF-8?B?{}?=",
        base64::engine::general_purpose::STANDARD.encode(text)
    )
}

fn strip_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len() / 2);
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Styles and scripts go whole; any other tag becomes a space, and the end of a block a new paragraph.
            let skip = ["style", "script", "head", "title"]
                .iter()
                .find(|t| lower[i + 1..].starts_with(*t));
            let end = match skip {
                Some(tag) => lower[i..]
                    .find(&format!("</{tag}"))
                    .map(|e| i + e)
                    .and_then(|e| lower[e..].find('>').map(|g| e + g)),
                None => lower[i..].find('>').map(|g| i + g),
            };
            match end {
                Some(end) => {
                    let tag = &lower[i + 1..end];
                    let block = skip.is_none()
                        && (tag.starts_with("br")
                            || tag.starts_with("/p")
                            || tag.starts_with("/div")
                            || tag.starts_with("/tr")
                            || tag.starts_with("/li")
                            || tag.starts_with("/h"));
                    text.push_str(if block { "\n\n" } else { " " });
                    i = end + 1;
                }
                None => break,
            }
        } else {
            let ch = html[i..].chars().next().unwrap_or(' ');
            text.push(ch);
            i += ch.len_utf8();
        }
    }
    for (entity, plain) in [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&aacute;", "á"),
        ("&eacute;", "é"),
        ("&iacute;", "í"),
        ("&oacute;", "ó"),
        ("&uacute;", "ú"),
        ("&ntilde;", "ñ"),
    ] {
        text = text.replace(entity, plain);
    }
    text
}

/// One line of spaces between words, control characters out, at most `max` characters.
pub fn one_line(text: &str, max: usize) -> String {
    let mut out = String::new();
    for word in text
        .split(|c: char| {
            c.is_whitespace()
                || c.is_control()
                || c == '\u{200c}'
                || c == '\u{feff}'
                || c == '\u{34f}'
        })
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        if out.chars().count() >= max {
            break;
        }
    }
    out.chars().take(max).collect()
}

/// The text as paragraphs the page shows one under the other: split at blank lines, each one on a line of its own.
pub fn paragraphs(text: &str) -> Vec<String> {
    let text = text.replace("\r\n", "\n");
    let mut out = Vec::new();
    let mut total = 0;
    for block in text.split("\n\n") {
        let paragraph = one_line(block, MAX_CHARS);
        if paragraph.is_empty() {
            continue;
        }
        total += paragraph.chars().count();
        out.push(paragraph);
        if out.len() >= MAX_PARAGRAPHS || total >= MAX_CHARS {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_the_sender_the_subject_and_the_flags_of_the_headers() {
        let header = b"From: =?UTF-8?B?THVjw61hIFByYWRv?= <Lucia@Ejemplo.test>\r\nSubject: =?utf-8?Q?Cena_del_s=C3=A1bado?=\r\nMessage-ID: <abc@ejemplo.test>\r\nList-Unsubscribe: <mailto:x@y.test>\r\nPrecedence: bulk\r\nAuthentication-Results: mx.test; spf=pass; dkim=fail header.d=x.test\r\n";
        let parsed = parse(header, b"hola");
        assert_eq!(
            (parsed.from_name.as_str(), parsed.from_address.as_str()),
            ("Lucía Prado", "lucia@ejemplo.test")
        );
        assert_eq!(parsed.subject, "Cena del sábado");
        assert_eq!(parsed.message_id, "<abc@ejemplo.test>");
        assert!(parsed.unsubscribe && parsed.bulk && parsed.auth_failed);
    }

    #[test]
    fn a_header_line_with_an_accent_where_a_name_would_end_does_not_panic() {
        let parsed = parse(
            "Subject: Tu pedido está en camino
From: a@b.test
"
            .as_bytes(),
            b"hola",
        );
        assert_eq!(parsed.subject, "Tu pedido está en camino");
        assert_eq!(parsed.from_address, "a@b.test");
    }

    #[test]
    fn it_reads_who_replies_go_to_and_everyone_the_message_went_to() {
        let header = b"From: Tienda <noreply@tienda.test>\r\nReply-To: \"Soporte, Tienda\" <Ayuda@Tienda.test>\r\nTo: Ana <ana@ejemplo.test>, \"Prado, Luis\" <luis@ejemplo.test>\r\nCc: marta@ejemplo.test;\r\n undisclosed-recipients:;\r\n";
        let parsed = parse(header, b"hola");
        assert_eq!(
            parsed.reply_to,
            [(
                "Soporte, Tienda".to_string(),
                "ayuda@tienda.test".to_string()
            )]
        );
        let others: Vec<&str> = parsed
            .others
            .iter()
            .map(|(_, address)| address.as_str())
            .collect();
        assert_eq!(
            others,
            [
                "ana@ejemplo.test",
                "luis@ejemplo.test",
                "marta@ejemplo.test"
            ]
        );
        assert_eq!(parsed.others[1].0, "Prado, Luis");
        assert!(parse(b"From: a@b.test\r\n", b"").reply_to.is_empty());
    }

    #[test]
    fn a_plain_message_has_none_of_the_flags() {
        let parsed = parse(b"From: ana@ejemplo.test\r\nSubject: Hola\r\nAuthentication-Results: mx; spf=softfail; dkim=pass\r\n", b"Hola\r\n");
        assert_eq!(
            (parsed.from_name.as_str(), parsed.from_address.as_str()),
            ("ana@ejemplo.test", "ana@ejemplo.test")
        );
        assert!(!parsed.unsubscribe && !parsed.bulk && !parsed.auth_failed);
    }

    #[test]
    fn it_takes_the_text_part_of_a_multipart_and_decodes_it() {
        let body = b"--b\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>solo html</p>\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nHola, ven a cenar. Ser=C3=A1 el s=C3=A1bado.\r\n--b--\r\n";
        let parsed = parse(
            b"Content-Type: multipart/alternative; boundary=\"b\"\r\n",
            body,
        );
        assert_eq!(parsed.text.trim(), "Hola, ven a cenar. Será el sábado.");
    }

    #[test]
    fn html_loses_its_tags_and_keeps_its_paragraphs() {
        let parsed = parse(
            b"Content-Type: text/html; charset=utf-8\r\n",
            b"<style>p{color:red}</style><p>Uno</p><p>Dos &amp; tres<br>cuatro</p>",
        );
        assert_eq!(
            paragraphs(&parsed.text),
            vec!["Uno", "Dos & tres", "cuatro"]
        );
    }

    #[test]
    fn control_characters_never_reach_the_page() {
        assert_eq!(one_line("a\u{0}b\u{202c}  c\r\nd", 50), "a b\u{202c} c d");
        assert_eq!(paragraphs("uno\ndos\n\n\n\ntres").len(), 2);
    }

    #[test]
    fn the_files_of_a_message_are_listed_by_name_and_one_is_read_by_its_place_and_never_by_a_path()
    {
        let message = b"Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n--m\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nAdjunto el contrato\r\n--m\r\nContent-Type: application/pdf; name=\"contrato.pdf\"\r\nContent-Disposition: attachment; filename=\"..\\..\\contrato final.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nAQID\r\n--m\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename*=UTF-8''informe%20mar%C3%A7o.xlsx\r\nContent-Transfer-Encoding: base64\r\n\r\nBAUG\r\n--m\r\nContent-Type: application/pkcs7-mime\r\nContent-Transfer-Encoding: base64\r\n\r\nBwgJ\r\n--m--\r\n";
        let whole = parse_whole(message);
        assert_eq!(whole.text.trim(), "Adjunto el contrato");
        let names: Vec<&str> = whole.attachments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "_.._contrato final.pdf",
                "informe março.xlsx",
                "adjunto.p7m"
            ],
            "a name never carries a folder, and a part without one gets its kind's ending"
        );
        assert_eq!(whole.attachments[0].size, 3);
        assert_eq!(
            attachment_at(message, 1).map(|(info, bytes)| (info.index, bytes)),
            Some((1, vec![4, 5, 6]))
        );
        assert!(attachment_at(message, 3).is_none());
    }

    #[test]
    fn each_file_is_looked_at_and_a_picture_is_shown_in_place_from_memory() {
        // A PNG's signature, then a program dressed as a PDF.
        let message = b"Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n--m\r\nContent-Type: text/plain\r\n\r\nFotos\r\n--m\r\nContent-Type: image/png\r\nContent-Disposition: attachment; filename=\"foto.png\"\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgoAAAA=\r\n--m\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"factura.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nTVqQAAMAAAA=\r\n--m--\r\n";
        let whole = parse_whole(message);
        let (photo, bill) = (&whole.attachments[0], &whole.attachments[1]);
        assert_eq!(photo.risk.level, crate::inspect::Level::Safe);
        assert!(
            photo
                .preview
                .as_deref()
                .is_some_and(|p| p.starts_with("data:image/png;base64,")),
            "{photo:?}"
        );
        assert_eq!(bill.risk.level, crate::inspect::Level::Danger);
        assert!(bill.preview.is_none());
    }

    #[test]
    fn a_whole_message_keeps_its_html_its_pictures_by_content_id_and_not_its_attachments() {
        let message = b"Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n--m\r\nContent-Type: multipart/related; boundary=\"r\"\r\n\r\n--r\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Total S/ 25.36</p><img src=\"cid:logo@echo.test\">\r\n--r\r\nContent-Type: image/png\r\nContent-ID: <logo@echo.test>\r\nContent-Transfer-Encoding: base64\r\n\r\nAQID\r\n--r--\r\n--m\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Disposition: attachment; filename=\"notas.txt\"\r\n\r\nNo debe leerse como cuerpo\r\n--m--\r\n";
        let whole = parse_whole(message);
        assert!(
            whole.html.contains("<p>Total S/ 25.36</p>"),
            "{}",
            whole.html
        );
        assert_eq!(
            whole.pictures.get("logo@echo.test"),
            Some(&("image/png".to_string(), vec![1, 2, 3]))
        );
        assert!(
            whole.text.contains("Total S/ 25.36") && !whole.text.contains("No debe leerse"),
            "{}",
            whole.text
        );
    }

    #[test]
    fn a_picture_marked_attachment_can_still_be_shown_in_place_by_its_content_id() {
        let message = b"Content-Type: multipart/mixed; boundary=\"m\"\r\n\r\n--m\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<img src=\"cid:qr@echo.test\">\r\n--m\r\nContent-Type: image/png; name=\"qr.png\"\r\nContent-Disposition: attachment; filename=\"qr.png\"\r\nContent-ID: <qr@echo.test>\r\nContent-Transfer-Encoding: base64\r\n\r\nAQID\r\n--m--\r\n";
        let whole = parse_whole(message);
        assert_eq!(
            whole.pictures.get("qr@echo.test"),
            Some(&("image/png".to_string(), vec![1, 2, 3]))
        );
        assert_eq!(whole.attachments.len(), 1);
    }

    #[test]
    fn a_whole_message_with_a_text_part_reads_that_part_as_its_text() {
        let whole = parse_whole(b"Content-Type: multipart/alternative; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/html\r\n\r\n<p>solo html</p>\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nSer=C3=A1 el s=C3=A1bado.\r\n--b--\r\n");
        assert_eq!(whole.text.trim(), "Será el sábado.");
        assert_eq!(whole.html, "<p>solo html</p>");
    }

    #[test]
    fn a_subject_with_accents_is_encoded_for_a_draft() {
        assert_eq!(encode_word("Re: hola"), "Re: hola");
        assert_eq!(decode_words(&encode_word("Re: sábado")), "Re: sábado");
    }

    #[test]
    fn the_unsubscribe_link_prefers_https_and_knows_one_press() {
        assert_eq!(
            unsubscribe_link(
                "<mailto:u@list.test?subject=stop>, <https://list.test/u?x=1>",
                "List-Unsubscribe=One-Click"
            ),
            Some(("https://list.test/u?x=1".into(), true))
        );
        assert_eq!(
            unsubscribe_link("<mailto:u@list.test>", ""),
            Some(("mailto:u@list.test".into(), false))
        );
        assert_eq!(
            unsubscribe_link("<http://list.test/u>, <javascript:x>", ""),
            None
        );
        assert_eq!(unsubscribe_link("https://no-brackets.test", ""), None);
    }
}
