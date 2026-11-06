//! Shared fixtures for the Office readers' tests: packages are written here
//! with the zip crate so no binary files are committed.
use std::io::Write;

use base64::Engine;
use zip::write::SimpleFileOptions;

/// A zip package from `(path, bytes)` entries.
pub(crate) fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for (name, bytes) in entries {
        writer.start_file(*name, options).expect("a zip entry");
        writer.write_all(bytes).expect("entry bytes");
    }
    writer.finish().expect("a finished zip").into_inner()
}

/// A one-pixel PNG.
pub(crate) fn png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==")
        .expect("valid base64")
}

pub(crate) const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub(crate) const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A `.rels` part from `(id, type, target)` entries.
pub(crate) fn rels(entries: &[(&str, &str, &str)]) -> String {
    let items: String = entries
        .iter()
        .map(|(id, kind, target)| {
            format!("<Relationship Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{target}\"/>")
        })
        .collect();
    format!("<Relationships xmlns=\"{RELS_NS}\">{items}</Relationships>")
}
