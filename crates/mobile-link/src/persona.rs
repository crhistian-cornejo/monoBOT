//! A personal bot's 3D face on the phone: the same picture the desktop draws
//! (`monobot_persona`), rendered here and kept as a PNG in a folder the phone
//! names, where its image views load it from.

use crate::LinkResult;
use monobot_persona::{Look, Pose, MAX_SIDE, REVISION};
use std::{
    fs,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Once,
    },
};

/// The file in `directory` holding `face` (`3d:<mascot>:<role>`, as the
/// desktop sends it) in `color` (`#rrggbb`) and `pose`, `side` pixels square.
/// It is rendered unless an earlier call left it there.
pub(crate) fn picture(
    face: &str,
    color: &str,
    pose: &str,
    side: u32,
    portrait: bool,
    directory: &str,
) -> LinkResult<String> {
    let (species, outfit) = monobot_persona::parse(face).ok_or("Not a bot's face")?;
    let color = color
        .strip_prefix('#')
        .filter(|hex| hex.len() == 6)
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .ok_or("Invalid colour")?;
    let pose = match pose {
        "rest" => Pose::Rest,
        "blink" => Pose::Blink,
        "talk" => Pose::Talk,
        _ => return Err("Unknown pose".into()),
    };
    let side = side.clamp(8, MAX_SIDE);
    let look = Look {
        species,
        outfit,
        color,
    };
    let name = format!(
        "{REVISION:016x}-{species:?}-{outfit:?}-{color:06x}-{pose:?}-{side}{}.png",
        if portrait { "-portrait" } else { "" }
    );
    let directory = Path::new(directory);
    static PRUNED: Once = Once::new();
    PRUNED.call_once(|| prune(directory));
    let path = directory.join(&name);
    if path.is_file() {
        return Ok(name);
    }
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let png = encode(
        &monobot_persona::render_at(look, pose, side, portrait),
        side,
    )?;
    // Written whole under another name, then renamed: two calls can render
    // one picture at once, and an image view must never read half of it.
    static PARTS: AtomicU64 = AtomicU64::new(0);
    let part = directory.join(format!(
        ".{name}.{}-{}.part",
        std::process::id(),
        PARTS.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&part, png).map_err(|e| e.to_string())?;
    fs::rename(&part, &path).map_err(|e| {
        let _ = fs::remove_file(&part);
        e.to_string()
    })?;
    Ok(name)
}

fn encode(rgba: &[u8], side: u32) -> LinkResult<Vec<u8>> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, side, side);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(png)
}

/// Removes, once a launch, what an older renderer drew and what a write cut
/// short left behind. Only names this module writes are touched.
fn prune(directory: &Path) {
    let current = format!("{REVISION:016x}-");
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let picture = name.ends_with(".png")
            && name.len() > 17
            && name.as_bytes()[16] == b'-'
            && name[..16].bytes().all(|b| b.is_ascii_hexdigit());
        let stale = (picture && !name.starts_with(&current))
            || (name.starts_with('.') && name.ends_with(".part"));
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}
