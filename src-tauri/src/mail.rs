//! The user's mail in the Inbox: accounts over IMAP with an app password (the
//! `monobot-email` crate, ported from Echo). The account list (no secrets) is a
//! JSON file in the app's data folder; passwords live in the system's store.
//!
//! Nothing here sends by itself: `mail_send` runs only when the user presses
//! Send, and the reply goes to the message's sender alone. No agent tool calls it.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use monobot_email::{Account, Action, Heard, Mail, Opened, Overview, Recipients, Triage, MAX_ACCOUNTS};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// A reading this recent is shown again instead of asking the servers.
const FRESH: Duration = Duration::from_secs(30);

fn mail() -> &'static Arc<Mail> {
    static MAIL: OnceLock<Arc<Mail>> = OnceLock::new();
    MAIL.get_or_init(|| Arc::new(Mail::new()))
}

fn last_read() -> &'static Mutex<Option<Instant>> {
    static READ: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    READ.get_or_init(|| Mutex::new(None))
}

/// Accounts already being watched for new mail.
fn watched() -> &'static Mutex<HashSet<String>> {
    static WATCHED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    WATCHED.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Saved {
    accounts: Vec<Account>,
    /// Senders the user marked as interesting: their mail is priority.
    interesting: Vec<String>,
}

fn saved_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("mail");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("accounts.json"))
}

fn load(app: &AppHandle) -> Saved {
    saved_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn store(app: &AppHandle, saved: &Saved) -> Result<(), String> {
    let text = serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?;
    fs::write(saved_path(app)?, text).map_err(|e| e.to_string())
}

/// The last reading, kept on disk so the tab paints at once on the next start;
/// each body is cut short, as the list only needs its start.
const SNAPSHOT_BODY: usize = 1_500;

fn snapshot_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(saved_path(app)?.with_file_name("snapshot.json"))
}

fn keep_snapshot(app: &AppHandle, overview: &Overview) {
    let kept = for_snapshot(overview);
    if let (Ok(path), Ok(text)) = (snapshot_path(app), serde_json::to_string(&kept)) {
        let _ = fs::write(path, text);
    }
}

/// The reading with each body cut to `SNAPSHOT_BODY` bytes, never mid-character.
fn for_snapshot(overview: &Overview) -> Overview {
    let mut kept = overview.clone();
    for message in &mut kept.messages {
        let mut left = SNAPSHOT_BODY;
        message.body.retain_mut(|paragraph| {
            if left == 0 {
                return false;
            }
            if paragraph.len() > left {
                let mut end = left;
                while !paragraph.is_char_boundary(end) {
                    end -= 1;
                }
                paragraph.truncate(end);
            }
            left = left.saturating_sub(paragraph.len());
            true
        });
    }
    kept
}

fn read_snapshot(app: &AppHandle) -> Option<Overview> {
    let text = fs::read_to_string(snapshot_path(app).ok()?).ok()?;
    serde_json::from_str(&text).ok()
}

/// A full reading from the servers, kept as the snapshot.
fn read_now(app: &AppHandle, saved: &Saved) -> Overview {
    let overview = mail().overview(&saved.accounts, &interesting(saved));
    keep_snapshot(app, &overview);
    *last_read().lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
    overview
}

/// Reads the mail in the background a little after the app starts, and listens
/// for new messages, so the Email tab opens with fresh mail and none is missed.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(8));
        let saved = load(&app);
        if saved.accounts.is_empty() {
            return;
        }
        if let Some(snapshot) = read_snapshot(&app) {
            mail().restore(snapshot);
        }
        watch_all(&app, &saved.accounts);
        read_now(&app, &saved);
        let _ = app.emit("mail-changed", false);
    });
}

fn interesting(saved: &Saved) -> HashSet<String> {
    saved
        .interesting
        .iter()
        .map(|address| address.to_lowercase())
        .collect()
}

/// Listens for new mail on an account and tells the page to read again.
fn watch(app: &AppHandle, account: Account) {
    if !watched()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(account.id.clone())
    {
        return;
    }
    let reader = app.clone();
    let teller = app.clone();
    mail().watch(
        account,
        Arc::new(move || interesting(&load(&reader))),
        Arc::new(move |_, heard| {
            let arrived = matches!(heard, Heard::Arrived(_));
            let _ = teller.emit("mail-changed", arrived);
        }),
    );
}

fn watch_all(app: &AppHandle, accounts: &[Account]) {
    for account in accounts {
        watch(app, account.clone());
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailAccount {
    id: String,
    name: String,
    address: String,
    provider: String,
}

#[tauri::command]
pub fn mail_accounts(app: AppHandle) -> Vec<MailAccount> {
    load(&app)
        .accounts
        .into_iter()
        .map(|account| MailAccount {
            id: account.id,
            name: account.name,
            address: account.address,
            provider: account.provider,
        })
        .collect()
}

/// Connects an account: the server must accept the app password first, and only
/// then is it kept in the system's store.
#[tauri::command]
pub async fn mail_add_account(
    app: AppHandle,
    address: String,
    password: String,
    name: String,
    provider: String,
    host: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let account = Account::new(&address, &name, &provider, host.as_deref())?;
        let mut saved = load(&app);
        if saved.accounts.iter().any(|a| a.id == account.id) {
            return Err("That account is already connected.".into());
        }
        if saved.accounts.len() >= MAX_ACCOUNTS {
            return Err(format!("MonoBOT keeps up to {MAX_ACCOUNTS} mail accounts."));
        }
        mail().add_account(&account, &password)?;
        saved.accounts.push(account.clone());
        store(&app, &saved)?;
        *last_read().lock().unwrap_or_else(|p| p.into_inner()) = None;
        watch(&app, account);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Forgets an account here (its password included). Nothing changes on its server.
#[tauri::command]
pub fn mail_remove_account(app: AppHandle, id: String) -> Result<(), String> {
    let mut saved = load(&app);
    let Some(at) = saved.accounts.iter().position(|a| a.id == id) else {
        return Ok(());
    };
    let removed = saved.accounts.remove(at);
    mail().remove_account(&removed);
    watched()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&removed.id);
    *last_read().lock().unwrap_or_else(|p| p.into_inner()) = None;
    // Nothing of its messages stays on this computer.
    if let Ok(path) = snapshot_path(&app) {
        let _ = fs::remove_file(path);
    }
    store(&app, &saved)
}

/// The accounts' newest messages. A reading under 30 s old is reused unless
/// `force`; an account that cannot be read says why in `problems`.
#[tauri::command]
pub async fn mail_overview(app: AppHandle, force: bool) -> Result<Overview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let saved = load(&app);
        watch_all(&app, &saved.accounts);
        let at = last_read().lock().unwrap_or_else(|p| p.into_inner());
        if !force && at.is_some_and(|read| read.elapsed() < FRESH) {
            if let Some(last) = mail().last() {
                return Ok(last);
            }
        }
        drop(at);
        Ok(read_now(&app, &saved))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The last reading, at once: from memory, else the snapshot on disk. None
/// before the first reading. The page shows it, then asks for a fresh one.
#[tauri::command]
pub async fn mail_last(app: AppHandle) -> Result<Option<Overview>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(last) = mail().last() {
            return Ok(Some(last));
        }
        let snapshot = read_snapshot(&app);
        if let Some(snapshot) = &snapshot {
            mail().restore(snapshot.clone());
        }
        Ok(snapshot)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The most messages one search gives back per account.
const SEARCH_MAX: usize = 50;

/// A message a search found, as an agent reads it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundMail {
    /// When it arrived, in Unix milliseconds (0: the server did not say).
    at: i64,
    from: String,
    address: String,
    subject: String,
    /// The first lines of its text, without HTML or attachments.
    text: String,
    /// The same on every server that keeps it: what an agent keys a movement by.
    message_id: String,
    /// Gmail's own id, empty on other servers.
    gmail_id: String,
}

/// What one account answered to a search.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSearch {
    address: String,
    found: Vec<FoundMail>,
    /// More messages matched than `limit`.
    more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    problem: Option<String>,
}

/// Searches the whole history of the accounts named in `addresses` (all of them
/// when unset) with the words of Gmail's search box, each account at once. Read
/// only: nothing is marked, moved or kept. The page decides which accounts an
/// agent may search; this only searches the ones it is given.
#[tauri::command]
pub async fn mail_search(
    app: AppHandle,
    query: String,
    addresses: Option<Vec<String>>,
    limit: usize,
    oldest_first: bool,
) -> Result<Vec<AccountSearch>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let limit = limit.clamp(1, SEARCH_MAX);
        let accounts: Vec<Account> = load(&app)
            .accounts
            .into_iter()
            .filter(|account| {
                addresses.as_ref().is_none_or(|wanted| {
                    wanted.iter().any(|a| a.eq_ignore_ascii_case(&account.address))
                })
            })
            .collect();
        let results = std::thread::scope(|scope| {
            let jobs: Vec<_> = accounts
                .iter()
                .map(|account| {
                    let query = query.as_str();
                    scope.spawn(move || (account, mail().search(account, query, limit, oldest_first)))
                })
                .collect();
            jobs.into_iter().filter_map(|job| job.join().ok()).collect::<Vec<_>>()
        });
        Ok(results
            .into_iter()
            .map(|(account, result)| match result {
                Ok((found, more)) => AccountSearch {
                    address: account.address.clone(),
                    found: found
                        .into_iter()
                        .map(|f| FoundMail {
                            at: f.at.saturating_mul(1000),
                            from: f.from_name,
                            address: f.from_address,
                            subject: f.subject,
                            text: f.text,
                            message_id: f.message_id,
                            gmail_id: f.gmail_id,
                        })
                        .collect(),
                    more,
                    problem: None,
                },
                Err(problem) => AccountSearch {
                    address: account.address.clone(),
                    found: Vec::new(),
                    more: false,
                    problem: Some(problem),
                },
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reads ahead the messages the user is likely to open next, so opening them
/// comes from memory. Nothing is marked read.
#[tauri::command]
pub fn mail_prefetch(app: AppHandle, ids: Vec<String>) {
    let ids: Vec<String> = ids.into_iter().filter(|id| !mail().is_warm(id)).collect();
    if ids.is_empty() {
        return;
    }
    std::thread::spawn(move || mail().prefetch(&load(&app).accounts, &ids));
}

/// The next stretch of the archive.
#[tauri::command]
pub async fn mail_archive_more(app: AppHandle) -> Result<Overview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let saved = load(&app);
        mail().archive_more(&saved.accounts, &interesting(&saved))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A message read whole. Opening it does not mark it read.
#[tauri::command]
pub async fn mail_open(app: AppHandle, id: String) -> Result<Opened, String> {
    tauri::async_runtime::spawn_blocking(move || mail().open(&load(&app).accounts, &id))
        .await
        .map_err(|e| e.to_string())?
}

/// read, star, unstar or archive.
#[tauri::command]
pub async fn mail_act(app: AppHandle, id: String, action: String) -> Result<(), String> {
    let action = Action::parse(&action).ok_or("Unknown mail action")?;
    tauri::async_runtime::spawn_blocking(move || mail().act(&load(&app).accounts, &id, action))
        .await
        .map_err(|e| e.to_string())?
}

/// Marks a sender interesting, or not: their mail goes to priority.
#[tauri::command]
pub fn mail_interesting(app: AppHandle, address: String, on: bool) -> Result<(), String> {
    let mut saved = load(&app);
    let address = address.trim().to_lowercase();
    saved
        .interesting
        .retain(|kept| kept.to_lowercase() != address);
    if on {
        saved.interesting.push(address);
    }
    *last_read().lock().unwrap_or_else(|p| p.into_inner()) = None;
    store(&app, &saved)
}

/// Held messages (a scam, a failed sender check) cannot be answered.
fn may_answer(id: &str) -> Result<(), String> {
    let held = mail()
        .last()
        .and_then(|overview| overview.messages.into_iter().find(|m| m.id == id))
        .is_some_and(|message| message.triage == Triage::Held);
    if held {
        Err(
            "This message is held back: it may be a scam, so it can't be answered from here."
                .into(),
        )
    } else {
        Ok(())
    }
}

/// Who a reply goes to: whom the message asks replies to go to, else its sender, and with `all` a copy to everyone
/// else it went to. The page shows them; it cannot add anyone.
#[tauri::command]
pub async fn mail_recipients(app: AppHandle, id: String, all: bool) -> Result<Recipients, String> {
    tauri::async_runtime::spawn_blocking(move || mail().recipients(&load(&app).accounts, &id, all))
        .await
        .map_err(|e| e.to_string())?
}

/// Saves the reply as a draft on the account's server. `subject` is the one the user chose, else «Re: » and theirs.
#[tauri::command]
pub async fn mail_draft(app: AppHandle, id: String, text: String, subject: Option<String>, all: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        may_answer(&id)?;
        mail().draft(&load(&app).accounts, &id, &text, subject.as_deref(), all, now_ms())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Sends the reply to the people `mail_recipients` names. Only the user's own press calls this.
#[tauri::command]
pub async fn mail_send(app: AppHandle, id: String, text: String, subject: Option<String>, all: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        may_answer(&id)?;
        if text.trim().is_empty() {
            return Err("Write the reply first.".into());
        }
        mail().send(&load(&app).accounts, &id, &text, subject.as_deref(), all, now_ms())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A picture on the web the message names, which the user chose to load.
#[tauri::command]
pub async fn mail_image(id: String, url: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || mail().image(&id, &url))
        .await
        .map_err(|e| e.to_string())?
}

/// Saves an attachment to the app's cache and returns its path, for the user
/// to open with the system's app. It is never run.
#[tauri::command]
pub async fn mail_attachment(app: AppHandle, id: String, index: usize) -> Result<String, String> {
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let (name, bytes) = mail().attachment(&load(&app).accounts, &id, index)?;
        let folder = cache
            .join("mail-attachments")
            .join(id.replace(['/', '\\', ':'], "_"))
            .join(index.to_string());
        fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let path = folder.join(name);
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overview(body: Vec<String>) -> Overview {
        serde_json::from_value(serde_json::json!({
            "accounts": [],
            "problems": [],
            "messages": [{
                "id": "a.inbox.1", "accountId": "a", "folder": "inbox", "from": "Ana",
                "address": "ana@mina.pe", "subject": "Pozas", "preview": "…", "body": body,
                "at": 0, "unread": true, "starred": false, "triage": "routine", "labels": []
            }]
        }))
        .unwrap()
    }

    #[test]
    fn a_snapshot_keeps_the_start_of_each_body() {
        let long = "ñ".repeat(SNAPSHOT_BODY); // two bytes each
        let kept = for_snapshot(&overview(vec![
            "Hola Ana".into(),
            long,
            "never kept".into(),
        ]));
        let body = &kept.messages[0].body;
        assert_eq!(body[0], "Hola Ana");
        assert!(body[1].len() <= SNAPSHOT_BODY - "Hola Ana".len());
        assert!(body[1].chars().all(|c| c == 'ñ'));
        assert_eq!(body.len(), 2);
    }

    #[test]
    fn a_short_body_is_kept_whole() {
        let kept = for_snapshot(&overview(vec!["Gracias".into()]));
        assert_eq!(kept.messages[0].body, vec!["Gracias".to_string()]);
    }
}
