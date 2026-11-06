//! Who a group message is for when it names someone with @Name, anywhere in
//! it (`groupMentions.ts`): one member, the lead, or nobody in particular.
//! Names may hold spaces, so the longest name that matches wins.
use super::js_len;
use regex::Regex;
use std::{collections::HashSet, sync::LazyLock};
use unicode_normalization::UnicodeNormalization;

/// One of the people a message can name with @.
pub trait Named {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
}

/// Someone a message can name: an id and a display name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Person {
    pub id: String,
    pub name: String,
}

impl Person {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

impl Named for Person {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
}

impl Named for super::brief::Member {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
}

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_]").unwrap());
static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{M}$").unwrap());
static ADDRESS_GAP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\s,:;.-]+").unwrap());

/// Whether a letter, digit or underscore opens `text`: what keeps a name going.
pub(crate) fn starts_with_word(text: &str) -> bool {
    WORD.is_match(text)
}

fn is_word(c: char) -> bool {
    starts_with_word(c.encode_utf8(&mut [0; 4]))
}

/// Accents and case aside: "Análisis" reads as "analisis".
fn fold(text: &str) -> String {
    text.nfd()
        .filter(|c| !MARK.is_match(c.encode_utf8(&mut [0; 4])))
        .collect::<String>()
        .to_lowercase()
}

/// An @Name in a message: who, and where it starts and ends (byte offsets
/// into the message, `@` included).
#[derive(Debug, PartialEq, Eq)]
pub struct Mention<'a, T> {
    pub person: &'a T,
    pub start: usize,
    pub end: usize,
}

impl<T> Clone for Mention<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Mention<'_, T> {}

/// Each @Name in `text`, by the index of the name in `names`.
fn find_mentions(text: &str, names: &[&str]) -> Vec<(usize, usize, usize)> {
    let mut named: Vec<(usize, Vec<char>)> = names
        .iter()
        .enumerate()
        .filter(|(_, name)| !name.trim().is_empty())
        .map(|(index, name)| (index, fold(name.trim()).chars().collect()))
        .collect();
    // Stable: of names as long, the first listed wins.
    named.sort_by_key(|(_, name)| std::cmp::Reverse(js_len(&name.iter().collect::<String>())));
    let original: Vec<(usize, char)> = text.char_indices().collect();
    // Folding removes accents one for one only on text without combining
    // marks; fall back to case alone when lengths differ.
    let folded: Vec<char> = fold(text).chars().collect();
    let haystack: Vec<char> = if folded.len() == original.len() {
        folded
    } else {
        let lower: Vec<char> = text.to_lowercase().chars().collect();
        if lower.len() == original.len() {
            lower
        } else {
            original
                .iter()
                .map(|&(_, c)| c.to_lowercase().next().unwrap_or(c))
                .collect()
        }
    };
    let offset = |at: usize| original.get(at).map_or(text.len(), |&(byte, _)| byte);
    let mut found = Vec::new();
    for at in (0..haystack.len()).filter(|&at| haystack[at] == '@') {
        // An address (ana@gmail.com) is not a mention.
        if at > 0 && is_word(haystack[at - 1]) {
            continue;
        }
        let hit = named.iter().find(|(_, name)| {
            let end = at + 1 + name.len();
            haystack.get(at + 1..end) == Some(name.as_slice())
                && !haystack.get(end).is_some_and(|&c| is_word(c))
        });
        if let Some((index, name)) = hit {
            found.push((*index, offset(at), offset(at + 1 + name.len())));
        }
    }
    found
}

/// Each @Name in `text`, in order, with where it starts and ends.
pub fn mentions_in<'a, T: Named>(text: &str, people: &'a [T]) -> Vec<Mention<'a, T>> {
    let names: Vec<&str> = people.iter().map(Named::name).collect();
    find_mentions(text, &names)
        .into_iter()
        .map(|(index, start, end)| Mention {
            person: &people[index],
            start,
            end,
        })
        .collect()
}

/// Whom a message names alone.
#[derive(Debug, PartialEq, Eq)]
pub enum Addressed<'a, T> {
    /// Only this member answers, around the lead.
    Member { member: &'a T, message: String },
    /// Only the lead answers, without a round.
    Lead,
}

/// Whom a message names alone. One member: that member answers, and gets the
/// message without its @ («oye @Finance, ¿cuánto gasté?» → «oye Finance,
/// ¿cuánto gasté?»; a leading @Finance is dropped). The lead: the lead alone.
/// Several people, or nobody: None, and the whole group takes it.
pub fn addressed_member<'a, T: Named>(
    text: &str,
    members: &'a [T],
    lead: Option<&dyn Named>,
) -> Option<Addressed<'a, T>> {
    let mut people: Vec<(&str, &str)> = members
        .iter()
        .map(|member| (member.id(), member.name()))
        .collect();
    if let Some(lead) = lead {
        people.push((lead.id(), lead.name()));
    }
    let names: Vec<&str> = people.iter().map(|(_, name)| *name).collect();
    let mentions = find_mentions(text, &names);
    let ids: HashSet<&str> = mentions
        .iter()
        .map(|(index, _, _)| people[*index].0)
        .collect();
    if ids.len() != 1 {
        return None;
    }
    let (id, name) = people[mentions[0].0];
    let member = members.iter().find(|member| member.id() == id);
    let Some(member) = member else {
        // Only the lead, which is not one of the members, is left.
        return Some(Addressed::Lead);
    };
    let mut message = String::new();
    let mut from = 0;
    for &(_, start, end) in &mentions {
        message.push_str(&text[from..start]);
        // A name that opens the message is only the address.
        if !text[..start].trim().is_empty() {
            message.push_str(name.trim());
        }
        from = end;
    }
    message.push_str(&text[from..]);
    let gap = ADDRESS_GAP.find(&message).map_or(0, |gap| gap.end());
    let message = message[gap..].trim().to_string();
    if message.is_empty() {
        return None;
    }
    Some(Addressed::Member { member, message })
}

/// The people an @ being typed can name, those whose name starts with it first.
pub fn rank_mention_people<'a, T: Named>(people: &'a [T], query: &str) -> Vec<&'a T> {
    let wanted = fold(query.trim());
    if wanted.is_empty() {
        return people.iter().collect();
    }
    let mut starts = Vec::new();
    let mut holds = Vec::new();
    for person in people {
        let name = fold(person.name());
        if name.starts_with(&wanted)
            || name
                .split_whitespace()
                .any(|word| word.starts_with(&wanted))
        {
            starts.push(person);
        } else if name.contains(&wanted) {
            holds.push(person);
        }
    }
    starts.extend(holds);
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members() -> Vec<Person> {
        vec![
            Person::new("fin", "Finance"),
            Person::new("docs", "Documents"),
            Person::new("asis", "Mi Asistente"),
            Person::new("mi", "Mi"),
        ]
    }
    fn lead() -> Person {
        Person::new("ivy", "Ivy")
    }
    fn to<'a>(member: &'a Person, message: &str) -> Option<Addressed<'a, Person>> {
        Some(Addressed::Member {
            member,
            message: message.into(),
        })
    }
    fn message_of(addressed: Option<Addressed<'_, Person>>) -> Option<(String, String)> {
        match addressed? {
            Addressed::Member { member, message } => Some((member.id.clone(), message)),
            Addressed::Lead => None,
        }
    }

    #[test]
    fn groups_send_a_message_that_opens_with_at_name_to_that_member_without_the_name() {
        let (members, lead) = (members(), lead());
        assert_eq!(
            addressed_member("@Finance ¿cuánto gasté este mes?", &members, Some(&lead)),
            to(&members[0], "¿cuánto gasté este mes?")
        );
        assert_eq!(
            message_of(addressed_member(
                "@finance, revisa el correo",
                &members,
                Some(&lead)
            ))
            .unwrap()
            .1,
            "revisa el correo"
        );
    }

    #[test]
    fn groups_find_the_name_anywhere_and_keep_it_as_a_plain_word() {
        let (members, lead) = (members(), lead());
        assert_eq!(
            addressed_member("oye @Finance, ¿cuánto gasté?", &members, Some(&lead)),
            to(&members[0], "oye Finance, ¿cuánto gasté?")
        );
    }

    #[test]
    fn groups_prefer_the_longest_name_spaces_included() {
        let members = members();
        assert_eq!(
            message_of(addressed_member(
                "@Mi Asistente agenda la reunión",
                &members,
                None
            )),
            Some(("asis".into(), "agenda la reunión".into()))
        );
        assert_eq!(
            message_of(addressed_member("@Mi dime algo", &members, None))
                .unwrap()
                .0,
            "mi"
        );
    }

    #[test]
    fn groups_give_the_lead_its_own_messages_without_a_round() {
        let (members, lead) = (members(), lead());
        assert_eq!(
            addressed_member("@Ivy ¿qué opinas?", &members, Some(&lead)),
            Some(Addressed::Lead)
        );
    }

    #[test]
    fn groups_leave_several_people_nobody_or_only_a_name_to_the_whole_group() {
        let (members, lead) = (members(), lead());
        let lead = Some(&lead as &dyn Named);
        assert_eq!(
            addressed_member("@Finance y @Documents hagan el Excel", &members, lead),
            None
        );
        assert_eq!(addressed_member("@Finance y @Ivy", &members, lead), None);
        assert_eq!(addressed_member("¿qué hacemos hoy?", &members, lead), None);
        assert_eq!(addressed_member("@Finance", &members, lead), None);
        assert_eq!(addressed_member("@Financiero dime", &members, lead), None);
    }

    #[test]
    fn groups_count_one_member_named_twice_as_one() {
        let members = members();
        assert_eq!(
            message_of(addressed_member(
                "@Finance mira esto, @Finance",
                &members,
                None
            )),
            Some(("fin".into(), "mira esto, Finance".into()))
        );
    }

    #[test]
    fn groups_do_not_take_an_email_address_for_a_mention() {
        assert_eq!(mentions_in("escribe a ana@Finance.com", &members()), vec![]);
    }

    #[test]
    fn groups_ignore_accents_and_case() {
        let asistente = vec![Person::new("a", "Asistente")];
        assert_eq!(
            message_of(addressed_member("@ASISTENTE hola", &asistente, None))
                .unwrap()
                .1,
            "hola"
        );
        let analisis = vec![Person::new("a", "Analisis")];
        assert_eq!(
            message_of(addressed_member("@Análisis hola", &analisis, None))
                .unwrap()
                .1,
            "hola"
        );
        // Offsets are bytes into the message as written, accents and all.
        let text = "ver @Análisis ya";
        let found = mentions_in(text, &analisis);
        assert_eq!(&text[found[0].start..found[0].end], "@Análisis");
    }

    #[test]
    fn groups_list_names_that_start_with_what_was_typed_first() {
        let members = members();
        let ids = |query: &str| -> Vec<String> {
            rank_mention_people(&members, query)
                .iter()
                .map(|p| p.id.clone())
                .collect()
        };
        assert_eq!(ids("as"), vec!["asis"]);
        assert_eq!(ids("mi"), vec!["asis", "mi"]);
        assert_eq!(ids("nan"), vec!["fin"]);
        assert_eq!(ids("").len(), 4);
    }
}
