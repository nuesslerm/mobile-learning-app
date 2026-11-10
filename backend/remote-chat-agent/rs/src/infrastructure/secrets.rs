//! The registered secrets live in `service-kit`, shared with every service; this keeps the path the service's own code uses.

pub use service_kit::secrets::{env_vars, load};
