use sha2::{Digest, Sha256};
use std::path::Path;

fn name(dir: &Path, kind: &str) -> String {
    format!(
        "mail-workspace-{kind}-{:x}",
        Sha256::digest(dir.to_string_lossy().as_bytes())
    )
}
#[cfg(not(test))]
pub(super) fn set(dir: &Path, kind: &str, value: &str) -> Result<(), String> {
    let entry = crate::secrets::entry("com.monocode.desktop", &name(dir, kind))
        .map_err(|_| "System credential store is unavailable.")?;
    if value.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Could not remove mail credential.".into()),
        }
    } else {
        entry
            .set_password(value)
            .map_err(|_| "Could not save mail credential.".into())
    }
}
#[cfg(not(test))]
pub(super) fn get(dir: &Path, kind: &str) -> Result<String, String> {
    crate::secrets::entry("com.monocode.desktop", &name(dir, kind))
        .and_then(|entry| entry.get_password())
        .map_err(|_| format!("Configure the {kind} token in the mail workspace settings."))
}
#[cfg(test)]
fn memory() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    static MEMORY: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, String>>,
    > = std::sync::OnceLock::new();
    MEMORY.get_or_init(Default::default)
}
#[cfg(test)]
pub(super) fn set(dir: &Path, kind: &str, value: &str) -> Result<(), String> {
    let mut memory = memory().lock().unwrap();
    if value.is_empty() {
        memory.remove(&name(dir, kind));
    } else {
        memory.insert(name(dir, kind), value.into());
    }
    Ok(())
}
#[cfg(test)]
pub(super) fn get(dir: &Path, kind: &str) -> Result<String, String> {
    memory()
        .lock()
        .unwrap()
        .get(&name(dir, kind))
        .cloned()
        .ok_or_else(|| "Missing credential.".into())
}
