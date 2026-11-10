//! The proxy's adapters and wiring: the HTTP router (`api`) and the OCI read-link reader (`infrastructure`); read-only, reached over the tailnet only.
pub mod api;
pub mod infrastructure;
