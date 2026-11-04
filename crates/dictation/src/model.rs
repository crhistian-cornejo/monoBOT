//! The speech model on disk: Parakeet TDT 0.6B v3 (int8) and the Silero voice
//! detector, each pinned by size and SHA-256. Nothing is fetched while the
//! microphone is open; the user installs the model once from the app.
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Error;

pub struct ModelFile {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    /// None: the file is in Parakeet's pinned repository.
    pub url: Option<&'static str>,
}

const PARAKEET: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78";

/// Parakeet v3 recognizes 25 European languages, Spanish and English among
/// them, and tells them apart on its own.
pub const FILES: [ModelFile; 5] = [
    ModelFile {
        name: "encoder.int8.onnx",
        bytes: 652_184_281,
        sha256: "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247",
        url: None,
    },
    ModelFile {
        name: "decoder.int8.onnx",
        bytes: 11_845_275,
        sha256: "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e",
        url: None,
    },
    ModelFile {
        name: "joiner.int8.onnx",
        bytes: 6_355_277,
        sha256: "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3",
        url: None,
    },
    ModelFile {
        name: "tokens.txt",
        bytes: 93_939,
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        url: None,
    },
    ModelFile {
        name: "silero_vad.onnx",
        bytes: 643_854,
        sha256: "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6",
        url: Some("https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx"),
    },
];

pub fn url_of(file: &ModelFile) -> String {
    match file.url {
        Some(url) => url.to_owned(),
        None => format!("{PARAKEET}/{}", file.name),
    }
}

pub fn total_bytes() -> u64 {
    FILES.iter().map(|file| file.bytes).sum()
}

/// Every file present at its pinned size. The full checksum is verified when
/// a file is downloaded, not on every launch.
pub fn installed(dir: &Path) -> bool {
    FILES.iter().all(|file| {
        fs::metadata(dir.join(file.name)).is_ok_and(|meta| meta.len() == file.bytes)
    })
}

pub fn path(dir: &Path, name: &str) -> Result<String, Error> {
    let path = dir.join(name);
    path.to_str()
        .filter(|path| !path.contains('\0'))
        .map(str::to_owned)
        .ok_or(Error::Path(path))
}

/// Downloads what is missing, verifying each file's SHA-256 as it streams,
/// and reports bytes done out of the total after every chunk.
pub fn install(dir: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), Error> {
    fs::create_dir_all(dir)?;
    let total = total_bytes();
    let mut done = 0;
    for file in &FILES {
        let target = dir.join(file.name);
        if fs::metadata(&target).is_ok_and(|meta| meta.len() == file.bytes) {
            done += file.bytes;
            progress(done, total);
            continue;
        }
        download(file, &target, |chunk| {
            done += chunk;
            progress(done, total);
        })?;
    }
    Ok(())
}

fn download(file: &ModelFile, target: &Path, mut on_chunk: impl FnMut(u64)) -> Result<(), Error> {
    let partial = target.with_extension("part");
    let response = ureq::get(&url_of(file))
        .call()
        .map_err(|error| Error::Download(format!("{}: {error}", file.name)))?;
    let mut reader = response.into_reader();
    let mut out = File::create(&partial)?;
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        written += read as u64;
        if written > file.bytes {
            drop(out);
            let _ = fs::remove_file(&partial);
            return Err(Error::Integrity(file.name.into()));
        }
        hasher.update(&buffer[..read]);
        out.write_all(&buffer[..read])?;
        on_chunk(read as u64);
    }
    out.flush()?;
    drop(out);
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if written != file.bytes || hex != file.sha256 {
        let _ = fs::remove_file(&partial);
        return Err(Error::Integrity(file.name.into()));
    }
    fs::rename(&partial, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_names_its_pinned_source() {
        for file in &FILES {
            let url = url_of(file);
            assert!(url.starts_with("https://"), "{url}");
            assert!(url.ends_with(file.name), "{url}");
            assert_eq!(file.sha256.len(), 64);
        }
        assert!(url_of(&FILES[0]).contains("2bda32ec70b097a55adaa07d9a7173915b43cc78"));
    }

    #[test]
    fn a_folder_with_a_short_file_is_not_installed() {
        let dir = std::env::temp_dir().join(format!("monobot-dictation-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tokens.txt"), b"short").unwrap();
        assert!(!installed(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }
}
