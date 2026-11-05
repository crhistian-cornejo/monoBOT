//! A first look at a file that came with a message, before the owner opens it: what it really is (its first bytes, not
//! its name), and the signs of a file meant to harm: a program or a script, a name that hides its ending, an Office
//! document with macros, an archive with a program inside or a password on it, a PDF that launches or runs something.
//! Nothing is opened, unpacked or run: the bytes are only read, in memory.

use serde::Serialize;

/// How a file looks: nothing found, something to be careful with, or something MonoBOT won't open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Safe,
    Caution,
    Danger,
}

/// The verdict on a file and what led to it, in words for the owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Risk {
    pub level: Level,
    pub reasons: Vec<String>,
}

/// Endings of programs and scripts a click would run.
const PROGRAMS: &[&str] = &[
    "exe", "scr", "com", "pif", "bat", "cmd", "msi", "msp", "cpl", "dll", "sys", "jar", "js", "jse", "vbs", "vbe", "wsf", "wsh", "ps1", "psm1", "hta", "lnk", "reg", "app", "command", "sh",
    "apk", "dmg", "pkg", "iso", "img", "vhd", "vhdx", "appx", "appxbundle", "msix", "scf", "url", "inf", "gadget", "chm", "xll", "xlam", "one", "application", "settingcontent-ms",
];
/// Office documents that carry macros by their kind.
const MACROS: &[&str] = &["docm", "dotm", "xlsm", "xltm", "xlsb", "pptm", "potm", "ppsm", "sldm"];
/// Pages a browser opens: a sign-in form that sends a password anywhere often comes this way.
const PAGES: &[&str] = &["html", "htm", "shtml", "xhtml", "svg", "mht", "mhtml"];
/// Archives whose contents can't be looked into here.
const SEALED: &[&str] = &["rar", "7z", "cab", "arj", "ace", "gz", "tgz", "xz", "bz2", "tar"];
/// Endings a document has, which a program's name puts before its own to pass for one.
const DOCUMENTS: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "jpg", "jpeg", "png", "gif", "zip", "rtf", "csv", "odt"];

/// The most entries of a ZIP that are read.
const MAX_ENTRIES: usize = 2_000;

/// What the first bytes say a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sniffed {
    Windows,
    Unix,
    Mac,
    Script,
    Zip,
    Ole,
    Pdf,
    Rtf,
    Rar,
    SevenZip,
    Png,
    Jpeg,
    Gif,
    Webp,
    Html,
    Unknown,
}

fn sniff(bytes: &[u8]) -> Sniffed {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    if starts(b"MZ") {
        Sniffed::Windows
    } else if starts(b"\x7fELF") {
        Sniffed::Unix
    } else if [[0xfe, 0xed, 0xfa, 0xce], [0xfe, 0xed, 0xfa, 0xcf], [0xce, 0xfa, 0xed, 0xfe], [0xcf, 0xfa, 0xed, 0xfe], [0xca, 0xfe, 0xba, 0xbe]].iter().any(|magic| starts(magic)) {
        Sniffed::Mac
    } else if starts(b"#!") {
        Sniffed::Script
    } else if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        Sniffed::Zip
    } else if starts(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
        Sniffed::Ole
    } else if bytes.len() >= 5 && bytes[..bytes.len().min(1024)].windows(5).any(|w| w == b"%PDF-") {
        Sniffed::Pdf
    } else if starts(b"{\\rtf") {
        Sniffed::Rtf
    } else if starts(b"Rar!") {
        Sniffed::Rar
    } else if starts(&[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c]) {
        Sniffed::SevenZip
    } else if starts(b"\x89PNG\r\n\x1a\n") {
        Sniffed::Png
    } else if starts(&[0xff, 0xd8, 0xff]) {
        Sniffed::Jpeg
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Sniffed::Gif
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Sniffed::Webp
    } else {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
        let head = head.trim_start_matches('\u{feff}').trim_start();
        if head.starts_with("<!doctype html") || head.starts_with("<html") || head.starts_with("<svg") || head.starts_with("<?xml") && head.contains("<svg") {
            Sniffed::Html
        } else {
            Sniffed::Unknown
        }
    }
}

/// The media type of a picture the reader may show from its bytes, when they are one.
pub fn picture_type(bytes: &[u8]) -> Option<&'static str> {
    match sniff(bytes) {
        Sniffed::Png => Some("image/png"),
        Sniffed::Jpeg => Some("image/jpeg"),
        Sniffed::Gif => Some("image/gif"),
        Sniffed::Webp => Some("image/webp"),
        _ => None,
    }
}

fn ending(name: &str) -> String {
    name.rsplit_once('.').map(|(_, end)| end.trim().to_ascii_lowercase()).unwrap_or_default()
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

/// The names in a ZIP (read from its local headers, not unpacked) and whether any entry has a password on it.
fn zip_entries(bytes: &[u8]) -> (Vec<String>, bool) {
    let (mut names, mut locked, mut at) = (Vec::new(), false, 0);
    while names.len() < MAX_ENTRIES {
        let Some(found) = bytes[at..].windows(4).position(|w| w == b"PK\x03\x04") else { break };
        let start = at + found;
        let field = |offset: usize| bytes.get(start + offset..start + offset + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
        let (Some(flags), Some(length)) = (field(6), field(26)) else { break };
        locked |= flags & 1 == 1;
        if let Some(name) = bytes.get(start + 30..start + 30 + length) {
            names.push(String::from_utf8_lossy(name).into_owned());
        }
        at = start + 4;
    }
    (names, locked)
}

/// Looks at one file: its name as the message gives it, the media type it claims and its bytes.
pub fn inspect(name: &str, kind: &str, bytes: &[u8]) -> Risk {
    let mut found: Vec<(Level, String)> = Vec::new();
    let mut say = |level: Level, reason: String| found.push((level, reason));
    let end = ending(name);
    let sniffed = sniff(bytes);

    if name.chars().any(|c| matches!(c, '\u{202e}' | '\u{202d}' | '\u{200e}' | '\u{200f}' | '\u{2066}'..='\u{2069}')) {
        say(Level::Danger, "Its name hides its real ending with a reversed-text character.".into());
    }
    let parts: Vec<String> = name.to_ascii_lowercase().split('.').map(|p| p.trim().to_string()).collect();
    if parts.len() >= 3 && PROGRAMS.contains(&end.as_str()) && DOCUMENTS.contains(&parts[parts.len() - 2].as_str()) {
        say(Level::Danger, format!("Its name passes for a .{} but it ends in .{end}.", parts[parts.len() - 2]));
    }
    if name.contains("   ") {
        say(Level::Caution, "Its name has a run of spaces that can push its real ending out of view.".into());
    }

    match sniffed {
        Sniffed::Windows => say(Level::Danger, "It is a Windows program.".into()),
        Sniffed::Unix => say(Level::Danger, "It is a Linux program.".into()),
        Sniffed::Mac => say(Level::Danger, "It is a Mac program.".into()),
        Sniffed::Script => say(Level::Danger, "It is a script that runs commands.".into()),
        _ if PROGRAMS.contains(&end.as_str()) => say(Level::Danger, format!("A .{end} file runs as a program when opened.")),
        _ => {}
    }
    if MACROS.contains(&end.as_str()) {
        say(Level::Danger, "It is an Office document made to carry macros.".into());
    }

    match sniffed {
        Sniffed::Zip => {
            let (names, locked) = zip_entries(bytes);
            if names.iter().any(|n| n.ends_with("vbaProject.bin")) {
                say(Level::Danger, "It is an Office document with macros.".into());
            }
            if let Some(program) = names.iter().find(|n| PROGRAMS.contains(&ending(n).as_str()) && !n.ends_with('/')) {
                say(Level::Danger, format!("It holds a program: {}.", program.rsplit('/').next().unwrap_or(program)));
            }
            if names.iter().any(|n| MACROS.contains(&ending(n).as_str())) {
                say(Level::Danger, "It holds an Office document with macros.".into());
            }
            if locked {
                say(Level::Caution, "It has a password, so what is inside can't be checked.".into());
            }
            if names.iter().any(|n| SEALED.contains(&ending(n).as_str()) || ending(n) == "zip") {
                say(Level::Caution, "It holds another archive, which can't be checked here.".into());
            }
        }
        Sniffed::Ole if contains(bytes, b"_VBA_PROJECT") || contains(bytes, b"V\0B\0A\0") => {
            say(Level::Danger, "It is an Office document with macros.".into());
        }
        Sniffed::Pdf => {
            if contains(bytes, b"/Launch") {
                say(Level::Danger, "The PDF asks to start a program.".into());
            }
            if contains(bytes, b"/JavaScript") || contains(bytes, b"/JS ") || contains(bytes, b"/JS(") || contains(bytes, b"/JS<") {
                say(Level::Caution, "The PDF carries code that runs when it opens.".into());
            }
            if contains(bytes, b"/EmbeddedFile") {
                say(Level::Caution, "The PDF has other files inside it.".into());
            }
        }
        Sniffed::Rtf if contains(bytes, b"\\objdata") || contains(bytes, b"\\objupdate") => {
            say(Level::Danger, "The document carries an embedded object, a common way to attack.".into());
        }
        Sniffed::Rar | Sniffed::SevenZip => say(Level::Caution, "It is an archive whose contents can't be checked here.".into()),
        Sniffed::Html => say(Level::Caution, "It is a web page: fake sign-in forms often come this way.".into()),
        _ => {}
    }
    if sniffed == Sniffed::Unknown {
        if PAGES.contains(&end.as_str()) {
            say(Level::Caution, "It is a web page: fake sign-in forms often come this way.".into());
        } else if SEALED.contains(&end.as_str()) {
            say(Level::Caution, "It is an archive whose contents can't be checked here.".into());
        }
    }

    // A name that promises one thing over bytes that are another.
    let claims = |ends: &[&str], is: &[Sniffed]| ends.contains(&end.as_str()) && !is.contains(&sniffed);
    if claims(&["pdf"], &[Sniffed::Pdf])
        || claims(&["png", "jpg", "jpeg", "gif", "webp"], &[Sniffed::Png, Sniffed::Jpeg, Sniffed::Gif, Sniffed::Webp])
        || claims(&["docx", "xlsx", "pptx", "zip", "odt", "ods"], &[Sniffed::Zip])
        || claims(&["doc", "xls", "ppt"], &[Sniffed::Ole, Sniffed::Zip])
    {
        say(Level::Caution, format!("It is not the .{end} its name says it is."));
    }
    if kind.starts_with("application/x-msdownload") || kind == "application/x-msdos-program" {
        say(Level::Danger, "The message calls it a Windows program.".into());
    }

    found.dedup_by(|a, b| a.1 == b.1);
    let level = found.iter().map(|(level, _)| *level).max().unwrap_or(Level::Safe);
    Risk { level, reasons: found.into_iter().map(|(_, reason)| reason).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip(names: &[(&str, u16)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, flags) in names {
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&[20, 0]);
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&[0; 18]);
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(b"data");
        }
        out
    }

    #[test]
    fn a_plain_pdf_or_picture_is_safe() {
        assert_eq!(inspect("factura.pdf", "application/pdf", b"%PDF-1.7\n1 0 obj << /Type /Catalog >>").level, Level::Safe);
        assert_eq!(inspect("foto.jpg", "image/jpeg", &[0xff, 0xd8, 0xff, 0xe0, 1, 2]).level, Level::Safe);
        assert_eq!(inspect("informe.docx", "application/vnd.openxmlformats", &zip(&[("word/document.xml", 0)])).level, Level::Safe);
    }

    #[test]
    fn a_program_is_blocked_whatever_its_name_says() {
        let risk = inspect("factura.pdf", "application/pdf", b"MZ\x90\x00 program");
        assert_eq!(risk.level, Level::Danger);
        assert!(risk.reasons.iter().any(|r| r.contains("Windows program")), "{risk:?}");
        assert!(risk.reasons.iter().any(|r| r.contains("not the .pdf")), "{risk:?}");
        assert_eq!(inspect("setup.exe", "application/octet-stream", b"anything").level, Level::Danger);
        assert_eq!(inspect("run.sh", "text/plain", b"#!/bin/sh\nrm -rf ~").level, Level::Danger);
    }

    #[test]
    fn a_name_that_hides_its_ending_is_blocked() {
        assert_eq!(inspect("factura.pdf.exe", "application/octet-stream", b"x").level, Level::Danger);
        let reversed = inspect("factura\u{202e}fdp.exe", "application/octet-stream", b"x");
        assert!(reversed.reasons.iter().any(|r| r.contains("reversed-text")), "{reversed:?}");
    }

    #[test]
    fn macros_and_programs_in_archives_are_blocked_and_a_password_is_a_caution() {
        assert_eq!(inspect("pedido.docx", "", &zip(&[("word/document.xml", 0), ("word/vbaProject.bin", 0)])).level, Level::Danger);
        assert_eq!(inspect("tabla.xlsm", "", &zip(&[("xl/workbook.xml", 0)])).level, Level::Danger);
        let held = inspect("fotos.zip", "application/zip", &zip(&[("fotos/1.jpg", 0), ("fotos/ver.scr", 0)]));
        assert_eq!(held.level, Level::Danger);
        assert!(held.reasons.iter().any(|r| r.contains("ver.scr")), "{held:?}");
        let locked = inspect("docs.zip", "application/zip", &zip(&[("a.pdf", 1)]));
        assert_eq!(locked.level, Level::Caution);
        assert!(locked.reasons[0].contains("password"));
        assert_eq!(inspect("viejo.doc", "", b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1....._VBA_PROJECT").level, Level::Danger);
    }

    #[test]
    fn a_pdf_that_runs_or_launches_and_a_web_page_are_flagged() {
        assert_eq!(inspect("a.pdf", "", b"%PDF-1.4 /OpenAction << /S /Launch /F (cmd.exe) >>").level, Level::Danger);
        assert_eq!(inspect("a.pdf", "", b"%PDF-1.4 /OpenAction << /S /JavaScript /JS (app.alert(1)) >>").level, Level::Caution);
        assert_eq!(inspect("acceso.html", "text/html", b"<!DOCTYPE html><form>").level, Level::Caution);
        assert_eq!(inspect("x.rtf", "", b"{\\rtf1 {\\object\\objemb{\\objdata 0105}}}").level, Level::Danger);
        assert_eq!(inspect("archivo.rar", "", b"Rar!\x1a\x07").level, Level::Caution);
    }

    #[test]
    fn it_knows_a_picture_by_its_bytes_and_never_an_svg() {
        assert_eq!(picture_type(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(picture_type(b"<svg onload=alert(1)>"), None);
        assert_eq!(picture_type(b"MZ"), None);
    }
}
