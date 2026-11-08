//! Optional HTTPS delivery for a reply the owner has approved. No retries or SMTP
//! fallback: a timeout may mean the provider already accepted the message.
use crate::{Recipients, valid_address};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Transport {
    #[default]
    Smtp,
    Resend,
    Cloudflare {
        account_id: String,
    },
}

pub fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
        return Err("Enter a valid API token without line breaks.".into());
    }
    Ok(())
}

fn request(
    transport: &Transport,
    from: &str,
    people: &Recipients,
    raw: &str,
) -> Result<(String, Value), String> {
    let count = people.to.len() + people.cc.len();
    if !valid_address(from)
        || count == 0
        || count > 50
        || people
            .to
            .iter()
            .chain(&people.cc)
            .any(|p| !valid_address(&p.address))
    {
        return Err("Choose between 1 and 50 valid recipients.".into());
    }
    if raw.len() > 2 * 1024 * 1024 {
        return Err("The reply is too large for HTTP delivery.".into());
    }
    let (head, body) = raw.split_once("\r\n\r\n").ok_or("Invalid reply message.")?;
    let parsed = crate::mime::parse(head.as_bytes(), body.as_bytes());
    let mut headers = serde_json::Map::new();
    for line in head.lines() {
        if let Some((name, value)) = line.split_once(':')
            && ["In-Reply-To", "References", "Message-ID"]
                .iter()
                .any(|n| name.eq_ignore_ascii_case(n))
        {
            headers.insert(name.to_string(), json!(value.trim()));
        }
    }
    let to: Vec<&str> = people.to.iter().map(|p| p.address.as_str()).collect();
    let cc: Vec<&str> = people.cc.iter().map(|p| p.address.as_str()).collect();
    let payload = json!({
        "from": from, "to": to, "cc": cc, "subject": parsed.subject,
        "text": crate::mime::parse_whole(raw.as_bytes()).text, "headers": headers,
    });
    let url = match transport {
        Transport::Smtp => return Err("SMTP does not use HTTP delivery.".into()),
        Transport::Resend => "https://api.resend.com/emails".to_owned(),
        Transport::Cloudflare { account_id } => {
            if account_id.len() != 32 || !account_id.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err("Cloudflare account ID must contain 32 hexadecimal characters.".into());
            }
            format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/email/sending/send")
        }
    };
    Ok((url, payload))
}

fn accepted(transport: &Transport, value: &Value, recipients: &[&str]) -> Result<(), String> {
    let ok = match transport {
        Transport::Resend => value["id"].as_str().is_some_and(|id| !id.is_empty()),
        Transport::Cloudflare { .. } => {
            value["success"] == true
                && value["result"]["permanent_bounces"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                && !recipients.is_empty()
                && recipients.iter().all(|address| {
                    ["delivered", "queued"].iter().any(|key| {
                        value["result"][key].as_array().is_some_and(|v| {
                            v.iter().any(|entry| {
                                entry
                                    .as_str()
                                    .is_some_and(|sent| sent.eq_ignore_ascii_case(address))
                            })
                        })
                    })
                })
        }
        Transport::Smtp => false,
    };
    ok.then_some(()).ok_or_else(|| {
        "The provider did not confirm delivery to everyone. Some recipients may already have received it; check the provider before sending again.".into()
    })
}

pub fn send(
    transport: &Transport,
    token: &str,
    from: &str,
    people: &Recipients,
    raw: &str,
) -> Result<(), String> {
    validate_token(token)?;
    let (url, payload) = request(transport, from, people, raw)?;
    let recipients: Vec<&str> = people
        .to
        .iter()
        .chain(&people.cc)
        .map(|p| p.address.as_str())
        .collect();
    deliver(&url, transport, token, &payload, &recipients)
}

fn deliver(
    url: &str,
    transport: &Transport,
    token: &str,
    payload: &Value,
    recipients: &[&str],
) -> Result<(), String> {
    let response = monobot_http::agent()
        .redirects(0)
        .timeout(Duration::from_secs(30))
        .build()
        .post(url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => format!("Email provider returned HTTP {status}. No automatic retry was attempted."),
            _ => "Email delivery could not be confirmed. Check the provider before retrying; it may already have accepted this reply.".to_owned(),
        })?;
    if !(200..300).contains(&response.status()) {
        return Err(format!(
            "Email provider returned HTTP {}. Redirects are never followed.",
            response.status()
        ));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(
            |_| "Email provider response could not be read. Check delivery before retrying.",
        )?;
    if bytes.len() > 65_536 {
        return Err(
            "Email provider response was too large. Check delivery before retrying.".into(),
        );
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(
        |_| "Email provider returned an invalid response. Check delivery before retrying.",
    )?;
    accepted(transport, &value, recipients)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Person;

    fn people() -> Recipients {
        Recipients {
            to: vec![Person {
                name: "Person".into(),
                address: "a@example.com".into(),
            }],
            cc: vec![],
        }
    }
    #[test]
    fn providers_use_documented_urls_and_preserve_reply_headers() {
        let raw = "From: me@example.com\r\nTo: a@example.com\r\nSubject: Re: Hola\r\nIn-Reply-To: <original@example.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHola";
        for transport in [
            Transport::Resend,
            Transport::Cloudflare {
                account_id: "a".repeat(32),
            },
        ] {
            let (url, payload) = request(&transport, "me@example.com", &people(), raw).unwrap();
            assert!(url.starts_with("https://api."));
            assert_eq!(payload["text"], "Hola");
            assert_eq!(payload["to"], json!(["a@example.com"]));
            assert_eq!(payload["headers"]["In-Reply-To"], "<original@example.com>");
        }
    }
    #[test]
    fn validates_before_network_and_never_accepts_partial_cloudflare_delivery() {
        assert!(validate_token("secret\r\nHeader: injected").is_err());
        assert!(request(&Transport::Resend, "bad\r\nfrom@example.com", &people(), "").is_err());
        assert!(
            request(
                &Transport::Cloudflare {
                    account_id: "../bad".into()
                },
                "me@example.com",
                &people(),
                "Subject: Test\r\n\r\nBody"
            )
            .is_err()
        );
        assert!(
            accepted(
                &Transport::Resend,
                &json!({"id":"accepted"}),
                &["a@example.com"]
            )
            .is_ok()
        );
        assert!(accepted(&Transport::Cloudflare { account_id: "a".repeat(32) }, &json!({
            "success":true,"result":{"delivered":["a@example.com"],"queued":[],"permanent_bounces":["b@example.com"]}
        }), &["a@example.com", "b@example.com"]).is_err());
        assert!(accepted(&Transport::Cloudflare { account_id: "a".repeat(32) }, &json!({
            "success":true,"result":{"delivered":["a@example.com"],"queued":[],"permanent_bounces":[]}
        }), &["a@example.com","missing@example.com"]).is_err());
    }

    #[test]
    fn real_http_path_posts_json_and_authorization_without_retry_or_redirect() {
        use std::{
            io::{BufRead, BufReader, Write},
            net::TcpListener,
            thread,
        };
        for status in ["200 OK", "302 Found"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/emails", listener.local_addr().unwrap());
            let handle = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line);
                }
                let size: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .and_then(|s| s.parse().ok())
                    })
                    .unwrap();
                let mut body = vec![0; size];
                reader.read_exact(&mut body).unwrap();
                let response = r#"{"id":"test-accepted"}"#;
                write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:1/must-not-be-followed\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
                (headers, serde_json::from_slice::<Value>(&body).unwrap())
            });
            let result = deliver(
                &url,
                &Transport::Resend,
                "test-token",
                &json!({"text":"approved reply"}),
                &["a@example.com"],
            );
            let (headers, body) = handle.join().unwrap();
            assert!(headers.starts_with("POST /emails "));
            assert!(
                headers
                    .to_lowercase()
                    .contains("authorization: bearer test-token")
            );
            assert_eq!(body["text"], "approved reply");
            // 3xx must never be mistaken for accepted delivery, even with an ID.
            if status.starts_with("200") {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err());
            }
        }
    }
}
