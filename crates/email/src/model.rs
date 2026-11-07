//! What the page gets: the accounts and their messages, in the shape of `apps/desktop/src/features/mail/model/types.ts`.

use serde::{Deserialize, Serialize};

/// Where a message is on its account. The page's «Prioridad» and «Destacados» are views over these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Folder {
    Inbox,
    Sent,
    Drafts,
    Archive,
}

impl Folder {
    pub fn as_str(self) -> &'static str {
        match self {
            Folder::Inbox => "inbox",
            Folder::Sent => "sent",
            Folder::Drafts => "drafts",
            Folder::Archive => "archive",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "inbox" => Folder::Inbox,
            "sent" => Folder::Sent,
            "drafts" => Folder::Drafts,
            "archive" => Folder::Archive,
            _ => return None,
        })
    }
}

/// What Echo made of a message: the sender is one the user marked as interesting, it is routine, or it is held back
/// (spam, a notice of shipping, a newsletter, anything its checks find dangerous) until the user decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Triage {
    Priority,
    Routine,
    Held,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub account_id: String,
    pub folder: Folder,
    pub from: String,
    pub address: String,
    pub subject: String,
    pub preview: String,
    /// The text, one entry per paragraph. The page shows it as text, never as HTML.
    pub body: Vec<String>,
    /// Milliseconds since the epoch.
    pub at: i64,
    pub unread: bool,
    pub starred: bool,
    pub triage: Triage,
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
    /// The sender failed SPF, DKIM or DMARC: a fact the core hands to Decider to weigh, never a rule of its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auth_failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub name: String,
    pub address: String,
    pub provider: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub accounts: Vec<AccountView>,
    pub messages: Vec<Message>,
    /// What could not be read, one line each, in the user's words (a wrong password, no connection).
    pub problems: Vec<String>,
    /// How many each mailbox holds on its server: `messages` only has the newest of each.
    #[serde(default)]
    pub totals: Vec<BoxTotal>,
}

/// How many messages a mailbox of an account holds, as its server last said. A mailbox that could not be opened has none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxTotal {
    pub account_id: String,
    pub folder: Folder,
    pub total: u32,
}

impl Overview {
    /// One more or one less in a mailbox, until the next reading says how many there are.
    pub(crate) fn recount(&mut self, account: &str, folder: Folder, by: i32) {
        if let Some(entry) = self
            .totals
            .iter_mut()
            .find(|t| t.account_id == account && t.folder == folder)
        {
            entry.total = entry.total.saturating_add_signed(by);
        }
    }
}

/// A message read whole, for the reader: its HTML made safe (`sanitize.rs`), its text, and the pictures on the web it
/// names, which the owner allows one by one (`Mail::image`). The frame that shows the HTML runs nothing and loads nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub id: String,
    /// Empty when the message has no HTML part.
    pub html: String,
    pub text: Vec<String>,
    pub remote: Vec<String>,
    /// The owner allowed this message's pictures on the web before (the core keeps that choice): they load at once.
    pub images_allowed: bool,
    pub attachments: Vec<Attachment>,
}

/// A file that came with a message: what the reader lists. Its bytes are not here: they are fetched from the server only
/// when the owner opens one (`Mail::attachment`), by its `index` among the message's attachments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub index: usize,
    /// A name that is safe to give a file (no folder, no character a system refuses).
    pub name: String,
    pub kind: String,
    pub size: usize,
    /// What a look at its bytes found (`inspect.rs`): nothing, something to be careful with, or a file not to open.
    pub risk: crate::inspect::Risk,
    /// A picture's own bytes as a data URL, for the reader to show in place: nothing is saved to disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// What the owner does to a message on its server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Star,
    Unstar,
    Archive,
}

impl Action {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "read" => Action::Read,
            "star" => Action::Star,
            "unstar" => Action::Unstar,
            "archive" => Action::Archive,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_said_to_the_page_in_camel_case_without_what_is_empty() {
        let message = Message {
            id: "a.inbox.7".into(),
            account_id: "a".into(),
            folder: Folder::Inbox,
            from: "Ana".into(),
            address: "ana@ejemplo.test".into(),
            subject: "Hola".into(),
            preview: "Hola".into(),
            body: vec!["Hola".into()],
            at: 1,
            unread: true,
            starred: false,
            triage: Triage::Held,
            labels: vec![],
            reason: None,
            auth_failed: false,
        };
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["accountId"], "a");
        assert_eq!(json["folder"], "inbox");
        assert_eq!(json["triage"], "held");
        assert!(json.get("reason").is_none());
    }

    #[test]
    fn folders_and_actions_are_read_by_name() {
        assert_eq!(Folder::parse("drafts"), Some(Folder::Drafts));
        assert_eq!(Folder::parse("../x"), None);
        assert_eq!(Action::parse("archive"), Some(Action::Archive));
        assert_eq!(Action::parse("delete"), None);
    }
}
