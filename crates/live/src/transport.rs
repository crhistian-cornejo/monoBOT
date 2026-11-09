//! The connection under a live session: a blocking WebSocket over TLS on the
//! session's own thread, read with a short timeout so the same thread can
//! also send. Tests stand in a channel through `Connect`.
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// How the connection ended, as the far end said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Closed {
    pub code: u16,
    pub reason: String,
}

pub trait Transport: Send {
    fn send(&mut self, message: &str) -> Result<(), Closed>;
    /// Waits up to `wait` for a message; `Ok(None)` when none came.
    fn recv(&mut self, wait: Duration) -> Result<Option<String>, Closed>;
    fn close(&mut self) {}
}

/// Opens connections; a session reconnects through it.
pub trait Connect: Send + 'static {
    fn connect(
        &self,
        url: &str,
        headers: &[(&'static str, String)],
    ) -> Result<Box<dyn Transport>, String>;
}

/// The real network.
pub struct Network;

impl Connect for Network {
    fn connect(
        &self,
        url: &str,
        headers: &[(&'static str, String)],
    ) -> Result<Box<dyn Transport>, String> {
        Ok(Box::new(Socket::open(url, headers)?))
    }
}

struct Socket {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    wait: Duration,
}

impl Socket {
    fn open(url: &str, headers: &[(&'static str, String)]) -> Result<Self, String> {
        let mut request = url
            .into_client_request()
            .map_err(|_| "Invalid live voice address.".to_string())?;
        for (name, value) in headers {
            let value = HeaderValue::from_str(value).map_err(|_| "Invalid API key.".to_string())?;
            request.headers_mut().insert(*name, value);
        }
        let host = request.uri().host().unwrap_or_default().to_owned();
        let port = request
            .uri()
            .port_u16()
            .unwrap_or(if url.starts_with("wss://") { 443 } else { 80 });
        let address = (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addresses| addresses.next())
            .ok_or_else(|| format!("Could not find {host}. Check the network."))?;
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(15))
            .map_err(|_| format!("Could not reach {host}. Check the network."))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(15))))
            .and_then(|()| stream.set_nodelay(true))
            .map_err(|error| error.to_string())?;
        let limits = WebSocketConfig::default()
            .max_message_size(Some(4 << 20))
            .max_frame_size(Some(4 << 20));
        let (socket, _) = tungstenite::client_tls_with_config(request, stream, Some(limits), None)
            .map_err(|error| match error {
                tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response)) => {
                    match response.status().as_u16() {
                        401 | 403 => {
                            "The API key was refused. Check it in Settings › Voice.".into()
                        }
                        429 => "The live voice service is busy or out of quota.".into(),
                        status => format!("The live voice service answered {status}."),
                    }
                }
                _ => format!("Could not open the live call with {host}."),
            })?;
        Ok(Self {
            socket,
            wait: Duration::from_millis(20),
        })
    }

    fn timeout(&mut self, wait: Duration) {
        if wait == self.wait {
            return;
        }
        self.wait = wait;
        let stream = match self.socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream,
            MaybeTlsStream::Rustls(stream) => stream.get_mut(),
            _ => return,
        };
        let _ = stream.set_read_timeout(Some(wait.max(Duration::from_millis(1))));
    }
}

fn closed(error: tungstenite::Error) -> Closed {
    match error {
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => Closed {
            code: 1000,
            reason: String::new(),
        },
        other => Closed {
            code: 1006,
            reason: other.to_string(),
        },
    }
}

impl Transport for Socket {
    fn send(&mut self, message: &str) -> Result<(), Closed> {
        self.socket
            .send(Message::Text(message.into()))
            .map_err(closed)
    }

    fn recv(&mut self, wait: Duration) -> Result<Option<String>, Closed> {
        self.timeout(wait);
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => return Ok(Some(text.to_string())),
                // Gemini sends its JSON in binary frames.
                Ok(Message::Binary(bytes)) => {
                    return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()));
                }
                Ok(Message::Close(frame)) => {
                    return Err(frame.map_or_else(Closed::default, |frame| Closed {
                        code: frame.code.into(),
                        reason: frame.reason.to_string(),
                    }));
                }
                Ok(_) => continue,
                Err(tungstenite::Error::Io(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(closed(error)),
            }
        }
    }

    fn close(&mut self) {
        let _ = self.socket.close(None);
        let _ = self.socket.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain WebSocket server on this machine: the client sends its
    /// headers, gets a message back, and a short wait is not an error.
    #[test]
    #[allow(clippy::result_large_err)]
    fn talks_to_a_local_websocket_with_its_headers() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/live", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut key = None;
            let mut socket = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request, response| {
                    key = request
                        .headers()
                        .get("x-goog-api-key")
                        .map(|v| v.to_str().unwrap().to_owned());
                    Ok(response)
                },
            )
            .unwrap();
            let got = socket.read().unwrap().into_text().unwrap().to_string();
            socket
                .send(Message::Text(format!("echo {got}").into()))
                .unwrap();
            socket
                .send(Message::Binary(br#"{"setupComplete":{}}"#.to_vec().into()))
                .unwrap();
            let _ = socket.close(None);
            let _ = socket.flush();
            key
        });
        let mut socket = Network
            .connect(&url, &[("x-goog-api-key", "AIza-local".into())])
            .unwrap();
        socket.send("hello").unwrap();
        let mut got = Vec::new();
        loop {
            match socket.recv(Duration::from_millis(200)) {
                Ok(Some(message)) => got.push(message),
                Ok(None) => continue,
                Err(_) => break,
            }
        }
        assert_eq!(got, ["echo hello", r#"{"setupComplete":{}}"#]);
        assert_eq!(server.join().unwrap().as_deref(), Some("AIza-local"));
    }

    #[test]
    fn nobody_listening_is_an_error_not_a_hang() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/live", listener.local_addr().unwrap());
        drop(listener);
        assert!(Network.connect(&url, &[]).is_err());
    }
}
