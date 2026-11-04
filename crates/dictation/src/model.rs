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

/// The same four Parakeet files from sherpa-onnx's own GitHub release, for a
/// network that refuses Hugging Face (a company proxy, say), as Echo does. The
/// archive is pinned as published, and each member it holds is byte for byte
/// the file pinned above, so it passes the same checksum.
pub const ARCHIVE: ModelFile = ModelFile {
    name: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
    bytes: 487_170_055,
    sha256: "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
    url: Some("https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2"),
};
const ARCHIVE_FOLDER: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";
/// Room for what the archive holds besides the model (samples, a README).
const UNPACK_SLACK: u64 = 64 * 1024 * 1024;

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
        let before = done;
        match download(file, &target, |chunk| {
            done += chunk;
            progress(done, total);
        }) {
            Ok(()) => {}
            // Hugging Face refused or never answered: the rest of Parakeet
            // comes from GitHub, and the loop then finds it in place.
            Err(Error::Download(_)) if file.url.is_none() => {
                done = before + from_archive(dir, before, total, &mut progress)?;
                progress(done, total);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn present(dir: &Path, file: &ModelFile) -> bool {
    fs::metadata(dir.join(file.name)).is_ok_and(|meta| meta.len() == file.bytes)
}

/// Downloads Parakeet's archive from GitHub, unpacks the files still missing
/// and deletes the archive. Returns the bytes of model it put in place.
fn from_archive(
    dir: &Path,
    done: u64,
    total: u64,
    progress: &mut impl FnMut(u64, u64),
) -> Result<u64, Error> {
    let missing: u64 = FILES
        .iter()
        .filter(|file| file.url.is_none() && !present(dir, file))
        .map(|file| file.bytes)
        .sum();
    let archive = dir.join(ARCHIVE.name);
    if !present(dir, &ARCHIVE) {
        let mut got = 0u64;
        // The bar keeps moving over the model's own bytes, not the archive's.
        download(&ARCHIVE, &archive, |chunk| {
            got += chunk;
            progress(done + got * missing / ARCHIVE.bytes, total);
        })?;
    }
    let unpacked = unpack(dir, &archive);
    let _ = fs::remove_file(&archive);
    unpacked.map(|()| missing)
}

/// A reader that refuses to give more than `left` bytes: a crafted archive
/// cannot fill the disk.
struct Bounded<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.left = self
            .left
            .checked_sub(count as u64)
            .ok_or(std::io::ErrorKind::InvalidData)?;
        Ok(count)
    }
}

/// Takes Parakeet's files out of the verified archive and nothing else. Every
/// path must stay inside its one folder, anything but a plain file or folder
/// refuses the archive, and each member is written under this code's name, at
/// its pinned size, to a temporary file that becomes the model file only once
/// its SHA-256 matches.
fn unpack(dir: &Path, archive: &Path) -> Result<(), Error> {
    use std::path::Component;
    let bad = || Error::Integrity(ARCHIVE.name.into());
    let members: Vec<&ModelFile> = FILES.iter().filter(|file| file.url.is_none()).collect();
    let limit = members.iter().map(|file| file.bytes).sum::<u64>() + UNPACK_SLACK;
    let source = std::io::BufReader::new(File::open(archive)?);
    let mut tar = tar::Archive::new(Bounded {
        inner: bzip2::read::BzDecoder::new(source),
        left: limit,
    });
    let mut found = vec![false; members.len()];
    for entry in tar.entries().map_err(|_| bad())? {
        let mut entry = entry.map_err(|_| bad())?;
        let kind = entry.header().entry_type();
        let name = entry.path().map_err(|_| bad())?.into_owned();
        let mut parts = name.components();
        if parts.next() != Some(Component::Normal(ARCHIVE_FOLDER.as_ref()))
            || !parts.clone().all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(bad());
        }
        if kind == tar::EntryType::Directory {
            continue;
        }
        if kind != tar::EntryType::Regular {
            return Err(bad());
        }
        let Some(at) = members
            .iter()
            .position(|file| Path::new(file.name) == parts.as_path())
        else {
            continue;
        };
        let member = members[at];
        if found[at] || entry.size() != member.bytes {
            return Err(bad());
        }
        found[at] = true;
        if present(dir, member) {
            continue;
        }
        let target = dir.join(member.name);
        let partial = target.with_extension("unpack");
        let written = (|| -> Result<(), Error> {
            let mut out = File::create(&partial)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 1 << 16];
            let mut left = member.bytes;
            while left > 0 {
                let count = entry.read(&mut buffer).map_err(|_| bad())?;
                if count == 0 || count as u64 > left {
                    return Err(bad());
                }
                hasher.update(&buffer[..count]);
                out.write_all(&buffer[..count])?;
                left -= count as u64;
            }
            out.flush()?;
            if hex(hasher) != member.sha256 {
                return Err(Error::Integrity(member.name.into()));
            }
            Ok(())
        })();
        if let Err(error) = written {
            let _ = fs::remove_file(&partial);
            return Err(error);
        }
        fs::rename(&partial, &target)?;
    }
    if found.iter().all(|found| *found) {
        Ok(())
    } else {
        Err(bad())
    }
}

fn hex(hasher: Sha256) -> String {
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The HTTP client for the model files. On the Mac it trusts the public
/// authorities bundled with the app; on Windows, the system's own store.
fn agent() -> ureq::Agent {
    let builder = ureq::AgentBuilder::new();
    #[cfg(windows)]
    let builder = match system_tls() {
        Ok(config) => builder.tls_config(config),
        Err(_) => builder,
    };
    builder.build()
}

/// Nothing is switched off: the verifier is Windows' own certificate check,
/// so a certificate nobody trusts is still refused.
#[cfg(windows)]
fn system_tls() -> Result<std::sync::Arc<rustls::ClientConfig>, rustls::Error> {
    use std::sync::Arc;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls_platform_verifier::Verifier::new(provider.clone())?;
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn download(file: &ModelFile, target: &Path, mut on_chunk: impl FnMut(u64)) -> Result<(), Error> {
    let partial = target.with_extension("part");
    let response = agent()
        .get(&url_of(file))
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
    if written != file.bytes || hex(hasher) != file.sha256 {
        let _ = fs::remove_file(&partial);
        return Err(Error::Integrity(file.name.into()));
    }
    fs::rename(&partial, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Reaches the GitHub archive with the client the download uses; needs
    /// the network, so run it by hand: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn reaches_the_github_archive() {
        let response = agent().head(&url_of(&ARCHIVE)).call().unwrap();
        assert!(response.status() < 400);
        let length: u64 = response.header("content-length").unwrap().parse().unwrap();
        assert_eq!(length, ARCHIVE.bytes);
    }

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
