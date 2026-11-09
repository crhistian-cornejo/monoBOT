//! `cargo xtask logo`: redraws every MonoBOT icon from `assets/logo/monobot.svg`.
//!
//! The mark is one path. Each icon places it on a square: alone in white or
//! black for the app to tint, on black, cut out of black, or on the rounded
//! tile the desktop icons use. resvg draws them (the optional `logo` feature;
//! this command turns it on by itself).
//!
//! The Liquid Glass icon of macOS 26 is an asset catalog only Xcode's `actool`
//! writes: this command draws its layer, `assets/macos/AppIcon.icon`, and
//! `cargo xtask build` compiles it into the app on a Mac.
// Without the feature only the re-run and the tests use these helpers.
#![cfg_attr(not(feature = "logo"), allow(dead_code))]
use crate::util::Result;

const USAGE: &str = "usage: cargo xtask logo";
const SOURCE: &str = "assets/logo/monobot.svg";

/// How an icon shows the mark.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    /// The mark alone in white, for the app to tint.
    White,
    /// The mark alone in black.
    Black,
    /// Black everywhere but the mark.
    CutOut,
    /// White on a black square.
    OnBlack,
    /// White on the desktop icon's rounded black tile, which stops `inset` of
    /// the side short of each edge.
    Tile { inset: f32 },
}

/// The pictures: file, side in pixels, the mark's width as a part of the
/// side, and its look.
const PICTURES: &[(&str, u32, f32, Look)] = &[
    (
        "crates/monocode-gpui/assets/monocode.png",
        1024,
        0.84,
        Look::White,
    ),
    // The menu bar draws it 18 points wide, so it fills more of its square.
    ("assets/macos/status-icon.png", 1024, 0.9, Look::White),
    (
        "assets/macos/AppIcon.icon/Assets/icon.png",
        1024,
        0.66,
        Look::White,
    ),
    // The Linux launcher icon.
    (
        "assets/icons/icon.png",
        256,
        TILE_MARK,
        Look::Tile { inset: ICO_INSET },
    ),
    (
        "mobile/assets/images/monocode-mark.png",
        1024,
        0.84,
        Look::White,
    ),
    (
        "mobile/assets/images/monocode-mark-black.png",
        512,
        0.84,
        Look::Black,
    ),
    (
        "mobile/assets/images/monocode-mark-inverse.png",
        512,
        0.84,
        Look::CutOut,
    ),
    ("mobile/assets/images/icon.png", 1024, 0.66, Look::OnBlack),
    (
        "mobile/assets/app.icon/Assets/icon.png",
        1024,
        0.66,
        Look::White,
    ),
    // Android masks the launcher icon to the middle of the square.
    (
        "mobile/assets/images/android-icon-foreground.png",
        512,
        0.52,
        Look::White,
    ),
    (
        "mobile/assets/images/android-icon-monochrome.png",
        432,
        0.52,
        Look::White,
    ),
    ("mobile/assets/images/favicon.png", 48, 0.76, Look::OnBlack),
    ("mobile/assets/images/splash-icon.png", 456, 1., Look::White),
];

/// The mark's width inside the tile, as a part of the tile.
const TILE_MARK: f32 = 0.68;
const ICO_SIDES: [u32; 6] = [16, 24, 32, 48, 64, 256];
const ICO_INSET: f32 = 0.066;
/// The macOS icon's entries, on Apple's grid: an 824 tile in a 1024 square.
const ICNS_ENTRIES: [(&[u8; 4], u32); 8] = [
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic13", 256),
    (b"ic08", 256),
    (b"ic14", 512),
    (b"ic09", 512),
    (b"ic10", 1024),
];
const ICNS_INSET: f32 = 100. / 1024.;

/// A Windows icon's tile inset and mark width at `side`. Up to 32 pixels the
/// tile fills the square and the mark most of the tile: a margin would leave
/// the eyes less than a pixel each.
fn ico_tile(side: u32) -> (f32, f32) {
    if side <= 32 {
        (0., 0.8)
    } else {
        (ICO_INSET, TILE_MARK)
    }
}

/// The mark: its path and the box the path fills.
#[derive(Debug, PartialEq)]
struct Mark {
    path: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// The `d` of the source's one path.
fn path_data(svg: &str) -> Result<&str> {
    let missing = || format!("{SOURCE} has no path");
    let start = svg.find(" d=\"").ok_or_else(missing)? + 4;
    let end = svg[start..].find('"').ok_or_else(missing)?;
    Ok(&svg[start..start + end])
}

/// The SVG of one icon: `side` square, the mark `width` of its room wide and
/// centred. A cut-out is drawn as the white mark and inverted afterwards.
fn compose(mark: &Mark, side: u32, width: f32, look: Look) -> String {
    let side = side as f32;
    let (backdrop, room, fill) = match look {
        Look::White | Look::CutOut => (String::new(), side, "#fff"),
        Look::Black => (String::new(), side, "#000"),
        Look::OnBlack => (
            format!(r#"<rect width="{side}" height="{side}"/>"#),
            side,
            "#fff",
        ),
        Look::Tile { inset } => {
            let room = side * (1. - 2. * inset);
            // A hairline inside the edge keeps the tile apart from a dark taskbar.
            let edge = side / 200.;
            let (at, across) = (side * inset + edge / 2., room - edge);
            (
                format!(
                    r##"<rect x="{at}" y="{at}" width="{across}" height="{across}" rx="{}" stroke="#fff" stroke-opacity="0.14" stroke-width="{edge}"/>"##,
                    across * 0.225
                ),
                room,
                "#fff",
            )
        }
    };
    let scale = room * width / mark.width.max(mark.height);
    let x = (side - mark.width * scale) / 2. - mark.x * scale;
    let y = (side - mark.height * scale) / 2. - mark.y * scale;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {side} {side}" width="{side}" height="{side}">{backdrop}<path fill="{fill}" fill-rule="evenodd" transform="translate({x} {y}) scale({scale})" d="{}"/></svg>"#,
        mark.path
    )
}

/// A Windows icon of PNG pictures, each with its side.
fn ico(pictures: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut file = vec![0, 0, 1, 0];
    file.extend((pictures.len() as u16).to_le_bytes());
    let mut at = 6 + 16 * pictures.len();
    for (side, png) in pictures {
        // A side of 256 is written as 0.
        file.extend([*side as u8, *side as u8, 0, 0, 1, 0, 32, 0]);
        file.extend((png.len() as u32).to_le_bytes());
        file.extend((at as u32).to_le_bytes());
        at += png.len();
    }
    for (_, png) in pictures {
        file.extend(png);
    }
    file
}

/// A macOS icon of PNG pictures, each under its four-letter type.
fn icns(pictures: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let length: usize = pictures.iter().map(|(_, png)| 8 + png.len()).sum();
    let mut file = b"icns".to_vec();
    file.extend((8 + length as u32).to_be_bytes());
    for (kind, png) in pictures {
        file.extend(*kind);
        file.extend((8 + png.len() as u32).to_be_bytes());
        file.extend(*png);
    }
    file
}

#[cfg(not(feature = "logo"))]
pub fn main(argv: &[String]) -> Result<()> {
    crate::util::rerun_with_feature("logo", argv)
}

#[cfg(feature = "logo")]
pub fn main(argv: &[String]) -> Result<()> {
    use crate::util::write;
    if !argv.is_empty() {
        return Err(USAGE.into());
    }
    let root = crate::util::root();
    let source =
        std::fs::read_to_string(root.join(SOURCE)).map_err(|e| format!("{SOURCE}: {e}"))?;
    let mark = draw::mark(&source)?;
    for &(path, side, width, look) in PICTURES {
        write(&root.join(path), draw::png(&mark, side, width, look)?)?;
    }
    let mut windows = Vec::new();
    for side in ICO_SIDES {
        let (inset, width) = ico_tile(side);
        windows.push((side, draw::png(&mark, side, width, Look::Tile { inset })?));
    }
    write(&root.join("assets/icons/icon.ico"), ico(&windows))?;
    let mut tiles = std::collections::BTreeMap::new();
    for (_, side) in ICNS_ENTRIES {
        if let std::collections::btree_map::Entry::Vacant(tile) = tiles.entry(side) {
            let look = Look::Tile { inset: ICNS_INSET };
            tile.insert(draw::png(&mark, side, TILE_MARK, look)?);
        }
    }
    let entries: Vec<_> = ICNS_ENTRIES
        .iter()
        .map(|(kind, side)| (*kind, tiles[side].as_slice()))
        .collect();
    write(&root.join("assets/icons/icon.icns"), icns(&entries))?;
    println!("Drew {} icons from {SOURCE}.", PICTURES.len() + 2);
    Ok(())
}

#[cfg(feature = "logo")]
mod draw {
    use super::{compose, path_data, Look, Mark, SOURCE};
    use crate::util::Result;
    use resvg::{tiny_skia, usvg};

    fn tree(svg: &str) -> Result<usvg::Tree> {
        usvg::Tree::from_str(svg, &usvg::Options::default()).map_err(|e| e.to_string())
    }

    pub fn mark(source: &str) -> Result<Mark> {
        let bounds = tree(source)
            .map_err(|e| format!("{SOURCE}: {e}"))?
            .root()
            .abs_bounding_box();
        Ok(Mark {
            path: path_data(source)?.to_owned(),
            x: bounds.x(),
            y: bounds.y(),
            width: bounds.width(),
            height: bounds.height(),
        })
    }

    pub fn png(mark: &Mark, side: u32, width: f32, look: Look) -> Result<Vec<u8>> {
        let tree = tree(&compose(mark, side, width, look))?;
        let mut pixmap = tiny_skia::Pixmap::new(side, side).ok_or("An icon needs a side")?;
        resvg::render(
            &tree,
            tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        if look == Look::CutOut {
            for pixel in pixmap.pixels_mut() {
                let clear = 255 - pixel.alpha();
                *pixel = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, clear).unwrap();
            }
        }
        pixmap.encode_png().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARK: &str = include_str!("../../assets/logo/monobot.svg");

    fn mark(x: f32, y: f32, width: f32, height: f32) -> Mark {
        Mark {
            path: "M0 0".into(),
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn reads_the_one_evenodd_path_of_the_source() {
        let path = path_data(MARK).unwrap();
        assert!(path.starts_with("M24 28H48L64 44"));
        assert!(path.trim_end().ends_with("M76 48H92V64H76Z"));
        // `compose` draws the path with this rule, which makes the eyes holes.
        assert!(MARK.contains(r#"fill-rule="evenodd""#));
        assert_eq!(MARK.matches("<path").count(), 1);
        assert!(path_data("<svg/>").is_err());
    }

    #[test]
    fn centres_the_mark_at_the_asked_width() {
        // A mark that already is 104 of 128 wide and centred stays where it is.
        let svg = compose(&mark(12., 28., 104., 72.), 128, 104. / 128., Look::White);
        assert!(svg.contains(
            r##"<path fill="#fff" fill-rule="evenodd" transform="translate(0 0) scale(1)""##
        ));
        assert!(!svg.contains("<rect"));
        let svg = compose(&mark(0., 0., 64., 32.), 256, 0.5, Look::Black);
        assert!(svg.contains(
            r##"fill="#000" fill-rule="evenodd" transform="translate(64 96) scale(2)""##
        ));
        // A tall mark is measured by its height.
        let svg = compose(&mark(0., 0., 32., 64.), 256, 0.5, Look::OnBlack);
        assert!(svg.contains(r#"<rect width="256" height="256"/>"#));
        assert!(svg.contains("translate(96 64) scale(2)"));
    }

    #[test]
    fn the_tile_keeps_its_inset_and_the_mark_fits_the_tile() {
        let svg = compose(
            &mark(0., 0., 50., 50.),
            200,
            0.5,
            Look::Tile { inset: 0.25 },
        );
        // The tile is 100 across at 50; its hairline of 1 sits inside the edge.
        assert!(svg.contains(r#"<rect x="50.5" y="50.5" width="99" height="99" rx="22.275""#));
        assert!(svg.contains("translate(75 75) scale(1)"));
        assert_eq!(ico_tile(32), (0., 0.8));
        assert_eq!(ico_tile(48), (ICO_INSET, TILE_MARK));
    }

    #[test]
    fn writes_a_windows_icon_directory() {
        let file = ico(&[(16, vec![1, 2, 3]), (256, vec![4, 5])]);
        assert_eq!(file[..6], [0, 0, 1, 0, 2, 0]);
        assert_eq!(
            file[6..22],
            [16, 16, 0, 0, 1, 0, 32, 0, 3, 0, 0, 0, 38, 0, 0, 0]
        );
        assert_eq!(
            file[22..38],
            [0, 0, 0, 0, 1, 0, 32, 0, 2, 0, 0, 0, 41, 0, 0, 0]
        );
        assert_eq!(file[38..], [1, 2, 3, 4, 5]);
    }

    #[test]
    fn writes_a_macos_icon_container() {
        let file = icns(&[(b"ic07", &[1, 2, 3]), (b"ic10", &[4])]);
        assert_eq!(&file[..4], b"icns");
        assert_eq!(file[4..8], 28u32.to_be_bytes());
        assert_eq!(&file[8..12], b"ic07");
        assert_eq!(file[12..16], 11u32.to_be_bytes());
        assert_eq!(file[16..19], [1, 2, 3]);
        assert_eq!(&file[19..23], b"ic10");
        assert_eq!(file[23..], [0, 0, 0, 9, 4]);
        // Every size the macOS icon lists is one this command draws once.
        let sides: std::collections::BTreeSet<u32> =
            ICNS_ENTRIES.iter().map(|(_, side)| *side).collect();
        assert_eq!(
            sides.into_iter().collect::<Vec<_>>(),
            [32, 64, 128, 256, 512, 1024]
        );
    }

    #[cfg(feature = "logo")]
    #[test]
    fn draws_the_mark_white_on_its_tile() {
        use resvg::tiny_skia::Pixmap;
        let mark = draw::mark(MARK).unwrap();
        assert_eq!(
            (mark.x, mark.y, mark.width, mark.height),
            (12., 28., 104., 72.)
        );
        let pixel = |look, x: u32, y: u32| {
            let png = draw::png(&mark, 128, TILE_MARK, look).unwrap();
            let color = Pixmap::decode_png(&png).unwrap().pixel(x, y).unwrap();
            (color.red(), color.alpha())
        };
        let tile = Look::Tile { inset: ICNS_INSET };
        // The M's middle is white, the tile above it black, the corner clear.
        assert_eq!(pixel(tile, 64, 64), (255, 255));
        assert_eq!(pixel(tile, 64, 20), (0, 255));
        assert_eq!(pixel(tile, 2, 2), (0, 0));
        // A cut-out is black where the mark is not and clear where it is.
        assert_eq!(pixel(Look::CutOut, 64, 64), (0, 0));
        assert_eq!(pixel(Look::CutOut, 2, 2), (0, 255));
    }
}
