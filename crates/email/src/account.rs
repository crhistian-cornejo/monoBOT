//! The accounts the user connected. An account is an address, a server and a password; the password is kept in the
//! system's store of secrets (`secrets.rs`), never here.

use serde::{Deserialize, Serialize};

/// How many accounts Echo keeps at once.
pub const MAX_ACCOUNTS: usize = 8;
const MAX_NAME: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Stable for an address (`id_of`): what the messages say they belong to.
    pub id: String,
    pub name: String,
    pub address: String,
    /// `gmail`, `icloud` or `imap` (any other server).
    pub provider: String,
    pub host: String,
    pub port: u16,
}

impl Account {
    /// An account from what the user typed. `provider`: `gmail`, `icloud` or `imap`, which also needs `host`.
    pub fn new(address: &str, name: &str, provider: &str, host: Option<&str>) -> Result<Self, String> {
        let address = address.trim().to_lowercase();
        if !valid_address(&address) {
            return Err("Escribe la dirección de correo completa.".into());
        }
        let (provider, host) = match provider {
            "gmail" => ("gmail", "imap.gmail.com".to_string()),
            "icloud" => ("icloud", "imap.mail.me.com".to_string()),
            "imap" => {
                let host = host.map(str::trim).unwrap_or_default().to_lowercase();
                if !valid_host(&host) {
                    return Err("Escribe el servidor IMAP de la cuenta (por ejemplo imap.tudominio.com).".into());
                }
                ("imap", host)
            }
            _ => return Err("Ese proveedor no está disponible.".into()),
        };
        let name: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_NAME).collect();
        let name = if name.is_empty() { address.split('@').next().unwrap_or_default().to_string() } else { name };
        Ok(Self { id: id_of(&address), name, address, provider: provider.into(), host, port: 993 })
    }

    pub fn is_gmail(&self) -> bool {
        self.provider == "gmail"
    }
}

/// A short stable name for an address, for ids and for the secret's name (FNV-1a).
pub fn id_of(address: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in address.trim().to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// An address that can safely go into a command: no spaces, quotes, backslashes or control characters.
pub fn valid_address(address: &str) -> bool {
    if address.len() > 254 || address.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\\' | '<' | '>' | '(' | ')' | ',' | ';')) {
        return false;
    }
    let Some((local, domain)) = address.split_once('@') else { return false };
    !local.is_empty() && !domain.contains('@') && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn valid_host(host: &str) -> bool {
    host.len() <= 253 && host.contains('.') && !host.starts_with('.') && !host.ends_with('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gmail_account_has_its_server_and_a_stable_id() {
        let account = Account::new("  Ana@Ejemplo.test ", "", "gmail", None).unwrap();
        assert_eq!((account.address.as_str(), account.host.as_str(), account.port, account.name.as_str()), ("ana@ejemplo.test", "imap.gmail.com", 993, "ana"));
        assert_eq!(account.id, id_of("ANA@ejemplo.test"));
        assert_eq!(account.id.len(), 16);
    }

    #[test]
    fn what_cannot_go_into_a_command_is_refused() {
        assert!(Account::new("ana ejemplo.test", "", "gmail", None).is_err());
        assert!(Account::new("a\"b@ejemplo.test", "", "gmail", None).is_err());
        assert!(Account::new("ana@ejemplo.test", "", "imap", Some("mail.x y.test")).is_err());
        assert!(Account::new("ana@ejemplo.test", "", "imap", None).is_err());
        assert!(Account::new("ana@ejemplo.test", "", "otro", None).is_err());
        assert_eq!(Account::new("ana@ejemplo.test", "Estudio", "imap", Some("IMAP.Ejemplo.test")).unwrap().host, "imap.ejemplo.test");
    }
}
