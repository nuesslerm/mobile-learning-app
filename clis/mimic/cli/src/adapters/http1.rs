//! HTTP/1.1 on a byte stream, as the proxies of `wf mimic` read and write it: a message head, its body by length, by chunks or to
//! the end of the connection, and the accept key of a WebSocket handshake.
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sha1::{Digest, Sha1};
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};

pub const MAX_HEAD: usize = 64 * 1024;
const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// The head of a message: its start line and its header lines.
pub struct Head {
    pub start: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// The head as it goes on the wire, without the blank line that ends it.
    pub fn text(&self, skip: &dyn Fn(&str) -> bool) -> String {
        let mut out = format!("{}\r\n", self.start);
        for (name, value) in &self.headers {
            if !skip(&name.to_lowercase()) {
                out.push_str(&format!("{name}: {value}\r\n"));
            }
        }
        out
    }

    pub fn upgrades(&self) -> bool {
        self.get("upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
    }
}

/// The start line's method and target.
pub fn start_line(start: &str) -> (String, String) {
    let mut parts = start.split_whitespace();
    (parts.next().unwrap_or("GET").to_string(), parts.next().unwrap_or("/").to_string())
}

/// The status code of a start line.
pub fn status_of(start: &str) -> u16 {
    start.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Reads the head of a message; what came after it is returned beside it.
pub fn read_head(stream: &mut impl Read) -> io::Result<(Head, Vec<u8>)> {
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let text = String::from_utf8_lossy(&buf[..end]).into_owned();
            let mut lines = text.split("\r\n");
            let start = lines.next().unwrap_or_default().to_string();
            let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
            return Ok((Head { start, headers }, buf[end + 4..].to_vec()));
        }
        if buf.len() > MAX_HEAD {
            return Err(io::ErrorKind::InvalidData.into());
        }
        match stream.read(&mut chunk)? {
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            n => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

// Preserve chunk framing and trailers on the wire. Only the decoded entity goes to the recorder.
pub struct Body {
    pub wire: Vec<u8>,
    pub decoded: Option<Vec<u8>>,
}

impl Body {
    pub fn empty() -> Body {
        Body { wire: Vec::new(), decoded: None }
    }

    pub fn observed(&self) -> &[u8] {
        self.decoded.as_deref().unwrap_or(&self.wire)
    }
}

pub fn body(stream: &mut impl Read, head: &Head, initial: Vec<u8>, to_eof: bool) -> io::Result<Body> {
    let mut reader = BufReader::new(Cursor::new(initial).chain(stream));
    let mut wire = Vec::new();
    if head.get("transfer-encoding").is_some_and(|v| v.split(',').any(|part| part.trim().eq_ignore_ascii_case("chunked"))) {
        let mut decoded = Vec::new();
        loop {
            let start = wire.len();
            reader.by_ref().take(MAX_HEAD as u64).read_until(b'\n', &mut wire)?;
            let line = wire[start..].strip_suffix(b"\r\n").ok_or(io::ErrorKind::InvalidData)?;
            let size = std::str::from_utf8(line.split(|c| *c == b';').next().unwrap_or_default())
                .ok()
                .and_then(|s| u64::from_str_radix(s.trim(), 16).ok())
                .ok_or(io::ErrorKind::InvalidData)?;
            if size == 0 {
                loop {
                    let start = wire.len();
                    reader.by_ref().take(MAX_HEAD as u64).read_until(b'\n', &mut wire)?;
                    let trailer = wire[start..].strip_suffix(b"\r\n").ok_or(io::ErrorKind::InvalidData)?;
                    if trailer.is_empty() {
                        return Ok(Body { wire, decoded: Some(decoded) });
                    }
                }
            }
            let start = wire.len();
            if reader.by_ref().take(size).read_to_end(&mut wire)? as u64 != size {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            decoded.extend_from_slice(&wire[start..]);
            let mut crlf = [0; 2];
            reader.read_exact(&mut crlf)?;
            if crlf != *b"\r\n" {
                return Err(io::ErrorKind::InvalidData.into());
            }
            wire.extend_from_slice(&crlf);
        }
    }
    if let Some(length) = head.get("content-length") {
        let length: u64 = length.parse().map_err(|_| io::ErrorKind::InvalidData)?;
        if reader.take(length).read_to_end(&mut wire)? as u64 != length {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
    } else if to_eof {
        reader.read_to_end(&mut wire)?;
    }
    Ok(Body { wire, decoded: None })
}

pub fn bodiless(status: u16, method: &str) -> bool {
    status == 204 || status == 304 || (100..200).contains(&status) || method == "HEAD"
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// One whole answer with its length, on a connection that closes after it.
pub fn write_answer(stream: &mut impl Write, status: u16, headers: &[(String, String)], body: &[u8], method: &str) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if bodiless(status, method) {
        return write!(stream, "{head}Connection: close\r\n\r\n");
    }
    write!(stream, "{head}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(body)
}

/// The `Sec-WebSocket-Accept` a server answers a client's key with.
pub fn accept_key(key: &str) -> String {
    let mut hash = Sha1::new();
    hash.update(key.trim().as_bytes());
    hash.update(WS_GUID.as_bytes());
    STANDARD.encode(hash.finalize())
}
