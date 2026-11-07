use std::{env, fs, path::PathBuf};

mod dependency_guard;

fn main() {
    for name in [
        "MONOCODE_NATIVE_VERSION",
        "MONOCODE_NATIVE_UPDATER_ENDPOINT",
        "MONOCODE_NATIVE_UPDATER_PUBKEY",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let version = env::var("MONOCODE_NATIVE_VERSION")
        .unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=MONOCODE_NATIVE_VERSION={version}");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon(&manifest_dir);
    }
    dependency_guard::check(&manifest_dir);
}

/// GPUI loads the window and taskbar icon from resource ID 1 of the executable,
/// which Explorer and the installer shortcuts also show.
fn embed_windows_icon(manifest_dir: &std::path::Path) {
    let icon = manifest_dir.join("../../assets/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    assert!(
        icon.is_file(),
        "The Windows build requires assets/icons/icon.ico"
    );
    // Not canonicalize: its \\?\ verbatim prefix is rejected by rc.exe.
    let icon = icon.to_string_lossy().replace('\\', "/");
    let rc = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("monocode.rc");
    // Task Manager and Explorer name the process by its FileDescription.
    let version = env::var("MONOCODE_NATIVE_VERSION")
        .unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").unwrap());
    let numbers: Vec<u16> = version
        .split(['.', '-'])
        .take(3)
        .map(|part| part.parse().unwrap_or(0))
        .chain([0, 0, 0])
        .take(3)
        .collect();
    let comma = format!("{},{},{},0", numbers[0], numbers[1], numbers[2]);
    let resource = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {comma}
PRODUCTVERSION {comma}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "monoBOT"
      VALUE "ProductName", "monoBOT"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "InternalName", "monocode-gpui"
      VALUE "OriginalFilename", "monocode-gpui.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    fs::write(&rc, resource).unwrap();
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
