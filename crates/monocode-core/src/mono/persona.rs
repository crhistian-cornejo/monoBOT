//! A personal bot's 3D face. The renderer is its own crate, `monobot_persona`,
//! because the phone links it too and draws the same faces itself.

pub use monobot_persona::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn look(mascot: &str, role: &str) -> Look {
        let (species, outfit) = parse(&key(mascot, Some(role))).unwrap();
        Look {
            species,
            outfit,
            color: 0x38bdf8,
        }
    }

    #[test]
    fn a_bot_wears_a_persona_and_every_other_mono_its_pixels() {
        let mut mono = crate::mono::Mono {
            mascot: "cat".into(),
            ..Default::default()
        };
        assert_eq!(mono.face(), "cat");
        mono.kind = Some("bot".into());
        mono.bot_role = Some("writer".into());
        assert_eq!(mono.face(), "3d:cat:writer");
        // The look picker shows what each mascot would become.
        assert_eq!(mono.face_as("frog"), "3d:frog:writer");
        mono.kind = Some("group".into());
        assert_eq!(mono.face(), "cat");
    }

    #[test]
    fn species_and_outfits_are_told_apart() {
        let side = 40;
        let mascots = crate::project_appearance::MASCOTS;
        let roles: Vec<&str> = crate::mono::bots::ROLES.iter().map(|r| r.role).collect();
        // Bare-headed and under a hat alike: a hat hides what grows on top
        // of a head, never which species wears it.
        for role in ["custom", "finance"] {
            let pictures: Vec<Vec<u8>> = mascots
                .iter()
                .map(|mascot| render(look(mascot, role), Pose::Rest, side))
                .collect();
            for (a, first) in pictures.iter().enumerate() {
                for (b, second) in pictures.iter().enumerate().skip(a + 1) {
                    assert_ne!(first, second, "{role}: {} and {}", mascots[a], mascots[b]);
                }
            }
        }
        // Every role the app offers dresses the ghost differently.
        let dressed: Vec<Vec<u8>> = roles
            .iter()
            .map(|role| render(look("ghost", role), Pose::Rest, side))
            .collect();
        for (a, first) in dressed.iter().enumerate() {
            for (b, second) in dressed.iter().enumerate().skip(a + 1) {
                assert_ne!(first, second, "{} and {}", roles[a], roles[b]);
            }
        }
    }

    /// Writes every species in every outfit to the PNG named by
    /// `MONOBOT_PERSONA_SHEET`, a row per species, then the poses and the
    /// small sizes, to look at them:
    /// `MONOBOT_PERSONA_SHEET=sheet.png cargo test -p monocode-core persona -- --ignored`.
    #[test]
    #[ignore]
    fn writes_a_contact_sheet() {
        let path = std::env::var("MONOBOT_PERSONA_SHEET").expect("MONOBOT_PERSONA_SHEET");
        let side = 112;
        let colors = crate::mono::COLORS_MONO;
        let mascots = crate::project_appearance::MASCOTS;
        let roles: Vec<&str> = crate::mono::bots::ROLES.iter().map(|r| r.role).collect();
        let columns = roles.len() as u32;
        let rows = mascots.len() as u32 + 1;
        let mut sheet = image::RgbaImage::from_pixel(
            columns * side,
            rows * side,
            image::Rgba([36, 38, 46, 255]),
        );
        let mut place = |column: u32, row: u32, picture: image::RgbaImage| {
            image::imageops::overlay(
                &mut sheet,
                &picture,
                (column * side) as i64,
                (row * side) as i64,
            );
        };
        let draw = |look: Look, pose: Pose, at: u32| {
            let picture = image::RgbaImage::from_raw(at, at, render(look, pose, at)).unwrap();
            image::imageops::resize(&picture, side, side, image::imageops::FilterType::Nearest)
        };
        for (row, mascot) in mascots.iter().enumerate() {
            for (column, role) in roles.iter().enumerate() {
                let mut look = look(mascot, role);
                look.color = colors[row % colors.len()];
                place(column as u32, row as u32, draw(look, Pose::Rest, side));
            }
        }
        let last = mascots.len() as u32;
        let extras = [
            (look("cat", "writer"), Pose::Blink, side),
            (look("cat", "writer"), Pose::Talk, side),
            (look("robot", "custom"), Pose::Blink, side),
            (look("robot", "custom"), Pose::Talk, side),
            (look("skull", "finance"), Pose::Talk, side),
            (look("frog", "custom"), Pose::Blink, side),
            (look("cat", "principal"), Pose::Rest, 16),
            (look("robot", "analyst"), Pose::Rest, 16),
            (look("skull", "finance"), Pose::Rest, 16),
            (look("frog", "research"), Pose::Rest, 24),
            (look("mushroom", "inbox"), Pose::Rest, 24),
            (look("crab", "custom"), Pose::Talk, 24),
            (look("ghost", "planner"), Pose::Rest, 32),
        ];
        for (column, (look, pose, at)) in extras.into_iter().enumerate() {
            place(column as u32, last, draw(look, pose, at));
        }
        sheet.save(path).unwrap();
    }
}
