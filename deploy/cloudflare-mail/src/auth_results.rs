//! Conservative reading of the receiver's `Authentication-Results` header.

use std::collections::HashMap;

const MAX_HEADER_BYTES: usize = 64 * 1024;

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// The raw header block as a latin1 string, or `None` if no blank line ends it
/// within 64 KiB (oversized or malformed header section).
pub fn extract_header_block(raw: &[u8]) -> Option<String> {
    let limit = raw.len().min(MAX_HEADER_BYTES);
    for i in 0..limit {
        if raw[i] != b'\n' {
            continue;
        }
        // "\n\n" or "\n\r\n"; the first line may be empty only for header-less mail.
        let next = raw.get(i + 1).copied();
        if next == Some(b'\n') || (next == Some(b'\r') && raw.get(i + 2) == Some(&b'\n')) {
            return Some(latin1(&raw[..=i]));
        }
    }
    (raw.len() <= MAX_HEADER_BYTES).then(|| latin1(raw))
}

/// Joins folded continuation lines (`\r?\n` followed by spaces or tabs) into one space.
fn unfold(block: &str) -> String {
    let chars: Vec<char> = block.chars().collect();
    let mut out = String::with_capacity(block.len());
    let mut i = 0;
    while i < chars.len() {
        let newline_at = if chars[i] == '\r' && chars.get(i + 1) == Some(&'\n') {
            Some(i + 1)
        } else if chars[i] == '\n' {
            Some(i)
        } else {
            None
        };
        if let Some(n) = newline_at {
            let mut j = n + 1;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            if j > n + 1 {
                out.push(' ');
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub fn header_values(block: &str, name: &str) -> Vec<String> {
    let wanted = name.to_lowercase();
    unfold(block)
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter_map(|line| {
            let colon = line.find(':').filter(|&c| c > 0)?;
            (line[..colon].trim().to_lowercase() == wanted)
                .then(|| line[colon + 1..].trim().to_string())
        })
        .collect()
}

/// Removes `( … )` comments, innermost first, until none are left.
fn strip_comments(value: &str) -> String {
    let mut current = value.to_string();
    loop {
        let mut next = String::with_capacity(current.len());
        let mut open: Option<usize> = None;
        let mut copied = 0;
        for (i, c) in current.char_indices() {
            match c {
                '(' => open = Some(i),
                ')' => {
                    if let Some(start) = open.take() {
                        next.push_str(&current[copied..start]);
                        next.push(' ');
                        copied = i + 1;
                    }
                }
                _ => {}
            }
        }
        next.push_str(&current[copied..]);
        if next == current {
            return current;
        }
        current = next;
    }
}

struct MethodResult {
    result: String,
    props: HashMap<String, String>,
}

struct Parsed {
    authserv_id: String,
    methods: HashMap<String, Vec<MethodResult>>,
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn take_while(s: &str, keep: impl Fn(char) -> bool) -> (&str, &str) {
    let end = s
        .char_indices()
        .find(|&(_, c)| !keep(c))
        .map_or(s.len(), |(i, _)| i);
    s.split_at(end)
}

/// `method = result rest`, e.g. `spf=pass smtp.mailfrom=a@b`.
fn method_head(part: &str) -> Option<(String, String, &str)> {
    let s = part.trim_start();
    let (method, s) = take_while(s, |c| c.is_ascii_alphanumeric() || c == '-');
    if method.is_empty() {
        return None;
    }
    let s = s.trim_start().strip_prefix('=')?.trim_start();
    let (result, rest) = take_while(s, |c| c.is_ascii_alphabetic());
    if result.is_empty() || rest.chars().next().is_some_and(is_word) {
        return None;
    }
    Some((method.to_lowercase(), result.to_lowercase(), rest))
}

/// `ptype.property = value` pairs, e.g. `smtp.mailfrom=a@b` or `header.d="b"`.
fn properties(rest: &str) -> HashMap<String, String> {
    let mut props = HashMap::new();
    let mut i = 0;
    while i < rest.len() {
        if !rest.is_char_boundary(i) {
            i += 1;
            continue;
        }
        if let Some((key, value, len)) = property_at(&rest[i..]) {
            let value = value.strip_prefix('"').unwrap_or(value);
            let value = value.strip_suffix('"').unwrap_or(value);
            props.insert(key.to_lowercase(), value.to_lowercase());
            i += len;
        } else {
            i += 1;
        }
    }
    props
}

fn property_at(s: &str) -> Option<(&str, &str, usize)> {
    let (ptype, after) = take_while(s, |c| c.is_ascii_alphabetic());
    if ptype.is_empty() {
        return None;
    }
    let after = after.strip_prefix('.')?;
    let (name, after) = take_while(after, |c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if name.is_empty() {
        return None;
    }
    let key = &s[..ptype.len() + 1 + name.len()];
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let value = if let Some(close) = after.strip_prefix('"').and_then(|v| v.find('"')) {
        &after[..close + 2]
    } else {
        let (value, _) = take_while(after, |c| !c.is_whitespace() && c != ';');
        if value.is_empty() {
            return None;
        }
        value
    };
    let len = s.len() - after.len() + value.len();
    Some((key, value, len))
}

fn parse(value: &str) -> Option<Parsed> {
    let stripped = strip_comments(value);
    let mut parts = stripped.split(';');
    let authserv_id = parts.next()?.split_whitespace().next()?.to_lowercase();
    let mut methods: HashMap<String, Vec<MethodResult>> = HashMap::new();
    for part in parts {
        if let Some((method, result, rest)) = method_head(part) {
            methods.entry(method).or_default().push(MethodResult {
                result,
                props: properties(rest),
            });
        }
    }
    Some(Parsed {
        authserv_id,
        methods,
    })
}

fn domain_of(value: &str) -> &str {
    let domain = &value[value.rfind('@').map_or(0, |at| at + 1)..];
    let domain = domain.strip_prefix('<').unwrap_or(domain);
    domain.strip_suffix('>').unwrap_or(domain)
}

/// True unless the message carries exactly one `Authentication-Results`
/// header from the trusted authserv-id proving SPF on the envelope sender's
/// domain and DMARC.
///
/// Senders control every header in the raw message, so a forged header cannot
/// be told apart from the real one by content. The defence is structural:
/// Cloudflare prepends its own header, therefore a forgery shows up as a second
/// header with the trusted id and is rejected as ambiguous.
pub fn is_auth_failed(raw: &[u8], envelope_from: &str, trusted_authserv_id: &str) -> bool {
    let Some(block) = extract_header_block(raw) else {
        return true;
    };
    let trusted = trusted_authserv_id.trim().to_lowercase();
    let candidates: Vec<Parsed> = header_values(&block, "Authentication-Results")
        .iter()
        .filter_map(|value| parse(value))
        .filter(|parsed| parsed.authserv_id == trusted)
        .collect();
    let [only] = candidates.as_slice() else {
        return true;
    };
    let (Some([spf]), Some([dmarc])) = (
        only.methods.get("spf").map(Vec::as_slice),
        only.methods.get("dmarc").map(Vec::as_slice),
    ) else {
        return true;
    };
    if spf.result != "pass" || dmarc.result != "pass" {
        return true;
    }
    let Some(mail_from) = spf.props.get("smtp.mailfrom") else {
        return true;
    };
    domain_of(mail_from) != domain_of(&envelope_from.to_lowercase())
}
