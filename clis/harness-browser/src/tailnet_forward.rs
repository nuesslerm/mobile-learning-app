//! A localhost listener that dials a target through a SOCKS5 proxy (tailscaled's userspace listener, default
//! 127.0.0.1:1055) and pipes bytes both ways: a transparent, protocol-agnostic tunnel for plain HTTP and WebSockets.
//! One listener per `host:port` for the life of the daemon.
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const DEFAULT_SOCKS_PORT: u16 = 1055;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

fn active() -> &'static Mutex<HashMap<String, u16>> {
    static ACTIVE: OnceLock<Mutex<HashMap<String, u16>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn socks_port() -> u16 {
    std::env::var("WF_SOCKS_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_SOCKS_PORT)
}

/// SOCKS5 client handshake with no-auth only; the target goes over as a domain because the proxy resolves it.
fn socks5_connect(socks: u16, host: &str, port: u16) -> io::Result<TcpStream> {
    let bad = |m: &str| io::Error::other(m.to_string());
    if host.len() > 255 {
        return Err(bad("host name too long"));
    }
    let mut s = TcpStream::connect(("127.0.0.1", socks))?;
    s.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    s.write_all(&[5, 1, 0])?;
    let mut method = [0u8; 2];
    s.read_exact(&mut method)?;
    if method[0] != 5 || method[1] != 0 {
        return Err(bad("proxy refused the no-auth method"));
    }
    let mut req = vec![5, 1, 0, 3, host.len() as u8];
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    s.write_all(&req)?;
    let mut head = [0u8; 4];
    s.read_exact(&mut head)?;
    if head[0] != 5 {
        return Err(bad("bad proxy reply version"));
    }
    if head[1] != 0 {
        return Err(bad("proxy could not connect to the target"));
    }
    let addr_len = match head[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut l = [0u8; 1];
            s.read_exact(&mut l)?;
            l[0] as usize
        }
        _ => return Err(bad("bad proxy address type")),
    };
    let mut rest = vec![0u8; addr_len + 2];
    s.read_exact(&mut rest)?;
    s.set_read_timeout(None)?;
    Ok(s)
}

fn pipe(mut from: TcpStream, mut to: TcpStream) {
    let _ = io::copy(&mut from, &mut to);
    let _ = to.shutdown(Shutdown::Write);
    let _ = from.shutdown(Shutdown::Read);
}

fn forward(client: TcpStream, socks: u16, host: String, port: u16) {
    let Ok(upstream) = socks5_connect(socks, &host, port) else { return };
    let (Ok(client_rx), Ok(upstream_rx)) = (client.try_clone(), upstream.try_clone()) else { return };
    thread::spawn(move || pipe(upstream_rx, client_rx));
    pipe(client, upstream);
}

fn accept(listener: TcpListener, socks: u16, host: String, port: u16) {
    while let Ok((client, _)) = listener.accept() {
        let host = host.clone();
        thread::spawn(move || forward(client, socks, host, port));
    }
}

/// `(local port, reused)`: starts the listener for `host:port`, or returns the running one. A requested local port that
/// something else already holds counts as reused, as the Node daemon reports it.
pub fn ensure(host: &str, port: u16, local_port: Option<u16>) -> Result<(u16, bool), String> {
    let key = format!("{host}:{port}");
    if let Some(existing) = active().lock().map_err(|_| "forward registry poisoned")?.get(&key) {
        return Ok((*existing, true));
    }
    let listener = match TcpListener::bind(("127.0.0.1", local_port.unwrap_or(0))) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => return Ok((local_port.unwrap_or(0), true)),
        Err(e) => return Err(format!("cannot listen on 127.0.0.1:{}: {e}", local_port.unwrap_or(0))),
    };
    let bound = listener.local_addr().map_err(|e| e.to_string())?.port();
    let host_owned = host.to_string();
    let socks = socks_port();
    thread::Builder::new().name("wf-forward".into()).spawn(move || accept(listener, socks, host_owned, port)).map_err(|e| e.to_string())?;
    active().lock().map_err(|_| "forward registry poisoned")?.insert(key, bound);
    Ok((bound, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    enum Proxy {
        Accepts,
        RefusesAuth,
        FailsConnect,
    }

    fn fake_proxy(mode: Proxy) -> (u16, thread::JoinHandle<Vec<u8>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut greet = [0u8; 3];
            c.read_exact(&mut greet).unwrap();
            assert_eq!(greet, [5, 1, 0]);
            if matches!(mode, Proxy::RefusesAuth) {
                c.write_all(&[5, 0xFF]).unwrap();
                return vec![];
            }
            c.write_all(&[5, 0]).unwrap();
            let mut head = [0u8; 5];
            c.read_exact(&mut head).unwrap();
            let mut rest = vec![0u8; head[4] as usize + 2];
            c.read_exact(&mut rest).unwrap();
            let mut req = head.to_vec();
            req.extend(rest);
            if matches!(mode, Proxy::FailsConnect) {
                c.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
                return req;
            }
            c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
            let mut buf = [0u8; 4];
            c.read_exact(&mut buf).unwrap();
            c.write_all(&buf.map(|b| b.to_ascii_uppercase())).unwrap();
            req
        });
        (port, h)
    }

    #[test]
    fn given_a_proxy_that_accepts_when_a_client_sends_bytes_then_the_domain_target_is_requested_and_bytes_round_trip() {
        let (socks, proxy) = fake_proxy(Proxy::Accepts);
        let mut up = socks5_connect(socks, "box.tail.ts.net", 9222).unwrap();
        up.write_all(b"ping").unwrap();
        let mut out = [0u8; 4];
        up.read_exact(&mut out).unwrap();
        assert_eq!(&out, b"PING");
        let req = proxy.join().unwrap();
        assert_eq!(&req[..5], &[5, 1, 0, 3, 15]);
        assert_eq!(&req[5..20], b"box.tail.ts.net");
        assert_eq!(&req[20..], &9222u16.to_be_bytes());
    }

    #[test]
    fn given_a_proxy_that_refuses_the_no_auth_method_when_connecting_then_it_is_an_error() {
        let (socks, proxy) = fake_proxy(Proxy::RefusesAuth);
        let err = socks5_connect(socks, "h", 1).unwrap_err();
        assert!(err.to_string().contains("no-auth"));
        proxy.join().unwrap();
    }

    #[test]
    fn given_a_proxy_that_fails_the_connect_when_forwarding_then_the_client_is_closed_without_data() {
        let (socks, proxy) = fake_proxy(Proxy::FailsConnect);
        let front = TcpListener::bind("127.0.0.1:0").unwrap();
        let fport = front.local_addr().unwrap().port();
        thread::spawn(move || accept(front, socks, "h".into(), 80));
        let mut c = TcpStream::connect(("127.0.0.1", fport)).unwrap();
        let mut buf = Vec::new();
        c.read_to_end(&mut buf).unwrap();
        assert!(buf.is_empty());
        proxy.join().unwrap();
    }

    #[test]
    fn given_a_forwarder_when_a_client_talks_through_it_then_bytes_flow_both_ways() {
        let (socks, proxy) = fake_proxy(Proxy::Accepts);
        let front = TcpListener::bind("127.0.0.1:0").unwrap();
        let fport = front.local_addr().unwrap().port();
        thread::spawn(move || accept(front, socks, "h".into(), 80));
        let mut c = TcpStream::connect(("127.0.0.1", fport)).unwrap();
        c.write_all(b"abcd").unwrap();
        let mut out = [0u8; 4];
        c.read_exact(&mut out).unwrap();
        assert_eq!(&out, b"ABCD");
        proxy.join().unwrap();
    }
}
