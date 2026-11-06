//! The mail as the core uses it: the accounts' messages read together, what the owner does to one, a reply saved as
//! a draft. It keeps nothing on disk: the list of accounts and the senders the owner finds interesting are the
//! core's to save, the passwords are the system's (`secrets.rs`), and the messages are read from the servers each
//! time. In memory it only remembers which server message each id of the last reading is.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::account::Account;
use crate::cache::Cache;
use crate::imap::{Imap, Mailbox, Raw};
use crate::mime::{self, Parsed};
use crate::model::{AccountView, Action, BoxTotal, Folder, Message, Opened, Overview, Triage};
use crate::search::{Found, MAX_QUERY, Query, found_of};
use crate::{remote, sanitize, secrets, triage};

/// How many of the newest messages of the inbox, and of Sent and Drafts, are read.
const INBOX_COUNT: u32 = 40;
const SIDE_COUNT: u32 = 15;
/// How many more of the archive «Cargar más» brings.
const ARCHIVE_MORE: usize = 40;
/// What Gmail's «Todos» holds that no other mailbox does: what was archived.
const ARCHIVED_ONLY: &str = "-in:inbox -in:sent -in:drafts";
/// The longest reply that is saved, in characters.
const MAX_REPLY: usize = 10_000;
/// The longest subject a reply goes with, in characters.
const MAX_SUBJECT: usize = 200;
/// How many opened messages keep the pictures on the web they name (`Mail::image` fetches only those).
const MAX_OPENED: usize = 50;
/// The most messages one `prefetch` reads ahead.
pub const PREFETCH_MAX: usize = 6;
/// How much memory the messages opened lately and the pictures fetched lately may take (their bytes, no more).
const OPENED_BYTES: usize = 24 * 1024 * 1024;
const PICTURES_BYTES: usize = 32 * 1024 * 1024;

/// Which message of which server an id of the page is, and who a reply to it goes to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) account_id: String,
    pub(crate) from_name: String,
    pub(crate) from_address: String,
    /// Where the sender asks replies to go (`Reply-To`); empty when it does not say.
    pub(crate) reply_to: Vec<(String, String)>,
    /// Everyone else the message went to (`To`, `Cc`): the copies of a reply to all.
    pub(crate) others: Vec<(String, String)>,
    pub(crate) subject: String,
    pub(crate) message_id: String,
}

/// Who a reply goes to: `to` gets it, `cc` a copy.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipients {
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Person {
    pub name: String,
    pub address: String,
}

/// Who a reply to `entry` goes to, for the account `own`: whom the message asks replies to go to (its `Reply-To`),
/// else its sender, and, `all`, a copy to everyone else it went to. Never the account itself, never anyone twice. A
/// message the account sent itself is answered to the people it went to.
pub(crate) fn recipients_of(entry: &Entry, own: &str, all: bool) -> Recipients {
    let mut seen = HashSet::from([own.trim().to_lowercase()]);
    let mut take = |people: &[(String, String)]| -> Vec<Person> {
        people
            .iter()
            .filter(|(_, address)| {
                address.contains('@') && seen.insert(address.trim().to_lowercase())
            })
            .map(|(name, address)| Person {
                name: name.clone(),
                address: address.clone(),
            })
            .collect()
    };
    let from = [(entry.from_name.clone(), entry.from_address.clone())];
    let to = take(if entry.reply_to.is_empty() {
        &from
    } else {
        &entry.reply_to
    });
    if to.is_empty() {
        // The account wrote it: the reply goes on to the people it was for.
        return Recipients {
            to: take(&entry.others),
            cc: Vec::new(),
        };
    }
    let cc = if all { take(&entry.others) } else { Vec::new() };
    Recipients { to, cc }
}

/// Opens the connection to an account's server, before logging in.
pub(crate) type Opener = Box<dyn Fn(&Account) -> Result<Imap, String> + Send + Sync>;

pub struct Mail {
    entries: Mutex<HashMap<String, Entry>>,
    /// The pictures on the web of each message opened lately: the only addresses `image` fetches.
    named: Mutex<HashMap<String, HashSet<String>>>,
    /// The last reading of the accounts, which the page shows at once when it comes up again.
    last: Mutex<Option<Overview>>,
    /// The messages read whole lately: opened again, they come from here, with no connection to their server.
    opened: Mutex<Cache<Opened>>,
    /// The pictures on the web fetched lately, by address.
    pictures: Mutex<Cache<String>>,
    /// The special mailboxes of each account, found once: the next reading goes straight to them.
    boxes: Mutex<HashMap<String, Boxes>>,
    /// The accounts whose arrivals are watched (`watch.rs`), each with the flag that stops its watch.
    pub(crate) watchers: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// One `prefetch` at a time: the page may ask again before the last one ended, and two would only fight for the server.
    warming: Mutex<()>,
    pub(crate) open: Opener,
}

impl Default for Mail {
    fn default() -> Self {
        Self::new()
    }
}

/// What a reading brings of an account: its messages, and how many each mailbox holds.
type AccountReading = (Vec<(Message, Entry)>, Vec<(Folder, u32)>);

/// The mailboxes of an account that Echo writes to or reads besides the inbox.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Boxes {
    pub sent: Option<String>,
    pub drafts: Option<String>,
    pub archive: Option<String>,
    /// The archive is the account's whole mail (Gmail's «Todos»), which also holds what the other mailboxes hold.
    pub archive_is_all: bool,
}

pub fn boxes_of(list: &[Mailbox]) -> Boxes {
    let flagged = |flag: &str| {
        list.iter()
            .find(|m| m.flags.iter().any(|f| f.eq_ignore_ascii_case(flag)))
            .map(|m| m.name.clone())
    };
    let named = |names: &[&str]| {
        list.iter()
            .find(|m| names.iter().any(|n| m.name.eq_ignore_ascii_case(n)))
            .map(|m| m.name.clone())
    };
    Boxes {
        sent: flagged("\\Sent").or_else(|| named(&["Sent", "Sent Messages", "Enviados"])),
        drafts: flagged("\\Drafts").or_else(|| named(&["Drafts", "Borradores"])),
        archive: flagged("\\Archive")
            .or_else(|| flagged("\\All"))
            .or_else(|| named(&["Archive", "Archives", "Archivo"])),
        archive_is_all: flagged("\\Archive").is_none() && flagged("\\All").is_some(),
    }
}

/// The UIDs of what the open archive holds, lowest first. `None` for an «all mail» box with no way to tell what no other
/// mailbox holds (only Gmail's can be asked): its newest would be the inbox again.
fn archived_uids(
    imap: &mut Imap,
    account: &Account,
    boxes: &Boxes,
) -> Result<Option<Vec<u64>>, String> {
    match (boxes.archive_is_all, account.is_gmail()) {
        (true, true) => imap.search("", Some(ARCHIVED_ONLY)).map(Some),
        (true, false) => Ok(None),
        (false, _) => imap.search("ALL", None).map(Some),
    }
}

pub fn id(account: &str, folder: Folder, uid: u64) -> String {
    format!("{account}.{}.{uid}", folder.as_str())
}

fn parse_id(id: &str) -> Option<(String, Folder, u64)> {
    let mut parts = id.splitn(3, '.');
    let (account, folder, uid) = (
        parts.next()?,
        Folder::parse(parts.next()?)?,
        parts.next()?.parse().ok()?,
    );
    (account.len() == 16 && account.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| (account.to_string(), folder, uid))
}

/// A message of the server as the page shows it, and what is kept to act on it.
pub(crate) fn message_from(
    account: &Account,
    folder: Folder,
    raw: &Raw,
    interesting: &HashSet<String>,
) -> (Message, Entry) {
    let parsed: Parsed = mime::parse(&raw.header, &raw.text);
    let (triage, reason) = if folder == Folder::Inbox {
        triage::classify(&parsed, interesting)
    } else {
        (Triage::Routine, None)
    };
    let outgoing = matches!(folder, Folder::Sent | Folder::Drafts);
    let (from, address) = if outgoing {
        (parsed.to_name.clone(), parsed.to_address.clone())
    } else {
        (parsed.from_name.clone(), parsed.from_address.clone())
    };
    let has = |flag: &str| raw.flags.iter().any(|f| f.eq_ignore_ascii_case(flag));
    let message = Message {
        id: id(&account.id, folder, raw.uid),
        account_id: account.id.clone(),
        folder,
        from: if from.is_empty() {
            "(sin remitente)".into()
        } else {
            from
        },
        address,
        subject: if parsed.subject.is_empty() {
            "(sin asunto)".into()
        } else {
            parsed.subject.clone()
        },
        preview: mime::one_line(&parsed.text, 140),
        body: mime::paragraphs(&parsed.text),
        at: raw.date.unwrap_or(0),
        unread: folder == Folder::Inbox && !has("\\Seen"),
        starred: has("\\Flagged"),
        triage,
        labels: raw.labels.clone(),
        reason,
        auth_failed: parsed.auth_failed,
    };
    let entry = Entry {
        account_id: account.id.clone(),
        from_name: parsed.from_name,
        from_address: parsed.from_address,
        reply_to: parsed.reply_to,
        others: parsed.others,
        subject: parsed.subject,
        message_id: parsed.message_id,
    };
    (message, entry)
}

pub(crate) fn connect(open: &Opener, account: &Account, password: &str) -> Result<Imap, String> {
    let mut imap = open(account)?;
    imap.login(&account.address, password, &account.provider)?;
    Ok(imap)
}

impl Mail {
    pub fn new() -> Self {
        Self::with_opener(Box::new(|account| Imap::open(&account.host, account.port)))
    }

    /// With another way to open a connection: the tests', over a socket on this machine.
    pub fn with_opener(open: Opener) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            named: Mutex::new(HashMap::new()),
            last: Mutex::new(None),
            opened: Mutex::new(Cache::new(OPENED_BYTES)),
            pictures: Mutex::new(Cache::new(PICTURES_BYTES)),
            boxes: Mutex::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
            warming: Mutex::new(()),
            open,
        }
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The message read whole, as the reader shows it: its HTML made safe, its text, and the pictures on the web it
    /// names. Nothing is marked as read; a message that moved has another id there (the next reading names it). A
    /// message opened before comes from memory: no connection to its server.
    pub fn open(&self, accounts: &[Account], id: &str) -> Result<Opened, String> {
        let (account_id, folder, uid) = parse_id(id).ok_or("No reconozco ese correo.")?;
        let account = accounts
            .iter()
            .find(|a| a.id == account_id)
            .ok_or("Esa cuenta ya no está conectada.")?;
        let cached = self
            .opened
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id);
        if let Some(opened) = cached {
            self.remember_named(id, &opened.remote);
            return Ok(opened);
        }
        let mut imap = self.session(account, folder)?;
        let raw = imap.fetch_message(uid)?;
        imap.logout();
        Ok(self.finish(id, &raw))
    }

    /// One file of a message, by its place among the message's attachments (`Opened::attachments`): its name and its bytes.
    /// It is read from the server when asked (never kept in memory) and nothing is marked as read.
    pub fn attachment(
        &self,
        accounts: &[Account],
        id: &str,
        index: usize,
    ) -> Result<(String, Vec<u8>), String> {
        let (account_id, folder, uid) = parse_id(id).ok_or("No reconozco ese correo.")?;
        let account = accounts
            .iter()
            .find(|a| a.id == account_id)
            .ok_or("Esa cuenta ya no está conectada.")?;
        let mut imap = self.session(account, folder)?;
        let raw = imap.fetch_message(uid)?;
        imap.logout();
        let (info, bytes) =
            mime::attachment_at(&raw, index).ok_or("Ese correo ya no trae ese archivo.")?;
        // What the look at its bytes blocked is never handed over to be opened, whatever the page asks.
        if info.risk.level == crate::inspect::Level::Danger {
            return Err(format!(
                "MonoBOT won't open this file. {}",
                info.risk.reasons.join(" ")
            ));
        }
        Ok((info.name, bytes))
    }

    /// A connection to the account's server with the folder of the message selected (read-only).
    fn session(&self, account: &Account, folder: Folder) -> Result<Imap, String> {
        let password = secrets::get(&account.address)?;
        let mut imap = connect(&self.open, account, &password)?;
        let mailbox = match folder {
            Folder::Inbox => "INBOX".to_string(),
            Folder::Sent => boxes_of(&imap.mailboxes()?)
                .sent
                .ok_or("Esta cuenta no tiene carpeta de enviados.")?,
            Folder::Drafts => boxes_of(&imap.mailboxes()?)
                .drafts
                .ok_or("Esta cuenta no tiene carpeta de borradores.")?,
            Folder::Archive => boxes_of(&imap.mailboxes()?)
                .archive
                .ok_or("Esta cuenta no tiene carpeta de archivo.")?,
        };
        imap.select(&mailbox, false)?;
        Ok(imap)
    }

    /// What was fetched of a message, made safe and kept in memory for the next time it opens.
    fn finish(&self, id: &str, raw: &[u8]) -> Opened {
        let whole = mime::parse_whole(raw);
        let cleaned = sanitize::clean(&whole.html, whole.pictures);
        self.remember_named(id, &cleaned.remote);
        let opened = Opened {
            id: id.to_string(),
            html: cleaned.html,
            text: mime::paragraphs(&whole.text),
            remote: cleaned.remote,
            images_allowed: false,
            attachments: whole.attachments,
        };
        let previews: usize = opened
            .attachments
            .iter()
            .filter_map(|a| a.preview.as_ref())
            .map(String::len)
            .sum();
        let size =
            opened.html.len() + opened.text.iter().map(String::len).sum::<usize>() + previews;
        self.opened.lock().unwrap_or_else(|p| p.into_inner()).put(
            id.to_string(),
            opened.clone(),
            size,
        );
        opened
    }

    /// Reads ahead the messages the owner is likely to open next (the ones under the pointer, the next in the list), so
    /// that opening them comes from memory. One connection per account and folder; whatever fails is left for `open`.
    /// Nothing is marked as read. At most `PREFETCH_MAX` messages, and one call at a time.
    pub fn prefetch(&self, accounts: &[Account], ids: &[String]) {
        let _one = self.warming.lock().unwrap_or_else(|p| p.into_inner());
        let mut groups: Vec<((String, Folder), Vec<(String, u64)>)> = Vec::new();
        for id in ids.iter().take(PREFETCH_MAX) {
            let Some((account_id, folder, uid)) = parse_id(id) else {
                continue;
            };
            if self.is_warm(id) {
                continue;
            }
            match groups
                .iter_mut()
                .find(|(key, _)| *key == (account_id.clone(), folder))
            {
                Some((_, list)) => list.push((id.clone(), uid)),
                None => groups.push(((account_id, folder), vec![(id.clone(), uid)])),
            }
        }
        for ((account_id, folder), list) in groups {
            let Some(account) = accounts.iter().find(|a| a.id == account_id) else {
                continue;
            };
            let Ok(mut imap) = self.session(account, folder) else {
                continue;
            };
            for (id, uid) in list {
                match imap.fetch_message(uid) {
                    Ok(raw) => {
                        self.finish(&id, &raw);
                    }
                    Err(_) => break,
                }
            }
            imap.logout();
        }
    }

    /// Whether the message already opens from memory (`prefetch` or an earlier `open` kept it).
    pub fn is_warm(&self, id: &str) -> bool {
        self.opened
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .is_some()
    }

    /// The last reading of the accounts, as it was: no connection. `None` before the first reading.
    pub fn last(&self) -> Option<Overview> {
        self.last.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// One picture on the web that the message `id` (opened lately) names, as a data URL, for the owner who allowed it.
    /// A picture fetched lately comes from memory.
    pub fn image(&self, id: &str, url: &str) -> Result<String, String> {
        let named = self
            .named
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .is_some_and(|urls| urls.contains(url));
        if !named {
            return Err("Abre el correo de nuevo para ver sus imágenes.".into());
        }
        let cached = self
            .pictures
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(url);
        if let Some(data) = cached {
            return Ok(data);
        }
        let data = remote::fetch(url)?;
        let size = data.len();
        self.pictures.lock().unwrap_or_else(|p| p.into_inner()).put(
            url.to_string(),
            data.clone(),
            size,
        );
        Ok(data)
    }

    fn remember_named(&self, id: &str, remote: &[String]) {
        let mut named = self.named.lock().unwrap_or_else(|p| p.into_inner());
        if named.len() >= MAX_OPENED {
            named.clear();
        }
        named.insert(id.to_string(), remote.iter().cloned().collect());
    }

    /// Checks that the server takes the password and keeps it in the system's store. Nothing is kept if it does not.
    pub fn add_account(&self, account: &Account, password: &str) -> Result<(), String> {
        // Google shows the app password in groups of four with spaces: they are not part of it.
        let password: String = if account.is_gmail() {
            password.chars().filter(|c| !c.is_whitespace()).collect()
        } else {
            password.trim().to_string()
        };
        let password = password.as_str();
        if password.is_empty() {
            return Err("Escribe la contraseña de aplicación de la cuenta.".into());
        }
        let mut imap = connect(&self.open, account, password)?;
        imap.select("INBOX", false)?;
        imap.logout();
        secrets::save(&account.address, password)
    }

    /// The account's password is forgotten and so are its messages.
    pub fn remove_account(&self, account: &Account) {
        self.unwatch(&account.id);
        self.boxes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&account.id);
        secrets::forget(&account.address);
        self.entries()
            .retain(|_, entry| entry.account_id != account.id);
        let prefix = format!("{}.", account.id);
        self.opened
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|key| !key.starts_with(&prefix));
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.pictures
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// The newest messages of every account, newest first. An account that cannot be read says why in `problems`
    /// and the others are still shown.
    pub fn overview(&self, accounts: &[Account], interesting: &HashSet<String>) -> Overview {
        let results: Vec<(&Account, Result<AccountReading, String>)> =
            std::thread::scope(|scope| {
                let handles: Vec<_> = accounts
                    .iter()
                    .map(|account| {
                        (
                            account,
                            scope.spawn(move || self.read_account(account, interesting)),
                        )
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|(account, handle)| {
                        (
                            account,
                            handle
                                .join()
                                .unwrap_or_else(|_| Err("No se pudo leer esta cuenta.".into())),
                        )
                    })
                    .collect()
            });
        let mut messages = Vec::new();
        let mut problems = Vec::new();
        let mut totals = Vec::new();
        let mut entries = self.entries();
        for (account, result) in results {
            match result {
                Ok((read, counted)) => {
                    entries.retain(|_, entry| entry.account_id != account.id);
                    for (message, entry) in read {
                        entries.insert(message.id.clone(), entry);
                        messages.push(message);
                    }
                    totals.extend(counted.into_iter().map(|(folder, total)| BoxTotal {
                        account_id: account.id.clone(),
                        folder,
                        total,
                    }));
                }
                Err(why) => problems.push(format!("{}: {why}", account.name)),
            }
        }
        messages.sort_by(|a, b| b.at.cmp(&a.at));
        let overview = Overview {
            accounts: accounts.iter().map(view_of).collect(),
            messages,
            problems,
            totals,
        };
        *self.last.lock().unwrap_or_else(|p| p.into_inner()) = Some(overview.clone());
        overview
    }

    fn read_account(
        &self,
        account: &Account,
        interesting: &HashSet<String>,
    ) -> Result<AccountReading, String> {
        // Durations only, never a name or a subject: where a reading's time goes (`correo: …` in echo.log).
        let began = Instant::now();
        let mut lap = Instant::now();
        let mut took = || {
            let ms = lap.elapsed().as_millis();
            lap = Instant::now();
            ms
        };
        let password = secrets::get(&account.address)?;
        let mut imap = (self.open)(account)?;
        let opened = took();
        imap.login(&account.address, &password, &account.provider)?;
        let login = took();
        let mut out = Vec::new();
        let mut folders = String::new();
        let mut counted = Vec::new();
        // The inbox first: it is what the page shows; the mailboxes it only names (sent, drafts) come after it.
        let mut boxes = self
            .boxes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&account.id)
            .cloned();
        for (folder, count) in [
            (Folder::Inbox, INBOX_COUNT),
            (Folder::Sent, SIDE_COUNT),
            (Folder::Drafts, SIDE_COUNT),
        ] {
            let mailbox = match folder {
                Folder::Inbox => Some("INBOX".to_string()),
                _ => {
                    if boxes.is_none() {
                        let found = boxes_of(&imap.mailboxes().unwrap_or_default());
                        self.boxes
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .insert(account.id.clone(), found.clone());
                        folders.push_str(&format!(", carpetas {} ms", took()));
                        boxes = Some(found);
                    }
                    let known = boxes.as_ref();
                    if folder == Folder::Sent {
                        known.and_then(|b| b.sent.clone())
                    } else {
                        known.and_then(|b| b.drafts.clone())
                    }
                }
            };
            let Some(mailbox) = mailbox else { continue };
            // A side mailbox that cannot be read does not take the inbox with it.
            let read = imap.select(&mailbox, false).and_then(|exists| {
                counted.push((folder, exists));
                imap.fetch_recent(exists, count, account.is_gmail())
            });
            let fetched = took();
            match read {
                Ok(raws) => {
                    out.extend(
                        raws.iter()
                            .map(|raw| message_from(account, folder, raw, interesting)),
                    );
                    folders.push_str(&format!(
                        ", {} {fetched} ms + proceso {} ms ({})",
                        folder.as_str(),
                        took(),
                        raws.len()
                    ));
                }
                Err(why) if folder == Folder::Inbox => return Err(why),
                Err(_) => {}
            }
        }
        // The archive: how many it holds and its newest («Cargar más» brings the rest, `archive_more`). An «all mail» box
        // that cannot be told apart is not listed, and what the other mailboxes hold is taken out of its count (an
        // estimate: a message in two of them is taken out twice).
        if let Some(known) = boxes.as_ref().filter(|b| b.archive.is_some()) {
            if let Ok(exists) = imap.select(known.archive.as_deref().unwrap_or_default(), false) {
                match archived_uids(&mut imap, account, known) {
                    Ok(Some(uids)) => {
                        counted.push((Folder::Archive, uids.len() as u32));
                        let newest: Vec<u64> = uids
                            .iter()
                            .rev()
                            .take(SIDE_COUNT as usize)
                            .copied()
                            .collect();
                        if let Ok(raws) = imap.fetch_listed(&newest, account.is_gmail()) {
                            out.extend(raws.iter().map(|raw| {
                                message_from(account, Folder::Archive, raw, interesting)
                            }));
                        }
                    }
                    _ => {
                        let elsewhere: u32 = if known.archive_is_all {
                            counted.iter().map(|(_, n)| *n).sum()
                        } else {
                            0
                        };
                        counted.push((Folder::Archive, exists.saturating_sub(elsewhere)));
                    }
                }
            }
            folders.push_str(&format!(", archivo {} ms", took()));
        }
        imap.logout();
        crate::base::log::line(format!(
            "correo: {} · conexión {opened} ms, acceso {login} ms{folders}, total {} ms",
            account.provider,
            began.elapsed().as_millis()
        ));
        Ok((out, counted))
    }

    /// The next stretch of the archive of each account whose list has fewer than it holds, older than what it has: they
    /// join the last reading, which is returned. Read only. An account with all of it on the list is not connected to.
    pub fn archive_more(
        &self,
        accounts: &[Account],
        interesting: &HashSet<String>,
    ) -> Result<Overview, String> {
        let last = self
            .last()
            .ok_or("Actualiza la bandeja y vuelve a intentarlo.")?;
        let mut read = Vec::new();
        for account in accounts {
            let listed: Vec<u64> = last
                .messages
                .iter()
                .filter_map(|m| parse_id(&m.id))
                .filter(|(owner, folder, _)| *owner == account.id && *folder == Folder::Archive)
                .map(|(_, _, uid)| uid)
                .collect();
            let total = last
                .totals
                .iter()
                .find(|t| t.account_id == account.id && t.folder == Folder::Archive)
                .map_or(0, |t| t.total as usize);
            if listed.len() >= total {
                continue;
            }
            let password = secrets::get(&account.address)?;
            let mut imap = connect(&self.open, account, &password)?;
            let boxes = boxes_of(&imap.mailboxes()?);
            imap.select(
                boxes
                    .archive
                    .as_deref()
                    .ok_or("Esta cuenta no tiene carpeta de archivo.")?,
                false,
            )?;
            let uids = archived_uids(&mut imap, account, &boxes)?.unwrap_or_default();
            let oldest = listed.iter().min().copied().unwrap_or(u64::MAX);
            let next: Vec<u64> = uids
                .iter()
                .rev()
                .filter(|uid| **uid < oldest)
                .take(ARCHIVE_MORE)
                .copied()
                .collect();
            let raws = imap.fetch_listed(&next, account.is_gmail())?;
            imap.logout();
            read.extend(
                raws.iter()
                    .map(|raw| message_from(account, Folder::Archive, raw, interesting)),
            );
        }
        let mut entries = self.entries();
        let mut kept = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let overview = kept
            .as_mut()
            .ok_or("Actualiza la bandeja y vuelve a intentarlo.")?;
        for (message, entry) in read {
            entries.insert(message.id.clone(), entry);
            overview.messages.retain(|m| m.id != message.id);
            overview.messages.push(message);
        }
        overview.messages.sort_by(|a, b| b.at.cmp(&a.at));
        Ok(overview.clone())
    }

    /// What was kept of an earlier reading (the core's snapshot) as the last one, so the page shows it at once. It
    /// also teaches the ids which sender and subject they are, for what the owner does to a message before the next
    /// reading. A reading already made is not replaced by it.
    pub fn restore(&self, overview: Overview) {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        if last.is_some() {
            return;
        }
        let mut entries = self.entries();
        for message in &overview.messages {
            entries.entry(message.id.clone()).or_insert_with(|| Entry {
                account_id: message.account_id.clone(),
                from_name: message.from.clone(),
                from_address: message.address.clone(),
                subject: message.subject.clone(),
                ..Entry::default()
            });
        }
        *last = Some(overview);
    }

    /// New messages of an account's inbox, told by its watch: they join the last reading, newest first, so the page
    /// has them without reading the account again.
    pub(crate) fn arrived(&self, pairs: Vec<(Message, Entry)>) {
        let mut entries = self.entries();
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        for (message, entry) in pairs {
            entries.insert(message.id.clone(), entry);
            if let Some(overview) = last.as_mut() {
                if !overview.messages.iter().any(|m| m.id == message.id) {
                    overview.recount(&message.account_id, message.folder, 1);
                }
                overview.messages.retain(|m| m.id != message.id);
                overview.messages.push(message);
            }
        }
        if let Some(overview) = last.as_mut() {
            overview.messages.sort_by(|a, b| b.at.cmp(&a.at));
        }
    }

    /// Who a reply to a message of the last reading goes to: only to whom it asks, or `all`.
    pub fn recipients(
        &self,
        accounts: &[Account],
        id: &str,
        all: bool,
    ) -> Result<Recipients, String> {
        let entry = self
            .entries()
            .get(id)
            .cloned()
            .ok_or("Actualiza la bandeja y vuelve a intentarlo.")?;
        let account = accounts
            .iter()
            .find(|a| a.id == entry.account_id)
            .ok_or("Esa cuenta ya no está conectada.")?;
        Ok(recipients_of(&entry, &account.address, all))
    }

    /// The address a message of the last reading came from.
    pub fn sender_of(&self, id: &str) -> Option<String> {
        self.entries()
            .get(id)
            .map(|entry| entry.from_address.clone())
            .filter(|a| !a.is_empty())
    }

    /// Reads, stars or archives a message on its server. Nothing is deleted.
    pub fn act(&self, accounts: &[Account], id: &str, action: Action) -> Result<(), String> {
        let (account_id, folder, uid) = parse_id(id).ok_or("No reconozco ese correo.")?;
        let account = accounts
            .iter()
            .find(|a| a.id == account_id)
            .ok_or("Esa cuenta ya no está conectada.")?;
        let password = secrets::get(&account.address)?;
        let mut imap = connect(&self.open, account, &password)?;
        let boxes = if folder != Folder::Inbox || action == Action::Archive {
            boxes_of(&imap.mailboxes()?)
        } else {
            Boxes::default()
        };
        let mailbox = match folder {
            Folder::Inbox => "INBOX".to_string(),
            Folder::Sent => boxes
                .sent
                .clone()
                .ok_or("Esta cuenta no tiene carpeta de enviados.")?,
            Folder::Drafts => boxes
                .drafts
                .clone()
                .ok_or("Esta cuenta no tiene carpeta de borradores.")?,
            Folder::Archive if action == Action::Archive => {
                return Err("Ese correo ya está archivado.".into());
            }
            Folder::Archive => boxes
                .archive
                .clone()
                .ok_or("Esta cuenta no tiene carpeta de archivo.")?,
        };
        imap.select(&mailbox, true)?;
        match action {
            Action::Read => imap.flag(uid, "\\Seen", true)?,
            Action::Star => imap.flag(uid, "\\Flagged", true)?,
            Action::Unstar => imap.flag(uid, "\\Flagged", false)?,
            Action::Archive => {
                let to = boxes
                    .archive
                    .ok_or("Esta cuenta no tiene carpeta de archivo.")?;
                imap.move_to(uid, &to)?;
                self.entries().remove(id);
            }
        }
        imap.logout();
        self.touch(id, action);
        Ok(())
    }

    /// The copy of the last reading follows what the owner did, so the page does not show the old state at its return.
    fn touch(&self, id: &str, action: Action) {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let Some(overview) = last.as_mut() else {
            return;
        };
        let Some(message) = overview.messages.iter_mut().find(|m| m.id == id) else {
            return;
        };
        match action {
            Action::Read => message.unread = false,
            Action::Star => message.starred = true,
            Action::Unstar => message.starred = false,
            Action::Archive => {
                let (account, from) = (message.account_id.clone(), message.folder);
                message.folder = Folder::Archive;
                message.unread = false;
                overview.recount(&account, from, -1);
                overview.recount(&account, Folder::Archive, 1);
            }
        }
    }

    /// Saves `text` as a draft of a reply to a message of the last reading.
    /// `subject` is the one the user chose for the reply; without it, «Re: » and the message's.
    pub fn draft(
        &self,
        accounts: &[Account],
        id: &str,
        text: &str,
        subject: Option<&str>,
        all: bool,
        now_ms: i64,
    ) -> Result<(), String> {
        let (account, entry, text) = self.reply_parts(accounts, id, text)?;
        let people = recipients_of(&entry, &account.address, all);
        let password = secrets::get(&account.address)?;
        let mut imap = connect(&self.open, &account, &password)?;
        let drafts = boxes_of(&imap.mailboxes()?)
            .drafts
            .ok_or("Esta cuenta no tiene carpeta de borradores.")?;
        imap.save_draft(
            &drafts,
            &compose_reply(&account, &entry, &people, subject, &text, now_ms),
        )?;
        imap.logout();
        Ok(())
    }

    /// Sends `text` as the reply to a message of the last reading, to the people `recipients_of` names and no one else.
    /// Only the owner's press gets here. Gmail keeps its own copy of what it sends; any other account gets one in its sent folder, and a failure there does
    /// not undo what already left.
    pub fn send(
        &self,
        accounts: &[Account],
        id: &str,
        text: &str,
        subject: Option<&str>,
        all: bool,
        now_ms: i64,
    ) -> Result<(), String> {
        let (account, entry, text) = self.reply_parts(accounts, id, text)?;
        let people = recipients_of(&entry, &account.address, all);
        let addresses: Vec<String> = people
            .to
            .iter()
            .chain(&people.cc)
            .map(|p| p.address.clone())
            .collect();
        let password = secrets::get(&account.address)?;
        let message = compose_reply(&account, &entry, &people, subject, &text, now_ms);
        crate::smtp::send(&account, &password, &addresses, &message)?;
        if !account.is_gmail() {
            if let Ok(mut imap) = connect(&self.open, &account, &password) {
                if let Some(sent) = imap.mailboxes().ok().and_then(|list| boxes_of(&list).sent) {
                    let _ = imap.save_sent(&sent, &message);
                }
                imap.logout();
            }
        }
        Ok(())
    }

    /// The account and the message a reply goes with, and its text, checked.
    fn reply_parts(
        &self,
        accounts: &[Account],
        id: &str,
        text: &str,
    ) -> Result<(Account, Entry, String), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Escribe la respuesta.".into());
        }
        if text.chars().count() > MAX_REPLY {
            return Err("La respuesta es demasiado larga.".into());
        }
        let entry = self
            .entries()
            .get(id)
            .cloned()
            .ok_or("Actualiza la bandeja y vuelve a intentarlo.")?;
        let account = accounts
            .iter()
            .find(|a| a.id == entry.account_id)
            .ok_or("Esa cuenta ya no está conectada.")?
            .clone();
        Ok((account, entry, text.to_string()))
    }

    /// The messages of one account that `query` finds (the words of Gmail's search box, `search.rs`), at most `limit`:
    /// the oldest of them when `oldest_first` (to read a stretch of mail in order), else the newest. Also whether more
    /// matched. Gmail is searched over «Todos», so a filter that skips the inbox does not hide a message; any other
    /// server, its inbox. Read only: `EXAMINE` and `BODY.PEEK`, nothing is marked, moved or kept.
    pub fn search(
        &self,
        account: &Account,
        query: &str,
        limit: usize,
        oldest_first: bool,
    ) -> Result<(Vec<Found>, bool), String> {
        let query: String = query
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_QUERY)
            .collect();
        let asked = Query::parse(&query);
        let password = secrets::get(&account.address)?;
        let mut imap = connect(&self.open, account, &password)?;
        let gmail = account.is_gmail();
        let mailbox = if gmail {
            imap.mailboxes()?
                .into_iter()
                .find(|m| m.flags.iter().any(|f| f.eq_ignore_ascii_case("\\All")))
                .map_or_else(|| "INBOX".to_string(), |m| m.name)
        } else {
            "INBOX".to_string()
        };
        imap.select(&mailbox, false)?;
        let uids = imap.search(
            &asked.criteria(),
            gmail.then_some(query.trim()).filter(|q| !q.is_empty()),
        )?;
        let more = uids.len() > limit;
        let chosen: Vec<u64> = if oldest_first {
            uids.iter().take(limit).copied().collect()
        } else {
            uids.iter().rev().take(limit).copied().collect()
        };
        let raws = imap.fetch_uids(&chosen, gmail)?;
        imap.logout();
        let mut found: Vec<Found> = raws
            .iter()
            .map(found_of)
            .filter(|f| gmail || asked.holds(f))
            .collect();
        found.sort_by_key(|f| f.uid);
        if !oldest_first {
            found.reverse();
        }
        Ok((found, more))
    }
}

pub fn view_of(account: &Account) -> AccountView {
    AccountView {
        id: account.id.clone(),
        name: account.name.clone(),
        address: account.address.clone(),
        provider: account.provider.clone(),
    }
}

/// A header value with nothing that could end the line or start another header.
fn safe(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// People as a header names them, «Ana Pérez <ana@x.test>, luis@x.test»: each name encoded, nothing that ends the line.
fn header_people(people: &[Person]) -> String {
    people
        .iter()
        .map(|p| {
            let address = safe(&p.address);
            let name = safe(&p.name);
            if name.is_empty() || name.eq_ignore_ascii_case(&address) {
                address
            } else {
                format!("{} <{address}>", mime::encode_word(&name.replace('"', "")))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The subject a reply goes with: the one the user chose, on one line and cut to `MAX_SUBJECT`; else «Re: » and the
/// message's. It still answers their message (`In-Reply-To`) whatever it says.
fn reply_subject(entry: &Entry, chosen: Option<&str>) -> String {
    let chosen: String = chosen
        .map(safe)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_SUBJECT)
        .collect();
    if !chosen.is_empty() {
        return chosen;
    }
    let subject = safe(&entry.subject);
    if subject.to_ascii_lowercase().starts_with("re:") {
        subject
    } else {
        format!("Re: {subject}")
    }
}

/// The reply as a message: to `people`, in reply to theirs, its text in base64 so no server can alter it.
fn compose_reply(
    account: &Account,
    entry: &Entry,
    people: &Recipients,
    subject: Option<&str>,
    text: &str,
    now_ms: i64,
) -> String {
    use base64::Engine;
    let subject = reply_subject(entry, subject);
    let mut message = format!(
        "From: {} <{}>\r\nTo: {}\r\n",
        mime::encode_word(&safe(&account.name)),
        account.address,
        header_people(&people.to)
    );
    if !people.cc.is_empty() {
        message.push_str(&format!("Cc: {}\r\n", header_people(&people.cc)));
    }
    message.push_str(&format!(
        "Subject: {}\r\nDate: {}\r\n",
        mime::encode_word(&subject),
        rfc2822(now_ms)
    ));
    if !entry.message_id.is_empty() {
        message.push_str(&format!(
            "In-Reply-To: {0}\r\nReferences: {0}\r\n",
            safe(&entry.message_id)
        ));
    }
    message.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    let body = base64::engine::general_purpose::STANDARD.encode(text);
    for chunk in body.as_bytes().chunks(76) {
        message.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        message.push_str("\r\n");
    }
    message
}

/// «Sat, 24 Oct 2026 12:00:00 +0000».
fn rfc2822(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let weekday =
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][(days + 4).rem_euclid(7) as usize];
    let month_name = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(month - 1) as usize];
    format!(
        "{weekday}, {day:02} {month_name} {year} {:02}:{:02}:{:02} +0000",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// The civil date of a day count since 1970-01-01 (Howard Hinnant's algorithm).
pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imap::days_from_civil;
    use crate::testkit::{fetch_answer, server};
    use std::net::TcpStream;
    use std::time::Duration;

    fn account() -> Account {
        account_of("ana@ejemplo.test")
    }

    /// Each test that keeps a password has an address of its own: the tests run side by side.
    fn account_of(address: &str) -> Account {
        Account::new(address, "Ana", "imap", Some("imap.ejemplo.test")).unwrap()
    }

    /// A mail whose connections go to the fake server on this machine.
    fn mail_on(port: u16) -> Mail {
        Mail::with_opener(Box::new(move |_| {
            let tcp = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
            tcp.set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| e.to_string())?;
            Imap::over(Box::new(tcp))
        }))
    }

    #[test]
    fn the_civil_date_and_the_day_count_agree() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 10, 24), (1969, 12, 31)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
        assert_eq!(
            rfc2822(1_792_231_200_000),
            "Sat, 17 Oct 2026 10:00:00 +0000"
        );
    }

    #[test]
    fn an_id_says_its_account_folder_and_uid_and_nothing_else_is_accepted() {
        let a = account();
        assert_eq!(
            parse_id(&id(&a.id, Folder::Drafts, 12)),
            Some((a.id.clone(), Folder::Drafts, 12))
        );
        assert_eq!(parse_id("../../x.inbox.1"), None);
        assert_eq!(parse_id(&format!("{}.trash.1", a.id)), None);
        assert_eq!(parse_id(&format!("{}.inbox.x", a.id)), None);
    }

    #[test]
    fn it_finds_the_special_mailboxes_by_flag_and_by_name() {
        let list = |name: &str, flags: &[&str]| Mailbox {
            name: name.into(),
            flags: flags.iter().map(|f| f.to_string()).collect(),
        };
        let gmail = boxes_of(&[
            list("INBOX", &[]),
            list("[Gmail]/Enviados", &["\\Sent"]),
            list("[Gmail]/Borradores", &["\\Drafts"]),
            list("[Gmail]/Todos", &["\\All"]),
        ]);
        assert_eq!(
            (
                gmail.sent.as_deref(),
                gmail.drafts.as_deref(),
                gmail.archive.as_deref()
            ),
            (
                Some("[Gmail]/Enviados"),
                Some("[Gmail]/Borradores"),
                Some("[Gmail]/Todos")
            )
        );
        let plain = boxes_of(&[list("INBOX", &[]), list("Sent", &[]), list("Archive", &[])]);
        assert_eq!(
            (
                plain.sent.as_deref(),
                plain.drafts,
                plain.archive.as_deref()
            ),
            (Some("Sent"), None, Some("Archive"))
        );
    }

    #[test]
    fn a_message_is_held_when_its_sender_is_not_interesting_and_prioritised_when_it_is() {
        let a = account();
        let raw = Raw { uid: 9, flags: vec!["\\Flagged".into()], date: Some(5), header: b"From: Tienda <noreply@tienda.test>\r\nSubject: Tu pedido est\xc3\xa1 en camino\r\nList-Unsubscribe: <mailto:x@y.test>\r\n".to_vec(), text: "Seguimiento".as_bytes().to_vec(), ..Raw::default() };
        let (held, entry) = message_from(&a, Folder::Inbox, &raw, &HashSet::new());
        assert_eq!(
            (held.triage, held.unread, held.starred, held.at),
            (Triage::Held, true, true, 5)
        );
        assert!(held.reason.as_deref().unwrap().contains("envío"));
        assert_eq!(entry.from_address, "noreply@tienda.test");
        let mut interesting = HashSet::new();
        interesting.insert("noreply@tienda.test".to_string());
        assert_eq!(
            message_from(&a, Folder::Inbox, &raw, &interesting).0.triage,
            Triage::Priority
        );
        assert_eq!(
            message_from(&a, Folder::Sent, &raw, &HashSet::new())
                .0
                .triage,
            Triage::Routine
        );
    }

    #[test]
    fn a_reply_goes_where_the_message_asks_and_to_all_only_when_asked() {
        let person = |name: &str, address: &str| (name.to_string(), address.to_string());
        let entry = Entry {
            from_name: "Tienda".into(),
            from_address: "noreply@tienda.test".into(),
            reply_to: vec![person("Soporte", "ayuda@tienda.test")],
            others: vec![
                person("Ana", "Ana@Ejemplo.test"),
                person("Luis", "luis@ejemplo.test"),
                person("Soporte", "ayuda@tienda.test"),
            ],
            ..Entry::default()
        };
        let addresses =
            |people: &[Person]| people.iter().map(|p| p.address.clone()).collect::<Vec<_>>();
        let one = recipients_of(&entry, "ana@ejemplo.test", false);
        assert_eq!(
            (addresses(&one.to), addresses(&one.cc)),
            (vec!["ayuda@tienda.test".to_string()], vec![])
        );
        let all = recipients_of(&entry, "ana@ejemplo.test", true);
        assert_eq!(
            addresses(&all.cc),
            ["luis@ejemplo.test"],
            "never the account, never anyone twice"
        );
        let plain = Entry {
            reply_to: vec![],
            ..entry.clone()
        };
        assert_eq!(
            addresses(&recipients_of(&plain, "ana@ejemplo.test", false).to),
            ["noreply@tienda.test"]
        );
        let mine = Entry {
            from_address: "ana@ejemplo.test".into(),
            reply_to: vec![],
            others: vec![person("Luis", "luis@ejemplo.test")],
            ..Entry::default()
        };
        assert_eq!(
            addresses(&recipients_of(&mine, "Ana@ejemplo.test", false).to),
            ["luis@ejemplo.test"]
        );
        let message = compose_reply(&account(), &entry, &all, None, "Hola", 1_792_231_200_000);
        assert!(
            message.contains("To: Soporte <ayuda@tienda.test>\r\nCc: Luis <luis@ejemplo.test>\r\n"),
            "{message}"
        );
    }

    #[test]
    fn a_reply_names_its_recipient_and_cannot_add_headers() {
        let entry = Entry {
            account_id: "a".into(),
            from_address: "lucia@ejemplo.test\r\nBcc: x@y.test".into(),
            subject: "Cena del sábado".into(),
            message_id: "<abc@ejemplo.test>".into(),
            ..Entry::default()
        };
        let people = recipients_of(&entry, "ana@ejemplo.test", false);
        let reply = compose_reply(
            &account(),
            &entry,
            &people,
            None,
            "Gracias, ¡nos vemos!",
            1_792_231_200_000,
        );
        assert!(
            reply.contains("To: lucia@ejemplo.testBcc: x@y.test\r\n"),
            "{reply}"
        );
        assert!(!reply.contains("\r\nBcc:"));
        assert!(reply.contains("Subject: =?UTF-8?B?"));
        assert!(reply.contains("In-Reply-To: <abc@ejemplo.test>\r\n"));
        assert!(reply.contains("Date: Sat, 17 Oct 2026 10:00:00 +0000\r\n"));
        assert!(
            reply.ends_with("\r\n") && !reply.contains("Gracias"),
            "the text goes in base64"
        );
    }

    #[test]
    fn a_reply_takes_the_subject_the_user_chose_on_one_line() {
        let entry = Entry {
            subject: "Cena del sábado".into(),
            ..Entry::default()
        };
        assert_eq!(reply_subject(&entry, None), "Re: Cena del sábado");
        assert_eq!(reply_subject(&entry, Some("   ")), "Re: Cena del sábado");
        assert_eq!(
            reply_subject(
                &Entry {
                    subject: "RE: hola".into(),
                    ..Entry::default()
                },
                None
            ),
            "RE: hola"
        );
        assert_eq!(
            reply_subject(&entry, Some(" Nos vemos el sábado\r\nBcc: x@y.test ")),
            "Nos vemos el sábadoBcc: x@y.test"
        );
        assert_eq!(
            reply_subject(&entry, Some(&"a".repeat(300)))
                .chars()
                .count(),
            MAX_SUBJECT
        );
        let reply = compose_reply(
            &account(),
            &entry,
            &recipients_of(&entry, "ana@ejemplo.test", false),
            Some("Nos vemos"),
            "Hola",
            0,
        );
        assert!(reply.contains("Subject: Nos vemos\r\n"), "{reply}");
    }

    #[test]
    fn the_overview_reads_the_inbox_holds_the_junk_and_remembers_what_each_id_is() {
        let a = account_of("lectura@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let header = "From: Lucía <lucia@ejemplo.test>\r\nSubject: Cena\r\n\r\n";
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("EXAMINE \"INBOX\"", b"* 1 EXISTS\r\n".to_vec()),
            (
                "FETCH 1:1",
                fetch_answer(1, 7, "", header, "¿Vienes el sábado?"),
            ),
            (
                "LIST",
                b"* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n".to_vec(),
            ),
            ("LOGOUT", vec![]),
        ]);
        let overview = mail_on(port).overview(std::slice::from_ref(&a), &HashSet::new());
        assert!(overview.problems.is_empty(), "{:?}", overview.problems);
        assert_eq!(overview.accounts.len(), 1);
        let message = &overview.messages[0];
        assert_eq!(
            (
                message.from.as_str(),
                message.subject.as_str(),
                message.unread,
                message.triage
            ),
            ("Lucía", "Cena", true, Triage::Routine)
        );
        assert_eq!(message.body, vec!["¿Vienes el sábado?"]);
        handle.join().unwrap();
        secrets::forget(&a.address);
    }

    #[test]
    fn a_reading_says_how_many_each_mailbox_holds_and_an_all_mail_box_it_cannot_tell_apart_is_an_estimate_with_no_list()
     {
        let a = account_of("conteo@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("EXAMINE \"INBOX\"", b"* 120 EXISTS\r\n".to_vec()),
            ("FETCH 81:120", fetch_answer(120, 7, "", "From: Ana <ana@ejemplo.test>\r\nSubject: Hola\r\n\r\n", "Hola")),
            ("LIST", b"* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\Sent) \"/\" \"[Gmail]/Enviados\"\r\n* LIST (\\Drafts) \"/\" \"[Gmail]/Borradores\"\r\n* LIST (\\All) \"/\" \"[Gmail]/Todos\"\r\n".to_vec()),
            ("EXAMINE \"[Gmail]/Enviados\"", b"* 30 EXISTS\r\n".to_vec()),
            ("FETCH 16:30", vec![]),
            ("EXAMINE \"[Gmail]/Borradores\"", b"* 0 EXISTS\r\n".to_vec()),
            ("EXAMINE \"[Gmail]/Todos\"", b"* 1000 EXISTS\r\n".to_vec()),
            ("LOGOUT", vec![]),
        ]);
        let mail = mail_on(port);
        let overview = mail.overview(std::slice::from_ref(&a), &HashSet::new());
        let asked = handle.join().unwrap();
        assert!(overview.problems.is_empty(), "{:?}", overview.problems);
        let total = |overview: &Overview, folder: Folder| {
            overview
                .totals
                .iter()
                .find(|t| t.account_id == a.id && t.folder == folder)
                .map(|t| t.total)
        };
        assert_eq!(
            total(&overview, Folder::Inbox),
            Some(120),
            "the server's count, not the forty read"
        );
        assert_eq!(total(&overview, Folder::Sent), Some(30));
        assert_eq!(total(&overview, Folder::Drafts), Some(0));
        assert_eq!(
            total(&overview, Folder::Archive),
            Some(850),
            "all mail, less what the other mailboxes hold"
        );
        assert!(
            !overview
                .messages
                .iter()
                .any(|m| m.folder == Folder::Archive),
            "it holds the inbox too: nothing to list"
        );
        assert!(
            asked
                .iter()
                .all(|line| !line.contains("STORE") && !line.contains("SELECT")),
            "counting is read only: {asked:?}"
        );
        // What the owner archives, and what arrives, move the count until the next reading.
        mail.touch(&id(&a.id, Folder::Inbox, 7), Action::Archive);
        let raw = Raw {
            uid: 8,
            date: Some(30),
            header: b"From: Ana <ana@ejemplo.test>\r\nSubject: Otra\r\n".to_vec(),
            text: b"Hola".to_vec(),
            ..Raw::default()
        };
        let pair = || message_from(&a, Folder::Inbox, &raw, &HashSet::new());
        mail.arrived(vec![pair()]);
        mail.arrived(vec![pair()]);
        let last = mail.last().unwrap();
        assert_eq!(
            (total(&last, Folder::Inbox), total(&last, Folder::Archive)),
            (Some(120), Some(851)),
            "one out and one in, heard twice"
        );
        secrets::forget(&a.address);
    }

    /// The answer of a `UID SEARCH`: these UIDs.
    fn found(uids: std::ops::RangeInclusive<u64>) -> Vec<u8> {
        format!(
            "* SEARCH {}\r\n",
            uids.map(|uid| uid.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes()
    }

    #[test]
    fn a_real_archive_mailbox_is_counted_whole_and_its_newest_are_listed() {
        let a = account_of("archivo-real@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let list =
            b"* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST () \"/\" \"Archive\"\r\n".to_vec();
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("EXAMINE \"INBOX\"", b"* 5 EXISTS\r\n".to_vec()),
            ("FETCH 1:5", vec![]),
            ("LIST", list),
            ("EXAMINE \"Archive\"", b"* 70 EXISTS\r\n".to_vec()),
            ("UID SEARCH ALL", found(1..=70)),
            (
                "UID FETCH 70,69,68,67,66,65,64,63,62,61,60,59,58,57,56 (UID FLAGS",
                fetch_answer(
                    70,
                    70,
                    "\\Seen \\Flagged",
                    "From: Ana <ana@ejemplo.test>\r\nSubject: Guardado\r\n\r\n",
                    "Hola",
                ),
            ),
            ("LOGOUT", vec![]),
        ]);
        let overview = mail_on(port).overview(std::slice::from_ref(&a), &HashSet::new());
        let asked = handle.join().unwrap();
        let of = |folder: Folder| {
            overview
                .totals
                .iter()
                .find(|t| t.folder == folder)
                .map(|t| t.total)
        };
        assert_eq!(
            (of(Folder::Inbox), of(Folder::Archive), of(Folder::Sent)),
            (Some(5), Some(70), None)
        );
        let archived = overview
            .messages
            .iter()
            .find(|m| m.folder == Folder::Archive)
            .unwrap();
        assert_eq!(
            (
                archived.id.clone(),
                archived.subject.as_str(),
                archived.starred,
                archived.unread
            ),
            (id(&a.id, Folder::Archive, 70), "Guardado", true, false)
        );
        assert!(
            asked
                .iter()
                .all(|line| !line.contains("STORE") && !line.contains("SELECT")),
            "listing is read only: {asked:?}"
        );
        secrets::forget(&a.address);
    }

    #[test]
    fn gmails_archive_is_what_no_other_mailbox_holds_and_its_count_is_exact() {
        let a = Account::new("archivo-gmail@ejemplo.test", "Ana", "gmail", None).unwrap();
        secrets::save(&a.address, "abcd efgh").unwrap();
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("EXAMINE \"INBOX\"", b"* 3 EXISTS\r\n".to_vec()),
            ("FETCH 1:3", vec![]),
            ("LIST", b"* LIST (\\Sent) \"/\" \"[Gmail]/Enviados\"\r\n* LIST (\\All) \"/\" \"[Gmail]/Todos\"\r\n".to_vec()),
            ("EXAMINE \"[Gmail]/Enviados\"", b"* 2 EXISTS\r\n".to_vec()),
            ("FETCH 1:2", vec![]),
            ("EXAMINE \"[Gmail]/Todos\"", b"* 1000 EXISTS\r\n".to_vec()),
            ("X-GM-RAW", b"+ go\r\n".to_vec()),
            ("-in:inbox -in:sent -in:drafts", found(7..=9)),
            ("UID FETCH 9,8,7 (UID FLAGS INTERNALDATE X-GM-LABELS", fetch_answer(900, 9, "\\Seen", "From: Ana <ana@ejemplo.test>\r\nSubject: Viejo\r\n\r\n", "Hola")),
            ("LOGOUT", vec![]),
        ]);
        let overview = mail_on(port).overview(std::slice::from_ref(&a), &HashSet::new());
        handle.join().unwrap();
        assert!(overview.problems.is_empty(), "{:?}", overview.problems);
        assert_eq!(
            overview
                .totals
                .iter()
                .find(|t| t.folder == Folder::Archive)
                .map(|t| t.total),
            Some(3),
            "what was found, not all the mail"
        );
        assert_eq!(
            overview
                .messages
                .iter()
                .filter(|m| m.folder == Folder::Archive)
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
            [id(&a.id, Folder::Archive, 9)]
        );
        secrets::forget(&a.address);
    }

    #[test]
    fn more_of_the_archive_comes_older_than_what_is_listed_and_only_while_there_is_more() {
        let a = account_of("archivo-mas@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let listed = |uid: u64| {
            let raw = Raw {
                uid,
                date: Some(uid as i64),
                header: b"From: Ana <ana@ejemplo.test>\r\nSubject: Viejo\r\n".to_vec(),
                text: b"Hola".to_vec(),
                ..Raw::default()
            };
            message_from(&a, Folder::Archive, &raw, &HashSet::new()).0
        };
        let total = BoxTotal {
            account_id: a.id.clone(),
            folder: Folder::Archive,
            total: 60,
        };
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("LIST", b"* LIST () \"/\" \"Archive\"\r\n".to_vec()),
            ("EXAMINE \"Archive\"", b"* 60 EXISTS\r\n".to_vec()),
            ("UID SEARCH ALL", found(1..=60)),
            (
                "UID FETCH 45,44,",
                fetch_answer(
                    45,
                    45,
                    "\\Seen",
                    "From: Ana <ana@ejemplo.test>\r\nSubject: Otro\r\n\r\n",
                    "Hola",
                ),
            ),
            ("LOGOUT", vec![]),
        ]);
        let mail = mail_on(port);
        mail.restore(Overview {
            accounts: vec![view_of(&a)],
            messages: (46..=60).map(listed).collect(),
            totals: vec![total.clone()],
            ..Overview::default()
        });
        let overview = mail
            .archive_more(std::slice::from_ref(&a), &HashSet::new())
            .unwrap();
        let asked = handle.join().unwrap();
        assert!(
            asked[4].contains("UID FETCH 45,44,") && asked[4].contains(",7,6 (UID FLAGS"),
            "the next forty, from the newest down: {}",
            asked[4]
        );
        assert_eq!(
            overview.messages.len(),
            16,
            "the fifteen it had, and what came"
        );
        assert_eq!(
            mail.sender_of(&id(&a.id, Folder::Archive, 45)).as_deref(),
            Some("ana@ejemplo.test"),
            "what came can be acted on"
        );
        // With everything listed it connects to nothing (the server above is gone).
        let whole = Mail::with_opener(Box::new(|_| Err("no debe conectar".to_string())));
        whole.restore(Overview {
            accounts: vec![view_of(&a)],
            messages: (1..=60).map(listed).collect(),
            totals: vec![total],
            ..Overview::default()
        });
        assert_eq!(
            whole
                .archive_more(std::slice::from_ref(&a), &HashSet::new())
                .unwrap()
                .messages
                .len(),
            60
        );
        secrets::forget(&a.address);
    }

    #[test]
    fn an_archived_message_opens_and_takes_a_star_but_is_not_archived_twice() {
        let a = account_of("archivo-abre@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let raw = "Content-Type: text/html; charset=utf-8\r\n\r\n<p>Reserva lista</p>";
        let mut fetch = format!("* 1 FETCH (UID 31 BODY[] {{{}}}\r\n", raw.len()).into_bytes();
        fetch.extend_from_slice(raw.as_bytes());
        fetch.extend_from_slice(b")\r\n");
        let list = b"* LIST (\\Archive) \"/\" \"Archive\"\r\n".to_vec();
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("LIST", list.clone()),
            ("EXAMINE \"Archive\"", b"* 40 EXISTS\r\n".to_vec()),
            ("UID FETCH 31 (BODY.PEEK[])", fetch),
            ("LOGOUT", vec![]),
        ]);
        let message_id = id(&a.id, Folder::Archive, 31);
        assert!(
            mail_on(port)
                .open(std::slice::from_ref(&a), &message_id)
                .unwrap()
                .html
                .contains("Reserva lista")
        );
        handle.join().unwrap();
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("LIST", list.clone()),
            ("SELECT \"Archive\"", b"* 40 EXISTS\r\n".to_vec()),
            ("UID STORE 31 +FLAGS.SILENT (\\Flagged)", vec![]),
            ("LOGOUT", vec![]),
        ]);
        mail_on(port)
            .act(std::slice::from_ref(&a), &message_id, Action::Star)
            .unwrap();
        handle.join().unwrap();
        let (port, handle) = server(vec![("LOGIN", vec![]), ("LIST", list)]);
        assert!(
            mail_on(port)
                .act(std::slice::from_ref(&a), &message_id, Action::Archive)
                .unwrap_err()
                .contains("ya está archivado")
        );
        handle.join().unwrap();
        secrets::forget(&a.address);
    }

    #[test]
    fn what_was_kept_of_a_reading_is_shown_at_once_and_arrivals_join_it_newest_first() {
        let a = account_of("guardado@ejemplo.test");
        let message = |uid: u64, at: i64| {
            let raw = Raw {
                uid,
                date: Some(at),
                header: format!("From: Lucía <lucia@ejemplo.test>\r\nSubject: Asunto {uid}\r\n")
                    .into_bytes(),
                text: b"Hola".to_vec(),
                ..Raw::default()
            };
            message_from(&a, Folder::Inbox, &raw, &HashSet::new())
        };
        let mail = Mail::new();
        assert!(mail.last().is_none());
        mail.restore(Overview {
            accounts: vec![view_of(&a)],
            messages: vec![message(1, 10).0],
            ..Overview::default()
        });
        assert_eq!(mail.last().unwrap().messages.len(), 1);
        assert_eq!(
            mail.sender_of(&id(&a.id, Folder::Inbox, 1)).as_deref(),
            Some("lucia@ejemplo.test"),
            "the ids of what was kept can be acted on"
        );
        // A reading already made is not replaced by an older snapshot.
        mail.restore(Overview::default());
        assert_eq!(mail.last().unwrap().messages.len(), 1);
        mail.arrived(vec![message(2, 20)]);
        mail.arrived(vec![message(2, 20), message(3, 30)]);
        let ids: Vec<_> = mail
            .last()
            .unwrap()
            .messages
            .iter()
            .map(|m| m.id.clone())
            .collect();
        assert_eq!(
            ids,
            vec![
                id(&a.id, Folder::Inbox, 3),
                id(&a.id, Folder::Inbox, 2),
                id(&a.id, Folder::Inbox, 1)
            ]
        );
    }

    #[test]
    fn an_account_that_cannot_be_read_says_why_and_does_not_hide_the_others() {
        let a = account_of("sin-clave@ejemplo.test");
        secrets::forget(&a.address);
        let overview = Mail::new().overview(&[a], &HashSet::new());
        assert!(overview.messages.is_empty());
        assert!(
            overview.problems[0].starts_with("Ana: Falta la contraseña"),
            "{:?}",
            overview.problems
        );
    }

    #[test]
    fn an_opened_message_is_read_whole_cleaned_and_its_pictures_on_the_web_wait_for_the_owner() {
        let a = account_of("abrir@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let raw = "Content-Type: multipart/related; boundary=\"r\"\r\n\r\n--r\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<table><tr><td><img src=\"cid:logo@echo.test\"><img src=\"https://imagenes.ejemplo.test/banner.png\"><script>robar()</script><p>Total S/ 25.36</p></td></tr></table>\r\n--r\r\nContent-Type: image/png\r\nContent-ID: <logo@echo.test>\r\nContent-Transfer-Encoding: base64\r\n\r\nAQID\r\n--r--\r\n";
        let mut fetch = format!("* 1 FETCH (UID 7 BODY[] {{{}}}\r\n", raw.len()).into_bytes();
        fetch.extend_from_slice(raw.as_bytes());
        fetch.extend_from_slice(b")\r\n");
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("EXAMINE \"INBOX\"", b"* 1 EXISTS\r\n".to_vec()),
            ("UID FETCH 7 (BODY.PEEK[])", fetch),
            ("LOGOUT", vec![]),
        ]);
        let mail = mail_on(port);
        let message_id = id(&a.id, Folder::Inbox, 7);
        let opened = mail.open(std::slice::from_ref(&a), &message_id).unwrap();
        handle.join().unwrap();
        assert!(
            opened.html.contains("data:image/png;base64,AQID")
                && opened.html.contains("Total S/ 25.36"),
            "{}",
            opened.html
        );
        assert!(
            !opened.html.contains("robar") && !opened.html.contains("<script"),
            "{}",
            opened.html
        );
        assert_eq!(
            opened.remote,
            vec!["https://imagenes.ejemplo.test/banner.png"]
        );
        assert!(
            opened
                .text
                .iter()
                .any(|paragraph| paragraph.contains("Total S/ 25.36")),
            "{:?}",
            opened.text
        );
        // Only a picture the message named is fetched, and only after the owner asks: any other address is refused first.
        assert!(
            mail.image(&message_id, "https://127.0.0.1/interno.png")
                .is_err()
        );
        assert!(
            mail.image("otra.inbox.7", "https://imagenes.ejemplo.test/banner.png")
                .is_err()
        );
        secrets::forget(&a.address);
    }

    #[test]
    fn a_message_opened_again_and_a_picture_fetched_again_come_from_memory() {
        let a = account_of("memoria@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let raw = "Content-Type: text/html; charset=utf-8\r\n\r\n<p>Total S/ 25.36</p><img src=\"https://imagenes.ejemplo.test/banner.png\">";
        let mut fetch = format!("* 1 FETCH (UID 9 BODY[] {{{}}}\r\n", raw.len()).into_bytes();
        fetch.extend_from_slice(raw.as_bytes());
        fetch.extend_from_slice(b")\r\n");
        // The server answers once: a second connection would fail, so a second reading must come from memory.
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("EXAMINE \"INBOX\"", b"* 1 EXISTS\r\n".to_vec()),
            ("UID FETCH 9 (BODY.PEEK[])", fetch),
            ("LOGOUT", vec![]),
        ]);
        let mail = mail_on(port);
        let message_id = id(&a.id, Folder::Inbox, 9);
        let first = mail.open(std::slice::from_ref(&a), &message_id).unwrap();
        handle.join().unwrap();
        let second = mail.open(std::slice::from_ref(&a), &message_id).unwrap();
        assert_eq!(first, second);
        // The address is named by the message, so its picture is kept once fetched: the second request takes memory.
        mail.pictures.lock().unwrap().put(
            "https://imagenes.ejemplo.test/banner.png".into(),
            "data:image/png;base64,AQID".into(),
            24,
        );
        assert_eq!(
            mail.image(&message_id, "https://imagenes.ejemplo.test/banner.png")
                .unwrap(),
            "data:image/png;base64,AQID"
        );
        secrets::forget(&a.address);
    }

    #[test]
    fn a_message_read_ahead_opens_from_memory_without_a_second_connection() {
        let a = account_of("adelantado@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let raw = "Content-Type: text/html; charset=utf-8\r\n\r\n<p>Total S/ 25.36</p>";
        let mut fetch = format!("* 1 FETCH (UID 11 BODY[] {{{}}}\r\n", raw.len()).into_bytes();
        fetch.extend_from_slice(raw.as_bytes());
        fetch.extend_from_slice(b")\r\n");
        // One connection serves the whole read-ahead; the open afterwards finds no server at all.
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("EXAMINE \"INBOX\"", b"* 1 EXISTS\r\n".to_vec()),
            ("UID FETCH 11 (BODY.PEEK[])", fetch),
            ("LOGOUT", vec![]),
        ]);
        let mail = mail_on(port);
        let message_id = id(&a.id, Folder::Inbox, 11);
        assert!(!mail.is_warm(&message_id));
        mail.prefetch(
            std::slice::from_ref(&a),
            &[message_id.clone(), "no-es-un-id".into()],
        );
        handle.join().unwrap();
        assert!(mail.is_warm(&message_id));
        assert!(
            mail.open(std::slice::from_ref(&a), &message_id)
                .unwrap()
                .html
                .contains("Total S/ 25.36")
        );
        // Asking again for what is already kept connects to nothing.
        mail.prefetch(std::slice::from_ref(&a), &[message_id]);
        secrets::forget(&a.address);
    }

    #[test]
    fn the_last_reading_follows_what_the_owner_did_to_a_message() {
        let mail = Mail::new();
        let message = Message {
            id: "a.inbox.3".into(),
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
            triage: Triage::Routine,
            labels: vec![],
            reason: None,
            auth_failed: false,
        };
        *mail.last.lock().unwrap() = Some(Overview {
            messages: vec![message],
            ..Overview::default()
        });
        mail.touch("a.inbox.3", Action::Star);
        mail.touch("a.inbox.3", Action::Read);
        mail.touch("a.inbox.3", Action::Archive);
        let kept = mail.last().unwrap().messages.remove(0);
        assert_eq!(
            (kept.starred, kept.unread, kept.folder),
            (true, false, Folder::Archive)
        );
    }

    #[test]
    fn archiving_moves_the_message_out_of_the_inbox_and_forgets_its_id() {
        let a = account_of("archivo@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            (
                "LIST",
                b"* LIST (\\HasNoChildren \\Archive) \"/\" \"Archive\"\r\n".to_vec(),
            ),
            ("SELECT \"INBOX\"", b"* 3 EXISTS\r\n".to_vec()),
            ("CAPABILITY", b"* CAPABILITY IMAP4rev1 MOVE\r\n".to_vec()),
            ("UID MOVE 7 \"Archive\"", vec![]),
            ("LOGOUT", vec![]),
        ]);
        let mail = mail_on(port);
        let message_id = id(&a.id, Folder::Inbox, 7);
        mail.entries().insert(
            message_id.clone(),
            Entry {
                account_id: a.id.clone(),
                from_address: "x@y.test".into(),
                subject: "S".into(),
                ..Entry::default()
            },
        );
        mail.act(std::slice::from_ref(&a), &message_id, Action::Archive)
            .unwrap();
        handle.join().unwrap();
        assert_eq!(mail.sender_of(&message_id), None);
        secrets::forget(&a.address);
    }

    #[test]
    fn a_gmail_app_password_is_kept_without_the_spaces_google_shows_in_it() {
        let a = Account::new("claves@ejemplo.test", "Ana", "gmail", None).unwrap();
        let (port, handle) = server(vec![
            ("LOGIN \"claves@ejemplo.test\" \"abcdefghijklmnop\"", vec![]),
            ("EXAMINE \"INBOX\"", b"* 0 EXISTS\r\n".to_vec()),
            ("LOGOUT", vec![]),
        ]);
        mail_on(port)
            .add_account(&a, " abcd efgh ijkl mnop ")
            .unwrap();
        handle.join().unwrap();
        assert_eq!(secrets::get(&a.address).unwrap(), "abcdefghijklmnop");
        secrets::forget(&a.address);
        assert!(
            Mail::new()
                .add_account(&a, "   ")
                .unwrap_err()
                .contains("contraseña")
        );
    }

    #[test]
    fn a_search_reads_the_oldest_of_a_stretch_first_says_more_are_left_and_checks_what_the_server_cannot()
     {
        let a = account_of("busca@ejemplo.test");
        secrets::save(&a.address, "abcd efgh").unwrap();
        let mut fetched = fetch_answer(
            1,
            7,
            "\\Seen",
            "From: BCP <avisos@bcp.com.pe>\r\nSubject: Consumo\r\n\r\n",
            "Consumo de S/ 45.00",
        );
        fetched.extend(fetch_answer(
            2,
            9,
            "",
            "From: Ana <ana@ejemplo.test>\r\nSubject: Hola\r\n\r\n",
            "Hola",
        ));
        let (port, handle) = server(vec![
            ("LOGIN", vec![]),
            ("EXAMINE \"INBOX\"", b"* 3 EXISTS\r\n".to_vec()),
            (
                "UID SEARCH SINCE 17-Oct-2026 OR FROM \"bcp.com.pe\" FROM \"yape.pe\"",
                b"* SEARCH 12 7 9\r\n".to_vec(),
            ),
            (
                "UID FETCH 7,9 (UID INTERNALDATE BODY.PEEK[HEADER.FIELDS (FROM SUBJECT MESSAGE-ID",
                fetched,
            ),
            ("LOGOUT", vec![]),
        ]);
        let (found, more) = mail_on(port)
            .search(
                &a,
                "{from:bcp.com.pe from:yape.pe} after:1792200000",
                2,
                true,
            )
            .unwrap();
        let asked = handle.join().unwrap();
        assert!(more, "a third message is left for the next reading");
        assert_eq!(
            found.len(),
            1,
            "the one whose sender was not asked for is left out: {found:?}"
        );
        assert_eq!(
            (
                found[0].uid,
                found[0].at,
                found[0].from_address.as_str(),
                found[0].text.as_str()
            ),
            (7, 1_792_231_200, "avisos@bcp.com.pe", "Consumo de S/ 45.00")
        );
        assert!(found[0].gmail_id.is_empty());
        assert!(
            asked
                .iter()
                .all(|line| !line.contains("STORE") && !line.contains("SELECT")),
            "read only: {asked:?}"
        );
        secrets::forget(&a.address);
    }

    #[test]
    fn an_action_on_an_unknown_message_or_account_is_refused_before_any_connection() {
        let mail = Mail::new();
        assert!(mail.act(&[account()], "basura", Action::Read).is_err());
        assert!(
            mail.act(&[], &id(&account().id, Folder::Inbox, 1), Action::Read)
                .unwrap_err()
                .contains("ya no está conectada")
        );
        assert!(mail.draft(&[account()], "x", "  ", None, false, 0).is_err());
        assert!(
            mail.draft(
                &[account()],
                &id(&account().id, Folder::Inbox, 1),
                "hola",
                None,
                false,
                0
            )
            .unwrap_err()
            .contains("Actualiza")
        );
    }
}
