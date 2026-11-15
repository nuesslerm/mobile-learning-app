//! The pure core of the `wf mimic` step interpreter. A case is a list of entries (an HTTP call, a WebSocket open, send or close); the
//! interpreter runs them against a `Transport`, waits by the `Policy` through a `Clock` and reports what crossed the wire as cassette steps
//! built by `mimic_core::wire`. Reference and candidate run the same code; only the `Policy` differs. No I/O here: the adapters implement the
//! ports (`docs/plans/mimic/plan.md`, issue #83 step 1).
pub mod case;
pub mod interpreter;
pub mod policy;
pub mod ports;
pub mod vars;

#[cfg(feature = "testing")]
pub mod testing;

pub use interpreter::Interpreter;
pub use policy::Policy;
pub use ports::{Clock, Event, HttpCall, HttpReply, Target, Transport, TransportError};
