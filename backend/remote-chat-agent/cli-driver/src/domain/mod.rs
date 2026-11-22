//! The pure rules of running a provider CLI.

pub mod claude;
pub mod codex;
pub mod driver;
pub mod login;
pub mod opencode;
pub mod provider_env;
pub mod translate;

pub use driver::{Driver, ToolServer, TurnRequest, DRIVERS};
pub use login::{credential_files, driver_of_command, login_args, login_methods, read_prompt, LoginMethod, Prompt};
pub use provider_env::{provider_env, GatewayRoute, PROVIDERS_ROOT};
pub use translate::ExecEvent;
pub use codex::Verdict;
