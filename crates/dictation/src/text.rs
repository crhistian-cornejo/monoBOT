//! The transcript as the composer receives it. Parakeet already writes
//! capitals and punctuation, so phrases mostly need joining cleanly. Spanish
//! also gets what a phrase-by-phrase recognizer misses, as IVY's dictation
//! does: «¿» and «¡» opening what «?» and «!» close, and a full stop at a
//! pause before a word that goes on («… el informe. y luego …») becoming a
//! comma. The language is told phrase by phrase, so a dictation that mixes
//! Spanish and English keeps each as its own language writes it.

pub fn join(phrases: &[String]) -> String {
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
            let joined = squeeze(run);
            if *spanish {
                tidy(&rejoin_pauses(&joined))
            } else {
                joined
            }
        })
        .collect();
    tidied.join(" ")
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
