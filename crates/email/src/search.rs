//! A search of one account, read only, with the words of Gmail's own search box: `from:`, `subject:`, `after:` and
//! `before:` (a date `AAAA/MM/DD` or Unix seconds), `{…}` for «any of these», and other words that the text holds.
//! Gmail is asked with them as they are (`X-GM-RAW`, over «Todos»); any other server with the IMAP criteria they
//! become (`SINCE`, `BEFORE`, `FROM`, `SUBJECT`, `TEXT`), and what such a server cannot be asked (a word with accents,
//! the exact second of `after:`) is checked here, on what came back. Nothing is marked as read (`EXAMINE`, `BODY.PEEK`).

use crate::imap::{quote, Raw};
use crate::mime;
use crate::triage::fold;

/// The longest search, in characters, and the most of a message's text a search gives back.
pub const MAX_QUERY: usize = 300;
const MAX_TEXT: usize = 4_000;

/// A message a search found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub uid: u64,
    /// When it arrived, in Unix seconds (0: the server did not say).
    pub at: i64,
    /// Gmail's message and thread ids in hexadecimal (the ones of its web addresses); empty on other servers.
    pub gmail_id: String,
    pub gmail_thread: String,
    /// The `Message-ID` header, the same on every server that keeps the message.
    pub message_id: String,
    pub from_name: String,
    pub from_address: String,
    pub subject: String,
    /// The first lines of its text (no HTML, no attachments).
    pub text: String,
}

/// What a search asks, read from Gmail's words.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Query {
    /// `{from:a from:b}`: from any of them.
    pub any_from: Vec<String>,
    pub from: Vec<String>,
    pub subject: Vec<String>,
    pub words: Vec<String>,
    /// Unix seconds.
    pub after: Option<i64>,
    pub before: Option<i64>,
    /// The words written with accents: a server is not asked for them (its quoted strings are ASCII, and without
    /// the accent they would not match), `holds` checks them.
    pub here_only: Vec<String>,
}

impl Query {
    pub fn parse(text: &str) -> Self {
        let mut query = Query::default();
        let mut grouped = false;
        for token in text.split_whitespace() {
            let opens = token.starts_with('{');
            let closes = token.ends_with('}');
            let token = token.trim_start_matches('{').trim_end_matches('}').trim_matches('"');
            if opens {
                grouped = true;
            }
            let in_group = grouped;
            if closes {
                grouped = false;
            }
            // A word left out (`-promo`) or an operator Echo does not know is not asked: the search is wider, never narrower.
            if token.is_empty() || token.starts_with('-') || token == "OR" {
                continue;
            }
            if !token.is_ascii() {
                query.here_only.push(fold(token.split_once(':').map_or(token, |(_, value)| value)));
            }
            match token.split_once(':') {
                Some(("from", who)) if !who.is_empty() => if in_group { query.any_from.push(fold(who)) } else { query.from.push(fold(who)) },
                Some(("subject", what)) if !what.is_empty() => query.subject.push(fold(what)),
                Some(("after" | "newer", when)) => query.after = date(when),
                Some(("before" | "older", when)) => query.before = date(when),
                Some(_) => {}
                None => query.words.push(fold(token)),
            }
        }
        query
    }

    /// The IMAP criteria of a server that is not Gmail. Only what can go as a plain quoted string is asked; the rest
    /// is checked by `holds`.
    pub fn criteria(&self) -> String {
        let plain = |text: &str| text.is_ascii() && !text.chars().any(|c| c.is_control()) && !self.here_only.iter().any(|w| w == text);
        let mut parts = Vec::new();
        if let Some(after) = self.after {
            parts.push(format!("SINCE {}", imap_date(after)));
        }
        if let Some(before) = self.before {
            // BEFORE is by day: the day of `before` itself is still asked for and `holds` cuts it.
            parts.push(format!("BEFORE {}", imap_date(before + 86_400)));
        }
        for who in self.from.iter().filter(|w| plain(w)) {
            parts.push(format!("FROM {}", quote(who)));
        }
        for what in self.subject.iter().filter(|w| plain(w)) {
            parts.push(format!("SUBJECT {}", quote(what)));
        }
        for word in self.words.iter().filter(|w| plain(w)) {
            parts.push(format!("TEXT {}", quote(word)));
        }
        let any: Vec<&String> = self.any_from.iter().filter(|w| plain(w)).collect();
        if !any.is_empty() && any.len() == self.any_from.len() {
            // `OR FROM a OR FROM b FROM c`: IMAP's OR takes two.
            let mut group = format!("FROM {}", quote(any[any.len() - 1]));
            for who in any.iter().rev().skip(1) {
                group = format!("OR FROM {} {group}", quote(who));
            }
            parts.push(group);
        }
        if parts.is_empty() { "ALL".into() } else { parts.join(" ") }
    }

    /// Whether a message found on a server that is not Gmail is what was asked.
    pub fn holds(&self, found: &Found) -> bool {
        let sender = fold(&format!("{} {}", found.from_name, found.from_address));
        let subject = fold(&found.subject);
        let all = format!("{sender} {subject} {}", fold(&found.text));
        let after = self.after.is_none_or(|after| found.at == 0 || found.at >= after);
        let before = self.before.is_none_or(|before| found.at == 0 || found.at < before);
        after
            && before
            && self.from.iter().all(|who| sender.contains(who.as_str()))
            && (self.any_from.is_empty() || self.any_from.iter().any(|who| sender.contains(who.as_str())))
            && self.subject.iter().all(|what| subject.contains(what.as_str()))
            && self.words.iter().all(|word| all.contains(word.as_str()))
    }
}

/// A message of a search's fetch, as the core is given it.
pub(crate) fn found_of(raw: &Raw) -> Found {
    let parsed = mime::parse(&raw.header, &raw.text);
    Found {
        uid: raw.uid,
        at: raw.date.map_or(0, |ms| ms / 1000),
        gmail_id: raw.gmail_id.map(|id| format!("{id:x}")).unwrap_or_default(),
        gmail_thread: raw.gmail_thread.or(raw.gmail_id).map(|id| format!("{id:x}")).unwrap_or_default(),
        message_id: parsed.message_id,
        from_name: parsed.from_name,
        from_address: parsed.from_address,
        subject: parsed.subject,
        text: parsed.text.chars().take(MAX_TEXT).collect(),
    }
}

/// `AAAA/MM/DD`, `AAAA-MM-DD` or Unix seconds, as Unix seconds (the day's start, in UTC).
fn date(text: &str) -> Option<i64> {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) {
        return text.parse().ok();
    }
    let mut parts = text.split(['/', '-']);
    let (year, month, day): (i64, i64, i64) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then(|| crate::imap::days_from_civil(year, month, day) * 86_400)
}

/// Unix seconds as IMAP's date: `5-Oct-2026`.
fn imap_date(at: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (year, month, day) = crate::service::civil_from_days(at.div_euclid(86_400));
    format!("{day}-{}-{year}", MONTHS[(month - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmails_words_become_imap_criteria_and_what_cannot_go_is_checked_here() {
        let query = Query::parse("{from:bcp.com.pe from:yape.pe} after:1792231200 subject:Consumo café -promo");
        assert_eq!(query.any_from, vec!["bcp.com.pe", "yape.pe"]);
        assert_eq!((query.after, query.subject.as_slice(), query.words.as_slice()), (Some(1_792_231_200), &["consumo".to_string()][..], &["cafe".to_string()][..]));
        // («café» has an accent: the server is not asked for it.)
        assert_eq!(query.criteria(), "SINCE 17-Oct-2026 SUBJECT \"consumo\" OR FROM \"bcp.com.pe\" FROM \"yape.pe\"");
        assert_eq!(Query::parse("cafe").criteria(), "TEXT \"cafe\"");
        assert_eq!(Query::parse("from:tienda.test before:2026/10/05").criteria(), "BEFORE 6-Oct-2026 FROM \"tienda.test\"");
        assert_eq!(Query::parse("").criteria(), "ALL");
        // A word with accents is not asked of the server (its quoted strings are ASCII), and is still checked.
        let accented = Query::parse("subject:operación");
        assert_eq!(accented.criteria(), "ALL");
        let found = |subject: &str, at: i64| Found { from_name: "BCP".into(), from_address: "avisos@bcp.com.pe".into(), subject: subject.into(), at, ..Found::default() };
        assert!(accented.holds(&found("Operación realizada", 5)));
        assert!(!accented.holds(&found("Bienvenido", 5)));
        // `after:` in seconds is finer than IMAP's day.
        let later = Query::parse("{from:bcp.com.pe from:yape.pe} after:100");
        assert!(later.holds(&found("x", 100)) && !later.holds(&found("x", 99)));
        assert!(!Query::parse("from:interbank.pe").holds(&found("x", 5)));
    }

    #[test]
    fn a_quote_in_the_words_cannot_end_the_command() {
        assert_eq!(Query::parse("from:a\"b@x.test").criteria(), "FROM \"a\\\"b@x.test\"");
        assert_eq!(Query::parse("from:a\\b").criteria(), "FROM \"a\\\\b\"");
    }
}
