//! `cargo xtask build`: the optimized app, and its `.app` on macOS.
use crate::{
    bundle::{bundle_macos, Bundle, DataMode},
    util::{self, metadata_args, parse_metadata, target_dir, Platform, Result, Tool, APP_MANIFEST},
};
use std::path::Path;

/// cargo with the sherpa libraries found and, unless set, one build job.
pub fn cargo(root: &Path) -> Tool {
    let mut tool = Tool::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())).cwd(root);
    for (key, value) in crate::sherpa::cargo_env(root) {
        tool = tool.env(&key, value);
    }
    tool
}

fn with_jobs(tool: Tool) -> Tool {
    if std::env::var("CARGO_BUILD_JOBS").map_or(true, |jobs| jobs.is_empty()) {
        tool.env("CARGO_BUILD_JOBS", "1")
    } else {
        tool
    }
}

pub fn main() -> Result<()> {
    let root = util::root();
    let metadata = util::run_system(&with_jobs(cargo(&root)).args(metadata_args()).capture())?;
    let metadata = parse_metadata(&metadata)?;
    util::run_system(&with_jobs(cargo(&root)).args([
        "build",
        "--manifest-path",
        APP_MANIFEST,
        "--locked",
        "-p",
        util::APP_PACKAGE,
        "--release",
    ]))?;
    let target = target_dir(
        &metadata,
        std::env::var("CARGO_BUILD_TARGET").ok().as_deref(),
    );
    let release = target.join("release");
    if Platform::current() == Platform::Mac {
        let identity = std::env::var("MONOCODE_NATIVE_SIGNING_IDENTITY").ok();
        bundle_macos(
            &Bundle {
                root: &root,
                profile_dir: &release,
                version: &metadata.version,
                data_mode: DataMode::MonocodeData,
                identity: identity.as_deref(),
            },
            &util::System,
        )?;
    } else {
        println!(
            "Executable: {}",
            release.join(Platform::current().executable()).display()
        );
    }
    Ok(())
}
