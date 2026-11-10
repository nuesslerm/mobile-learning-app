//! The pure core: versions, the sparse index, lockfile and manifest text, and the decisions of `prune` and `status`. It touches no
//! file, process, socket or clock; the caller passes in the text and the time.

pub mod index;
pub mod libraries;
pub mod lockfile;
pub mod manifest;
pub mod prune;
pub mod version;
