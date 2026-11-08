//! monobot-email, ported from Echo's echo-email (MIT, Echo contributors): the owner's mail, over IMAP with an app password (Gmail, iCloud or any server): the accounts
//! read together, the messages sorted by rules (priority, routine, held back), what the owner does to one (read,
//! star, archive), a reply saved as a draft and the reply the owner sends with their own press (`smtp.rs`). It sends nothing by
//! itself, never deletes and never runs what a message says.
//!
//! It is a crate of its own so it can be taken away: nothing else in Echo depends on it but one file of the core
//! (`crates/core/src/surfaces/mail.rs`) and the lines that name it (README).

mod account;
pub mod avatar;
mod base;
mod cache;
mod clean;
pub mod http_send;
pub mod imap;
pub mod inspect;
pub mod mime;
pub mod model;
mod remote;
mod sanitize;
pub mod search;
mod secrets;
mod service;
mod smtp;
#[cfg(test)]
mod testkit;
pub mod triage;
mod watch;

pub use account::{Account, MAX_ACCOUNTS, valid_address};
pub use model::{
    AccountView, Action, Attachment, BoxTotal, Folder, Message, Opened, Overview, Triage,
};
pub use search::Found;
pub use service::{MAX_RECIPIENTS, Mail, Outgoing, Person, Recipients, parse_people, view_of};
pub use watch::{Heard, Interesting, Listener};
