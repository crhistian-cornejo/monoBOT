//! The speech model on disk: Parakeet TDT 0.6B v3 (int8) and the Silero voice
//! detector, each pinned by size and SHA-256. Nothing is fetched while the
//! microphone is open; the user installs the model once from the app.
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::shared::{self, Place};
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

/// Gets what is missing, reporting bytes done out of the total after every
/// chunk: from another app on this computer that already has the same file
/// (see `shared`), else downloaded, verifying each file's SHA-256 either way.
pub fn install(dir: &Path, progress: impl FnMut(u64, u64)) -> Result<(), Error> {
    install_from(dir, &FILES, &shared::places(), progress)
}

pub(crate) fn install_from(
    dir: &Path,
    files: &[ModelFile],
    places: &[Place],
    mut progress: impl FnMut(u64, u64),
) -> Result<(), Error> {
    fs::create_dir_all(dir)?;
    let total = files.iter().map(|file| file.bytes).sum();
    let mut done = 0;
    // What GitHub's archive put in place, counted when it did.
    let mut unpacked: Vec<&str> = Vec::new();
    for file in files {
        let target = dir.join(file.name);
        if present(dir, file) {
            if !unpacked.contains(&file.name) {
                done += file.bytes;
            }
            progress(done, total);
            continue;
        }
        let before = done;
        let adopted = shared::find(file, places).is_some_and(|found| {
            shared::adopt(&found.path, file, &target, |chunk| {
                done += chunk;
                progress(done, total);
            })
            .is_ok()
        });
        if adopted {
            continue;
        }
        done = before;
        match download(file, &target, |chunk| {
            done += chunk;
            progress(done, total);
        }) {
            Ok(()) => {}
            // Hugging Face refused, never answered or sent something else:
            // the rest of Parakeet comes from GitHub, and the loop then finds
            // it in place.
            Err(Error::Download(_)) if file.url.is_none() => {
                unpacked = from_archive(dir, before, total, &mut progress)?;
                done = before
                    + FILES
                        .iter()
                        .filter(|file| unpacked.contains(&file.name))
                        .map(|file| file.bytes)
                        .sum::<u64>();
                progress(done, total);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// The app that already has every missing file, so the install prompt can
/// offer to use it instead of downloading. Checks names and sizes only.
pub fn shared_from(dir: &Path) -> Option<&'static str> {
    shared_from_places(dir, &FILES, &shared::places())
}

pub(crate) fn shared_from_places(
    dir: &Path,
    files: &[ModelFile],
    places: &[Place],
) -> Option<&'static str> {
    let mut app = None;
    for file in files {
        if fs::metadata(dir.join(file.name)).is_ok_and(|meta| meta.len() == file.bytes) {
            continue;
        }
        let found = shared::find(file, places)?;
        app.get_or_insert(found.app);
    }
    app
}

fn present(dir: &Path, file: &ModelFile) -> bool {
    fs::metadata(dir.join(file.name)).is_ok_and(|meta| meta.len() == file.bytes)
}

/// Downloads Parakeet's archive from GitHub, unpacks the files still missing
/// and deletes the archive. Returns the names of the files it put in place.
fn from_archive(
    dir: &Path,
    done: u64,
    total: u64,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<&'static str>, Error> {
    let names: Vec<&'static str> = FILES
        .iter()
        .filter(|file| file.url.is_none() && !present(dir, file))
        .map(|file| file.name)
        .collect();
    let missing: u64 = FILES
        .iter()
        .filter(|file| names.contains(&file.name))
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
    unpacked.map(|()| names)
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

/// The HTTP client for the model files, with the app's certificate checks.
fn agent() -> ureq::Agent {
    monobot_http::agent().build()
}

/// Downloads one pinned file to `target`, reporting each chunk's size. The
/// file only lands at `target` when its size and SHA-256 both match; the
/// voice crate fetches its model through here too.
///
/// A company proxy that blocks a site may answer with a page of its own
/// instead of the file. Anything that is not the file (a web page, another
/// length, a connection cut short) is a failed download, so Parakeet still
/// has GitHub to try; the right length with other bytes is a checksum error.
pub fn download(file: &ModelFile, target: &Path, mut on_chunk: impl FnMut(u64)) -> Result<(), Error> {
    let failed = |why: String| Error::Download(format!("{}: {why}", file.name));
    let partial = target.with_extension("part");
    let response = agent()
        .get(&url_of(file))
        .call()
        .map_err(|error| failed(error.to_string()))?;
    if let Some(why) = not_the_file(&response, file) {
        return Err(failed(why));
    }
    let mut reader = response.into_reader();
    let mut out = File::create(&partial)?;
    let streamed = (|| -> Result<Sha256, Error> {
        let mut hasher = Sha256::new();
        let mut written = 0u64;
        let mut buffer = vec![0u8; 1 << 16];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| failed(error.to_string()))?;
            if read == 0 {
                break;
            }
            written += read as u64;
            if written > file.bytes {
                return Err(failed(format!("more than the {} bytes expected", file.bytes)));
            }
            hasher.update(&buffer[..read]);
            out.write_all(&buffer[..read])?;
            on_chunk(read as u64);
        }
        if written != file.bytes {
            return Err(failed(format!("{written} of the {} bytes expected", file.bytes)));
        }
        out.flush()?;
        Ok(hasher)
    })();
    drop(out);
    let verified = streamed.and_then(|hasher| {
        if hex(hasher) == file.sha256 {
            Ok(())
        } else {
            Err(Error::Integrity(file.name.into()))
        }
    });
    if let Err(error) = verified {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    fs::rename(&partial, target)?;
    Ok(())
}

/// Why a response cannot be the pinned file, from its headers alone: a web
/// page, or a stated length other than the file's.
fn not_the_file(response: &ureq::Response, file: &ModelFile) -> Option<String> {
    let kind = response.content_type().trim();
    if kind.eq_ignore_ascii_case("text/html") || kind.eq_ignore_ascii_case("application/xhtml+xml") {
        return Some(format!("the network answered with a web page ({kind}), not the file"));
    }
    match response.header("content-length").map(str::parse::<u64>) {
        Some(Ok(length)) if length != file.bytes => Some(format!(
            "the network offered {length} bytes, not the {} expected",
            file.bytes
        )),
        _ => None,
    }
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

    /// Two small files standing in for the model: sha256("hello"), ("vad").
    const SMALL: [ModelFile; 2] = [
        ModelFile {
            name: "tokens.txt",
            bytes: 5,
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            url: None,
        },
        ModelFile {
            name: "silero_vad.onnx",
            bytes: 3,
            sha256: "651bfad0aa5b42c5a5b8dad76f49c4a122fe1d1658fc55cfdd1ea9923fe3fd9e",
            url: Some("https://example.invalid/silero_vad.onnx"),
        },
    ];

    fn echo_with(root: &Path, files: &[(&str, &str, &[u8])]) -> Vec<Place> {
        let voice = root.join("Echo").join("voice");
        for (folder, name, data) in files {
            fs::create_dir_all(voice.join(folder)).unwrap();
            fs::write(voice.join(folder).join(name), data).unwrap();
        }
        vec![Place { app: "Echo", dir: voice }]
    }

    #[test]
    fn takes_the_model_from_echo_instead_of_downloading_it() {
        let root = std::env::temp_dir().join(format!("monobot-dictation-echo-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let files = SMALL;
        let places = echo_with(
            &root,
            &[("parakeet-v3-int8", "tokens.txt", b"hello"), ("whisper-turbo-int8", "silero_vad.onnx", b"vad")],
        );
        let dir = root.join("monobot");
        assert_eq!(shared_from_places(&dir, &files, &places), Some("Echo"));
        let mut last = (0, 0);
        // The silero URL cannot be reached: had it downloaded, this would fail.
        install_from(&dir, &files, &places, |done, total| last = (done, total)).unwrap();
        assert_eq!(last, (8, 8));
        assert_eq!(fs::read(dir.join("tokens.txt")).unwrap(), b"hello");
        assert_eq!(fs::read(dir.join("silero_vad.onnx")).unwrap(), b"vad");
        // Installed: nothing left to offer.
        assert_eq!(shared_from_places(&dir, &files, &places), None);
        fs::remove_dir_all(&root).unwrap();
    }

    /// With the real model where this user's Echo keeps it: `cargo test -p
    /// monobot-dictation -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn takes_the_real_model_from_this_computers_echo() {
        let dir = std::env::temp_dir().join(format!("monobot-dictation-real-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let places = shared::places();
        eprintln!("looking in {places:#?}");
        assert!(shared_from_places(&dir, &FILES, &places).is_some(), "no app here has the model");
        let started = std::time::Instant::now();
        install_from(&dir, &FILES, &places, |_, _| {}).unwrap();
        eprintln!("installed from another app in {:?}", started.elapsed());
        assert!(installed(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Answers one request on this machine with `reply`, as a site (or a
    /// proxy standing in for it) would, and returns the address to ask.
    fn serve(reply: &[u8]) -> &'static str {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/tokens.txt", listener.local_addr().unwrap());
        let reply = reply.to_vec();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut request = BufReader::new(stream);
            let mut line = String::new();
            // The request's headers end with an empty line.
            while request.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let mut stream = request.into_inner();
            stream.write_all(&reply).unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });
        Box::leak(url.into_boxed_str())
    }

    /// Downloads SMALL's tokens.txt ("hello") from a server that answers
    /// with `reply`; returns what landed in place.
    fn fetch(tag: &str, reply: &[u8]) -> Result<Vec<u8>, Error> {
        let dir = std::env::temp_dir().join(format!("monobot-dictation-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = ModelFile {
            url: Some(serve(reply)),
            ..SMALL[0]
        };
        let target = dir.join(file.name);
        let result = download(&file, &target, |_| {}).map(|()| fs::read(&target).unwrap());
        // Nothing half-written stays behind, and nothing wrong lands.
        assert!(!target.with_extension("part").exists());
        assert_eq!(target.exists(), result.is_ok());
        fs::remove_dir_all(&dir).unwrap();
        result
    }

    fn reply(kind: &str, length: Option<usize>, body: &str) -> Vec<u8> {
        let length = length.map_or("Connection: close\r\n".into(), |length| {
            format!("Content-Length: {length}\r\n")
        });
        format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\n{length}\r\n{body}").into_bytes()
    }

    #[test]
    fn the_pinned_file_lands_in_place() {
        let got = fetch("ok", &reply("text/plain; charset=utf-8", Some(5), "hello"));
        assert_eq!(got.unwrap(), b"hello");
        // Without a stated length too: the body ends with the connection.
        let got = fetch("unsized", &reply("application/octet-stream", None, "hello"));
        assert_eq!(got.unwrap(), b"hello");
    }

    #[test]
    fn a_proxy_page_instead_of_the_file_is_a_failed_download() {
        let page = "<html><body>Blocked by your organization</body></html>";
        let got = fetch("page", &reply("text/html; charset=utf-8", Some(page.len()), page));
        assert!(matches!(got, Err(Error::Download(_))), "{got:?}");
        // A page is a page even at the file's length.
        let got = fetch("page5", &reply("TEXT/HTML", Some(5), "hello"));
        assert!(matches!(got, Err(Error::Download(_))), "{got:?}");
        let refused = b"HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: 2\r\n\r\nno";
        let got = fetch("refused", refused);
        assert!(matches!(got, Err(Error::Download(_))), "{got:?}");
    }

    #[test]
    fn another_length_is_a_failed_download() {
        let kind = "application/octet-stream";
        for (tag, length, body) in [
            ("stated", Some(4), "hell"),
            ("short", None, "hell"),
            ("long", None, "hello, world"),
        ] {
            let got = fetch(tag, &reply(kind, length, body));
            assert!(matches!(got, Err(Error::Download(_))), "{tag}: {got:?}");
        }
    }

    #[test]
    fn the_right_length_with_other_bytes_fails_its_checksum() {
        let got = fetch("tampered", &reply("application/octet-stream", Some(5), "hellO"));
        assert!(matches!(got, Err(Error::Integrity(_))), "{got:?}");
    }

    /// Through this computer's own network, company proxy and all: silero
    /// comes from GitHub, and Hugging Face either sends tokens.txt or fails
    /// as a download, which turns install to GitHub's archive. Run it by
    /// hand: `cargo test -p monobot-dictation -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn downloads_through_this_network() {
        let dir = std::env::temp_dir().join(format!("monobot-dictation-net-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let named = |name: &str| FILES.iter().find(|file| file.name == name).unwrap();
        let silero = named("silero_vad.onnx");
        download(silero, &dir.join(silero.name), |_| {}).unwrap();
        assert!(present(&dir, silero));
        let tokens = named("tokens.txt");
        match download(tokens, &dir.join(tokens.name), |_| {}) {
            Ok(()) => eprintln!("Hugging Face sent tokens.txt"),
            Err(Error::Download(why)) => eprintln!("Hugging Face failed as a download: {why}"),
            Err(error) => panic!("{error}"),
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The whole model through this computer's network as a new user gets it,
    /// with no other app to take it from; behind a proxy that refuses Hugging
    /// Face, Parakeet comes from GitHub. The bar only moves forward and ends
    /// full. About 1.2 GB: `cargo test -p monobot-dictation -- --ignored
    /// --nocapture installs_through_this_network`.
    #[test]
    #[ignore]
    fn installs_through_this_network() {
        let dir = std::env::temp_dir().join(format!("monobot-dictation-full-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let started = std::time::Instant::now();
        let mut last = (0, 0);
        install_from(&dir, &FILES, &[], |done, total| {
            assert!(done >= last.0 && done <= total, "{done} after {} of {total}", last.0);
            last = (done, total);
        })
        .unwrap();
        eprintln!("installed in {:?}", started.elapsed());
        assert_eq!(last, (total_bytes(), total_bytes()));
        assert!(installed(&dir));
        assert!(!dir.join(ARCHIVE.name).exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn offers_echo_only_when_it_has_everything_missing() {
        let root = std::env::temp_dir().join(format!("monobot-dictation-part-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let places = echo_with(&root, &[("parakeet-v3-int8", "tokens.txt", b"hello")]);
        let dir = root.join("monobot");
        assert_eq!(shared_from_places(&dir, &SMALL, &places), None);
        // Once the missing file is here, the rest is in Echo.
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("silero_vad.onnx"), b"vad").unwrap();
        assert_eq!(shared_from_places(&dir, &SMALL, &places), Some("Echo"));
        fs::remove_dir_all(&root).unwrap();
    }
}
