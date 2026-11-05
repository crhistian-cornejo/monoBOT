//! The arrivals of an account's inbox, heard as they happen. One connection per account stays open in `IDLE`: the
//! server says «EXISTS» the moment a message lands, the new ones are read (`UID FETCH` from the next UID, as a list
//! reading does), they join the last reading (`Mail::arrived`) and the owner of the watch is told who they are from.
//! Nothing is polled: while nothing arrives, the connection only waits. Like every reading it is read only (`EXAMINE`):
//! it marks nothing as read and moves nothing.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::account::Account;
use crate::model::{Folder, Message};
use crate::service::{Mail, connect, message_from};
use crate::secrets;

/// A server ends an `IDLE` after about half an hour; a new one starts before that.
const IDLE_FOR: Duration = Duration::from_secs(20 * 60);
/// More new messages than this at once (after a long sleep): only the newest are told.
const MAX_BATCH: usize = 25;
/// The wait after a failed connection grows from this to `MAX_WAIT`.
const FIRST_WAIT: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(5 * 60);

/// What a watch heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Messages that landed in the inbox, oldest first, as the page shows them.
    Arrived(Vec<Message>),
    /// Messages gone from the inbox or with new flags (read on the phone, archived elsewhere): the page reads again.
    Changed,
}

/// The senders the owner finds interesting, asked at each arrival (the owner may change them any time).
pub type Interesting = Arc<dyn Fn() -> HashSet<String> + Send + Sync>;
/// Who is told what the watch of an account heard.
pub type Listener = Arc<dyn Fn(&Account, Heard) + Send + Sync>;

impl Mail {
    /// Starts watching an account's inbox. A watch already running for it is stopped first.
    pub fn watch(self: &Arc<Self>, account: Account, interesting: Interesting, listener: Listener) {
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(old) = self.watchers.lock().unwrap_or_else(|p| p.into_inner()).insert(account.id.clone(), stop.clone()) {
            old.store(true, Ordering::SeqCst);
        }
        let mail = self.clone();
        let _ = std::thread::Builder::new().name("correo-aviso".into()).spawn(move || run(&mail, &account, &stop, &interesting, &listener));
    }

    /// Stops watching an account (it was removed).
    pub fn unwatch(&self, account_id: &str) {
        if let Some(stop) = self.watchers.lock().unwrap_or_else(|p| p.into_inner()).remove(account_id) {
            stop.store(true, Ordering::SeqCst);
        }
    }

    /// Stops every watch (Echo closes).
    pub fn unwatch_all(&self) {
        for (_, stop) in self.watchers.lock().unwrap_or_else(|p| p.into_inner()).drain() {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

fn run(mail: &Arc<Mail>, account: &Account, stop: &AtomicBool, interesting: &Interesting, listener: &Listener) {
    let mut failures = 0u32;
    while !stop.load(Ordering::SeqCst) {
        let started = Instant::now();
        let result = session(mail, account, stop, interesting, listener);
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let why = result.err().unwrap_or_else(|| "El servidor cerró la conexión.".into());
        // A session that lasted is a hiccup of the network; one that never got going backs off.
        failures = if started.elapsed() > Duration::from_secs(120) { 1 } else { failures + 1 };
        crate::base::log::line(format!("correo: la espera de correo nuevo se cortó ({why})"));
        // A password the server or the system's store refuses does not get better by trying again: the owner reconnects.
        if why.contains("contraseña") {
            return;
        }
        let until = Instant::now() + (FIRST_WAIT * 2u32.saturating_pow(failures.min(7))).min(MAX_WAIT);
        while Instant::now() < until && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

fn session(mail: &Arc<Mail>, account: &Account, stop: &AtomicBool, interesting: &Interesting, listener: &Listener) -> Result<(), String> {
    let password = secrets::get(&account.address)?;
    let mut imap = connect(&mail.open, account, &password)?;
    drop(password);
    let (_, next) = imap.examine_next("INBOX")?;
    let mut next = next.ok_or("El servidor de correo no dice cuál es el próximo correo.")?;
    crate::base::log::line(format!("correo: atento a los correos nuevos ({})", account.provider));
    while !stop.load(Ordering::SeqCst) {
        let idled = imap.idle(|| !stop.load(Ordering::SeqCst), IDLE_FOR)?;
        if idled.arrived {
            let mut raws = imap.fetch_from(next, account.is_gmail())?;
            if let Some(top) = raws.iter().map(|raw| raw.uid).max() {
                next = top + 1;
            }
            if raws.len() > MAX_BATCH {
                raws.drain(..raws.len() - MAX_BATCH);
            }
            let senders = interesting();
            let pairs: Vec<_> = raws.iter().map(|raw| message_from(account, Folder::Inbox, raw, &senders)).collect();
            if !pairs.is_empty() {
                let messages: Vec<Message> = pairs.iter().map(|(message, _)| message.clone()).collect();
                mail.arrived(pairs);
                listener(account, Heard::Arrived(messages));
            }
        }
        if idled.changed {
            listener(account, Heard::Changed);
        }
    }
    imap.logout();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imap::Imap;
    use crate::model::Overview;
    use crate::service::view_of;
    use crate::testkit::fetch_answer;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::channel;

    /// A server that lets a message land while the client waits in `IDLE`, as a real one does: its «+», later its
    /// «EXISTS», and the end of the wait only when the client says `DONE`.
    fn landing(port_for: std::sync::mpsc::Sender<u16>) -> std::thread::JoinHandle<Vec<String>> {
        std::thread::spawn(move || {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            port_for.send(listener.local_addr().unwrap().port()).unwrap();
            let (mut socket, _) = listener.accept().unwrap();
            socket.write_all(b"* OK ready\r\n").unwrap();
            let mut input = BufReader::new(socket.try_clone().unwrap());
            let mut asked = Vec::new();
            let mut waits = 0;
            loop {
                let mut line = String::new();
                if input.read_line(&mut line).unwrap() == 0 {
                    return asked;
                }
                let line = line.trim_end().to_string();
                let tag = line.split(' ').next().unwrap().to_string();
                asked.push(line.clone());
                if line.contains("LOGIN") {
                    socket.write_all(format!("{tag} OK in\r\n").as_bytes()).unwrap();
                } else if line.contains("EXAMINE") {
                    socket.write_all(format!("* 2 EXISTS\r\n* OK [UIDNEXT 8] next\r\n{tag} OK done\r\n").as_bytes()).unwrap();
                } else if line.ends_with("IDLE") {
                    waits += 1;
                    socket.write_all(b"+ idling\r\n").unwrap();
                    if waits == 1 {
                        std::thread::sleep(Duration::from_millis(150));
                        socket.write_all(b"* 3 EXISTS\r\n").unwrap();
                    }
                    let mut done = String::new();
                    input.read_line(&mut done).unwrap();
                    asked.push(done.trim_end().to_string());
                    socket.write_all(format!("{tag} OK idle done\r\n").as_bytes()).unwrap();
                } else if line.contains("UID FETCH 8:*") {
                    let header = "From: Lucía <lucia@ejemplo.test>\r\nSubject: Cena\r\n\r\n";
                    let mut answer = fetch_answer(3, 8, "", header, "¿Vienes el sábado?");
                    answer.extend_from_slice(format!("{tag} OK done\r\n").as_bytes());
                    socket.write_all(&answer).unwrap();
                } else if line.contains("LOGOUT") {
                    return asked;
                }
            }
        })
    }

    #[test]
    fn a_message_that_lands_is_told_with_who_sent_it_and_joins_the_last_reading() {
        let account = Account::new("aviso@ejemplo.test", "Ana", "imap", Some("imap.ejemplo.test")).unwrap();
        secrets::save(&account.address, "abcd efgh").unwrap();
        let (sent, port) = channel();
        let server = landing(sent);
        let port = port.recv().unwrap();
        let mail = Arc::new(Mail::with_opener(Box::new(move |_| {
            let tcp = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
            tcp.set_read_timeout(Some(Duration::from_millis(100))).map_err(|e| e.to_string())?;
            Imap::over(Box::new(tcp))
        })));
        mail.restore(Overview { accounts: vec![view_of(&account)], ..Overview::default() });
        let (heard, listen) = channel();
        mail.watch(account.clone(), Arc::new(HashSet::new), Arc::new(move |_, what| { let _ = heard.send(what); }));
        let Heard::Arrived(messages) = listen.recv_timeout(Duration::from_secs(5)).expect("the arrival is told") else { panic!("an arrival") };
        assert_eq!(messages.len(), 1);
        assert_eq!((messages[0].from.as_str(), messages[0].subject.as_str(), messages[0].unread), ("Lucía", "Cena", true));
        assert_eq!(mail.last().unwrap().messages.len(), 1, "it is in the last reading, ready for the page");
        mail.unwatch(&account.id);
        let asked = server.join().unwrap();
        assert!(asked.iter().any(|line| line.contains("EXAMINE")) && asked.iter().all(|line| !line.contains("SELECT") && !line.contains("STORE")), "read only: {asked:?}");
        assert!(asked.iter().any(|line| line == "DONE"));
        secrets::forget(&account.address);
    }
}
