//! Bounded PDF text extraction and page-based excerpts for paper actions.
use lopdf::{content::Content, Document, LoadOptions, Object, ObjectId};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::OnceLock,
};

const MAX_PDF_BYTES: usize = 40 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;
const MAX_PAGES: usize = 80;
const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageSpan {
    pub text: String,
    /// Normalized [0.0, 1.0] top-left coordinates: [x, y, width, height].
    pub rect: [f32; 4],
}

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
    for (page, page_id) in pages.iter().take(MAX_PAGES) {
        let mut text = doc
            .extract_text_with_limit(&[*page], MAX_STREAM_BYTES)
            .unwrap_or_default();
        if text.trim().is_empty() {
            let spans = extract_page_spans_from_doc(&doc, *page_id);
            if !spans.is_empty() {
                text = spans
                    .into_iter()
                    .map(|span| span.text)
                    .collect::<Vec<_>>()
                    .join(" ");
            }
        }
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

/// Extract positioned word spans for a zero-based page index, normalized to
/// `[0.0, 1.0]` top-left coordinates `[x, y, width, height]`.
pub fn extract_page_spans(bytes: &[u8], page_index: usize) -> Vec<PageSpan> {
    if bytes.len() > MAX_PDF_BYTES || !bytes.starts_with(b"%PDF-") {
        return Vec::new();
    }
    let Ok(doc) = Document::load_mem_with_options(
        bytes,
        LoadOptions::with_max_decompressed_size(MAX_STREAM_BYTES),
    ) else {
        return Vec::new();
    };
    let pages = doc.get_pages();
    let Some((_, &page_id)) = pages.iter().nth(page_index) else {
        return Vec::new();
    };
    extract_page_spans_from_doc(&doc, page_id)
}

/// Merge adjacent word spans on the same horizontal line into clean highlight
/// bars `[x, y, width, height]`.
pub fn merge_span_rects(spans: &[PageSpan]) -> Vec<[f32; 4]> {
    let mut out: Vec<[f32; 4]> = Vec::new();
    for span in spans {
        let [x, y, w, h] = span.rect;
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        if let Some(last) = out.last_mut() {
            let last_cy = last[1] + last[3] * 0.5;
            let cur_cy = y + h * 0.5;
            let max_h = last[3].max(h);
            let last_right = last[0] + last[2];
            if (last_cy - cur_cy).abs() <= max_h * 0.65
                && x >= last[0] - 0.01
                && x <= last_right + 0.06
            {
                let min_x = last[0].min(x);
                let min_y = last[1].min(y);
                let max_x = last_right.max(x + w);
                let max_y = (last[1] + last[3]).max(y + h);
                *last = [min_x, min_y, max_x - min_x, max_y - min_y];
                continue;
            }
        }
        out.push([x, y, w, h]);
    }
    out
}

fn obj_f64(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(f) => Some(*f as f64),
        _ => None,
    }
}

fn page_box(doc: &Document, page_id: ObjectId) -> [f64; 4] {
    let mut cur = Some(page_id);
    for _ in 0..16 {
        let Some(id) = cur else {
            break;
        };
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        for key in [b"CropBox".as_slice(), b"MediaBox".as_slice()] {
            if let Ok(obj) = dict.get_deref(key, doc) {
                if let Ok(arr) = obj.as_array() {
                    if arr.len() == 4 {
                        if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                            obj_f64(&arr[0]),
                            obj_f64(&arr[1]),
                            obj_f64(&arr[2]),
                            obj_f64(&arr[3]),
                        ) {
                            let w = (x1 - x0).abs();
                            let h = (y1 - y0).abs();
                            if w > 0.0 && h > 0.0 {
                                return [x0.min(x1), y0.min(y1), w, h];
                            }
                        }
                    }
                }
            }
        }
        cur = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    [0.0, 0.0, 612.0, 792.0]
}

fn mat_mul(m1: [f64; 6], m2: [f64; 6]) -> [f64; 6] {
    [
        m1[0] * m2[0] + m1[1] * m2[2],
        m1[0] * m2[1] + m1[1] * m2[3],
        m1[2] * m2[0] + m1[3] * m2[2],
        m1[2] * m2[1] + m1[3] * m2[3],
        m1[4] * m2[0] + m1[5] * m2[2] + m2[4],
        m1[4] * m2[1] + m1[5] * m2[3] + m2[5],
    ]
}

fn decode_pdf_bytes(encoding: Option<&lopdf::Encoding>, bytes: &[u8]) -> String {
    if let Some(enc) = encoding {
        if let Ok(decoded) = Document::decode_text(enc, bytes) {
            if !decoded.is_empty() {
                return decoded;
            }
        }
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    bytes.iter().map(|&b| b as char).collect()
}

fn extract_page_spans_from_doc(doc: &Document, page_id: ObjectId) -> Vec<PageSpan> {
    let [bx, by, bw, bh] = page_box(doc, page_id);
    let Ok(content_data) = doc.get_page_content_with_limit(page_id, MAX_STREAM_BYTES) else {
        return Vec::new();
    };
    let Ok(content) = Content::decode(&content_data) else {
        return Vec::new();
    };
    let encodings: BTreeMap<Vec<u8>, lopdf::Encoding> = doc
        .get_page_fonts(page_id)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(name, font)| {
            font.get_font_encoding_with_limit(doc, MAX_STREAM_BYTES)
                .ok()
                .map(|enc| (name, enc))
        })
        .collect();

    let mut spans = Vec::new();
    let mut ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut ctm_stack: Vec<[f64; 6]> = Vec::new();
    let mut tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut tlm = tm;
    let mut font_size = 12.0_f64;
    let mut leading = 0.0_f64;
    let mut horiz_scale = 1.0_f64;
    let mut current_encoding: Option<&lopdf::Encoding> = None;

    let push_run = |spans: &mut Vec<PageSpan>,
                    run: &str,
                    tm: &mut [f64; 6],
                    ctm: [f64; 6],
                    font_size: f64,
                    horiz_scale: f64| {
        let char_adv = (font_size.abs().max(1.0)) * 0.52 * horiz_scale;
        let mut word = String::new();
        let mut word_start_adv = 0.0_f64;
        let mut cur_adv = 0.0_f64;

        let flush_word = |spans: &mut Vec<PageSpan>,
                          word: &mut String,
                          start_adv: f64,
                          end_adv: f64,
                          base_tm: [f64; 6]| {
            if word.is_empty() {
                return;
            }
            let text = std::mem::take(word);
            let m_start = mat_mul(mat_mul([1.0, 0.0, 0.0, 1.0, start_adv, 0.0], base_tm), ctm);
            let m_end = mat_mul(mat_mul([1.0, 0.0, 0.0, 1.0, end_adv, 0.0], base_tm), ctm);
            let scale_y = (m_start[1].hypot(m_start[3])).max(1.0) * font_size.abs().max(1.0);
            let x_min = m_start[4].min(m_end[4]);
            let x_max = m_start[4].max(m_end[4]).max(x_min + scale_y * 0.25);
            let y_bottom = m_start[5] - scale_y * 0.2;
            let y_top = m_start[5] + scale_y * 0.85;
            let nx = ((x_min - bx) / bw).clamp(0.0, 1.0) as f32;
            let ny = (1.0 - (y_top - by) / bh).clamp(0.0, 1.0) as f32;
            let nw = ((x_max - x_min) / bw).clamp(0.002, (1.0 - nx as f64).max(0.002)) as f32;
            let nh = ((y_top - y_bottom) / bh).clamp(0.005, (1.0 - ny as f64).max(0.005)) as f32;
            spans.push(PageSpan {
                text,
                rect: [nx, ny, nw, nh],
            });
        };

        let base_tm = *tm;
        for ch in run.chars() {
            if ch.is_whitespace() {
                flush_word(spans, &mut word, word_start_adv, cur_adv, base_tm);
                cur_adv += char_adv;
                word_start_adv = cur_adv;
            } else {
                if word.is_empty() {
                    word_start_adv = cur_adv;
                }
                word.push(ch);
                cur_adv += char_adv;
            }
        }
        flush_word(spans, &mut word, word_start_adv, cur_adv, base_tm);
        *tm = mat_mul([1.0, 0.0, 0.0, 1.0, cur_adv, 0.0], base_tm);
    };

    for op in &content.operations {
        match op.operator.as_str() {
            "q" => ctm_stack.push(ctm),
            "Q" => {
                if let Some(prev) = ctm_stack.pop() {
                    ctm = prev;
                }
            }
            "cm" if op.operands.len() == 6 => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    obj_f64(&op.operands[0]),
                    obj_f64(&op.operands[1]),
                    obj_f64(&op.operands[2]),
                    obj_f64(&op.operands[3]),
                    obj_f64(&op.operands[4]),
                    obj_f64(&op.operands[5]),
                ) {
                    ctm = mat_mul([a, b, c, d, e, f], ctm);
                }
            }
            "BT" => {
                tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                tlm = tm;
            }
            "Tf" => {
                if let Some(Ok(name)) = op.operands.first().map(Object::as_name) {
                    current_encoding = encodings.get(name);
                }
                if let Some(size) = op.operands.get(1).and_then(obj_f64) {
                    font_size = size;
                }
            }
            "TL" => {
                if let Some(l) = op.operands.first().and_then(obj_f64) {
                    leading = l;
                }
            }
            "Tz" => {
                if let Some(s) = op.operands.first().and_then(obj_f64) {
                    horiz_scale = (s / 100.0).max(0.1);
                }
            }
            "Td" if op.operands.len() >= 2 => {
                if let (Some(tx), Some(ty)) = (obj_f64(&op.operands[0]), obj_f64(&op.operands[1])) {
                    tlm = mat_mul([1.0, 0.0, 0.0, 1.0, tx, ty], tlm);
                    tm = tlm;
                }
            }
            "TD" if op.operands.len() >= 2 => {
                if let (Some(tx), Some(ty)) = (obj_f64(&op.operands[0]), obj_f64(&op.operands[1])) {
                    leading = -ty;
                    tlm = mat_mul([1.0, 0.0, 0.0, 1.0, tx, ty], tlm);
                    tm = tlm;
                }
            }
            "Tm" if op.operands.len() == 6 => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    obj_f64(&op.operands[0]),
                    obj_f64(&op.operands[1]),
                    obj_f64(&op.operands[2]),
                    obj_f64(&op.operands[3]),
                    obj_f64(&op.operands[4]),
                    obj_f64(&op.operands[5]),
                ) {
                    tm = [a, b, c, d, e, f];
                    tlm = tm;
                }
            }
            "T*" => {
                tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -leading], tlm);
                tm = tlm;
            }
            "Tj" => {
                if let Some(Object::String(bytes, _)) = op.operands.first() {
                    let s = decode_pdf_bytes(current_encoding, bytes);
                    push_run(&mut spans, &s, &mut tm, ctm, font_size, horiz_scale);
                }
            }
            "'" => {
                tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -leading], tlm);
                tm = tlm;
                if let Some(Object::String(bytes, _)) = op.operands.first() {
                    let s = decode_pdf_bytes(current_encoding, bytes);
                    push_run(&mut spans, &s, &mut tm, ctm, font_size, horiz_scale);
                }
            }
            "\"" => {
                tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -leading], tlm);
                tm = tlm;
                if let Some(Object::String(bytes, _)) = op.operands.get(2) {
                    let s = decode_pdf_bytes(current_encoding, bytes);
                    push_run(&mut spans, &s, &mut tm, ctm, font_size, horiz_scale);
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = op.operands.first() {
                    let mut chunk = String::new();
                    for item in items {
                        match item {
                            Object::String(bytes, _) => {
                                chunk.push_str(&decode_pdf_bytes(current_encoding, bytes));
                            }
                            other => {
                                if let Some(adj) = obj_f64(other) {
                                    if adj <= -120.0 {
                                        if !chunk.is_empty() {
                                            push_run(
                                                &mut spans,
                                                &chunk,
                                                &mut tm,
                                                ctm,
                                                font_size,
                                                horiz_scale,
                                            );
                                            chunk.clear();
                                        }
                                        let shift = (-adj / 1000.0) * font_size * horiz_scale;
                                        tm = mat_mul([1.0, 0.0, 0.0, 1.0, shift, 0.0], tm);
                                    }
                                }
                            }
                        }
                    }
                    if !chunk.is_empty() {
                        push_run(&mut spans, &chunk, &mut tm, ctm, font_size, horiz_scale);
                    }
                }
            }
            _ => {}
        }
    }
    spans
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

    #[test]
    fn extracts_lopdf_positioned_word_spans_and_merges_line_rects() {
        let stream =
            "BT /F1 12 Tf 40 240 Td (Alpha beta gamma) Tj 0 -24 Td (Second line text) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );

        let spans = extract_page_spans(&pdf, 0);
        let words: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            words,
            vec!["Alpha", "beta", "gamma", "Second", "line", "text"]
        );
        let first_line = merge_span_rects(&spans[0..3]);
        assert_eq!(first_line.len(), 1);
        let both_lines = merge_span_rects(&spans);
        assert_eq!(both_lines.len(), 2);
        assert!(both_lines[0][1] < both_lines[1][1]);
    }
}
