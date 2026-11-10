use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// Where the proxy joins a connection: one local port for every destination, or a local port per `host:port` (a destination not in
/// the map is unreachable, as a tailnet host that is down is).
#[derive(Clone)]
pub enum Upstream {
    All(u16),
    Map(HashMap<String, u16>),
}

/// The tailnet as a black-box CLI test sees it: tailscaled's userspace SOCKS5 proxy, which resolves a host name itself. It accepts
/// every CONNECT, records the `host:port` it was asked for and joins the connection to a local service. Point the CLI at it with
/// `WF_TAILNET_SOCKS=<address>` (or `WF_SOCKS_PORT`). Port of `mock-tailnet.ts`.
pub struct MockTailnet {
    pub port: u16,
    destinations: Arc<Mutex<Vec<String>>>,
}

fn serve(mut client: TcpStream, upstream: Upstream, destinations: Arc<Mutex<Vec<String>>>) {
    let mut buf = [0u8; 512];
    if client.read(&mut buf).unwrap_or(0) == 0 || client.write_all(&[5, 0]).is_err() {
        return;
    }
    let n = client.read(&mut buf).unwrap_or(0);
    if n < 5 {
        return;
    }
    let (host, offset) = match buf[3] {
        3 => {
            let len = buf[4] as usize;
            (String::from_utf8_lossy(&buf[5..5 + len]).into_owned(), 5 + len)
        }
        1 => (buf[4..8].iter().map(u8::to_string).collect::<Vec<_>>().join("."), 8),
        _ => ("ipv6".to_string(), 20),
    };
    if n < offset + 2 {
        return;
    }
    let destination = format!("{host}:{}", u16::from_be_bytes([buf[offset], buf[offset + 1]]));
    destinations.lock().unwrap().push(destination.clone());
    let target = match &upstream {
        Upstream::All(port) => Some(*port),
        Upstream::Map(map) => map.get(&destination).copied(),
    };
    let Some(target) = target else {
        let _ = client.write_all(&[5, 4, 0, 1, 0, 0, 0, 0, 0, 0]);
        return;
    };
    let Ok(upstream) = TcpStream::connect(("127.0.0.1", target)) else { return };
    if client.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).is_err() {
        return;
    }
    let (mut client_read, mut upstream_write) = (client.try_clone().unwrap(), upstream.try_clone().unwrap());
    let forward = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_read, &mut upstream_write);
        let _ = upstream_write.shutdown(std::net::Shutdown::Write);
    });
    let (mut upstream_read, mut client_write) = (upstream, client);
    let mut chunk = [0u8; 8192];
    loop {
        let n = upstream_read.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        // A `MockReply { drop: true }` answers with this marker; the proxy then ends the connection instead of passing the answer on, which
        // is what the client sees when a tailnet host resets it.
        if String::from_utf8_lossy(&chunk[..n]).to_ascii_lowercase().contains(super::mock_server::DROP_MARKER) {
            let _ = client_write.shutdown(std::net::Shutdown::Both);
            let _ = upstream_read.shutdown(std::net::Shutdown::Both);
            let _ = forward.join();
            return;
        }
        if client_write.write_all(&chunk[..n]).is_err() {
            break;
        }
    }
    let _ = client_write.shutdown(std::net::Shutdown::Write);
    let _ = forward.join();
}

impl MockTailnet {
    pub fn start(upstream: Upstream) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a SOCKS listener");
        let port = listener.local_addr().unwrap().port();
        let destinations = Arc::new(Mutex::new(Vec::new()));
        let seen = destinations.clone();
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                let (seen, upstream) = (seen.clone(), upstream.clone());
                std::thread::spawn(move || serve(client, upstream, seen));
            }
        });
        Self { port, destinations }
    }

    /// `host:port` for WF_TAILNET_SOCKS.
    pub fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    /// Every `host:port` a client asked the proxy to reach, in order.
    pub fn destinations(&self) -> Vec<String> {
        self.destinations.lock().unwrap().clone()
    }
}
