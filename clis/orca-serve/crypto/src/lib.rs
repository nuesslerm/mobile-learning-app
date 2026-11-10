//! Orca's paired-device crypto as a pure core. The phone speaks the pinned reference's wire formats (`references/orca`,
//! `src/shared/*e2ee*`); this crate is the host side of them: no I/O, entropy and storage behind ports.

pub mod auth;
pub mod b64;
pub mod boxes;
pub mod entropy;
pub mod legacy;
pub mod pairing;
pub mod ports;
pub mod v2;
pub mod v2_frame;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use boxes::{KeyPair, KEY_LEN};
pub use entropy::{Entropy, SystemEntropy};
