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

use monobot_email::{Account, Action, Heard, Mail, Opened, Overview, Triage, MAX_ACCOUNTS};
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
    store(&app, &saved)
}

/// The accounts' newest messages. A reading under 30 s old is reused unless
/// `force`; an account that cannot be read says why in `problems`.
#[tauri::command]
pub async fn mail_overview(app: AppHandle, force: bool) -> Result<Overview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let saved = load(&app);
        watch_all(&app, &saved.accounts);
        let mut at = last_read().lock().unwrap_or_else(|p| p.into_inner());
        if !force && at.is_some_and(|read| read.elapsed() < FRESH) {
            if let Some(last) = mail().last() {
                return Ok(last);
            }
        }
        let overview = mail().overview(&saved.accounts, &interesting(&saved));
        *at = Some(Instant::now());
        Ok(overview)
    })
    .await
    .map_err(|e| e.to_string())?
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

/// Saves the reply as a draft on the account's server.
#[tauri::command]
pub async fn mail_draft(app: AppHandle, id: String, text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        may_answer(&id)?;
        mail().draft(&load(&app).accounts, &id, &text, now_ms())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Sends the reply to the message's sender. Only the user's own press calls this.
#[tauri::command]
pub async fn mail_send(app: AppHandle, id: String, text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        may_answer(&id)?;
        if text.trim().is_empty() {
            return Err("Write the reply first.".into());
        }
        mail().send(&load(&app).accounts, &id, &text, now_ms())
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
