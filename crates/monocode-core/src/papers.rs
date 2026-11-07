//! Shared paper passage matching, independent of the PDF renderer.
//!
//! An agent's quotation can differ from the PDF text at line breaks, ligatures
//! and punctuation. Offsets returned here are UTF-8 byte offsets into each
//! original item, suitable for slicing the original Rust strings.
use unicode_normalization::UnicodeNormalization;

#[path = "papers_text.rs"]
mod text;
pub use text::{
    Excerpt, Figure, OutlineEntry, PageSpan, PaperText, detect_doi, excerpt, extract_page_spans,
    extract_pdf_text, figures, merge_span_rects, outline, section,
};
#[path = "papers_store.rs"]
mod store;
pub use store::{Paper, PaperMeta, PaperStore, cache_dir, key_of, reference_key};
#[path = "papers_catalog.rs"]
mod catalog;
pub use catalog::{find, find_input, resolve};

const MIN_CHARS: usize = 18;
const MAX_PASSAGE_CHARS: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct Located {
    pub start_item: usize,
    pub start_offset: usize,
    pub end_item: usize,
    pub end_offset: usize,
    pub coverage: f32,
}

#[derive(Clone, Copy)]
struct Position {
    item: usize,
    start: usize,
    end: usize,
}

#[derive(Default)]
struct Folded {
    text: String,
    at: Vec<Position>,
}

fn fold(items: &[impl AsRef<str>]) -> Folded {
    let mut result = Folded::default();
    for (item, text) in items.iter().enumerate() {
        for (start, ch) in text.as_ref().char_indices() {
            // Normalize each original scalar separately, preserving where an
            // expanded ligature came from in the source.
            for normalized in ch.to_string().nfkc().flat_map(char::to_lowercase) {
                if normalized.is_alphanumeric() {
                    result.text.push(normalized);
                    result.at.push(Position {
                        item,
                        start,
                        end: start + ch.len_utf8(),
                    });
                }
            }
        }
    }
    result
}

fn range(folded: &Folded, byte: usize, length: usize, coverage: f32) -> Located {
    let first = folded.text[..byte].chars().count();
    let start = folded.at[first];
    let end = folded.at[first + length - 1];
    Located {
        start_item: start.item,
        start_offset: start.start,
        end_item: end.item,
        end_offset: end.end,
        coverage,
    }
}

/// Find a quotation in text items in reading order. A whole match wins,
/// followed by matching ends or a sufficiently long prefix or suffix.
pub fn locate_in_items(items: &[impl AsRef<str>], passage: &str) -> Option<Located> {
    let wanted = fold(&[passage]).text;
    let chars: Vec<char> = wanted.chars().collect();
    let size = chars.len();
    if !(MIN_CHARS..=MAX_PASSAGE_CHARS).contains(&size) {
        return None;
    }
    let folded = fold(items);
    if folded.at.len() < MIN_CHARS {
        return None;
    }
    if let Some(at) = folded.text.find(&wanted) {
        return Some(range(&folded, at, size, 1.0));
    }

    let edge = 24.min(size / 2);
    let head: String = chars[..edge].iter().collect();
    let tail: String = chars[size - edge..].iter().collect();
    if let Some(start) = folded.text.find(&head) {
        let after_head = start + head.len();
        if let Some(tail_at) = folded.text[after_head..].find(&tail) {
            let end = after_head + tail_at + tail.len();
            let length = folded.text[start..end].chars().count();
            if length * 2 <= size * 3 + 80 {
                return Some(range(&folded, start, length, 0.9));
            }
        }
    }

    let floor = MIN_CHARS.max((size * 4).div_ceil(10));
    let mut best: Option<(usize, usize)> = None;
    let mut length = size;
    while length >= floor {
        for part in [&chars[..length], &chars[size - length..]] {
            let candidate: String = part.iter().collect();
            if let Some(at) = folded.text.find(&candidate) {
                best = Some((at, length));
                break;
            }
        }
        if best.is_some() || length < floor + 4 {
            break;
        }
        length -= 4;
    }
    let (at, length) = best?;
    Some(range(&folded, at, length, length as f32 / size as f32))
}

/// Pick the best page, preferring a valid one-based hint for equal matches.
pub fn locate_page(
    pages: &[impl AsRef<str>],
    passage: &str,
    hint: Option<usize>,
) -> Option<(usize, f32)> {
    let mut order = Vec::with_capacity(pages.len());
    if let Some(hint) = hint.filter(|page| *page > 0 && *page <= pages.len()) {
        order.push(hint - 1);
    }
    order.extend((0..pages.len()).filter(|index| Some(index + 1) != hint));
    let mut best: Option<(usize, f32)> = None;
    for index in order {
        if let Some(found) = locate_in_items(&[pages[index].as_ref()], passage) {
            if found.coverage == 1.0 {
                return Some((index + 1, 1.0));
            }
            if best.is_none_or(|(_, coverage)| found.coverage > coverage) {
                best = Some((index + 1, found.coverage));
            }
        }
    }
    best
}

/// Resolve a scholarly citation without making a network request. Other links
/// and papers without a cached PDF stay browser links.
pub fn cached_citation(
    store: &PaperStore,
    href: &str,
    passage: Option<&str>,
) -> Option<(std::path::PathBuf, usize)> {
    let url = url::Url::parse(href).ok()?;
    if !matches!(url.scheme(), "https" | "http") {
        return None;
    }
    let key = if matches!(url.host_str()?, "doi.org" | "dx.doi.org" | "arxiv.org") {
        reference_key(href)
    } else {
        store.source_paper(href).ok().flatten()?
    };
    store.meta(&key).ok().flatten()?;
    let path = store.pdf_path(&key);
    if !path.is_file() {
        return None;
    }
    let page = passage
        .and_then(|passage| store.text(&key).ok().flatten().map(|text| (passage, text)))
        .and_then(|(passage, text)| locate_page(&text.pages, passage, None))
        .map_or(1, |(page, _)| page);
    Some((path, page))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSAGE: &str = "The package allows users to calibrate and run hourly to annual models on catchment sets and to analyse their outputs.";

    #[test]
    fn finds_quotation_across_items_and_preserves_unicode_offsets() {
        let items = ["Intro. ", "The ef-", "ﬁciency of the “GR4J” model is high."];
        let found =
            locate_in_items(&items, "the efficiency of the \"gr4j\" model is HIGH").unwrap();
        assert_eq!(found.coverage, 1.0);
        assert_eq!(found.start_item, 1);
        assert_eq!(found.end_item, 2);
        assert_eq!(&items[found.start_item][found.start_offset..], "The ef-");
        assert_eq!(
            &items[found.end_item][..found.end_offset],
            "ﬁciency of the “GR4J” model is high"
        );
    }

    #[test]
    fn finds_reworded_middle_or_a_partial_prefix() {
        let middle = [
            "The package allows users to calibrate and run models of many kinds on catchment sets and to analyse their outputs.",
        ];
        assert_eq!(locate_in_items(&middle, PASSAGE).unwrap().coverage, 0.9);
        let shorter = [
            "The package allows users to calibrate and run hourly to annual models on catchment sets.",
        ];
        let found = locate_in_items(&shorter, &format!("{PASSAGE} It is very good.")).unwrap();
        assert!(found.coverage > 0.6 && found.coverage < 1.0);
    }

    #[test]
    fn declines_short_unrelated_and_unbounded_passages() {
        let items = ["Calibration of the GR4J model uses a Michel algorithm."];
        assert!(locate_in_items(&items, "the model").is_none());
        assert!(
            locate_in_items(
                &items,
                "Deep learning dominates benchmark leaderboards today."
            )
            .is_none()
        );
        assert!(locate_in_items(&items, &"x".repeat(MAX_PASSAGE_CHARS + 1)).is_none());
    }

    #[test]
    fn page_hint_wins_only_when_it_is_a_better_match() {
        let pages = [PASSAGE, "Other text.", PASSAGE];
        assert_eq!(locate_page(&pages, PASSAGE, None), Some((1, 1.0)));
        assert_eq!(locate_page(&pages, PASSAGE, Some(3)), Some((3, 1.0)));
        assert_eq!(locate_page(&pages, PASSAGE, Some(2)), Some((1, 1.0)));
        assert_eq!(
            locate_page(
                &pages,
                "Unrelated long quotation that cannot be found.",
                None
            ),
            None
        );
    }

    #[test]
    fn cached_citations_find_the_passage_and_other_links_fall_back() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let store = PaperStore::new(dir.path().to_path_buf());
        let key = "10.1234/example";
        let meta: PaperMeta = serde_json::from_value(json!({
            "paper": {"key":key,"title":"Example","authors":[],"pdfUrls":[],"access":"open"},
            "availability":"full-text"
        }))
        .unwrap();
        store.put_meta(&meta).unwrap();
        std::fs::write(store.pdf_path(key), b"%PDF-1.7 fixture").unwrap();
        std::fs::write(
            dir.path().join(format!("{}.json", key_of(key))),
            serde_json::to_vec(&json!({"pages":["Opening.", PASSAGE]})).unwrap(),
        )
        .unwrap();
        let target =
            cached_citation(&store, "https://doi.org/10.1234/EXAMPLE", Some(PASSAGE)).unwrap();
        assert_eq!(target.0, store.pdf_path(key));
        assert_eq!(target.1, 2);
        assert!(
            cached_citation(&store, "https://example.com/10.1234/example", Some(PASSAGE)).is_none()
        );
        assert!(
            cached_citation(&store, "https://doi.org/10.1234/unknown", Some(PASSAGE)).is_none()
        );
    }
}
