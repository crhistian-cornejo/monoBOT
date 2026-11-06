//! A small IMAP client over TLS, written for what Echo's mail does and nothing else: log in with an app password,
//! find the account's folders, read the newest messages without marking them (`BODY.PEEK`), set a flag, move a
//! message, save a draft and keep a copy of what the owner sent. A file of a message is read only when the owner opens it (inside the whole message, `fetch_message`), no links are followed, nothing is deleted.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::mime::TEXT_BYTES;

/// The biggest piece of a response that is read; more is refused. A whole message may be bigger (its pictures travel
/// inside it), up to `MAX_MESSAGE`, and only when it is opened.
const MAX_LITERAL: usize = 256 * 1024;
pub const MAX_MESSAGE: usize = 25 * 1024 * 1024;
const MAX_LINE: usize = 64 * 1024;
const WAIT: Duration = Duration::from_secs(60);
/// How often an `IDLE` wakes to see whether it was told to stop.
#[cfg(not(test))]
const IDLE_POLL: Duration = Duration::from_secs(30);
#[cfg(test)]
const IDLE_POLL: Duration = Duration::from_millis(100);

/// What the connection reads and writes: TLS over a socket, or in a test a plain one.
pub trait Wire: Read + Write + Send {}
impl<T: Read + Write + Send> Wire for T {}

/// One response line: its text pieces and the literals (`{n}` + n bytes) between them.
#[derive(Debug, Default)]
pub struct Line {
    pub pieces: Vec<String>,
    pub literals: Vec<Vec<u8>>,
}

impl Line {
    pub fn text(&self) -> String {
        self.pieces.join(" ")
    }
}

/// A message as the server gave it, before it is read.
#[derive(Debug, Default, Clone)]
pub struct Raw {
    pub uid: u64,
    pub flags: Vec<String>,
    /// INTERNALDATE, in milliseconds since the epoch.
    pub date: Option<i64>,
    /// Gmail's labels (`X-GM-LABELS`), the user's own ones.
    pub labels: Vec<String>,
    /// Gmail's message and thread ids (`X-GM-MSGID`, `X-GM-THRID`), when a search asked for them.
    pub gmail_id: Option<u64>,
    pub gmail_thread: Option<u64>,
    pub header: Vec<u8>,
    pub text: Vec<u8>,
}

/// A mailbox of the account and what the server says it is for (`\Sent`, `\Drafts`, `\Archive`, `\All`…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailbox {
    pub flags: Vec<String>,
    pub name: String,
}

pub struct Imap {
    stream: BufReader<Box<dyn Wire>>,
    tag: u32,
    /// Bytes of a line whose end has not arrived yet (a read may time out in the middle).
    pending: Vec<u8>,
    /// The biggest literal the next answer may carry: `MAX_LITERAL`, or `MAX_MESSAGE` while a whole message is read.
    literal_limit: usize,
}

impl Imap {
    /// A TLS connection to `host:port` (certificates checked as the rest of Echo does).
    pub fn open(host: &str, port: u16) -> Result<Self, String> {
        let offline =
            || "No hay conexión con el servidor de correo. Revisa tu internet.".to_string();
        let address = (host, port)
            .to_socket_addrs()
            .map_err(|_| offline())?
            .next()
            .ok_or_else(offline)?;
        let tcp =
            TcpStream::connect_timeout(&address, Duration::from_secs(20)).map_err(|_| offline())?;
        let _ = tcp.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = tcp.set_write_timeout(Some(Duration::from_secs(30)));
        let config = crate::base::tls::client()?;
        let name =
            rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
        let tls = rustls::ClientConnection::new(Arc::new(config), name)
            .map_err(|e| format!("TLS: {e}"))?;
        Self::over(Box::new(rustls::StreamOwned::new(tls, tcp)))
    }

    /// A connection over any stream, after reading the server's greeting.
    pub fn over(stream: Box<dyn Wire>) -> Result<Self, String> {
        let mut imap = Self {
            stream: BufReader::new(stream),
            tag: 0,
            pending: Vec::new(),
            literal_limit: MAX_LITERAL,
        };
        let greeting = imap
            .read_line(Duration::from_secs(30))?
            .ok_or("El servidor de correo no respondió.")?;
        if !greeting.text().starts_with("* OK") && !greeting.text().starts_with("* PREAUTH") {
            return Err("El servidor de correo no aceptó la conexión.".into());
        }
        Ok(imap)
    }

    /// One raw line (without CRLF), or `None` when nothing came within `wait`.
    fn read_raw(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, String> {
        let deadline = Instant::now() + wait;
        loop {
            match self.stream.read_until(b'\n', &mut self.pending) {
                Ok(0) => return Err("El servidor de correo cerró la conexión.".into()),
                Ok(_) if self.pending.ends_with(b"\n") => {
                    let mut line = std::mem::take(&mut self.pending);
                    while matches!(line.last(), Some(b'\n' | b'\r')) {
                        line.pop();
                    }
                    return Ok(Some(line));
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if Instant::now() >= deadline {
                        return Ok(None);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err("Se cortó la conexión con el servidor de correo.".into()),
            }
            if self.pending.len() > MAX_LINE {
                return Err("El servidor de correo envió una respuesta demasiado larga.".into());
            }
        }
    }

    /// A whole response line with its literals, or `None` when nothing came within `wait`.
    pub fn read_line(&mut self, wait: Duration) -> Result<Option<Line>, String> {
        let mut line = Line::default();
        let mut wait = wait;
        loop {
            let Some(raw) = self.read_raw(wait)? else {
                return if line.pieces.is_empty() {
                    Ok(None)
                } else {
                    Err("El servidor de correo dejó una respuesta a medias.".into())
                };
            };
            // Inside a line the rest must follow: give it time.
            wait = WAIT;
            let text = String::from_utf8_lossy(&raw).into_owned();
            let size = text
                .strip_suffix('}')
                .and_then(|t| t.rsplit_once('{'))
                .and_then(|(_, n)| n.parse::<usize>().ok());
            line.pieces.push(text);
            let Some(size) = size else {
                return Ok(Some(line));
            };
            if size > self.literal_limit {
                return Err("El servidor de correo envió un bloque demasiado grande.".into());
            }
            let mut literal = vec![0u8; size];
            let mut filled = 0;
            let deadline = Instant::now() + WAIT;
            while filled < size {
                match Read::read(&mut self.stream, &mut literal[filled..]) {
                    Ok(0) => return Err("El servidor de correo cerró la conexión.".into()),
                    Ok(n) => filled += n,
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        if Instant::now() >= deadline {
                            return Err(
                                "El servidor de correo tardó demasiado en responder.".into()
                            );
                        }
                    }
                    Err(_) => return Err("Se cortó la conexión con el servidor de correo.".into()),
                }
            }
            line.literals.push(literal);
        }
    }

    fn send(&mut self, text: &str) -> Result<(), String> {
        let stream = self.stream.get_mut();
        stream
            .write_all(text.as_bytes())
            .and_then(|_| stream.flush())
            .map_err(|_| "Se cortó la conexión con el servidor de correo.".to_string())
    }

    /// Sends a command and returns the untagged lines of its answer; `Err` with the server's reason on NO / BAD.
    pub fn command(&mut self, command: &str) -> Result<Vec<Line>, String> {
        self.tag += 1;
        let tag = format!("E{}", self.tag);
        self.send(&format!("{tag} {command}\r\n"))?;
        self.answer(&tag)
    }

    /// The same, with `literal` sent last as an IMAP literal (`{n}`, then its bytes once the server says «+»).
    pub fn command_with(&mut self, command: &str, literal: &str) -> Result<Vec<Line>, String> {
        self.tag += 1;
        let tag = format!("E{}", self.tag);
        self.send(&format!("{tag} {command} {{{}}}\r\n", literal.len()))?;
        let go = self
            .read_line(WAIT)?
            .ok_or("El servidor de correo tardó demasiado en responder.")?;
        if !go.text().starts_with('+') {
            return Err(go.text().chars().take(160).collect());
        }
        self.send(&format!("{literal}\r\n"))?;
        self.answer(&tag)
    }

    /// The untagged lines of an answer, until `tag`'s own; `Err` with the server's reason on NO / BAD.
    fn answer(&mut self, tag: &str) -> Result<Vec<Line>, String> {
        let mut lines = Vec::new();
        loop {
            let line = self
                .read_line(WAIT)?
                .ok_or("El servidor de correo tardó demasiado en responder.")?;
            let text = line.text();
            if let Some(rest) = text.strip_prefix(&format!("{tag} ")) {
                return if rest.starts_with("OK") {
                    Ok(lines)
                } else {
                    Err(rest.chars().take(160).collect())
                };
            }
            lines.push(line);
        }
    }

    pub fn login(&mut self, address: &str, password: &str, provider: &str) -> Result<(), String> {
        if address
            .chars()
            .chain(password.chars())
            .any(|c| c.is_control())
        {
            return Err(
                "La dirección o la contraseña tienen caracteres que no se pueden usar.".into(),
            );
        }
        self.command(&format!("LOGIN {} {}", quote(address), quote(password))).map(|_| ()).map_err(|reason| {
            let reason = reason.to_lowercase();
            let app = match provider {
                "gmail" => "Usa una contraseña de aplicación (Cuenta de Google › Seguridad › Verificación en dos pasos › Contraseñas de aplicaciones).",
                "icloud" => "Usa una contraseña específica de app (appleid.apple.com › Inicio de sesión y seguridad › Contraseñas específicas de app).",
                _ => "Revisa la dirección y la contraseña; algunas cuentas piden una contraseña de aplicación.",
            };
            if reason.contains("imap") && reason.contains("disabled") {
                "IMAP está desactivado en esta cuenta (en su configuración, o lo bloquea el administrador de la empresa).".to_string()
            } else {
                format!("El servidor no aceptó esa contraseña. {app}")
            }
        })
    }

    /// The server's extensions, in capitals (`MOVE`, `UIDPLUS`…).
    pub fn capabilities(&mut self) -> Vec<String> {
        let Ok(lines) = self.command("CAPABILITY") else {
            return Vec::new();
        };
        lines
            .iter()
            .map(Line::text)
            .filter_map(|t| {
                t.strip_prefix("* CAPABILITY ").map(|rest| {
                    rest.split_whitespace()
                        .map(str::to_ascii_uppercase)
                        .collect::<Vec<_>>()
                })
            })
            .flatten()
            .collect()
    }

    /// Every mailbox of the account with what it is for.
    pub fn mailboxes(&mut self) -> Result<Vec<Mailbox>, String> {
        let lines = self.command("LIST \"\" \"*\"")?;
        Ok(lines.iter().filter_map(|l| parse_list(&l.text())).collect())
    }

    /// Opens a mailbox (read-only unless `write`) and says how many messages it holds.
    pub fn select(&mut self, mailbox: &str, write: bool) -> Result<u32, String> {
        let lines = self.command(&format!(
            "{} {}",
            if write { "SELECT" } else { "EXAMINE" },
            quote(mailbox)
        ))?;
        for line in &lines {
            let text = line.text();
            if let Some(rest) = text.strip_prefix("* ") {
                if let Some((count, word)) = rest.split_once(' ') {
                    if word.trim() == "EXISTS" {
                        return Ok(count.parse().unwrap_or(0));
                    }
                }
            }
        }
        Ok(0)
    }

    /// What a list reading asks of each message: flags, date, the headers the rules read and the first bytes of the text.
    fn list_items(gmail: bool) -> String {
        let labels = if gmail { " X-GM-LABELS" } else { "" };
        format!(
            "(UID FLAGS INTERNALDATE{labels} BODY.PEEK[HEADER.FIELDS (FROM TO CC REPLY-TO SUBJECT MESSAGE-ID CONTENT-TYPE CONTENT-TRANSFER-ENCODING LIST-UNSUBSCRIBE PRECEDENCE AUTO-SUBMITTED AUTHENTICATION-RESULTS)] BODY.PEEK[TEXT]<0.{TEXT_BYTES}>)"
        )
    }

    /// The newest `count` messages of the open mailbox, with no message marked as read.
    pub fn fetch_recent(
        &mut self,
        exists: u32,
        count: u32,
        gmail: bool,
    ) -> Result<Vec<Raw>, String> {
        if exists == 0 || count == 0 {
            return Ok(Vec::new());
        }
        let first = exists.saturating_sub(count - 1).max(1);
        let lines = self.command(&format!(
            "FETCH {first}:{exists} {}",
            Self::list_items(gmail)
        ))?;
        Ok(raws_of(lines))
    }

    /// The messages of the open mailbox from `uid` on (what arrived since it was the next one), read as a list reading
    /// does. A server answers `UID n:*` with the last message even when it is older than `n`: that one is left out.
    pub fn fetch_from(&mut self, uid: u64, gmail: bool) -> Result<Vec<Raw>, String> {
        let lines = self.command(&format!("UID FETCH {uid}:* {}", Self::list_items(gmail)))?;
        Ok(raws_of(lines)
            .into_iter()
            .filter(|raw| raw.uid >= uid)
            .collect())
    }

    /// The UIDs of the open mailbox that `criteria` finds (`UID SEARCH`), lowest first. With `gmail_raw`, the words of
    /// Gmail's own search box go as one literal (`X-GM-RAW`: they may hold quotes and accents); else `criteria` is the
    /// IMAP search as it is (`search::Query::criteria`, ASCII only).
    pub fn search(&mut self, criteria: &str, gmail_raw: Option<&str>) -> Result<Vec<u64>, String> {
        let lines = match gmail_raw {
            Some(raw) => self.command_with("UID SEARCH CHARSET UTF-8 X-GM-RAW", raw)?,
            None => self.command(&format!("UID SEARCH {criteria}"))?,
        };
        let mut uids: Vec<u64> = lines
            .iter()
            .map(Line::text)
            .filter_map(|l| l.strip_prefix("* SEARCH").map(str::to_string))
            .flat_map(|rest| {
                rest.split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect::<Vec<u64>>()
            })
            .collect();
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }

    /// The messages of the open mailbox with these UIDs, read as a list reading does, with Gmail's ids when `gmail`.
    pub fn fetch_uids(&mut self, uids: &[u64], gmail: bool) -> Result<Vec<Raw>, String> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set: Vec<String> = uids.iter().map(u64::to_string).collect();
        let ids = if gmail { " X-GM-MSGID X-GM-THRID" } else { "" };
        let lines = self.command(&format!(
            "UID FETCH {} (UID INTERNALDATE{ids} BODY.PEEK[HEADER.FIELDS (FROM SUBJECT MESSAGE-ID CONTENT-TYPE CONTENT-TRANSFER-ENCODING)] BODY.PEEK[TEXT]<0.{TEXT_BYTES}>)",
            set.join(",")
        ))?;
        Ok(raws_of(lines)
            .into_iter()
            .filter(|raw| uids.contains(&raw.uid))
            .collect())
    }

    /// The messages of the open mailbox with these UIDs, as `fetch_recent` reads its own: for a list of messages.
    pub fn fetch_listed(&mut self, uids: &[u64], gmail: bool) -> Result<Vec<Raw>, String> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set: Vec<String> = uids.iter().map(u64::to_string).collect();
        let lines = self.command(&format!(
            "UID FETCH {} {}",
            set.join(","),
            Self::list_items(gmail)
        ))?;
        Ok(raws_of(lines)
            .into_iter()
            .filter(|raw| uids.contains(&raw.uid))
            .collect())
    }

    /// The whole message as the server keeps it (`BODY.PEEK[]`: nothing is marked as read). Its literal may be as big
    /// as `MAX_MESSAGE`.
    pub fn fetch_message(&mut self, uid: u64) -> Result<Vec<u8>, String> {
        self.literal_limit = MAX_MESSAGE;
        let answer = self.command(&format!("UID FETCH {uid} (BODY.PEEK[])"));
        self.literal_limit = MAX_LITERAL;
        for line in answer? {
            for (piece, literal) in line.pieces.iter().zip(&line.literals) {
                if piece.contains("BODY[]") {
                    return Ok(literal.clone());
                }
            }
        }
        Err("El servidor de correo no devolvió ese correo.".into())
    }

    /// Adds or removes one flag (`\Seen`, `\Flagged`) of a message of the open mailbox.
    pub fn flag(&mut self, uid: u64, flag: &str, on: bool) -> Result<(), String> {
        self.command(&format!(
            "UID STORE {uid} {}FLAGS.SILENT ({flag})",
            if on { "+" } else { "-" }
        ))
        .map(|_| ())
    }

    /// Moves a message of the open mailbox to `to`. With no `MOVE`, it is copied and only that message is marked
    /// and expunged (`UIDPLUS`); with neither, nothing is done rather than risk removing others.
    pub fn move_to(&mut self, uid: u64, to: &str) -> Result<(), String> {
        let capabilities = self.capabilities();
        if capabilities.iter().any(|c| c == "MOVE") {
            return self
                .command(&format!("UID MOVE {uid} {}", quote(to)))
                .map(|_| ());
        }
        if !capabilities.iter().any(|c| c == "UIDPLUS") {
            return Err("Este servidor no permite archivar con seguridad.".into());
        }
        self.command(&format!("UID COPY {uid} {}", quote(to)))?;
        self.flag(uid, "\\Deleted", true)?;
        self.command(&format!("UID EXPUNGE {uid}")).map(|_| ())
    }

    /// Saves a message in `mailbox` as a draft, read and not sent.
    pub fn save_draft(&mut self, mailbox: &str, message: &str) -> Result<(), String> {
        self.command_with(
            &format!("APPEND {} (\\Draft \\Seen)", quote(mailbox)),
            message,
        )
        .map(|_| ())
    }

    /// Keeps a copy of what was sent in the account's sent folder.
    pub fn save_sent(&mut self, mailbox: &str, message: &str) -> Result<(), String> {
        self.command_with(&format!("APPEND {} (\\Seen)", quote(mailbox)), message)
            .map(|_| ())
    }

    /// Ends the session without waiting for the server's goodbye (a reading does not need it, and it costs half a second).
    pub fn logout(&mut self) {
        self.tag += 1;
        let tag = format!("E{}", self.tag);
        let _ = self.send(&format!("{tag} LOGOUT\r\n"));
    }

    /// Opens a mailbox read-only and says how many messages it holds and the UID the next one will have (`None` when the
    /// server does not say: then no new message can be told from an old one).
    pub fn examine_next(&mut self, mailbox: &str) -> Result<(u32, Option<u64>), String> {
        let lines = self.command(&format!("EXAMINE {}", quote(mailbox)))?;
        let mut exists = 0;
        let mut next = None;
        for line in &lines {
            let text = line.text();
            if text.starts_with("* ") && text.trim_end().ends_with(" EXISTS") {
                exists = text[2..]
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
            }
            if let Some(n) = number_after(&text, "UIDNEXT ") {
                next = Some(n);
            }
        }
        Ok((exists, next))
    }

    /// Waits in `IDLE` until the server says something changed, `alive` says to stop, or `limit` passes. A server ends
    /// an `IDLE` after about half an hour, so the caller asks again before that.
    pub fn idle(&mut self, alive: impl Fn() -> bool, limit: Duration) -> Result<Idled, String> {
        self.tag += 1;
        let tag = format!("E{}", self.tag);
        self.send(&format!("{tag} IDLE\r\n"))?;
        let go = self
            .read_line(WAIT)?
            .ok_or("El servidor de correo tardó demasiado en responder.")?;
        if !go.text().starts_with('+') {
            return Err("El servidor de correo no admite la espera de correo nuevo.".into());
        }
        let started = Instant::now();
        let mut idled = Idled::default();
        let mut done = false;
        loop {
            match self.read_line(IDLE_POLL)? {
                Some(line) => {
                    let text = line.text();
                    if let Some(rest) = text.strip_prefix(&format!("{tag} ")) {
                        return if rest.starts_with("OK") {
                            Ok(idled)
                        } else {
                            Err(rest.chars().take(160).collect())
                        };
                    }
                    if text.ends_with(" EXISTS") {
                        idled.arrived = true;
                    } else if text.ends_with(" EXPUNGE") || text.contains(" FETCH ") {
                        idled.changed = true;
                    }
                    if (idled.arrived || idled.changed) && !done {
                        self.send("DONE\r\n")?;
                        done = true;
                    }
                }
                None if done => {
                    return Err("El servidor de correo tardó demasiado en responder.".into());
                }
                None => {}
            }
            if !done && (!alive() || started.elapsed() > limit) {
                self.send("DONE\r\n")?;
                done = true;
            }
        }
    }
}

/// What an `IDLE` heard: new messages (`EXISTS`), or others gone or with new flags.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Idled {
    pub arrived: bool,
    pub changed: bool,
}

/// The messages of a `FETCH` answer, oldest first.
fn raws_of(lines: Vec<Line>) -> Vec<Raw> {
    let mut messages = Vec::new();
    for line in lines {
        let text = line.text();
        if !text.contains(" FETCH ") {
            continue;
        }
        let Some(uid) = number_after(&text, "UID ") else {
            continue;
        };
        let mut raw = Raw {
            uid,
            flags: parenthesized(&text, "FLAGS (")
                .map(|f| f.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
            ..Raw::default()
        };
        raw.date = quoted_after(&text, "INTERNALDATE \"").and_then(|d| parse_date(&d));
        raw.labels = labels_of(&text);
        raw.gmail_id = number_after(&text, "X-GM-MSGID ");
        raw.gmail_thread = number_after(&text, "X-GM-THRID ");
        // Each literal follows the piece that names it: «… BODY[HEADER.FIELDS (…)] {n}» or «… BODY[TEXT]<0> {n}».
        for (piece, literal) in line.pieces.iter().zip(&line.literals) {
            if piece.contains("BODY[TEXT]") {
                raw.text = literal.clone();
            } else {
                raw.header = literal.clone();
            }
        }
        messages.push(raw);
    }
    messages.sort_by_key(|m| m.uid);
    messages
}

/// A string as an IMAP quoted string. Control characters never get here (`login` and the accounts refuse them).
pub fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// «* LIST (\HasNoChildren \Sent) "/" "[Gmail]/Enviados"» → its flags and its name.
pub fn parse_list(text: &str) -> Option<Mailbox> {
    let rest = text.strip_prefix("* LIST (")?;
    let (flags, rest) = rest.split_once(") ")?;
    let rest = rest.trim_start();
    // The delimiter: a quoted character or NIL.
    let rest = if let Some(after) = rest.strip_prefix('"') {
        after.split_once('"')?.1
    } else {
        rest.split_once(' ')?.1
    };
    let name = rest.trim();
    let name = match name.strip_prefix('"').and_then(|n| n.strip_suffix('"')) {
        Some(quoted) => quoted.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => name.to_string(),
    };
    if name.is_empty() {
        return None;
    }
    Some(Mailbox {
        flags: flags.split_whitespace().map(str::to_string).collect(),
        name,
    })
}

fn number_after(text: &str, marker: &str) -> Option<u64> {
    let at = text.find(marker)? + marker.len();
    let digits: String = text[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The text between `marker` (which ends with an opening parenthesis) and its closing one.
fn parenthesized<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let at = text.find(marker)? + marker.len();
    let end = text[at..].find(')')?;
    Some(&text[at..at + end])
}

fn quoted_after(text: &str, marker: &str) -> Option<String> {
    let at = text.find(marker)? + marker.len();
    let end = text[at..].find('"')?;
    Some(text[at..at + end].to_string())
}

/// Gmail's labels of a message, the user's own ones: `X-GM-LABELS ("\\Inbox" "Trabajo" Reembolsos)`.
fn labels_of(text: &str) -> Vec<String> {
    let Some(at) = text.find("X-GM-LABELS (") else {
        return Vec::new();
    };
    let rest = &text[at + "X-GM-LABELS (".len()..];
    let (mut labels, mut current, mut quoted, mut escaped) =
        (Vec::new(), String::new(), false, false);
    for c in rest.chars() {
        if escaped {
            current.push(c);
            escaped = false;
        } else if quoted && c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if !quoted && (c == ' ' || c == ')') {
            if !current.is_empty() {
                labels.push(std::mem::take(&mut current));
            }
            if c == ')' {
                break;
            }
        } else {
            current.push(c);
        }
    }
    // «\Inbox», «\Important» and Gmail's categories are the system's, not the user's.
    labels.retain(|l| !l.starts_with('\\') && !l.starts_with("CATEGORY_"));
    labels.truncate(4);
    labels
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// «17-Oct-2026 12:00:00 +0200» → milliseconds since the epoch.
pub fn parse_date(text: &str) -> Option<i64> {
    let mut parts = text.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    let zone = parts.next().unwrap_or("+0000");
    let mut d = date.split('-');
    let day: i64 = d.next()?.parse().ok()?;
    let name = d.next()?;
    let year: i64 = d.next()?.parse().ok()?;
    let month = MONTHS.iter().position(|m| name.eq_ignore_ascii_case(m))? as i64 + 1;
    let mut t = time.split(':');
    let (h, m, s): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let zone = zone.trim_start_matches(['+', '-']);
    let offset = sign
        * (zone.get(..2)?.parse::<i64>().ok()? * 3600 + zone.get(2..4)?.parse::<i64>().ok()? * 60);
    Some((days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s - offset) * 1000)
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{client, server};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn a_list_line_gives_its_flags_and_name() {
        let gmail =
            parse_list("* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Enviados\"").unwrap();
        assert_eq!(
            (gmail.flags.as_slice(), gmail.name.as_str()),
            (
                &["\\HasNoChildren".to_string(), "\\Sent".to_string()][..],
                "[Gmail]/Enviados"
            )
        );
        assert_eq!(parse_list("* LIST () NIL INBOX").unwrap().name, "INBOX");
        assert_eq!(
            parse_list("* LIST (\\All) \".\" \"A \\\"x\\\"\"")
                .unwrap()
                .name,
            "A \"x\""
        );
        assert!(parse_list("* OK nothing").is_none());
    }

    #[test]
    fn a_whole_message_comes_in_one_literal_bigger_than_a_list_line_and_nothing_is_marked() {
        let raw = format!("Subject: Hola\r\n\r\n{}", "x".repeat(300_000));
        let mut answer = format!("* 1 FETCH (UID 41 BODY[] {{{}}}\r\n", raw.len()).into_bytes();
        answer.extend_from_slice(raw.as_bytes());
        answer.extend_from_slice(b")\r\n");
        let (port, handle) = server(vec![("UID FETCH 41 (BODY.PEEK[])", answer)]);
        let mut imap = client(port);
        assert_eq!(imap.fetch_message(41).unwrap(), raw.as_bytes());
        assert!(handle.join().unwrap()[0].contains("BODY.PEEK[]"));
    }

    #[test]
    fn it_reads_dates_with_their_zone() {
        assert_eq!(parse_date("01-Jan-1970 00:00:00 +0000"), Some(0));
        assert_eq!(
            parse_date("17-Oct-2026 12:00:00 +0200"),
            Some(1_792_231_200_000)
        );
        assert_eq!(
            parse_date("17-Oct-2026 10:00:00 +0000"),
            Some(1_792_231_200_000)
        );
        assert_eq!(parse_date("nonsense"), None);
    }

    #[test]
    fn labels_keep_the_users_own_and_drop_the_systems() {
        assert_eq!(
            labels_of(
                "12 FETCH (X-GM-LABELS (\\\\Inbox \"Trabajo\" Reembolsos \"Con espacio\" CATEGORY_PROMOTIONS) UID 5"
            ),
            vec!["Trabajo", "Reembolsos", "Con espacio"]
        );
        assert!(labels_of("12 FETCH (UID 5)").is_empty());
    }

    #[test]
    fn a_quoted_string_cannot_close_early() {
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn it_reads_the_newest_messages_with_their_literals_and_flags() {
        let header = "From: Ana <ana@ejemplo.test>\r\nSubject: Hola\r\n\r\n";
        let text = "Hola, ¿cenamos?";
        let mut fetch = Vec::new();
        fetch.extend_from_slice(format!("* 2 FETCH (UID 41 FLAGS (\\Seen \\Flagged) INTERNALDATE \"17-Oct-2026 10:00:00 +0000\" X-GM-LABELS (\"Trabajo\") BODY[HEADER.FIELDS (FROM SUBJECT)] {{{}}}\r\n", header.len()).as_bytes());
        fetch.extend_from_slice(header.as_bytes());
        fetch.extend_from_slice(format!(" BODY[TEXT]<0> {{{}}}\r\n", text.len()).as_bytes());
        fetch.extend_from_slice(text.as_bytes());
        fetch.extend_from_slice(b")\r\n");
        let (port, handle) = server(vec![
            ("LOGIN \"ana@ejemplo.test\" \"abcd efgh\"", vec![]),
            ("EXAMINE \"INBOX\"", b"* 2 EXISTS\r\n".to_vec()),
            (
                "FETCH 1:2 (UID FLAGS INTERNALDATE X-GM-LABELS BODY.PEEK[HEADER.FIELDS",
                fetch,
            ),
        ]);
        let mut imap = client(port);
        imap.login("ana@ejemplo.test", "abcd efgh", "gmail")
            .unwrap();
        let exists = imap.select("INBOX", false).unwrap();
        assert_eq!(exists, 2);
        let messages = imap.fetch_recent(exists, 40, true).unwrap();
        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!((message.uid, message.date), (41, Some(1_792_231_200_000)));
        assert_eq!(message.flags, vec!["\\Seen", "\\Flagged"]);
        assert_eq!(message.labels, vec!["Trabajo"]);
        assert_eq!(message.header, header.as_bytes());
        assert_eq!(String::from_utf8(message.text.clone()).unwrap(), text);
        let asked = handle.join().unwrap();
        assert!(
            asked[2].contains("BODY.PEEK[TEXT]<0.12000>"),
            "nothing is marked as read: {}",
            asked[2]
        );
    }

    #[test]
    fn a_wrong_password_is_said_with_how_to_make_the_right_one() {
        let (port, _server) = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let handle = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket.write_all(b"* OK ready\r\n").unwrap();
                let mut line = [0u8; 256];
                let n = socket.read(&mut line).unwrap();
                let tag = String::from_utf8_lossy(&line[..n])
                    .split(' ')
                    .next()
                    .unwrap()
                    .to_string();
                socket
                    .write_all(
                        format!("{tag} NO [AUTHENTICATIONFAILED] Invalid credentials\r\n")
                            .as_bytes(),
                    )
                    .unwrap();
            });
            (port, handle)
        };
        let mut imap = client(port);
        let why = imap.login("ana@ejemplo.test", "mala", "gmail").unwrap_err();
        assert!(why.contains("contraseña de aplicación"), "{why}");
        assert!(
            imap.login("ana@ejemplo.test", "mal\r\na", "gmail")
                .unwrap_err()
                .contains("caracteres")
        );
    }

    #[test]
    fn a_message_is_moved_when_the_server_can_and_not_at_all_when_it_cannot_do_it_safely() {
        let (port, handle) = server(vec![
            ("CAPABILITY", b"* CAPABILITY IMAP4rev1 MOVE\r\n".to_vec()),
            ("UID MOVE 41 \"[Gmail]/Todos\"", vec![]),
        ]);
        let mut imap = client(port);
        imap.move_to(41, "[Gmail]/Todos").unwrap();
        handle.join().unwrap();
        let (port, _handle) = server(vec![("CAPABILITY", b"* CAPABILITY IMAP4rev1\r\n".to_vec())]);
        assert!(
            client(port)
                .move_to(41, "Archive")
                .unwrap_err()
                .contains("con seguridad")
        );
    }

    #[test]
    fn a_draft_is_sent_as_a_literal_to_the_drafts_mailbox() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.write_all(b"* OK ready\r\n").unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            socket.write_all(b"+ go\r\n").unwrap();
            let mut body = String::new();
            reader.read_line(&mut body).unwrap();
            let tag = first.split(' ').next().unwrap().to_string();
            socket
                .write_all(format!("{tag} OK appended\r\n").as_bytes())
                .unwrap();
            (first, body)
        });
        let mut imap = client(port);
        imap.save_draft("Drafts", "Subject: Re: Hola\r\n\r\nGracias")
            .unwrap_or_default();
        let (first, _body) = handle.join().unwrap();
        assert!(
            first.contains("APPEND \"Drafts\" (\\Draft \\Seen) {"),
            "{first}"
        );
    }
}
