//! The inspector: the elements under a point on the device's screen, front
//! to back, read from the runner's accessibility tree (the same lines
//! `describe_ui` gives agents), and what "Add to chat" attaches for one of
//! them: its frame, its description and a crop of the screen.
use std::path::PathBuf;

/// One element of the tree, with its frame in points.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    /// How many listed ancestors it has.
    pub depth: usize,
    /// "Button", "StaticText"…
    pub kind: String,
    /// Its label, when it has one.
    pub label: Option<String>,
    /// The line without its position: kind, label, identifier, value, state.
    pub summary: String,
    /// x, y, width, height in points.
    pub frame: (f32, f32, f32, f32),
}

impl Element {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let (left, top, width, height) = self.frame;
        x >= left && y >= top && x <= left + width && y <= top + height
    }

    /// What goes into the chat with the crop.
    pub fn chat_text(&self, device: &str, note: &str) -> String {
        let (x, y, width, height) = self.frame;
        let mut text = format!(
            "Simulator element on {device}: {} — frame x {x}, y {y}, {width}×{height} pt.",
            self.summary
        );
        if !note.trim().is_empty() {
            text.push_str("\nNote: ");
            text.push_str(note.trim());
        }
        text
    }
}

/// Reads one line of the runner's tree:
/// `  Button "Sign in" id=login at (201, 412) size 120×44`.
pub(crate) fn parse_line(line: &str) -> Option<Element> {
    let trimmed = line.trim_start_matches(' ');
    let depth = (line.len() - trimmed.len()) / 2;
    let (summary, position) = trimmed.rsplit_once(" at (")?;
    let (center, size) = position.split_once(") size ")?;
    let (cx, cy) = center.split_once(", ")?;
    let (width, height) = size.trim().split_once('×')?;
    let number = |text: &str| text.trim().parse::<f32>().ok();
    let (cx, cy, width, height) = (number(cx)?, number(cy)?, number(width)?, number(height)?);
    let kind = summary.split(' ').next()?.to_owned();
    let label = summary
        .strip_prefix(&kind)
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix('"'))
        .and_then(|rest| {
            // The label ends at the quote before the next field, or the last.
            let end = [
                " id=",
                " value=",
                " placeholder=",
                " secure",
                " disabled",
                " selected",
                " focused",
            ]
            .iter()
            .filter_map(|field| rest.find(&format!("\"{field}")))
            .min()
            .or_else(|| rest.rfind('"'))?;
            Some(rest[..end].to_owned())
        });
    Some(Element {
        depth,
        kind,
        label,
        summary: summary.to_owned(),
        frame: (
            (cx - width / 2.).round(),
            (cy - height / 2.).round(),
            width,
            height,
        ),
    })
}

pub fn parse(tree: &str) -> Vec<Element> {
    tree.lines().filter_map(parse_line).collect()
}

/// The elements under a point, in front first: the deepest, and among
/// equals the one listed later (drawn later). The inspector steps through
/// these with "behind" and "in front".
pub fn stack_at(elements: &[Element], x: f32, y: f32) -> Vec<usize> {
    let mut hits: Vec<usize> = (0..elements.len())
        .filter(|&i| elements[i].contains(x, y))
        .collect();
    hits.sort_by(|&a, &b| elements[b].depth.cmp(&elements[a].depth).then(b.cmp(&a)));
    hits
}

/// The tree of what is on screen: the app `bundle` when it is in front,
/// else the home screen.
pub fn describe(udid: &str, bundle: Option<&str>) -> Result<Vec<Element>, String> {
    let (_, result) = super::act(Some(udid), "describe", |_| {
        let mut body = serde_json::json!({"max_lines": 3000});
        if let Some(bundle) = bundle {
            body["bundle_id"] = bundle.into();
            body["guessed"] = true.into();
        }
        body
    })?;
    Ok(parse(result["text"].as_str().unwrap_or_default()))
}

/// Cuts `frame` (points) out of a BGRA screen image drawn at `scale`
/// pixels per point, with a little margin, as RGBA.
pub fn crop(
    bgra: &image::RgbaImage,
    frame: (f32, f32, f32, f32),
    scale: f32,
) -> Option<image::RgbaImage> {
    const MARGIN: f32 = 8.;
    let (x, y, width, height) = frame;
    let left = ((x - MARGIN) * scale).max(0.) as u32;
    let top = ((y - MARGIN) * scale).max(0.) as u32;
    let right = (((x + width + MARGIN) * scale) as u32).min(bgra.width());
    let bottom = (((y + height + MARGIN) * scale) as u32).min(bgra.height());
    if right <= left || bottom <= top {
        return None;
    }
    let mut out = image::imageops::crop_imm(bgra, left, top, right - left, bottom - top).to_image();
    for pixel in out.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(out)
}

/// Saves a crop as a PNG the composer can attach.
pub fn save_crop(crop: &image::RgbaImage) -> Result<PathBuf, String> {
    let dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("com.monocode.desktop/simulator-inspector");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let path = dir.join(format!("element-{stamp}.png"));
    crop.save_with_format(&path, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TREE: &str = "App \"Example\" at (201, 437) size 402×874
  Window at (201, 437) size 402×874
    Button \"Sign in\" id=login at (201, 412) size 120×44
      StaticText \"Sign in\" at (201, 412) size 60×20
    TextField \"Email\" value=\"a@b.c\" placeholder=\"you@example.com\" focused at (201, 300) size 300×40
    Button id=close at (380, 60) size 44×44";

    #[test]
    fn tree_lines_become_elements_with_frames() {
        let elements = parse(TREE);
        assert_eq!(elements.len(), 6);
        let button = &elements[2];
        assert_eq!(button.depth, 2);
        assert_eq!(button.kind, "Button");
        assert_eq!(button.label.as_deref(), Some("Sign in"));
        assert_eq!(button.summary, "Button \"Sign in\" id=login");
        assert_eq!(button.frame, (141., 390., 120., 44.));
        assert_eq!(elements[4].label.as_deref(), Some("Email"));
        assert_eq!(elements[5].label, None);
        assert!(parse_line("… more elements not listed").is_none());
    }

    #[test]
    fn the_stack_under_a_point_goes_front_to_back() {
        let elements = parse(TREE);
        // The text sits on the button, which sits in the window.
        assert_eq!(stack_at(&elements, 201., 412.), vec![3, 2, 1, 0]);
        // Beside the text, the button is in front.
        assert_eq!(stack_at(&elements, 150., 400.), vec![2, 1, 0]);
        assert!(stack_at(&elements, 900., 900.).is_empty());
    }

    #[test]
    fn a_crop_takes_the_frame_with_a_margin_in_rgba() {
        let mut screen = image::RgbaImage::new(30, 30);
        screen.put_pixel(15, 15, image::Rgba([1, 2, 3, 255]));
        // A 2 × 2 point frame at (6, 6) on a 1× screen, with 8 pt around it.
        let crop = crop(&screen, (6., 6., 2., 2.), 1.).unwrap();
        assert_eq!(crop.dimensions(), (16, 16));
        assert_eq!(crop.get_pixel(15, 15).0, [3, 2, 1, 255]);
        assert!(super::crop(&screen, (100., 100., 2., 2.), 1.).is_none());
    }

    #[test]
    fn the_chat_text_names_the_element_its_frame_and_the_note() {
        let element = &parse(TREE)[2];
        assert_eq!(
            element.chat_text("iPhone 18 Pro", " too small "),
            "Simulator element on iPhone 18 Pro: Button \"Sign in\" id=login — frame x 141, y 390, 120×44 pt.\nNote: too small"
        );
    }
}
