//! The password of an account, in the system's store of secrets (Credential Manager on Windows, Keychain on the
//! Mac): never in a file, a setting or a log. Echo Dev and the installed Echo keep their own. In this crate's tests
//! the passwords live in memory: a test never reads, or makes the system ask for, the user's own secrets.

use crate::account::id_of;

#[cfg(not(test))]
// MonoBOT's internal identifier, like its other stored secrets.
const SERVICE: &str = "com.monocode.desktop";

fn name(address: &str) -> String {
    format!("email-imap-{}", id_of(address))
}

#[cfg(test)]
fn memory() -> std::sync::MutexGuard<'static, std::collections::HashMap<String, String>> {
    static MEMORY: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> = std::sync::OnceLock::new();
    MEMORY.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(not(test))]
pub fn save(address: &str, password: &str) -> Result<(), String> {
    crate::base::keychain::entry(SERVICE, &name(address))
        .and_then(|entry| entry.set_password(password))
        .map_err(|_| "El sistema no dejó guardar la contraseña de esta cuenta.".to_string())
}

#[cfg(test)]
pub fn save(address: &str, password: &str) -> Result<(), String> {
    memory().insert(name(address), password.to_string());
    Ok(())
}

#[cfg(not(test))]
pub fn get(address: &str) -> Result<String, String> {
    crate::base::keychain::entry(SERVICE, &name(address))
        .and_then(|entry| entry.get_password())
        .map_err(|_| "Falta la contraseña de esta cuenta: quítala y vuelve a añadirla.".to_string())
}

#[cfg(test)]
pub fn get(address: &str) -> Result<String, String> {
    memory().get(&name(address)).cloned().ok_or_else(|| "Falta la contraseña de esta cuenta: quítala y vuelve a añadirla.".to_string())
}

/// Forgets the password; one that was not there is not an error.
#[cfg(not(test))]
pub fn forget(address: &str) {
    if let Ok(entry) = crate::base::keychain::entry(SERVICE, &name(address)) {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
pub fn forget(address: &str) {
    memory().remove(&name(address));
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_password_is_kept_and_forgotten_by_address() {
        super::save("ana@ejemplo.test", "abcd efgh").unwrap();
        assert_eq!(super::get("ANA@ejemplo.test").unwrap(), "abcd efgh");
        super::forget("ana@ejemplo.test");
        assert!(super::get("ana@ejemplo.test").is_err());
    }
}
