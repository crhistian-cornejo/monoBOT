//! A group learns as it goes (`groupLearning.ts`): once an exchange in its
//! chat settles, a small model reads it and the facts worth keeping join the
//! project's memory, whether or not the lead thought to save them. The
//! members read that memory with every message the group sends them.
use super::{block::Block, flat, js_len, js_prefix};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::LazyLock,
};

const FACTS_MAX: usize = 5;
const FACT_MAX_CHARS: usize = 300;
const EXCHANGE_MAX_CHARS: usize = 12_000;

/// The last block each group's chat was learned up to, by session id. The
/// desktop app keeps it in `localStorage` under [`LearnedMarks::KEY`]; the
/// caller loads and saves it. Without a mark an exchange is read again, and
/// its facts dedupe.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LearnedMarks(pub BTreeMap<String, String>);

impl LearnedMarks {
    pub const KEY: &'static str = "monocode.groupLearned";

    /// Reads the stored JSON; anything but an object is empty.
    pub fn parse(raw: &str) -> Self {
        let value: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
        Self(
            value
                .as_object()
                .map(|marks| {
                    marks
                        .iter()
                        .filter_map(|(session, block)| {
                            Some((session.clone(), block.as_str()?.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        )
    }

    pub fn learned_up_to(&self, session_id: &str) -> Option<&str> {
        self.0.get(session_id).map(String::as_str)
    }

    pub fn mark_learned(&mut self, session_id: &str, block_id: &str) {
        self.0.insert(session_id.into(), block_id.into());
    }
}

/// What was said since the chat was last learned from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupExchange {
    /// The block it ends on, the mark for next time.
    pub last_id: String,
    pub lines: Vec<String>,
}

/// What was said since the chat was last learned from: the user's messages,
/// the members' answers and the lead's replies. None until the user has
/// written something new and someone answered it.
pub fn pending_exchange(
    blocks: &[Block],
    mark: Option<&str>,
    lead_name: &str,
) -> Option<GroupExchange> {
    let start = match mark.filter(|mark| !mark.is_empty()) {
        Some(mark) => blocks
            .iter()
            .position(|block| block.id == mark)
            .map_or(0, |index| index + 1),
        None => 0,
    };
    let mut lines = Vec::new();
    let mut user_wrote = false;
    let mut answered = false;
    let mut last_id = None;
    for block in &blocks[start..] {
        if block.internal || block.draft || block.tool.is_some() || block.text.trim().is_empty() {
            continue;
        }
        let text = flat(&block.text);
        if block.is_user() {
            user_wrote = true;
            lines.push(format!("User: {text}"));
        } else if block.is_assistant() {
            answered = answered || user_wrote;
            let who = block
                .group_member
                .as_ref()
                .map_or(lead_name, |post| post.name.as_str());
            lines.push(format!("{who}: {text}"));
        } else {
            continue;
        }
        last_id = Some(block.id.clone());
    }
    let last_id = last_id.filter(|_| user_wrote && answered)?;
    let mut budget = EXCHANGE_MAX_CHARS as i64;
    let mut kept = Vec::new();
    for line in lines.iter().rev() {
        if budget <= 0 {
            break;
        }
        kept.insert(0, js_prefix(line, budget as usize).to_string());
        budget -= js_len(line) as i64;
    }
    Some(GroupExchange {
        last_id,
        lines: kept,
    })
}

/// What the small model reads to pick the facts worth keeping.
pub fn learning_prompt(
    group_name: &str,
    goal: Option<&str>,
    memory: &str,
    exchange: &GroupExchange,
) -> String {
    let goal = goal
        .filter(|goal| !goal.is_empty())
        .map(|goal| format!(", which is for: {goal}"))
        .unwrap_or_default();
    let memory = if memory.trim().is_empty() {
        "(empty)"
    } else {
        memory.trim()
    };
    let mut lines = vec![
        format!("You keep the memory of the project \"{group_name}\"{goal}. Below is what it already remembers and the latest exchange in its group chat."),
        "List the facts from the exchange that the project will need again: decisions, the user's preferences and corrections, deadlines, people and their roles, names of files, folders and links that matter, and what was agreed to do next. Leave out greetings, what is already remembered, one-off answers and anything that only held for this exchange. Never include passwords, tokens or keys.".into(),
        format!("Reply with at most {FACTS_MAX} lines, each \"- \" and one short self-contained fact in the language of the exchange. If there is nothing worth keeping, reply exactly NONE."),
        String::new(),
        "<memory>".into(),
        memory.into(),
        "</memory>".into(),
        String::new(),
        "<exchange>".into(),
    ];
    lines.extend(exchange.lines.iter().cloned());
    lines.push("</exchange>".into());
    lines.join("\n")
}

static NONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s*NONE\s*$").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-*•]\s+").unwrap());

/// The facts in the model's answer, without those already remembered.
pub fn parse_learned(output: &str, memory: &str) -> Vec<String> {
    if NONE.is_match(output) {
        return Vec::new();
    }
    let known = memory.to_lowercase();
    let mut seen = HashSet::new();
    output
        .split('\n')
        .map(str::trim)
        .filter_map(|line| BULLET.find(line).map(|bullet| line[bullet.end()..].trim()))
        .filter(|fact| {
            let key = fact.to_lowercase();
            if fact.is_empty() || js_len(fact) > FACT_MAX_CHARS || key == "none" {
                return false;
            }
            if known.contains(&key) || seen.contains(&key) {
                return false;
            }
            seen.insert(key);
            true
        })
        .take(FACTS_MAX)
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::groups::{
        block::GroupMemberPost,
        group::{group_message, project_memory},
    };

    fn block(id: &str, role: &str, text: &str) -> Block {
        Block {
            id: id.into(),
            role: role.into(),
            text: text.into(),
            ..Block::default()
        }
    }

    fn blocks() -> Vec<Block> {
        vec![
            block("u1", "user", "Usemos el informe de octubre"),
            Block {
                internal: true,
                ..block("c1", "user", "hidden")
            },
            block("l1", "assistant", "Documents lo revisa"),
            Block {
                group_member: Some(GroupMemberPost {
                    mono_id: "d".into(),
                    name: "Documents".into(),
                    mascot: "m".into(),
                    color: "c".into(),
                    request: "r".into(),
                    at: 1,
                    ..GroupMemberPost::default()
                }),
                ..block("p1", "assistant", "Listo: 3 m/s")
            },
        ]
    }

    #[test]
    fn groups_read_the_exchange_since_the_mark_naming_who_said_what() {
        let blocks = blocks();
        let exchange = pending_exchange(&blocks, None, "IVY").unwrap();
        assert_eq!(
            exchange,
            GroupExchange {
                last_id: "p1".into(),
                lines: vec![
                    "User: Usemos el informe de octubre".into(),
                    "IVY: Documents lo revisa".into(),
                    "Documents: Listo: 3 m/s".into(),
                ],
            }
        );
        assert_eq!(pending_exchange(&blocks, Some("p1"), "IVY"), None);
        // Members answering after the mark wait for the user's next message.
        assert_eq!(pending_exchange(&blocks, Some("l1"), "IVY"), None);
        let prompt = learning_prompt("Nexa", Some("Informe IF"), "- 2026-11-01 · Lima", &exchange);
        assert!(prompt.contains("\"Nexa\", which is for: Informe IF"));
        assert!(prompt.contains("Documents: Listo: 3 m/s"));
        assert!(prompt.contains("NONE"));
    }

    #[test]
    fn groups_keep_new_short_facts_only() {
        let memory = "- 2026-11-01 · El cliente es Nexa";
        assert_eq!(parse_learned("NONE", memory), Vec::<String>::new());
        let output = format!(
            "Facts:\n- El cliente es Nexa\n- Usar el informe de octubre\n* Usar el informe de octubre\n- none\n- {}",
            "x".repeat(400)
        );
        assert_eq!(
            parse_learned(&output, memory),
            vec!["Usar el informe de octubre"]
        );
    }

    #[test]
    fn groups_hand_members_the_projects_live_memory() {
        assert_eq!(
            project_memory(Some("# Memory\n- a\n- ~~old~~\n* b\ntext")),
            "- a\n* b"
        );
        let message = group_message(
            "Nexa",
            None,
            "Revisa",
            None,
            Some("- 2026 · Usar octubre"),
            false,
        );
        assert!(message.contains("What the project remembers"));
        assert!(message.contains("- 2026 · Usar octubre"));
        assert!(!group_message("Nexa", None, "Revisa", None, None, false).contains("remembers"));
    }

    #[test]
    fn groups_remember_where_each_chat_was_learned_up_to() {
        let mut marks = LearnedMarks::parse("[1]");
        assert_eq!(marks.learned_up_to("g1"), None);
        marks.mark_learned("g1", "p1");
        let saved = serde_json::to_string(&marks).unwrap();
        assert_eq!(saved, "{\"g1\":\"p1\"}");
        assert_eq!(LearnedMarks::parse(&saved).learned_up_to("g1"), Some("p1"));
    }
}
