//! The voice model on disk: Supertonic 3 (int8), as sherpa-onnx publishes it
//! in one archive pinned by size and SHA-256. The archive is verified while
//! it downloads, unpacked, and removed; a marker naming its checksum says the
//! files beside it came from that archive.
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Component, Path};

use monobot_dictation::model::{download, ModelFile};

use crate::Error;

/// One multilingual model: 31 languages, Spanish and English among them, and
/// ten voices to tell the members of a call apart.
pub const ARCHIVE: ModelFile = ModelFile {
    name: "sherpa-onnx-supertonic-3-tts-int8-2026-05-11.tar.bz2",
    bytes: 128_774_318,
    sha256: "82fa96f91c4ef8abaae3a14a3f4153facf88bed821d1f7331cec2700f432c427",
    url: Some(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/sherpa-onnx-supertonic-3-tts-int8-2026-05-11.tar.bz2",
    ),
};

/// The files the synthesizer loads, taken from the archive's top folder.
pub const FILES: [&str; 7] = [
    "duration_predictor.int8.onnx",
    "text_encoder.int8.onnx",
    "vector_estimator.int8.onnx",
    "vocoder.int8.onnx",
    "tts.json",
    "unicode_indexer.bin",
    "voice.bin",
];

pub(crate) const MARKER: &str = "installed";

/// Every file present, unpacked from the pinned archive.
pub fn installed(dir: &Path) -> bool {
    fs::read_to_string(dir.join(MARKER)).is_ok_and(|sha| sha.trim() == ARCHIVE.sha256)
        && FILES
            .iter()
            .all(|name| fs::metadata(dir.join(name)).is_ok_and(|meta| meta.len() > 0))
}

/// Deletes the model: the marker first, so a half-deleted folder never reads
/// as installed.
pub fn remove(dir: &Path) -> Result<(), Error> {
    for name in std::iter::once(MARKER).chain(FILES) {
        match fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// What the model files take on disk, in bytes.
pub fn disk_bytes(dir: &Path) -> u64 {
    FILES
        .iter()
        .filter_map(|name| fs::metadata(dir.join(name)).ok())
        .map(|meta| meta.len())
        .sum()
}

pub fn path(dir: &Path, name: &str) -> Result<String, Error> {
    let path = dir.join(name);
    path.to_str()
        .filter(|path| !path.contains('\0'))
        .map(str::to_owned)
        .ok_or(Error::Path(path))
}

/// Downloads and verifies the archive, reporting bytes done out of the total,
/// then unpacks the model files next to it.
pub fn install(dir: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), Error> {
    if installed(dir) {
        progress(ARCHIVE.bytes, ARCHIVE.bytes);
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    let _ = fs::remove_file(dir.join(MARKER));
    let archive = dir.join(ARCHIVE.name);
    let mut done = 0;
    download(&ARCHIVE, &archive, |chunk| {
        done += chunk;
        progress(done, ARCHIVE.bytes);
    })?;
    let unpacked = File::open(&archive)
        .map_err(Error::from)
        .and_then(|file| unpack(BufReader::new(file), dir));
    let _ = fs::remove_file(&archive);
    unpacked?;
    fs::write(dir.join(MARKER), ARCHIVE.sha256)?;
    Ok(())
}

/// Writes the archive's model files into `dir` by their bare names. Anything
/// else in the archive (samples, readme) is skipped, and so is any entry whose
/// path is not a plain `top/name`.
pub(crate) fn unpack(archive: impl Read, dir: &Path) -> Result<(), Error> {
    let mut found = [false; FILES.len()];
    let mut entries = tar::Archive::new(bzip2::read::BzDecoder::new(archive));
    for entry in entries.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?.into_owned();
        let Some(index) = model_file(&path) else {
            continue;
        };
        let target = dir.join(FILES[index]);
        let partial = target.with_extension("part");
        let mut out = File::create(&partial)?;
        io::copy(&mut entry, &mut out)?;
        drop(out);
        fs::rename(&partial, &target)?;
        found[index] = true;
    }
    match FILES.iter().zip(found).find(|(_, found)| !found) {
        Some((missing, _)) => Err(Error::Missing(missing)),
        None => Ok(()),
    }
}

/// Which model file an archive path is: exactly one folder, then a name.
fn model_file(path: &Path) -> Option<usize> {
    let parts: Vec<_> = path
        .components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect();
    let [Component::Normal(_), Component::Normal(name)] = parts.as_slice() else {
        return None;
    };
    FILES.iter().position(|file| name.to_str() == Some(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("monobot-voice-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, *data).unwrap();
        }
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        bz.write_all(&tar.into_inner().unwrap()).unwrap();
        bz.finish().unwrap()
    }

    #[test]
    fn pins_one_https_archive() {
        assert!(
            monobot_dictation::model::url_of(&ARCHIVE).starts_with("https://github.com/k2-fsa/")
        );
        assert!(monobot_dictation::model::url_of(&ARCHIVE).ends_with(ARCHIVE.name));
        assert_eq!(ARCHIVE.sha256.len(), 64);
    }

    #[test]
    fn unpacks_only_the_model_files_by_name() {
        let dir = temp_dir("unpack");
        let mut entries: Vec<(String, &[u8])> = FILES
            .iter()
            .map(|name| {
                (
                    format!("sherpa-onnx-supertonic-3/{name}"),
                    b"model".as_slice(),
                )
            })
            .collect();
        entries.push(("sherpa-onnx-supertonic-3/README.md".into(), b"readme"));
        entries.push((
            "sherpa-onnx-supertonic-3/test_wavs/voice.bin".into(),
            b"nested",
        ));
        let borrowed: Vec<(&str, &[u8])> = entries.iter().map(|(p, d)| (p.as_str(), *d)).collect();
        unpack(archive(&borrowed).as_slice(), &dir).unwrap();
        for name in FILES {
            assert_eq!(fs::read(dir.join(name)).unwrap(), b"model", "{name}");
        }
        assert!(!dir.join("README.md").exists());
        assert!(!dir.join("test_wavs").exists());
        // Unpacked but not marked: a download cut short is never "installed".
        assert!(!installed(&dir));
        fs::write(dir.join(MARKER), ARCHIVE.sha256).unwrap();
        assert!(installed(&dir));
        assert_eq!(disk_bytes(&dir), 5 * FILES.len() as u64);
        // Removing deletes the model and its marker, and can run twice.
        remove(&dir).unwrap();
        assert!(!installed(&dir));
        assert!(!dir.join(MARKER).exists());
        assert_eq!(disk_bytes(&dir), 0);
        remove(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_archive_missing_a_file_fails() {
        let dir = temp_dir("missing");
        let error = unpack(archive(&[("top/tts.json", b"{}")]).as_slice(), &dir).unwrap_err();
        assert!(matches!(
            error,
            Error::Missing("duration_predictor.int8.onnx")
        ));
        fs::remove_dir_all(&dir).unwrap();
    }
}
