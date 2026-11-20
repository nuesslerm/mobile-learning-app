//! The forward proxy the executor's Chromium is launched through (`--proxy-server`): every connection the browser makes (a page, a redirect, a
//! subresource, a WebSocket) arrives here as an HTTP CONNECT or a plain HTTP request, and the proxy resolves the name itself, refuses a blocked
//! address, and connects to the address it checked, so a name cannot be answered one way for the check and another for the connection.
use exec_browser::{parse_url, Policy, Target};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const HEAD_MAX: usize = 32 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub type Resolve = Arc<dyn Fn(&str, u16) -> std::io::Result<Vec<SocketAddr>> + Send + Sync>;
pub type Connect = Arc<dyn Fn(SocketAddr) -> std::io::Result<TcpStream> + Send + Sync>;

/// Whether an address is one a page must never reach on its own: loopback, unspecified, multicast, link-local (the metadata address among them),
/// private (RFC 1918, unique local), shared (100.64.0.0/10, a tailnet), benchmarking and protocol-assignment ranges. An IPv4 address inside an IPv6
/// one (mapped, compatible, NAT64, 6to4) is judged as the IPv4 one.
pub fn blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => blocked_v4(a),
        IpAddr::V6(a) => match embedded_v4(a) {
            Some(v4) => blocked_v4(v4),
            None => {
                let s = a.segments();
                a.is_loopback() || a.is_unspecified() || a.is_multicast() || (s[0] & 0xffc0) == 0xfe80 || (s[0] & 0xffc0) == 0xfec0 || (s[0] & 0xfe00) == 0xfc00 || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1)
            }
        },
    }
}

fn embedded_v4(a: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = a.segments();
    let from = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    if let Some(v4) = a.to_ipv4_mapped() {
        return Some(v4);
    }
    if s[..6] == [0, 0, 0, 0, 0, 0] && (s[6] != 0 || s[7] > 1) {
        return Some(from(s[6], s[7]));
    }
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return Some(from(s[6], s[7]));
    }
    (s[0] == 0x2002).then(|| from(s[1], s[2]))
}

fn blocked_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    a.is_loopback()
        || a.is_unspecified()
        || a.is_broadcast()
        || a.is_multicast()
        || a.is_link_local()
        || a.is_private()
        || (o[0] == 100 && (64..128).contains(&o[1]))
        || o[0] == 0
        || o[0] >= 240
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
}

/// What the proxy lets a connection reach: the policy's named `host:port`s, and otherwise only addresses that are not blocked.
#[derive(Clone)]
pub struct Egress {
    policy: Policy,
    resolve: Resolve,
    connect: Connect,
}

impl Egress {
    pub fn new(policy: Policy) -> Egress {
        Egress {
            policy,
            resolve: Arc::new(|host, port| (host, port).to_socket_addrs().map(|all| all.collect())),
            connect: Arc::new(|addr| TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)),
        }
    }

    pub fn with(policy: Policy, resolve: Resolve, connect: Connect) -> Egress {
        Egress { policy, resolve, connect }
    }

    /// Resolves the name once, refuses it when any answer is blocked (or none came), and connects to an address of that one answer.
    pub fn open(&self, target: &Target) -> Result<TcpStream, String> {
        let named = self.policy.names(target);
        if !named && self.policy.internal(target) {
            return Err(format!("{} is an internal name", target.host));
        }
        let addrs = match target.host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, target.port)],
            Err(_) => (self.resolve)(&target.host, target.port).map_err(|e| format!("{} does not resolve: {e}", target.host))?,
        };
        if addrs.is_empty() {
            return Err(format!("{} resolves to no address", target.host));
        }
        if !named {
            if let Some(a) = addrs.iter().find(|a| blocked_ip(a.ip())) {
                return Err(format!("{} resolves to {}, an address a page may not reach", target.host, a.ip()));
            }
        }
        let mut last = String::from("no address");
        for addr in addrs {
            match (self.connect)(addr) {
                Ok(s) => return Ok(s),
                Err(e) => last = e.to_string(),
            }
        }
        Err(format!("cannot connect to {}: {last}", target.host))
    }
}

pub struct Proxy {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Proxy {
    pub fn start(egress: Egress) -> std::io::Result<Proxy> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = conn {
                    let egress = egress.clone();
                    std::thread::spawn(move || serve(stream, &egress));
                }
            }
        });
        Ok(Proxy { port, stop })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn refuse(client: &mut TcpStream, status: &str) {
    let _ = client.write_all(format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").as_bytes());
}

fn read_head(client: &mut TcpStream) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = client.read(&mut chunk).ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(at + 4);
            return Some((buf, rest));
        }
        if buf.len() > HEAD_MAX {
            return None;
        }
    }
}

fn authority_target(authority: &str) -> Option<Target> {
    parse_url(&format!("http://{authority}/")).ok()
}

fn serve(mut client: TcpStream, egress: &Egress) {
    let Some((head, rest)) = read_head(&mut client) else { return };
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.split("\r\n");
    let request = lines.next().unwrap_or_default();
    let mut parts = request.split(' ');
    let (method, uri, version) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), parts.next().unwrap_or("HTTP/1.1"));
    if method.eq_ignore_ascii_case("CONNECT") {
        let Some(target) = parse_url(&format!("https://{uri}/")).ok().filter(|_| uri.contains(':')).map(|t| Target { host: t.host, port: t.port }) else { return refuse(&mut client, "400 Bad Request") };
        match egress.open(&target) {
            Ok(upstream) => {
                if client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").is_ok() {
                    tunnel(client, upstream, &rest);
                }
            }
            Err(_) => refuse(&mut client, "403 Forbidden"),
        }
        return;
    }
    let Some(after) = uri.strip_prefix("http://") else { return refuse(&mut client, "400 Bad Request") };
    let (authority, path) = after.split_once('/').map_or((after, "/".to_string()), |(a, p)| (a, format!("/{p}")));
    let Some(target) = authority_target(authority) else { return refuse(&mut client, "400 Bad Request") };
    match egress.open(&target) {
        Ok(upstream) => {
            let mut out = format!("{method} {path} {version}\r\n");
            for line in lines.filter(|l| !l.is_empty()) {
                let name = line.split(':').next().unwrap_or_default().to_ascii_lowercase();
                if !matches!(name.as_str(), "proxy-connection" | "connection" | "proxy-authorization") {
                    out.push_str(line);
                    out.push_str("\r\n");
                }
            }
            out.push_str("Connection: close\r\n\r\n");
            let mut forwarded = out.into_bytes();
            forwarded.extend_from_slice(&rest);
            tunnel(client, upstream, &forwarded);
        }
        Err(_) => refuse(&mut client, "403 Forbidden"),
    }
}

fn tunnel(client: TcpStream, mut upstream: TcpStream, first: &[u8]) {
    if !first.is_empty() && upstream.write_all(first).is_err() {
        return;
    }
    let (Ok(mut from_client), Ok(mut to_client)) = (client.try_clone(), client.try_clone()) else { return };
    let Ok(mut from_upstream) = upstream.try_clone() else { return };
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut from_client, &mut upstream);
        let _ = upstream.shutdown(Shutdown::Write);
    });
    let _ = std::io::copy(&mut from_upstream, &mut to_client);
    let _ = to_client.shutdown(Shutdown::Both);
    let _ = up.join();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn addr(ip: &str, port: u16) -> SocketAddr {
        SocketAddr::new(ip.parse().unwrap(), port)
    }

    fn target(host: &str, port: u16) -> Target {
        Target { host: host.into(), port }
    }

    #[test]
    fn given_addresses_when_judged_then_every_non_public_range_is_blocked_and_public_ones_are_not() {
        // Given addresses of each non-public range, including IPv4 ones carried inside IPv6
        for ip in [
            "127.0.0.1", "169.254.169.254", "10.1.2.3", "172.16.0.1", "192.168.0.1", "100.64.0.1", "0.0.0.0", "224.0.0.1", "192.0.0.8", "198.18.0.1", "198.19.255.1", "::1", "::", "fe80::1", "fd00:ec2::254", "fc00::1", "ff02::1",
            "::ffff:127.0.0.1", "::ffff:169.254.169.254", "::127.0.0.1", "64:ff9b::7f00:1", "64:ff9b::a9fe:a9fe", "2002:7f00:1::", "2002:a9fe:a9fe::1", "64:ff9b:1::1",
        ] {
            // Then each is blocked
            assert!(blocked_ip(ip.parse().unwrap()), "{ip}");
        }
        // And public ones, also inside NAT64 and 6to4, are not
        for ip in ["93.184.216.34", "100.63.0.1", "172.32.0.1", "198.20.0.1", "2001:4860:4860::8888", "64:ff9b::5db8:d822", "2002:5db8:d822::1", "::ffff:93.184.216.34"] {
            assert!(!blocked_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn given_a_name_whose_answer_changes_when_the_proxy_connects_then_it_connects_only_to_the_address_it_checked() {
        // Given a resolver that answers a public address first and loopback after, and a connector that records what it is asked to reach
        let answers = Arc::new(Mutex::new(vec![vec![addr("127.0.0.1", 80)], vec![addr("93.184.216.34", 80)]]));
        let asked = Arc::new(Mutex::new(0));
        let reached = Arc::new(Mutex::new(Vec::new()));
        let (a, n) = (answers.clone(), asked.clone());
        let resolve: Resolve = Arc::new(move |_, _| {
            *n.lock().unwrap() += 1;
            Ok(a.lock().unwrap().pop().unwrap())
        });
        let r = reached.clone();
        let connect: Connect = Arc::new(move |addr| {
            r.lock().unwrap().push(addr);
            Err(std::io::Error::other("test connector"))
        });
        let egress = Egress::with(Policy::default(), resolve, connect);
        // When it opens the name
        let first = egress.open(&target("rebind.example", 80)).unwrap_err();
        // Then it resolved once, and the one answer it checked is the one it connected to
        assert!(first.contains("cannot connect") && *asked.lock().unwrap() == 1, "{first}");
        assert_eq!(*reached.lock().unwrap(), vec![addr("93.184.216.34", 80)]);
        // And when the name's answer is a blocked address it is refused and nothing is connected
        answers.lock().unwrap().push(vec![addr("93.184.216.34", 80), addr("127.0.0.1", 80)]);
        let refused = egress.open(&target("mixed.example", 80)).unwrap_err();
        assert!(refused.contains("may not reach"), "{refused}");
        assert_eq!(reached.lock().unwrap().len(), 1);
    }

    #[test]
    fn given_a_name_that_does_not_resolve_when_opened_then_it_is_refused() {
        // Given a resolver that fails
        let resolve: Resolve = Arc::new(|_, _| Err(std::io::Error::other("nxdomain")));
        let connect: Connect = Arc::new(|_| panic!("no connection is made"));
        // When the name is opened, then it is refused, not passed
        assert!(Egress::with(Policy::default(), resolve, connect).open(&target("gone.example", 443)).unwrap_err().contains("does not resolve"));
    }

    #[test]
    fn given_an_allowlisted_loopback_server_when_connections_are_proxied_then_only_the_named_host_and_port_pass() {
        // Given a loopback server and a proxy whose policy names only it
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for conn in server.incoming().flatten() {
                let mut conn = conn;
                let mut buf = [0u8; 512];
                let n = conn.read(&mut buf).unwrap_or(0);
                let _ = conn.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}", if buf[..n].starts_with(b"GET / ") { "ok" } else { "no" }).as_bytes());
            }
        });
        let proxy = Proxy::start(Egress::new(Policy::parse(&format!("127.0.0.1:{port}")).unwrap())).unwrap();
        let ask = |request: String| {
            let mut s = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
            s.write_all(request.as_bytes()).unwrap();
            let mut reply = String::new();
            let _ = std::io::BufRead::read_line(&mut std::io::BufReader::new(&mut s), &mut reply);
            if reply.starts_with("HTTP/1.1 200 OK") {
                let mut rest = String::new();
                let _ = s.read_to_string(&mut rest);
                reply.push_str(&rest);
            }
            reply
        };
        // When a plain request and a CONNECT name it, then they are served
        assert!(ask(format!("GET http://127.0.0.1:{port}/ HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")).starts_with("HTTP/1.1 200 OK"));
        assert!(ask(format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\n")).starts_with("HTTP/1.1 200"));
        // And the same host on another port, the metadata address and a name for loopback are refused
        for request in [format!("GET http://127.0.0.1:{}/ HTTP/1.1\r\n\r\n", port.wrapping_add(1).max(1024)), "CONNECT 169.254.169.254:80 HTTP/1.1\r\n\r\n".to_string(), "CONNECT localhost:80 HTTP/1.1\r\n\r\n".to_string(), "GET http://[::1]:80/ HTTP/1.1\r\n\r\n".to_string()] {
            assert!(ask(request.clone()).starts_with("HTTP/1.1 403"), "{request}");
        }
    }
}
