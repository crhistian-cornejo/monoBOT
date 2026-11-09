//! A group call through a live model (Gemini Live or Phonic): the model hosts
//! the call with one voice, hears the user, and hands each request a member
//! should answer to that member's own seat through `ask_member`, as
//! `ivy-review.md` designs it. One live session for the whole group keeps the
//! cost down and the echo simple.
use super::call::{call_message, nearness, words};
use super::mentions::Named;
use serde_json::{json, Value};

/// The tool the host calls to reach a member.
pub const ASK_MEMBER: &str = "ask_member";
/// The longest request a member is handed.
const MAX_REQUEST: usize = 4_000;

/// What the live model is told: who is on the call and how to reach them.
pub fn instructions(group: &str, goal: Option<&str>, members: &[(String, String)]) -> String {
    let roster = members
        .iter()
        .map(|(name, about)| {
            if about.trim().is_empty() {
                format!("- {name}")
            } else {
                format!("- {name}: {}", about.trim())
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let goal = goal
        .filter(|goal| !goal.trim().is_empty())
        .map(|goal| format!("The group is for: {}\n", goal.trim()))
        .unwrap_or_default();
    format!(
        "You host a voice call between the user and their agents in the group \"{group}\".\n{goal}\
         The agents on the call:\n{roster}\n\n\
         Speak in the language the user speaks, briefly, as plain speech.\n\
         When the user asks for something one of the agents should do or know, or names an agent, call {ASK_MEMBER} with that agent's name and the whole request, including what the user said before that it needs. You may ask several agents.\n\
         While an agent works, tell the user it is on it; when its answer arrives, tell the user what it said in your own words.\n\
         Never make up an agent's answer, and never claim something was done before its answer says so.\n\
         Small talk and questions about the call itself you answer yourself."
    )
}

/// `ask_member`'s declaration, with the members' names as its choices.
pub fn ask_member_schema(names: &[String]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "member": {
                "type": "string",
                "enum": names,
                "description": "The agent to ask, by name.",
            },
            "request": {
                "type": "string",
                "description": "The whole request for that agent, with the context it needs.",
            },
        },
        "required": ["member", "request"],
    })
}

pub fn ask_member_description() -> &'static str {
    "Ask one of the agents on this call to do something or answer something. It works in its own session with its own tools; slow work answers later."
}

/// The member and request an `ask_member` call names. A name the model
/// spelled differently («Edit» for «Edith») still finds its member.
pub fn ask_target<T: Named>(members: &[T], args: &Value) -> Result<(String, String), String> {
    let request = args
        .get("request")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if request.is_empty() {
        return Err("Say what to ask the agent.".into());
    }
    if request.len() > MAX_REQUEST {
        return Err("That request is too long; say it in a few sentences.".into());
    }
    let named = args
        .get("member")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let folded = words(named);
    let exact = members
        .iter()
        .find(|member| words(member.name()) == folded || member.id() == named);
    let member = exact.or_else(|| {
        members
            .iter()
            .map(|member| (nearness(named, member.name()), member))
            .filter(|(distance, _)| *distance <= 0.34)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, member)| member)
    });
    match member {
        Some(member) => Ok((member.id().to_owned(), request.to_owned())),
        None => Err(format!(
            "No agent called {named} is on this call. The agents are: {}.",
            members
                .iter()
                .map(|member| member.name())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// What the member's seat is sent: the request, framed as a call turn
/// addressed to it, so it answers for speech and never passes.
pub fn member_prompt(group: &str, goal: Option<&str>, others: &[String], request: &str) -> String {
    call_message(group, goal, others, request, &[], true)
}

/// The tool's answer to the live model.
pub fn answer(member: &str, reply: Result<&str, &str>) -> Value {
    match reply {
        Ok(text) if !text.trim().is_empty() => json!({"member": member, "answer": text.trim()}),
        Ok(_) => json!({"member": member, "answer": "(no answer)"}),
        Err(why) => json!({"member": member, "error": why}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::groups::mentions::Person;

    fn members() -> Vec<Person> {
        [
            ("m-jarvis", "Jarvis"),
            ("m-edith", "Edith"),
            ("m-trip", "Trip Planner"),
        ]
        .into_iter()
        .map(|(id, name)| Person::new(id, name))
        .collect()
    }

    #[test]
    fn the_host_knows_who_is_on_the_call_and_how_to_ask() {
        let text = instructions(
            "Agency",
            Some("Plan trips"),
            &[
                ("Jarvis".into(), "Research".into()),
                ("Edith".into(), String::new()),
            ],
        );
        assert!(text.contains("\"Agency\""));
        assert!(text.contains("The group is for: Plan trips"));
        assert!(text.contains("- Jarvis: Research"));
        assert!(text.contains("- Edith\n"));
        assert!(text.contains(ASK_MEMBER));
        assert!(text.contains("Never make up"));
        assert!(!instructions("G", Some("  "), &[]).contains("The group is for"));
    }

    #[test]
    fn the_schema_offers_the_members_by_name() {
        let schema = ask_member_schema(&["Jarvis".into(), "Edith".into()]);
        assert_eq!(
            schema["properties"]["member"]["enum"],
            json!(["Jarvis", "Edith"])
        );
        assert_eq!(schema["required"], json!(["member", "request"]));
    }

    #[test]
    fn a_named_member_is_found_even_misspelled() {
        let people = members();
        let ask = |member: &str| {
            ask_target(
                &people,
                &json!({"member": member, "request": " Busca vuelos "}),
            )
        };
        assert_eq!(
            ask("Jarvis"),
            Ok(("m-jarvis".into(), "Busca vuelos".into()))
        );
        assert_eq!(ask("jarvis").unwrap().0, "m-jarvis");
        assert_eq!(ask("Edit").unwrap().0, "m-edith");
        assert_eq!(ask("trip planner").unwrap().0, "m-trip");
        assert_eq!(ask("m-edith").unwrap().0, "m-edith", "by id too");
        let nobody = ask("Mateo").unwrap_err();
        assert!(nobody.contains("Jarvis, Edith, Trip Planner"), "{nobody}");
    }

    #[test]
    fn an_empty_or_huge_request_is_refused() {
        let people = members();
        assert!(ask_target(&people, &json!({"member": "Jarvis"})).is_err());
        assert!(ask_target(&people, &json!({"member": "Jarvis", "request": "  "})).is_err());
        let huge = "x".repeat(MAX_REQUEST + 1);
        assert!(ask_target(&people, &json!({"member": "Jarvis", "request": huge})).is_err());
    }

    #[test]
    fn a_member_is_asked_as_addressed_and_answers_back() {
        let prompt = member_prompt("Agency", None, &["Edith".into()], "Busca vuelos a Lima");
        assert!(prompt.contains("The user said: Busca vuelos a Lima"));
        assert!(!prompt.contains("PASS"), "an addressed member never passes");
        assert_eq!(
            answer("Jarvis", Ok(" Hay tres vuelos. ")),
            json!({"member": "Jarvis", "answer": "Hay tres vuelos."})
        );
        assert_eq!(
            answer("Jarvis", Ok("")),
            json!({"member": "Jarvis", "answer": "(no answer)"})
        );
        assert_eq!(
            answer("Jarvis", Err("busy")),
            json!({"member": "Jarvis", "error": "busy"})
        );
    }
}
