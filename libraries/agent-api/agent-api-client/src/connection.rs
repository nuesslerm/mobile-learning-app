//! Opening the socket: the address a `ws://` URL names, the bearer on the handshake, and what a refusal of it is.
use crate::error::{code, Error};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::{HandshakeError, WebSocket};

/// Where the agent API listens, after the service's address.
pub const SOCKET_PATH: &str = "/agent/v1/socket";

/// `host:port` of a `ws://host:port` URL (a path after it is ignored). Another scheme is refused: nothing here speaks TLS.
pub fn authority(url: &str) -> Result<String, Error> {
    let Some(rest) = url.strip_prefix("ws://") else {
        let scheme = url.split("://").next().filter(|s| *s != url).unwrap_or("no scheme");
        return Err(Error::new(code::CONNECT, format!("only ws:// is supported, the URL has {scheme}")));
    };
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(Error::new(code::CONNECT, "the URL names no host"));
    }
    Ok(authority.to_string())
}

fn connect_error(message: impl std::fmt::Display) -> Error {
    Error::new(code::CONNECT, message.to_string())
}

/// The socket to `url`, authenticated by `bearer`. A 401 or 403 on the handshake is `UNAUTHORIZED`, anything else that stops it `CONNECT`.
pub fn open(url: &str, bearer: &str, timeout: Duration) -> Result<WebSocket<TcpStream>, Error> {
    let authority = authority(url)?;
    let addrs = authority.to_socket_addrs().map_err(|e| connect_error(format!("{authority} did not resolve: {e}")))?;
    let mut last = connect_error(format!("{authority} resolved to no address"));
    for addr in addrs {
        let stream = match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => stream,
            Err(e) => {
                last = connect_error(format!("{addr}: {e}"));
                continue;
            }
        };
        // The handshake must not hang on a service that accepts and says nothing
        stream.set_read_timeout(Some(timeout)).map_err(connect_error)?;
        stream.set_nodelay(true).map_err(connect_error)?;
        let mut request = format!("ws://{authority}{SOCKET_PATH}").into_client_request().map_err(connect_error)?;
        request.headers_mut().insert("authorization", format!("Bearer {bearer}").parse().map_err(|_| connect_error("the bearer is not a valid header value"))?);
        return match tungstenite::client(request, stream) {
            Ok((socket, _)) => Ok(socket),
            Err(HandshakeError::Failure(tungstenite::Error::Http(response))) if matches!(response.status().as_u16(), 401 | 403) => {
                Err(Error::new(code::UNAUTHORIZED, format!("{authority} refused the bearer ({})", response.status())))
            }
            Err(HandshakeError::Failure(e)) => Err(connect_error(format!("{authority}: {e}"))),
            Err(HandshakeError::Interrupted(_)) => Err(connect_error(format!("{authority} did not finish the handshake in time"))),
        };
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_urls_when_their_authority_is_read_then_only_ws_is_accepted_and_the_message_names_what_the_url_has() {
        // Given a ws URL with and without a path, a wss URL, an http URL, text with no scheme and a ws URL with no host
        // When the authority is read
        // Then the ws ones give host:port and the others are CONNECT errors that name the scheme
        assert_eq!(authority("ws://127.0.0.1:8080"), Ok("127.0.0.1:8080".to_string()));
        assert_eq!(authority("ws://rca:9/agent/v1/socket"), Ok("rca:9".to_string()));
        for (url, has) in [("wss://rca:9", "wss"), ("http://rca:9", "http"), ("rca:9", "no scheme")] {
            let refused = authority(url).unwrap_err();
            assert_eq!((refused.code.as_str(), refused.message.contains(has)), (code::CONNECT, true), "{url}: {}", refused.message);
        }
        assert_eq!(authority("ws://").unwrap_err().message, "the URL names no host");
    }
}
