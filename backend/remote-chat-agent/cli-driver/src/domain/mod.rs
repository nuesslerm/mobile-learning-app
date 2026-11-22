//! The pure rules of running a provider CLI.

pub mod codex;
pub mod driver;
pub mod login;
pub mod provider_env;
pub mod translate;

pub use driver::{command_for, command_for_host, Command, Driver, TurnRequest, DRIVERS};
pub use login::{credential_files, driver_of_command, login_args, login_methods, read_prompt, LoginMethod, Prompt};
pub use provider_env::{provider_env, through_gateway, GatewayRoute, PROVIDERS_ROOT};
pub use translate::{session_of, translate, ExecEvent};
pub use codex::Verdict;
