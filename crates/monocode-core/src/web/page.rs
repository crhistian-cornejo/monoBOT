//! A web page as an agent should read it: its main text as light Markdown,
//! who published it and when, its picture and icon, the PDF it offers, and
//! the links worth following. Ports src/features/web/model/webPage.ts
//! (`readWebPage`); the HTML is parsed, never run.

use std::sync::OnceLock;

use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};
use serde::Serialize;
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebLink {
    pub text: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebPageRead {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub site_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// og:image, for a preview card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// The site's icon: the page's own, else /favicon.ico.
    pub icon: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
    /// A scholarly page's DOI and PDF (citation_* tags): read them as a paper.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_url: Option<String>,
    pub outline: Vec<String>,
    pub text: String,
    /// Characters of main text before it was cut to `max_chars`.
    pub chars: usize,
    pub truncated: bool,
    /// The paragraphs that best match `query`, when one was asked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passages: Option<Vec<String>>,
    pub links: Vec<WebLink>,
}

impl WebPageRead {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

const REMOVED: &str = "script,style,noscript,template,svg,canvas,iframe,object,nav,footer,aside,form,button,dialog,[role=navigation],[role=banner],[role=contentinfo],[role=complementary],[aria-hidden=true],[hidden]";

const BLOCKS: [&str; 19] = [
    "address",
    "article",
    "blockquote",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "figcaption",
    "figure",
    "header",
    "hr",
    "li",
    "main",
    "ol",
    "p",
    "section",
    "summary",
    "ul",
];

const MAX_LINKS: usize = 40;
const MAX_OUTLINE: usize = 30;
pub const DEFAULT_MAX_CHARS: usize = 20_000;

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("a fixed selector")
}

/// JavaScript's `\s`, which differs from Rust's `char::is_whitespace`.
fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// Runs of whitespace as one space (`replace(/\s+/g, " ")`).
fn collapse(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_space = false;
    for c in value.chars() {
        if is_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// Collapsed and trimmed, `None` when nothing is left.
fn clean(value: Option<&str>) -> Option<String> {
    let text = collapse(value?);
    let text = text.trim_matches(is_space);
    (!text.is_empty()).then(|| text.to_string())
}

fn text_of(element: ElementRef<'_>) -> String {
    element.text().collect()
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn safe_url(url: &str) -> Option<Url> {
    Url::parse(url).ok()
}

/// `href` as an absolute http(s) URL, or `None`.
fn resolve(href: &str, base: Option<&Url>) -> Option<String> {
    let href = href.trim();
    let url = match base {
        Some(base) => base.join(href).ok()?,
        None => Url::parse(href).ok()?,
    };
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// The href of the first `<link>` whose rel names `kind` ("shortcut icon" too).
fn link_href(doc: &Html, kind: &str) -> Option<String> {
    for link in doc.select(&selector("link[rel][href]")) {
        let rel = link.value().attr("rel").unwrap_or("").to_lowercase();
        if rel.split(is_space).any(|part| part == kind) {
            return link.value().attr("href").map(str::to_string);
        }
    }
    None
}

/// Every non-empty `<meta>` content among `keys`, by property, name or itemprop.
fn meta_all(doc: &Html, keys: &[&str]) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    for element in doc.select(&selector("meta")) {
        let value = element.value();
        let key = value
            .attr("property")
            .or_else(|| value.attr("name"))
            .or_else(|| value.attr("itemprop"))
            .unwrap_or("")
            .to_lowercase();
        if !keys.iter().any(|wanted| wanted.to_lowercase() == key) {
            continue;
        }
        if let Some(content) = clean(value.attr("content")) {
            if !values.contains(&content) {
                values.push(content);
            }
        }
    }
    values
}

/// The first non-empty `<meta>` among `keys`, in the order of `keys`.
fn meta(doc: &Html, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| meta_all(doc, &[key]).into_iter().next())
}

#[derive(Default)]
struct LinkedData {
    headline: Option<String>,
    author: Option<String>,
    publisher: Option<String>,
    published: Option<String>,
    modified: Option<String>,
}

/// schema.org JSON-LD: an article's headline, authors, publisher and dates.
fn linked_data(doc: &Html) -> LinkedData {
    let mut found = LinkedData::default();
    for script in doc.select(&selector(r#"script[type="application/ld+json"]"#)) {
        let Ok(data) = serde_json::from_str::<Value>(&text_of(script)) else {
            continue;
        };
        let mut items = Vec::new();
        flatten(&data, &mut items);
        for item in items {
            if found.headline.is_none() {
                found.headline =
                    json_text(item.get("headline")).or_else(|| json_text(item.get("name")));
            }
            if found.author.is_none() {
                found.author = json_names(item.get("author"));
            }
            if found.publisher.is_none() {
                found.publisher = json_names(item.get("publisher"));
            }
            if found.published.is_none() {
                found.published = json_text(item.get("datePublished"));
            }
            if found.modified.is_none() {
                found.modified = json_text(item.get("dateModified"));
            }
        }
    }
    found
}

fn flatten<'a>(data: &'a Value, out: &mut Vec<&'a serde_json::Map<String, Value>>) {
    match data {
        Value::Array(list) => list.iter().for_each(|entry| flatten(entry, out)),
        Value::Object(item) => {
            out.push(item);
            if let Some(graph) = item.get("@graph") {
                flatten(graph, out);
            }
        }
        _ => {}
    }
}

fn json_text(value: Option<&Value>) -> Option<String> {
    clean(value?.as_str())
}

fn json_names(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let entries: Vec<&Value> = match value {
        Value::Array(list) => list.iter().collect(),
        other => vec![other],
    };
    let list: Vec<String> = entries
        .into_iter()
        .filter_map(|entry| match entry {
            Value::String(name) => clean(Some(name)),
            Value::Object(object) => json_text(object.get("name")),
            _ => None,
        })
        .collect();
    (!list.is_empty()).then(|| list.into_iter().take(6).collect::<Vec<_>>().join(", "))
}

/// The article, else main, else the body: wherever the page keeps its text.
fn main_content(doc: &Html) -> ElementRef<'_> {
    // The longest article; the first one wins a tie.
    let mut longest: Option<(usize, ElementRef<'_>)> = None;
    for article in doc.select(&selector("article")) {
        let length = utf16_len(&text_of(article));
        if longest.is_none_or(|(best, _)| length > best) {
            longest = Some((length, article));
        }
    }
    if let Some((_, article)) = longest {
        if utf16_len(text_of(article).trim_matches(is_space)) > 200 {
            return article;
        }
    }
    doc.select(&selector("main, [role=main]"))
        .next()
        .or_else(|| doc.select(&selector("body")).next())
        .unwrap_or_else(|| doc.root_element())
}

fn page_links(root: ElementRef<'_>, base: Option<&Url>) -> Vec<WebLink> {
    let mut seen: Vec<String> = Vec::new();
    let mut links = Vec::new();
    let own = base.map(|base| base.as_str().split('#').next().unwrap_or("").to_string());
    for anchor in root.select(&selector("a[href]")) {
        let Some(url) = resolve(anchor.value().attr("href").unwrap_or(""), base) else {
            continue;
        };
        let plain = url.split('#').next().unwrap_or("").to_string();
        if own.as_deref() == Some(plain.as_str()) || seen.contains(&plain) {
            continue;
        }
        let Some(label) =
            clean(Some(&text_of(anchor))).or_else(|| clean(anchor.value().attr("title")))
        else {
            continue;
        };
        seen.push(plain);
        links.push(WebLink {
            text: label.chars().take(80).collect(),
            url,
        });
        if links.len() >= MAX_LINKS {
            break;
        }
    }
    links
}

fn flush_line(line: &mut String, blocks: &mut Vec<String>) {
    static SPACES: OnceLock<Regex> = OnceLock::new();
    static AROUND_BREAKS: OnceLock<Regex> = OnceLock::new();
    let spaces = SPACES.get_or_init(|| Regex::new(r"[ \t]+").expect("a fixed pattern"));
    let around = AROUND_BREAKS.get_or_init(|| Regex::new(r" *\n *").expect("a fixed pattern"));
    let once = spaces.replace_all(line, " ");
    let twice = around.replace_all(&once, "\n");
    let value = twice.trim_matches(is_space);
    if !value.is_empty() {
        blocks.push(value.to_string());
    }
    line.clear();
}

fn walk(node: ElementRef<'_>, line: &mut String, blocks: &mut Vec<String>) {
    for child in node.children() {
        match child.value() {
            Node::Text(text) => line.push_str(&collapse(text)),
            Node::Element(element) => {
                let Some(element_ref) = ElementRef::wrap(child) else {
                    continue;
                };
                let tag = element.name();
                if let Some(level) = heading_level(tag) {
                    flush_line(line, blocks);
                    if let Some(value) = clean(Some(&text_of(element_ref))) {
                        blocks.push(format!("{} {value}", "#".repeat(level)));
                    }
                } else if tag == "li" {
                    flush_line(line, blocks);
                    if let Some(value) = clean(Some(&text_of(element_ref))) {
                        blocks.push(format!("- {value}"));
                    }
                } else if tag == "pre" {
                    flush_line(line, blocks);
                    let text = text_of(element_ref);
                    let value = text.trim_end_matches(is_space);
                    if !value.trim_matches(is_space).is_empty() {
                        blocks.push(format!("```\n{value}\n```"));
                    }
                } else if tag == "blockquote" {
                    flush_line(line, blocks);
                    if let Some(value) = clean(Some(&text_of(element_ref))) {
                        blocks.push(format!("> {value}"));
                    }
                } else if tag == "table" {
                    flush_line(line, blocks);
                    let rows: Vec<String> = element_ref
                        .select(&selector("tr"))
                        .map(|row| {
                            row.select(&selector("th, td"))
                                .map(|cell| clean(Some(&text_of(cell))).unwrap_or_default())
                                .collect::<Vec<_>>()
                                .join(" | ")
                        })
                        .filter(|row| row.chars().any(|c| c != ' ' && c != '|'))
                        .collect();
                    if !rows.is_empty() {
                        blocks.push(rows.join("\n"));
                    }
                } else if tag == "br" {
                    line.push('\n');
                } else if tag == "img" {
                    if let Some(alt) = clean(element.attr("alt")) {
                        line.push_str(&format!(" [image: {alt}] "));
                    }
                } else if BLOCKS.contains(&tag) {
                    flush_line(line, blocks);
                    walk(element_ref, line, blocks);
                    flush_line(line, blocks);
                } else {
                    walk(element_ref, line, blocks);
                }
            }
            _ => {}
        }
    }
}

fn heading_level(tag: &str) -> Option<usize> {
    let digit = tag.strip_prefix('h')?;
    match digit.as_bytes() {
        [b @ b'1'..=b'6'] => Some((b - b'0') as usize),
        _ => None,
    }
}

/// Headings, paragraphs, lists, quotes, code and tables as light Markdown.
fn to_markdown(root: ElementRef<'_>) -> String {
    let mut blocks = Vec::new();
    let mut line = String::new();
    walk(root, &mut line, &mut blocks);
    flush_line(&mut line, &mut blocks);
    blocks.join("\n\n")
}

/// Cut at a paragraph so a sentence isn't left half-read.
fn cut(text: &str, max: usize) -> String {
    let Some((byte, _)) = text.char_indices().nth(max) else {
        return text.to_string();
    };
    let head = &text[..byte];
    match head.rfind("\n\n") {
        Some(end) if (head[..end].chars().count() as f64) > max as f64 * 0.6 => {
            head[..end].to_string()
        }
        _ => head.to_string(),
    }
}

/// Paragraphs holding most of the query's words, the best first.
fn best_paragraphs(text: &str, query: &str) -> Vec<String> {
    let terms: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| term.chars().count() > 2)
        .map(str::to_string)
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    static BREAKS: OnceLock<Regex> = OnceLock::new();
    let breaks = BREAKS.get_or_init(|| Regex::new(r"\n\n+").expect("a fixed pattern"));
    let mut scored: Vec<(usize, usize, &str)> = breaks
        .split(text)
        .enumerate()
        .map(|(index, paragraph)| {
            let lower = paragraph.to_lowercase();
            let score = terms
                .iter()
                .filter(|term| lower.contains(term.as_str()))
                .count();
            (score, index, paragraph)
        })
        .filter(|(score, _, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(6)
        .map(|(_, _, paragraph)| paragraph.chars().take(1500).collect())
        .collect()
}

/// Reads `html` fetched from `url`. `max_chars` defaults to 20 000; a
/// non-empty `query` also picks the paragraphs that best answer it.
pub fn read_web_page(
    html: &str,
    url: &str,
    max_chars: Option<usize>,
    query: Option<&str>,
) -> WebPageRead {
    let mut doc = Html::parse_document(html);
    let base = safe_url(url);
    let linked = linked_data(&doc);
    let absolute = |href: Option<String>| href.and_then(|href| resolve(&href, base.as_ref()));

    let title = meta(
        &doc,
        &["og:title", "twitter:title", "citation_title", "dc.title"],
    )
    .or(linked.headline.clone())
    .or_else(|| {
        clean(
            doc.select(&selector("title"))
                .next()
                .map(text_of)
                .as_deref(),
        )
    })
    .or_else(|| clean(doc.select(&selector("h1")).next().map(text_of).as_deref()));
    let description = meta(
        &doc,
        &[
            "og:description",
            "twitter:description",
            "description",
            "dc.description",
        ],
    );
    let site_name = meta(
        &doc,
        &["og:site_name", "application-name", "citation_journal_title"],
    )
    .or(linked.publisher.clone())
    .or_else(|| {
        base.as_ref()
            .and_then(|base| base.host_str())
            .map(|host| host.strip_prefix("www.").unwrap_or(host).to_string())
    })
    .unwrap_or_default();
    let authors = meta_all(&doc, &["citation_author", "author", "article:author"]);
    let author = if authors.is_empty() {
        linked.author.clone()
    } else {
        Some(authors.into_iter().take(6).collect::<Vec<_>>().join(", "))
    };
    let published = meta(
        &doc,
        &[
            "article:published_time",
            "citation_publication_date",
            "citation_date",
            "dc.date",
            "date",
        ],
    )
    .or(linked.published.clone())
    .or_else(|| {
        doc.select(&selector("time[datetime]"))
            .next()
            .and_then(|time| time.value().attr("datetime"))
            .map(str::to_string)
    })
    .filter(|value| !value.is_empty());
    let modified = meta(&doc, &["article:modified_time"]).or(linked.modified.clone());
    static DOI: OnceLock<Regex> = OnceLock::new();
    let doi_pattern = DOI.get_or_init(|| Regex::new(r"10\.\d{4,9}/\S+").expect("a fixed pattern"));
    let doi = meta(&doc, &["citation_doi", "dc.identifier", "prism.doi"]).and_then(|value| {
        doi_pattern
            .find(&value)
            .map(|found| found.as_str().to_string())
    });
    let pdf_url = absolute(meta(&doc, &["citation_pdf_url"]));
    let image = absolute(meta(&doc, &["og:image", "twitter:image", "og:image:url"]));
    let icon = absolute(link_href(&doc, "icon").or_else(|| link_href(&doc, "apple-touch-icon")))
        .or_else(|| {
            base.as_ref()
                .map(|base| format!("{}/favicon.ico", base.origin().ascii_serialization()))
        })
        .unwrap_or_default();
    let canonical = absolute(link_href(&doc, "canonical"));
    let language = clean(doc.root_element().value().attr("lang"));

    let removed: Vec<_> = doc
        .select(&selector(REMOVED))
        .map(|element| element.id())
        .collect();
    for id in removed {
        if let Some(mut node) = doc.tree.get_mut(id) {
            node.detach();
        }
    }
    let root = main_content(&doc);
    let outline: Vec<String> = root
        .select(&selector("h1, h2, h3"))
        .filter_map(|heading| clean(Some(&text_of(heading))))
        .take(MAX_OUTLINE)
        .collect();
    let links = page_links(root, base.as_ref());
    let full = to_markdown(root);
    let max_chars = max_chars.unwrap_or(DEFAULT_MAX_CHARS);
    let text = cut(&full, max_chars);
    let passages = query
        .filter(|query| !query.trim_matches(is_space).is_empty())
        .map(|query| best_paragraphs(&full, query));

    let page_url = base
        .as_ref()
        .map(Url::to_string)
        .unwrap_or_else(|| url.to_string());
    let canonical = canonical.filter(|canonical| *canonical != page_url);
    WebPageRead {
        url: page_url,
        title,
        description: description.map(|value| value.chars().take(400).collect()),
        site_name,
        author,
        published,
        modified,
        language,
        image,
        icon,
        canonical,
        doi,
        pdf_url,
        outline,
        chars: full.chars().count(),
        truncated: text.chars().count() < full.chars().count(),
        text,
        passages,
        links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r#"<!doctype html>
<html lang="es">
<head>
  <title>Pozas de sedimentación | Minería Hoy</title>
  <meta property="og:title" content="Cómo diseñar pozas de sedimentación">
  <meta property="og:description" content="Guía práctica para manejar aguas superficiales en minería.">
  <meta property="og:site_name" content="Minería Hoy">
  <meta property="og:image" content="/img/poza.jpg">
  <meta name="author" content="Ana Pérez">
  <meta property="article:published_time" content="2024-05-02T10:00:00Z">
  <link rel="shortcut icon" href="/static/icon.png">
  <link rel="canonical" href="https://mineria.example/pozas">
  <script type="application/ld+json">{"@graph":[{"@type":"NewsArticle","headline":"Pozas","dateModified":"2024-06-01","publisher":{"name":"Minería Hoy SAC"}}]}</script>
  <script>window.track = "should never appear";</script>
  <style>body { color: red }</style>
</head>
<body>
  <nav><a href="/">Inicio</a> <a href="/contacto">Contacto</a></nav>
  <article>
    <h1>Cómo diseñar pozas de sedimentación</h1>
    <p>Una poza de sedimentación retiene los sólidos del agua de escorrentía antes de descargarla.
       El <a href="https://example.org/guia.pdf">manual de diseño</a> explica el tiempo de retención.</p>
    <h2>Dimensionamiento</h2>
    <ul><li>Caudal de diseño</li><li>Tiempo de retención de 24 horas</li></ul>
    <table><tr><th>Partícula</th><th>Velocidad</th></tr><tr><td>Arena fina</td><td>0.8 cm/s</td></tr></table>
    <p>El área superficial depende de la velocidad de sedimentación de la partícula más fina.</p>
  </article>
  <footer>© 2024 Minería Hoy</footer>
</body>
</html>"#;

    #[test]
    fn reads_who_published_the_page_when_and_its_picture_and_icon() {
        let page = read_web_page(ARTICLE, "https://mineria.example/pozas?utm=1", None, None);
        assert_eq!(
            page.title.as_deref(),
            Some("Cómo diseñar pozas de sedimentación")
        );
        assert_eq!(
            page.description.as_deref(),
            Some("Guía práctica para manejar aguas superficiales en minería.")
        );
        assert_eq!(page.site_name, "Minería Hoy");
        assert_eq!(page.author.as_deref(), Some("Ana Pérez"));
        assert_eq!(page.published.as_deref(), Some("2024-05-02T10:00:00Z"));
        assert_eq!(page.modified.as_deref(), Some("2024-06-01"));
        assert_eq!(page.language.as_deref(), Some("es"));
        assert_eq!(
            page.image.as_deref(),
            Some("https://mineria.example/img/poza.jpg")
        );
        assert_eq!(page.icon, "https://mineria.example/static/icon.png");
        assert_eq!(
            page.canonical.as_deref(),
            Some("https://mineria.example/pozas")
        );
    }

    #[test]
    fn keeps_the_articles_text_as_markdown_without_scripts_or_navigation() {
        let page = read_web_page(ARTICLE, "https://mineria.example/pozas", None, None);
        assert!(page.text.contains("# Cómo diseñar pozas de sedimentación"));
        assert!(page.text.contains("## Dimensionamiento"));
        assert!(page.text.contains("- Tiempo de retención de 24 horas"));
        assert!(page
            .text
            .contains("Partícula | Velocidad\nArena fina | 0.8 cm/s"));
        for gone in ["should never appear", "color: red", "Contacto", "© 2024"] {
            assert!(!page.text.contains(gone), "{gone}");
        }
        assert_eq!(
            page.outline,
            ["Cómo diseñar pozas de sedimentación", "Dimensionamiento"]
        );
        assert_eq!(
            page.links,
            [WebLink {
                text: "manual de diseño".into(),
                url: "https://example.org/guia.pdf".into()
            }]
        );
    }

    #[test]
    fn cuts_long_text_at_a_paragraph_and_finds_the_passages_asked_for() {
        let page = read_web_page(
            ARTICLE,
            "https://mineria.example/pozas",
            Some(200),
            Some("velocidad de sedimentación"),
        );
        assert!(page.truncated);
        assert!(page.text.chars().count() <= 200);
        assert!(page.chars > 200);
        let first = &page.passages.as_ref().unwrap()[0];
        assert!(first.contains("velocidad de sedimentación"), "{first}");
    }

    #[test]
    fn finds_a_scholarly_pages_doi_and_pdf() {
        let page = read_web_page(
            r#"<html><head>
        <meta name="citation_title" content="Settling pond design">
        <meta name="citation_author" content="Lee, J."><meta name="citation_author" content="Kim, S.">
        <meta name="citation_doi" content="doi:10.1016/j.mineng.2020.106123">
        <meta name="citation_pdf_url" content="/content/pond.pdf">
        <meta name="citation_journal_title" content="Minerals Engineering">
      </head><body><p>Abstract.</p></body></html>"#,
            "https://journal.example/article/1",
            None,
            None,
        );
        assert_eq!(page.title.as_deref(), Some("Settling pond design"));
        assert_eq!(page.author.as_deref(), Some("Lee, J., Kim, S."));
        assert_eq!(page.site_name, "Minerals Engineering");
        assert_eq!(page.doi.as_deref(), Some("10.1016/j.mineng.2020.106123"));
        assert_eq!(
            page.pdf_url.as_deref(),
            Some("https://journal.example/content/pond.pdf")
        );
        assert_eq!(page.icon, "https://journal.example/favicon.ico");
    }

    #[test]
    fn falls_back_to_the_title_the_host_and_the_body() {
        let page = read_web_page(
            "<html><head><title>  Plain\n page </title></head><body><p>Hello <b>world</b></p><script>x()</script></body></html>",
            "https://www.example.org/a#top",
            None,
            None,
        );
        assert_eq!(page.title.as_deref(), Some("Plain page"));
        assert_eq!(page.site_name, "example.org");
        assert_eq!(page.text, "Hello world");
        assert_eq!(page.url, "https://www.example.org/a#top");
        assert!(page.author.is_none() && page.passages.is_none() && !page.truncated);
    }

    #[test]
    fn serialises_like_the_tauri_app_with_absent_fields_left_out() {
        let page = read_web_page(
            "<html><body><p>Hi</p></body></html>",
            "https://example.org/",
            None,
            None,
        );
        let json = page.to_json();
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        for key in [
            "url",
            "siteName",
            "icon",
            "outline",
            "text",
            "chars",
            "truncated",
            "links",
        ] {
            assert!(keys.contains(&key), "{key}");
        }
        for key in ["title", "pdfUrl", "passages", "canonical", "doi"] {
            assert!(!keys.contains(&key), "{key}");
        }
    }

    #[test]
    fn an_asked_query_with_no_usable_words_gives_empty_passages() {
        let page = read_web_page(ARTICLE, "https://mineria.example/pozas", None, Some("a de"));
        assert_eq!(page.passages, Some(Vec::new()));
        let none = read_web_page(ARTICLE, "https://mineria.example/pozas", None, Some("  "));
        assert!(none.passages.is_none());
    }

    #[test]
    fn prefers_the_longest_article_and_skips_hidden_and_form_content() {
        let long = "Texto largo. ".repeat(30);
        let html = format!(
            r#"<body><article><p>short</p></article>
            <article><p>{long}</p><div hidden>secret</div><p aria-hidden="true">decor</p><form>login</form></article></body>"#
        );
        let page = read_web_page(&html, "https://example.org/", None, None);
        assert!(page.text.starts_with("Texto largo."));
        for gone in ["short", "secret", "decor", "login"] {
            assert!(!page.text.contains(gone), "{gone}");
        }
    }

    #[test]
    fn renders_code_quotes_images_and_line_breaks() {
        let page = read_web_page(
            "<body><main><pre>fn a() {}\n\n</pre><blockquote> A  quote </blockquote><p>one<br>two <img alt=\"fig 1\" src=x></p></main></body>",
            "https://example.org/",
            None,
            None,
        );
        assert_eq!(
            page.text,
            "```\nfn a() {}\n```\n\n> A quote\n\none\ntwo [image: fig 1]"
        );
    }

    #[test]
    fn links_skip_the_page_itself_repeats_and_unlabelled_anchors() {
        let page = read_web_page(
            r##"<body><a href="#top">Top</a><a href="/self">Self</a><a href="/x#a">X</a><a href="/x#b">X again</a>
            <a href="/img"><img src=a></a><a href="/t" title="Titled"></a><a href="javascript:void(0)">js</a></body>"##,
            "https://example.org/self",
            None,
            None,
        );
        assert_eq!(
            page.links,
            [
                WebLink {
                    text: "X".into(),
                    url: "https://example.org/x#a".into()
                },
                WebLink {
                    text: "Titled".into(),
                    url: "https://example.org/t".into()
                },
            ]
        );
    }
}
