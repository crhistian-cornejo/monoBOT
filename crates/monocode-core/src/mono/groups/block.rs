//! The parts of a desktop session's block that groups read: who spoke, what,
//! which app request it answers and whether it is a member's post. It reads
//! a desktop session's `blocks` JSON as it is (camelCase) and ignores the rest.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    /// `user`, `assistant`, `system` or `image`.
    pub role: String,
    pub text: String,
    /// Epoch ms when this user turn started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    /// Epoch ms when this message joined a turn that was already running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<i64>,
    /// Saved to the session but not sent yet.
    #[serde(skip_serializing_if = "is_false")]
    pub draft: bool,
    /// The app request that sent this user turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_request_id: Option<String>,
    /// Written by the app on the user's behalf; the transcript hides it.
    #[serde(skip_serializing_if = "is_false")]
    pub internal: bool,
    /// Set on a tool call; groups only ask whether it is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<Value>,
    /// Set on a subagent run (Agent/Task tool blocks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_run: Option<Value>,
    /// `error` or `interrupt` on a system row the reader must not miss.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_metrics: Option<TurnMetrics>,
    /// A group member's answer, posted in the group's chat under its name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_member: Option<GroupMemberPost>,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl Block {
    pub fn is_user(&self) -> bool {
        self.role == "user"
    }
    pub fn is_assistant(&self) -> bool {
        self.role == "assistant"
    }
    /// A message the user wrote rather than the app on their behalf.
    pub(crate) fn is_user_turn(&self) -> bool {
        self.is_user() && !self.internal
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TurnMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

/// A member's answer as the group's chat shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GroupMemberPost {
    pub mono_id: String,
    pub name: String,
    pub mascot: String,
    pub color: String,
    /// The member's request id; one post per request.
    pub request: String,
    pub at: i64,
    /// What it answered with, for the mark on its avatar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_read_a_desktop_block_as_it_is() {
        let block: Block = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "role": "assistant",
            "text": "Done",
            "streaming": false,
            "groupMember": {
                "monoId": "docs", "name": "Documents", "mascot": "m", "color": "c",
                "request": "app-g1-r1", "at": 5, "harness": "claude"
            }
        }))
        .unwrap();
        assert_eq!(block.group_member.as_ref().unwrap().request, "app-g1-r1");
        let back = serde_json::to_value(&block).unwrap();
        assert_eq!(back["groupMember"]["monoId"], "docs");
        assert!(back.get("draft").is_none());
    }
}
