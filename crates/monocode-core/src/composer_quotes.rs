//! Text added to the chat with "Add to chat" (from an answer, a paper, a file)
//! waits in the composer as a compact chip instead of a block of `>` lines in
//! the draft. Sending puts each one back as a quote, before what was typed, so
//! the agent reads the same message it did when quotes went into the draft.
//!
//! Port of `src/features/sessions/model/composerQuotes.ts` and the quote part of
//! `draftCache.ts`.
//!
//! UI flow: "Add to chat" calls [`add`] on the session's quotes (and
//! [`QuoteDrafts::set`] to keep them); the chip shows [`chip_label`] and
//! [`chip_source`]; on send, the message text is [`with_quotes`] and the quotes
//! are cleared ([`QuoteDrafts::clear`]). Plain inserts still go into the draft
//! text through [`crate::composer`].
//!
//! Persistence: Tauri keeps quotes in memory only (a map by session id, cleared
//! with the draft). [`QuoteDrafts`] is that map. [`save`] / [`load`] additionally
//! keep it in the store setting [`SETTING_KEY`] (`monocode.composerQuotes`) so the
//! native app can survive a restart; the UI may skip them to match Tauri.

use crate::composer::append_selection_quote;
use crate::store::Store;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Store setting that holds the waiting quotes of every session.
pub const SETTING_KEY: &str = "monocode.composerQuotes";

/// Longest chip label, in characters.
pub const CHIP_LABEL_MAX: usize = 64;

/// One quote waiting beside the attachments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    pub id: String,
    pub text: String,
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The quoted words and, when the quote ends with one, the line that says where
/// they come from (`— Author (year), “Title”, p. 3 (link)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    pub body: String,
    pub source: Option<String>,
}

pub fn split_source(text: &str) -> Split {
    static SOURCE: OnceLock<Regex> = OnceLock::new();
    let value = normalize(text).trim().to_owned();
    let pattern = SOURCE.get_or_init(|| Regex::new(r"\n\s*\n(— [^\n]+)$").expect("source line"));
    match pattern.captures(&value) {
        Some(found) => Split {
            body: value[..found.get(0).map_or(0, |m| m.start())]
                .trim()
                .to_owned(),
            source: Some(found[1].to_owned()),
        },
        None => Split {
            body: value,
            source: None,
        },
    }
}

/// The chip's words: the start of the quote on one line, `max` characters at
/// most (see [`CHIP_LABEL_MAX`]).
pub fn chip_label(text: &str, max: usize) -> String {
    let body = split_source(text).body;
    let line = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > max {
        let head: String = line.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", head.trim_end())
    } else {
        line
    }
}

/// Where the quote is from, without the link, for under the chip's words.
pub fn chip_source(text: &str) -> Option<String> {
    static LEAD: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    let source = split_source(text).source?;
    let lead = LEAD.get_or_init(|| Regex::new(r"^—\s*").expect("lead"));
    let link = LINK.get_or_init(|| Regex::new(r"\s*\([^()]*\)$").expect("link"));
    let source = lead.replace(&source, "");
    Some(link.replace(&source, "").into_owned())
}

/// The file and 1-based page a quote from a PDF names in its source line
/// (`— name, p. 3 (/path/to/file.pdf)`).
pub fn paper_reference(text: &str) -> Option<(String, usize)> {
    let source = split_source(text).source?;
    let inner = source.strip_suffix(')')?;
    let open = inner.rfind(" (")?;
    let path = inner[open + 2..].trim();
    let page = inner[..open].rsplit_once(", p. ")?.1.trim().parse().ok()?;
    path.to_ascii_lowercase()
        .ends_with(".pdf")
        .then(|| (path.to_owned(), page))
}

/// Adds a quote unless the same words are already waiting. Returns the new list.
pub fn add(quotes: &[Quote], text: &str, id: &str) -> Vec<Quote> {
    let value = normalize(text).trim().to_owned();
    let mut next = quotes.to_vec();
    if value.is_empty() || quotes.iter().any(|quote| quote.text == value) {
        return next;
    }
    next.push(Quote {
        id: id.into(),
        text: value,
    });
    next
}

/// Removes the chip with `id`.
pub fn remove(quotes: &[Quote], id: &str) -> Vec<Quote> {
    quotes.iter().filter(|q| q.id != id).cloned().collect()
}

/// The message as sent: every quote as a `>` block, then what was typed.
pub fn with_quotes(message: &str, quotes: &[Quote]) -> String {
    if quotes.is_empty() {
        return message.into();
    }
    let quoted = quotes
        .iter()
        .fold(String::new(), |draft, quote| {
            append_selection_quote(&draft, &quote.text)
        })
        .trim_end()
        .to_owned();
    if message.trim().is_empty() {
        quoted
    } else {
        format!("{quoted}\n\n{message}")
    }
}

/// A sent message read back as [`with_quotes`] wrote it: the quotes it
/// opens with, unquoted, and what was typed after them. A message that does
/// not open with a quote comes back whole, with no quotes.
pub fn split_sent(message: &str) -> (Vec<String>, String) {
    let message = normalize(message);
    let lines: Vec<&str> = message.split('\n').collect();
    let mut quotes = Vec::new();
    let mut at = 0;
    loop {
        while lines.get(at).is_some_and(|line| line.trim().is_empty()) {
            at += 1;
        }
        let start = at;
        while lines.get(at).is_some_and(|line| line.starts_with('>')) {
            at += 1;
        }
        if at == start {
            break;
        }
        let quote = lines[start..at]
            .iter()
            .map(|line| {
                let line = &line[1..];
                line.strip_prefix(' ').unwrap_or(line)
            })
            .collect::<Vec<_>>()
            .join("\n");
        quotes.push(quote.trim().to_owned());
    }
    if quotes.is_empty() {
        return (Vec::new(), message);
    }
    (
        quotes,
        lines[at.min(lines.len())..].join("\n").trim().to_owned(),
    )
}

/// The quotes waiting in each session's composer, by session id. Kept apart from
/// the draft text: clearing what was typed leaves the quotes waiting, and
/// clearing the whole draft ([`QuoteDrafts::clear`]) drops them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuoteDrafts(BTreeMap<String, Vec<Quote>>);

impl QuoteDrafts {
    pub fn get(&self, session: &str) -> &[Quote] {
        self.0.get(session).map_or(&[], Vec::as_slice)
    }

    /// An empty list removes the entry.
    pub fn set(&mut self, session: &str, quotes: Vec<Quote>) {
        if quotes.is_empty() {
            self.0.remove(session);
        } else {
            self.0.insert(session.into(), quotes);
        }
    }

    pub fn clear(&mut self, session: &str) {
        self.0.remove(session);
    }
}

/// Keep every session's waiting quotes in the store.
pub fn save(store: &Store, drafts: &QuoteDrafts) -> Result<(), String> {
    store.save_setting(
        SETTING_KEY,
        &serde_json::to_string(drafts).map_err(|e| e.to_string())?,
    )
}

/// The saved quotes; empty when none were saved or the setting is unreadable.
pub fn load(store: &Store) -> Result<QuoteDrafts, String> {
    Ok(store
        .setting(SETTING_KEY)?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FROM_PAPER: &str = "Flexible models need scripting.\n\n— Chlumsky et al. (2022), “RavenR v2.1.4”, p. 3 (https://doi.org/10.5194/gmd-15-7017-2022)";

    fn quote(id: &str, text: &str) -> Quote {
        Quote {
            id: id.into(),
            text: text.into(),
        }
    }

    #[test]
    fn separates_the_words_from_the_line_that_says_where_they_are_from() {
        assert_eq!(
            split_source(FROM_PAPER),
            Split {
                body: "Flexible models need scripting.".into(),
                source: Some("— Chlumsky et al. (2022), “RavenR v2.1.4”, p. 3 (https://doi.org/10.5194/gmd-15-7017-2022)".into()),
            }
        );
    }

    #[test]
    fn leaves_a_quote_from_the_chat_whole() {
        assert_eq!(
            split_source("Line one\nline two"),
            Split {
                body: "Line one\nline two".into(),
                source: None
            }
        );
    }

    #[test]
    fn shows_the_start_of_the_quote_on_one_line() {
        assert_eq!(chip_label("a\nb   c", CHIP_LABEL_MAX), "a b c");
        assert_eq!(
            chip_label(&"x".repeat(80), 10),
            format!("{}…", "x".repeat(9))
        );
    }

    #[test]
    fn names_the_paper_and_page_without_the_link() {
        assert_eq!(
            chip_source(FROM_PAPER).as_deref(),
            Some("Chlumsky et al. (2022), “RavenR v2.1.4”, p. 3")
        );
        assert_eq!(chip_source("plain"), None);
    }

    #[test]
    fn a_quote_from_a_pdf_names_its_file_and_page() {
        let quote =
            "Words.\n\n— airGR and teaching, p. 3 (/Users/a/Research/airGR and teaching.pdf)";
        assert_eq!(
            paper_reference(quote),
            Some(("/Users/a/Research/airGR and teaching.pdf".into(), 3))
        );
        assert_eq!(
            chip_source(quote).as_deref(),
            Some("airGR and teaching, p. 3")
        );
        assert_eq!(paper_reference(FROM_PAPER), None, "a link is not a file");
        assert_eq!(paper_reference("plain"), None);
    }

    #[test]
    fn adds_each_quote_once() {
        let one = add(&[], " words ", "a");
        assert_eq!(one, [quote("a", "words")]);
        assert_eq!(add(&one, "words", "b"), one);
        assert_eq!(add(&one, "  ", "c"), one);
        assert_eq!(remove(&one, "a"), []);
    }

    #[test]
    fn sends_the_quotes_as_blocks_before_what_was_typed() {
        assert_eq!(
            with_quotes(
                "¿Qué método usa?",
                &[quote("a", "First\nquote"), quote("b", "Second")]
            ),
            "> First\n> quote\n\n> Second\n\n¿Qué método usa?"
        );
    }

    #[test]
    fn reads_a_sent_message_back_into_its_quotes_and_what_was_typed() {
        let quotes = [quote("a", FROM_PAPER), quote("b", "Second\nline")];
        let sent = with_quotes("¿Qué método usa?", &quotes);
        assert_eq!(
            split_sent(&sent),
            (
                vec![FROM_PAPER.to_owned(), "Second\nline".to_owned()],
                "¿Qué método usa?".to_owned()
            )
        );
        assert_eq!(
            split_sent(&with_quotes("", &quotes[..1])),
            (vec![FROM_PAPER.to_owned()], String::new())
        );
        assert_eq!(
            split_sent("hola\n> no es una cita al inicio"),
            (Vec::new(), "hola\n> no es una cita al inicio".to_owned())
        );
    }

    #[test]
    fn sends_only_the_quotes_when_nothing_was_typed() {
        assert_eq!(with_quotes("  ", &[quote("a", "Only")]), "> Only");
    }

    #[test]
    fn leaves_a_message_without_quotes_as_it_is() {
        assert_eq!(with_quotes("hola", &[]), "hola");
    }

    #[test]
    fn keeps_quotes_per_session_and_in_the_store() {
        let mut drafts = QuoteDrafts::default();
        drafts.set("s1", vec![quote("a", "x")]);
        drafts.set("s2", vec![quote("b", "y")]);
        drafts.set("s2", vec![]);
        assert_eq!(drafts.get("s1"), [quote("a", "x")]);
        assert_eq!(drafts.get("s2"), []);
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert_eq!(load(&store).unwrap(), QuoteDrafts::default());
        save(&store, &drafts).unwrap();
        assert_eq!(load(&store).unwrap(), drafts);
        drafts.clear("s1");
        assert_eq!(drafts.get("s1"), []);
    }
}
