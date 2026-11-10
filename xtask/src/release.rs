//! Native preview releases: version rules, the signed update feed
//! (`latest.json`) and the update signing key.
//!
//! Keys and signatures use the format the app's update client verifies
//! (`monocode_core::app_update`): base64 of the minisign key or signature box
//! text. `cargo xtask verify-release <dir>` checks a release with that client.
use crate::util::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign::{KeyPair, SecretKey, SecretKeyBox};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub const NATIVE_TARGETS: [&str; 6] = [
    "darwin-aarch64-app",
    "darwin-x86_64-app",
    "windows-x86_64-nsis",
    "linux-x86_64-appimage",
    "linux-x86_64-deb",
    "linux-x86_64-rpm",
];

const KEY_COMMENT: &str = "MonoBOT update signing key";
const SIGNATURE_COMMENT: &str = "signature from MonoBOT update signing key";

/// A release version: `0.0.1`, or a preview such as `0.16.0-preview.2`.
#[derive(Debug, PartialEq, Eq)]
pub struct ReleaseVersion {
    pub version: String,
    /// `0.16.0`
    pub base: String,
    /// `2` for `-preview.2`; none for a regular release.
    pub sequence: Option<String>,
    /// `0.16.0.2`, or `0.0.1.0` for a regular release: fits a Windows version
    /// resource.
    pub windows: String,
}

impl ReleaseVersion {
    /// The rpm `Release` field: `1` for a regular release, `0.preview.N` for a
    /// preview, so a preview sorts before the release it leads to.
    pub fn rpm_release(&self) -> String {
        match &self.sequence {
            Some(sequence) => format!("0.preview.{sequence}"),
            None => "1".into(),
        }
    }
}

pub fn release_version(version: &str) -> Result<ReleaseVersion> {
    let error = || "Expected a release version such as 0.0.1 or 0.16.0-preview.1".to_owned();
    let (base, sequence) = match version.split_once('-') {
        Some((base, rest)) => (base, Some(rest.strip_prefix("preview.").ok_or_else(error)?)),
        None => (version, None),
    };
    let mut parts: Vec<&str> = base.split('.').collect();
    if parts.len() != 3 {
        return Err(error());
    }
    parts.push(sequence.unwrap_or("0"));
    for part in &parts {
        let number = part.bytes().all(|b| b.is_ascii_digit())
            && !part.is_empty()
            && (*part == "0" || !part.starts_with('0'));
        if !number || part.len() > 5 || part.parse::<u32>().map_err(|_| error())? > 65535 {
            return Err(error());
        }
    }
    Ok(ReleaseVersion {
        version: version.into(),
        base: base.into(),
        sequence: sequence.map(str::to_owned),
        windows: parts.join("."),
    })
}

/// What `/download/<key>` serves: the installer a person downloads by hand.
pub const DOWNLOAD_KEYS: [&str; 6] = [
    "windows",
    "macos-arm64",
    "macos-x64",
    "linux-appimage",
    "linux-deb",
    "linux-rpm",
];

/// One package a platform build produced (`artifacts-<kind>-<arch>.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub filename: String,
    /// The updater target it serves, or none for download-only files (.dmg).
    pub target: Option<String>,
    /// The `/download/<key>` it answers, if people download it by hand.
    pub download: Option<String>,
    pub signature: Option<String>,
}

impl Artifact {
    pub fn to_json(&self) -> Value {
        json!({ "filename": self.filename, "target": self.target, "download": self.download })
    }

    fn from_json(value: &Value) -> Result<Self> {
        Ok(Artifact {
            filename: value["filename"]
                .as_str()
                .ok_or("Invalid artifact filename")?
                .into(),
            target: value["target"].as_str().map(str::to_owned),
            download: value["download"].as_str().map(str::to_owned),
            signature: None,
        })
    }
}

/// A bare file name: no folders, no backslashes.
pub fn plain_filename(name: &str) -> bool {
    Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name) && !name.contains('\\')
}

/// `encodeURIComponent`'s set: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )`.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// Whether a release must carry an updater target or a download key. A
/// release made without the Apple signing credentials
/// (`MONOBOT_RELEASE_WITHOUT_MACOS=1`) leaves the macOS ones out.
fn required(name: &str, without_macos: bool) -> bool {
    !(without_macos && (name.starts_with("darwin-") || name.starts_with("macos-")))
}

pub fn native_manifest(
    version: &str,
    base_url: &str,
    artifacts: &[Artifact],
    notes: &str,
    without_macos: bool,
) -> Result<Value> {
    release_version(version)?;
    let insecure =
        || "The native download base must be an HTTPS URL without credentials, query or fragment";
    let base = url::Url::parse(base_url).map_err(|_| insecure())?;
    if base.scheme() != "https"
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(insecure().into());
    }
    let base = base.as_str().trim_end_matches('/').to_owned();
    let mut platforms = serde_json::Map::new();
    for artifact in artifacts {
        if !plain_filename(&artifact.filename) {
            return Err("An artifact must be a plain filename".into());
        }
        let Some(target) = &artifact.target else {
            continue;
        };
        if !NATIVE_TARGETS.contains(&target.as_str()) || platforms.contains_key(target) {
            return Err(format!("Duplicate or unexpected native target: {target}"));
        }
        let signature = artifact
            .signature
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("Missing signature for {target}"))?;
        platforms.insert(
            target.clone(),
            json!({
                "url": format!(
                    "{base}/releases/{version}/{}",
                    utf8_percent_encode(&artifact.filename, URI_COMPONENT)
                ),
                "signature": signature,
            }),
        );
    }
    for target in NATIVE_TARGETS {
        if required(target, without_macos) && !platforms.contains_key(target) {
            return Err(format!("Missing native package: {target}"));
        }
    }
    Ok(json!({
        "app_id": "dev.usemono.gpui",
        "channel": "native-preview",
        "version": version,
        "notes": notes,
        "platforms": platforms,
    }))
}

fn password(env: &HashMap<String, String>) -> String {
    env.get("MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD")
        .cloned()
        .unwrap_or_default()
}

fn write_private(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Err(format!(
            "{} already exists; refusing to overwrite a key",
            path.display()
        ));
    }
    fs::write(path, contents).map_err(|e| e.to_string())?;
    crate::util::set_mode(path, 0o600)
}

/// Writes `monobot-updater.key` and `monobot-updater.key.pub` into `dir`.
pub fn generate_keys(dir: &Path, env: &HashMap<String, String>) -> Result<()> {
    let pair =
        KeyPair::generate_encrypted_keypair(Some(password(env))).map_err(|e| e.to_string())?;
    let secret = pair
        .sk
        .to_box(Some(KEY_COMMENT))
        .map_err(|e| e.to_string())?;
    let public = pair.pk.to_box().map_err(|e| e.to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    write_private(
        &dir.join("monobot-updater.key"),
        &STANDARD.encode(secret.into_string()),
    )?;
    fs::write(
        dir.join("monobot-updater.key.pub"),
        STANDARD.encode(public.into_string()),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Wrote monobot-updater.key and monobot-updater.key.pub in {}",
        dir.display()
    );
    Ok(())
}

/// Signs files with the release key from the environment.
pub struct Signer {
    secret: SecretKey,
    timestamp: u64,
}

impl Signer {
    pub fn from_env(env: &HashMap<String, String>) -> Result<Self> {
        let encoded = env
            .get("MONOBOT_UPDATER_PRIVATE_KEY")
            .ok_or("Missing MONOBOT_UPDATER_PRIVATE_KEY")?;
        let text = String::from_utf8(
            STANDARD
                .decode(encoded.trim())
                .map_err(|_| "Invalid private key encoding")?,
        )
        .map_err(|_| "Invalid private key encoding")?;
        let secret = SecretKeyBox::from_string(&text)
            .and_then(|key| key.into_secret_key(Some(password(env))))
            .map_err(|_| "Could not open the private key (wrong key or password)")?;
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        Ok(Signer { secret, timestamp })
    }

    /// Writes `<file>.sig` and returns its contents.
    pub fn sign(&self, path: &Path) -> Result<String> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid file name")?;
        let data = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let trusted = format!("timestamp:{}\tfile:{name}", self.timestamp);
        let signature = minisign::sign(
            None,
            &self.secret,
            data,
            Some(&trusted),
            Some(SIGNATURE_COMMENT),
        )
        .map_err(|e| format!("Could not sign {name}: {e}"))?;
        let encoded = STANDARD.encode(signature.into_string());
        let mut sig = path.as_os_str().to_owned();
        sig.push(".sig");
        fs::write(PathBuf::from(sig), &encoded).map_err(|e| e.to_string())?;
        Ok(encoded)
    }
}

/// The installers people download by hand, by `/download/<key>`:
/// `{ "version": …, "files": { "windows": "MonoBOT_…_x64-setup.exe", … } }`.
/// Every key in `DOWNLOAD_KEYS` must have exactly one file.
pub fn downloads_manifest(
    version: &str,
    artifacts: &[Artifact],
    without_macos: bool,
) -> Result<Value> {
    release_version(version)?;
    let mut files = serde_json::Map::new();
    for artifact in artifacts {
        let Some(key) = &artifact.download else {
            continue;
        };
        if !DOWNLOAD_KEYS.contains(&key.as_str()) || files.contains_key(key) {
            return Err(format!("Duplicate or unexpected download: {key}"));
        }
        if !plain_filename(&artifact.filename) {
            return Err("An artifact must be a plain filename".into());
        }
        files.insert(key.clone(), artifact.filename.clone().into());
    }
    for key in DOWNLOAD_KEYS {
        if required(key, without_macos) && !files.contains_key(key) {
            return Err(format!("Missing download: {key}"));
        }
    }
    Ok(json!({ "version": version, "files": files }))
}

/// Signs every updater package listed in `artifacts-*.json` in `directory`,
/// then writes the signed feed (`latest.json`), the manual downloads
/// (`downloads.json`) and the list of files to publish under
/// `releases/<version>/` (`release-files.json`). Returns that list.
pub fn prepare_release(
    directory: &Path,
    env: &HashMap<String, String>,
    root: &Path,
) -> Result<Vec<String>> {
    let version = env
        .get("MONOCODE_NATIVE_VERSION")
        .map(String::as_str)
        .unwrap_or_default();
    release_version(version)?;
    if !env.contains_key("MONOBOT_UPDATER_PRIVATE_KEY")
        || !env.contains_key("MONOBOT_UPDATER_PUBKEY")
    {
        return Err("The MonoBOT update signing key and public key are required".into());
    }
    let signer = Signer::from_env(env)?;
    let mut metadata: Vec<PathBuf> = fs::read_dir(directory)
        .map_err(|e| format!("{}: {e}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("artifacts-") && n.ends_with(".json"))
        })
        .collect();
    metadata.sort();
    let mut records = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for path in metadata {
        let list: Vec<Value> = serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        for value in &list {
            let mut record = Artifact::from_json(value)?;
            if !plain_filename(&record.filename) {
                return Err("Invalid artifact filename".into());
            }
            if files.contains(&record.filename) {
                return Err(format!("Duplicate native artifact: {}", record.filename));
            }
            files.push(record.filename.clone());
            if record.target.is_some() {
                record.signature = Some(signer.sign(&directory.join(&record.filename))?);
                files.push(format!("{}.sig", record.filename));
            }
            records.push(record);
        }
    }
    let notes = match env.get("NATIVE_RELEASE_NOTES").filter(|n| !n.is_empty()) {
        Some(notes) => notes.clone(),
        None => fs::read_to_string(root.join("docs/done/release-notes.md"))
            .map_err(|e| format!("docs/done/release-notes.md: {e}"))?,
    };
    // The update Worker (deploy/cloudflare-updates) that serves the bucket.
    let base_url = env
        .get("MONOBOT_UPDATES_URL")
        .map(String::as_str)
        .unwrap_or_default();
    let without_macos = env
        .get("MONOBOT_RELEASE_WITHOUT_MACOS")
        .is_some_and(|v| v == "1");
    let manifest = native_manifest(version, base_url, &records, &notes, without_macos)?;
    let downloads = downloads_manifest(version, &records, without_macos)?;
    let payload = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    let payload_path = directory.join("native-manifest.payload");
    fs::write(&payload_path, &payload).map_err(|e| e.to_string())?;
    let signature = signer.sign(&payload_path)?;
    let feed = json!({ "payload": STANDARD.encode(&payload), "signature": signature });
    fs::write(directory.join("latest.json"), feed.to_string()).map_err(|e| e.to_string())?;
    fs::write(directory.join("downloads.json"), downloads.to_string())
        .map_err(|e| e.to_string())?;
    fs::write(
        directory.join("release-files.json"),
        serde_json::to_string(&files).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifacts() -> Vec<Artifact> {
        NATIVE_TARGETS
            .iter()
            .map(|target| Artifact {
                target: Some(target.to_string()),
                filename: format!("{target} with spaces.bin"),
                download: None,
                signature: Some("signed-package".into()),
            })
            .collect()
    }

    fn downloads() -> Vec<Artifact> {
        DOWNLOAD_KEYS
            .iter()
            .map(|key| Artifact {
                target: None,
                filename: format!("{key}.installer"),
                download: Some(key.to_string()),
                signature: None,
            })
            .collect()
    }

    fn manifest(
        version: Option<&str>,
        base: Option<&str>,
        artifacts: Option<Vec<Artifact>>,
    ) -> Result<Value> {
        native_manifest(
            version.unwrap_or("0.16.0-preview.2"),
            base.unwrap_or("https://dl.example.test/"),
            &artifacts.unwrap_or_else(self::artifacts),
            "",
            false,
        )
    }

    #[test]
    fn all_native_architectures_and_installation_types_have_isolated_versioned_urls() {
        let result = manifest(None, None, None).unwrap();
        assert_eq!(result["app_id"], "dev.usemono.gpui");
        assert_eq!(result["channel"], "native-preview");
        let platforms = result["platforms"].as_object().unwrap();
        let mut keys: Vec<&str> = platforms.keys().map(String::as_str).collect();
        let mut expected = NATIVE_TARGETS.to_vec();
        keys.sort();
        expected.sort();
        assert_eq!(keys, expected);
        for artifact in platforms.values() {
            let url = artifact["url"].as_str().unwrap();
            assert!(
                url.starts_with("https://dl.example.test/releases/0.16.0-preview.2/"),
                "{url}"
            );
            assert!(url.contains("%20"), "{url}");
        }
    }

    #[test]
    fn a_missing_intel_mac_or_linux_package_prevents_publishing_the_feed() {
        for target in NATIVE_TARGETS {
            let without = artifacts()
                .into_iter()
                .filter(|a| a.target.as_deref() != Some(target))
                .collect();
            let error = manifest(None, None, Some(without)).unwrap_err();
            assert!(error.contains("Missing native package"), "{error}");
        }
    }

    #[test]
    fn duplicate_targets_unsigned_packages_and_path_traversal_are_refused() {
        let mut duplicate = artifacts();
        duplicate.push(duplicate[0].clone());
        assert!(manifest(None, None, Some(duplicate))
            .unwrap_err()
            .contains("Duplicate"));
        let mut unsigned = artifacts();
        unsigned[0].signature = Some(String::new());
        assert!(manifest(None, None, Some(unsigned))
            .unwrap_err()
            .contains("Missing signature"));
        for filename in ["../latest.json", "directory\\latest.json"] {
            let mut traversal = artifacts();
            traversal[0].filename = filename.into();
            assert!(manifest(None, None, Some(traversal))
                .unwrap_err()
                .contains("plain filename"));
        }
    }

    #[test]
    fn every_manual_download_is_listed_once() {
        let listed = downloads_manifest("0.0.1", &downloads(), false).unwrap();
        assert_eq!(listed["version"], "0.0.1");
        assert_eq!(listed["files"]["windows"], "windows.installer");
        assert_eq!(
            listed["files"].as_object().unwrap().len(),
            DOWNLOAD_KEYS.len()
        );
        let mut missing = downloads();
        missing.pop();
        assert!(downloads_manifest("0.0.1", &missing, false)
            .unwrap_err()
            .contains("Missing download"));
        let mut duplicate = downloads();
        duplicate.push(duplicate[0].clone());
        assert!(downloads_manifest("0.0.1", &duplicate, false)
            .unwrap_err()
            .contains("Duplicate"));
    }

    #[test]
    fn a_release_without_the_apple_credentials_leaves_only_macos_out() {
        let without_mac: Vec<Artifact> = artifacts()
            .into_iter()
            .filter(|a| !a.target.as_deref().unwrap().starts_with("darwin-"))
            .collect();
        assert!(
            native_manifest("0.0.1", "https://dl.example.test", &without_mac, "", false)
                .unwrap_err()
                .contains("Missing native package")
        );
        let feed =
            native_manifest("0.0.1", "https://dl.example.test", &without_mac, "", true).unwrap();
        assert_eq!(feed["platforms"].as_object().unwrap().len(), 4);
        let mut no_windows = without_mac.clone();
        no_windows.retain(|a| a.target.as_deref() != Some("windows-x86_64-nsis"));
        assert!(
            native_manifest("0.0.1", "https://dl.example.test", &no_windows, "", true)
                .unwrap_err()
                .contains("windows")
        );
        let downloads: Vec<Artifact> = self::downloads()
            .into_iter()
            .filter(|a| !a.download.as_deref().unwrap().starts_with("macos-"))
            .collect();
        assert!(downloads_manifest("0.0.1", &downloads, false).is_err());
        let listed = downloads_manifest("0.0.1", &downloads, true).unwrap();
        assert_eq!(listed["files"].as_object().unwrap().len(), 4);
    }

    #[test]
    fn regular_versions_are_releases() {
        let parsed = release_version("0.0.1").unwrap();
        assert_eq!(parsed.base, "0.0.1");
        assert_eq!(parsed.sequence, None);
        assert_eq!(parsed.windows, "0.0.1.0");
        assert_eq!(parsed.rpm_release(), "1");
        assert_eq!(
            release_version("0.0.1-preview.3").unwrap().rpm_release(),
            "0.preview.3"
        );
        manifest(Some("0.0.1"), None, None).unwrap();
    }

    #[test]
    fn tags_and_insecure_download_bases_cannot_create_native_releases() {
        for version in [
            "v0.16.0",
            "0.16",
            "0.16.0-",
            "native-v0.16.0-preview.1",
            "0.16.0-beta.1",
            "0.16.0-preview.1/../latest.json",
        ] {
            let error = manifest(Some(version), None, None).unwrap_err();
            assert!(error.contains("release version"), "{version}: {error}");
        }
        for base in [
            "http://dl.example.test",
            "https://user:secret@dl.example.test",
            "https://dl.example.test?channel=stable",
        ] {
            let error = manifest(None, Some(base), None).unwrap_err();
            assert!(error.contains("HTTPS URL"), "{base}: {error}");
        }
    }

    #[test]
    fn preview_sequence_fits_native_windows_version_resources() {
        assert_eq!(
            release_version("0.16.0-preview.12").unwrap().windows,
            "0.16.0.12"
        );
        assert!(release_version("0.16.0-preview.65536").is_err());
        assert!(release_version("0.16.0-preview.01").is_err());
        let parsed = release_version("1.2.3-preview.4").unwrap();
        assert_eq!(
            (parsed.base.as_str(), parsed.sequence.as_deref()),
            ("1.2.3", Some("4"))
        );
    }

    /// Signs fake packages with a throwaway key; the signatures verify against
    /// the public key and the feed lists every file to publish.
    #[test]
    fn release_signs_every_package_and_the_feed() {
        let dir = tempfile::tempdir().unwrap();
        let keys = dir.path().join("keys");
        let mut env = HashMap::from([(
            "MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD".to_owned(),
            "fixture".to_owned(),
        )]);
        generate_keys(&keys, &env).unwrap();
        assert!(generate_keys(&keys, &env)
            .unwrap_err()
            .contains("refusing to overwrite"));
        let private = fs::read_to_string(keys.join("monobot-updater.key")).unwrap();
        let public = fs::read_to_string(keys.join("monobot-updater.key.pub")).unwrap();
        let dist = dir.path().join("dist");
        fs::create_dir(&dist).unwrap();
        let mut list = Vec::new();
        for target in NATIVE_TARGETS {
            let name = format!("{target} package.bin");
            fs::write(dist.join(&name), target).unwrap();
            list.push(json!({ "filename": name, "target": target }));
        }
        for key in DOWNLOAD_KEYS {
            let name = format!("download {key}.installer");
            fs::write(dist.join(&name), key).unwrap();
            list.push(json!({ "filename": name, "target": null, "download": key }));
        }
        fs::write(
            dist.join("artifacts-all-x86_64.json"),
            Value::from(list).to_string(),
        )
        .unwrap();
        env.extend([
            ("MONOCODE_NATIVE_VERSION".into(), "0.16.0-preview.2".into()),
            ("MONOBOT_UPDATER_PRIVATE_KEY".into(), private),
            ("MONOBOT_UPDATER_PUBKEY".into(), public.clone()),
            (
                "MONOBOT_UPDATES_URL".into(),
                "https://dl.example.test".into(),
            ),
            ("NATIVE_RELEASE_NOTES".into(), "Fixture notes".into()),
        ]);
        let files = prepare_release(&dist, &env, &crate::util::root()).unwrap();
        assert_eq!(files.len(), 12 + DOWNLOAD_KEYS.len(), "{files:?}");
        assert!(files.contains(&"download windows.installer".to_owned()));
        assert!(!files.contains(&"download windows.installer.sig".to_owned()));
        let downloads: Value =
            serde_json::from_str(&fs::read_to_string(dist.join("downloads.json")).unwrap())
                .unwrap();
        assert_eq!(
            downloads["files"]["linux-rpm"],
            "download linux-rpm.installer"
        );
        let listed: Vec<String> =
            serde_json::from_str(&fs::read_to_string(dist.join("release-files.json")).unwrap())
                .unwrap();
        assert_eq!(listed, files);

        let key_text = String::from_utf8(STANDARD.decode(public.trim()).unwrap()).unwrap();
        let key = minisign::PublicKeyBox::from_string(&key_text)
            .unwrap()
            .into_public_key()
            .unwrap();
        let verify = |data: &[u8], sig: &str| {
            let text = String::from_utf8(STANDARD.decode(sig.trim()).unwrap()).unwrap();
            let signature = minisign::SignatureBox::from_string(&text).unwrap();
            minisign::verify(
                &key,
                &signature,
                std::io::Cursor::new(data),
                true,
                false,
                false,
            )
        };
        let feed: Value =
            serde_json::from_str(&fs::read_to_string(dist.join("latest.json")).unwrap()).unwrap();
        let payload = STANDARD.decode(feed["payload"].as_str().unwrap()).unwrap();
        verify(&payload, feed["signature"].as_str().unwrap()).unwrap();
        assert!(verify(b"tampered", feed["signature"].as_str().unwrap()).is_err());
        let manifest: Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(manifest["notes"], "Fixture notes");
        for target in NATIVE_TARGETS {
            let platform = &manifest["platforms"][target];
            let name = format!("{target} package.bin");
            verify(target.as_bytes(), platform["signature"].as_str().unwrap()).unwrap();
            assert_eq!(
                fs::read_to_string(dist.join(format!("{name}.sig"))).unwrap(),
                platform["signature"].as_str().unwrap()
            );
        }

        env.insert(
            "MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD".into(),
            "wrong".into(),
        );
        assert!(prepare_release(&dist, &env, &crate::util::root())
            .unwrap_err()
            .contains("wrong key or password"));
    }
}
