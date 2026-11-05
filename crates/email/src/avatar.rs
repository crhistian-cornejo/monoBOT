//! The logo of whoever writes, when their domain has one: the icon its own web publishes. Echo asks that web and no one else,
//! from `remote.rs` (public `https` only, no cookies, no redirects, only pictures), so what it learns is this computer's address
//! and that someone asked for its icon, not who or which message. The answer is kept on this computer for weeks, so a domain is
//! asked rarely. An address of a free mail service says nothing of the person, and its service's logo would say the wrong thing.

use std::path::Path;
use std::time::{Duration, SystemTime};

const FREE_MAIL: &[&str] = &[
    "gmail.com", "googlemail.com", "outlook.com", "hotmail.com", "live.com", "msn.com", "yahoo.com", "yahoo.es", "ymail.com", "icloud.com", "me.com", "mac.com",
    "proton.me", "protonmail.com", "aol.com", "gmx.com", "gmx.net", "mail.com", "zoho.com", "yandex.com", "tutanota.com",
];
/// How long a found logo, and the word that a domain has none, are believed.
const KEEP_FOUND: Duration = Duration::from_secs(30 * 24 * 3600);
const KEEP_NONE: Duration = Duration::from_secs(7 * 24 * 3600);

/// The domain an address writes from, in a form safe for a web address and a file name; none for one with no logo to look for.
pub fn domain_of(address: &str) -> Option<String> {
    let domain = address.rsplit_once('@')?.1.trim().to_ascii_lowercase();
    let valid = domain.contains('.') && domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') && !domain.starts_with(['.', '-']);
    (valid && !FREE_MAIL.contains(&domain.as_str())).then_some(domain)
}

/// The company's own domain under a sending one: `mail.notion.so` is `notion.so`, `correo.bcp.com.pe` is `bcp.com.pe`.
fn company_of(domain: &str) -> String {
    let labels: Vec<&str> = domain.split('.').collect();
    let two_part = labels.len() >= 3 && labels[labels.len() - 1].len() == 2 && labels[labels.len() - 2].len() <= 3;
    labels[labels.len().saturating_sub(if two_part { 3 } else { 2 })..].join(".")
}

/// Where the logo may be, the sharpest first.
pub fn places_of(domain: &str) -> Vec<String> {
    let company = company_of(domain);
    let mut hosts = vec![company.clone(), format!("www.{company}")];
    if domain != company {
        hosts.insert(0, domain.to_string());
    }
    ["apple-touch-icon.png", "favicon.ico"].iter().flat_map(|file| hosts.iter().map(move |host| format!("https://{host}/{file}"))).collect()
}

/// The logo of `domain`, as a data URL: the first place that gives a picture.
pub fn find(domain: &str, fetch: &dyn Fn(&str) -> Result<String, String>) -> Option<String> {
    places_of(domain).iter().find_map(|place| fetch(place).ok())
}

/// What is kept of a domain: its picture, or that it has none (`Some(None)`); `None` when nothing is, or it is too old.
fn kept(dir: &Path, domain: &str, now: SystemTime) -> Option<Option<String>> {
    let file = dir.join(domain);
    let age = now.duration_since(std::fs::metadata(&file).ok()?.modified().ok()?).unwrap_or_default();
    let text = std::fs::read_to_string(&file).ok()?;
    if age > (if text.is_empty() { KEEP_NONE } else { KEEP_FOUND }) {
        return None;
    }
    Some((!text.is_empty()).then_some(text))
}

/// The logo of whoever writes from `address`: what is kept of the domain, or else what its web gives, which is kept.
fn of_address(dir: &Path, address: &str, fetch: &dyn Fn(&str) -> Result<String, String>) -> Option<String> {
    let domain = domain_of(address)?;
    if let Some(known) = kept(dir, &domain, SystemTime::now()) {
        return known;
    }
    let found = find(&domain, fetch);
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = std::fs::write(dir.join(&domain), found.as_deref().unwrap_or_default());
    }
    found
}

/// The logo of whoever writes from `address`, from the web of their domain.
pub fn logo(dir: &Path, address: &str) -> Option<String> {
    of_address(dir, address, &crate::remote::fetch_icon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn looks_for_a_logo_only_where_it_says_something_of_the_sender() {
        assert_eq!(domain_of("Ana@Notion.so").as_deref(), Some("notion.so"));
        assert_eq!(domain_of("persona@gmail.com"), None);
        assert_eq!(domain_of("x@evil.test/../../secreto"), None);
        assert_eq!(domain_of("sin-arroba"), None);
        assert_eq!(domain_of("x@localhost"), None);
    }

    #[test]
    fn asks_the_senders_own_domain_and_its_company() {
        assert_eq!(places_of("mail.notion.so")[..3], ["https://mail.notion.so/apple-touch-icon.png", "https://notion.so/apple-touch-icon.png", "https://www.notion.so/apple-touch-icon.png"]);
        assert!(places_of("correo.bcp.com.pe").contains(&"https://bcp.com.pe/favicon.ico".to_string()));
        assert!(places_of("notion.so").iter().all(|place| !place.contains("//so/")));
    }

    #[test]
    fn keeps_what_it_found_and_that_there_is_nothing_so_it_asks_once() {
        let dir = std::env::temp_dir().join(format!("echo-avatares-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let asked = RefCell::new(0);
        let found = |place: &str| { *asked.borrow_mut() += 1; if place.ends_with("favicon.ico") { Ok("data:image/x-icon;base64,AA".to_string()) } else { Err("no".to_string()) } };
        assert_eq!(of_address(&dir, "a@tienda.test", &found).as_deref(), Some("data:image/x-icon;base64,AA"));
        let times = *asked.borrow();
        assert_eq!(of_address(&dir, "b@tienda.test", &found).as_deref(), Some("data:image/x-icon;base64,AA"));
        assert_eq!(*asked.borrow(), times, "the second sender of a domain asks nothing");
        let none = |_: &str| { *asked.borrow_mut() += 1; Err("no".to_string()) };
        assert_eq!(of_address(&dir, "a@nada.test", &none), None);
        let times = *asked.borrow();
        assert_eq!(of_address(&dir, "a@nada.test", &none), None);
        assert_eq!(*asked.borrow(), times, "that a domain has none is kept too");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
