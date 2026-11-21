use crate::http::exchange;
use crate::socket::Socket;
use mimic_script_core::{Event, HttpCall, HttpReply, Transport, TransportError};
use std::net::{SocketAddr, TcpStream};
use tungstenite::client::IntoClientRequest;

/// A server on `addr`: one HTTP client and at most one WebSocket.
pub struct TcpTransport {
    addr: SocketAddr,
    socket: Option<Socket>,
}

impl TcpTransport {
    pub fn new(addr: SocketAddr) -> TcpTransport {
        TcpTransport { addr, socket: None }
    }
}

impl Transport for TcpTransport {
    fn http(&mut self, call: &HttpCall) -> Result<HttpReply, TransportError> {
        exchange(self.addr, call)
    }

    fn ws_open(&mut self, path: &str) -> Result<u16, TransportError> {
        self.socket = None;
        let stream = TcpStream::connect(self.addr).map_err(|e| TransportError(e.to_string()))?;
        let request = format!("ws://{}{path}", self.addr).into_client_request().map_err(|e| TransportError(e.to_string()))?;
        match tungstenite::client(request, stream) {
            Ok((ws, response)) => {
                self.socket = Some(Socket::spawn(ws));
                Ok(response.status().as_u16())
            }
            // an upgrade the server refused is an answer, with the status it gave
            Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response))) => Ok(response.status().as_u16()),
            Err(e) => Err(TransportError(e.to_string())),
        }
    }

    fn ws_send(&mut self, text: &str) -> Result<(), TransportError> {
        self.socket.as_ref().map(|s| s.send(text)).ok_or_else(|| TransportError("no socket".into()))
    }

    fn ws_drain(&mut self) -> Vec<Event> {
        self.socket.as_ref().map(Socket::drain).unwrap_or_default()
    }

    fn ws_close(&mut self, code: u16, reason: &str) -> Result<(), TransportError> {
        self.socket.as_ref().map(|s| s.close(code, reason)).ok_or_else(|| TransportError("no socket".into()))
    }
}
