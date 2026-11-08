//! "Find in conversation": which messages of a transcript hold a query.
//!
//! Ports `src/features/sessions/model/transcriptFind.ts`. Prompts, replies,
//! tool steps, plans and generated images are searched, each by what the
//! chat shows of it; thoughts and app bookkeeping are not.
use super::Row;
use crate::tool::ToolCall;
use std::borrow::Cow;

/// What a message of `role` can be found by, from its joined `text`.
pub fn searchable<'a>(role: &str, text: &'a str) -> Option<Cow<'a, str>> {
    match role {
        "user" | "assistant" => Some(Cow::Borrowed(text)),
        "tool" => {
            let call = serde_json::from_str::<ToolCall>(text).ok()?;
            let shown = call.presentation();
            let mut parts = vec![shown.action, shown.target];
            parts.extend(shown.description);
            parts.extend(shown.command);
            if let Some(plan) = crate::plan::exit_plan_mode_plan(&call.input) {
                parts.push(plan);
            }
            parts.push(call.output);
            Some(Cow::Owned(parts.join("\n")))
        }
        crate::plan::ROLE => serde_json::from_str::<crate::plan::PlanRow>(text)
            .ok()
            .map(|plan| Cow::Owned(plan.text)),
        crate::images::REPLY_ROLE => serde_json::from_str::<crate::images::GeneratedImage>(text)
            .ok()
            .map(|image| Cow::Owned(format!("{}\n{}", image.name, image.alt.unwrap_or_default()))),
        _ => None,
    }
}

/// The first rows of the messages that contain `query`, case-insensitively,
/// in transcript order. An empty query finds nothing.
pub fn find(rows: &[Row], query: &str) -> Vec<usize> {
    let needle = query.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let Ok(matcher) = regex::RegexBuilder::new(&regex::escape(needle))
        .case_insensitive(true)
        .build()
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut at = 0;
    while at < rows.len() {
        let start = at;
        at += 1;
        while at < rows.len() && !rows[at].first {
            at += 1;
        }
        let text: Cow<str> = if at - start == 1 {
            Cow::Borrowed(rows[start].text.as_str())
        } else {
            Cow::Owned(
                rows[start..at]
                    .iter()
                    .map(|row| row.text.as_str())
                    .collect(),
            )
        };
        if searchable(&rows[start].role, &text).is_some_and(|text| matcher.is_match(&text)) {
            found.push(start);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolState;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    #[test]
    fn finds_prompts_replies_tools_plans_and_images() {
        let mut tool = ToolCall::new("t", "Bash");
        tool.input = r#"{"command":"cargo test --workspace"}"#.into();
        tool.output = "test result: ok".into();
        tool.state = ToolState::Completed;
        let rows = vec![
            row("user", "Fix the Parser"),
            row("reasoning", "parser thoughts stay private"),
            row("assistant", "The parser"),
            Row {
                first: false,
                ..row("assistant", " is fixed")
            },
            row("tool", &serde_json::to_string(&tool).unwrap()),
            row(crate::plan::ROLE, r##"{"text":"# Rewrite the parser"}"##),
            row(
                crate::images::REPLY_ROLE,
                r#"{"path":"/x.png","name":"fox.png","mimeType":"image/png","size":1,"alt":"A fox"}"#,
            ),
        ];
        assert_eq!(find(&rows, "PARSER"), vec![0, 2, 5]);
        // A match across a message's row boundary is still one match.
        assert_eq!(find(&rows, "parser is fixed"), vec![2]);
        assert_eq!(find(&rows, "cargo test"), vec![4]);
        assert_eq!(find(&rows, "result: ok"), vec![4]);
        assert_eq!(find(&rows, "a fox"), vec![6]);
        assert_eq!(find(&rows, "thoughts"), Vec::<usize>::new());
        assert_eq!(find(&rows, "   "), Vec::<usize>::new());
        // Regex characters are literal.
        assert_eq!(find(&rows, "pars.r"), Vec::<usize>::new());
    }
}
