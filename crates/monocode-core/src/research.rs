//! What a turn looked at, read from its tool calls: the web searches it ran,
//! the pages it opened and which of them its answer ended up citing. The chat
//! shows the answer; this is the legwork behind it.
//!
//! Port of `src/features/monos/model/turnResearch.ts`.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::citations;
use crate::response;
use crate::tool::ToolCall;
use crate::transcript::Row;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    /// How many pages the search listed.
    pub results: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub host: String,
    pub title: Option<String>,
    /// Its number in the answer's citations, when the answer cites it.
    pub cited: Option<u32>,
    /// The agent opened the page, rather than only seeing it in results.
    pub read: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnResearch {
    pub searches: Vec<Search>,
    pub sources: Vec<Source>,
    /// Search results it neither opened nor cited.
    pub skipped: usize,
}

impl TurnResearch {
    pub fn is_empty(&self) -> bool {
        self.searches.is_empty() && self.sources.is_empty()
    }
}

/// The tool calls in a turn's rows. A call spans its first row and the rows
/// that continue it.
pub fn tool_calls(rows: &[Row]) -> Vec<ToolCall> {
    let mut texts: Vec<String> = Vec::new();
    let mut previous_tool = false;
    for row in rows {
        if row.role != "tool" {
            previous_tool = false;
            continue;
        }
        if row.first || !previous_tool {
            texts.push(String::new());
        }
        if let Some(text) = texts.last_mut() {
            text.push_str(&row.text);
        }
        previous_tool = true;
    }
    texts
        .iter()
        .filter_map(|text| serde_json::from_str(text).ok())
        .collect()
}

/// A page's address without its fragment, query or trailing slash, so one page
/// is one source however it was linked.
fn same_url(url: &str) -> String {
    let end = url.find(['#', '?']).unwrap_or(url.len());
    url[..end].trim_end_matches('/').to_lowercase()
}

fn is_http(url: &str) -> bool {
    let start = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    start.starts_with("http://") || start.starts_with("https://")
}

/// `mcp__exa__web_search_exa` and `WebSearch` both read as `websearchexa` /
/// `websearch`.
fn kind(name: &str) -> String {
    let tool = name
        .rsplit_once("__")
        .or_else(|| name.rsplit_once(':'))
        .map_or(name, |(_, tool)| tool);
    tool.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn is_search(call: &ToolCall) -> bool {
    matches!(
        kind(&call.name).as_str(),
        "websearch" | "searchweb" | "websearchexa" | "websearchpreview"
    )
}

/// The `Links: [{"title":…,"url":…}]` list a web search ends with.
fn search_links(output: &str) -> Vec<(String, Option<String>)> {
    let Some(at) = output.find("Links:") else {
        return Vec::new();
    };
    let Some(start) = output[at..].find('[').map(|i| at + i) else {
        return Vec::new();
    };
    let end = output[start..]
        .find('\n')
        .map_or(output.len(), |i| start + i);
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(output[start..end].trim()) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let url = entry["url"].as_str().filter(|url| is_http(url))?;
            let title = entry["title"]
                .as_str()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_owned);
            Some((url.to_owned(), title))
        })
        .collect()
}

/// The page a call opened: a fetch tool's `url`, the app's `web.read` /
/// `web.download`, or the address a provider previews.
fn fetched_url(call: &ToolCall) -> Option<String> {
    let input: Value = serde_json::from_str(&call.input).unwrap_or(Value::Null);
    let preview = call
        .display
        .as_ref()
        .map(|display| &display["preview"])
        .unwrap_or(&Value::Null);
    let previewed = ["url", "path"]
        .iter()
        .find_map(|key| preview[*key].as_str())
        .filter(|url| is_http(url));
    if let Some(url) = previewed {
        return Some(url.to_owned());
    }
    let opens_pages = kind(&call.name).contains("fetch")
        || input["action"]
            .as_str()
            .is_some_and(|action| action.starts_with("web."));
    if opens_pages {
        if let Some(url) = input["url"].as_str().filter(|url| is_http(url)) {
            return Some(url.to_owned());
        }
    }
    let shown = call.presentation();
    let command = shown.command.as_deref()?;
    matches!(
        crate::tool::app_cli_action(command),
        Some("web.read" | "web.download")
    )
    .then(|| {
        command
            .split(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '\\'))
            .find(|word| is_http(word))
            .map(str::to_owned)
    })
    .flatten()
}

/// What the turn in `rows` researched. `rows` are the turn's rows; the answer
/// is its last assistant message.
pub fn turn_research(rows: &[Row]) -> TurnResearch {
    let calls = tool_calls(rows);
    let mut searches = Vec::new();
    let mut sources: Vec<(String, Source)> = Vec::new();
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut listed: Vec<String> = Vec::new();
    let mut seen = 0;
    fn add(sources: &mut Vec<(String, Source)>, url: &str, update: impl FnOnce(&mut Source)) {
        let key = same_url(url);
        let at = match sources.iter().position(|(known, _)| *known == key) {
            Some(at) => at,
            None => {
                sources.push((
                    key,
                    Source {
                        url: url.to_owned(),
                        host: citations::host(url),
                        title: None,
                        cited: None,
                        read: false,
                    },
                ));
                sources.len() - 1
            }
        };
        update(&mut sources[at].1);
    }
    for call in &calls {
        if is_search(call) {
            let query = call.presentation().target;
            if query.trim().is_empty() {
                continue;
            }
            let links = search_links(&call.output);
            for (url, title) in &links {
                if let Some(title) = title {
                    titles.insert(same_url(url), title.clone());
                }
                listed.push(same_url(url));
            }
            seen += links.len();
            searches.push(Search {
                query,
                results: links.len(),
            });
        } else if let Some(url) = fetched_url(call) {
            add(&mut sources, &url, |source| source.read = true);
        }
    }
    let answer = response::final_prose(rows);
    for cited in citations::collect(&answer) {
        add(&mut sources, &cited.url, |source| {
            source.cited = Some(cited.number);
            if cited.title != cited.host {
                source.title = Some(cited.title.clone());
            }
        });
    }
    let used: HashSet<&String> = sources.iter().map(|(key, _)| key).collect();
    let skipped = listed
        .iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .filter(|url| !used.contains(*url))
        .count();
    let mut list: Vec<Source> = sources
        .iter()
        .map(|(key, source)| Source {
            title: source.title.clone().or_else(|| titles.get(key).cloned()),
            ..source.clone()
        })
        .collect();
    list.sort_by_key(|source| (source.cited.unwrap_or(u32::MAX), !source.read));
    TurnResearch {
        searches,
        sources: list,
        skipped: if seen > 0 { skipped } else { 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(id: &str, name: &str, input: Value, output: &str) -> Row {
        let mut call = ToolCall::new(id, name);
        call.input = input.to_string();
        call.output = output.into();
        Row {
            role: "tool".into(),
            text: serde_json::to_string(&call).unwrap(),
            first: true,
            sent_at: None,
        }
    }

    fn assistant(text: &str) -> Row {
        Row {
            role: "assistant".into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    #[test]
    fn lists_searches_pages_read_and_cited_sources_cited_first() {
        let links = json!([
            {"title": "Mining report", "url": "https://a.example/report?utm=1"},
            {"title": "Other", "url": "https://b.example/page"},
            {"title": "Unused", "url": "https://c.example/"},
        ]);
        let rows = vec![
            tool(
                "1",
                "WebSearch",
                json!({"query": "copper output"}),
                &format!("Web search results\nLinks: {links}\n"),
            ),
            tool(
                "2",
                "WebFetch",
                json!({"url": "https://b.example/page#top"}),
                "text",
            ),
            assistant(
                "Copper rose [1](https://a.example/report \"Mining report | Output grew 4%\").",
            ),
        ];
        let research = turn_research(&rows);
        assert_eq!(
            research.searches,
            [Search {
                query: "copper output".into(),
                results: 3
            }]
        );
        let shown: Vec<_> = research
            .sources
            .iter()
            .map(|s| (s.host.as_str(), s.cited, s.read, s.title.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("a.example", Some(1), false, Some("Mining report")),
                ("b.example", None, true, Some("Other")),
            ]
        );
        assert_eq!(research.skipped, 1);
    }

    #[test]
    fn a_turn_without_the_web_has_no_research() {
        let rows = vec![
            tool("1", "Read", json!({"file_path": "/a/b.rs"}), "fn main() {}"),
            assistant("Done."),
        ];
        assert!(turn_research(&rows).is_empty());
    }

    #[test]
    fn reads_pages_the_app_opened_for_the_agent() {
        let rows = vec![tool(
            "1",
            "monocode_app__app",
            json!({"action": "web.read", "url": "https://journal.example/x"}),
            "ok",
        )];
        let research = turn_research(&rows);
        assert_eq!(research.sources.len(), 1);
        assert!(research.sources[0].read);
    }
}
