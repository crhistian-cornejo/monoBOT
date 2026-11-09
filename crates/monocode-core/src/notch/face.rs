//! The face the notch wears at rest: two eyes that blink now and then,
//! glance toward the pointer and cheer when an agent finishes. Only the
//! arithmetic lives here; the notch draws it.

/// How long a blink takes, in seconds.
pub const BLINK: f32 = 0.2;
/// How long the eyes stay glad after an agent finishes, in seconds.
pub const CHEER: f32 = 4.;

/// How open the eyes are `since` seconds into a blink: closing over its
/// first half, opening over the second; 1 outside it.
pub fn openness(since: f32) -> f32 {
    if !(0. ..BLINK).contains(&since) {
        return 1.;
    }
    let half = BLINK / 2.;
    ((since - half).abs() / half).clamp(0., 1.)
}

/// Where the eyes look for a pointer at `pointer` on a screen `width`
/// wide, with the face at `center`: -1 (left) to 1 (right), reaching the
/// full glance a third of the screen away.
pub fn gaze(pointer: f32, center: f32, width: f32) -> f32 {
    if width <= 0. {
        return 0.;
    }
    ((pointer - center) / (width / 3.)).clamp(-1., 1.)
}

/// The glance while a blink changes it: the eyes move while they are shut,
/// so `from` holds until the blink's middle and `to` after.
pub fn glance(from: f32, to: f32, since: f32) -> f32 {
    if since < BLINK / 2. {
        from
    } else {
        to
    }
}

/// How often the eyes look for the pointer, in seconds.
pub const FOLLOW: f32 = 0.08;

/// The glance one look later, on its way from `shown` to where the pointer
/// is (`toward`): most of the way at once and the rest easing in, so the
/// eyes trail a fast pointer a little and rest exactly on a still one.
pub fn follow(shown: f32, toward: f32) -> f32 {
    let next = shown + (toward - shown) * 0.45;
    if (toward - next).abs() < 0.02 {
        toward
    } else {
        next
    }
}

/// The wait before blink number `count`, in seconds: uneven, as eyes are,
/// between three and six seconds.
pub fn wait(count: u32) -> f32 {
    3. + ((count.wrapping_mul(2_654_435_761) >> 16) % 3000) as f32 / 1000.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blink_shuts_and_opens_the_eyes() {
        assert_eq!(openness(-1.), 1.);
        assert_eq!(openness(0.), 1.);
        assert!(openness(BLINK / 2.) < 0.01);
        assert!((openness(BLINK * 0.75) - 0.5).abs() < 0.01);
        assert_eq!(openness(BLINK), 1.);
    }

    #[test]
    fn the_eyes_follow_the_pointer_and_move_while_shut() {
        assert_eq!(gaze(960., 960., 1920.), 0.);
        assert_eq!(gaze(0., 960., 1920.), -1.);
        assert_eq!(gaze(1920., 960., 1920.), 1.);
        assert!((gaze(1280., 960., 1920.) - 0.5).abs() < 0.01);
        assert_eq!(gaze(10., 0., 0.), 0.);
        assert_eq!(glance(-1., 1., 0.), -1.);
        assert_eq!(glance(-1., 1., BLINK * 0.6), 1.);
    }

    #[test]
    fn the_eyes_trail_the_pointer_and_come_to_rest_on_it() {
        let first = follow(0., 1.);
        assert!(first > 0.4 && first < 1.);
        let mut shown = 0.;
        let mut looks = 0;
        while shown != 1. {
            shown = follow(shown, 1.);
            looks += 1;
            assert!(looks < 12, "it settles within a second");
        }
        // A still pointer moves nothing: no frame is asked for.
        assert_eq!(follow(-0.3, -0.3), -0.3);
    }

    #[test]
    fn blinks_come_unevenly_every_few_seconds() {
        let waits: Vec<f32> = (0..20).map(wait).collect();
        assert!(waits.iter().all(|wait| (3. ..6.).contains(wait)));
        assert!(waits.windows(2).any(|pair| (pair[0] - pair[1]).abs() > 0.3));
    }
}
