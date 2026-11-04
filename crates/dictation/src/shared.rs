//! Pinned model files other apps on this computer already downloaded. Echo,
//! whose dictation this is, keeps the same Parakeet and Silero files in its
//! voice folder, and sherpa-onnx models also land in Hugging Face's cache. A
//! file found there with the pinned name and size is taken in rather than
//! downloaded again, and only kept once its SHA-256 matches.
//!
//! The folders come from the environment of whoever runs the app, as each of
//! those apps finds its own; nothing here names one person's computer. Other
//! apps' files are only read, never moved or changed.
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::model::ModelFile;
use crate::Error;

/// A folder of another app's models.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// Who keeps it, for the install prompt: "Echo".
    pub app: &'static str,
    pub dir: PathBuf,
}

/// A pinned file found in another app's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub app: &'static str,
    pub path: PathBuf,
}

/// Echo's channels, by the data folder each keeps (Echo's `Profile::folder`).
const ECHO_FOLDERS: [&str; 3] = ["Echo", "Echo Dev", "Echo Nightly"];
/// Deep enough for Hugging Face's `models--org--name/snapshots/rev/file`.
const MAX_DEPTH: usize = 4;
/// Folders looked into per place, so a huge cache cannot stall the prompt.
const MAX_FOLDERS: usize = 4_000;

/// Where to look on this computer, for the user running the app.
pub fn places() -> Vec<Place> {
    places_from(&|key| std::env::var_os(key).filter(|value| !value.is_empty()))
}

/// The places, from environment variables read through `env`.
pub(crate) fn places_from(env: &dyn Fn(&str) -> Option<OsString>) -> Vec<Place> {
    let home = env(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let mut places = Vec::new();
    let mut add = |app: &'static str, dir: PathBuf| {
        if !places.iter().any(|place: &Place| place.dir == dir) {
            places.push(Place { app, dir });
        }
    };
    // Echo's own override wins there, so it is looked at first here too.
    if let Some(dir) = env("ECHO_DATA_DIR") {
        add("Echo", PathBuf::from(dir).join("voice"));
    }
    for folder in ECHO_FOLDERS {
        if let Some(dir) = echo_data_dir(env, home.as_deref(), folder) {
            add("Echo", dir.join("voice"));
        }
    }
    let hub = env("HF_HUB_CACHE")
        .map(PathBuf::from)
        .or_else(|| env("HF_HOME").map(|dir| PathBuf::from(dir).join("hub")))
        .or_else(|| {
            home.as_ref()
                .map(|home| home.join(".cache").join("huggingface").join("hub"))
        });
    if let Some(hub) = hub {
        add("Hugging Face", hub);
    }
    places
}

/// Echo's data folder for one channel, as Echo's `paths::data_dir_for` finds it.
fn echo_data_dir(
    env: &dyn Fn(&str) -> Option<OsString>,
    home: Option<&Path>,
    folder: &str,
) -> Option<PathBuf> {
    if cfg!(windows) {
        env("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join(folder))
    } else if cfg!(target_os = "macos") {
        home.map(|home| home.join("Library/Application Support").join(folder))
    } else {
        home.map(|home| {
            home.join(".local/share")
                .join(folder.to_lowercase().replace(' ', "-"))
        })
    }
}

/// The first copy of `file` in `places` with its pinned name and size. The
/// checksum is checked when it is taken in, not here, so asking is cheap.
pub fn find(file: &ModelFile, places: &[Place]) -> Option<Found> {
    places.iter().find_map(|place| {
        search(&place.dir, file).map(|path| Found {
            app: place.app,
            path,
        })
    })
}

fn search(root: &Path, file: &ModelFile) -> Option<PathBuf> {
    let mut folders = vec![(root.to_path_buf(), 0)];
    let mut seen = 0;
    while let Some((dir, depth)) = folders.pop() {
        seen += 1;
        if seen > MAX_FOLDERS {
            return None;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    folders.push((path, depth + 1));
                }
            } else if entry.file_name() == file.name
                // A snapshot may be a link to the blob: its size is the blob's.
                && fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() == file.bytes)
            {
                return Some(path);
            }
        }
    }
    None
}

/// Takes in another app's copy of `file` as `target`: a hard link when both
/// are on one disk (no space used, and it outlives the other app deleting
/// its own), else a copy. The result is hashed before it is kept, reporting
/// each chunk read; a mismatch leaves nothing behind.
pub fn adopt(
    source: &Path,
    file: &ModelFile,
    target: &Path,
    mut on_chunk: impl FnMut(u64),
) -> Result<(), Error> {
    let source = fs::canonicalize(source)?;
    let partial = target.with_extension("part");
    let _ = fs::remove_file(&partial);
    if fs::hard_link(&source, &partial).is_err() {
        fs::copy(&source, &partial)?;
    }
    match sha256(&partial, &mut on_chunk) {
        Ok(hex) if hex == file.sha256 => {
            fs::rename(&partial, target)?;
            Ok(())
        }
        Ok(_) => {
            let _ = fs::remove_file(&partial);
            Err(Error::Integrity(file.name.into()))
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error.into())
        }
    }
}

fn sha256(path: &Path, on_chunk: &mut impl FnMut(u64)) -> std::io::Result<String> {
    let mut reader = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        on_chunk(read as u64);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// sha256("hello"), so a five-byte file can stand in for a model.
    const HELLO: ModelFile = ModelFile {
        name: "tokens.txt",
        bytes: 5,
        sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        url: None,
    };

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("monobot-shared-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(key, value)| (key.to_string(), OsString::from(value)))
            .collect();
        move |key| vars.get(key).cloned()
    }

    #[test]
    fn looks_in_every_echo_channel_and_the_hugging_face_cache() {
        let places = places_from(&env(&[
            ("LOCALAPPDATA", "/local"),
            ("USERPROFILE", "/home/ana"),
            ("HOME", "/home/ana"),
            ("ECHO_DATA_DIR", "/custom/echo"),
            ("HF_HOME", "/hf"),
        ]));
        let dirs: Vec<_> = places
            .iter()
            .map(|place| (place.app, place.dir.clone()))
            .collect();
        assert_eq!(
            dirs[0],
            ("Echo", PathBuf::from("/custom/echo").join("voice"))
        );
        let echo = |folder: &str| {
            if cfg!(windows) {
                PathBuf::from("/local").join(folder).join("voice")
            } else if cfg!(target_os = "macos") {
                PathBuf::from("/home/ana/Library/Application Support")
                    .join(folder)
                    .join("voice")
            } else {
                PathBuf::from("/home/ana/.local/share")
                    .join(folder.to_lowercase().replace(' ', "-"))
                    .join("voice")
            }
        };
        for folder in ECHO_FOLDERS {
            assert!(dirs.contains(&("Echo", echo(folder))), "{folder}: {dirs:?}");
        }
        assert_eq!(
            dirs.last().unwrap(),
            &("Hugging Face", PathBuf::from("/hf").join("hub"))
        );
        // Without HF_HOME the cache is the default one in the user's home.
        let places = places_from(&env(&[("USERPROFILE", "/home/ana"), ("HOME", "/home/ana")]));
        assert_eq!(
            places.last().unwrap().dir,
            PathBuf::from("/home/ana")
                .join(".cache")
                .join("huggingface")
                .join("hub")
        );
    }

    #[test]
    fn finds_a_file_by_name_and_size_in_any_subfolder() {
        let root = temp_dir("find");
        let echo = root.join("Echo Dev").join("voice");
        fs::create_dir_all(echo.join("whisper-turbo-int8")).unwrap();
        fs::create_dir_all(echo.join("parakeet-v3-int8")).unwrap();
        // The wrong size is not the pinned file, wherever it is.
        fs::write(
            echo.join("whisper-turbo-int8").join("tokens.txt"),
            b"too long",
        )
        .unwrap();
        fs::write(echo.join("parakeet-v3-int8").join("tokens.txt"), b"hello").unwrap();
        let places = [
            Place {
                app: "Echo",
                dir: root.join("missing"),
            },
            Place {
                app: "Echo",
                dir: echo.clone(),
            },
        ];
        assert_eq!(
            find(&HELLO, &places),
            Some(Found {
                app: "Echo",
                path: echo.join("parakeet-v3-int8").join("tokens.txt"),
            })
        );
        assert_eq!(find(&HELLO, &places[..1]), None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn takes_in_a_matching_copy_and_leaves_the_original() {
        let root = temp_dir("adopt");
        let source = root.join("echo-tokens.txt");
        fs::write(&source, b"hello").unwrap();
        let target = root.join("tokens.txt");
        let mut read = 0;
        adopt(&source, &HELLO, &target, |chunk| read += chunk).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"hello");
        assert_eq!(read, 5);
        assert_eq!(fs::read(&source).unwrap(), b"hello");
        // The other app deleting its copy does not take ours with it.
        fs::remove_file(&source).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"hello");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn refuses_a_copy_whose_checksum_differs() {
        let root = temp_dir("mismatch");
        let source = root.join("echo-tokens.txt");
        fs::write(&source, b"jello").unwrap();
        let target = root.join("tokens.txt");
        let error = adopt(&source, &HELLO, &target, |_| {}).unwrap_err();
        assert!(matches!(error, Error::Integrity(_)));
        assert!(!target.exists());
        assert!(!root.join("tokens.part").exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
