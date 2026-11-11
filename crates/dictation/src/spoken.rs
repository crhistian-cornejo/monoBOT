//! Spoken formatting and symbols: «nuevo párrafo» or "new line" break the
//! text, «abre comillas» opens a quote, «arroba» or "at sign" writes «@».
//! Only commands that cannot be ordinary words are taken: several words, or
//! one word that is never anything else in dictation. «coma» alone stays a
//! word, since Parakeet already writes the commas. A command right after an
//! article («el guion de la película», "a new line of products") is talking
//! about the thing, so it stays words too. Case and accents do not matter,
//! and the commas or full stops the recognizer put around a command go.

use crate::text::{fold, parts};

#[derive(Clone, Copy)]
enum Action {
    /// A blank line.
    Paragraph,
    /// A line break.
    Line,
    OpenQuote,
    CloseQuote,
    /// A symbol, joined to the word before it, the word after it, or both.
    Symbol {
        text: &'static str,
        before: bool,
        after: bool,
    },
}

struct Command {
    /// Folded (lower case, no accents).
    words: &'static [&'static str],
    action: Action,
    spanish: bool,
    /// Words after which this is not the command.
    not_after: &'static [&'static str],
    /// Words before which this is not the command («barra de progreso»).
    not_before: &'static [&'static str],
}

const fn symbol(text: &'static str, before: bool, after: bool) -> Action {
    Action::Symbol {
        text,
        before,
        after,
    }
}

const fn command(words: &'static [&'static str], action: Action, spanish: bool) -> Command {
    Command {
        words,
        action,
        spanish,
        not_after: &[],
        not_before: &[],
    }
}

/// Articles, demonstratives and possessives: after one of these a command is
/// a noun («un nuevo párrafo», "the colon"). English adds "to", after which
/// "slash" or "dash" is a verb.
const SPANISH_ARTICLES: &[&str] = &[
    "el", "la", "los", "las", "un", "una", "unos", "unas", "este", "esta", "ese", "esa", "aquel",
    "aquella", "mi", "tu", "su", "mis", "tus", "sus", "nuestro", "nuestra", "del", "al", "cada",
    "otro", "otra",
];
const ENGLISH_ARTICLES: &[&str] = &[
    "the", "a", "an", "this", "that", "my", "your", "his", "her", "its", "our", "their", "to",
    "each", "another", "every",
];

/// «dos puntos» is also a score: not after a verb that counts it, nor before
/// what follows a number («dos puntos de ventaja», «dos puntos más»).
const SCORE_BEFORE: &[&str] = &[
    "de", "del", "mas", "menos", "extra", "en", "por", "para", "a", "y", "o", "que", "porque",
];
const SCORE_AFTER: &[&str] = &[
    "los", "unos", "estos", "esos", "sus", "mis", "tus", "otros", "con", "sin", "por", "de",
    "gane", "gano", "ganamos", "perdi", "perdio", "perdimos", "tengo", "tiene", "tienes",
    "tenemos", "solo", "suma", "sumo", "resta", "resto", "quedan", "faltan", "mas",
];

const COMMANDS: &[Command] = &[
    command(&["nuevo", "parrafo"], Action::Paragraph, true),
    command(&["new", "paragraph"], Action::Paragraph, false),
    command(&["nueva", "linea"], Action::Line, true),
    command(&["salto", "de", "linea"], Action::Line, true),
    command(&["new", "line"], Action::Line, false),
    command(&["abre", "comillas"], Action::OpenQuote, true),
    command(&["abrir", "comillas"], Action::OpenQuote, true),
    command(&["open", "quote"], Action::OpenQuote, false),
    command(&["open", "quotes"], Action::OpenQuote, false),
    command(&["cierra", "comillas"], Action::CloseQuote, true),
    command(&["cerrar", "comillas"], Action::CloseQuote, true),
    command(&["close", "quote"], Action::CloseQuote, false),
    command(&["close", "quotes"], Action::CloseQuote, false),
    command(&["end", "quote"], Action::CloseQuote, false),
    command(&["arroba"], symbol("@", false, true), true),
    command(&["at", "sign"], symbol("@", false, true), false),
    command(&["guion", "bajo"], symbol("_", true, true), true),
    command(&["underscore"], symbol("_", true, true), false),
    command(&["punto", "y", "coma"], symbol(";", true, false), true),
    command(&["semicolon"], symbol(";", true, false), false),
    Command {
        not_after: SCORE_AFTER,
        not_before: SCORE_BEFORE,
        ..command(&["dos", "puntos"], symbol(":", true, false), true)
    },
    command(&["colon"], symbol(":", true, false), false),
    Command {
        not_before: &["de", "del"],
        ..command(&["guion"], symbol("-", true, true), true)
    },
    command(&["hyphen"], symbol("-", true, true), false),
    Command {
        not_before: &["of"],
        ..command(&["dash"], symbol("-", true, true), false)
    },
    command(&["barra", "invertida"], symbol("\\", true, true), true),
    Command {
        not_before: &["de", "del", "lateral", "libre", "espaciadora", "invertida"],
        ..command(&["barra"], symbol("/", true, true), true)
    },
    command(&["backslash"], symbol("\\", true, true), false),
    command(&["slash"], symbol("/", true, true), false),
    command(&["hashtag"], symbol("#", false, true), true),
    command(&["hashtag"], symbol("#", false, true), false),
];

/// One piece of the output, and whether the next one follows it with no
/// space.
struct Piece {
    text: String,
    glued: bool,
    /// Written by a command: its marks are not the recognizer's to strip.
    command: bool,
}

/// Turns the spoken commands in `text` into what they name.
pub(crate) fn apply(text: &str) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut pieces: Vec<Piece> = Vec::new();
    let (mut capital, mut lower) = (false, false);
    let mut i = 0;
    while i < tokens.len() {
        let Some(command) = matching(&tokens, i) else {
            let mut word = tokens[i].to_owned();
            if std::mem::take(&mut capital) {
                word = recase(&word, true);
            }
            if std::mem::take(&mut lower) {
                word = recase(&word, false);
            }
            pieces.push(Piece {
                text: word,
                glued: false,
                command: false,
            });
            i += 1;
            continue;
        };
        let last = tokens[i + command.words.len() - 1];
        i += command.words.len();
        let (_, _, trail) = parts(last);
        match command.action {
            Action::Paragraph | Action::Line => {
                if let Some(previous) = pieces.last_mut() {
                    strip(previous, &[',', ';', ':']);
                    previous.glued = true;
                }
                let paragraph = matches!(command.action, Action::Paragraph);
                pieces.push(Piece {
                    text: if paragraph { "\n\n" } else { "\n" }.into(),
                    glued: true,
                    command: true,
                });
                capital = true;
                lower = false;
            }
            Action::OpenQuote => {
                if let Some(previous) = pieces.last_mut() {
                    strip(previous, &[',']);
                }
                pieces.push(Piece {
                    text: "\"".into(),
                    glued: true,
                    command: true,
                });
            }
            Action::CloseQuote => {
                if let Some(previous) = pieces.last_mut() {
                    strip(previous, &[',']);
                    previous.glued = true;
                }
                // A sentence that ends with the quote keeps its full stop.
                let stop: String = trail
                    .chars()
                    .filter(|c| matches!(c, '.' | '?' | '!'))
                    .take(1)
                    .collect();
                pieces.push(Piece {
                    text: format!("\"{stop}"),
                    glued: false,
                    command: true,
                });
            }
            Action::Symbol {
                text,
                before,
                after,
            } => {
                if let Some(previous) = pieces.last_mut() {
                    if before {
                        strip(previous, &[',', '.', ';', ':']);
                        previous.glued = true;
                    } else {
                        strip(previous, &[',']);
                    }
                }
                pieces.push(Piece {
                    text: text.into(),
                    glued: after,
                    command: true,
                });
                // «punto y coma.» then «Luego»: the capital was the full
                // stop's, and the sentence goes on.
                lower = text == ";" && trail.contains('.');
            }
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut glued = true;
    for piece in &pieces {
        if !glued && !piece.text.starts_with('\n') {
            out.push(' ');
        }
        out.push_str(&piece.text);
        glued = piece.glued;
    }
    out
}

/// The command that starts at `tokens[at]`, if one does: its words with no
/// punctuation between them, not after an article, and with the words a
/// joined symbol needs on each side.
fn matching(tokens: &[&str], at: usize) -> Option<&'static Command> {
    let previous = at
        .checked_sub(1)
        .map(|before| fold(parts(tokens[before]).1));
    COMMANDS
        .iter()
        .filter(|command| {
            let len = command.words.len();
            let Some(run) = tokens.get(at..at + len) else {
                return false;
            };
            let said = run
                .iter()
                .zip(command.words)
                .enumerate()
                .all(|(k, (token, word))| {
                    let (lead, core, trail) = parts(token);
                    (k == 0 || lead.is_empty())
                        && (k + 1 == len || trail.is_empty())
                        && fold(core) == *word
                });
            if !said {
                return false;
            }
            let next = tokens.get(at + len).map(|token| fold(parts(token).1));
            let articles = if command.spanish {
                SPANISH_ARTICLES
            } else {
                ENGLISH_ARTICLES
            };
            let after_word =
                |list: &[&str]| previous.as_deref().is_some_and(|word| list.contains(&word));
            if after_word(articles) || after_word(command.not_after) {
                return false;
            }
            if next
                .as_deref()
                .is_some_and(|word| command.not_before.contains(&word))
            {
                return false;
            }
            match command.action {
                // A joined symbol with nothing to join is the word itself.
                Action::Symbol { before, after, .. } => {
                    (!before || previous.as_deref().is_some_and(|word| !word.is_empty()))
                        && (!after || next.as_deref().is_some_and(|word| !word.is_empty()))
                }
                _ => true,
            }
        })
        .max_by_key(|command| command.words.len())
}

/// Takes the trailing `marks` off a word («Hola,» → «Hola»).
fn strip(piece: &mut Piece, marks: &[char]) {
    while !piece.command && piece.text.ends_with(marks) {
        piece.text.pop();
    }
}

/// A word with its first letter in upper case, or a capitalized word
/// («Luego») in lower case. A word in capitals («API») stays.
fn recase(word: &str, upper: bool) -> String {
    let Some((at, first)) = word.char_indices().find(|(_, c)| c.is_alphabetic()) else {
        return word.to_owned();
    };
    let rest = &word[at + first.len_utf8()..];
    if !upper && (!first.is_uppercase() || rest.chars().any(char::is_uppercase)) {
        return word.to_owned();
    }
    let first: String = if upper {
        first.to_uppercase().collect()
    } else {
        first.to_lowercase().collect()
    };
    format!("{}{first}{rest}", &word[..at])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_line_breaks_in_spanish_and_english() {
        assert_eq!(
            apply("Hola, nuevo párrafo, el informe está listo."),
            "Hola\n\nEl informe está listo."
        );
        assert_eq!(apply("Listo. Nueva línea. Mañana."), "Listo.\nMañana.");
        assert_eq!(apply("uno salto de línea dos"), "uno\nDos");
        assert_eq!(
            apply("Dear Ana, new paragraph. thanks for the notes."),
            "Dear Ana\n\nThanks for the notes."
        );
        assert_eq!(apply("first New Line second"), "first\nSecond");
    }

    #[test]
    fn quotes_open_and_close_around_the_words() {
        assert_eq!(
            apply("Dijo abre comillas ya voy cierra comillas y se fue."),
            "Dijo \"ya voy\" y se fue."
        );
        assert_eq!(
            apply("He said, open quote, ship it, close quote."),
            "He said \"ship it\"."
        );
    }

    #[test]
    fn spoken_symbols_are_written_and_joined() {
        assert_eq!(apply("Escríbele a arroba ana."), "Escríbele a @ana.");
        assert_eq!(apply("ping at sign carlos"), "ping @carlos");
        assert_eq!(apply("nombre guion bajo usuario"), "nombre_usuario");
        assert_eq!(apply("user underscore id"), "user_id");
        assert_eq!(
            apply("Primero esto, punto y coma. Luego aquello."),
            "Primero esto; luego aquello."
        );
        assert_eq!(apply("Nota dos puntos revisar."), "Nota: revisar.");
        assert_eq!(apply("Note colon check it"), "Note: check it");
        assert_eq!(apply("monobot guion desktop"), "monobot-desktop");
        assert_eq!(apply("well hyphen known"), "well-known");
        assert_eq!(apply("src barra main"), "src/main");
        assert_eq!(apply("src slash lib"), "src/lib");
        assert_eq!(apply("hashtag monobot"), "#monobot");
        assert_eq!(apply("C dos puntos barra invertida temp"), "C:\\temp");
    }

    #[test]
    fn case_and_accents_do_not_matter() {
        assert_eq!(apply("Nombre Guión Bajo Usuario"), "Nombre_Usuario");
        assert_eq!(apply("hola NUEVO PARRAFO adiós"), "hola\n\nAdiós");
    }

    #[test]
    fn words_that_only_look_like_commands_stay() {
        for said in [
            "Pon una coma después.",
            "Gané dos puntos en el examen.",
            "Tengo dos puntos de ventaja.",
            "El guion de la película.",
            "La barra de progreso.",
            "Nos vemos en la barra.",
            "Abre el archivo.",
            "A new line of products.",
            "Add a dash of salt.",
            "We need to slash prices.",
            "The colon is an organ.",
            "Pon un guion bajo aquí.",
            "Solo guion",
            "slash",
        ] {
            assert_eq!(apply(said), said, "{said}");
        }
    }

    #[test]
    fn a_command_split_by_punctuation_is_not_one() {
        assert_eq!(apply("Fue nuevo, párrafo"), "Fue nuevo, párrafo");
        assert_eq!(apply("punto, y coma"), "punto, y coma");
    }
}
