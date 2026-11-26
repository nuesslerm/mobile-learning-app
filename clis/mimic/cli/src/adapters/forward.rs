//! `Forward`: the explicit proxy. A client that is told to use it asks for a host by name (`CONNECT host:port`, or an absolute
//! `http://` request). An approved host is intercepted: the client meets a leaf of the disposable CA, the real server's
//! certificate is validated, and what crosses is reported. Every other host is tunnelled untouched or, offline, refused.
use super::conn::dial;
use super::http1::{bodiless, body, read_head, start_line, status_of, Body, Head};
use super::proxy::{whole, Frames, Options, Sink};
use super::tls::{connect, Authority, ClientSide, ServerSide};
use mimic_core::cassette::Dir;
use mimic_proxy_core::domain::host_policy::{HostPolicy, Route, Seen};
use mimic_proxy_core::domain::traffic::{Event, Failure};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(5);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const BAD_GATEWAY: &[u8] = b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
pub(super) const FORBIDDEN: &[u8] = b"HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
pub(super) const ESTABLISHED: &[u8] = b"HTTP/1.1 200 Connection Established\r\n\r\n";

/// What an explicit proxy is made of.
pub struct Setup {
    pub policy: HostPolicy,
    pub authority: Authority,
    pub upstream: Arc<rustls::ClientConfig>,
    pub sink: Sink,
    pub seen: Mutex<Vec<Seen>>,
    pub options: Options,
    pub exchanges: AtomicU32,
    pub connections: AtomicU32,
}

impl Setup {
    fn saw(&self, seen: Seen) {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).push(seen);
    }
}

pub struct Forward {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    live: Arc<AtomicU32>,
    accepting: Option<JoinHandle<()>>,
}

struct Live(Arc<AtomicU32>);

impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Forward {
    pub fn listen(addr: SocketAddr, setup: Arc<Setup>, stop: Arc<AtomicBool>) -> io::Result<Forward> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let bound = listener.local_addr()?;
        let live = Arc::new(AtomicU32::new(0));
        let accepting = {
            let (stop, live) = (stop.clone(), live.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((client, _)) => {
                            let (setup, stop) = (setup.clone(), stop.clone());
                            live.fetch_add(1, Ordering::SeqCst);
                            let guard = Live(live.clone());
                            std::thread::spawn(move || {
                                let _ = guard;
                                if serve(client, &setup, &stop).is_err() {
                                    (setup.sink)(Event::Failed { failure: Failure::Incomplete });
                                }
                            });
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(POLL),
                        Err(_) => return,
                    }
                }
            })
        };
        Ok(Forward { addr: bound, stop, live, accepting: Some(accepting) })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn live(&self) -> u32 {
        self.live.load(Ordering::SeqCst)
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(accepting) = self.accepting.take() {
            let _ = accepting.join();
        }
    }
}

/// `host:port` of an authority, with the scheme's port when it names none.
pub(super) fn host_port(authority: &str, default_port: u16) -> Option<(String, u16)> {
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.contains(']') => Some((host.trim_matches(['[', ']']).to_string(), port.parse().ok()?)),
        _ if !authority.is_empty() => Some((authority.trim_matches(['[', ']']).to_string(), default_port)),
        _ => None,
    }
}

/// The host, port and path of an absolute `http://` request target.
pub(super) fn absolute(target: &str) -> Option<(String, u16, String)> {
    let rest = target.strip_prefix("http://")?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, String::from("/")),
    };
    let (host, port) = host_port(authority, 80)?;
    Some((host, port, path))
}

fn reach(host: &str, port: u16) -> io::Result<TcpStream> {
    let addr = (host, port).to_socket_addrs()?.next().ok_or(ErrorKind::NotFound)?;
    let stream = dial(addr)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(READ_TIMEOUT))?;
    Ok(stream)
}

fn serve(mut client: TcpStream, setup: &Setup, stop: &AtomicBool) -> io::Result<()> {
    client.set_nonblocking(false)?;
    client.set_read_timeout(Some(READ_TIMEOUT))?;
    client.set_write_timeout(Some(READ_TIMEOUT))?;
    let Ok((head, rest)) = read_head(&mut client) else {
        (setup.sink)(Event::Failed { failure: Failure::RequestHead });
        return Ok(());
    };
    let (method, target) = start_line(&head.start);
    if method == "CONNECT" {
        let Some((host, port)) = host_port(&target, 443) else { return client.write_all(FORBIDDEN) };
        return tunnel(client, setup, stop, &host, port);
    }
    let Some((host, port, path)) = absolute(&target) else { return client.write_all(FORBIDDEN) };
    let name = format!("{host}:{port}");
    let route = setup.policy.route(&host, port);
    if route == Route::Refuse {
        setup.saw(Seen::Refused(name));
        return client.write_all(FORBIDDEN);
    }
    let Ok(mut up) = reach(&host, port) else {
        (setup.sink)(Event::Failed { failure: Failure::NoAnswer });
        return client.write_all(BAD_GATEWAY);
    };
    let observed = route == Route::Intercept;
    setup.saw(if observed { Seen::Intercepted(name) } else { Seen::Tunnelled(name) });
    let origin = Head { start: format!("{method} {path} HTTP/1.1"), headers: head.headers };
    exchange(setup, origin, rest, &mut client, &mut up, &target, observed).map(|_| ())
}

fn tunnel(mut client: TcpStream, setup: &Setup, stop: &AtomicBool, host: &str, port: u16) -> io::Result<()> {
    let name = format!("{host}:{port}");
    match setup.policy.route(host, port) {
        Route::Refuse => {
            setup.saw(Seen::Refused(name));
            client.write_all(FORBIDDEN)
        }
        Route::Tunnel => {
            let Ok(up) = reach(host, port) else { return client.write_all(BAD_GATEWAY) };
            setup.saw(Seen::Tunnelled(name));
            client.write_all(ESTABLISHED)?;
            blind(client, up)
        }
        Route::Intercept => {
            client.write_all(ESTABLISHED)?;
            let mut inner = setup.authority.accept(host, client).map_err(io::Error::other)?;
            // the handshake runs with the first read: a client that will not trust the test CA ends it here
            let Ok((head, rest)) = read_head(&mut inner) else {
                setup.saw(Seen::RejectedCa(name));
                return Ok(());
            };
            setup.saw(Seen::Intercepted(name.clone()));
            let (_, path) = start_line(&head.start);
            let shown = if port == 443 { format!("https://{host}{path}") } else { format!("https://{name}{path}") };
            let Ok(server) = reach(host, port) else {
                (setup.sink)(Event::Failed { failure: Failure::NoAnswer });
                return inner.write_all(BAD_GATEWAY);
            };
            let mut up = connect(&setup.upstream, host, server).map_err(io::Error::other)?;
            if head.upgrades() {
                return socket(setup, stop, head, rest, inner, up, &shown, &name);
            }
            if exchange(setup, head, rest, &mut inner, &mut up, &shown, true)? == Upstream::Untrusted {
                setup.saw(Seen::UntrustedUpstream(name));
            }
            inner.conn.send_close_notify();
            inner.flush()
        }
    }
}

/// Copies both ways without looking: what an unapproved host says is not this proxy's to read.
fn blind(client: TcpStream, up: TcpStream) -> io::Result<()> {
    client.set_read_timeout(None)?;
    up.set_read_timeout(None)?;
    let (mut client_in, mut up_out) = (client.try_clone()?, up.try_clone()?);
    let outward = std::thread::spawn(move || {
        let _ = io::copy(&mut client_in, &mut up_out);
        let _ = up_out.shutdown(Shutdown::Write);
    });
    let (mut up_in, mut client_out) = (up, client);
    let _ = io::copy(&mut up_in, &mut client_out);
    let _ = client_out.shutdown(Shutdown::Write);
    let _ = outward.join();
    Ok(())
}

/// How a server took an exchange.
#[derive(PartialEq)]
enum Upstream {
    Answered,
    Silent,
    /// Its certificate did not validate: nothing was sent to it.
    Untrusted,
}

fn untrusted(e: &io::Error) -> bool {
    e.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()).is_some_and(|tls| matches!(tls, rustls::Error::InvalidCertificate(_)))
}

/// One HTTP exchange between two streams. A server that cannot be talked to is a 502 for the client.
fn exchange<C: Read + Write, U: Read + Write>(setup: &Setup, head: Head, rest: Vec<u8>, client: &mut C, up: &mut U, shown: &str, observed: bool) -> io::Result<Upstream> {
    let report = |event: Event| {
        if observed {
            (setup.sink)(event);
        }
    };
    let (method, _) = start_line(&head.start);
    let request = body(client, &head, rest, false)?;
    let exchange = setup.exchanges.fetch_add(1, Ordering::SeqCst) + 1;
    report(Event::HttpRequest { exchange, method: method.clone(), target: shown.to_string(), headers: head.headers.clone(), body: request.observed().to_vec() });
    let skip = |k: &str| matches!(k, "connection" | "proxy-connection") || (observed && setup.options.readable_bodies && k == "accept-encoding");
    let sent = up.write_all(&whole(format!("{}Connection: close\r\n\r\n", head.text(&skip)), &request.wire)).and_then(|()| up.flush());
    let (answer, early) = match sent.and_then(|()| read_head(up)) {
        Ok(pair) => pair,
        Err(e) => {
            report(Event::Failed { failure: Failure::NoAnswer });
            client.write_all(BAD_GATEWAY)?;
            client.flush()?;
            return Ok(if untrusted(&e) { Upstream::Untrusted } else { Upstream::Silent });
        }
    };
    let status = status_of(&answer.start);
    let answered = if bodiless(status, &method) { Body::empty() } else { body(up, &answer, early, true)? };
    report(Event::HttpResponse { exchange, status, headers: answer.headers.clone(), body: answered.observed().to_vec() });
    let skip = |k: &str| matches!(k, "connection" | "keep-alive");
    client.write_all(&whole(format!("{}Connection: close\r\n\r\n", answer.text(&skip)), &answered.wire))?;
    client.flush()?;
    Ok(Upstream::Answered)
}

/// A socket through an intercepted connection: the handshake goes through without the compression offer, then both directions
/// are relayed by one thread, which looks at each side in turn.
#[allow(clippy::too_many_arguments)]
fn socket(setup: &Setup, stop: &AtomicBool, head: Head, rest: Vec<u8>, mut client: ServerSide, mut up: ClientSide, shown: &str, name: &str) -> io::Result<()> {
    let connection = setup.connections.fetch_add(1, Ordering::SeqCst) + 1;
    let skip = |k: &str| matches!(k, "connection" | "sec-websocket-extensions");
    let sent = up.write_all(&whole(format!("{}Connection: Upgrade\r\n\r\n", head.text(&skip)), &rest)).and_then(|()| up.flush());
    let (answer, early) = match sent.and_then(|()| read_head(&mut up)) {
        Ok(pair) => pair,
        Err(e) => {
            if untrusted(&e) {
                setup.saw(Seen::UntrustedUpstream(name.to_string()));
            }
            (setup.sink)(Event::Failed { failure: Failure::NoHandshake });
            return client.write_all(BAD_GATEWAY);
        }
    };
    let status = status_of(&answer.start);
    let skip = |k: &str| matches!(k, "connection" | "keep-alive");
    if status != 101 {
        let refused = body(&mut up, &answer, early, true)?;
        (setup.sink)(Event::WsRefused { connection, target: shown.to_string(), status, body: refused.observed().to_vec() });
        return client.write_all(&whole(format!("{}Connection: close\r\n\r\n", answer.text(&skip)), &refused.wire));
    }
    (setup.sink)(Event::WsOpen { connection, target: shown.to_string(), status });
    client.write_all(format!("{}Connection: Upgrade\r\n\r\n", answer.text(&skip)).as_bytes())?;
    client.flush()?;
    client.sock.set_read_timeout(Some(POLL))?;
    up.sock.set_read_timeout(Some(POLL))?;
    let (mut outward, mut inward) = (Frames::new(Dir::C2s, connection), Frames::new(Dir::S2c, connection));
    inward.feed(&early, &setup.sink);
    client.write_all(&early)?;
    let mut chunk = [0u8; 8192];
    let mut relay = |from: &mut dyn Read, to: &mut dyn Write, frames: &mut Frames| -> io::Result<bool> {
        match from.read(&mut chunk) {
            Ok(0) => Ok(false),
            Ok(n) => {
                frames.feed(&chunk[..n], &setup.sink);
                to.write_all(&chunk[..n])?;
                to.flush()?;
                Ok(true)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => Ok(true),
            Err(e) => Err(e),
        }
    };
    while !stop.load(Ordering::SeqCst) {
        // a side that ended or failed ends the socket
        let open = relay(&mut client, &mut up, &mut outward).unwrap_or(false) && relay(&mut up, &mut client, &mut inward).unwrap_or(false);
        if !open {
            break;
        }
    }
    (setup.sink)(Event::WsClose { connection, direction: None, code: None });
    Ok(())
}
