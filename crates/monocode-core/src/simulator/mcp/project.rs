//! Whether a folder is an iOS project, so the app offers the live Simulator
//! only where it is of use. It counts when, a few folders down at most, it
//! has an Xcode project or workspace, a Swift package, or an Expo / React
//! Native app that builds for iOS (an `ios/` folder, or an `app.json` whose
//! Expo platforms include iOS).
//!
//! It only lists folders and reads at most a few small `app.json` and
//! `package.json` files, and it gives up after a fixed number of entries, so
//! a home folder or a huge monorepo costs a few milliseconds. It still
//! blocks: the app runs it off the interface thread.
use serde_json::Value;
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};

/// Folders below the project the search goes into (the same depth
/// `find_projects` uses): `apps/mobile/ios/App.xcodeproj` is found.
const DEPTH: usize = 3;
/// Entries looked at before the answer is no.
const BUDGET: usize = 5000;
/// `app.json` and `package.json` larger than this are not read.
const MANIFEST_LIMIT: u64 = 256 * 1024;
/// Folders no app lives in, or that hold thousands of files. Hidden folders
/// (`.git`, `.build`, `.expo`…) are skipped as well.
const SKIPPED: [&str; 8] = [
    "node_modules",
    "target",
    "Pods",
    "build",
    "DerivedData",
    "vendor",
    "Carthage",
    "dist",
];

/// Whether `root` holds an iOS or Swift project. A missing or unreadable
/// folder is not one.
pub fn is_ios_project(root: &Path) -> bool {
    let mut queue = VecDeque::from([(root.to_path_buf(), 0)]);
    let mut seen = 0;
    while let Some((folder, depth)) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut folders = Vec::new();
        let (mut package_json, mut ios_folder, mut app_json, mut app_config) =
            (false, false, false, false);
        for entry in entries.flatten() {
            seen += 1;
            if seen > BUDGET {
                return false;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            // Not followed: a link can lead anywhere, even back up.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if name.ends_with(".xcodeproj") || name.ends_with(".xcworkspace") {
                    return true;
                }
                if name == "ios" {
                    ios_folder = true;
                }
                if depth < DEPTH && !name.starts_with('.') && !SKIPPED.contains(&name) {
                    folders.push(entry.path());
                }
            } else if kind.is_file() {
                match name {
                    "Package.swift" => return true,
                    "package.json" => package_json = true,
                    "app.json" => app_json = true,
                    "app.config.js" | "app.config.ts" => app_config = true,
                    _ => {}
                }
            }
        }
        if package_json && js_app_targets_ios(&folder, ios_folder, app_json, app_config) {
            return true;
        }
        folders.sort();
        queue.extend(folders.into_iter().map(|path| (path, depth + 1)));
    }
    false
}

/// A JavaScript app (it has a `package.json`) that builds for iOS: React
/// Native's or a prebuilt Expo app's `ios/` folder, or an Expo config.
fn js_app_targets_ios(folder: &Path, ios_folder: bool, app_json: bool, app_config: bool) -> bool {
    if ios_folder {
        return true;
    }
    if app_json && read(&folder.join("app.json")).is_some_and(|text| expo_targets_ios(&text)) {
        return true;
    }
    // A config in code cannot be read; an Expo dependency builds for iOS
    // unless told otherwise.
    app_config
        && read(&folder.join("package.json")).is_some_and(|text| {
            serde_json::from_str::<Value>(&text).is_ok_and(|package| {
                ["dependencies", "devDependencies"]
                    .iter()
                    .any(|key| package[key].get("expo").is_some())
            })
        })
}

/// An `app.json` with an Expo config whose platforms include iOS. Expo
/// builds for iOS and Android when `platforms` is left out.
pub(crate) fn expo_targets_ios(app_json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(app_json) else {
        return false;
    };
    let Some(expo) = value.get("expo").and_then(Value::as_object) else {
        return false;
    };
    match expo.get("platforms").and_then(Value::as_array) {
        Some(platforms) => platforms.iter().any(|p| p.as_str() == Some("ios")),
        None => true,
    }
}

fn read(path: &PathBuf) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MANIFEST_LIMIT {
        return None;
    }
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(paths: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for path in paths {
            let path = dir.path().join(path);
            if path.to_string_lossy().ends_with('/') {
                fs::create_dir_all(&path).unwrap();
            } else {
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, "{}").unwrap();
            }
        }
        dir
    }

    fn write(dir: &tempfile::TempDir, path: &str, text: &str) {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn xcode_projects_workspaces_and_swift_packages_count() {
        assert!(is_ios_project(tree(&["App.xcodeproj/"]).path()));
        assert!(is_ios_project(tree(&["App.xcworkspace/"]).path()));
        assert!(is_ios_project(tree(&["Package.swift"]).path()));
        assert!(is_ios_project(
            tree(&["apps/mobile/ios/App.xcodeproj/"]).path()
        ));
        assert!(is_ios_project(tree(&["Sources/Kit/Package.swift"]).path()));
    }

    #[test]
    fn other_projects_do_not() {
        assert!(!is_ios_project(
            tree(&["Cargo.toml", "src/main.rs", "README.md"]).path()
        ));
        assert!(!is_ios_project(tree(&[]).path()));
        assert!(!is_ios_project(Path::new("/no/such/folder/anywhere")));
        // A web app with a package.json and nothing for iOS.
        assert!(!is_ios_project(
            tree(&["package.json", "src/index.ts"]).path()
        ));
        // A file named like a project is not one.
        assert!(!is_ios_project(tree(&["notes.xcodeproj"]).path()));
        // An ios folder without a JavaScript app beside it.
        assert!(!is_ios_project(tree(&["docs/ios/notes.md"]).path()));
    }

    #[test]
    fn heavy_and_hidden_folders_are_not_searched() {
        for skipped in [
            "node_modules/pkg/ios/Lib.xcodeproj/",
            "Pods/Pods.xcodeproj/",
            "target/debug/App.xcodeproj/",
            "build/App.xcodeproj/",
            "DerivedData/App.xcodeproj/",
            "vendor/sdk/Package.swift",
            ".git/App.xcodeproj/",
            ".build/checkouts/kit/Package.swift",
        ] {
            assert!(!is_ios_project(tree(&[skipped]).path()), "{skipped}");
        }
    }

    #[test]
    fn the_search_stays_shallow() {
        assert!(is_ios_project(tree(&["a/b/c/App.xcodeproj/"]).path()));
        assert!(!is_ios_project(tree(&["a/b/c/d/App.xcodeproj/"]).path()));
        assert!(!is_ios_project(tree(&["a/b/c/d/Package.swift"]).path()));
    }

    #[test]
    fn react_native_and_expo_apps_that_build_for_ios_count() {
        // React Native, or an Expo app after prebuild: the ios folder.
        assert!(is_ios_project(
            tree(&["mobile/package.json", "mobile/ios/"]).path()
        ));
        // Expo with its default platforms.
        let expo = tree(&["mobile/package.json"]);
        write(&expo, "mobile/app.json", r#"{"expo": {"name": "App"}}"#);
        assert!(is_ios_project(expo.path()));
        // Expo for Android and the web only.
        let android = tree(&["package.json"]);
        write(
            &android,
            "app.json",
            r#"{"expo": {"platforms": ["android", "web"]}}"#,
        );
        assert!(!is_ios_project(android.path()));
        // A config in code, with Expo among the dependencies.
        let config = tree(&["app.config.ts"]);
        write(
            &config,
            "package.json",
            r#"{"dependencies": {"expo": "~54.0.0"}}"#,
        );
        assert!(is_ios_project(config.path()));
        let plain = tree(&["app.config.ts"]);
        write(&plain, "package.json", r#"{"dependencies": {"vite": "7"}}"#);
        assert!(!is_ios_project(plain.path()));
        // An app.json that is not Expo's.
        let other = tree(&["package.json"]);
        write(&other, "app.json", r#"{"name": "server"}"#);
        assert!(!is_ios_project(other.path()));
    }

    #[test]
    fn expo_platforms() {
        assert!(expo_targets_ios(r#"{"expo": {}}"#));
        assert!(expo_targets_ios(r#"{"expo": {"platforms": ["ios"]}}"#));
        assert!(!expo_targets_ios(r#"{"expo": {"platforms": ["web"]}}"#));
        assert!(!expo_targets_ios(r#"{"name": "x"}"#));
        assert!(!expo_targets_ios("not json"));
    }

    #[test]
    fn a_huge_folder_gives_up() {
        let dir = tree(&[]);
        for i in 0..(BUDGET + 10) {
            fs::write(dir.path().join(format!("f{i}")), "").unwrap();
        }
        // Past the budget: the project inside a later folder is not reached.
        fs::create_dir_all(dir.path().join("zz/App.xcodeproj")).unwrap();
        assert!(!is_ios_project(dir.path()));
    }
}
