//! The adapters of the step interpreter's `Transport` and `Clock` ports over the operating system: a blocking HTTP/1.1 client written for
//! what a recorder must keep (the status as sent, repeated header names, no redirect, a gzip body decoded as `fetch` decodes it), and a
//! WebSocket whose frames a worker thread queues for the interpreter to drain.
mod clock;
mod http;
mod sidecar;
mod socket;
mod transport;

pub use clock::SystemClock;
pub use sidecar::{Sidecar, SidecarPhone};
pub use transport::TcpTransport;
