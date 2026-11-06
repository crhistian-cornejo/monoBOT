//! A small SMTP client over TLS, for one thing: the reply the owner sends with their own press. It never talks in the clear
//! after the greeting: a server that offers no TLS gets no password and no message. It logs in with the same app password as
//! IMAP and has no way to choose a recipient but the ones it is given.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use crate::account::Account;
use crate::imap::Wire;

const MAX_LINE: usize = 8 * 1024;

/// Where an account sends from, and whether TLS starts with the connection (465) or after `STARTTLS` (587).
pub(crate) fn server_of(account: &Account) -> (String, u16, bool) {
    match account.provider.as_str() {
        "gmail" => ("smtp.gmail.com".into(), 465, true),
        "icloud" => ("smtp.mail.me.com".into(), 587, false),
        _ if account.host.ends_with("office365.com") || account.host.ends_with("outlook.com") => {
            ("smtp.office365.com".into(), 587, false)
        }
        _ => (
            account
                .host
                .strip_prefix("imap.")
                .map_or_else(|| account.host.clone(), |rest| format!("smtp.{rest}")),
            587,
            false,
        ),
    }
}

/// A reply of the server: its code and its last line.
fn reply(stream: &mut impl BufRead) -> Result<(u16, String), String> {
    loop {
        let mut line = String::new();
        let read = stream
            .by_ref()
            .take(MAX_LINE as u64)
            .read_line(&mut line)
            .map_err(|_| "El servidor de correo dejó de responder.".to_string())?;
        if read == 0 {
            return Err("El servidor de correo cerró la conexión.".into());
        }
        let code = line
            .get(..3)
            .and_then(|c| c.parse::<u16>().ok())
            .ok_or("El servidor de correo respondió algo que no se entiende.")?;
        // «250-…» says more lines follow; «250 …» is the last.
        if line.as_bytes().get(3) != Some(&b'-') {
            return Ok((
                code,
                line.get(4..)
                    .unwrap_or_default()
                    .trim()
                    .chars()
                    .take(160)
                    .collect(),
            ));
        }
    }
}

/// Sends a command and expects a code of the `want` hundreds.
fn command(
    stream: &mut BufReader<impl Read + Write>,
    line: &str,
    want: u16,
) -> Result<(u16, String), String> {
    stream
        .get_mut()
        .write_all(format!("{line}\r\n").as_bytes())
        .and_then(|()| stream.get_mut().flush())
        .map_err(|_| "Se cortó la conexión con el servidor de correo.".to_string())?;
    let (code, said) = reply(stream)?;
    if code / 100 != want {
        return Err(if code == 535 || code == 534 {
            "El servidor rechazó la contraseña de aplicación: revísala en «Cuentas».".to_string()
        } else {
            format!("El servidor de correo no lo aceptó ({code}: {said}).")
        });
    }
    Ok((code, said))
}

/// A message as the `DATA` command carries it: lines ending in CRLF, a line that starts with a dot doubled, and the end mark.
fn dot_stuffed(message: &str) -> String {
    let mut out = String::with_capacity(message.len() + 8);
    for line in message.replace("\r\n", "\n").split('\n') {
        if line.starts_with('.') {
            out.push('.');
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(".\r\n");
    out
}

/// One address alone: what goes on `MAIL FROM` and `RCPT TO` cannot carry another command.
fn plain_address(address: &str) -> Result<&str, String> {
    let valid = address.contains('@')
        && !address
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | ',' | ';'));
    valid
        .then_some(address)
        .ok_or_else(|| "Ese correo no tiene una dirección a la que responder.".to_string())
}

/// The most people one reply goes to: a reply to all of a long list is still a reply, not a mailing.
pub(crate) const MAX_RECIPIENTS: usize = 50;

/// From the login to the end, over a stream already secured (its greeting read). The password goes nowhere else. Every
/// address in `to` is checked before anything is sent: one bad one and none of them gets the message.
pub(crate) fn deliver(
    stream: impl Read + Write,
    login: &str,
    password: &str,
    to: &[String],
    message: &str,
) -> Result<(), String> {
    use base64::Engine;
    if to.is_empty() || to.len() > MAX_RECIPIENTS {
        return Err("Ese correo no tiene una dirección a la que responder.".into());
    }
    let to = to
        .iter()
        .map(|address| plain_address(address))
        .collect::<Result<Vec<_>, _>>()?;
    let from = plain_address(login)?;
    let mut stream = BufReader::new(stream);
    command(&mut stream, "EHLO echo.local", 2)?;
    let token = base64::engine::general_purpose::STANDARD.encode(format!("\0{login}\0{password}"));
    command(&mut stream, &format!("AUTH PLAIN {token}"), 2)?;
    command(&mut stream, &format!("MAIL FROM:<{from}>"), 2)?;
    for to in to {
        command(&mut stream, &format!("RCPT TO:<{to}>"), 2)?;
    }
    command(&mut stream, "DATA", 3)?;
    stream
        .get_mut()
        .write_all(dot_stuffed(message).as_bytes())
        .map_err(|_| "Se cortó la conexión con el servidor de correo.".to_string())?;
    stream
        .get_mut()
        .flush()
        .map_err(|_| "Se cortó la conexión con el servidor de correo.".to_string())?;
    let (code, said) = reply(&mut stream)?;
    if code / 100 != 2 {
        return Err(format!(
            "El servidor de correo no lo aceptó ({code}: {said})."
        ));
    }
    let _ = stream.get_mut().write_all(b"QUIT\r\n");
    Ok(())
}

/// The plain part of a `STARTTLS` connection: the greeting, whether the server offers TLS, and its go-ahead. Nothing of the
/// owner's is sent before TLS: a server that does not offer it is left there.
fn start_tls(stream: &mut BufReader<impl Read + Write>) -> Result<(), String> {
    reply(stream)?;
    stream
        .get_mut()
        .write_all(b"EHLO echo.local\r\n")
        .map_err(|_| "Se cortó la conexión con el servidor de correo.".to_string())?;
    let mut offered = false;
    loop {
        let mut line = String::new();
        if stream
            .by_ref()
            .take(MAX_LINE as u64)
            .read_line(&mut line)
            .map_err(|_| "El servidor de correo dejó de responder.".to_string())?
            == 0
        {
            return Err("El servidor de correo cerró la conexión.".into());
        }
        offered |= line.to_ascii_uppercase().contains("STARTTLS");
        if line.as_bytes().get(3) != Some(&b'-') {
            break;
        }
    }
    if !offered {
        return Err(
            "Ese servidor de correo no ofrece una conexión cifrada: no se envió nada.".into(),
        );
    }
    command(stream, "STARTTLS", 2).map(|_| ())
}

/// Connects to the account's server, secures the connection and delivers the message.
pub(crate) fn send(
    account: &Account,
    password: &str,
    to: &[String],
    message: &str,
) -> Result<(), String> {
    let (host, port, implicit) = server_of(account);
    let offline = || "No hay conexión con el servidor de envío. Revisa tu internet.".to_string();
    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| offline())?
        .next()
        .ok_or_else(offline)?;
    let tcp =
        TcpStream::connect_timeout(&address, Duration::from_secs(20)).map_err(|_| offline())?;
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(60)));
    let tcp = if implicit {
        tcp
    } else {
        let mut plain = BufReader::new(tcp);
        start_tls(&mut plain)?;
        plain.into_inner()
    };
    let config = crate::base::tls::client()?;
    let name = rustls::pki_types::ServerName::try_from(host).map_err(|e| e.to_string())?;
    let tls =
        rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| format!("TLS: {e}"))?;
    let mut secure: Box<dyn Wire> = Box::new(rustls::StreamOwned::new(tls, tcp));
    if implicit {
        let mut greeted = BufReader::new(&mut secure);
        reply(&mut greeted)?;
    }
    deliver(secure, &account.address, password, to, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// A server that answers each line it reads with the next answer, and gives back what it heard.
    fn server(answers: Vec<&'static str>) -> (TcpStream, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut writer = socket;
            let mut heard = Vec::new();
            let mut data = false;
            let mut answers = answers.into_iter();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return heard;
                }
                heard.push(line.trim_end().to_string());
                if data && line.trim_end() != "." {
                    continue;
                }
                data = line.trim_end() == "DATA";
                let Some(answer) = answers.next() else {
                    return heard;
                };
                writer.write_all(answer.as_bytes()).unwrap();
            }
        });
        (TcpStream::connect(("127.0.0.1", port)).unwrap(), handle)
    }

    fn to(addresses: &[&str]) -> Vec<String> {
        addresses.iter().map(|a| a.to_string()).collect()
    }

    fn account(provider: &str, host: &str) -> Account {
        Account {
            id: "a".into(),
            name: "Ana".into(),
            address: "ana@ejemplo.test".into(),
            provider: provider.into(),
            host: host.into(),
            port: 993,
        }
    }

    #[test]
    fn delivers_to_the_one_recipient_with_the_message_stuffed() {
        let (stream, heard) = server(vec![
            "250-hola\r\n250 AUTH PLAIN\r\n",
            "235 ok\r\n",
            "250 ok\r\n",
            "250 ok\r\n",
            "354 sigue\r\n",
            "250 aceptado\r\n",
            "221 adiós\r\n",
        ]);
        deliver(
            stream,
            "ana@ejemplo.test",
            "clave",
            &to(&["lucia@ejemplo.test"]),
            "Subject: Hola\r\n\r\nUno\r\n.punto\r\n",
        )
        .unwrap();
        let heard = heard.join().unwrap();
        assert!(heard.contains(&"MAIL FROM:<ana@ejemplo.test>".to_string()));
        assert_eq!(
            heard
                .iter()
                .filter(|l| l.starts_with("RCPT TO:"))
                .collect::<Vec<_>>(),
            [&"RCPT TO:<lucia@ejemplo.test>".to_string()]
        );
        assert!(
            heard.contains(&"..punto".to_string()),
            "a line that starts with a dot is doubled"
        );
        assert!(
            !heard.iter().any(|l| l.contains("clave")),
            "the password travels only inside the login's token"
        );
    }

    #[test]
    fn says_when_the_password_is_refused_and_sends_no_message() {
        let (stream, heard) = server(vec!["250 hola\r\n", "535 no\r\n"]);
        let said = deliver(
            stream,
            "ana@ejemplo.test",
            "clave",
            &to(&["lucia@ejemplo.test"]),
            "x",
        )
        .unwrap_err();
        assert!(said.contains("contraseña de aplicación"));
        assert!(
            !heard
                .join()
                .unwrap()
                .iter()
                .any(|l| l.starts_with("MAIL FROM"))
        );
    }

    #[test]
    fn says_when_the_server_refuses_the_recipient() {
        let (stream, _heard) = server(vec![
            "250 hola\r\n",
            "235 ok\r\n",
            "250 ok\r\n",
            "550 no existe\r\n",
        ]);
        assert!(
            deliver(
                stream,
                "ana@ejemplo.test",
                "clave",
                &to(&["x@ejemplo.test"]),
                "x"
            )
            .unwrap_err()
            .contains("550")
        );
    }

    #[test]
    fn an_address_cannot_carry_another_command() {
        let (stream, _heard) = server(vec![]);
        assert!(
            deliver(
                stream,
                "ana@ejemplo.test",
                "clave",
                &to(&["a@b.test>\r\nRCPT TO:<c@d.test"]),
                "x"
            )
            .is_err()
        );
    }

    #[test]
    fn delivers_to_each_recipient_and_to_none_if_one_is_bad() {
        let (stream, heard) = server(vec![
            "250 hola\r\n",
            "235 ok\r\n",
            "250 ok\r\n",
            "250 ok\r\n",
            "250 ok\r\n",
            "354 sigue\r\n",
            "250 aceptado\r\n",
            "221 adiós\r\n",
        ]);
        deliver(
            stream,
            "ana@ejemplo.test",
            "clave",
            &to(&["lucia@ejemplo.test", "luis@ejemplo.test"]),
            "x",
        )
        .unwrap();
        assert_eq!(
            heard
                .join()
                .unwrap()
                .iter()
                .filter(|l| l.starts_with("RCPT TO:"))
                .count(),
            2
        );
        let (stream, heard) = server(vec![]);
        assert!(
            deliver(
                stream,
                "ana@ejemplo.test",
                "clave",
                &to(&["lucia@ejemplo.test", "a b@c.test"]),
                "x"
            )
            .is_err()
        );
        drop(heard);
        let (stream, _heard) = server(vec![]);
        assert!(deliver(stream, "ana@ejemplo.test", "clave", &[], "x").is_err());
    }

    #[test]
    fn a_server_with_no_tls_gets_nothing_of_the_owners() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.write_all(b"220 hola\r\n").unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            socket.write_all(b"250-hola\r\n250 AUTH PLAIN\r\n").unwrap();
            let mut rest = String::new();
            let _ = reader.read_line(&mut rest);
            rest
        });
        let mut stream = BufReader::new(TcpStream::connect(("127.0.0.1", port)).unwrap());
        let said = start_tls(&mut stream).unwrap_err();
        assert!(said.contains("no se envió nada"));
        drop(stream);
        assert_eq!(
            handle.join().unwrap(),
            "",
            "after the refusal not one more line goes out"
        );
    }

    #[test]
    fn knows_where_each_account_sends_from() {
        assert_eq!(
            server_of(&account("gmail", "imap.gmail.com")),
            ("smtp.gmail.com".into(), 465, true)
        );
        assert_eq!(
            server_of(&account("icloud", "imap.mail.me.com")),
            ("smtp.mail.me.com".into(), 587, false)
        );
        assert_eq!(
            server_of(&account("imap", "imap.ejemplo.test")),
            ("smtp.ejemplo.test".into(), 587, false)
        );
        assert_eq!(
            server_of(&account("imap", "outlook.office365.com")),
            ("smtp.office365.com".into(), 587, false)
        );
    }
}
