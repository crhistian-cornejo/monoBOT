//! "While dictating" (Settings › Voice): leave what plays alone, pause it,
//! or lower the output's volume while a dictation listens, and put it back
//! once the last dictation ends. Both the composer's mic and the system
//! dictation follow it; a call's mic does not.
//!
//! [`Quiet`] counts the dictations that run and says when the first one
//! should quiet things and what the last one has to undo. Media is paused
//! per player ([`to_pause`], [`to_resume`]), so a player the user started
//! or stopped meanwhile is left as the user left it. The volume goes
//! through [`Ducking`], which puts it back only while it is still the
//! level it set.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What happens to the computer's sound while a dictation listens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum WhileDictating {
    /// Leave it alone.
    #[default]
    Nothing,
    /// Pause every player that plays, and play them again after.
    PauseMedia,
    /// Lower the output's volume, and raise it again after.
    LowerVolume,
}

impl WhileDictating {
    pub const ALL: [Self; 3] = [Self::Nothing, Self::PauseMedia, Self::LowerVolume];

    /// How Settings stores it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Nothing => "nothing",
            Self::PauseMedia => "pause-media",
            Self::LowerVolume => "lower-volume",
        }
    }

    /// Anything Settings does not know (a later version's choice) reads as
    /// doing nothing.
    pub fn from_id(id: &str) -> Self {
        match id {
            "pause-media" => Self::PauseMedia,
            "lower-volume" => Self::LowerVolume,
            _ => Self::Nothing,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Nothing => "Do nothing",
            Self::PauseMedia => "Pause media",
            Self::LowerVolume => "Lower volume",
        }
    }
}

impl From<String> for WhileDictating {
    fn from(id: String) -> Self {
        Self::from_id(&id)
    }
}

impl From<WhileDictating> for String {
    fn from(choice: WhileDictating) -> Self {
        choice.id().to_owned()
    }
}

/// What the first dictation asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    PauseMedia,
    LowerVolume,
}

/// What was done, for the last dictation to undo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Held {
    /// These players were paused (by the app that plays, as the system
    /// names it).
    Paused(Vec<String>),
    /// The volume was lowered (the [`Ducking`] keeps by how much).
    Lowered,
}

/// The dictations that run now (each by an id the app gives it: a
/// composer, the system dictation) and what they quieted.
#[derive(Clone, Debug, Default)]
pub struct Quiet {
    active: BTreeSet<u64>,
    held: Option<Held>,
    /// A [`Want`] was handed out and its [`Held`] has not come back yet.
    pending: bool,
}

impl Quiet {
    /// Dictation `who` starts listening. Returns what to do when it is the
    /// first one and the choice is to quiet something; a dictation already
    /// counted, one that starts beside another, or one that starts while
    /// the last quieting is still being done (it then owns that one),
    /// changes nothing.
    pub fn begin(&mut self, who: u64, choice: WhileDictating) -> Option<Want> {
        let first = self.active.is_empty();
        if !self.active.insert(who) || !first || self.held.is_some() || self.pending {
            return None;
        }
        let want = match choice {
            WhileDictating::Nothing => None,
            WhileDictating::PauseMedia => Some(Want::PauseMedia),
            WhileDictating::LowerVolume => Some(Want::LowerVolume),
        };
        self.pending = want.is_some();
        want
    }

    /// What was done for the [`Want`] that [`Quiet::begin`] returned. It is
    /// kept while a dictation runs; when every dictation ended before it
    /// was done, it comes back, to be undone at once.
    pub fn hold(&mut self, held: Held) -> Option<Held> {
        self.pending = false;
        if self.active.is_empty() {
            return Some(held);
        }
        self.held = Some(held);
        None
    }

    /// Dictation `who` ended (stopped, cancelled, failed or closed).
    /// Returns what to undo when it was the last one; an id that never
    /// began, or ended already, changes nothing.
    pub fn end(&mut self, who: u64) -> Option<Held> {
        if !self.active.remove(&who) || !self.active.is_empty() {
            return None;
        }
        self.held.take()
    }

    /// `who` is listening or not: begins or ends it, as [`Quiet::begin`]
    /// and [`Quiet::end`] do. Returns what to do then.
    pub fn follow(&mut self, who: u64, dictating: bool, choice: WhileDictating) -> Option<Step> {
        if dictating {
            self.begin(who, choice).map(Step::Quiet)
        } else {
            self.end(who).map(Step::Undo)
        }
    }

    /// Whether any dictation runs.
    pub fn active(&self) -> bool {
        !self.active.is_empty()
    }
}

/// What [`Quiet::follow`] asks the app to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Quiet(Want),
    Undo(Held),
}

/// The players to pause: each one that plays now, once (players are
/// `(app, playing)`).
pub fn to_pause<'a>(players: impl IntoIterator<Item = (&'a str, bool)>) -> Vec<String> {
    let mut apps: Vec<String> = Vec::new();
    for (app, playing) in players {
        if playing && !app.is_empty() && !apps.iter().any(|kept| kept == app) {
            apps.push(app.to_owned());
        }
    }
    apps
}

/// The paused players to play again: those still there and still paused.
/// One the user played meanwhile is left playing (a toggle would pause it),
/// and one that went away is forgotten.
pub fn to_resume<'a>(
    paused: &[String],
    players: impl IntoIterator<Item = (&'a str, bool)>,
) -> Vec<String> {
    let players: Vec<(&str, bool)> = players.into_iter().collect();
    paused
        .iter()
        .filter(|app| {
            players
                .iter()
                .any(|(there, playing)| there == app && !playing)
        })
        .cloned()
        .collect()
}

/// The share of the volume kept while dictating.
pub const LOWER_TO: f32 = 0.3;
/// A change smaller than this is not worth making: the volume is already
/// about as low as it would go.
const LEAST_CHANGE: f32 = 0.02;
/// How far the volume may be from what was set and still count as ours: an
/// output may round what it is given.
const OURS: f32 = 0.02;

/// The volume while dictating, for an output at `volume`; `None` when it is
/// already about that quiet.
pub fn lowered(volume: f32) -> Option<f32> {
    let volume = volume.clamp(0., 1.);
    let to = volume * LOWER_TO;
    (volume - to >= LEAST_CHANGE).then_some(to)
}

/// Lowers the volume once and puts it back once, remembering the level
/// before and the level set. It runs where the volume is read and set, in
/// order, so a restore always follows its own lowering.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ducking {
    held: Option<(f32, f32)>,
}

impl Ducking {
    /// The output reads `now` (volume, muted): the level to set, or `None`
    /// when there is nothing to lower (muted, already quiet, unreadable,
    /// or already lowered).
    pub fn lower(&mut self, now: Option<(f32, bool)>) -> Option<f32> {
        if self.held.is_some() {
            return None;
        }
        let (volume, muted) = now?;
        if muted {
            return None;
        }
        let to = lowered(volume)?;
        self.held = Some((volume.clamp(0., 1.), to));
        Some(to)
    }

    /// The output did not take the level [`Ducking::lower`] returned.
    pub fn not_set(&mut self) {
        self.held = None;
    }

    /// The dictations ended and the output reads `now`: the level to put
    /// back, or `None` when nothing was lowered or the user changed the
    /// volume since (their level stays).
    pub fn restore(&mut self, now: Option<(f32, bool)>) -> Option<f32> {
        let (from, to) = self.held.take()?;
        let (volume, _) = now?;
        ((volume - to).abs() <= OURS).then_some(from)
    }

    /// Whether the volume is lowered now.
    pub fn lowered(&self) -> bool {
        self.held.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_stored_by_id_and_unknown_ids_do_nothing() {
        for choice in WhileDictating::ALL {
            assert_eq!(WhileDictating::from_id(choice.id()), choice);
            let json = serde_json::to_string(&choice).unwrap();
            assert_eq!(json, format!("\"{}\"", choice.id()));
            assert_eq!(
                serde_json::from_str::<WhileDictating>(&json).unwrap(),
                choice
            );
        }
        assert_eq!(
            serde_json::from_str::<WhileDictating>("\"duck-to-zero\"").unwrap(),
            WhileDictating::Nothing
        );
        assert_eq!(WhileDictating::default(), WhileDictating::Nothing);
        assert_eq!(WhileDictating::PauseMedia.label(), "Pause media");
    }

    #[test]
    fn the_first_dictation_quiets_and_the_last_one_undoes() {
        let mut quiet = Quiet::default();
        assert_eq!(
            quiet.begin(1, WhileDictating::PauseMedia),
            Some(Want::PauseMedia)
        );
        assert_eq!(quiet.hold(Held::Paused(vec!["Spotify".into()])), None);
        // A second dictation beside it, and the first one again, do nothing.
        assert_eq!(quiet.begin(2, WhileDictating::PauseMedia), None);
        assert_eq!(quiet.begin(1, WhileDictating::PauseMedia), None);
        assert_eq!(quiet.end(1), None);
        assert!(quiet.active());
        assert_eq!(quiet.end(2), Some(Held::Paused(vec!["Spotify".into()])));
        assert!(!quiet.active());
        // Ending again, or something that never began, changes nothing.
        assert_eq!(quiet.end(2), None);
        assert_eq!(quiet.end(7), None);
    }

    #[test]
    fn doing_nothing_holds_nothing_and_the_choice_is_read_at_the_start() {
        let mut quiet = Quiet::default();
        assert_eq!(quiet.begin(1, WhileDictating::Nothing), None);
        // Changed in Settings mid-dictation: this one keeps its start.
        assert_eq!(quiet.begin(2, WhileDictating::LowerVolume), None);
        assert_eq!(quiet.end(1), None);
        assert_eq!(quiet.end(2), None);
        // The next one lowers, and its undo is the lowering, whatever the
        // setting says when it ends.
        assert_eq!(
            quiet.follow(3, true, WhileDictating::LowerVolume),
            Some(Step::Quiet(Want::LowerVolume))
        );
        assert_eq!(quiet.hold(Held::Lowered), None);
        assert_eq!(
            quiet.follow(3, false, WhileDictating::Nothing),
            Some(Step::Undo(Held::Lowered))
        );
    }

    #[test]
    fn a_quieting_done_after_every_dictation_ended_is_undone_at_once() {
        let mut quiet = Quiet::default();
        assert_eq!(
            quiet.begin(1, WhileDictating::PauseMedia),
            Some(Want::PauseMedia)
        );
        assert_eq!(quiet.end(1), None, "nothing done yet");
        let paused = Held::Paused(vec!["Music".into()]);
        assert_eq!(quiet.hold(paused.clone()), Some(paused));
        assert_eq!(
            quiet.begin(2, WhileDictating::PauseMedia),
            Some(Want::PauseMedia)
        );
    }

    #[test]
    fn a_dictation_that_starts_while_the_last_quieting_is_done_owns_it() {
        let mut quiet = Quiet::default();
        assert_eq!(
            quiet.begin(1, WhileDictating::LowerVolume),
            Some(Want::LowerVolume)
        );
        assert_eq!(quiet.end(1), None);
        // Not lowered twice: the next dictation takes the first lowering.
        assert_eq!(quiet.begin(2, WhileDictating::LowerVolume), None);
        assert_eq!(quiet.hold(Held::Lowered), None);
        assert_eq!(quiet.end(2), Some(Held::Lowered));
    }

    #[test]
    fn only_players_that_play_are_paused_and_only_those_still_paused_resume() {
        let now = [
            ("Spotify", true),
            ("chrome", false),
            ("Spotify", true),
            ("", true),
            ("vlc", true),
        ];
        let paused = to_pause(now);
        assert_eq!(paused, ["Spotify", "vlc"]);
        // The user played vlc again by hand, and chrome was never ours.
        let later = [("Spotify", false), ("vlc", true), ("chrome", false)];
        assert_eq!(to_resume(&paused, later), ["Spotify"]);
        // A player that went away is not asked.
        assert!(to_resume(&paused, [("chrome", false)]).is_empty());
        assert!(to_pause([("Spotify", false)]).is_empty());
    }

    #[test]
    fn the_volume_goes_down_to_a_share_and_back_while_it_is_still_ours() {
        assert_eq!(lowered(0.5), Some(0.5 * LOWER_TO));
        assert_eq!(lowered(0.02), None, "already about that quiet");
        assert_eq!(lowered(1.4), Some(LOWER_TO), "clamped to the range");

        let mut ducking = Ducking::default();
        assert_eq!(ducking.lower(Some((0.8, false))), Some(0.8 * LOWER_TO));
        assert!(ducking.lowered());
        // Lowering twice does not lower the lowered level.
        assert_eq!(ducking.lower(Some((0.24, false))), None);
        assert_eq!(ducking.restore(Some((0.24 + 0.01, false))), Some(0.8));
        assert!(!ducking.lowered());
        assert_eq!(ducking.restore(Some((0.24, false))), None, "once");
    }

    #[test]
    fn a_volume_the_user_changed_muted_or_unreadable_is_left_alone() {
        let mut ducking = Ducking::default();
        assert_eq!(ducking.lower(Some((0.6, true))), None, "muted");
        assert_eq!(ducking.lower(None), None, "no output");
        assert_eq!(ducking.restore(Some((0.6, false))), None);

        ducking.lower(Some((0.6, false)));
        assert_eq!(ducking.restore(Some((0.5, false))), None, "user moved it");
        assert!(!ducking.lowered());

        ducking.lower(Some((0.6, false)));
        assert_eq!(ducking.restore(None), None, "output went away");

        ducking.lower(Some((0.6, false)));
        ducking.not_set();
        assert!(!ducking.lowered());
        assert_eq!(ducking.restore(Some((0.18, false))), None);
    }
}
