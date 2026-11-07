//! What the tests of this crate share: a server that says what a script says, over a plain socket on this machine.
//! Nothing here reaches the network.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use crate::imap::Imap;

/// A server that answers each line it is asked with the next entry of `script` (its untagged lines, then the tag's
/// own OK), checking that the line holds the text the entry expects. An answer that starts with `+` asks for a literal:
/// the next line is that literal, and the tag's OK goes after the next entry. It returns what was asked, once the script ended.
pub fn server(script: Vec<(&'static str, Vec<u8>)>) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket.write_all(b"* OK ready\r\n").unwrap();
        let mut asked = Vec::new();
        let mut buffer = BufReader::new(socket.try_clone().unwrap());
        let mut waiting: Option<String> = None;
        for (expect, answer) in script {
            let mut line = String::new();
            buffer.read_line(&mut line).unwrap();
            let line = line.trim_end().to_string();
            assert!(line.contains(expect), "asked «{line}», expected «{expect}»");
            let tag = waiting
                .take()
                .unwrap_or_else(|| line.split(' ').next().unwrap().to_string());
            asked.push(line);
            if answer.starts_with(b"+") {
                socket.write_all(&answer).unwrap();
                waiting = Some(tag);
                continue;
            }
            let mut reply = answer.clone();
            reply.extend_from_slice(format!("{tag} OK done\r\n").as_bytes());
            // (A client that ends its session does not wait for the goodbye: the write may find the socket closed.)
            let _ = socket.write_all(&reply);
        }
        asked
    });
    (port, handle)
}

pub fn client(port: u16) -> Imap {
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    Imap::over(Box::new(tcp)).unwrap()
}

/// A FETCH answer for one message, as a server writes it: the header and the text as literals.
pub fn fetch_answer(seq: u32, uid: u64, flags: &str, header: &str, text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("* {seq} FETCH (UID {uid} FLAGS ({flags}) INTERNALDATE \"17-Oct-2026 10:00:00 +0000\" BODY[HEADER.FIELDS (FROM SUBJECT)] {{{}}}\r\n", header.len()).as_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(format!(" BODY[TEXT]<0> {{{}}}\r\n", text.len()).as_bytes());
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(b")\r\n");
    out
}
