//! The speech model on disk, each file pinned by size and SHA-256: Parakeet
//! TDT 0.6B v3 and the Silero voice detector. On Apple silicon Parakeet runs
//! on the Neural Engine through Core ML (`Engine::Neural`); elsewhere, the
//! int8 export runs on the CPU through sherpa-onnx (`Engine::Cpu`). The app
//! fetches them itself in the background the first time it runs, and nothing
//! is fetched while the microphone is open.
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
        url: Some(
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
        ),
    },
];

/// Which build of Parakeet v3 transcribes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// The int8 ONNX export on the CPU (sherpa-onnx): any platform, about
    /// three quarters of a second per ten seconds of speech, over a gigabyte
    /// of memory while loaded.
    Cpu,
    /// The Core ML conversion on Apple's Neural Engine: on Apple silicon,
    /// about ten times as fast, loaded in a tenth of a second and some 70 MB
    /// of memory (FluidInference's measurements, which IVY confirmed).
    Neural,
}

impl Engine {
    /// The build this computer should use.
    pub fn preferred() -> Self {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Self::Neural
        } else {
            Self::Cpu
        }
    }

    /// Every file this build needs, with where it goes under the dictation
    /// folder. Silero is shared by both.
    pub fn files(self) -> Vec<(&'static str, &'static ModelFile)> {
        match self {
            Self::Cpu => FILES.iter().map(|file| ("", file)).collect(),
            Self::Neural => NEURAL_FILES
                .iter()
                .map(|file| (NEURAL_FOLDER, file))
                .chain(std::iter::once(("", silero())))
                .collect(),
        }
    }

    pub fn bytes(self) -> u64 {
        self.files().iter().map(|(_, file)| file.bytes).sum()
    }

    pub fn installed(self, dir: &Path) -> bool {
        self.files()
            .iter()
            .all(|(folder, file)| present(&dir.join(folder), file))
    }
}

/// The Silero detector, which both builds share.
pub fn silero() -> &'static ModelFile {
    &FILES[4]
}

/// The build that is on disk, the preferred one first: a computer that had
/// the CPU build before the Neural one arrived keeps dictating meanwhile.
pub fn installed_engine(dir: &Path) -> Option<Engine> {
    let preferred = Engine::preferred();
    [preferred, Engine::Cpu]
        .into_iter()
        .find(|engine| engine.installed(dir))
}

/// Smart Turn v3.2, the unquantised checkpoint (`smart-turn-v3.2-gpu.onnx`
/// of pipecat-ai/smart-turn-v3 at a pinned commit), as IVY pins it: the
/// call's end-of-turn detector, 32 MB, in the dictation folder.
pub const SMART_TURN: ModelFile = ModelFile {
    name: "smart-turn-v3.2.onnx",
    bytes: 32_411_198,
    sha256: "ab8dc64b88713f90b571c15b714bd1330e6c883cad8763dacf65c9376dc539be",
    url: Some("https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-gpu.onnx"),
};

/// Whether Smart Turn is on disk at its pinned size.
pub fn turn_installed(dir: &Path) -> bool {
    present(dir, &SMART_TURN)
}

/// Downloads Smart Turn when it is missing.
pub fn install_turn(dir: &Path) -> Result<(), Error> {
    if turn_installed(dir) {
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    download(&SMART_TURN, &dir.join(SMART_TURN.name), |_| {})
}

/// Where the Neural build lives under the dictation folder.
pub const NEURAL_FOLDER: &str = "neural";

macro_rules! neural {
    ($name:literal, $bytes:expr, $sha:literal) => {
        ModelFile {
            name: $name,
            bytes: $bytes,
            sha256: $sha,
            url: Some(concat!(
                "https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v3-coreml/resolve/7dd20fe6b1797d35f5e3307e8b1732d9a178edfe/",
                $name
            )),
        }
    };
}

/// Parakeet v3 converted to Core ML by FluidInference (CC-BY-4.0, from
/// NVIDIA's model), at a pinned revision: only the parts that are loaded,
/// about 460 MB. The same files IVY pins.
pub const NEURAL_FILES: [ModelFile; 22] = [
    neural!(
        "config.json",
        475,
        "97f19ecccd0fdc730d76fb918090fa8bc64bce8ea4ad43715d5e9c2c7350db66"
    ),
    neural!(
        "parakeet_v3_vocab.json",
        151_122,
        "7ec60e05f1b24480736ec0eed40900f4626bce1fa9a60fd700ec7e2a59198735"
    ),
    neural!(
        "Decoder.mlmodelc/coremldata.bin",
        554,
        "18647af085d87bd8f3121c8a9b4d4564c1ede038dab63d295b4e745cf2d7fb99"
    ),
    neural!(
        "Decoder.mlmodelc/metadata.json",
        3_427,
        "a39e93cd8371b8ded92635c7804fcd0590f0d1dd9415c6d19a0484be073077d9"
    ),
    neural!(
        "Decoder.mlmodelc/model.mil",
        13_110,
        "ef2a0a281695398a62fde86ac269c68f73d5b578d7ed3b31f2ba91a2d1ea1f35"
    ),
    neural!(
        "Decoder.mlmodelc/analytics/coremldata.bin",
        243,
        "4238c4e81ecd0dc94bd7dfbb60f7e2cc824107c1ffe0387b8607b72833dba350"
    ),
    neural!(
        "Decoder.mlmodelc/weights/weight.bin",
        23_604_992,
        "48adf0f0d47c406c8253d4f7fef967436a39da14f5a65e66d5a4b407be355d41"
    ),
    neural!(
        "Encoder.mlmodelc/coremldata.bin",
        485,
        "d48034a167a82e88fc3df64f60af963ab3983538271175b8319e7d5720a0fb86"
    ),
    neural!(
        "Encoder.mlmodelc/metadata.json",
        2_921,
        "da24da9cca943fb29d7fa8e376d57fca7cb3aa08ca51b956b0b0e56813f087e9"
    ),
    neural!(
        "Encoder.mlmodelc/model.mil",
        959_769,
        "ed7b19156ca29fa7dfd6891deb9fda4b0e8893f68597c985d135736546a43808"
    ),
    neural!(
        "Encoder.mlmodelc/analytics/coremldata.bin",
        243,
        "42e638870d73f26b332918a3496ce36793fbb413a81cbd3d16ba01328637a105"
    ),
    neural!(
        "Encoder.mlmodelc/weights/weight.bin",
        445_187_200,
        "e2020f323703477a5b21d7c2d282c403e371afb5962e79877e3033e73ba6f421"
    ),
    neural!(
        "JointDecisionv3.mlmodelc/coremldata.bin",
        521,
        "f5fc08b741400f0088492c9e839418b1e18522f19cba28d361dd030c5f398342"
    ),
    neural!(
        "JointDecisionv3.mlmodelc/metadata.json",
        3_453,
        "d9307211b9a37e0f0ac260c7660b1571a3de25841035cfdf9b58fd40425f890f"
    ),
    neural!(
        "JointDecisionv3.mlmodelc/model.mil",
        11_775,
        "be60732943389a047175111a83f8839f3eb39d4803adafa828a0871b2f39818d"
    ),
    neural!(
        "JointDecisionv3.mlmodelc/analytics/coremldata.bin",
        243,
        "26def4bf73dd56d29dee21c8ef97cb8969e62f6120ed1adc91e46828e2737b6c"
    ),
    neural!(
        "JointDecisionv3.mlmodelc/weights/weight.bin",
        12_642_764,
        "4e0e63d840032f7f07ddb1d64446051166281e5491bf22da8a945c41f6eedb3e"
    ),
    neural!(
        "Preprocessor.mlmodelc/coremldata.bin",
        486,
        "dbde3f2300842c1fd51ef3ff948a0bcffe65ffd2dca10707f2509f32c1d65b1d"
    ),
    neural!(
        "Preprocessor.mlmodelc/metadata.json",
        2_841,
        "2a98699e22d279dd37fa1d238aeb1c6db1df0d6fad687775324157689d8f3acf"
    ),
    neural!(
        "Preprocessor.mlmodelc/model.mil",
        28_181,
        "4b8518a956450fec57f06c2a21bdffc26973f7f1fa6842fb38fe917f896b6b93"
    ),
    neural!(
        "Preprocessor.mlmodelc/analytics/coremldata.bin",
        243,
        "c9beeb989c8d66f8be11df59bc6df277ec76cee404f6865b46243835ef562f6d"
    ),
    neural!(
        "Preprocessor.mlmodelc/weights/weight.bin",
        491_072,
        "129b76e3aeafa8afa3ea76d995b964b145fe83700d579f6ff42c4c38fa0968ea"
    ),
];

/// The same four Parakeet files from sherpa-onnx's own GitHub release, for a
/// network that refuses Hugging Face (a company proxy, say). The
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

/// The download size of the build this computer should use.
pub fn total_bytes() -> u64 {
    Engine::preferred().bytes()
}

/// Some build present, every file at its pinned size. The full checksum is
/// verified when a file is downloaded, not on every launch.
pub fn installed(dir: &Path) -> bool {
    installed_engine(dir).is_some()
}

pub fn path(dir: &Path, name: &str) -> Result<String, Error> {
    let path = dir.join(name);
    path.to_str()
        .filter(|path| !path.contains('\0'))
        .map(str::to_owned)
        .ok_or(Error::Path(path))
}

/// Downloads the build this computer should use.
pub fn install(dir: &Path, progress: impl FnMut(u64, u64)) -> Result<(), Error> {
    match Engine::preferred() {
        Engine::Cpu => install_cpu(dir, progress),
        Engine::Neural => install_neural(dir, progress),
    }
}

/// Downloads what the Neural build is missing, each file verified. Once it is
/// all in place the CPU build's Parakeet files, over 650 MB the Neural build
/// replaces, are deleted; Silero stays, both use it.
pub fn install_neural(dir: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), Error> {
    let files = Engine::Neural.files();
    let total = files.iter().map(|(_, file)| file.bytes).sum();
    let mut done = 0;
    for (folder, file) in files {
        let folder = dir.join(folder);
        if !present(&folder, file) {
            let target = folder.join(file.name);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            download(file, &target, |chunk| {
                done += chunk;
                progress(done, total);
            })?;
        } else {
            done += file.bytes;
        }
        progress(done, total);
    }
    remove_cpu(dir);
    Ok(())
}

/// Deletes every build and the detector, so the folder holds no model; a
/// later dictation offers the download again.
pub fn remove(dir: &Path) -> Result<(), Error> {
    remove_cpu(dir);
    for name in [SMART_TURN.name, silero().name] {
        let _ = fs::remove_file(dir.join(name));
        let _ = fs::remove_file(partial_of(&dir.join(name)));
    }
    match fs::remove_dir_all(dir.join(NEURAL_FOLDER)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// What the model files on disk take, in bytes, for Settings.
pub fn disk_bytes(dir: &Path) -> u64 {
    [Engine::Cpu, Engine::Neural]
        .iter()
        .flat_map(|engine| engine.files())
        .filter(|(folder, file)| present(&dir.join(folder), file))
        .map(|(folder, file)| (folder.to_owned() + file.name, file.bytes))
        .collect::<std::collections::HashMap<_, _>>()
        .values()
        .sum()
}

/// Deletes the CPU build's own files (not Silero). Errors are ignored: a file
/// left behind only takes room.
pub fn remove_cpu(dir: &Path) {
    for file in FILES.iter().filter(|file| file.url.is_none()) {
        let _ = fs::remove_file(dir.join(file.name));
        let _ = fs::remove_file(partial_of(&dir.join(file.name)));
    }
    let _ = fs::remove_file(partial_of(&dir.join(ARCHIVE.name)));
}

/// Downloads what the CPU build is missing, reporting bytes done out of the
/// total after every chunk and verifying each file's SHA-256.
pub fn install_cpu(dir: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), Error> {
    fs::create_dir_all(dir)?;
    let files = &FILES;
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
                // What Hugging Face sent of them before it failed.
                for name in &unpacked {
                    let _ = fs::remove_file(partial_of(&dir.join(name)));
                }
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
            || !parts
                .clone()
                .all(|part| matches!(part, Component::Normal(_)))
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
///
/// A connection cut short leaves what arrived in `<target>.part`, and the
/// next download asks only for the rest (an HTTP range). A server that
/// ignores the range sends the whole file, which then starts over. The
/// SHA-256 always covers the whole file. `on_chunk` hears the bytes already
/// on disk first, so a bar ends full either way.
pub fn download(
    file: &ModelFile,
    target: &Path,
    mut on_chunk: impl FnMut(u64),
) -> Result<(), Error> {
    fetch(file, target, &mut on_chunk)
}

fn fetch(file: &ModelFile, target: &Path, on_chunk: &mut dyn FnMut(u64)) -> Result<(), Error> {
    let failed = |why: String| Error::Download(format!("{}: {why}", file.name));
    let partial = partial_of(target);
    let mut have = fs::metadata(&partial).map_or(0, |meta| meta.len());
    if have > file.bytes {
        let _ = fs::remove_file(&partial);
        have = 0;
    }
    // All of it arrived before: only the checksum is left.
    if have == file.bytes {
        if hex(hash_of(&partial)?) == file.sha256 {
            on_chunk(have);
            fs::rename(&partial, target)?;
            return Ok(());
        }
        let _ = fs::remove_file(&partial);
        have = 0;
    }
    let ask = |from: u64| {
        let request = agent().get(&url_of(file));
        let answer = if from == 0 {
            request.call()
        } else {
            request.set("Range", &format!("bytes={from}-")).call()
        };
        answer.map_err(Box::new)
    };
    let response = match ask(have) {
        Ok(response) => response,
        // The range is past the server's file: start over.
        Err(error) if have > 0 && matches!(*error, ureq::Error::Status(416, _)) => {
            let _ = fs::remove_file(&partial);
            have = 0;
            ask(0).map_err(|error| failed(error.to_string()))?
        }
        Err(error) => return Err(failed(error.to_string())),
    };
    let from = match (have, response.status()) {
        (0, _) => 0,
        (_, 206) if resumes_at(&response, file) == Some(have) => have,
        (_, 206) => {
            let _ = fs::remove_file(&partial);
            return Err(failed("the server resumed from another place".into()));
        }
        // The range was ignored: the whole file comes.
        _ => 0,
    };
    if let Some(why) = not_the_file(&response, file.bytes - from) {
        return Err(failed(why));
    }
    let mut reader = response.into_reader();
    let (mut out, mut hasher) = if from > 0 {
        let hasher = hash_of(&partial)?;
        let out = fs::OpenOptions::new().append(true).open(&partial)?;
        on_chunk(from);
        (out, hasher)
    } else {
        (File::create(&partial)?, Sha256::new())
    };
    // Whether what arrived is worth keeping for the next try: a cut is, too
    // many bytes are not.
    let streamed = (|| -> Result<(), (Error, bool)> {
        let mut written = from;
        let mut buffer = vec![0u8; 1 << 16];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| (failed(error.to_string()), true))?;
            if read == 0 {
                break;
            }
            written += read as u64;
            if written > file.bytes {
                let why = format!("more than the {} bytes expected", file.bytes);
                return Err((failed(why), false));
            }
            hasher.update(&buffer[..read]);
            out.write_all(&buffer[..read])
                .map_err(|error| (error.into(), true))?;
            on_chunk(read as u64);
        }
        out.flush().map_err(|error| (error.into(), true))?;
        if written != file.bytes {
            let why = format!("{written} of the {} bytes expected", file.bytes);
            return Err((failed(why), true));
        }
        Ok(())
    })();
    drop(out);
    let verified = streamed.and_then(|()| {
        if hex(hasher) == file.sha256 {
            Ok(())
        } else {
            Err((Error::Integrity(file.name.into()), false))
        }
    });
    if let Err((error, keep)) = verified {
        if !keep {
            let _ = fs::remove_file(&partial);
        }
        return Err(error);
    }
    fs::rename(&partial, target)?;
    Ok(())
}

/// Where a download in progress keeps what has arrived.
fn partial_of(target: &Path) -> std::path::PathBuf {
    target.with_extension("part")
}

fn hash_of(path: &Path) -> Result<Sha256, Error> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hasher)
}

/// Where a 206 answer's bytes start («Content-Range: bytes 300-999/1000»),
/// when it is a range of this file.
fn resumes_at(response: &ureq::Response, file: &ModelFile) -> Option<u64> {
    let range = response.header("content-range")?.trim();
    let (span, total) = range.strip_prefix("bytes ")?.split_once('/')?;
    if total.trim() != "*" && total.trim().parse::<u64>().ok()? != file.bytes {
        return None;
    }
    let (start, end) = span.split_once('-')?;
    let (start, end) = (start.trim().parse().ok()?, end.trim().parse::<u64>().ok()?);
    (end + 1 == file.bytes).then_some(start)
}

/// Why a response cannot be the pinned file, from its headers alone: a web
/// page, or a stated length other than the bytes `expected`.
fn not_the_file(response: &ureq::Response, expected: u64) -> Option<String> {
    let kind = response.content_type().trim();
    if kind.eq_ignore_ascii_case("text/html") || kind.eq_ignore_ascii_case("application/xhtml+xml")
    {
        return Some(format!(
            "the network answered with a web page ({kind}), not the file"
        ));
    }
    match response.header("content-length").map(str::parse::<u64>) {
        Some(Ok(length)) if length != expected => Some(format!(
            "the network offered {length} bytes, not the {expected} expected"
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
    fn the_neural_build_is_pinned_and_shares_silero() {
        let files = Engine::Neural.files();
        assert_eq!(files.len(), NEURAL_FILES.len() + 1);
        for (folder, file) in &files {
            let url = url_of(file);
            assert!(url.starts_with("https://"), "{url}");
            assert!(url.ends_with(file.name), "{url}");
            assert_eq!(file.sha256.len(), 64);
            if file.name == "silero_vad.onnx" {
                assert_eq!(*folder, "");
            } else {
                assert_eq!(*folder, NEURAL_FOLDER);
                assert!(url.contains("7dd20fe6b1797d35f5e3307e8b1732d9a178edfe"));
            }
        }
        // Each of the four models brings its weights.
        for model in ["Preprocessor", "Encoder", "Decoder", "JointDecisionv3"] {
            let weights = format!("{model}.mlmodelc/weights/weight.bin");
            assert!(
                NEURAL_FILES.iter().any(|file| file.name == weights),
                "{weights}"
            );
        }
        assert!(Engine::Neural.bytes() < Engine::Cpu.bytes());
    }

    #[test]
    fn the_neural_build_is_preferred_on_apple_silicon_only() {
        let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Engine::Neural
        } else {
            Engine::Cpu
        };
        assert_eq!(Engine::preferred(), expected);
        assert_eq!(total_bytes(), expected.bytes());
    }

    /// Writes `file` at its pinned size (not its bytes: `installed` only
    /// looks at sizes).
    fn stand_in(dir: &Path, folder: &str, file: &ModelFile) {
        let target = dir.join(folder).join(file.name);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        File::create(&target).unwrap().set_len(file.bytes).unwrap();
    }

    #[test]
    fn the_build_on_disk_is_the_one_used_until_the_preferred_arrives() {
        let dir =
            std::env::temp_dir().join(format!("monobot-dictation-builds-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(installed_engine(&dir), None);
        for (folder, file) in Engine::Cpu.files() {
            stand_in(&dir, folder, file);
        }
        assert_eq!(installed_engine(&dir), Some(Engine::Cpu));
        for (folder, file) in Engine::Neural.files() {
            stand_in(&dir, folder, file);
        }
        assert_eq!(installed_engine(&dir), Some(Engine::preferred()));
        assert_eq!(
            disk_bytes(&dir),
            Engine::Cpu.bytes() + Engine::Neural.bytes() - silero().bytes,
            "Silero counts once"
        );
        // Once the Neural build is in, the CPU one's model goes; Silero stays.
        remove_cpu(&dir);
        assert!(!Engine::Cpu.installed(&dir));
        assert!(present(&dir, silero()));
        assert!(Engine::Neural.installed(&dir));
        assert_eq!(disk_bytes(&dir), Engine::Neural.bytes());
        // Removing leaves nothing, and removing again is fine.
        remove(&dir).unwrap();
        assert_eq!(installed_engine(&dir), None);
        assert_eq!(disk_bytes(&dir), 0);
        remove(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
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

    /// Answers one request on this machine with `reply`, as a site (or a
    /// proxy standing in for it) would, and returns the address to ask.
    /// Also returns the Range header each request asked for (empty without
    /// one); a server asked nothing sends nothing.
    fn serve(reply: &[u8]) -> (&'static str, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/tokens.txt", listener.local_addr().unwrap());
        let reply = reply.to_vec();
        let (asked, ranges) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut request = BufReader::new(stream);
            let mut line = String::new();
            let mut range = String::new();
            // The request's headers end with an empty line.
            while request.read_line(&mut line).unwrap() > 2 {
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("range") {
                        range = value.trim().to_owned();
                    }
                }
                line.clear();
            }
            let _ = asked.send(range);
            let mut stream = request.into_inner();
            stream.write_all(&reply).unwrap();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });
        (Box::leak(url.into_boxed_str()), ranges)
    }

    /// What one download left on disk.
    struct Fetched {
        result: Result<Vec<u8>, Error>,
        /// The `.part` left for the next try, if any.
        partial: Option<Vec<u8>>,
        /// The Range the request asked for.
        range: String,
        /// Every byte count `on_chunk` heard, summed.
        reported: u64,
    }

    /// Downloads SMALL's tokens.txt ("hello") from a server that answers
    /// with `reply`, with `before` already in its `.part`.
    fn fetch_after(tag: &str, before: Option<&[u8]>, reply: &[u8]) -> Fetched {
        let dir =
            std::env::temp_dir().join(format!("monobot-dictation-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (url, ranges) = serve(reply);
        let file = ModelFile {
            url: Some(url),
            ..SMALL[0]
        };
        let target = dir.join(file.name);
        if let Some(before) = before {
            fs::write(partial_of(&target), before).unwrap();
        }
        let mut reported = 0;
        let result = download(&file, &target, |chunk| reported += chunk)
            .map(|()| fs::read(&target).unwrap());
        // Nothing wrong lands.
        assert_eq!(target.exists(), result.is_ok());
        let partial = fs::read(partial_of(&target)).ok();
        fs::remove_dir_all(&dir).unwrap();
        Fetched {
            result,
            partial,
            range: ranges.try_recv().unwrap_or_default(),
            reported,
        }
    }

    /// As `fetch_after` with nothing on disk yet; nothing half-written stays
    /// behind unless the connection was cut.
    fn fetch(tag: &str, reply: &[u8]) -> Result<Vec<u8>, Error> {
        let fetched = fetch_after(tag, None, reply);
        assert!(fetched.partial.is_none(), "{tag} left a .part");
        fetched.result
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
        let got = fetch(
            "page",
            &reply("text/html; charset=utf-8", Some(page.len()), page),
        );
        assert!(matches!(got, Err(Error::Download(_))), "{got:?}");
        // A page is a page even at the file's length.
        let got = fetch("page5", &reply("TEXT/HTML", Some(5), "hello"));
        assert!(matches!(got, Err(Error::Download(_))), "{got:?}");
        let refused =
            b"HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: 2\r\n\r\nno";
        let got = fetch("refused", refused);
        assert!(matches!(got, Err(Error::Download(_))), "{got:?}");
    }

    #[test]
    fn another_length_is_a_failed_download() {
        let kind = "application/octet-stream";
        for (tag, length, body) in [("stated", Some(4), "hell"), ("long", None, "hello, world")] {
            let got = fetch(tag, &reply(kind, length, body));
            assert!(matches!(got, Err(Error::Download(_))), "{tag}: {got:?}");
        }
        // Cut short: what arrived is kept for the next try.
        let cut = fetch_after("short", None, &reply(kind, None, "hell"));
        assert!(matches!(cut.result, Err(Error::Download(_))));
        assert_eq!(cut.partial.as_deref(), Some(&b"hell"[..]));
    }

    fn partial_reply(from: usize, body: &str) -> Vec<u8> {
        let to = from + body.len() - 1;
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\n\
             Content-Range: bytes {from}-{to}/5\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn a_cut_download_resumes_from_where_it_stopped() {
        let resumed = fetch_after("resume", Some(b"hel"), &partial_reply(3, "lo"));
        assert_eq!(resumed.range, "bytes=3-");
        assert_eq!(resumed.result.unwrap(), b"hello");
        assert!(resumed.partial.is_none());
        // The bar heard the bytes already on disk, so it ends full.
        assert_eq!(resumed.reported, 5);
    }

    #[test]
    fn a_server_that_ignores_the_range_sends_the_whole_file_again() {
        let kind = "application/octet-stream";
        let fresh = fetch_after("norange", Some(b"hel"), &reply(kind, Some(5), "hello"));
        assert_eq!(fresh.range, "bytes=3-");
        assert_eq!(fresh.result.unwrap(), b"hello");
        assert_eq!(fresh.reported, 5);
    }

    #[test]
    fn a_resumed_file_still_has_to_match_its_checksum() {
        // What was on disk was wrong: the whole file fails and starts over.
        let bad = fetch_after("badresume", Some(b"HEL"), &partial_reply(3, "lo"));
        assert!(
            matches!(bad.result, Err(Error::Integrity(_))),
            "{:?}",
            bad.result
        );
        assert!(bad.partial.is_none());
        // A range from another place is not this download's rest.
        let elsewhere = fetch_after("elsewhere", Some(b"hel"), &partial_reply(2, "llo"));
        assert!(matches!(elsewhere.result, Err(Error::Download(_))));
        assert!(elsewhere.partial.is_none());
        // A whole .part only needs its checksum: nothing is asked.
        let whole = fetch_after("whole", Some(b"hello"), b"");
        assert_eq!(whole.result.unwrap(), b"hello");
        assert_eq!(whole.range, "");
        assert_eq!(whole.reported, 5);
    }

    #[test]
    fn the_right_length_with_other_bytes_fails_its_checksum() {
        let got = fetch(
            "tampered",
            &reply("application/octet-stream", Some(5), "hellO"),
        );
        assert!(matches!(got, Err(Error::Integrity(_))), "{got:?}");
    }

    /// Through this computer's own network, company proxy and all: silero
    /// comes from GitHub, and Hugging Face either sends tokens.txt or fails
    /// as a download, which turns install to GitHub's archive. Run it by
    /// hand: `cargo test -p monobot-dictation -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn downloads_through_this_network() {
        let dir =
            std::env::temp_dir().join(format!("monobot-dictation-net-{}", std::process::id()));
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
    /// behind a proxy that refuses Hugging
    /// Face, Parakeet comes from GitHub. The bar only moves forward and ends
    /// full. About 1.2 GB: `cargo test -p monobot-dictation -- --ignored
    /// --nocapture installs_through_this_network`.
    #[test]
    #[ignore]
    fn installs_through_this_network() {
        let dir =
            std::env::temp_dir().join(format!("monobot-dictation-full-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let started = std::time::Instant::now();
        let mut last = (0, 0);
        install_cpu(&dir, |done, total| {
            assert!(
                done >= last.0 && done <= total,
                "{done} after {} of {total}",
                last.0
            );
            last = (done, total);
        })
        .unwrap();
        eprintln!("installed in {:?}", started.elapsed());
        let bytes = Engine::Cpu.bytes();
        assert_eq!(last, (bytes, bytes));
        assert!(Engine::Cpu.installed(&dir));
        assert!(!dir.join(ARCHIVE.name).exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
