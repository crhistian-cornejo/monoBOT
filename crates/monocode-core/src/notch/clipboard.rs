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

    fn insert(&mut self, clip: Clip) {
        self.clips.insert(0, clip);
        self.clips.truncate(KEPT);
    }

    /// A kept copy was copied again: it becomes the newest.
    pub fn raise(&mut self, index: usize, now: u64) {
        if index < self.clips.len() {
            let mut clip = self.clips.remove(index);
            clip.at = now;
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
