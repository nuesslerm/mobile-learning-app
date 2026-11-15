//! One request, one connection: `Connection: close`, the whole answer read, chunked and gzip bodies decoded, repeated headers kept.
use flate2::read::{GzDecoder, ZlibDecoder};
use mimic_script_core::{HttpCall, HttpReply, TransportError};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

fn failed(e: impl ToString) -> TransportError {
    TransportError(e.to_string())
}

/// The cause of an I/O failure as a code a cassette can hold (`ECONNREFUSED`), the way Node names it.
fn code(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => "ECONNREFUSED".into(),
        std::io::ErrorKind::ConnectionReset => "ECONNRESET".into(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "timeout".into(),
        _ => e.to_string(),
    }
}

fn dechunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(line_end) = rest.windows(2).position(|w| w == b"\r\n") {
        let size = std::str::from_utf8(&rest[..line_end]).ok().and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok()).unwrap_or(0);
        let tail = &rest[line_end + 2..];
        if size == 0 || tail.len() < size {
            break;
        }
        out.extend_from_slice(&tail[..size]);
        rest = tail[size..].strip_prefix(b"\r\n").unwrap_or(&tail[size..]);
    }
    out
}

fn decode(body: Vec<u8>, encoding: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let read = match encoding.trim().to_lowercase().as_str() {
        "gzip" => GzDecoder::new(body.as_slice()).read_to_end(&mut out),
        "deflate" => ZlibDecoder::new(body.as_slice()).read_to_end(&mut out),
        _ => return body,
    };
    if read.is_ok() { out } else { body }
}

pub fn exchange(addr: SocketAddr, call: &HttpCall) -> Result<HttpReply, TransportError> {
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| TransportError(code(&e)))?;
    stream.set_read_timeout(Some(TIMEOUT)).map_err(failed)?;
    let body = call.body.as_deref().unwrap_or("");
    let mut head = format!("{} {} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nAccept-Encoding: gzip, deflate\r\n", call.method, call.path);
    // what `fetch` sends: a body is sized, a bodiless POST, PUT or PATCH says so, and a string body without a type is plain text
    if call.body.is_some() || matches!(call.method.as_str(), "POST" | "PUT" | "PATCH") {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    let typed = call.headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("content-type"));
    if call.body.is_some() && !typed {
        head.push_str("Content-Type: text/plain;charset=UTF-8\r\n");
    }
    for (name, value) in &call.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    stream.write_all(format!("{head}\r\n{body}").as_bytes()).map_err(|e| TransportError(code(&e)))?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| TransportError(code(&e)))?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| failed("the answer has no header end"))?;
    let (head, rest) = (String::from_utf8_lossy(&raw[..split]).into_owned(), &raw[split + 4..]);
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| failed("the answer has no status line"))?;
    let headers: Vec<(String, String)> = head.lines().skip(1).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    let find = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.to_lowercase());
    let body = if find("transfer-encoding").is_some_and(|v| v.contains("chunked")) { dechunk(rest) } else { rest.to_vec() };
    let body = decode(body, &find("content-encoding").unwrap_or_default());
    Ok(HttpReply { status, headers, body: String::from_utf8_lossy(&body).into_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_chunked_body_when_decoded_then_the_chunks_are_joined_and_extensions_ignored() {
        assert_eq!(dechunk(b"5;ext=1\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"), b"hello world");
    }
}
