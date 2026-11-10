//! What the user copied lately, for the notch's Clipboard page. Kept in
//! memory only and never written to disk: a copied secret leaves with the
//! app. The system's clipboard is read elsewhere; this is only the list.
use std::sync::Arc;

/// How many copies the history keeps.
pub const KEPT: usize = 30;
/// A copy larger than this is not kept: the history is for snippets, not
/// for a whole file's text held thirty times over.
pub const LARGEST: usize = 64 * 1024;
/// How many of the copies may be pictures, and how large each may be.
pub const PICTURES: usize = 4;
pub const LARGEST_PICTURE: usize = 8 * 1024 * 1024;

/// What a copy is, for its mark and how it is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Link,
    /// A color written as hex, as `0xRRGGBB`.
    Color(u32),
    /// Source code or a command line.
    Code,
    /// A file or folder's path.
    Path,
    Email,
    /// A picture, with its size in pixels.
    Image(u32, u32),
}

impl Kind {
    /// The kind in a word, beside the copy's age.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Text => "Text",
            Kind::Link => "Link",
            Kind::Color(_) => "Color",
            Kind::Code => "Code",
            Kind::Path => "Path",
            Kind::Email => "Email",
            Kind::Image(..) => "Image",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub kind: Kind,
    pub text: String,
    /// When it was copied, in milliseconds.
    pub at: u64,
    /// A picture's bytes, as a BMP file.
    pub image: Option<Arc<[u8]>>,
}

impl Clip {
    /// The copy on one line, for its row.
    pub fn preview(&self) -> String {
        let mut line = String::new();
        for word in self.text.split_whitespace() {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
            if line.len() > 160 {
                break;
            }
        }
        line
    }
}

impl Clip {
    /// A few short things to say about the copy beside its preview: a
    /// picture's size and shape, how much text there is, where a link goes.
    pub fn facts(&self) -> Vec<String> {
        let text = self.text.trim();
        let count =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let size = || {
            let mut facts = Vec::new();
            let lines = text.lines().count();
            if lines > 1 {
                facts.push(count(lines, "line", "lines"));
            }
            facts.push(count(text.split_whitespace().count(), "word", "words"));
            facts.push(count(text.chars().count(), "character", "characters"));
            facts
        };
        match self.kind {
            Kind::Image(width, height) => {
                let mut facts = vec![format!("{width} × {height}")];
                let pixels = width as f64 * height as f64;
                if pixels >= 1_000_000. {
                    facts.push(format!("{:.1} MP", pixels / 1_000_000.));
                }
                facts.push(
                    match width.cmp(&height) {
                        std::cmp::Ordering::Greater => "Landscape",
                        std::cmp::Ordering::Less => "Portrait",
                        std::cmp::Ordering::Equal => "Square",
                    }
                    .into(),
                );
                facts
            }
            Kind::Link => {
                let mut facts: Vec<String> = host(text).map(str::to_owned).into_iter().collect();
                facts.push(
                    if text.starts_with("https://") {
                        "Secure (https)"
                    } else {
                        "Not secure (http)"
                    }
                    .into(),
                );
                facts
            }
            Kind::Path => {
                let (name, folder) = path_parts(text);
                let mut facts = vec![name.to_owned()];
                if !folder.is_empty() {
                    facts.push(format!("in {folder}"));
                }
                facts
            }
            Kind::Email => text
                .split_once('@')
                .map(|(_, domain)| vec![domain.to_owned()])
                .unwrap_or_default(),
            Kind::Code => {
                let mut facts: Vec<String> =
                    language(text).map(str::to_owned).into_iter().collect();
                let lines = text.lines().count();
                facts.push(count(lines, "line", "lines"));
                facts.push(count(text.chars().count(), "character", "characters"));
                facts
            }
            Kind::Color(_) => Vec::new(),
            Kind::Text => size(),
        }
    }

    /// The copy's first `most` lines as code shows them: tabs as four
    /// spaces, the indentation every line shares taken off, blank lines at
    /// the ends left out. With them, how many lines did not fit.
    pub fn code_lines(&self, most: usize) -> (Vec<String>, usize) {
        let lines: Vec<String> = self
            .text
            .lines()
            .map(|line| line.trim_end().replace('\t', "    "))
            .collect();
        let first = lines.iter().position(|line| !line.is_empty()).unwrap_or(0);
        let last = lines
            .iter()
            .rposition(|line| !line.is_empty())
            .map_or(0, |at| at + 1);
        let lines = &lines[first.min(last)..last];
        let shared = lines
            .iter()
            .filter(|line| !line.is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        let shown = lines
            .iter()
            .take(most)
            .map(|line| line.get(shared..).unwrap_or("").to_owned())
            .collect();
        (shown, lines.len().saturating_sub(most))
    }
}

/// Where a link goes: `docs.rs` of `https://docs.rs/gpui/latest`.
pub fn host(link: &str) -> Option<&str> {
    let rest = link.trim().split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    // Past any `user@`, before any `:port`.
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

/// A path's last name and the folder it is in.
pub fn path_parts(path: &str) -> (&str, &str) {
    let path = path.trim();
    let trimmed = path.trim_end_matches(['/', '\\']);
    match trimmed.rfind(['/', '\\']) {
        Some(at) if at + 1 < trimmed.len() => (&trimmed[at + 1..], &trimmed[..at.max(1)]),
        _ => (path, ""),
    }
}

/// A color as it is written in code: hex, `rgb()` and `hsl()`.
pub fn color_formats(color: u32) -> [String; 3] {
    let (r, g, b) = ((color >> 16) & 0xff, (color >> 8) & 0xff, color & 0xff);
    let (rf, gf, bf) = (r as f32 / 255., g as f32 / 255., b as f32 / 255.);
    let (max, min) = (rf.max(gf).max(bf), rf.min(gf).min(bf));
    let lightness = (max + min) / 2.;
    let spread = max - min;
    let saturation = if spread == 0. {
        0.
    } else {
        spread / (1. - (2. * lightness - 1.).abs())
    };
    let hue = if spread == 0. {
        0.
    } else if max == rf {
        60. * ((gf - bf) / spread).rem_euclid(6.)
    } else if max == gf {
        60. * ((bf - rf) / spread + 2.)
    } else {
        60. * ((rf - gf) / spread + 4.)
    };
    [
        format!("#{color:06X}"),
        format!("rgb({r}, {g}, {b})"),
        format!(
            "hsl({}, {}%, {}%)",
            hue.round() as u32 % 360,
            (saturation * 100.).round() as u32,
            (lightness * 100.).round() as u32
        ),
    ]
}

/// The language a snippet reads as, when a line of it gives it away. A
/// guess for a label, nothing depends on it.
pub fn language(code: &str) -> Option<&'static str> {
    let text = code.trim();
    let starts = |words: &[&str]| {
        text.lines()
            .any(|line| words.iter().any(|word| line.trim_start().starts_with(word)))
    };
    if (text.starts_with('{') || text.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(text).is_ok()
    {
        return Some("JSON");
    }
    if starts(&[
        "fn ",
        "impl ",
        "pub fn ",
        "pub struct ",
        "let mut ",
        "use std::",
        "#[derive",
    ]) {
        return Some("Rust");
    }
    if starts(&["def ", "elif ", "from __future__", "import numpy", "print("])
        || (starts(&["import ", "from "]) && !text.contains(';') && !text.contains('{'))
    {
        return Some("Python");
    }
    if starts(&["#include", "int main("]) {
        return Some("C");
    }
    if starts(&[
        "<!doctype",
        "<!DOCTYPE",
        "<html",
        "<div",
        "<span",
        "<script",
    ]) {
        return Some("HTML");
    }
    if starts(&[
        "SELECT ",
        "select ",
        "INSERT INTO",
        "CREATE TABLE",
        "UPDATE ",
    ]) && text.to_ascii_lowercase().contains(" from ")
        || starts(&["INSERT INTO", "CREATE TABLE"])
    {
        return Some("SQL");
    }
    if starts(&[
        "$ ", "sudo ", "git ", "cargo ", "npm ", "pnpm ", "curl ", "docker ", "brew ", "cd ",
        "#!/bin/",
    ]) {
        return Some("Shell");
    }
    if starts(&["interface ", "type ", "export type ", "export interface "])
        || text.contains(": string")
        || text.contains(": number")
    {
        return Some("TypeScript");
    }
    if starts(&[
        "function ",
        "const ",
        "export ",
        "import ",
        "var ",
        "async function ",
    ]) || text.contains("=>")
    {
        return Some("JavaScript");
    }
    None
}

/// `#rgb` or `#rrggbb`, alone.
fn color(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => u32::from_str_radix(hex, 16).ok(),
        3 => {
            let value = u32::from_str_radix(hex, 16).ok()?;
            let (r, g, b) = ((value >> 8) & 0xf, (value >> 4) & 0xf, value & 0xf);
            Some(((r * 17) << 16) | ((g * 17) << 8) | (b * 17))
        }
        _ => None,
    }
}

/// One address, alone.
fn email(text: &str) -> bool {
    let Some((name, host)) = text.split_once('@') else {
        return false;
    };
    !name.is_empty()
        && !text.contains(char::is_whitespace)
        && !host.contains('@')
        && host
            .split_once('.')
            .is_some_and(|(domain, rest)| !domain.is_empty() && !rest.is_empty())
}

/// One line that names a place on a disk: `/usr/bin`, `~/notes`,
/// `C:\Users\me`, `\\server\share`.
fn path(text: &str) -> bool {
    if text.contains('\n') || text.len() < 3 {
        return false;
    }
    let bytes = text.as_bytes();
    let drive =
        bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/');
    drive
        || text.starts_with("\\\\")
        || text.starts_with("~/")
        || (text.starts_with('/') && !text.starts_with("//") && !text.contains(' '))
}

/// Words only code or a command line starts with.
const OPENERS: [&str; 19] = [
    "fn ",
    "def ",
    "function ",
    "#include",
    "impl ",
    "struct ",
    "enum ",
    "const ",
    "cargo ",
    "git ",
    "npm ",
    "pnpm ",
    "$ ",
    "sudo ",
    "curl ",
    "docker ",
    "if (",
    "for (",
    "while (",
];
/// Words code starts with that prose does too ("Use the…", "From what…"):
/// they count only beside some of code's punctuation.
const SHARED: [&str; 10] = [
    "pub ", "let ", "var ", "class ", "import ", "from ", "export ", "return ", "use ", "select ",
];
const SIGNS: [&str; 12] = [
    "=>", "->", "::", "();", "{", "}", "==", "!=", "&&", "||", "</", "/>",
];

/// Whether text reads as code: a line that opens as code does, or enough of
/// the punctuation prose does not use.
fn code(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let opens = |words: &[&str]| {
        lower
            .lines()
            .any(|line| words.iter().any(|word| line.trim_start().starts_with(word)))
    };
    let indented = text
        .lines()
        .filter(|line| line.starts_with("    ") || line.starts_with('\t'))
        .count();
    let signs = SIGNS.iter().filter(|sign| text.contains(**sign)).count()
        + usize::from(text.lines().any(|line| line.trim_end().ends_with(';')))
        + usize::from(indented > 1);
    opens(&OPENERS) || signs >= 2 || (signs >= 1 && opens(&SHARED))
}

pub fn kind_of(text: &str) -> Kind {
    let text = text.trim();
    if let Some(color) = color(text) {
        return Kind::Color(color);
    }
    let link = (text.starts_with("https://") || text.starts_with("http://"))
        && !text.contains(char::is_whitespace);
    if link {
        Kind::Link
    } else if email(text) {
        Kind::Email
    } else if path(text) {
        Kind::Path
    } else if code(text) {
        Kind::Code
    } else {
        Kind::Text
    }
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// A picture as the clipboard holds it (a device-independent bitmap: its
/// header, then its pixels) made into a BMP file, with its size in pixels.
pub fn bmp_from_dib(dib: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let header = le32(dib, 0)?;
    if header < 40 || dib.len() < header as usize {
        return None;
    }
    let width = le32(dib, 4)? as i32;
    let height = le32(dib, 8)? as i32;
    let bits = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
    let compression = le32(dib, 16)?;
    let used = le32(dib, 32)?;
    if width <= 0 || height == 0 {
        return None;
    }
    let palette = if bits <= 8 {
        if used == 0 {
            1u32 << bits
        } else {
            used
        }
    } else {
        used
    };
    // A plain 40-byte header keeps its three color masks after itself.
    let masks = if header == 40 && compression == 3 {
        12
    } else {
        0
    };
    let pixels = 14u32
        .checked_add(header)?
        .checked_add(palette.checked_mul(4)?)?
        .checked_add(masks)?;
    let size = u32::try_from(dib.len()).ok()?.checked_add(14)?;
    if pixels > size {
        return None;
    }
    let mut file = Vec::with_capacity(dib.len() + 14);
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&size.to_le_bytes());
    file.extend_from_slice(&[0; 4]);
    file.extend_from_slice(&pixels.to_le_bytes());
    file.extend_from_slice(dib);
    Some((file, width as u32, height.unsigned_abs()))
}

/// The copies, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    clips: Vec<Clip>,
}

impl History {
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// Takes a copy: blank or oversized text is not kept, and text copied
    /// again moves to the top instead of repeating. True when the list
    /// changed.
    pub fn push(&mut self, text: &str, now: u64) -> bool {
        if text.trim().is_empty() || text.len() > LARGEST {
            return false;
        }
        let same = |clip: &Clip| clip.image.is_none() && clip.text == text;
        if self.clips.first().is_some_and(same) {
            return false;
        }
        self.clips.retain(|clip| !same(clip));
        self.insert(Clip {
            kind: kind_of(text),
            text: text.to_owned(),
            at: now,
            image: None,
        });
        true
    }

    /// Takes a copied picture (a BMP file). The same picture again moves to
    /// the top; only the newest few pictures are kept.
    pub fn push_image(&mut self, bmp: Vec<u8>, width: u32, height: u32, now: u64) -> bool {
        if bmp.is_empty() || bmp.len() > LARGEST_PICTURE {
            return false;
        }
        let same = |clip: &Clip| clip.image.as_deref() == Some(&bmp[..]);
        if self.clips.first().is_some_and(same) {
            return false;
        }
        self.clips.retain(|clip| !same(clip));
        self.insert(Clip {
            kind: Kind::Image(width, height),
            text: format!("Image {width} × {height}"),
            at: now,
            image: Some(bmp.into()),
        });
        let mut pictures = 0;
        self.clips.retain(|clip| {
            pictures += usize::from(clip.image.is_some());
            clip.image.is_none() || pictures <= PICTURES
        });
        true
    }

    fn insert(&mut self, mut clip: Clip) {
        clip.at = self.stamp(clip.at);
        self.clips.insert(0, clip);
        self.clips.truncate(KEPT);
    }

    /// When a copy was made, later than every copy kept: `at` names a copy
    /// (the card shows the one pointed at by it), so two made in the same
    /// millisecond must not share it.
    fn stamp(&self, now: u64) -> u64 {
        self.clips
            .iter()
            .map(|clip| clip.at + 1)
            .max()
            .map_or(now, |after| now.max(after))
    }

    /// A kept copy was copied again: it becomes the newest.
    pub fn raise(&mut self, index: usize, now: u64) {
        if index < self.clips.len() {
            let mut clip = self.clips.remove(index);
            clip.at = self.stamp(now);
            self.clips.insert(0, clip);
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.clips.len() {
            self.clips.remove(index);
        }
    }

    pub fn clear(&mut self) {
        self.clips.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_made_in_the_same_millisecond_keep_their_own_time() {
        let mut history = History::default();
        for text in ["one", "two", "three"] {
            assert!(history.push(text, 5));
        }
        let mut times: Vec<u64> = history.clips().iter().map(|clip| clip.at).collect();
        assert_eq!(times, [7, 6, 5], "newest first, each its own");
        history.raise(2, 5);
        times = history.clips().iter().map(|clip| clip.at).collect();
        assert_eq!(times, [8, 7, 6]);
    }

    fn clip(text: &str) -> Clip {
        Clip {
            kind: kind_of(text),
            text: text.into(),
            at: 0,
            image: None,
        }
    }

    #[test]
    fn a_copy_says_a_few_things_about_itself() {
        let picture = Clip {
            kind: Kind::Image(1920, 1080),
            text: String::new(),
            at: 0,
            image: None,
        };
        assert_eq!(picture.facts(), ["1920 × 1080", "2.1 MP", "Landscape"]);
        let small = Clip {
            kind: Kind::Image(64, 64),
            ..picture.clone()
        };
        assert_eq!(small.facts(), ["64 × 64", "Square"]);
        assert_eq!(
            clip("https://user@docs.rs:443/gpui/latest?x=1").facts(),
            ["docs.rs", "Secure (https)"]
        );
        assert_eq!(
            clip("http://example.com").facts(),
            ["example.com", "Not secure (http)"]
        );
        assert_eq!(
            clip("/Users/me/notes/plan.md").facts(),
            ["plan.md", "in /Users/me/notes"]
        );
        assert_eq!(clip("ana@ejemplo.test").facts(), ["ejemplo.test"]);
        assert_eq!(
            clip("One line of plain words").facts(),
            ["5 words", "23 characters"]
        );
        assert_eq!(
            clip("Two lines\nof words").facts(),
            ["2 lines", "4 words", "18 characters"]
        );
        assert_eq!(
            clip("fn main() {\n    run();\n}").facts(),
            ["Rust", "3 lines", "24 characters"]
        );
        assert!(clip("#ff8800").facts().is_empty());
    }

    #[test]
    fn code_is_shown_without_the_indentation_its_lines_share() {
        let snippet = clip("\n\n        if ready {\n\t\t\tgo();\n        }\n\n");
        let (lines, more) = snippet.code_lines(8);
        assert_eq!(lines, ["if ready {", "    go();", "}"]);
        assert_eq!(more, 0);
        let (lines, more) = snippet.code_lines(2);
        assert_eq!((lines.len(), more), (2, 1));
        assert_eq!(clip("   ").code_lines(4), (Vec::new(), 0));
    }

    #[test]
    fn a_color_is_written_three_ways() {
        assert_eq!(
            color_formats(0xff8800),
            ["#FF8800", "rgb(255, 136, 0)", "hsl(32, 100%, 50%)"]
        );
        assert_eq!(
            color_formats(0x000000),
            ["#000000", "rgb(0, 0, 0)", "hsl(0, 0%, 0%)"]
        );
        assert_eq!(color_formats(0xffffff)[2], "hsl(0, 0%, 100%)");
        assert_eq!(color_formats(0x3366cc)[2], "hsl(220, 60%, 50%)");
    }

    #[test]
    fn a_snippets_language_is_guessed_only_when_a_line_gives_it_away() {
        assert_eq!(language("fn main() {}"), Some("Rust"));
        assert_eq!(language("def run():\n    pass"), Some("Python"));
        assert_eq!(language("{\"a\": [1, 2]}"), Some("JSON"));
        assert_eq!(language("git status"), Some("Shell"));
        assert_eq!(language("SELECT id FROM users"), Some("SQL"));
        assert_eq!(language("const a = () => 1;"), Some("JavaScript"));
        assert_eq!(language("interface A { b: string }"), Some("TypeScript"));
        assert_eq!(language("x == y && z"), None);
    }

    #[test]
    fn a_path_is_its_name_and_its_folder() {
        assert_eq!(path_parts("/usr/local/bin/"), ("bin", "/usr/local"));
        assert_eq!(path_parts("~/notes"), ("notes", "~"));
        assert_eq!(
            path_parts("C:\\Users\\me\\a.txt"),
            ("a.txt", "C:\\Users\\me")
        );
        assert_eq!(path_parts("/tmp"), ("tmp", "/"));
        assert_eq!(path_parts("name"), ("name", ""));
    }

    #[test]
    fn a_copy_is_told_apart_by_what_it_is() {
        assert_eq!(kind_of("#6E7BF2"), Kind::Color(0x6e7bf2));
        assert_eq!(kind_of(" #fa0 "), Kind::Color(0xffaa00));
        assert_eq!(kind_of("#nothex"), Kind::Text);
        assert_eq!(kind_of("https://getdroppy.app/blog"), Kind::Link);
        assert_eq!(kind_of("see https://a.b for more"), Kind::Text);
        assert_eq!(kind_of("Launch review moved to 10:00"), Kind::Text);
        assert_eq!(kind_of("ana@example.com"), Kind::Email);
        assert_eq!(kind_of("write to ana@example.com"), Kind::Text);
        assert_eq!(kind_of("C:\\Users\\ana\\notes.txt"), Kind::Path);
        assert_eq!(kind_of("D:/dev/monoBOT"), Kind::Path);
        assert_eq!(kind_of("~/projects/app"), Kind::Path);
        assert_eq!(kind_of("/usr/local/bin"), Kind::Path);
        assert_eq!(kind_of("cargo xtask dev"), Kind::Code);
        assert_eq!(kind_of("fn main() {\n    println!(\"hi\");\n}"), Kind::Code);
        assert_eq!(
            kind_of("const total = items.map(x => x.price);"),
            Kind::Code
        );
        assert_eq!(kind_of("We meet at 5; bring the notes."), Kind::Text);
        assert_eq!(kind_of("Use the blue one from the shelf"), Kind::Text);
        assert_eq!(kind_of("use std::fmt;"), Kind::Code);
        assert_eq!(Kind::Code.label(), "Code");
    }

    #[test]
    fn the_history_keeps_the_newest_once_each_and_only_so_many() {
        let mut history = History::default();
        assert!(!history.push("   \n", 1), "blank");
        assert!(!history.push(&"x".repeat(LARGEST + 1), 1), "too large");
        assert!(history.push("one", 1));
        assert!(history.push("two", 2));
        assert!(!history.push("two", 3), "the same copy again");
        // Copied again later, it moves to the top with its new time.
        assert!(history.push("one", 4));
        let texts: Vec<_> = history
            .clips()
            .iter()
            .map(|clip| (clip.text.as_str(), clip.at))
            .collect();
        assert_eq!(texts, [("one", 4), ("two", 2)]);
        history.raise(1, 9);
        assert_eq!(
            (history.clips()[0].text.as_str(), history.clips()[0].at),
            ("two", 9)
        );
        for at in 0..(KEPT as u64 + 5) {
            history.push(&format!("copy {at}"), at);
        }
        assert_eq!(history.clips().len(), KEPT);
        assert_eq!(history.clips()[0].text, format!("copy {}", KEPT + 4));
        history.remove(0);
        assert_eq!(history.clips().len(), KEPT - 1);
        history.remove(99);
        history.clear();
        assert!(history.clips().is_empty());
    }

    #[test]
    fn a_row_shows_the_copy_on_one_line() {
        let clip = Clip {
            kind: Kind::Text,
            text: "fn main() {\n    println!(\"hi\");\n}".into(),
            at: 0,
            image: None,
        };
        assert_eq!(clip.preview(), "fn main() { println!(\"hi\"); }");
    }

    /// A 2 × 1 picture at 24 bits, as the clipboard holds one.
    fn dib() -> Vec<u8> {
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&2i32.to_le_bytes());
        dib.extend_from_slice(&1i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&24u16.to_le_bytes());
        dib.extend_from_slice(&[0; 24]);
        dib.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
        dib
    }

    #[test]
    fn a_copied_picture_becomes_a_file_with_its_size() {
        let (file, width, height) = bmp_from_dib(&dib()).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(&file[..2], b"BM");
        assert_eq!(le32(&file, 2), Some(file.len() as u32));
        assert_eq!(le32(&file, 10), Some(54), "the pixels follow both headers");
        assert!(bmp_from_dib(&[0; 12]).is_none());
        assert!(bmp_from_dib(&[]).is_none());
    }

    #[test]
    fn pictures_are_kept_once_each_and_only_the_newest_few() {
        let mut history = History::default();
        let picture = |shade: u8| {
            let mut dib = dib();
            *dib.last_mut().unwrap() = shade;
            bmp_from_dib(&dib).unwrap().0
        };
        assert!(history.push_image(picture(1), 2, 1, 1));
        assert!(!history.push_image(picture(1), 2, 1, 2), "the same again");
        assert!(history.push("text between", 3));
        assert!(
            history.push_image(picture(1), 2, 1, 4),
            "it moves to the top"
        );
        assert_eq!(history.clips().len(), 2);
        assert_eq!(history.clips()[0].kind, Kind::Image(2, 1));
        assert_eq!(history.clips()[0].text, "Image 2 × 1");
        for shade in 2..10 {
            history.push_image(picture(shade), 2, 1, u64::from(shade) + 10);
        }
        let pictures = history.clips().iter().filter(|c| c.image.is_some()).count();
        assert_eq!(pictures, PICTURES);
        assert!(history
            .clips()
            .iter()
            .any(|clip| clip.text == "text between"));
        assert!(!history.push_image(Vec::new(), 0, 0, 99));
    }
}
