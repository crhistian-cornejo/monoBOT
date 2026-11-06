//! Office documents read for showing, not editing: spreadsheets (with their
//! pictures and charts), Word documents and PowerPoint decks. Ported from the
//! Tauri app's `src/features/files/model` readers so the native app shows the
//! same things. Parsing only: nothing here draws.
//!
//! Every format is a zip of XML parts. [`xml`] holds the shared reader.
use serde::{Deserialize, Serialize};

pub mod docx;
pub mod drawings;
#[cfg(test)]
pub(crate) mod fixtures;
pub mod numfmt;
pub mod pptx;
pub mod xlsx;
pub mod xml;

pub use docx::{read_docx, DocxDocument};
pub use pptx::{read_pptx, Deck};
pub use xlsx::{read_workbook, Workbook};

/// A document the agents' panel can show as the file draws it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DocumentKind {
    Pdf,
    Docx,
    Pptx,
}

fn extension_of(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or("");
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// The kind of document at a path, if any. The old binary formats (.doc,
/// .ppt) are not read here; they open in the system's app.
pub fn document_kind(path: &str) -> Option<DocumentKind> {
    match extension_of(path).as_str() {
        "pdf" => Some(DocumentKind::Pdf),
        "docx" | "docm" => Some(DocumentKind::Docx),
        "pptx" | "pptm" | "ppsx" => Some(DocumentKind::Pptx),
        _ => None,
    }
}

pub fn is_document_path(path: &str) -> bool {
    document_kind(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_files_from_a_path() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let csv = dir.path().join("datos.csv");
        std::fs::write(&csv, "a;b\n1;2\n").expect("a csv");
        let workbook = read_workbook(&csv).expect("a workbook");
        assert_eq!(workbook.sheets[0].name, "datos");
        assert_eq!(workbook.sheets[0].cells.len(), 4);

        let package = fixtures::package(&[("hello.txt", b"hi")]);
        let document = dir.path().join("a.docx");
        std::fs::write(&document, &package).expect("a package");
        assert_eq!(
            read_docx(&document),
            Err("This is not a Word document.".to_string())
        );
        let deck = dir.path().join("a.pptx");
        std::fs::write(&deck, &package).expect("a package");
        assert_eq!(
            read_pptx(&deck),
            Err("This is not a PowerPoint deck.".to_string())
        );
        assert!(read_pptx(dir.path().join("missing.pptx")).is_err());
    }

    #[test]
    fn knows_the_documents_the_panel_shows() {
        assert_eq!(
            document_kind("D:/x/Informe final.PDF"),
            Some(DocumentKind::Pdf)
        );
        assert_eq!(
            document_kind("C:\\a\\memoria.docx"),
            Some(DocumentKind::Docx)
        );
        assert_eq!(document_kind("/a/avance.pptx"), Some(DocumentKind::Pptx));
        assert_eq!(document_kind("/a/viejo.ppt"), None);
        assert_eq!(document_kind("/a/tabla.xlsx"), None);
        assert_eq!(document_kind("/a/.pdf"), None);
        assert!(is_document_path("/a/slides.ppsx"));
    }
}
