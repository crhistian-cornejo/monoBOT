//! Project appearance and the image treatments from React's background worker.
use image::{imageops, ImageReader, RgbaImage};
use serde::{Deserialize, Serialize};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};
pub type SourceImage = RgbaImage;

pub const COLORS: [u32; 9] = [
    0x8b949c, 0x459cf7, 0xea603e, 0xf4bf25, 0x39c363, 0xe45a9f, 0xb168d4, 0x37beb4, 0xef8c39,
];
pub const MASCOTS: [&str; 10] = [
    "invader", "ghost", "robot", "cat", "skull", "crab", "mushroom", "rocket", "dino", "frog",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectAppearance {
    pub name: String,
    pub color: Option<u32>,
    pub mascot: Option<String>,
    pub logo: Option<String>,
    pub background: Option<Background>,
    /// Colors this project uses instead of the app-wide Appearance settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<ProjectTheme>,
}
impl ProjectAppearance {
    pub fn label(&self, project: &str) -> String {
        if self.name.trim().is_empty() {
            Path::new(project)
                .file_name()
                .map_or_else(|| project.into(), |s| s.to_string_lossy().into_owned())
        } else {
            self.name.clone()
        }
    }
    pub fn color(&self, project: &str) -> u32 {
        self.color.filter(|c| *c <= 0xffffff).unwrap_or_else(|| {
            let hash = project
                .encode_utf16()
                .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c.into()));
            COLORS[(hash as usize % (COLORS.len() - 1)) + 1]
        })
    }
    pub fn mascot(&self, project: &str) -> usize {
        self.mascot
            .as_ref()
            .and_then(|name| MASCOTS.iter().position(|m| *m == name))
            .unwrap_or_else(|| {
                let hash = project
                    .encode_utf16()
                    .fold(0u32, |h, c| h.wrapping_mul(131).wrapping_add(c.into()));
                hash as usize % MASCOTS.len()
            })
    }
    pub fn normalize(&mut self) {
        self.name = self.name.trim().chars().take(200).collect();
        self.color = self.color.filter(|c| *c <= 0xffffff);
        self.mascot = self.mascot.take().filter(|s| MASCOTS.contains(&s.as_str()));
        if let Some(background) = &mut self.background {
            background.normalize();
        }
        self.background = self.background.take().filter(|b| !b.path.trim().is_empty());
        if let Some(theme) = &mut self.theme {
            theme.normalize();
        }
    }
}

/// A project's own copy of the Appearance page's color settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectTheme {
    pub workspace_theme: String,
    /// `#rrggbb`, or `None` for the workspace theme's own accent.
    pub accent_color: Option<String>,
    pub hue: u32,
    pub saturation: u32,
    pub dark_lightness: u32,
}
impl Default for ProjectTheme {
    fn default() -> Self {
        Self {
            workspace_theme: "default".into(),
            accent_color: None,
            hue: 240,
            saturation: 0,
            dark_lightness: 9,
        }
    }
}
impl ProjectTheme {
    pub fn normalize(&mut self) {
        self.hue = self.hue.min(360);
        self.saturation = self.saturation.min(100);
        self.dark_lightness = self.dark_lightness.min(30);
        self.accent_color = self.accent_color.take().filter(|hex| {
            hex.len() == 7
                && hex
                    .strip_prefix('#')
                    .is_some_and(|h| h.chars().all(|c| c.is_ascii_hexdigit()))
        });
        if self.workspace_theme.trim().is_empty() {
            self.workspace_theme = "default".into();
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    #[default]
    None,
    Dither,
    Ascii,
    Halftone,
    Scanlines,
    GradientBlur,
}
impl Effect {
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Dither,
        Self::Ascii,
        Self::Halftone,
        Self::Scanlines,
        Self::GradientBlur,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Dither => "Dither",
            Self::Ascii => "ASCII",
            Self::Halftone => "Halftone",
            Self::Scanlines => "Scanlines",
            Self::GradientBlur => "Haze",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::None => "Shows the original artwork.",
            Self::Dither => "Rebuilds the artwork with a dithered color palette.",
            Self::Ascii => "Recreates the artwork with colored characters on black.",
            Self::Halftone => "Recreates the artwork with colored print dots on black.",
            Self::Scanlines => "Adds a pronounced horizontal display-line texture.",
            Self::GradientBlur => "Blurs and fades the artwork into the background below.",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Empty,
    #[default]
    All,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Background {
    pub path: String,
    pub effect: Effect,
    pub scope: Scope,
    pub empty_opacity: f32,
    pub session_opacity: f32,
}
impl Default for Background {
    fn default() -> Self {
        Self {
            path: String::new(),
            effect: Effect::None,
            scope: Scope::All,
            empty_opacity: 0.24,
            session_opacity: 0.24,
        }
    }
}
impl Background {
    pub fn normalize(&mut self) {
        for value in [&mut self.empty_opacity, &mut self.session_opacity] {
            *value = if value.is_finite() {
                value.clamp(0.05, 0.65)
            } else {
                0.24
            };
        }
    }
    pub fn opacity(&self, empty: bool) -> f32 {
        if !empty && self.scope == Scope::Empty {
            0.
        } else if empty {
            self.empty_opacity
        } else {
            self.session_opacity
        }
    }
}

/// Decode with bounded allocations before resizing; all callers run off-thread.
pub fn read_image(path: &Path) -> Result<RgbaImage, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("Cannot read image: {e}"))?;
    if metadata.len() > 30 * 1024 * 1024 {
        return Err("Choose an image smaller than 30 MB.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| "Choose a PNG, JPEG, GIF, WebP, or BMP image.".to_owned())?;
    Ok(if image.width() > 2048 || image.height() > 2048 {
        image.thumbnail(2048, 2048)
    } else {
        image
    }
    .into_rgba8())
}

/// How long an animation's frame shows when its file asks for less than a
/// browser would: a GIF's 0 or 10 ms means "as fast as you like", which
/// browsers read as 100 ms.
const SLOWEST_FAST_FRAME: Duration = Duration::from_millis(100);

/// The frames of an animated picture (a GIF, an animated WebP or PNG), each
/// with how long it shows, scaled down to fit `max_side`. `None` for a still
/// picture. Past `max_bytes` of frames, every other frame is dropped and its
/// time given to the one before, so a long animation keeps its pace in less
/// memory.
pub fn read_animation(
    path: &Path,
    max_side: u32,
    max_bytes: usize,
) -> Result<Option<Vec<(RgbaImage, Duration)>>, String> {
    use image::{AnimationDecoder, ImageFormat};
    let metadata = std::fs::metadata(path).map_err(|e| format!("Cannot read image: {e}"))?;
    if metadata.len() > 30 * 1024 * 1024 {
        return Err("Choose an image smaller than 30 MB.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let frames = match image::guess_format(&bytes) {
        Ok(ImageFormat::Gif) => image::codecs::gif::GifDecoder::new(Cursor::new(&bytes))
            .map_err(|e| e.to_string())?
            .into_frames(),
        Ok(ImageFormat::WebP) => {
            let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(&bytes))
                .map_err(|e| e.to_string())?;
            if !decoder.has_animation() {
                return Ok(None);
            }
            decoder.into_frames()
        }
        Ok(ImageFormat::Png) => {
            let decoder = image::codecs::png::PngDecoder::new(Cursor::new(&bytes))
                .map_err(|e| e.to_string())?;
            if !decoder.is_apng().map_err(|e| e.to_string())? {
                return Ok(None);
            }
            decoder.apng().map_err(|e| e.to_string())?.into_frames()
        }
        _ => return Ok(None),
    };
    let mut kept: Vec<(RgbaImage, Duration)> = Vec::new();
    // How many of the file's frames each kept one stands for, and how many
    // it should: halving doubles the stride for every frame after it too.
    let mut stands_for: Vec<usize> = Vec::new();
    let mut stride = 1usize;
    let mut used = 0usize;
    for frame in frames {
        let frame = frame.map_err(|_| "This animation could not be read.".to_owned())?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let shown = Duration::from_micros(u64::from(numer) * 1000 / u64::from(denom.max(1)));
        let shown = if shown < Duration::from_millis(20) {
            SLOWEST_FAST_FRAME
        } else {
            shown
        };
        if let (Some(last), Some(count)) = (kept.last_mut(), stands_for.last_mut()) {
            if *count < stride {
                last.1 += shown;
                *count += 1;
                continue;
            }
        }
        let mut picture = frame.into_buffer();
        if picture.width() > max_side || picture.height() > max_side {
            picture = imageops::thumbnail(&picture, max_side, max_side);
        }
        used += picture.as_raw().len();
        kept.push((picture, shown));
        stands_for.push(1);
        // Halve the frames, keeping the total time, until they fit.
        while used > max_bytes && kept.len() > 1 {
            stride *= 2;
            let mut halved = Vec::with_capacity(kept.len().div_ceil(2));
            let mut counts = Vec::with_capacity(kept.len().div_ceil(2));
            let mut pairs = kept.into_iter().zip(stands_for);
            while let Some(((picture, mut shown), mut count)) = pairs.next() {
                if let Some(((dropped, extra), more)) = pairs.next() {
                    shown += extra;
                    count += more;
                    used -= dropped.as_raw().len();
                }
                halved.push((picture, shown));
                counts.push(count);
            }
            kept = halved;
            stands_for = counts;
        }
    }
    Ok((kept.len() > 1).then_some(kept))
}

/// Whether the picture at `path` moves.
pub fn is_animated(path: &Path) -> bool {
    read_animation(path, 16, usize::MAX).is_ok_and(|frames| frames.is_some())
}

/// Own a validated copy, so moving or deleting the original does not break it.
pub fn import_image(
    source: &Path,
    directory: &Path,
    project: &str,
    logo: bool,
) -> Result<String, String> {
    if !logo && crate::video::is_video(source) {
        return import_video(source, directory, project);
    }
    let image = read_image(source)?;
    // A moving background keeps its file as it is: a PNG of it would keep
    // only the first frame.
    if !logo && is_animated(source) {
        let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
        let extension = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Gif) => "gif",
            Ok(image::ImageFormat::WebP) => "webp",
            _ => "png",
        };
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let mut hash = DefaultHasher::new();
        project.hash(&mut hash);
        bytes.hash(&mut hash);
        let path = directory.join(format!("background-{:016x}.{extension}", hash.finish()));
        if !path.exists() {
            std::fs::write(&path, &bytes).map_err(|e| format!("Cannot save image: {e}"))?;
        }
        return Ok(path.to_string_lossy().into_owned());
    }
    let image = if logo && (image.width() > 256 || image.height() > 256) {
        imageops::thumbnail(&image, 256, 256)
    } else {
        image
    };
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut hash = DefaultHasher::new();
    project.hash(&mut hash);
    image.dimensions().hash(&mut hash);
    image.as_raw().hash(&mut hash);
    let path: PathBuf = directory.join(format!(
        "{}-{:016x}.png",
        if logo { "logo" } else { "background" },
        hash.finish()
    ));
    if !path.exists() {
        image
            .save(&path)
            .map_err(|e| format!("Cannot save image: {e}"))?;
    }
    Ok(path.to_string_lossy().into_owned())
}

/// A video background's own copy: a hard link when the drive allows one, so
/// a clip of gigabytes takes no more room, else a copy. Named by where it
/// came from, its size and its time, not its bytes, which would mean reading
/// all of it.
fn import_video(source: &Path, directory: &Path, project: &str) -> Result<String, String> {
    const LARGEST: u64 = 2 * 1024 * 1024 * 1024;
    let metadata = std::fs::metadata(source).map_err(|e| format!("Cannot read video: {e}"))?;
    if metadata.len() > LARGEST {
        return Err("Choose a video smaller than 2 GB.".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut hash = DefaultHasher::new();
    project.hash(&mut hash);
    source.hash(&mut hash);
    metadata.len().hash(&mut hash);
    metadata.modified().ok().hash(&mut hash);
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();
    let path = directory.join(format!("background-{:016x}.{extension}", hash.finish()));
    if !path.exists() && std::fs::hard_link(source, &path).is_err() {
        std::fs::copy(source, &path).map_err(|e| format!("Cannot save video: {e}"))?;
    }
    Ok(path.to_string_lossy().into_owned())
}

pub fn background_fade(y: f32, empty: bool, haze: bool) -> f32 {
    let stops = background_fade_stops(empty, haze);
    for pair in stops.windows(2) {
        if y <= pair[1].0 {
            let t = ((y - pair[0].0) / (pair[1].0 - pair[0].0)).clamp(0., 1.);
            return pair[0].1 + (pair[1].1 - pair[0].1) * t;
        }
    }
    0.
}

/// The fade's stops, top to bottom: where (0 to 1 down the view) and how
/// much of the picture shows there. Straight lines join them, and below the
/// last nothing shows, so a renderer can draw one gradient per gap.
pub fn background_fade_stops(empty: bool, haze: bool) -> &'static [(f32, f32)] {
    match (empty, haze) {
        (true, true) => &[
            (0., 1.),
            (0.19, 1.),
            (0.30, 0.94),
            (0.42, 0.72),
            (0.54, 0.42),
            (0.66, 0.16),
            (0.78, 0.04),
            (0.90, 0.),
        ],
        (false, true) => &[(0., 1.), (0.18, 1.), (0.76, 0.)],
        (true, false) => &[
            (0., 1.),
            (0.19, 1.),
            (0.30, 0.79),
            (0.42, 0.50),
            (0.54, 0.20),
            (0.64, 0.05),
            (0.72, 0.),
        ],
        (false, false) => &[
            (0., 1.),
            (0.18, 1.),
            (0.32, 0.61),
            (0.47, 0.32),
            (0.62, 0.11),
            (0.72, 0.),
        ],
    }
}

/// View-independent artwork, matching the React worker. Crop, fade and Haze
/// belong to the view and must not trigger another texture job during resize.
/// Takes the decoded source so plain artwork becomes the texture without a
/// second full-size copy.
pub fn background_texture(source: RgbaImage, effect: Effect) -> RgbaImage {
    if matches!(effect, Effect::None | Effect::GradientBlur) {
        source
    } else {
        effect_pixels(&source, effect, true, false, 1.)
    }
}

/// Backends without the Metal layer compositor bake Haze into the source;
/// their cover crop and opacity mask still follow the current view immediately.
pub fn software_background_texture(
    source: &RgbaImage,
    effect: Effect,
    empty: bool,
    preview: bool,
) -> RgbaImage {
    if effect == Effect::GradientBlur {
        effect_pixels(source, effect, empty, preview, 1.)
    } else {
        background_texture(source.clone(), effect)
    }
}

/// React processes texture effects at the artwork's resolution, then the view
/// cover-crops the result and applies its own fade. CSS blur uses view pixels.
pub fn render_background(
    source: &RgbaImage,
    effect: Effect,
    width: u32,
    height: u32,
    empty: bool,
) -> RgbaImage {
    render_background_scaled(source, effect, width, height, empty, false, 1.)
}
pub fn render_background_scaled(
    source: &RgbaImage,
    effect: Effect,
    width: u32,
    height: u32,
    empty: bool,
    preview: bool,
    scale: f32,
) -> RgbaImage {
    let textured = if matches!(
        effect,
        Effect::Dither | Effect::Ascii | Effect::Halftone | Effect::Scanlines
    ) {
        effect_pixels(source, effect, empty, preview, scale)
    } else {
        source.clone()
    };
    let covered = cover_image(&textured, width, height);
    let mut output = if effect == Effect::GradientBlur {
        effect_pixels(&covered, effect, empty, preview, scale)
    } else {
        covered
    };
    let height = output.height();
    for (_, y, pixel) in output.enumerate_pixels_mut() {
        pixel[3] = (pixel[3] as f32
            * background_fade(
                y as f32 / height as f32,
                empty,
                effect == Effect::GradientBlur,
            ))
        .round() as u8;
    }
    output
}
fn cover_image(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let width = width.clamp(1, 2048);
    let height = height.clamp(1, 2048);
    let aspect = width as f64 / height as f64;
    let crop_width = source
        .width()
        .min((source.height() as f64 * aspect).round().max(1.) as u32);
    let crop_height = source
        .height()
        .min((source.width() as f64 / aspect).round().max(1.) as u32);
    let cropped = imageops::crop_imm(
        source,
        (source.width() - crop_width) / 2,
        (source.height() - crop_height) / 2,
        crop_width,
        crop_height,
    )
    .to_image();
    imageops::resize(&cropped, width, height, imageops::FilterType::Triangle)
}
fn effect_pixels(
    source: &RgbaImage,
    effect: Effect,
    empty: bool,
    preview: bool,
    scale: f32,
) -> RgbaImage {
    let (width, height) = source.dimensions();
    let mut output = source.clone();
    let soft = (effect == Effect::GradientBlur).then(|| {
        imageops::blur(
            source,
            if preview {
                2.
            } else if empty {
                7.
            } else {
                5.
            } * scale,
        )
    });
    let strong = (effect == Effect::GradientBlur).then(|| {
        imageops::blur(
            source,
            if preview {
                5.
            } else if empty {
                18.
            } else {
                12.
            } * scale,
        )
    });
    const BAYER: [[f32; 4]; 4] = [
        [0., 8., 2., 10.],
        [12., 4., 14., 6.],
        [3., 11., 1., 9.],
        [15., 7., 13., 5.],
    ];
    const GLYPHS: [[u8; 7]; 10] = [
        [0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 4, 0],
        [0, 4, 0, 0, 4, 0, 0],
        [0, 0, 0, 14, 0, 0, 0],
        [0, 0, 14, 0, 14, 0, 0],
        [0, 4, 4, 31, 4, 4, 0],
        [0, 21, 14, 31, 14, 21, 0],
        [10, 10, 31, 10, 31, 10, 10],
        [17, 2, 4, 4, 8, 16, 17],
        [14, 17, 23, 21, 23, 16, 14],
    ];
    let sample = |x: u32, y: u32| source.get_pixel(x.min(width - 1), y.min(height - 1));
    let luma = |p: &image::Rgba<u8>| {
        (p[0] as f32 * 0.2126 + p[1] as f32 * 0.7152 + p[2] as f32 * 0.0722).round()
    };
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let at = y as f32 / height as f32;
        match effect {
            Effect::None => {}
            Effect::Dither => {
                let p = sample(x / 2 * 2 + 1, y / 2 * 2 + 1);
                let peak = p[0].max(p[1]).max(p[2]) as f32;
                let gain = if peak / 255.
                    > (BAYER[(y / 2 % 4) as usize][(x / 2 % 4) as usize] + 0.5) / 16.
                {
                    255. / peak.max(1.)
                } else {
                    0.08
                };
                for c in 0..3 {
                    pixel[c] = (p[c] as f32 * gain).round() as u8;
                }
                pixel[3] = p[3];
            }
            Effect::Ascii => {
                let p = sample(x / 6 * 6 + 3, y / 8 * 8 + 4);
                let glyph = &GLYPHS[((luma(p) / 255.).sqrt() * 9.) as usize];
                let ink =
                    x % 6 < 5 && y % 8 < 7 && (glyph[(y % 8) as usize] & (1 << (4 - x % 6))) != 0;
                for c in 0..3 {
                    pixel[c] = (pixel[c] as f32 * 0.6 + if ink { p[c] as f32 * 0.4 } else { 0. })
                        .round() as u8;
                }
            }
            Effect::Halftone => {
                let radius = 2. * (0.3 + 0.7 * (luma(sample(x / 4 * 4, y / 4 * 4)) / 255.).sqrt());
                let p = sample(x / 4 * 4 + 2, y / 4 * 4 + 2);
                let distance = (x as f32 % 4. - 1.5).hypot(y as f32 % 4. - 1.5);
                let coverage = (radius + 0.5 - distance).clamp(0., 1.) * p[3] as f32 / 255.;
                for c in 0..3 {
                    pixel[c] = (pixel[c] as f32 * 0.6 + p[c] as f32 * coverage * 0.4).round() as u8;
                }
            }
            Effect::Scanlines => {
                if y % 3 == 0 {
                    for c in 0..3 {
                        pixel[c] = (pixel[c] as f32 * 0.52).round() as u8;
                    }
                }
            }
            Effect::GradientBlur => {
                let soft_mix = ((at - 0.18) / if empty { 0.42 } else { 0.52 }).clamp(0., 1.);
                let strong_mix = ((at - if empty { 0.28 } else { 0.22 })
                    / if empty { 0.48 } else { 0.54 })
                .clamp(0., 1.);
                let darken = if empty {
                    ((at - 0.16) / 0.59).clamp(0., 1.) * 0.78
                } else {
                    ((at - 0.15) / 0.61).clamp(0., 1.) * 0.70
                };
                for c in 0..3 {
                    let a = pixel[c] as f32 * (1. - soft_mix)
                        + soft.as_ref().unwrap().get_pixel(x, y)[c] as f32 * soft_mix;
                    let b = a * (1. - strong_mix)
                        + strong.as_ref().unwrap().get_pixel(x, y)[c] as f32 * strong_mix;
                    let mix = if y % 3 == 0 {
                        1. - (1. - darken) * 0.64
                    } else {
                        darken
                    };
                    pixel[c] = (b * (1. - mix) + 23. * mix).round() as u8;
                }
            }
        }
    }
    output
}

pub fn png_bytes(image: RgbaImage) -> Result<Vec<u8>, String> {
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GIF of `count` frames, each `ms` long, on disk.
    fn gif(dir: &Path, count: u8, ms: u32) -> PathBuf {
        use image::codecs::gif::GifEncoder;
        let path = dir.join("moving.gif");
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = GifEncoder::new(file);
        for shade in 0..count {
            let picture = RgbaImage::from_pixel(64, 32, image::Rgba([shade * 40, 90, 160, 255]));
            let delay = image::Delay::from_numer_denom_ms(ms, 1);
            encoder
                .encode_frame(image::Frame::from_parts(picture, 0, 0, delay))
                .unwrap();
        }
        drop(encoder);
        path
    }

    #[test]
    fn an_animation_keeps_its_frames_and_pace_and_a_still_has_none() {
        let directory = tempfile::tempdir().unwrap();
        let path = gif(directory.path(), 4, 50);
        let frames = read_animation(&path, 32, usize::MAX).unwrap().unwrap();
        assert_eq!(frames.len(), 4);
        assert!(frames.iter().all(|(picture, shown)| {
            picture.width() <= 32 && *shown == Duration::from_millis(50)
        }));
        // Too many bytes: half the frames, each shown twice as long.
        let one = frames[0].0.as_raw().len();
        let halved = read_animation(&path, 32, one * 2).unwrap().unwrap();
        assert_eq!(halved.len(), 2);
        assert!(halved
            .iter()
            .all(|(_, shown)| *shown == Duration::from_millis(100)));
        // "As fast as you like" reads as a browser's 100 ms.
        let fast = gif(directory.path(), 2, 0);
        let frames = read_animation(&fast, 32, usize::MAX).unwrap().unwrap();
        assert_eq!(frames[0].1, Duration::from_millis(100));
        let still = directory.path().join("still.png");
        RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]))
            .save(&still)
            .unwrap();
        assert_eq!(read_animation(&still, 32, usize::MAX).unwrap(), None);
    }

    #[test]
    fn a_video_background_is_kept_beside_the_others() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Ocean.MP4");
        std::fs::write(&source, b"not really a video").unwrap();
        let kept = import_image(&source, &directory.path().join("assets"), "/p", false).unwrap();
        assert!(kept.ends_with(".mp4"));
        assert_eq!(std::fs::read(&kept).unwrap(), b"not really a video");
        // The same file again is the same copy.
        let again = import_image(&source, &directory.path().join("assets"), "/p", false).unwrap();
        assert_eq!(kept, again);
        // A video is never a logo.
        assert!(import_image(&source, &directory.path().join("assets"), "/p", true).is_err());
    }

    #[test]
    fn a_moving_background_is_kept_as_its_own_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = gif(directory.path(), 3, 80);
        let kept = import_image(&source, &directory.path().join("assets"), "/p", false).unwrap();
        assert!(kept.ends_with(".gif"));
        assert_eq!(
            std::fs::read(&kept).unwrap(),
            std::fs::read(&source).unwrap()
        );
        assert!(is_animated(Path::new(&kept)));
        // A logo is still one still picture.
        let logo = import_image(&source, &directory.path().join("assets"), "/p", true).unwrap();
        assert!(logo.ends_with(".png"));
    }
    #[test]
    fn dither_matches_the_react_workers_palette_and_two_pixel_bayer_cells() {
        let source = RgbaImage::from_pixel(9, 9, image::Rgba([130, 180, 210, 127]));
        let result = background_texture(source.clone(), Effect::Dither);
        let bayer = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
        for (x, y, pixel) in result.enumerate_pixels() {
            let expected = if bayer[(y / 2 % 4) as usize][(x / 2 % 4) as usize] <= 12 {
                [158, 219, 255, 127]
            } else {
                [10, 14, 17, 127]
            };
            assert_eq!(pixel.0, expected, "React worker sample at {x},{y}");
        }
        assert_eq!(background_texture(source.clone(), Effect::None), source);
        assert_eq!(
            background_texture(source.clone(), Effect::GradientBlur),
            source
        );
    }
    #[test]
    fn dither_texture_scales_with_the_artwork_in_the_preview() {
        let source = RgbaImage::from_pixel(128, 64, image::Rgba([130, 180, 210, 255]));
        let full = render_background(&source, Effect::Dither, 128, 64, true);
        let preview = render_background(&source, Effect::Dither, 64, 32, true);
        let reduced = imageops::resize(&full, 64, 32, imageops::FilterType::Triangle);
        for x in 0..64 {
            assert_eq!(
                preview.get_pixel(x, 0).0[..3],
                reduced.get_pixel(x, 0).0[..3]
            );
        }
    }
    #[test]
    fn effects_keep_cover_dimensions_fade_and_change_pixels() {
        let source = RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 5) as u8, 90, 255])
        });
        let plain = render_background(&source, Effect::None, 41, 33, true);
        for effect in Effect::ALL {
            let result = render_background(&source, effect, 41, 33, true);
            assert_eq!(result.dimensions(), (41, 33));
            assert_eq!(result.get_pixel(0, 0)[3], 255);
            assert_eq!(result.get_pixel(20, 32)[3], 0);
            if effect != Effect::None {
                assert_ne!(result, plain, "{effect:?}");
            }
        }
        let dark = RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 100]));
        assert_eq!(
            render_background(&dark, Effect::Dither, 4, 4, true)
                .get_pixel(0, 0)
                .0,
            [0, 0, 0, 100]
        );
    }
    #[test]
    fn project_settings_normalize_and_round_trip_without_crossing_projects() {
        let mut appearance = ProjectAppearance {
            name: "  My project  ".into(),
            color: Some(0xff000000),
            mascot: Some("missing".into()),
            background: Some(Background {
                path: "a.png".into(),
                empty_opacity: f32::NAN,
                session_opacity: 4.,
                scope: Scope::Empty,
                ..Default::default()
            }),
            ..Default::default()
        };
        appearance.normalize();
        assert_eq!(appearance.label("/tmp/project"), "My project");
        assert_eq!(appearance.color, None);
        assert_eq!(appearance.mascot, None);
        let background = appearance.background.as_ref().unwrap();
        assert_eq!(background.opacity(true), 0.24);
        assert_eq!(background.opacity(false), 0.);
        assert_eq!(background.session_opacity, 0.65);
        assert_eq!(
            serde_json::from_str::<ProjectAppearance>(&serde_json::to_string(&appearance).unwrap())
                .unwrap(),
            appearance
        );
    }
    #[test]
    fn importing_owns_the_image_and_rejects_invalid_files() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("original.png");
        RgbaImage::from_pixel(12, 8, image::Rgba([100, 180, 90, 255]))
            .save(&source)
            .unwrap();
        let imported =
            import_image(&source, &directory.path().join("assets"), "/project", false).unwrap();
        std::fs::remove_file(source).unwrap();
        assert_eq!(
            read_image(Path::new(&imported)).unwrap().dimensions(),
            (12, 8)
        );
        let rotated = directory.path().join("rotated.png");
        RgbaImage::from_pixel(8, 12, image::Rgba([100, 180, 90, 255]))
            .save(&rotated)
            .unwrap();
        let other = import_image(
            &rotated,
            &directory.path().join("assets"),
            "/project",
            false,
        )
        .unwrap();
        assert_ne!(
            imported, other,
            "equal pixel bytes with a different shape need separate assets"
        );
        assert_eq!(read_image(Path::new(&other)).unwrap().dimensions(), (8, 12));
        let bad = directory.path().join("bad.png");
        std::fs::write(&bad, b"not an image").unwrap();
        assert!(import_image(&bad, &directory.path().join("assets"), "/project", false).is_err());
    }
}
