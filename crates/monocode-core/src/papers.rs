//! Shared paper passage matching, independent of the PDF renderer.
//!
//! An agent's quotation can differ from the PDF text at line breaks, ligatures
//! and punctuation. Offsets returned here are UTF-8 byte offsets into each
//! original item, suitable for slicing the original Rust strings.
use unicode_normalization::UnicodeNormalization;

#[path = "papers_text.rs"]
mod text;
pub use text::{
    detect_doi, excerpt, extract_page_spans, extract_pdf_text, figures, merge_span_rects, outline,
    section, Excerpt, Figure, OutlineEntry, PageSpan, PaperText,
};
#[path = "papers_store.rs"]
mod store;
pub use store::{cache_dir, key_of, reference_key, CitedPassage, Paper, PaperMeta, PaperStore};
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

/// The cache key a citation's link names: its DOI, the arXiv DOI, or the PDF
/// without a DOI that the app downloaded from that link (paperKeyOf).
fn citation_key(store: &PaperStore, href: &str) -> Option<String> {
    let url = url::Url::parse(href).ok()?;
    if !matches!(url.scheme(), "https" | "http") {
        return None;
    }
    if matches!(url.host_str()?, "doi.org" | "dx.doi.org" | "arxiv.org") {
        Some(reference_key(href))
    } else {
        store.source_paper(href).ok().flatten()
    }
}

/// Resolve a scholarly citation to its cached PDF, the page the passage is on
/// and the paper's title, without making a network request. Other links and
/// papers without a cached PDF stay browser links.
pub fn cached_citation(
    store: &PaperStore,
    href: &str,
    passage: Option<&str>,
) -> Option<(std::path::PathBuf, usize, String)> {
    let key = citation_key(store, href)?;
    let title = store.meta(&key).ok().flatten()?.paper.title;
    let path = store.pdf_path(&key);
    if !path.is_file() {
        return None;
    }
    let page = passage
        .and_then(|passage| store.text(&key).ok().flatten().map(|text| (passage, text)))
        .and_then(|(passage, text)| locate_page(&text.pages, passage, None))
        .map_or(1, |(page, _)| page);
    Some((path, page, title))
}

/// A cited paper the app knows only by its record (authors, abstract): no
/// PDF was cached for it.
pub fn cached_record(store: &PaperStore, href: &str) -> Option<PaperMeta> {
    let key = citation_key(store, href)?;
    if store.pdf_path(&key).is_file() {
        return None;
    }
    store.meta(&key).ok().flatten()
}

/// Text selected in a PDF, as one line of prose: the layout's line breaks
/// go, and so do the hyphens that split a word across them ("increas-\ning").
pub fn clean_selection(text: &str) -> String {
    static BROKEN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let joined = BROKEN
        .get_or_init(|| regex::Regex::new(r"(\p{L})-\n\s*(\p{Ll})").unwrap())
        .replace_all(&text, "$1$2");
    joined.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How a cited passage is named in words: "citation 1, page 3".
pub fn cited_label(number: Option<u32>, page: Option<usize>) -> String {
    [
        number.map(|n| format!("citation {n}")),
        page.map(|p| format!("page {p}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ")
}

/// Where a saved copy sits, in words: "Documents / MonoBOT / Research", from
/// a path on either macOS or Windows, and the file's own name.
pub fn saved_location(path: &str) -> (String, String) {
    let mut parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let file = parts.pop().unwrap_or(path).to_owned();
    let shown = match parts.iter().rposition(|part| *part == "Documents") {
        Some(from) => &parts[from..],
        None => &parts[parts.len().saturating_sub(2)..],
    };
    (shown.join(" / "), file)
}

/// The box around `marks`, `[x, y, width, height]`.
pub fn union_rect(marks: &[[f32; 4]]) -> Option<[f32; 4]> {
    let first = marks.first()?;
    let (mut left, mut top) = (first[0], first[1]);
    let (mut right, mut bottom) = (first[0] + first[2], first[1] + first[3]);
    for [x, y, w, h] in marks.iter().copied() {
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + w);
        bottom = bottom.max(y + h);
    }
    Some([left, top, right - left, bottom - top])
}

/// The part of a page a citation's preview shows (passageCrop.ts): the
/// passage with some text around it. A passage in one column of a
/// two-column paper keeps to that column. All values are in page units.
pub fn crop_around(
    marks: &[[f32; 4]],
    page: (f32, f32),
    pad: f32,
    max_height: f32,
    min_width: f32,
) -> Option<[f32; 4]> {
    let [px, py, pw, ph] = union_rect(marks)?;
    let (page_w, page_h) = page;
    let one_column = pw < page_w * 0.62;
    let (left, right) = if one_column {
        (px - pad, px + pw + pad)
    } else {
        (0.0, page_w)
    };
    let width = page_w.min((right - left).max(min_width));
    let middle = (left + right) / 2.0;
    let left = (middle - width / 2.0).clamp(0.0, (page_w - width).max(0.0));
    let top = (py - pad).clamp(0.0, page_h);
    let bottom = (py + ph + pad).min(top + max_height).clamp(top, page_h);
    (width > 0.0 && bottom > top).then_some([left, top, width, bottom - top])
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
        assert!(locate_in_items(
            &items,
            "Deep learning dominates benchmark leaderboards today."
        )
        .is_none());
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
    fn selections_rejoin_words_split_by_the_layout() {
        assert_eq!(
            clean_selection("water use increas-\ning with\r\nheat and well-known  rules"),
            "water use increasing with heat and well-known rules"
        );
        assert_eq!(clean_selection("Section A-\nB stays"), "Section A- B stays");
    }

    #[test]
    fn cited_passages_are_kept_per_pdf_with_their_number() {
        let dir = tempfile::tempdir().unwrap();
        let store = PaperStore::new(dir.path().to_path_buf());
        let pdf = store.pdf_path("10.1234/example");
        assert!(!store.record_cited(&pdf, "too short", Some(1)).unwrap());
        assert!(store.record_cited(&pdf, PASSAGE, None).unwrap());
        assert!(store.record_cited(&pdf, PASSAGE, Some(2)).unwrap());
        assert!(
            !store.record_cited(&pdf, PASSAGE, Some(2)).unwrap(),
            "unchanged: no write"
        );
        let cited = store.cited(&pdf).unwrap();
        assert_eq!(cited.len(), 1);
        assert_eq!(cited[0].number, Some(2));
        assert!(store
            .cited(&store.pdf_path("10.1/other"))
            .unwrap()
            .is_empty());
        assert_eq!(cited_label(Some(1), Some(3)), "citation 1, page 3");
        assert_eq!(cited_label(None, Some(3)), "page 3");
        assert_eq!(
            saved_location("/Users/a/Documents/MonoBOT/Research/paper.pdf"),
            ("Documents / MonoBOT / Research".into(), "paper.pdf".into())
        );
        assert_eq!(
            saved_location(r"C:\Users\a\Papers\x.pdf"),
            ("a / Papers".into(), "x.pdf".into())
        );
    }

    #[test]
    fn crops_keep_to_the_passage_column() {
        let page = (600.0, 800.0);
        let crop = crop_around(&[[320.0, 400.0, 200.0, 12.0]], page, 18.0, 220.0, 220.0).unwrap();
        assert!(crop[0] > 250.0 && crop[0] + crop[2] <= 600.0, "{crop:?}");
        assert_eq!(crop[1], 382.0);
        let wide = crop_around(&[[40.0, 10.0, 520.0, 300.0]], page, 18.0, 220.0, 220.0).unwrap();
        assert_eq!((wide[0], wide[2]), (0.0, 600.0));
        assert_eq!(wide[1], 0.0);
        assert_eq!(wide[3], 220.0, "a long passage shows from its start");
        assert!(crop_around(&[], page, 18.0, 220.0, 220.0).is_none());
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
        assert_eq!(target.2, "Example");
        assert!(
            cached_citation(&store, "https://example.com/10.1234/example", Some(PASSAGE)).is_none()
        );
        assert!(
            cached_citation(&store, "https://doi.org/10.1234/unknown", Some(PASSAGE)).is_none()
        );
    }
}
