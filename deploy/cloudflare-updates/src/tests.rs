use super::*;

#[test]
fn routes_the_feed_downloads_and_published_files() {
    assert_eq!(route("GET", "/"), Route::Index);
    assert_eq!(route("GET", "/latest.json"), Route::Feed);
    assert_eq!(route("HEAD", "/latest.json"), Route::Feed);
    assert_eq!(route("GET", "/downloads.json"), Route::Downloads);
    assert_eq!(route("GET", "/download/"), Route::Downloads);
    assert_eq!(
        route("GET", "/download/windows"),
        Route::Download("windows".into())
    );
    assert_eq!(
        route("GET", "/releases/0.0.1/MonoBOT_0.0.1_x64-setup.exe"),
        Route::File {
            key: "releases/0.0.1/MonoBOT_0.0.1_x64-setup.exe".into()
        }
    );
    assert_eq!(
        route(
            "GET",
            "/releases/0.16.0-preview.2/monobot-0.16.0-0.preview.2.x86_64.rpm.sig"
        ),
        Route::File {
            key: "releases/0.16.0-preview.2/monobot-0.16.0-0.preview.2.x86_64.rpm.sig".into()
        }
    );
}

#[test]
fn only_reads_exist() {
    for method in ["POST", "PUT", "DELETE", "PATCH"] {
        assert_eq!(route(method, "/latest.json"), Route::MethodNotAllowed);
    }
}

#[test]
fn refuses_traversal_hidden_files_and_unknown_keys() {
    for path in [
        "/download/android",
        "/releases/../latest.json",
        "/releases/0.0.1/../../latest.json",
        "/releases/0.0.1/.env",
        "/releases/0.0.1/a%2Fb.exe",
        "/releases/0.0.1/a..b",
        "/releases/latest/MonoBOT.exe",
        "/releases/0.0.1",
        "/releases/0.0.1/sub/file.exe",
        "/raw/latest.json",
    ] {
        assert_eq!(route("GET", path), Route::NotFound, "{path}");
    }
}

#[test]
fn a_download_redirects_to_the_latest_installer() {
    let listed = br#"{"version":"0.0.1","files":{"windows":"MonoBOT_0.0.1_x64-setup.exe","macos-arm64":"MonoBOT_0.0.1_aarch64.dmg"}}"#;
    assert_eq!(
        download_path(listed, "windows").as_deref(),
        Some("/releases/0.0.1/MonoBOT_0.0.1_x64-setup.exe")
    );
    assert_eq!(download_path(listed, "linux-deb"), None);
    assert_eq!(download_path(b"not json", "windows"), None);
    let hostile = br#"{"version":"../x","files":{"windows":"a.exe"}}"#;
    assert_eq!(download_path(hostile, "windows"), None);
    let hostile = br#"{"version":"0.0.1","files":{"windows":"../latest.json"}}"#;
    assert_eq!(download_path(hostile, "windows"), None);
}

#[test]
fn installers_download_with_their_name_and_type() {
    assert_eq!(
        content_type("releases/0.0.1/MonoBOT_0.0.1_x64-setup.exe"),
        "application/vnd.microsoft.portable-executable"
    );
    assert_eq!(content_type("latest.json"), "application/json");
    assert_eq!(
        content_type("releases/0.0.1/MonoBOT_0.0.1_aarch64.app.tar.gz"),
        "application/gzip"
    );
    assert_eq!(
        disposition("releases/0.0.1/MonoBOT_0.0.1_amd64.AppImage").as_deref(),
        Some("attachment; filename=\"MonoBOT_0.0.1_amd64.AppImage\"")
    );
    assert_eq!(
        disposition("releases/0.0.1/MonoBOT_0.0.1_x64-setup.exe.sig"),
        None
    );
}

/// The Worker and the release tool must agree on the download keys.
#[test]
fn download_keys_match_the_release_tool() {
    let release = include_str!("../../../xtask/src/release.rs");
    for key in DOWNLOAD_KEYS {
        assert!(release.contains(&format!("\"{key}\"")), "{key}");
    }
}
