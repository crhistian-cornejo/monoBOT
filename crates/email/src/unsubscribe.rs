//! Stopping a list's mail with the one press its sender offers (RFC 8058): a `POST` of
//! `List-Unsubscribe=One-Click` to the `https` link of its `List-Unsubscribe`, with no cookie,
//! no login and no redirect followed. Only the owner's press gets here. A link to this computer or
//! its network is refused: a message must not make the app reach inside the owner's network.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

/// The host of an `https` link, if it is one a message may make the app reach.
pub fn public_host(link: &str) -> Result<String, String> {
    let rest = link
        .strip_prefix("https://")
        .or_else(|| link.strip_prefix("HTTPS://"))
        .ok_or("Only https links unsubscribe with one press.")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err("That unsubscribe link is not a plain web address.".into());
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or_default().to_owned()
    } else {
        authority
            .split(':')
            .next()
            .unwrap_or_default()
            .to_lowercase()
    };
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err("That unsubscribe link points inside your network.".into());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        let private = match ip {
            IpAddr::V4(v4) => {
                v4.is_private()
                    || v4.is_loopback()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4 == Ipv4Addr::BROADCAST
                    || v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1])
            }
            IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
                    || v6 == Ipv6Addr::LOCALHOST
            }
        };
        if private {
            return Err("That unsubscribe link points inside your network.".into());
        }
    }
    Ok(host)
}

/// Unsubscribes with one request to `link`.
pub fn one_click(link: &str) -> Result<(), String> {
    public_host(link)?;
    let response = monobot_http::agent()
        .redirects(0)
        .timeout(Duration::from_secs(20))
        .build()
        .post(link)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string("List-Unsubscribe=One-Click");
    match response {
        Ok(response) if (200..400).contains(&response.status()) => {
            let mut sink = Vec::new();
            let _ = response.into_reader().take(16_384).read_to_end(&mut sink);
            Ok(())
        }
        Ok(response) => Err(format!(
            "The sender's page answered HTTP {}. Try its link in the message instead.",
            response.status()
        )),
        Err(ureq::Error::Status(status, _)) => Err(format!(
            "The sender's page answered HTTP {status}. Try its link in the message instead."
        )),
        Err(_) => Err("The sender's page could not be reached.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_public_https_link_is_reached() {
        assert_eq!(
            public_host("https://news.example.com/u?id=1").unwrap(),
            "news.example.com"
        );
        assert!(public_host("http://news.example.com/u").is_err());
        assert!(public_host("https://localhost/u").is_err());
        assert!(public_host("https://192.168.1.1/u").is_err());
        assert!(public_host("https://10.0.0.5:8443/u").is_err());
        assert!(public_host("https://[::1]/u").is_err());
        assert!(public_host("https://user@evil.example/u").is_err());
        assert!(public_host("https://printer.local/u").is_err());
        assert!(public_host("https://93.184.216.34/u").is_ok());
    }
}
