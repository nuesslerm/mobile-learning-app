//! One adapter per technology behind the ports: git, cargo, and one publisher per registry kind.
pub mod bucket_sparse;
pub mod cargo;
pub mod cargo_publish;
pub mod git;
pub mod guard;
pub mod index_source;
