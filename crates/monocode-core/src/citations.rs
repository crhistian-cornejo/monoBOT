//! Cited sources. An agent cites a source as a numbered markdown link whose
//! title carries the source's name and, after a " | ", the passage that backs
//! the claim:
//!
//! ```text
//! The curve matters [1](https://example.com/a "Great animations | Most UI motion…").
//! ```
//!
//! Anywhere else (a docx, a PDF, another viewer) it is still a plain link, so
//! nothing about the format needs the app to be read.
//!
//! Port of `src/features/sessions/model/citations.ts`, plus the small pieces of
//! `CitationMonogram.tsx` / `CitationSources.tsx` that are logic (the monogram
//! letter, the "N sources" label, how many monograms stack). UI flow: render a
//! link as a numbered chip when [`number`] and [`is_href`] accept it; under the
//! reply show [`sources_label`] and the list from [`collect`].

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// How many site monograms overlap in the collapsed "N sources" button.
pub const STACK_MONOGRAMS: usize = 4;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("citation pattern"))
}

/// The title of a citation link, split into the source's name and its passage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub title: String,
    pub snippet: Option<String>,
}

/// A source the text cites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub title: String,
    pub snippet: Option<String>,
    pub url: String,
    pub host: String,
    /// Order of first citation, from 1. The agent's own numbers are not trusted.
    pub number: u32,
}

/// The number in a link label such as `1` or `[12]`, or `None` for any other link.
pub fn number(label: &str) -> Option<u32> {
    static LABEL: OnceLock<Regex> = OnceLock::new();
    re(&LABEL, r"^\[?(\d{1,3})\]?$")
        .captures(label.trim())
        .and_then(|found| found[1].parse().ok())
}

/// Only web sources are citations; a file link stays a file link.
pub fn is_href(href: &str) -> bool {
    let lower = href.get(..8).unwrap_or(href).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// The site's host without `www.`; the input itself when it isn't a URL.
pub fn host(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let host = parsed.host_str().unwrap_or("");
            host.strip_prefix("www.").unwrap_or(host).to_owned()
        }
        Err(_) => url.to_owned(),
    }
}

/// Splits the link title into the source's title and the supporting passage.
pub fn parse_title(title: Option<&str>, url: &str) -> Text {
    static SEPARATOR: OnceLock<Regex> = OnceLock::new();
    let Some(text) = title.map(str::trim).filter(|t| !t.is_empty()) else {
        return Text {
            title: host(url),
            snippet: None,
        };
    };
    // A lone "|" between spaces; a pipe inside a word (a|b) is part of the title.
    let Some(separator) = re(&SEPARATOR, r"(?:^|\s)\|(?:\s|$)").find(text) else {
        return Text {
            title: text.into(),
            snippet: None,
        };
    };
    let head = text[..separator.start()].trim();
    let snippet = text[separator.end()..].trim();
    Text {
        title: if head.is_empty() {
            host(url)
        } else {
            head.into()
        },
        snippet: (!snippet.is_empty()).then(|| snippet.into()),
    }
}

/// The distinct sources a text cites, numbered in the order it first cites
/// them. Code is skipped, so an example of the format is not a citation.
pub fn collect(markdown: &str) -> Vec<Source> {
    static FENCE: OnceLock<Regex> = OnceLock::new();
    static INLINE: OnceLock<Regex> = OnceLock::new();
    static LINK: OnceLock<Regex> = OnceLock::new();
    let text = re(&FENCE, r"(?s)```.*?(?:```|$)").replace_all(markdown, "");
    let text = re(&INLINE, r"`[^`\n]*`").replace_all(&text, "");
    let link = re(
        &LINK,
        r#"\[\[?(\d{1,3})\]?\]\((https?://[^\s)]+)(?:\s+"((?:[^"\\]|\\.)*)")?\)"#,
    );
    let mut sources: Vec<Source> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for found in link.captures_iter(&text) {
        let url = found[2].to_owned();
        let title = found.get(3).map(|m| m.as_str().replace("\\\"", "\""));
        let parsed = parse_title(title.as_deref(), &url);
        if let Some(&index) = seen.get(&url) {
            // A later mention may carry the passage the first one lacked.
            let known = &mut sources[index];
            if known.snippet.is_none() && parsed.snippet.is_some() {
                known.snippet = parsed.snippet;
            }
            continue;
        }
        seen.insert(url.clone(), sources.len());
        sources.push(Source {
            title: parsed.title,
            snippet: parsed.snippet,
            host: host(&url),
            url,
            number: sources.len() as u32 + 1,
        });
    }
    sources
}

/// The letter that stands in for a site's icon: the host's first letter, past a
/// URL's scheme and `www.`, uppercase.
pub fn monogram(host: &str) -> String {
    static SCHEME: OnceLock<Regex> = OnceLock::new();
    static WWW: OnceLock<Regex> = OnceLock::new();
    let bare = re(&SCHEME, r"(?i)^[a-z]+://").replace(host, "");
    let bare = re(&WWW, r"(?i)^www\.").replace(&bare, "");
    bare.chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

/// "1 source" / "3 sources", the button under a reply.
pub fn sources_label(count: usize) -> String {
    format!("{count} {}", if count == 1 { "source" } else { "sources" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_number_of_a_citation_label_and_nothing_else() {
        assert_eq!(number("1"), Some(1));
        assert_eq!(number(" [12] "), Some(12));
        assert_eq!(number("MDN"), None);
        assert_eq!(number("2024"), None);
        assert_eq!(number("1.2"), None);
    }

    #[test]
    fn treats_only_web_links_as_citations() {
        assert!(is_href("https://example.com/a"));
        assert!(is_href("http://example.com"));
        assert!(is_href("HTTPS://example.com"));
        assert!(!is_href("src/app/App.tsx"));
        assert!(!is_href("javascript:alert(1)"));
        assert!(!is_href(""));
    }

    #[test]
    fn names_the_site_by_its_host_without_www() {
        assert_eq!(host("https://www.web.dev/articles/x"), "web.dev");
        assert_eq!(host("not a url"), "not a url");
    }

    #[test]
    fn splits_the_title_from_the_supporting_passage() {
        assert_eq!(
            parse_title(
                Some("Great animations | UI motion should be fast."),
                "https://emilkowal.ski/a"
            ),
            Text {
                title: "Great animations".into(),
                snippet: Some("UI motion should be fast.".into())
            }
        );
    }

    #[test]
    fn collects_distinct_sources_numbered_by_first_citation() {
        let sources = collect(
            "A [4](https://a.dev/x \"Alpha\") B [[2]](https://b.dev/y \"Beta | Passage\") C [9](https://a.dev/x \"Alpha | Late passage\").",
        );
        assert_eq!(
            sources,
            [
                Source {
                    url: "https://a.dev/x".into(),
                    host: "a.dev".into(),
                    number: 1,
                    title: "Alpha".into(),
                    snippet: Some("Late passage".into()),
                },
                Source {
                    url: "https://b.dev/y".into(),
                    host: "b.dev".into(),
                    number: 2,
                    title: "Beta".into(),
                    snippet: Some("Passage".into()),
                },
            ]
        );
    }

    #[test]
    fn ignores_the_format_when_it_sits_in_code_or_in_ordinary_links() {
        assert_eq!(
            collect(
                "Write `[1](https://a.dev)` like this:\n```\n[2](https://b.dev)\n```\nSee [docs](https://c.dev) and [2024](https://d.dev)."
            ),
            []
        );
    }

    #[test]
    fn keeps_a_bare_title_and_falls_back_to_the_host_without_one() {
        let plain = |title: &str| Text {
            title: title.into(),
            snippet: None,
        };
        assert_eq!(
            parse_title(Some("Only a title"), "https://a.dev/x"),
            plain("Only a title")
        );
        assert_eq!(parse_title(None, "https://www.a.dev/x"), plain("a.dev"));
        assert_eq!(
            parse_title(Some(" | just a passage"), "https://a.dev/x"),
            Text {
                title: "a.dev".into(),
                snippet: Some("just a passage".into())
            }
        );
        assert_eq!(parse_title(Some("a|b"), "https://a.dev/x"), plain("a|b"));
    }

    #[test]
    fn unescapes_quotes_in_the_title_and_stops_at_an_unclosed_fence() {
        let sources = collect("[1](https://a.dev \"Say \\\"hi\\\" | ok\") ```\n[2](https://b.dev)");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].title, "Say \"hi\"");
    }

    #[test]
    fn the_monogram_and_the_label() {
        assert_eq!(monogram("www.web.dev"), "W");
        assert_eq!(monogram("https://www.example.com/x"), "E");
        assert_eq!(monogram(""), "");
        assert_eq!(sources_label(1), "1 source");
        assert_eq!(sources_label(3), "3 sources");
    }
}
