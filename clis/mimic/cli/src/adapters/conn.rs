//! The one place a proxy of `wf mimic` opens a connection of its own, so a run can say how many it opened. A replay opens none.
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};

static DIALS: AtomicU64 = AtomicU64::new(0);

pub fn dial(addr: SocketAddr) -> std::io::Result<TcpStream> {
    DIALS.fetch_add(1, Ordering::SeqCst);
    TcpStream::connect(addr)
}

/// How many connections this process has tried to open to a server.
pub fn dials() -> u64 {
    DIALS.load(Ordering::SeqCst)
}
