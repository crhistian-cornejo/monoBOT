//! Preset colors from workspaceThemes.ts, including surface tints and accents.
//! Themes named after an editor theme take their sidebar and editor colors
//! from it (VS Code's Monokai, Solarized and One themes, Catppuccin, Primer).
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    /// The window and sidebar: the chrome around the main pane.
    pub surface: u32,
    /// The main pane. Darker than `surface` in dark mode, lighter in light.
    pub panel: u32,
    pub solid: u32,
    pub content: u32,
    pub accent: u32,
}
pub(crate) const PALETTES: &[(&str, &str, Palette, Palette)] = &[
    (
        "default",
        "Default",
        // Tauri's Factory look (5b15a18e): orange accent; light mode with a
        // white workspace, #f9f9f9 inset surface and near-black (9%) text.
        Palette {
            surface: 0x0d0d0d,
            panel: 0x060606,
            solid: 0x141414,
            content: 0xebebeb,
            accent: 0xef6f2e,
        },
        Palette {
            surface: 0xf9f9f9,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x171717,
            accent: 0xef6f2e,
        },
    ),
    (
        "harbor",
        "Harbor",
        Palette {
            surface: 0x0b1018,
            panel: 0x060a10,
            solid: 0x121a26,
            content: 0xdbe4f0,
            accent: 0x5b9cf2,
        },
        Palette {
            surface: 0xeef2f7,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x1b2533,
            accent: 0x2f6fd0,
        },
    ),
    (
        "moss",
        "Moss",
        Palette {
            surface: 0x0d110e,
            panel: 0x070a08,
            solid: 0x151b17,
            content: 0xdde5dc,
            accent: 0x6fbf73,
        },
        Palette {
            surface: 0xf1f4ee,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x1f2a20,
            accent: 0x3d8a45,
        },
    ),
    (
        "ember",
        "Ember",
        Palette {
            surface: 0x120e0c,
            panel: 0x0a0706,
            solid: 0x1c1613,
            content: 0xecdfd6,
            accent: 0xf08a4b,
        },
        Palette {
            surface: 0xf7f2ee,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x2d211b,
            accent: 0xc75c1e,
        },
    ),
    (
        "orchid",
        "Orchid",
        Palette {
            surface: 0x110d16,
            panel: 0x09070c,
            solid: 0x1a1522,
            content: 0xe6def0,
            accent: 0xb57cf0,
        },
        Palette {
            surface: 0xf4f0f8,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x261e33,
            accent: 0x8048c8,
        },
    ),
    (
        "tidepool",
        "Tidepool",
        Palette {
            surface: 0x0a1213,
            panel: 0x050a0b,
            solid: 0x111c1e,
            content: 0xd8e8e8,
            accent: 0x3fc1b5,
        },
        Palette {
            surface: 0xedf5f5,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x162a2b,
            accent: 0x138a80,
        },
    ),
    (
        "dune",
        "Dune",
        Palette {
            surface: 0x13110d,
            panel: 0x0b0a07,
            solid: 0x1d1a14,
            content: 0xebe3d2,
            accent: 0xd9b25c,
        },
        Palette {
            surface: 0xf6f1e7,
            panel: 0xfffdf8,
            solid: 0xfffdf8,
            content: 0x2e2819,
            accent: 0xa57d1e,
        },
    ),
    (
        "rosewood",
        "Rosewood",
        Palette {
            surface: 0x140e10,
            panel: 0x0b0809,
            solid: 0x1e1518,
            content: 0xefdfe3,
            accent: 0xe7798f,
        },
        Palette {
            surface: 0xf8f0f2,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x2f1f24,
            accent: 0xc0405c,
        },
    ),
    (
        "glacier",
        "Glacier",
        Palette {
            surface: 0x0e1114,
            panel: 0x080a0c,
            solid: 0x161a1e,
            content: 0xe1e7ec,
            accent: 0x7fb4d9,
        },
        Palette {
            surface: 0xf2f5f8,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x20262d,
            accent: 0x3a7aa8,
        },
    ),
    (
        "arcade",
        "Arcade",
        Palette {
            surface: 0x0f0b1a,
            panel: 0x08060f,
            solid: 0x181229,
            content: 0xece6ff,
            accent: 0xff5fd2,
        },
        Palette {
            surface: 0xf5f2fc,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x231a3a,
            accent: 0xc2189a,
        },
    ),
    (
        "ink",
        "Ink",
        Palette {
            // True black stays in the main pane; the sidebar lifts off it.
            surface: 0x0a0a0a,
            panel: 0x000000,
            solid: 0x0a0a0a,
            content: 0xf5f5f5,
            accent: 0x7aa7ff,
        },
        Palette {
            surface: 0xffffff,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x000000,
            accent: 0x0050d8,
        },
    ),
    (
        "monokai",
        "Monokai",
        Palette {
            surface: 0x272822,
            panel: 0x1e1f1c,
            solid: 0x272822,
            content: 0xf8f8f2,
            accent: 0xf92672,
        },
        Palette {
            surface: 0xf8f8f2,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x272822,
            accent: 0xf92672,
        },
    ),
    (
        "github",
        "GitHub",
        Palette {
            surface: 0x0d1117,
            panel: 0x010409,
            solid: 0x161b22,
            content: 0xe6edf3,
            accent: 0x2f81f7,
        },
        Palette {
            surface: 0xf6f8fa,
            panel: 0xffffff,
            solid: 0xffffff,
            content: 0x1f2328,
            accent: 0x0969da,
        },
    ),
    (
        "solarized",
        "Solarized",
        Palette {
            surface: 0x002b36,
            panel: 0x00212b,
            solid: 0x073642,
            content: 0x93a1a1,
            accent: 0x268bd2,
        },
        Palette {
            surface: 0xeee8d5,
            panel: 0xfdf6e3,
            solid: 0xfdf6e3,
            content: 0x586e75,
            accent: 0x268bd2,
        },
    ),
    (
        "latte",
        "Latte",
        Palette {
            surface: 0x1e1e2e,
            panel: 0x181825,
            solid: 0x1e1e2e,
            content: 0xcdd6f4,
            accent: 0xcba6f7,
        },
        Palette {
            surface: 0xe6e9ef,
            panel: 0xeff1f5,
            solid: 0xeff1f5,
            content: 0x4c4f69,
            accent: 0x8839ef,
        },
    ),
    (
        "one-light",
        "One Light",
        Palette {
            surface: 0x282c34,
            panel: 0x21252b,
            solid: 0x2c313a,
            content: 0xabb2bf,
            accent: 0x61afef,
        },
        Palette {
            surface: 0xeaeaeb,
            panel: 0xfafafa,
            solid: 0xffffff,
            content: 0x383a42,
            accent: 0x4078f2,
        },
    ),
    (
        "paper",
        "Paper",
        Palette {
            surface: 0x1c1917,
            panel: 0x141210,
            solid: 0x24201c,
            content: 0xede6de,
            accent: 0xe07a5f,
        },
        Palette {
            surface: 0xf4ecd8,
            panel: 0xfaf5e9,
            solid: 0xfaf5e9,
            content: 0x433422,
            accent: 0xb85434,
        },
    ),
];
pub(crate) fn palette(id: &str, light: bool) -> Palette {
    let row = PALETTES.iter().find(|r| r.0 == id).unwrap_or(&PALETTES[0]);
    if light { row.3 } else { row.2 }
}
/// The tint picking preset `id` gives the color sliders: its dark surface as
/// hue, saturation and dark-mode lightness.
pub(crate) fn preset_tint(id: &str) -> (u32, u32, u32) {
    let color: gpui::Hsla = gpui::rgb(palette(id, false).surface).into();
    (
        (color.h * 360.).round() as u32 % 360,
        (color.s * 100.).round() as u32,
        ((color.l * 100.).round() as u32).min(30),
    )
}
/// Whether the sliders still hold preset `id`'s own tint. Then the preset's
/// sidebar and main-pane colors paint as designed in both modes; a tint the
/// user moved paints from the sliders instead.
pub(crate) fn is_preset_tint(id: &str, hue: u32, saturation: u32, dark_lightness: u32) -> bool {
    let (h, s, l) = preset_tint(id);
    s == saturation && l == dark_lightness && (s == 0 || h == hue % 360)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: u32) -> f32 {
        let gpui::Rgba { r, g, b, .. } = gpui::rgb(color);
        let linear = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }
    /// The sidebar as painted: the surface under the chrome's 5% ink wash.
    fn sidebar(p: Palette) -> u32 {
        let (s, c) = (gpui::rgb(p.surface), gpui::rgb(p.content));
        let mix = |a: f32, b: f32| ((a + (b - a) * 0.05) * 255.).round() as u32;
        mix(s.r, c.r) << 16 | mix(s.g, c.g) << 8 | mix(s.b, c.b)
    }

    #[test]
    fn every_preset_sets_its_sidebar_apart_from_the_main_pane() {
        for &(id, _, dark, light) in PALETTES {
            for (mode, p) in [("dark", dark), ("light", light)] {
                let (side, main) = (luminance(sidebar(p)), luminance(p.panel));
                let contrast = (side.max(main) + 0.05) / (side.min(main) + 0.05);
                assert!(
                    contrast >= 1.1,
                    "{id} {mode}: sidebar and main pane read alike ({contrast:.2})"
                );
                // Dark main panes sink below the chrome; light ones lift above it.
                assert_eq!(
                    main < side,
                    mode == "dark",
                    "{id} {mode}: main pane on the wrong side"
                );
            }
        }
    }

    #[test]
    fn a_preset_recognizes_its_own_tint_until_the_sliders_move() {
        for &(id, ..) in PALETTES {
            let (h, s, l) = preset_tint(id);
            assert!(is_preset_tint(id, h, s, l), "{id}");
            assert!(!is_preset_tint(id, h, s, (l + 3) % 31), "{id}");
        }
        let (h, s, l) = preset_tint("solarized");
        assert!(!is_preset_tint("solarized", (h + 40) % 360, s, l));
        // Hue means nothing to a neutral grey.
        let (_, s, l) = preset_tint("default");
        assert!(is_preset_tint("default", 240, s, l));
    }
}
