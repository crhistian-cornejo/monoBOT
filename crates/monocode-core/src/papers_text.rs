//! Bounded PDF text extraction and page-based excerpts for paper actions.
use lopdf::{Document, LoadOptions};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::OnceLock};

const MAX_PDF_BYTES: usize = 40 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGES: usize = 80;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PaperText {
    pub pages: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable_pages: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<usize>,
}

/// Extract at most 80 pages, bounding both eagerly decoded PDF objects and
/// per-page streams. A scanned or unsupported page stays empty, never guessed.
pub fn extract_pdf_text(bytes: &[u8]) -> Result<PaperText, String> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err("That PDF is too large (over 40 MB).".into());
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err("That file is not a PDF.".into());
    }
    let doc = Document::load_mem_with_options(
        bytes,
        LoadOptions::with_max_decompressed_size(MAX_STREAM_BYTES),
    )
    .map_err(|error| format!("Could not read this PDF: {error}"))?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        return Err("This PDF has no pages.".into());
    }
    let mut result = PaperText {
        total_pages: (pages.len() > MAX_PAGES).then_some(pages.len()),
        ..Default::default()
    };
    let mut total_bytes = 0;
    for page in pages.keys().take(MAX_PAGES) {
        let text = match doc.extract_text_with_limit(&[*page], MAX_STREAM_BYTES) {
            Ok(text) => text,
            Err(_) => String::new(),
        };
        if text.trim().is_empty() {
            result.unreadable_pages.push(*page as usize);
        }
        total_bytes += text.len();
        if total_bytes > MAX_TEXT_BYTES {
            return Err("The paper's text is too large to read safely.".into());
        }
        result.pages.push(text.trim().to_owned());
    }
    Ok(result)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Excerpt {
    pub page_count: usize,
    pub covers: String,
    pub text: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineEntry {
    pub title: String,
    pub level: usize,
    pub page: usize,
    line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Figure {
    pub label: String,
    pub page: usize,
    pub caption: String,
}

/// The DOI journals often print near the start of the paper.
pub fn detect_doi(paper: &PaperText) -> Option<String> {
    static DOI: OnceLock<Regex> = OnceLock::new();
    let pattern = DOI.get_or_init(|| {
        Regex::new(r#"(?i)\b10\.\d{4,9}/[^\s"<>).;\]]+(?:\.[^\s"<>);]+)*"#).unwrap()
    });
    paper
        .pages
        .iter()
        .take(3)
        .find_map(|page| pattern.find(page))
        .map(|matched| {
            matched
                .as_str()
                .trim_end_matches([',', '.', ';', ':'])
                .to_ascii_lowercase()
        })
}

fn lines(paper: &PaperText) -> Vec<(usize, &str)> {
    paper
        .pages
        .iter()
        .enumerate()
        .flat_map(|(index, page)| page.lines().map(move |line| (index + 1, line.trim())))
        .collect()
}

pub fn outline(paper: &PaperText) -> Vec<OutlineEntry> {
    static PLAIN: OnceLock<Regex> = OnceLock::new();
    static NUMBERED: OnceLock<Regex> = OnceLock::new();
    let plain = PLAIN.get_or_init(|| Regex::new(
        r"(?i)^(abstract|introduction|background|related work|literature review|methods?|methodology|materials and methods|experimental setup|experiments?|results|results and discussion|evaluation|discussion|conclusions?|conclusions and future work|acknowledge?ments|limitations|references|bibliography|appendix(?: [a-z0-9])?)\.?$",
    ).unwrap());
    let numbered = NUMBERED.get_or_init(|| {
        Regex::new(r"^(\d{1,2}(?:\.\d{1,2}){0,2})\.?\s+([A-Z][^.\n]{2,80})$").unwrap()
    });
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for (index, (page, line)) in lines(paper).into_iter().enumerate() {
        if !(3..=90).contains(&line.chars().count()) {
            continue;
        }
        let (title, level) = if let Some(captures) = numbered.captures(line) {
            (
                format!("{} {}", &captures[1], captures[2].trim()),
                captures[1].split('.').count(),
            )
        } else if plain.is_match(line) {
            (line.trim_end_matches('.').to_owned(), 1)
        } else {
            continue;
        };
        if seen.insert(title.to_lowercase()) {
            result.push(OutlineEntry {
                title,
                level,
                page,
                line: index,
            });
            if result.len() == 80 {
                break;
            }
        }
    }
    result
}

pub fn figures(paper: &PaperText) -> Vec<Figure> {
    static CAPTION: OnceLock<Regex> = OnceLock::new();
    let caption = CAPTION.get_or_init(|| {
        Regex::new(r"^(Figure|Fig\.|Table)\s+(\d{1,3})\s*[.:]\s*(\S.{8,240})$").unwrap()
    });
    let mut seen = HashSet::new();
    lines(paper)
        .into_iter()
        .filter_map(|(page, line)| {
            let found = caption.captures(line)?;
            let kind = if found[1].starts_with("Fig") {
                "Figure"
            } else {
                "Table"
            };
            let label = format!("{kind} {}", &found[2]);
            seen.insert(label.clone()).then(|| Figure {
                label,
                page,
                caption: found[3].trim().to_owned(),
            })
        })
        .collect()
}

/// A section ends at the next heading of the same or a higher level.
pub fn section(paper: &PaperText, name: &str, max_chars: usize) -> Option<Excerpt> {
    let wanted = name.trim().to_lowercase();
    if wanted.is_empty() {
        return None;
    }
    let headings = outline(paper);
    let synonyms: &[&str] = if wanted.contains("method") || wanted.contains("approach") {
        &[
            "method",
            "approach",
            "materials",
            "model",
            "experimental setup",
        ]
    } else if wanted.contains("result") || wanted.contains("finding") {
        &["result", "experiment", "evaluation", "finding"]
    } else if wanted.contains("conclu") || wanted.contains("summary") {
        &["conclu", "summary", "discussion"]
    } else if wanted.contains("intro") {
        &["introduction", "background"]
    } else if wanted.contains("related") || wanted.contains("prior") {
        &["related", "background", "literature"]
    } else {
        &[]
    };
    let start = headings
        .iter()
        .find(|entry| entry.title.to_lowercase().contains(&wanted))
        .or_else(|| {
            headings.iter().find(|entry| {
                let title = entry.title.to_lowercase();
                synonyms.iter().any(|term| title.contains(term))
            })
        })?;
    let all = lines(paper);
    let end = headings
        .iter()
        .find(|entry| entry.line > start.line && entry.level <= start.level)
        .map_or(all.len(), |entry| entry.line);
    let body = all[start.line..end]
        .iter()
        .map(|(_, text)| *text)
        .collect::<Vec<_>>()
        .join("\n");
    let body = body.trim();
    let truncated = body.chars().count() > max_chars;
    Some(Excerpt {
        page_count: paper.pages.len(),
        covers: format!(
            "section \"{}\", from page {}{}",
            start.title,
            start.page,
            if truncated {
                format!(", cut at {max_chars} characters")
            } else {
                String::new()
            }
        ),
        text: body.chars().take(max_chars).collect(),
        truncated,
    })
}

/// Parse one-based page ranges without creating ranges larger than the paper.
fn pages(spec: &str, total: usize) -> Result<Vec<usize>, String> {
    let mut selected = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let (first, last) = match part.split_once('-') {
            Some((first, last)) => (first.trim(), last.trim()),
            None => (part, part),
        };
        let first = first.parse::<usize>().map_err(|_| "Invalid page range.")?;
        let last = last.parse::<usize>().map_err(|_| "Invalid page range.")?;
        if first == 0 || first > last || first > total {
            return Err("Page range is outside the readable paper.".into());
        }
        for page in first..=last.min(total) {
            if !selected.contains(&page) {
                selected.push(page);
            }
        }
    }
    selected.sort_unstable();
    Ok(selected)
}

/// Return only requested pages, relevant paragraphs, or the paper's opening.
pub fn excerpt(
    paper: &PaperText,
    page_spec: Option<&str>,
    query: Option<&str>,
    max_chars: usize,
) -> Result<Excerpt, String> {
    let (covers, body) = if let Some(spec) = page_spec {
        let selected = pages(spec, paper.pages.len())?;
        (
            format!("pages {spec}"),
            selected
                .into_iter()
                .map(|page| format!("[page {page}]\n{}", paper.pages[page - 1].trim()))
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    } else if let Some(query) = query {
        let terms: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.chars().count() > 2)
            .map(str::to_lowercase)
            .collect();
        let mut found: Vec<(usize, usize, String)> = paper
            .pages
            .iter()
            .enumerate()
            .flat_map(|(page, text)| {
                let terms = &terms;
                text.split("\n\n").filter_map(move |paragraph| {
                    let cleaned = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
                    let lower = cleaned.to_lowercase();
                    let score = terms
                        .iter()
                        .filter(|term| lower.contains(term.as_str()))
                        .count();
                    (cleaned.chars().count() >= 60 && score > 0).then_some((
                        page + 1,
                        score,
                        cleaned,
                    ))
                })
            })
            .collect();
        found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        found.truncate(8);
        found.sort_by_key(|item| item.0);
        let count = found.len();
        (
            if count == 0 {
                format!("no passage matches \"{query}\"")
            } else {
                format!("{count} passages for \"{query}\"")
            },
            found
                .into_iter()
                .map(|(page, _, text)| format!("[page {page}] {text}"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    } else {
        (
            "the paper from the start".to_owned(),
            paper
                .pages
                .iter()
                .enumerate()
                .map(|(index, page)| format!("[page {}]\n{}", index + 1, page.trim()))
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    };
    let truncated = body.chars().count() > max_chars;
    Ok(Excerpt {
        page_count: paper.pages.len(),
        covers: if truncated {
            format!("{covers}, cut at {max_chars} characters")
        } else {
            covers
        },
        text: body.chars().take(max_chars).collect(),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges_are_bounded_unique_and_one_based() {
        assert_eq!(pages("3, 1-2, 2", 4).unwrap(), [1, 2, 3]);
        assert_eq!(pages("3-999999999", 4).unwrap(), [3, 4]);
        assert!(pages("0-2", 4).is_err());
        assert!(pages("4-1", 4).is_err());
        assert!(pages("5", 4).is_err());
        assert!(pages("2, nope", 4).is_err());
    }

    #[test]
    fn excerpts_preserve_pages_and_never_cut_utf8() {
        let paper = PaperText {
            pages: vec![
                "An introduction to the paper.".into(),
                "Resultados: café y análisis.".into(),
            ],
            ..Default::default()
        };
        let selected = excerpt(&paper, Some("2"), None, 500).unwrap();
        assert_eq!(selected.text, "[page 2]\nResultados: café y análisis.");
        let short = excerpt(&paper, None, None, 7).unwrap();
        assert!(short.truncated);
        assert_eq!(short.text.chars().count(), 7);
        assert!(excerpt(&paper, Some("0"), None, 500).is_err());
    }

    #[test]
    fn query_excerpts_return_only_matching_paragraphs() {
        let paper = PaperText {
            pages: vec![
                "The calibration method improves hydrology estimates by comparing rainfall observations with simulated discharge.".into(),
                "The second paragraph describes a completely different database application and its ordinary deployment.".into(),
            ],
            ..Default::default()
        };
        let found = excerpt(&paper, None, Some("hydrology rainfall"), 500).unwrap();
        assert!(found.text.contains("[page 1]"));
        assert!(!found.text.contains("[page 2]"));
        assert!(excerpt(&paper, None, Some("unmatched"), 500)
            .unwrap()
            .text
            .is_empty());
    }

    #[test]
    fn rejects_non_pdf_and_size_before_parsing() {
        assert!(extract_pdf_text(b"not a PDF").is_err());
        assert!(extract_pdf_text(&vec![0; MAX_PDF_BYTES + 1]).is_err());
    }

    #[test]
    fn outline_section_and_figures_keep_page_numbers() {
        let paper = PaperText {
            pages: vec![
                "Introduction\nOverview.\n2 Methods\nDetails.\n2.1 Model\nNested details.".into(),
                "3 Results\nFigure 1: Measurements across the complete sample.\nThe results are shown here.".into(),
            ],
            ..Default::default()
        };
        let headings = outline(&paper);
        assert_eq!(
            headings
                .iter()
                .map(|h| h.title.as_str())
                .collect::<Vec<_>>(),
            ["Introduction", "2 Methods", "2.1 Model", "3 Results"]
        );
        let methods = section(&paper, "methodology", 1000).unwrap();
        assert!(methods.text.contains("Nested details"));
        assert!(!methods.text.contains("3 Results"));
        assert_eq!(figures(&paper)[0].page, 2);
        assert_eq!(figures(&paper)[0].label, "Figure 1");
    }

    #[test]
    fn detects_printed_doi_without_terminal_punctuation() {
        let paper = PaperText {
            pages: vec!["Article DOI: 10.1234/EXAMPLE.42, full text follows.".into()],
            ..Default::default()
        };
        assert_eq!(detect_doi(&paper).as_deref(), Some("10.1234/example.42"));
    }
}
