//! Port of src/features/sessions/model/wordMotion.ts: how a streamed reply's
//! words come out and come into focus. A word fades in over WORD_FOCUS, keeps
//! the accent colour as a trail for WORD_TRAIL, and the words one paint lets
//! out start WORD_STAGGER apart. Markdown marks wait for the word after them,
//! and a half-received table waits for its header. (The web build also blurs
//! a word in by 4px; GPUI text has no blur, so focus is opacity alone.)
use std::time::Duration;

/// How long a word takes to come into focus.
pub(crate) const WORD_FOCUS: Duration = Duration::from_millis(560);
/// How long a word keeps a trail of the accent colour.
pub(crate) const WORD_TRAIL: Duration = Duration::from_millis(1120);
/// Words let out in the same paint start this far apart.
pub(crate) const WORD_STAGGER: Duration = Duration::from_millis(20);
/// The most a paced word waits behind the others of its paint.
pub(crate) const PACED_STAGGER_CAP: Duration = Duration::from_millis(60);

/// WORD_EASE, cubic-bezier(0.22, 1, 0.36, 1): quick to start, long to settle.
pub(crate) fn ease(t: f32) -> f32 {
    crate::reveal::bezier(t.clamp(0., 1.), 0.22, 1., 0.36, 1.)
}

/// When the word at `index` of one paint starts fading in.
pub(crate) fn stagger(index: usize, cap: Duration) -> Duration {
    (WORD_STAGGER * index.min(u32::MAX as usize) as u32).min(cap)
}

/// How long a word stays animated: the longest of its fade and its trail.
pub(crate) fn linger(fade: Duration) -> Duration {
    fade.max(WORD_TRAIL)
}

/// How much of the accent a word still wears, `elapsed` after it started:
/// all of it for the first 30% of the trail, then easing to none.
pub(crate) fn trail(elapsed: Duration) -> f32 {
    let t = elapsed.as_secs_f32() / WORD_TRAIL.as_secs_f32();
    if t <= 0.3 {
        1.
    } else {
        1. - ease((t - 0.3) / 0.7)
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\n' | b'\t' | b'\r')
}

/// A word that reads on its own. A Markdown mark alone (a list's `-`, `##`,
/// `|`, `>`, `1.`) goes out with the word after it.
pub(crate) fn word_reads(word: &str) -> bool {
    // Bytes, not a `str` slice: a word ending in "á" or an emoji has no char
    // boundary one byte before its end.
    let numbered = match word.as_bytes() {
        [digits @ .., b'.' | b')'] => !digits.is_empty() && digits.iter().all(u8::is_ascii_digit),
        _ => false,
    };
    word.chars().any(char::is_alphanumeric) && !numbered
}

/// Where the word that starts at or before `from` begins.
fn word_start(text: &[u8], from: usize) -> usize {
    let mut start = from;
    while start > 0 && !is_space(text[start - 1]) {
        start -= 1;
    }
    start
}

/// Where a stream still being written may stop: before the word still being
/// written, and before any marks waiting for the word after them.
fn held_end(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut end = word_start(bytes, bytes.len());
    loop {
        let mut word_end = end;
        while word_end > 0 && is_space(bytes[word_end - 1]) {
            word_end -= 1;
        }
        let start = word_start(bytes, word_end);
        if start == word_end || word_reads(&text[start..word_end]) {
            return end;
        }
        end = start;
    }
}

/// Where to stop revealing `text` for a reveal that has reached `at`: the
/// end of the word `at` falls in, and past any mark it lands on to the end
/// of the word after it. A stream still mid-word holds back at the last
/// whole word; a finished one runs out.
pub(crate) fn reveal_end(text: &str, at: usize, streaming: bool) -> usize {
    let bytes = text.as_bytes();
    let mut i = at.min(bytes.len());
    while i < bytes.len() {
        while i < bytes.len() && !is_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if word_reads(&text[word_start(bytes, i)..i]) {
            return i;
        }
        while i < bytes.len() && is_space(bytes[i]) {
            i += 1;
        }
    }
    if !streaming {
        return text.len();
    }
    held_end(text)
}

fn table_line(line: &str) -> bool {
    let indent = line.len() - line.trim_start_matches(' ').len();
    indent <= 3 && line[indent..].starts_with('|')
}

/// The cells of a table row, without the bars at its ends; `\|` is text.
fn table_cells(line: &str) -> Vec<String> {
    let inner = line.trim();
    let inner = inner.strip_prefix('|').unwrap_or(inner);
    let inner = if inner.ends_with('|') && !inner.ends_with("\\|") {
        &inner[..inner.len() - 1]
    } else {
        inner
    };
    let mut cells = vec![String::new()];
    let mut escaped = false;
    for c in inner.chars() {
        if c == '|' && !escaped {
            cells.push(String::new());
        } else {
            cells.last_mut().unwrap().push(c);
        }
        escaped = c == '\\';
    }
    cells
}

fn delimiter_row(line: &str, header: &str) -> bool {
    let cells = table_cells(line);
    cells.len() == table_cells(header).len()
        && cells.iter().all(|cell| {
            let cell = cell.trim();
            let cell = cell.strip_prefix(':').unwrap_or(cell);
            let cell = cell.strip_suffix(':').unwrap_or(cell);
            !cell.is_empty() && cell.bytes().all(|b| b == b'-')
        })
}

fn fence_open(text: &str) -> bool {
    text.lines()
        .filter(|line| {
            let indent = line.len() - line.trim_start_matches(' ').len();
            let rest = &line[indent..];
            indent <= 3 && (rest.starts_with("```") || rest.starts_with("~~~"))
        })
        .count()
        % 2
        == 1
}

/// `text` without a table it ends in that is not a table yet: its rows wait
/// until the header and the line under it have arrived. Inside an open code
/// fence nothing is held.
pub(crate) fn hold_half_table(text: &str) -> &str {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut start = body.len() + 1;
    let mut end = body.len();
    let mut lines = Vec::new();
    loop {
        let from = if end == 0 {
            0
        } else {
            body[..end].rfind('\n').map_or(0, |i| i + 1)
        };
        let line = &body[from..end];
        if !table_line(line) {
            break;
        }
        lines.insert(0, line);
        start = from;
        if from == 0 {
            break;
        }
        end = from - 1;
    }
    if lines.is_empty() || fence_open(text) {
        return text;
    }
    if lines.len() >= 2 && delimiter_row(lines[1], lines[0]) {
        return text;
    }
    &text[..start.min(text.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_do_not_read_on_their_own() {
        assert!(word_reads("Hello") && word_reads("42") && word_reads("**bold**"));
        for mark in ["-", "*", "##", "|", ">", "1.", "2)", "|---|"] {
            assert!(!word_reads(mark), "{mark}");
        }
    }

    #[test]
    fn marks_go_out_with_the_word_after_them() {
        let text = "Intro\n- item one";
        assert_eq!(reveal_end(text, 6, true), text.find(" one").unwrap());
        assert_eq!(reveal_end("## Title here", 0, true), 8);
        assert_eq!(reveal_end("1. First step", 0, true), 8);
        assert_eq!(reveal_end("Intro\n- ", 6, true), 6);
        assert_eq!(reveal_end("Intro\n- it", 6, true), 6);
        assert_eq!(reveal_end("Intro\n## ", 7, true), 6);
        assert_eq!(reveal_end("Intro\n- ", 6, false), 8);
    }

    #[test]
    fn words_ending_in_accents_or_emoji_read_without_panicking() {
        // A word whose last character takes more than one byte once sliced
        // the string inside it and aborted the app.
        for word in ["está", "Mamá", "ñandú", "café", "listo🎉", "¿cómo"] {
            assert!(word_reads(word), "{word}");
        }
        assert!(!word_reads("¿"));
        let reply = "Hola, ¿cómo está? Todo está listo 🎉 y funciona.";
        for at in 0..=reply.len() {
            let end = reveal_end(reply, at, true);
            assert!(reply.is_char_boundary(end), "{at} -> {end}");
            assert!(reply.is_char_boundary(reveal_end(reply, at, false)));
        }
        assert_eq!(reveal_end("Está bien", 0, true), "Está".len());
        assert_eq!(reveal_end("Todo está", 5, false), "Todo está".len());
    }

    #[test]
    fn a_table_waits_for_its_header() {
        let header = "Before\n\n| Name | Size |\n";
        assert_eq!(hold_half_table(header), "Before\n\n");
        assert_eq!(hold_half_table(&format!("{header}| --- |")), "Before\n\n");
        let table = format!("{header}| --- | :---: |\n| a | 1");
        assert_eq!(hold_half_table(&table), table);
        assert_eq!(
            hold_half_table("Plain text\nand more"),
            "Plain text\nand more"
        );
        let after = format!("{header}| --- | --- |\n\nAfter");
        assert_eq!(hold_half_table(&after), after);
        assert_eq!(
            hold_half_table("```sh\n| not a table"),
            "```sh\n| not a table"
        );
        let escaped = "| a \\| b | c |\n| --- | --- |";
        assert_eq!(hold_half_table(escaped), escaped);
    }

    #[test]
    fn timing_matches_the_web_build() {
        assert_eq!(stagger(0, PACED_STAGGER_CAP), Duration::ZERO);
        assert_eq!(stagger(1, PACED_STAGGER_CAP), Duration::from_millis(20));
        assert_eq!(stagger(50, PACED_STAGGER_CAP), PACED_STAGGER_CAP);
        assert_eq!(linger(WORD_FOCUS), WORD_TRAIL);
        assert_eq!(linger(Duration::from_secs(2)), Duration::from_secs(2));
        assert_eq!(trail(Duration::ZERO), 1.);
        assert_eq!(trail(WORD_TRAIL * 3 / 10), 1.);
        assert!(trail(WORD_TRAIL) < 0.001);
    }
}
