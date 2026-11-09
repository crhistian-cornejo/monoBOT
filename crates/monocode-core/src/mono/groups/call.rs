//! The decisions for a group voice call, shared by the native call UI and
//! its tests. Tauri's `groups/model/groupCall.ts` is the reference.
use super::{block::Block, mentions::Named};
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// Most members that answer an unaddressed turn.
pub const MAX_SPEAKERS: usize = 4;
/// The pause after the voice detector closes a phrase.
pub const TURN_GAP_MS: u64 = 600;

pub(crate) fn words(text: &str) -> String {
    let folded: String = text
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    format!(
        " {} ",
        folded.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

/// Members explicitly named in a spoken turn, in the order named.
pub fn addressed_members<T: Named>(members: &[T], said: &str) -> Vec<String> {
    let heard = words(said);
    let mut named: Vec<(usize, &T)> = members
        .iter()
        .filter_map(|member| {
            let name = words(member.name());
            if name.trim().is_empty() {
                return None;
            }
            heard.find(&name).map(|at| (at, member))
        })
        .collect();
    named.sort_by_key(|(at, _)| *at);
    named
        .into_iter()
        .map(|(_, member)| member.id().to_owned())
        .collect()
}

/// How a Spanish speaker's ear writes a word, so a name can be told from its
/// misheard forms («Jarvis» is said «Yarbis» and written «yares»): accents
/// and «h» go, «j» is «y», «v» and «w» are «b», «z» and a soft «c» are «s»,
/// a hard «c», «q» and «k» are «k», a soft «g» is «y», and doubles collapse.
fn sound(word: &str) -> String {
    let folded: Vec<char> = words(word).chars().filter(|c| c.is_alphabetic()).collect();
    let mut out = String::new();
    for (at, &c) in folded.iter().enumerate() {
        let next = folded.get(at + 1).copied();
        let mapped = match c {
            'h' => continue,
            'j' => 'y',
            'v' | 'w' => 'b',
            'z' => 's',
            'c' if matches!(next, Some('e' | 'i')) => 's',
            'c' | 'q' | 'k' => 'k',
            'g' if matches!(next, Some('e' | 'i')) => 'y',
            other => other,
        };
        if !out.ends_with(mapped) {
            out.push(mapped);
        }
    }
    out
}

/// The edit distance between two words as the ear writes them, over the
/// longer one's length: 0 the same, 1 nothing alike.
pub(crate) fn nearness(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (sound(a).chars().collect(), sound(b).chars().collect());
    if a.is_empty() || b.is_empty() {
        return 1.0;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (row[j + 1] + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(x != y));
            diagonal = above;
        }
    }
    row[b.len()] as f32 / a.len().max(b.len()) as f32
}

/// Words that open a turn without being anyone's name.
const OPENERS: [&str; 10] = [
    "hola", "oye", "oiga", "ey", "hey", "buenas", "dime", "dinos", "veamos", "hi",
];
const NOT_NAMES: [&str; 14] = [
    "como", "cuando", "donde", "quien", "para", "pero", "bueno", "buenos", "gracias", "dime",
    "cual", "porque", "what", "where",
];

/// A turn that calls a member by a name the recognizer wrote wrong («Hola
/// yares, ¿cómo está?» for «Hola, Jarvis…»), rewritten as «Jarvis, ¿cómo
/// está?» so it reaches that member as if it had been written right. None
/// when nobody is called, when the name was heard right, or when nothing
/// follows it. Only the first two words after a greeting count, as IVY does.
pub fn spoken_address<T: Named>(members: &[T], said: &str) -> Option<String> {
    let mut rest = said.trim();
    let first = rest
        .char_indices()
        .find(|(_, c)| !c.is_alphanumeric())
        .map_or(rest.len(), |(at, _)| at);
    if OPENERS.contains(&words(&rest[..first]).trim()) {
        rest = rest[first..].trim_start_matches([',', ' ', '.', '!', '¡']);
    }
    for _ in 0..2 {
        let end = rest.find([',', ' ', ':']).unwrap_or(rest.len());
        let word = rest[..end].trim_matches(|c: char| !c.is_alphanumeric());
        let after = rest[end..].trim_start_matches([',', ' ', ':', '.']).trim();
        let folded = words(word).trim().to_owned();
        if folded.chars().count() >= 4 && !NOT_NAMES.contains(&folded.as_str()) && !after.is_empty()
        {
            let best = members
                .iter()
                .filter(|member| !member.name().trim().is_empty())
                .map(|member| (nearness(word, member.name()), member.name()))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((distance, name)) = best {
                if distance <= 0.34 {
                    return (words(name).trim() != folded).then(|| format!("{name}, {after}"));
                }
            }
        }
        if end >= rest.len() {
            break;
        }
        rest = rest[end..].trim_start_matches([',', ' ', ':', '.']);
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnOrder {
    pub order: Vec<String>,
    pub addressed: bool,
}

/// Named members answer first; otherwise rotate the first speaker each turn.
pub fn turn_order<T: Named>(members: &[T], said: &str, previous_first: Option<&str>) -> TurnOrder {
    let named = addressed_members(members, said);
    if !named.is_empty() {
        return TurnOrder {
            order: named,
            addressed: true,
        };
    }
    let start = previous_first
        .and_then(|id| members.iter().position(|member| member.id() == id))
        .map_or(0, |index| (index + 1) % members.len().max(1));
    TurnOrder {
        order: members[start..]
            .iter()
            .chain(&members[..start])
            .map(|member| member.id().to_owned())
            .collect(),
        addressed: false,
    }
}

/// Frames a call turn for a member's own group seat.
pub fn call_message(
    group: &str,
    goal: Option<&str>,
    others: &[String],
    said: &str,
    earlier: &[(String, String)],
    addressed: bool,
) -> String {
    let company = if others.is_empty() {
        String::new()
    } else {
        format!(" and {}", others.join(", "))
    };
    let mut lines = vec![format!("You are on a voice call with the user{company}.")];
    if let Some(goal) = goal.filter(|goal| !goal.is_empty()) {
        lines.push(format!("The group is for: {goal}"));
    }
    lines.push(format!("The user said: {}", said.trim()));
    lines.extend(
        earlier
            .iter()
            .map(|(name, text)| format!("{name} said: {text}")),
    );
    let pass = if addressed {
        ""
    } else {
        " Add something new rather than repeat the others. If you have nothing to add, reply with exactly PASS."
    };
    format!(
        "<group_call group=\"{group}\">\n{}\n</group_call>\nAnswer out loud as yourself: one to three short sentences in the language the user spoke, as plain speech with no markdown, lists, code or links.{pass}",
        lines.join("\n")
    )
}

static PASS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)^[\s*_`"'.!\x{00A1}?\x{00BF}-]*pass[\s*_`"'.!?-]*$"#).unwrap()
});
static LETTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{L}$").unwrap());

/// A member had nothing to add.
pub fn is_pass(text: &str) -> bool {
    PASS.is_match(text)
}

/// A streaming reply might still become PASS and should not yet be spoken.
pub fn could_be_pass(text: &str) -> bool {
    let letters: String = text
        .chars()
        .filter(|c| LETTER.is_match(c.encode_utf8(&mut [0; 4])))
        .flat_map(char::to_lowercase)
        .collect();
    letters.chars().count() <= 4 && "pass".starts_with(&letters)
}

/// The member's written answer to this call request, around tool calls.
pub fn member_reply(blocks: &[Block], request: &str) -> String {
    let Some(start) = blocks
        .iter()
        .position(|block| block.is_user() && block.app_request_id.as_deref() == Some(request))
    else {
        return String::new();
    };
    blocks[start + 1..]
        .iter()
        .take_while(|block| !block.is_user())
        .filter(|block| {
            block.is_assistant()
                && block.tool.is_none()
                && !block.internal
                && !block.text.is_empty()
        })
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

static IMAGES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap());
static LINKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap());
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]*)`").unwrap());
static TAGS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static URLS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://\S+").unwrap());
static LIST_MARKERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*(?:#{1,6}|>|[-*+\x{2022}]|\d+[.)])\s+").unwrap());
static MARKDOWN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[*_~|`#]").unwrap());
static EMOJI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\p{Extended_Pictographic}|\x{fe0f}").unwrap());

/// Plain words for the synthesizer, without markdown, links or emoji.
pub fn speakable(text: &str) -> String {
    let text = IMAGES.replace_all(text, " ");
    let text = LINKS.replace_all(&text, "$1");
    let text = INLINE_CODE.replace_all(&text, "$1");
    let text = TAGS.replace_all(&text, " ");
    let text = URLS.replace_all(&text, " ");
    let text = LIST_MARKERS.replace_all(&text, "");
    let text = MARKDOWN.replace_all(&text, "");
    let text = EMOJI.replace_all(&text, "");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

static ABBREVIATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([\p{L}]+)[.]$").unwrap());
static ABBREVIATIONS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "sr sra srta dr dra ud uds etc ej vs mr mrs ms st no pag aprox approx eg ie e g i"
        .split_whitespace()
        .collect()
});

fn ends_with_abbreviation(text: &str) -> bool {
    let Some(last) = ABBREVIATION
        .captures(text.trim_end())
        .and_then(|found| found.get(1))
        .map(|found| found.as_str())
    else {
        return false;
    };
    ABBREVIATIONS.contains(last.to_lowercase().as_str())
        || (last.chars().count() == 1 && last.chars().next().is_some_and(char::is_uppercase))
}

#[derive(Debug, PartialEq, Eq)]
pub struct Sentences {
    pub sentences: Vec<String>,
    /// UTF-8 byte offset to resume at in the same reply.
    pub next: usize,
}

/// Finished sentences since `from`, skipping fenced code while streaming.
pub fn take_sentences(text: &str, from: usize, done: bool) -> Sentences {
    let mut from = from.min(text.len());
    while !text.is_char_boundary(from) {
        from -= 1;
    }
    let mut sentences = Vec::new();
    let mut push = |segment: &str| {
        let spoken = speakable(segment);
        if spoken.chars().any(char::is_alphanumeric) {
            sentences.push(spoken);
        }
    };
    let mut start = from;
    let mut index = start;
    while index < text.len() {
        if text[index..].starts_with("```") {
            push(&text[start..index]);
            let Some(close) = text[index + 3..].find("```") else {
                return Sentences {
                    sentences,
                    next: if done { text.len() } else { index },
                };
            };
            index += 3 + close + 3;
            start = index;
            continue;
        }
        let c = text[index..].chars().next().unwrap();
        let after = index + c.len_utf8();
        if c == '\n' {
            push(&text[start..index]);
            start = after;
            index = after;
            continue;
        }
        if matches!(c, '.' | '!' | '?' | '\u{2026}') {
            let mut end = after;
            while let Some(next) = text[end..].chars().next() {
                if !matches!(
                    next,
                    '.' | '!'
                        | '?'
                        | '\u{2026}'
                        | '"'
                        | '\''
                        | '\u{201d}'
                        | '\u{2019}'
                        | ')'
                        | ']'
                        | '\u{00bb}'
                ) {
                    break;
                }
                end += next.len_utf8();
            }
            let Some(next) = text[end..].chars().next() else {
                break;
            };
            if next.is_whitespace() && !ends_with_abbreviation(&text[start..end]) {
                push(&text[start..end]);
                start = end;
            }
            index = end;
            continue;
        }
        if c == ','
            && text[start..index].encode_utf16().count() > 280
            && text[after..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
        {
            push(&text[start..after]);
            start = after;
        }
        index = after;
    }
    if done && start < text.len() {
        push(&text[start..]);
        start = text.len();
    }
    Sentences {
        sentences,
        next: start,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    Spanish,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
        }
    }
}

pub fn default_language(locale: &str) -> Language {
    if locale.to_lowercase().starts_with("es") {
        Language::Spanish
    } else {
        Language::English
    }
}

static WORD_BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}']+").unwrap());
static SPANISH: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "el la los las que de del y en un una unos por para con es est\u{e1} est\u{e1}n son pero muy m\u{e1}s lo se su sus al como tambi\u{e9}n hola s\u{ed} gracias porque cuando donde d\u{f3}nde qu\u{e9} c\u{f3}mo yo t\u{fa} usted nosotros esto eso esta este hay ser tiene tengo puedo puede creo mejor bueno vamos ahora"
        .split_whitespace()
        .collect()
});
static ENGLISH: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "the and is are you to of it that this with for what i we be have has not but so can will just yes hello thanks because when where how my your there think good let now our they them"
        .split_whitespace()
        .collect()
});

/// Detect Spanish or English, keeping the caller's language on a tie.
pub fn reply_language(text: &str, fallback: Language) -> Language {
    let lower = text.to_lowercase();
    let mut spanish = lower
        .chars()
        .filter(|c| {
            matches!(
                c,
                '\u{00f1}'
                    | '\u{00bf}'
                    | '\u{00a1}'
                    | '\u{00e1}'
                    | '\u{00e9}'
                    | '\u{00ed}'
                    | '\u{00f3}'
                    | '\u{00fa}'
            )
        })
        .count()
        * 2;
    let mut english = 0;
    for word in WORD_BREAK.split(&lower) {
        spanish += usize::from(SPANISH.contains(word));
        english += usize::from(ENGLISH.contains(word));
    }
    match spanish.cmp(&english) {
        std::cmp::Ordering::Greater => Language::Spanish,
        std::cmp::Ordering::Less => Language::English,
        std::cmp::Ordering::Equal => fallback,
    }
}

/// Stable voices for each member, avoiding collisions while voices remain.
pub fn member_voices(ids: &[String], speakers: usize) -> HashMap<String, usize> {
    let count = speakers.max(1);
    let mut taken = HashSet::new();
    let mut voices = HashMap::new();
    for id in ids {
        let mut hash = 0x811c9dc5u32;
        for unit in id.encode_utf16() {
            hash ^= u32::from(unit);
            hash = hash.wrapping_mul(0x01000193);
        }
        let mut voice = hash as usize % count;
        for _ in 0..count {
            if !taken.contains(&voice) {
                break;
            }
            voice = (voice + 1) % count;
        }
        taken.insert(voice);
        voices.insert(id.clone(), voice);
    }
    voices
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStatus {
    Said,
    Thinking,
    Passed,
    Busy,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallLine {
    /// "user", or the member's Mono ID.
    pub who: String,
    pub name: String,
    pub text: String,
    pub status: LineStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallTranscript {
    pub title: String,
    pub body: String,
}

static EXCESS_NEWLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

/// The spoken lines to save among the group's documents. The caller formats
/// the call's start time for the user's locale.
pub fn call_transcript(group: &str, date: &str, lines: &[CallLine]) -> Option<CallTranscript> {
    let spoken: Vec<&CallLine> = lines
        .iter()
        .filter(|line| line.status == LineStatus::Said && !line.text.is_empty())
        .collect();
    if !spoken.iter().any(|line| line.who != "user") {
        return None;
    }
    let mut body = vec![format!("# Call with {group}"), format!("_{date}_")];
    body.extend(
        spoken
            .iter()
            .map(|line| format!("**{}:** {}", line.name, line.text)),
    );
    Some(CallTranscript {
        title: format!("Call with {group}, {date}"),
        body: EXCESS_NEWLINES
            .replace_all(&body.join("\n\n"), "\n\n")
            .into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::groups::mentions::Person;

    fn members() -> Vec<Person> {
        [
            ("m-ana", "Ana"),
            ("m-leo", "Leo"),
            ("m-sofia", "Sof\u{00ed}a"),
            ("m-trip", "Trip Planner"),
        ]
        .into_iter()
        .map(|(id, name)| Person::new(id, name))
        .collect()
    }

    #[test]
    fn a_misheard_name_at_the_start_reaches_its_member() {
        let people: Vec<Person> = [
            ("m-jarvis", "Jarvis"),
            ("m-edith", "Edith"),
            ("m-pepers", "Pepers"),
        ]
        .into_iter()
        .map(|(id, name)| Person::new(id, name))
        .collect();
        assert_eq!(
            spoken_address(&people, "Hola yares, cómo está").as_deref(),
            Some("Jarvis, cómo está")
        );
        assert_eq!(
            spoken_address(&people, "Oye, Edit, qué opinas?").as_deref(),
            Some("Edith, qué opinas?")
        );
        // Heard right, nobody called, or nothing after the name: as it was.
        assert_eq!(spoken_address(&people, "Hola Jarvis, cómo está"), None);
        assert_eq!(spoken_address(&people, "Hola, cómo están todos"), None);
        assert_eq!(spoken_address(&people, "Yarbis"), None);
        assert_eq!(spoken_address(&people, "Gracias por todo"), None);
        // The rewritten turn is addressed as usual.
        let fixed = spoken_address(&people, "Hola yares, cómo está").unwrap();
        assert_eq!(turn_order(&people, &fixed, None).order, ["m-jarvis"]);
    }

    #[test]
    fn the_ear_folds_what_sounds_alike() {
        assert_eq!(sound("Jarvis"), sound("yarbis"));
        assert_eq!(sound("Cecilia"), sound("sesilia"));
        assert!(nearness("yares", "Jarvis") <= 0.34);
        assert!(nearness("Mateo", "Jarvis") > 0.34);
    }

    #[test]
    fn selects_named_members_and_rotates_the_first_speaker() {
        let people = members();
        assert_eq!(
            addressed_members(&people, "Sofia y Ana, que opinan?"),
            ["m-sofia", "m-ana"]
        );
        assert_eq!(
            addressed_members(&people, "@leo what do you think"),
            ["m-leo"]
        );
        assert_eq!(
            addressed_members(&people, "ask the trip planner"),
            ["m-trip"]
        );
        assert!(addressed_members(&people, "Leonardo da Vinci painted").is_empty());
        assert_eq!(
            turn_order(&people, "What next?", Some("m-leo")),
            TurnOrder {
                order: ["m-sofia", "m-trip", "m-ana", "m-leo"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                addressed: false,
            }
        );
        assert_eq!(turn_order(&people, "Leo, you first", None).order, ["m-leo"]);
        assert_eq!(turn_order(&people, "hi", Some("gone")).order[0], "m-ana");
        assert!(turn_order::<Person>(&[], "hi", None).order.is_empty());
    }

    #[test]
    fn frames_the_call_and_recognizes_passes() {
        let message = call_message(
            "Trip",
            Some("Plan the trip"),
            &["Ana".into(), "Leo".into()],
            " Friday? ",
            &[("Ana".into(), "Yes.".into())],
            false,
        );
        assert!(message.contains("<group_call group=\"Trip\">"));
        assert!(message.contains("voice call with the user and Ana, Leo."));
        assert!(message.contains("The group is for: Plan the trip"));
        assert!(message.contains("The user said: Friday?"));
        assert!(message.contains("Ana said: Yes."));
        assert!(message.contains("reply with exactly PASS"));
        assert!(!call_message("Trip", None, &[], "Leo?", &[], true).contains("PASS"));
        for text in ["PASS", " pass. ", "**PASS**", "`Pass`"] {
            assert!(is_pass(text));
        }
        assert!(!is_pass("I'll pass on that, but Leo knows"));
        assert!(could_be_pass("PA"));
        assert!(could_be_pass("**"));
        assert!(!could_be_pass("Pablo"));
        assert!(!could_be_pass("S\u{00ed}"));
    }

    #[test]
    fn detects_language_and_assigns_stable_distinct_voices() {
        assert_eq!(default_language("es-PE"), Language::Spanish);
        assert_eq!(default_language("en-US"), Language::English);
        assert_eq!(
            reply_language(
                "Creo que el viernes es mejor para volar.",
                Language::English
            ),
            Language::Spanish
        );
        assert_eq!(
            reply_language(
                "I think Friday is better for the flight.",
                Language::Spanish
            ),
            Language::English
        );
        assert_eq!(
            reply_language("\u{00a1}S\u{00ed}!", Language::English),
            Language::Spanish
        );
        assert_eq!(reply_language("OK", Language::Spanish), Language::Spanish);
        let ids: Vec<_> = members().into_iter().map(|member| member.id).collect();
        let voices = member_voices(&ids, 10);
        assert_eq!(
            voices.values().copied().collect::<HashSet<_>>().len(),
            ids.len()
        );
        assert_eq!(voices, member_voices(&ids, 10));
        assert!(member_voices(&ids, 1).values().all(|voice| *voice == 0));
    }

    #[test]
    fn reads_only_the_requested_members_written_answer() {
        let block = |role: &str, text: &str| Block {
            role: role.into(),
            text: text.into(),
            ..Default::default()
        };
        let mut request = block("user", "call");
        request.app_request_id = Some("call-1".into());
        let mut tool = block("assistant", "tool output");
        tool.tool = Some(serde_json::json!({"title": "Search"}));
        let blocks = vec![
            block("user", "earlier"),
            block("assistant", "Old answer."),
            request,
            block("assistant", "Let me check."),
            tool,
            block("assistant", "Flights are cheap."),
            block("user", "next"),
            block("assistant", "Unrelated."),
        ];
        assert_eq!(
            member_reply(&blocks, "call-1"),
            "Let me check.\n\nFlights are cheap."
        );
        assert_eq!(member_reply(&blocks, "missing"), "");
    }

    #[test]
    fn cleans_markdown_and_streams_finished_sentences() {
        assert_eq!(
            speakable("## Plan\n- **Fly** on [Friday](https://x.y/z) with `LATAM` \u{1f680} see https://a.b"),
            "Plan Fly on Friday with LATAM see"
        );
        let text = "\u{00a1}Hola! Soy Ana. Creo que el viernes es mejor.";
        let first = take_sentences(text, 0, false);
        assert_eq!(first.sentences, ["\u{00a1}Hola!", "Soy Ana."]);
        assert!(take_sentences(text, first.next, false).sentences.is_empty());
        assert_eq!(
            take_sentences(text, first.next, true),
            Sentences {
                sentences: vec!["Creo que el viernes es mejor.".into()],
                next: text.len(),
            }
        );
        let abbreviations = "Cuesta 3.5 soles, dice el Sr. P\u{00e9}rez. J. R. Tolkien wrote it. ";
        assert_eq!(
            take_sentences(abbreviations, 0, false).sentences,
            [
                "Cuesta 3.5 soles, dice el Sr. P\u{00e9}rez.",
                "J. R. Tolkien wrote it."
            ]
        );
        let code = "Here it is:\n```ts\nconst a = 1. b\n```\nDone. Then ```py\nx = 1. y";
        let taken = take_sentences(code, 0, false);
        assert_eq!(taken.sentences, ["Here it is:", "Done.", "Then"]);
        assert!(code[taken.next..].starts_with("```py"));
        assert!(take_sentences(code, taken.next, true).sentences.is_empty());
        let long = format!("{}, y luego m\u{00e1}s", "palabra ".repeat(40));
        let taken = take_sentences(&long, 0, false);
        assert_eq!(taken.sentences.len(), 1);
        assert_eq!(&long[taken.next..], " y luego m\u{00e1}s");
    }

    #[test]
    fn saves_only_spoken_lines_when_a_member_spoke() {
        let line = |who: &str, name: &str, text: &str, status| CallLine {
            who: who.into(),
            name: name.into(),
            text: text.into(),
            status,
        };
        let lines = [
            line("user", "You", "Friday?", LineStatus::Said),
            line("m-ana", "Ana", "Yes.", LineStatus::Said),
            line("m-leo", "Leo", "", LineStatus::Passed),
        ];
        let transcript = call_transcript("Trip", "Nov 4, 2026", &lines).unwrap();
        assert_eq!(transcript.title, "Call with Trip, Nov 4, 2026");
        assert_eq!(
            transcript.body,
            "# Call with Trip\n\n_Nov 4, 2026_\n\n**You:** Friday?\n\n**Ana:** Yes."
        );
        assert!(call_transcript("Trip", "date", &lines[..1]).is_none());
        let mut with_breaks = lines.to_vec();
        with_breaks[1].text = "Yes.\n\n\nFriday.".into();
        assert!(!call_transcript("Trip", "date", &with_breaks)
            .unwrap()
            .body
            .contains("\n\n\n"));
    }
}
