//! The transcript as the composer receives it. Parakeet already writes
//! capitals and punctuation, so phrases mostly need joining cleanly. Spanish
//! also gets what a phrase-by-phrase recognizer misses, as IVY's dictation
//! does: «¿» and «¡» opening what «?» and «!» close, and a full stop at a
//! pause before a word that goes on («… el informe. y luego …») becoming a
//! comma. The language is told phrase by phrase, so a dictation that mixes
//! Spanish and English keeps each as its own language writes it.
//!
//! Dictation (not a call) also turns spoken commands into what they name
//! («nuevo párrafo», "at sign"; `spoken`), and the user's own vocabulary
//! goes in last (`apply_vocabulary`).

/// The transcript of a dictation: fillers and stutters dropped, spoken
/// formatting and symbols written out.
pub fn join(phrases: &[String]) -> String {
    join_with(phrases, true)
}

/// As `join`, for a call: what is said stays words («dos puntos» is a score).
pub(crate) fn join_call(phrases: &[String]) -> String {
    join_with(phrases, false)
}

/// A whole transcript cut at each sentence end, so `join` tells the language
/// of each sentence as it does of each phrase.
pub(crate) fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        current.push(c);
        let ends = matches!(c, '.' | '?' | '!' | '…');
        if ends && chars.peek().is_none_or(|next| next.is_whitespace()) {
            out.push(std::mem::take(&mut current).trim().to_owned());
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out.retain(|sentence| !sentence.is_empty());
    out
}

fn join_with(phrases: &[String], spoken: bool) -> String {
    let phrases: Vec<String> = phrases
        .iter()
        .map(|phrase| collapse_stutters(&drop_fillers(phrase)))
        .collect();
    // Runs of phrases in one language. A phrase that leans neither way
    // («OK.», a name) goes with the one before it; ahead of any that leans,
    // with the first that does.
    let mut spanish = phrases
        .iter()
        .map(|phrase| lean(phrase))
        .find(|lean| *lean != 0)
        .is_some_and(|lean| lean > 0);
    let mut runs: Vec<(bool, Vec<&str>)> = Vec::new();
    let said = phrases.iter().map(String::as_str);
    for phrase in said.filter(|phrase| !phrase.trim().is_empty()) {
        match lean(phrase) {
            0 => {}
            lean => spanish = lean > 0,
        }
        match runs.last_mut() {
            Some((language, run)) if *language == spanish => run.push(phrase),
            _ => runs.push((spanish, vec![phrase])),
        }
    }
    let tidied: Vec<String> = runs
        .iter()
        .map(|(spanish, run)| {
            let mut joined = squeeze(run);
            if spoken {
                joined = crate::spoken::apply(&joined);
            }
            if *spanish {
                tidy(&rejoin_pauses(&joined))
            } else {
                joined
            }
        })
        .collect();
    // A run that ends or starts a line takes no space next to it.
    let mut out = String::new();
    for run in tidied.iter().filter(|run| !run.is_empty()) {
        if !out.is_empty() && !out.ends_with('\n') && !run.starts_with('\n') {
            out.push(' ');
        }
        out.push_str(run);
    }
    out
}

/// Sounds made while thinking, not words, in English or Spanish. Words that
/// can also carry meaning («este», «o sea», "like") stay, and so does «ah»,
/// which in Spanish is a word («ah, ya entiendo»).
const FILLERS: [&str; 18] = [
    "eh", "ehh", "ehm", "em", "emm", "er", "erm", "hm", "hmm", "hmmm", "mm", "mmm", "um", "umm",
    "uh", "uhh", "uhm", "mhm",
];

/// A word's punctuation before and after it, and the word between:
/// «¿Hola,» → («¿», «Hola», «,»). All punctuation is `lead`.
pub(crate) fn parts(token: &str) -> (&str, &str, &str) {
    let Some(begin) = token.find(char::is_alphanumeric) else {
        return (token, "", "");
    };
    let end = token
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map_or(token.len(), |(at, c)| at + c.len_utf8());
    (&token[..begin], &token[begin..end], &token[end..])
}

/// A word to compare: lower case, without accents.
pub(crate) fn fold(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

/// Ends a sentence: «?», «!» or one full stop, not an ellipsis.
fn ends_sentence(word: &str) -> bool {
    word.ends_with(['?', '!']) || (word.ends_with('.') && !word.ends_with(".."))
}

/// The comma or ellipsis a pause left on a word, taken off.
fn without_pause(word: &str) -> Option<String> {
    if let Some(bare) = word.strip_suffix([',', '…']) {
        return Some(bare.to_owned());
    }
    word.ends_with("..")
        .then(|| word.trim_end_matches('.').to_owned())
}

/// «Eh, el informe, mmm, está listo» → «El informe está listo». A filler
/// that opened a sentence hands its capital to the next word, and one that
/// closed it hands over its full stop. The commas or ellipses the pause left
/// around it go too, except after a sentence's first word («So, um, we» →
/// «So, we»).
fn drop_fillers(phrase: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut capital = false;
    for token in phrase.split_whitespace() {
        let (_, bare, trail) = parts(token);
        if bare.is_empty() || !FILLERS.contains(&fold(bare).as_str()) {
            let mut word = token.to_owned();
            let first = word.chars().next().filter(|c| c.is_lowercase());
            if let (true, Some(first)) = (std::mem::take(&mut capital), first) {
                word = first.to_uppercase().chain(word.chars().skip(1)).collect();
            }
            out.push(word);
            continue;
        }
        let opened = out.last().is_none_or(|last| ends_sentence(last));
        capital |= opened && bare.chars().next().is_some_and(char::is_uppercase);
        // The word before the filler is the first of its sentence.
        let n = out.len();
        let opener = n == 1 || (n >= 2 && ends_sentence(&out[n - 2]));
        // An ellipsis after a filler is the pause going on, not a full stop.
        let stop = if trail.contains("..") || trail.contains('…') {
            String::new()
        } else {
            trail
                .chars()
                .filter(|c| matches!(c, '.' | '?' | '!'))
                .collect()
        };
        let Some(last) = out.last_mut().filter(|last| !ends_sentence(last)) else {
            continue;
        };
        if !stop.is_empty() {
            *last = without_pause(last).unwrap_or_else(|| last.clone());
            *last = last.trim_end_matches([',', ';', ':']).to_owned();
            last.push_str(&stop);
            capital = true;
        } else if !opener {
            if let Some(bare) = without_pause(last) {
                *last = bare;
            }
        }
    }
    out.join(" ")
}

/// Words said twice on purpose («muy muy», "very very", «no no»), and a few
/// that English doubles in grammar ("had had", "that that").
const DOUBLED_ON_PURPOSE: &[&str] = &[
    "muy", "mas", "mucho", "mucha", "muchos", "muchas", "poco", "poquito", "tan", "bien", "no",
    "si", "ya", "nunca", "otra", "dale", "corre", "rapido", "despacio", "chao", "adios", "ja",
    "jaja", "je", "very", "so", "really", "much", "more", "too", "far", "never", "again", "yes",
    "yeah", "bye", "ha", "go", "now", "please", "had", "that", "is", "ok", "okay", "hey", "bla",
    "blah", "tic", "toc", "pum",
];

/// «el el informe» → «el informe», "the the" → "the": a word said twice in a
/// row by a stumble. Numbers («dos dos», "7 7") and words said twice on
/// purpose stay.
fn collapse_stutters(phrase: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for token in phrase.split_whitespace() {
        if let Some(last) = out.last_mut().filter(|last| stutters(last, token)) {
            let (lead, word, _) = parts(last);
            let (_, _, trail) = parts(token);
            *last = format!("{lead}{word}{trail}");
            continue;
        }
        out.push(token.to_owned());
    }
    out.join(" ")
}

fn stutters(previous: &str, next: &str) -> bool {
    const NUMBERS: [&str; 22] = [
        "cero", "uno", "dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve", "diez",
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    let (_, word, trail) = parts(previous);
    let (lead, again, _) = parts(next);
    let folded = fold(word);
    !word.is_empty()
        && lead.is_empty()
        && matches!(trail, "" | ",")
        && folded == fold(again)
        && !word.chars().any(char::is_numeric)
        && !NUMBERS.contains(&folded.as_str())
        && !DOUBLED_ON_PURPOSE.contains(&folded.as_str())
}

/// A word the user wants written their way: `heard` as the recognizer
/// writes it («mono bot»), `write` as it should read («monoBOT»).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Replacement {
    pub heard: String,
    pub write: String,
}

/// Puts each `heard` the user listed as their `write`: whole words only,
/// ignoring case and accents, the punctuation around kept («Mono bot,» →
/// «monoBOT,»). A longer `heard` wins over a shorter one it starts with.
/// Line breaks are kept; a `heard` never spans one.
pub fn apply_vocabulary(text: &str, vocabulary: &[Replacement]) -> String {
    let mut wanted: Vec<(Vec<String>, &str)> = vocabulary
        .iter()
        .map(|entry| {
            let words = entry
                .heard
                .split_whitespace()
                .map(|word| fold(parts(word).1))
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>();
            (words, entry.write.as_str())
        })
        .filter(|(words, _)| !words.is_empty())
        .collect();
    if wanted.is_empty() {
        return text.to_owned();
    }
    wanted.sort_by_key(|(words, _)| std::cmp::Reverse(words.len()));
    // Each word's place in `text`.
    let mut spans = Vec::new();
    let mut start = None;
    for (at, c) in text.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(at),
            (true, Some(from)) => {
                spans.push((from, at));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        spans.push((from, text.len()));
    }
    let mut out = String::with_capacity(text.len());
    let mut written = 0;
    let mut i = 0;
    while i < spans.len() {
        let found = wanted.iter().find(|(words, _)| {
            let Some(run) = spans.get(i..i + words.len()) else {
                return false;
            };
            run.iter()
                .zip(words)
                .enumerate()
                .all(|(k, (&(from, to), word))| {
                    let (lead, core, trail) = parts(&text[from..to]);
                    let inner_before = k > 0 && !lead.is_empty();
                    let inner_after = k + 1 < words.len() && !trail.is_empty();
                    let broken = k > 0 && text[run[k - 1].1..from].contains('\n');
                    !inner_before && !inner_after && !broken && fold(core) == *word
                })
        });
        let Some((words, write)) = found else {
            i += 1;
            continue;
        };
        let (from, _) = spans[i];
        let (_, to) = spans[i + words.len() - 1];
        let (lead, _, _) = parts(&text[from..spans[i].1]);
        let (_, _, trail) = parts(&text[spans[i + words.len() - 1].0..to]);
        out.push_str(&text[written..from]);
        out.push_str(lead);
        out.push_str(write);
        out.push_str(trail);
        written = to;
        i += words.len();
    }
    out.push_str(&text[written..]);
    out
}

/// Why an entry cannot go in the vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VocabularyError {
    /// No words to listen for (blank, or only punctuation).
    NoHeard,
    /// Nothing to write instead.
    NoWrite,
    /// It would write exactly what was heard.
    Unchanged,
}

impl std::fmt::Display for VocabularyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoHeard => "Type the words as dictation writes them.",
            Self::NoWrite => "Type how they should be written.",
            Self::Unchanged => "That already reads the same.",
        })
    }
}

/// The words of `heard` as [`apply_vocabulary`] compares them.
fn heard_words(heard: &str) -> Vec<String> {
    heard
        .split_whitespace()
        .map(|word| fold(parts(word).1))
        .filter(|word| !word.is_empty())
        .collect()
}

/// Adds `heard` → `write` to `list`, both trimmed and with the spaces in
/// `heard` collapsed. An entry already listening for the same words (as
/// [`apply_vocabulary`] matches them: ignoring case, accents and the
/// punctuation around) takes the new spelling in its place. Returns the
/// entry's place in `list`.
pub fn add_replacement(
    list: &mut Vec<Replacement>,
    heard: &str,
    write: &str,
) -> Result<usize, VocabularyError> {
    let heard = heard.split_whitespace().collect::<Vec<_>>().join(" ");
    let write = write.trim();
    let words = heard_words(&heard);
    if words.is_empty() {
        return Err(VocabularyError::NoHeard);
    }
    if write.is_empty() {
        return Err(VocabularyError::NoWrite);
    }
    if heard == write {
        return Err(VocabularyError::Unchanged);
    }
    let entry = Replacement {
        heard,
        write: write.to_owned(),
    };
    match list
        .iter()
        .position(|kept| heard_words(&kept.heard) == words)
    {
        Some(at) => {
            list[at] = entry;
            Ok(at)
        }
        None => {
            list.push(entry);
            Ok(list.len() - 1)
        }
    }
}

fn squeeze<S: AsRef<str>>(phrases: &[S]) -> String {
    let joined = phrases
        .iter()
        .map(|phrase| phrase.as_ref().trim())
        .filter(|phrase| !phrase.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = String::with_capacity(joined.len());
    let mut previous_space = false;
    for ch in joined.chars() {
        let space = ch.is_whitespace();
        if space && previous_space {
            continue;
        }
        // No space before closing punctuation.
        if matches!(ch, ',' | '.' | '?' | '!' | ';' | ':') && out.ends_with(' ') {
            out.pop();
        }
        out.push(if space { ' ' } else { ch });
        previous_space = space;
    }
    out.trim().to_owned()
}

/// Which way a phrase leans: above zero its accented letters and common
/// words are Spanish's, below zero English's, zero when it tells neither.
/// English is left exactly as Parakeet wrote it.
fn lean(text: &str) -> i32 {
    const SPANISH: [&str; 32] = [
        "el", "la", "los", "las", "que", "de", "del", "y", "en", "un", "una", "por", "para", "con",
        "es", "pero", "muy", "lo", "se", "su", "al", "como", "hola", "gracias", "porque", "cuando",
        "yo", "esto", "eso", "hay", "ahora", "vamos",
    ];
    const ENGLISH: [&str; 24] = [
        "the", "and", "is", "are", "you", "to", "of", "it", "that", "this", "with", "for", "what",
        "i", "we", "have", "not", "but", "so", "can", "will", "my", "your", "they",
    ];
    let lower = text.to_lowercase();
    let mut score = lower
        .chars()
        .filter(|c| matches!(c, 'ñ' | '¿' | '¡' | 'á' | 'é' | 'í' | 'ó' | 'ú'))
        .count() as i32
        * 2;
    for word in lower.split(|c: char| !c.is_alphabetic()) {
        score += i32::from(SPANISH.contains(&word));
        score -= i32::from(ENGLISH.contains(&word));
    }
    score
}

/// Words that carry a sentence on: a full stop before one was a pause.
const GOES_ON: [&str; 25] = [
    "y", "e", "o", "u", "ni", "pero", "porque", "que", "si", "de", "del", "en", "con", "para",
    "por", "a", "al", "cuando", "como", "aunque", "sino", "entonces", "pues", "donde", "mientras",
];

/// «… el informe. Y luego …» → «… el informe, y luego …».
fn rejoin_pauses(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(". ") {
        let (before, after) = (&rest[..at], &rest[at + 2..]);
        let word: String = after.chars().take_while(|c| c.is_alphabetic()).collect();
        let goes_on = !word.is_empty()
            && word.chars().skip(1).all(char::is_lowercase)
            && GOES_ON.contains(&word.to_lowercase().as_str());
        out.push_str(before);
        if goes_on {
            out.push_str(", ");
            let mut letters = after.chars();
            out.extend(letters.next().into_iter().flat_map(char::to_lowercase));
            rest = letters.as_str();
        } else {
            out.push_str(". ");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Opening «¿» and «¡» where Spanish needs them, and a capital at each
/// sentence start. A full stop ends a sentence only before a space, so
/// «informe.pdf» and «3.50» go on.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut start = 0;
    let (mut asked, mut exclaimed) = (false, false);
    let mut capital = true;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '¿' => asked = true,
            '¡' => exclaimed = true,
            _ => {}
        }
        if capital && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            capital = false;
            continue;
        }
        if capital && c.is_numeric() {
            capital = false;
        }
        out.push(c);
        if matches!(c, '?' | '!') {
            let (opened, mark) = if c == '?' {
                (asked, '¿')
            } else {
                (exclaimed, '¡')
            };
            if !opened {
                let at = start + out[start..].len() - out[start..].trim_start().len();
                out.insert(at, mark);
            }
        }
        let ends = matches!(c, '?' | '!' | '…' | '\n')
            || (c == '.' && chars.peek().is_none_or(|next| next.is_whitespace()));
        if ends {
            start = out.len();
            (asked, exclaimed, capital) = (false, false, true);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_reads_as_written() {
        let said = |text: &str| join(&[text.to_owned()]);
        assert_eq!(
            said("hola. cuánto voy gastando este mes? qué bueno!"),
            "Hola. ¿Cuánto voy gastando este mes? ¡Qué bueno!"
        );
        assert_eq!(said("¿Ya está? sí, gracias"), "¿Ya está? Sí, gracias");
        assert_eq!(
            said("son 3.50 soles para el café"),
            "Son 3.50 soles para el café"
        );
        assert_eq!(
            said("abre el archivo informe.pdf y dime qué dice"),
            "Abre el archivo informe.pdf y dime qué dice"
        );
    }

    #[test]
    fn a_pause_before_a_word_that_goes_on_is_a_comma() {
        let phrases = [
            "Revisa el informe.".to_owned(),
            "Y luego mándalo a Ana.".to_owned(),
        ];
        assert_eq!(join(&phrases), "Revisa el informe, y luego mándalo a Ana.");
        let phrases = [
            "Ya terminé el informe.".to_owned(),
            "Mañana lo vemos.".to_owned(),
        ];
        assert_eq!(join(&phrases), "Ya terminé el informe. Mañana lo vemos.");
    }

    #[test]
    fn english_is_left_as_parakeet_wrote_it() {
        let phrases = ["How are you?".to_owned(), "A dog barked.".to_owned()];
        assert_eq!(join(&phrases), "How are you? A dog barked.");
        let phrases = ["I went home.".to_owned(), "And then I slept.".to_owned()];
        assert_eq!(join(&phrases), "I went home. And then I slept.");
    }

    #[test]
    fn a_dictation_in_both_languages_keeps_each_as_it_is_written() {
        let phrases = [
            "hola, cómo va el despliegue?".to_owned(),
            "Can you check the build?".to_owned(),
            "OK.".to_owned(),
            "y luego me avisas.".to_owned(),
        ];
        assert_eq!(
            join(&phrases),
            "¿Hola, cómo va el despliegue? Can you check the build? OK. Y luego me avisas."
        );
        // Mostly English with one Spanish aside: the aside alone is tidied.
        let phrases = [
            "The tests are green and the branch is ready.".to_owned(),
            "qué bueno!".to_owned(),
            "So we can merge it.".to_owned(),
        ];
        assert_eq!(
            join(&phrases),
            "The tests are green and the branch is ready. ¡Qué bueno! So we can merge it."
        );
        // A phrase that tells neither way ahead of the rest follows them.
        let phrases = ["Listo.".to_owned(), "ya está en producción?".to_owned()];
        assert_eq!(join(&phrases), "Listo. ¿Ya está en producción?");
    }

    #[test]
    fn sounds_made_while_thinking_are_dropped() {
        let said = |text: &str| join(&[text.to_owned()]);
        assert_eq!(
            said("Eh, el informe, mmm, está listo."),
            "El informe está listo."
        );
        assert_eq!(said("So, um, we should ship it."), "So, we should ship it.");
        assert_eq!(
            said("I checked the logs, uh. Then I left."),
            "I checked the logs. Then I left."
        );
        // Words that also mean something stay.
        assert_eq!(
            said("Este informe, o sea, el de ayer."),
            "Este informe, o sea, el de ayer."
        );
        // A phrase that was only a sound leaves nothing behind.
        assert_eq!(join(&["Mmm.".to_owned(), "Hola.".to_owned()]), "Hola.");
    }

    #[test]
    fn a_dropped_filler_takes_its_pause_marks_with_it() {
        let said = |text: &str| join(&[text.to_owned()]);
        assert_eq!(
            said("I think, er, we should wait."),
            "I think we should wait."
        );
        assert_eq!(said("Er, the build is green."), "The build is green.");
        assert_eq!(
            said("el informe… eh… está listo."),
            "El informe está listo."
        );
        assert_eq!(said("We ship, um... tomorrow."), "We ship tomorrow.");
        // After a sentence's first word the comma is the word's own.
        assert_eq!(said("Bueno, eh, vamos."), "Bueno, vamos.");
        // «ah» is a word in Spanish.
        assert_eq!(said("Ah, ya entiendo."), "Ah, ya entiendo.");
    }

    #[test]
    fn a_word_said_twice_by_a_stumble_is_written_once() {
        let said = |text: &str| join(&[text.to_owned()]);
        assert_eq!(said("revisa el el informe"), "Revisa el informe");
        assert_eq!(said("El, el informe está listo."), "El informe está listo.");
        assert_eq!(said("The the build is green."), "The build is green.");
        assert_eq!(said("I want to to ship it."), "I want to ship it.");
        // Doubled on purpose, or numbers.
        assert_eq!(said("Está muy muy bien."), "Está muy muy bien.");
        assert_eq!(said("That is very very good."), "That is very very good.");
        assert_eq!(said("no no, eso no"), "No no, eso no");
        assert_eq!(said("I know that that works."), "I know that that works.");
        assert_eq!(said("marca el dos dos cinco"), "Marca el dos dos cinco");
        assert_eq!(said("Room 7 7 is free."), "Room 7 7 is free.");
    }

    #[test]
    fn spoken_formatting_reaches_the_dictation() {
        let phrases = [
            "Hola Ana, nuevo párrafo.".to_owned(),
            "el informe está listo? escríbeme a arroba carlos.".to_owned(),
        ];
        assert_eq!(
            join(&phrases),
            "Hola Ana\n\n¿El informe está listo? Escríbeme a @carlos."
        );
        let phrases = [
            "Rename it to user underscore id.".to_owned(),
            "New line.".to_owned(),
            "Then run the tests.".to_owned(),
        ];
        assert_eq!(join(&phrases), "Rename it to user_id.\nThen run the tests.");
        // A call keeps the words.
        let phrases = ["Gana por dos puntos y punto y coma.".to_owned()];
        assert_eq!(join_call(&phrases), "Gana por dos puntos y punto y coma.");
    }

    #[test]
    fn vocabulary_entries_are_tidied_and_the_same_words_are_listed_once() {
        let mut list = Vec::new();
        assert_eq!(
            add_replacement(&mut list, "  mono   bot ", " monoBOT "),
            Ok(0)
        );
        assert_eq!(
            list,
            [Replacement {
                heard: "mono bot".into(),
                write: "monoBOT".into(),
            }]
        );
        assert_eq!(add_replacement(&mut list, "jason", "JSON"), Ok(1));
        // The same words, as matching sees them, take the new spelling.
        assert_eq!(add_replacement(&mut list, "Monó, Bot", "MonoBot"), Ok(0));
        assert_eq!(list[0].heard, "Monó, Bot");
        assert_eq!(list[0].write, "MonoBot");
        assert_eq!(list.len(), 2);
        assert_eq!(
            apply_vocabulary("abre mono bot", &list),
            "abre MonoBot",
            "the tidied entry still matches"
        );
        // Casing alone is a change worth keeping.
        assert_eq!(add_replacement(&mut list, "github", "GitHub"), Ok(2));

        for (heard, write, error) in [
            ("", "x", VocabularyError::NoHeard),
            (" … ", "x", VocabularyError::NoHeard),
            ("jason", "  ", VocabularyError::NoWrite),
            ("same", " same ", VocabularyError::Unchanged),
        ] {
            assert_eq!(add_replacement(&mut list, heard, write), Err(error));
        }
        assert_eq!(list.len(), 3);
        assert!(!VocabularyError::NoWrite.to_string().is_empty());
    }

    #[test]
    fn the_users_vocabulary_is_written_their_way() {
        let vocabulary = [
            Replacement {
                heard: "mono bot".into(),
                write: "monoBOT".into(),
            },
            Replacement {
                heard: "jason".into(),
                write: "JSON".into(),
            },
            Replacement {
                heard: "Pérez".into(),
                write: "Peres".into(),
            },
        ];
        assert_eq!(
            apply_vocabulary("Abre Mono Bot, y el Jason.", &vocabulary),
            "Abre monoBOT, y el JSON."
        );
        // Whole words only, accents and case aside.
        assert_eq!(
            apply_vocabulary("jasonette y PEREZ", &vocabulary),
            "jasonette y Peres"
        );
        // Words broken by punctuation or a line are not the phrase.
        assert_eq!(
            apply_vocabulary("mono, bot\nmono\nbot", &vocabulary),
            "mono, bot\nmono\nbot"
        );
        assert_eq!(
            apply_vocabulary("Hola\n\nmono bot", &vocabulary),
            "Hola\n\nmonoBOT"
        );
        assert_eq!(apply_vocabulary("nada", &[]), "nada");
    }

    #[test]
    fn a_whole_transcript_is_cut_at_each_sentence() {
        assert_eq!(
            sentences("hola. qué tal? Fine, thanks! 3.50 soles"),
            ["hola.", "qué tal?", "Fine, thanks!", "3.50 soles"]
        );
        assert!(sentences("  ").is_empty());
    }

    #[test]
    fn joins_phrases_with_single_spaces() {
        let phrases = vec![
            " Hola, ¿cómo estás? ".to_owned(),
            String::new(),
            "Let's   ship it .".to_owned(),
        ];
        assert_eq!(join(&phrases), "Hola, ¿cómo estás? Let's ship it.");
        assert_eq!(join(&[]), "");
    }
}
