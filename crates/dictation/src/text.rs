//! The transcript as the composer receives it: Parakeet already writes
//! capitals and punctuation, so phrases only need joining cleanly.

pub fn join(phrases: &[String]) -> String {
    let joined = phrases
        .iter()
        .map(|phrase| phrase.trim())
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

#[cfg(test)]
mod tests {
    use super::join;

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
