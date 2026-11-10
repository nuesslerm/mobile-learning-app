//! The driving adapter: a small blocking HTTP/1.1 server over std sockets (one thread per connection) that turns requests into
//! `Service` calls, and carries `/ws` once the core's gate lets a socket through. Framing of the socket is tungstenite's; the
//! RPC session is the core's `Connection`, polled between reads.
mod http;
mod socket;

use std::io;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use t3_core::Service;

pub struct Running {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl Running {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Blocks while the server runs (until `stop` from another thread).
    pub fn wait(mut self) {
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

pub fn serve(bind: impl ToSocketAddrs, svc: Service) -> io::Result<Running> {
    let listener = TcpListener::bind(bind)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let accept = std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let (svc, stop) = (svc.clone(), flag.clone());
                    std::thread::spawn(move || http::handle(stream, &svc, &stop));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    });
    Ok(Running { addr, stop, accept: Some(accept) })
}
