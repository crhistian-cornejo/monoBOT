//! The pictures on the web that a message names, fetched when the owner asks for them. Echo fetches them here, not in the
//! page: no cookie and no referrer goes with the request, and only an `https` address that is public is asked for
//! (never this machine, the network around it, or a name that stands for one). The picture must be of a known type and
//! at most `MAX_PICTURE` bytes, and a redirect is not followed: it would be a request to an address nobody checked.

use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

use base64::Engine;

/// The biggest picture that is taken.
pub const MAX_PICTURE: u64 = 5 * 1024 * 1024;
const TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The biggest logo that is taken, and what a logo may be besides a picture of a message: the icon of a web.
const MAX_ICON: u64 = 256 * 1024;
const ICON_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/x-icon",
    "image/vnd.microsoft.icon",
];

/// The picture at `url`, as a data URL. `url` must be one the message named (`service.rs` checks that first).
pub fn fetch(url: &str) -> Result<String, String> {
    fetch_within(url, TYPES, MAX_PICTURE, 15)
}

/// The icon of a sender's web (`avatar.rs`): smaller and quicker to give up on than a message's picture.
pub fn fetch_icon(url: &str) -> Result<String, String> {
    fetch_within(url, ICON_TYPES, MAX_ICON, 5)
}

fn fetch_within(url: &str, types: &[&str], limit: u64, seconds: u64) -> Result<String, String> {
    check(url)?;
    let agent = monobot_http::agent()
        .timeout(Duration::from_secs(seconds))
        .redirects(0)
        .build();
    let response = agent
        .get(url)
        .set("Accept", &types.join(","))
        .call()
        .map_err(|_| "Could not load that image.".to_string())?;
    let kind = response
        .header("content-type")
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !types.contains(&kind.as_str()) {
        return Err("That address is not an image MonoBOT shows.".into());
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The image was cut off.".to_string())?;
    if bytes.len() as u64 > limit {
        return Err("The image is too large.".into());
    }
    Ok(format!(
        "data:{kind};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Refuses anything but a public `https` address: a `user@` in it, a name for this machine or its network, a bracketed
/// IPv6 address, an address that is not public, or a name that resolves to one that is not.
pub fn check(url: &str) -> Result<(), String> {
    let refuse = || "Echo solo trae imágenes de direcciones https públicas.".to_string();
    let rest = url.strip_prefix("https://").ok_or_else(refuse)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') || authority.starts_with('[') {
        return Err(refuse());
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| refuse())?),
        None => (authority, 443),
    };
    let lower = host.to_ascii_lowercase();
    if ["localhost", ".localhost", ".local", ".internal", ".lan"]
        .iter()
        .any(|name| lower == *name || lower.ends_with(name))
    {
        return Err(refuse());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if public(ip) { Ok(()) } else { Err(refuse()) };
    }
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|_| "No se encontró el servidor de esa imagen.".to_string())?
        .collect();
    if addresses.is_empty() || addresses.iter().any(|address| !public(address.ip())) {
        return Err(refuse());
    }
    Ok(())
}

/// Whether an address is one of the internet's (not private, loopback, link-local, shared, reserved or documentation).
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || a >= 240)
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => public(IpAddr::V4(v4)),
            None => {
                let first = v6.segments()[0];
                !(v6.is_loopback()
                    || v6.is_unspecified()
                    || first & 0xfe00 == 0xfc00
                    || first & 0xffc0 == 0xfe80)
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_public_https_address_is_asked_for() {
        for url in [
            "https://8.8.8.8/logo.png",
            "https://93.184.216.34:443/logo.png",
        ] {
            assert!(check(url).is_ok(), "{url}");
        }
        for url in [
            "http://8.8.8.8/logo.png",
            "https://usuario:clave@93.184.216.34/logo.png",
            "https://127.0.0.1/logo.png",
            "https://10.1.2.3:8443/logo.png",
            "https://192.168.0.10/logo.png",
            "https://169.254.169.254/latest/meta-data",
            "https://100.64.0.1/logo.png",
            "https://[::1]/logo.png",
            "https://[fd00::1]/logo.png",
            "https://localhost/logo.png",
            "https://imagenes.localhost/logo.png",
            "https://equipo.local/logo.png",
            "https://",
            "https://ejemplo.test:puerto/logo.png",
        ] {
            assert!(check(url).is_err(), "{url} must be refused");
        }
    }

    #[test]
    fn a_mapped_private_address_is_not_public() {
        assert!(!public("::ffff:192.168.0.1".parse().unwrap()));
        assert!(public("::ffff:8.8.8.8".parse().unwrap()));
    }
}
