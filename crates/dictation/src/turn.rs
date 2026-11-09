//! When the user has finished their turn in a call, as JOSIE-Voice and IVY
//! tell it (`IVY/crates/rules/src/call.rs`). A pause ends a turn after a
//! length that depends on the chosen pace and on how the words so far end:
//! shorter after a question, longer after «y…», «pero», a comma or a
//! hesitation. Smart Turn, an audio model, adds its verdict when it is
//! installed (`smart_turn`). Also here: what counts as cutting in while the
//! agents speak, and telling the agents' own words heard back from the
//! user's. Pure rules, no audio.

/// How patient the call is: "patient", "normal" or "quick".
pub const PACES: [&str; 3] = ["patient", "normal", "quick"];
/// The least voice a turn needs, so a cough is not a turn.
pub const MIN_TURN_MS: u32 = 300;
/// The longest turn; it ends here even without a pause.
pub const MAX_TURN_MS: u32 = 30_000;
/// Smart Turn is asked this far into a pause, so its verdict is back before
/// the pause can end.
pub const ASK_AFTER_MS: u32 = 200;
/// At or above this, Smart Turn finds the turn complete.
pub const COMPLETE_AT: f32 = 0.5;
/// How much longer an unfinished turn waits.
pub const UNFINISHED_EXTRA_MS: u32 = 1_000;
/// How much voice over the agents' cuts them off, with words in it.
pub const BARGE_IN_MS: u32 = 1_000;
/// Share of heard words that, being the agents' own, make it their echo.
const ECHO_WORDS: f32 = 0.6;

/// The pause that ends a turn at `pace`, before the words are weighed.
pub fn base_pause_ms(pace: &str) -> u32 {
    match pace {
        "patient" => 1_400,
        "quick" => 450,
        _ => 800,
    }
}

/// Words a sentence does not end on, and hesitations: after one the user is
/// still thinking.
const OPEN_ENDINGS: [&str; 52] = [
    "y", "e", "o", "u", "pero", "que", "porque", "pues", "entonces", "de", "del", "a", "al", "con",
    "para", "por", "en", "el", "la", "los", "las", "un", "una", "mi", "este", "eh", "ehh", "em",
    "emm", "mm", "mmm", "sea", "osea", "digamos", "tipo", "and", "but", "or", "so", "because",
    "the", "an", "to", "of", "with", "for", "um", "uh", "uhm", "like", "my", "if",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// A question or an exclamation: closed for sure. A full stop says
    /// little; the recognizer ends every phrase with one.
    Closed,
    /// A comma, an ellipsis or a word a sentence does not end on.
    Open,
    Unknown,
}

pub fn ending(partial: &str) -> Ending {
    let text = partial.trim_end();
    if text.is_empty() {
        return Ending::Unknown;
    }
    if text.ends_with("...") || text.ends_with(['…', ',', ';', ':']) {
        return Ending::Open;
    }
    let last = |text: &str| {
        text.rsplit(|c: char| !c.is_alphanumeric())
            .find(|word| !word.is_empty())
            .unwrap_or("")
            .to_lowercase()
    };
    if text.ends_with(['.', '?', '!']) {
        if OPEN_ENDINGS.contains(&last(text.trim_end_matches(['.', '?', '!'])).as_str()) {
            return Ending::Open;
        }
        return if text.ends_with(['?', '!']) {
            Ending::Closed
        } else {
            Ending::Unknown
        };
    }
    if OPEN_ENDINGS.contains(&last(text).as_str()) {
        Ending::Open
    } else {
        Ending::Unknown
    }
}

/// The pause that ends the turn from the words alone.
pub fn pause_ms(pace: &str, partial: &str) -> u32 {
    let base = base_pause_ms(pace);
    match ending(partial) {
        Ending::Closed => (base * 3 / 5).max(350),
        Ending::Open => base * 9 / 5,
        Ending::Unknown => base,
    }
}

/// The pause once Smart Turn has spoken (`complete`, none until then or when
/// it is not installed): an unfinished turn waits longer; the words' pause is
/// never shortened.
pub fn pause_ms_with(pace: &str, partial: &str, complete: Option<f32>) -> u32 {
    match complete {
        Some(probability) if probability < COMPLETE_AT => {
            pause_ms(pace, partial) + UNFINISHED_EXTRA_MS
        }
        _ => pause_ms(pace, partial),
    }
}

/// Whether a turn is done: `voiced_ms` of voice in it, `silence_ms` of quiet
/// since its last voice, `turn_ms` since it began.
pub fn turn_done(
    pace: &str,
    partial: &str,
    voiced_ms: u32,
    silence_ms: u32,
    turn_ms: u32,
    complete: Option<f32>,
) -> bool {
    voiced_ms >= MIN_TURN_MS
        && (silence_ms >= pause_ms_with(pace, partial, complete) || turn_ms >= MAX_TURN_MS)
}

/// What a listener says to keep the talk going without taking the word.
/// «No» is not one: alone it objects.
const BACKCHANNEL: [&str; 42] = [
    "si", "aja", "aha", "aham", "mhm", "mjm", "mm", "mmm", "hm", "hmm", "ok", "okay", "oki",
    "vale", "claro", "ya", "bien", "eso", "exacto", "ah", "oh", "eh", "uh", "um", "uhm", "bueno",
    "listo", "perfecto", "dale", "genial", "entiendo", "cierto", "correcto", "sip", "yes", "yeah",
    "yep", "right", "sure", "cool", "great", "got",
];

/// Only a backchannel, or a laugh: it neither cuts the agents nor is a turn.
pub fn backchannel(heard: &str) -> bool {
    let words = words(heard);
    let laugh =
        |word: &str| word.len() >= 4 && word.chars().all(|c| matches!(c, 'j' | 'a' | 'e' | 'h'));
    !words.is_empty()
        && words.len() <= 3
        && words
            .iter()
            .all(|word| BACKCHANNEL.contains(&word.as_str()) || laugh(word))
}

/// Whether what the user says over the agents stops them: words, and not
/// only a backchannel. A cough or a breath never cuts.
pub fn cuts_in(partial: &str) -> bool {
    !words(partial).is_empty() && !backchannel(partial)
}

/// Whether `heard` is mostly the agents' own words (`said`), heard back.
pub fn echo_of(heard: &str, said: &str) -> bool {
    let heard = words(heard);
    let said: std::collections::HashSet<String> = words(said).into_iter().collect();
    if heard.is_empty() || said.is_empty() {
        return false;
    }
    let shared = heard.iter().filter(|word| said.contains(*word)).count();
    shared as f32 >= heard.len() as f32 * ECHO_WORDS
}

fn fold(word: &str) -> String {
    word.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            other => other,
        })
        .collect()
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(fold)
        .collect()
}

/// Follows one call turn as the voice detector reports it, in milliseconds
/// of audio, and says when it is done. The detector reports quiet only once
/// `lag_ms` of silence has passed, so that much is added to the silence it
/// measures.
#[derive(Clone, Debug)]
pub struct Tracker {
    pace: String,
    lag_ms: u32,
    /// When the turn's first voice was heard.
    started: Option<u64>,
    voiced_ms: u32,
    /// When the detector last heard voice, and whether it hears it now.
    last_voice: u64,
    speaking: bool,
    /// Smart Turn's verdict on the current pause, once asked.
    complete: Option<f32>,
}

impl Tracker {
    pub fn new(pace: &str, lag_ms: u32) -> Self {
        Self {
            pace: pace.to_owned(),
            lag_ms,
            started: None,
            voiced_ms: 0,
            last_voice: 0,
            speaking: false,
            complete: None,
        }
    }

    /// `ms` more audio went by at `now`, with voice or without.
    pub fn heard(&mut self, now: u64, ms: u32, speaking: bool) {
        if speaking {
            self.started
                .get_or_insert(now.saturating_sub(u64::from(ms)));
            self.voiced_ms += ms;
            self.last_voice = now;
            // A new pause gets a new verdict.
            self.complete = None;
        }
        self.speaking = speaking;
    }

    pub fn in_turn(&self) -> bool {
        self.started.is_some()
    }

    /// Quiet since the last voice, the detector's lag included.
    pub fn silence_ms(&self, now: u64) -> u32 {
        if self.speaking || self.started.is_none() {
            return 0;
        }
        (now.saturating_sub(self.last_voice) as u32).saturating_add(self.lag_ms)
    }

    /// Whether Smart Turn should be asked now: a pause long enough and no
    /// verdict on it yet.
    pub fn should_ask(&self, now: u64) -> bool {
        self.in_turn() && self.complete.is_none() && self.silence_ms(now) >= ASK_AFTER_MS
    }

    pub fn verdict(&mut self, complete: f32) {
        self.complete = Some(complete);
    }

    /// Whether the turn is done, given what was transcribed of it so far.
    pub fn done(&self, now: u64, partial: &str) -> bool {
        let Some(started) = self.started else {
            return false;
        };
        turn_done(
            &self.pace,
            partial,
            self.voiced_ms,
            self.silence_ms(now),
            now.saturating_sub(started) as u32,
            self.complete,
        )
    }

    /// Starts listening for the next turn.
    pub fn reset(&mut self) {
        self.started = None;
        self.voiced_ms = 0;
        self.complete = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_ends_sooner_and_an_open_thought_later() {
        assert_eq!(ending("¿Qué hora es?"), Ending::Closed);
        assert_eq!(ending("Quiero ir al cine y"), Ending::Open);
        assert_eq!(
            ending("Quiero ir al cine y."),
            Ending::Open,
            "the recognizer's stop"
        );
        assert_eq!(ending("Necesito, este..."), Ending::Open);
        assert_eq!(ending("I want to go and"), Ending::Open);
        assert_eq!(ending("Quiero ir al cine."), Ending::Unknown);
        assert_eq!(ending(""), Ending::Unknown);
        assert_eq!(pause_ms("normal", "¿Qué hora es?"), 480);
        assert_eq!(pause_ms("quick", "¿Qué?"), 350, "never under 350 ms");
        assert_eq!(pause_ms("normal", "Quiero ir y"), 1_440);
        assert_eq!(pause_ms("patient", "Hola."), 1_400);
        assert_eq!(base_pause_ms("anything"), 800);
    }

    #[test]
    fn smart_turn_only_ever_lengthens_the_pause() {
        assert_eq!(pause_ms_with("normal", "Hola.", None), 800);
        assert_eq!(pause_ms_with("normal", "Hola.", Some(0.9)), 800);
        assert_eq!(pause_ms_with("normal", "Hola.", Some(0.2)), 1_800);
    }

    #[test]
    fn a_turn_needs_voice_and_its_pause_or_the_limit() {
        assert!(
            !turn_done("normal", "Hola.", 200, 5_000, 5_000, None),
            "a cough"
        );
        assert!(!turn_done("normal", "Hola.", 900, 700, 2_000, None));
        assert!(turn_done("normal", "Hola.", 900, 800, 2_000, None));
        assert!(
            !turn_done("normal", "Hola.", 900, 800, 2_000, Some(0.1)),
            "unfinished"
        );
        assert!(
            turn_done("normal", "Y", 29_000, 0, MAX_TURN_MS, None),
            "the limit"
        );
    }

    #[test]
    fn the_tracker_ends_a_turn_after_its_pause_and_waits_on_doubt() {
        let mut turn = Tracker::new("normal", 450);
        assert!(!turn.done(0, ""));
        // A second of voice, then the detector reports quiet.
        for at in (40..=1_000).step_by(40) {
            turn.heard(at, 40, true);
        }
        turn.heard(1_040, 40, false);
        assert!(turn.in_turn());
        assert_eq!(turn.silence_ms(1_040), 490, "the detector's lag counts");
        assert!(turn.should_ask(1_040));
        assert!(
            !turn.done(1_200, "Busca un vuelo."),
            "650 ms of a normal 800"
        );
        assert!(turn.done(1_400, "Busca un vuelo."));
        // Smart Turn finds it unfinished: it waits a second more.
        turn.verdict(0.2);
        assert!(!turn.should_ask(1_400), "already asked");
        assert!(!turn.done(1_400, "Busca un vuelo."));
        assert!(turn.done(2_400, "Busca un vuelo."));
        // An open ending waits longer than a question.
        let mut open = Tracker::new("normal", 450);
        open.heard(500, 500, true);
        open.heard(540, 40, false);
        assert!(!open.done(1_400, "Quiero ir y"));
        assert!(open.done(1_400, "¿Qué hora es?"));
        open.reset();
        assert!(!open.in_turn());
        assert_eq!(open.silence_ms(5_000), 0);
    }

    #[test]
    fn speaking_again_clears_the_verdict_and_a_cough_is_no_turn() {
        let mut turn = Tracker::new("quick", 0);
        turn.heard(100, 100, true);
        turn.heard(140, 40, false);
        assert!(!turn.done(10_000, "eh"), "100 ms of voice is a cough");
        turn.verdict(0.1);
        turn.heard(400, 260, true);
        turn.heard(440, 40, false);
        assert!(turn.should_ask(640), "a new pause, a new question");
        assert!(turn.done(900, "Hola."));
    }

    #[test]
    fn backchannels_never_cut_in() {
        for said in ["sí", "Ajá.", "ok, vale", "jajaja", "yeah right"] {
            assert!(backchannel(said), "{said}");
            assert!(!cuts_in(said), "{said}");
        }
        for said in ["No, espera", "Para un momento", "Wait, that is wrong"] {
            assert!(cuts_in(said), "{said}");
        }
        assert!(!cuts_in(""), "a breath");
        assert!(
            !backchannel("sí claro vale perfecto"),
            "four words say something"
        );
    }

    #[test]
    fn the_agents_words_heard_back_are_echo() {
        let said = "Tienes tres reuniones mañana por la mañana.";
        assert!(echo_of("tres reuniones mañana", said));
        assert!(echo_of("Tienes tres reuniones", said));
        assert!(!echo_of("cancela la de las diez", said));
        assert!(!echo_of("", said));
        assert!(!echo_of("hola", ""));
    }
}
