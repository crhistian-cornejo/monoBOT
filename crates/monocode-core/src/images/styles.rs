//! Preset looks and formats for the Images bot: the user picks them from the
//! slash menu (`/screenprint`, `/portrait`) or the Styles panel, and
//! `images.generate` takes the style's id, adding its art direction to the
//! brief Gemini gets.
//!
//! The art direction is written as a narrative fragment, the way Google's
//! Nano Banana prompting guide asks for (a description of the look, not a
//! keyword list), so it reads naturally after the subject.

/// A named look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// What `images.generate` takes and the slash command (`/{id}`).
    pub id: &'static str,
    pub name: &'static str,
    /// One line for the menu and the panel.
    pub blurb: &'static str,
    /// The art direction added to the brief.
    pub direction: &'static str,
    /// Three colours that suggest the look, for the panel's swatch.
    pub swatch: [u32; 3],
}

/// A named picture shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    /// The slash command (`/{id}`): the format's own name, lowercased.
    pub id: &'static str,
    /// Older slash words that still mean this format, so prompts written
    /// with them keep working (`/cinema` before `/ultrawide`).
    pub aliases: &'static [&'static str],
    pub name: &'static str,
    /// The ratio `images.generate` takes.
    pub aspect: &'static str,
    /// Where it is used.
    pub use_for: &'static str,
}

pub const STYLES: &[Style] = &[
    Style {
        id: "photoreal",
        name: "Photoreal",
        blurb: "True-to-life photograph",
        direction: "a photorealistic photograph in soft natural light, shot on a 50mm lens with shallow depth of field, true-to-life colour and fine skin and material texture",
        swatch: [0x8a9a8c, 0xd8c3a5, 0x3b4a52],
    },
    Style {
        id: "cinematic",
        name: "Cinematic",
        blurb: "Film still with dramatic light",
        direction: "a cinematic film still with anamorphic widescreen framing, motivated dramatic lighting, a teal and orange grade, shallow depth of field and fine film grain",
        swatch: [0x1f4e5a, 0xe08a3c, 0x111418],
    },
    Style {
        id: "product",
        name: "Studio product",
        blurb: "Clean e-commerce shot",
        direction: "a studio product photograph on a seamless backdrop, three-point softbox lighting, crisp reflections, a 45-degree hero angle and a soft contact shadow",
        swatch: [0xf2f0ec, 0xc9c4bb, 0x2c2c2c],
    },
    Style {
        id: "film",
        name: "Film photo",
        blurb: "35mm colour film, grain and halation",
        direction: "a candid 35mm colour film photograph with soft grain, slight halation around highlights, faded blacks and warm, slightly shifted colours",
        swatch: [0xd9a066, 0x6f8f72, 0x3d2f2a],
    },
    Style {
        id: "editorial",
        name: "Editorial illustration",
        blurb: "Magazine illustration, ink and flat colour",
        direction: "a vintage editorial magazine illustration with confident ink linework, a flat limited palette, subtle paper grain and a mid-century print feel",
        swatch: [0x2f5d8a, 0xe8dcc4, 0xc0392b],
    },
    Style {
        id: "screenprint",
        name: "Screenprint poster",
        blurb: "Two or three inks, halftone, aged paper",
        direction: "a vintage screenprint poster printed in two or three flat ink colours, with halftone dots, slight misregistration between the inks and aged off-white paper",
        swatch: [0x1d4e89, 0xf2e6cf, 0xe4572e],
    },
    Style {
        id: "gouache",
        name: "Flat gouache",
        blurb: "Matte colour fields, simple shapes",
        direction: "a flat gouache painting with matte opaque colour fields, soft brush edges, simplified shapes, generous empty space and a muted, naive palette",
        swatch: [0x5b84c4, 0x7a8b4f, 0xf4f1e8],
    },
    Style {
        id: "watercolor",
        name: "Watercolor",
        blurb: "Loose washes on paper",
        direction: "a loose watercolor painting on cold-press paper with wet-on-wet washes, pigment blooms and granulation, white paper showing through and a light pencil underdrawing",
        swatch: [0x9cc5d9, 0xf0b8a8, 0xfaf7f0],
    },
    Style {
        id: "oil",
        name: "Oil painting",
        blurb: "Thick brushstrokes, painterly light",
        direction: "an impressionist oil painting with thick visible brushstrokes, rich layered colour, painterly light and the texture of canvas",
        swatch: [0xe3b23c, 0x3a6ea5, 0xb5442c],
    },
    Style {
        id: "sketch",
        name: "Ink sketch",
        blurb: "Pen and ink with hatching",
        direction: "a hand-drawn pen and ink sketch with loose confident lines, cross-hatching for shade, unfinished edges and a few watercolor accents on cream paper",
        swatch: [0x2b2b2b, 0xefe6d2, 0xb23a3a],
    },
    Style {
        id: "anime",
        name: "Anime",
        blurb: "Cel shading, bold line art",
        direction: "an anime key visual with clean cel shading, bold line art, expressive characters, vivid flat colours and a luminous sky gradient",
        swatch: [0x6ec6ff, 0xff8fab, 0x2d3047],
    },
    Style {
        id: "cartoon",
        name: "Bold cartoon",
        blurb: "Thick outlines, sticker-like colour",
        direction: "a bold cartoon illustration with thick black outlines, flat saturated colours, playful exaggerated shapes and a sticker-like finish",
        swatch: [0xf7d02c, 0xe63946, 0x1d1d1d],
    },
    Style {
        id: "render3d",
        name: "3D render",
        blurb: "Soft clay-like 3D",
        direction: "a soft 3D render with rounded clay-like forms, gentle global illumination, subtle ambient occlusion and a pastel palette",
        swatch: [0xb8a1e3, 0xffc8a2, 0x9fd8cb],
    },
    Style {
        id: "vector",
        name: "Flat vector",
        blurb: "Geometric shapes, few colours",
        direction: "a minimal flat vector illustration built from geometric shapes, two or three colours without gradients, crisp edges and generous negative space",
        swatch: [0x264653, 0x2a9d8f, 0xe9c46a],
    },
    Style {
        id: "pixel",
        name: "Pixel art",
        blurb: "16-bit sprites, hard pixels",
        direction: "detailed 16-bit pixel art with a limited palette, hard pixel edges, no anti-aliasing and the look of a classic console game",
        swatch: [0x3e2c55, 0x7ec850, 0xf4a261],
    },
    Style {
        id: "collage",
        name: "Surreal collage",
        blurb: "Cut-paper, mixed scale",
        direction: "a surreal paper collage with cut-out edges, vintage magazine photographs at mixed scales, bold colour blocking and a dreamlike composition",
        swatch: [0xe76f51, 0x3a5a8c, 0xf1e3c8],
    },
];

pub const FORMATS: &[Format] = &[
    Format {
        id: "square",
        aliases: &[],
        name: "Square",
        aspect: "1:1",
        use_for: "Instagram post, avatar, icon",
    },
    Format {
        id: "portrait",
        aliases: &[],
        name: "Portrait",
        aspect: "4:5",
        use_for: "Instagram portrait post",
    },
    Format {
        id: "story",
        aliases: &[],
        name: "Story",
        aspect: "9:16",
        use_for: "Stories, Reels, TikTok, phone wallpaper",
    },
    Format {
        id: "poster",
        aliases: &[],
        name: "Poster",
        aspect: "2:3",
        use_for: "Posters, book covers, Pinterest",
    },
    Format {
        id: "classic",
        aliases: &[],
        name: "Classic",
        aspect: "3:2",
        use_for: "Camera photo, blog header",
    },
    Format {
        id: "standard",
        aliases: &[],
        name: "Standard",
        aspect: "4:3",
        use_for: "Slides, tablets, prints",
    },
    Format {
        id: "landscape",
        aliases: &[],
        name: "Landscape",
        aspect: "16:9",
        use_for: "YouTube thumbnail, slides, desktop wallpaper",
    },
    Format {
        id: "ultrawide",
        aliases: &["cinema"],
        name: "Ultrawide",
        aspect: "21:9",
        use_for: "Banners and wide headers",
    },
];

/// A style by id, ignoring case and a leading slash.
pub fn style(id: &str) -> Option<&'static Style> {
    let id = id.trim().trim_start_matches('/');
    STYLES
        .iter()
        .find(|style| style.id.eq_ignore_ascii_case(id))
}

/// A format by id or one of its older aliases, ignoring case and a leading
/// slash.
pub fn format(id: &str) -> Option<&'static Format> {
    let id = id.trim().trim_start_matches('/');
    FORMATS.iter().find(|format| {
        format.id.eq_ignore_ascii_case(id)
            || format
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(id))
    })
}

/// The catalogue id of a style or format word (`/Cinema` → `ultrawide`).
pub fn preset_id(word: &str) -> Option<&'static str> {
    style(word)
        .map(|style| style.id)
        .or_else(|| format(word).map(|format| format.id))
}

/// The named format of a ratio, when it has one.
pub fn format_for_aspect(aspect: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|format| format.aspect == aspect)
}

/// How a picture sits: wider, taller or square.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Orientation {
    Square,
    Portrait,
    Landscape,
}

/// Width over height of a "16:9"-style ratio; square when it can't be read.
pub fn ratio(aspect: Option<&str>) -> f32 {
    aspect
        .and_then(|aspect| aspect.split_once(':'))
        .and_then(|(w, h)| Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?)))
        .filter(|(w, h)| *w > 0. && *h > 0.)
        .map_or(1., |(w, h)| w / h)
}

impl Orientation {
    pub fn of(aspect: Option<&str>) -> Self {
        let ratio = ratio(aspect);
        if (ratio - 1.).abs() < 0.01 {
            Self::Square
        } else if ratio < 1. {
            Self::Portrait
        } else {
            Self::Landscape
        }
    }
}

/// The composer's text with `/{id}` at its start, in place of any other
/// style (or format) word it already had: picking a look twice switches it
/// instead of stacking two. `None` when `id` is neither.
pub fn with_preset(text: &str, id: &str) -> Option<String> {
    let is_style = style(id).is_some();
    if !is_style && format(id).is_none() {
        return None;
    }
    let same_kind = |word: &str| {
        word.strip_prefix('/').is_some_and(|word| {
            if is_style {
                style(word).is_some()
            } else {
                format(word).is_some()
            }
        })
    };
    // Style words first, then format words, then the brief.
    let mut words: Vec<&str> = text.split(' ').filter(|word| !same_kind(word)).collect();
    while words.first() == Some(&"") {
        words.remove(0);
    }
    let presets = words
        .iter()
        .take_while(|word| {
            word.strip_prefix('/')
                .is_some_and(|w| style(w).is_some() || format(w).is_some())
        })
        .count();
    // Always the catalogue's own word, even when an older alias was given.
    let token = format!("/{}", preset_id(id)?);
    let at = if is_style { 0 } else { presets };
    words.insert(at, &token);
    let mut joined = words.join(" ");
    if !joined.ends_with(' ') {
        joined.push(' ');
    }
    Some(joined)
}

/// A preset named in a draft, for the composer's quiet hint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    /// "palette" for a look, "image-01" for a format.
    pub glyph: &'static str,
    /// "Screenprint poster", "Story 9:16".
    pub name: String,
    /// What it does, in a few words.
    pub note: &'static str,
}

/// The looks and formats a draft names with slash words, in its order, each
/// once.
pub fn picked(text: &str) -> Vec<Picked> {
    let mut seen = Vec::new();
    text.split_whitespace()
        .filter_map(|word| word.strip_prefix('/'))
        .filter_map(|word| {
            if let Some(style) = style(word) {
                Some((style.id, "palette", style.name.to_owned(), style.blurb))
            } else {
                format(word).map(|format| {
                    (
                        format.id,
                        "image-01",
                        format!("{} {}", format.name, format.aspect),
                        format.use_for,
                    )
                })
            }
        })
        .filter(|(id, ..)| {
            let new = !seen.contains(id);
            seen.push(*id);
            new
        })
        .map(|(_, glyph, name, note)| Picked { glyph, name, note })
        .collect()
}

/// Byte ranges of the preset slash words in a message (`/screenprint`,
/// `/story`), each a whole word: after the start or a space, and followed by
/// the end, a space or punctuation. The chat draws them as links.
pub fn preset_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    for (start, _) in text.match_indices('/') {
        if text[..start]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace() && !"([\"'".contains(c))
        {
            continue;
        }
        let word = &text[start + 1..];
        let len = word
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(word.len());
        let end = start + 1 + len;
        let next = text[end..].chars().next();
        if len == 0 || next.is_some_and(|c| !c.is_whitespace() && !".,;:!?)]\"'".contains(c)) {
            continue;
        }
        let id = &text[start + 1..end];
        if style(id).is_some() || format(id).is_some() {
            ranges.push(start..end);
        }
    }
    ranges
}

/// The catalogue an image bot reads, so `/screenprint` or `/story` in the
/// user's message, or a style the user names, maps to the right input.
pub fn catalog_rules() -> String {
    let styles = STYLES
        .iter()
        .map(|style| format!("/{} ({}): {}", style.id, style.name, style.blurb))
        .collect::<Vec<_>>()
        .join("; ");
    let formats = FORMATS
        .iter()
        .map(|format| {
            let aliases: String = format
                .aliases
                .iter()
                .map(|alias| format!(", also /{alias}"))
                .collect();
            format!(
                "/{} = {} ({}{aliases})",
                format.id, format.aspect, format.use_for
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "<image_styles>\nMonoBOT's preset looks and formats. When the user's message has one of these slash words, or names one of these looks or uses, pass it to images.generate: a style as \"style\":\"<id>\" (MonoBOT adds its art direction to your brief, so don't repeat it in the prompt; describe the subject, composition and light), a format as its \"aspect\". With a style but no format, pick the aspect the use needs. Without a style, write the look into the prompt yourself.\nStyles: {styles}.\nFormats: {formats}.\n</image_styles>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_slash_words_that_do_not_collide() {
        let mut ids: Vec<&str> = STYLES
            .iter()
            .map(|s| s.id)
            .chain(FORMATS.iter().map(|f| f.id))
            .chain(FORMATS.iter().flat_map(|f| f.aliases.iter().copied()))
            .collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two styles or formats share a word");
        for id in ids {
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "{id}"
            );
        }
    }

    #[test]
    fn every_format_is_a_ratio_gemini_takes() {
        for format in FORMATS {
            assert!(
                super::super::ASPECTS.contains(&format.aspect),
                "{}",
                format.id
            );
        }
    }

    #[test]
    fn finds_styles_and_formats_loosely() {
        assert_eq!(style("/Screenprint").unwrap().name, "Screenprint poster");
        assert_eq!(style(" anime ").unwrap().id, "anime");
        assert!(style("vaporwave").is_none());
        assert_eq!(format("/story").unwrap().aspect, "9:16");
        assert_eq!(format_for_aspect("4:5").unwrap().id, "portrait");
        assert!(format_for_aspect("5:4").is_none());
    }

    #[test]
    fn orientation_follows_the_ratio() {
        assert_eq!(Orientation::of(None), Orientation::Square);
        assert_eq!(Orientation::of(Some("1:1")), Orientation::Square);
        assert_eq!(Orientation::of(Some("4:5")), Orientation::Portrait);
        assert_eq!(Orientation::of(Some("21:9")), Orientation::Landscape);
        assert_eq!(Orientation::of(Some("wide")), Orientation::Square);
        assert_eq!(ratio(Some("16:9")), 16. / 9.);
        assert_eq!(ratio(Some("0:4")), 1.);
    }

    #[test]
    fn picking_a_preset_switches_it_in_the_composer() {
        assert_eq!(with_preset("", "anime").unwrap(), "/anime ");
        assert_eq!(
            with_preset("a fox in the snow", "/Screenprint").unwrap(),
            "/screenprint a fox in the snow "
        );
        // A second style replaces the first; a format sits after the styles.
        let text = with_preset("/anime a fox", "watercolor").unwrap();
        assert_eq!(text, "/watercolor a fox ");
        let text = with_preset(&text, "story").unwrap();
        assert_eq!(text, "/watercolor /story a fox ");
        assert_eq!(
            with_preset(&text, "portrait").unwrap(),
            "/watercolor /portrait a fox "
        );
        // Other slash words and unknown ids are left alone.
        assert_eq!(with_preset("/plan x", "pixel").unwrap(), "/pixel /plan x ");
        assert!(with_preset("x", "vaporwave").is_none());
    }

    #[test]
    fn a_draft_names_its_looks_and_formats_once_in_order() {
        let picked = picked("/story a fox /Screenprint /plan /story http://x/y");
        assert_eq!(
            picked,
            [
                Picked {
                    glyph: "image-01",
                    name: "Story 9:16".into(),
                    note: "Stories, Reels, TikTok, phone wallpaper",
                },
                Picked {
                    glyph: "palette",
                    name: "Screenprint poster".into(),
                    note: "Two or three inks, halftone, aged paper",
                },
            ]
        );
        assert!(super::picked("a fox").is_empty());
    }

    #[test]
    fn finds_whole_preset_words_only() {
        let text = "/screenprint a fox, /Story. Not a/pixel or /pixels or https://x/anime";
        let found: Vec<&str> = preset_ranges(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["/screenprint", "/Story"]);
        let text = "(/anime) y /plan";
        let found: Vec<&str> = preset_ranges(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["/anime"]);
        assert!(preset_ranges("/").is_empty());
        assert!(preset_ranges("ñ/anime").is_empty());
    }

    #[test]
    fn the_catalog_names_every_style_and_format() {
        let rules = catalog_rules();
        assert!(rules.starts_with("<image_styles>") && rules.ends_with("</image_styles>"));
        for style in STYLES {
            assert!(rules.contains(&format!("/{} (", style.id)), "{}", style.id);
        }
        assert!(rules.contains("/portrait = 4:5"));
        assert!(rules.contains("/ultrawide = 21:9 (Banners and wide headers, also /cinema)"));
    }

    #[test]
    fn formats_are_named_after_themselves() {
        for format in FORMATS {
            assert_eq!(format.id, format.name.to_ascii_lowercase());
        }
    }

    #[test]
    fn an_old_format_word_still_means_the_same_ratio() {
        let old = format("/cinema").unwrap();
        assert_eq!(old.id, "ultrawide");
        assert_eq!(old.aspect, "21:9");
        assert_eq!(format("Cinema"), format("ultrawide"));
        assert_eq!(preset_id("/cinema"), Some("ultrawide"));
        assert_eq!(preset_id("cinematic"), Some("cinematic"));
        assert_eq!(preset_id("plan"), None);
        // Typed in a prompt, it is still a preset word and still names the format.
        let text = "/pixel /cinema a pink lake";
        let found: Vec<&str> = preset_ranges(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["/pixel", "/cinema"]);
        assert_eq!(picked(text)[1].name, "Ultrawide 21:9");
        // Picking a format switches the old word for the new one.
        assert_eq!(
            with_preset(text, "landscape").unwrap(),
            "/pixel /landscape a pink lake "
        );
        assert_eq!(
            with_preset("a pink lake", "/cinema").unwrap(),
            "/ultrawide a pink lake "
        );
    }
}
