pub mod cli;
pub mod contract;
pub mod fsx;
pub mod guard;
mod help_table;
pub mod paths;
pub mod render;
pub mod sync;
pub mod verb;

pub use help_table::HELP;
pub use verb::COMMANDS;

pub type Res<T> = Result<T, String>;

pub const SOURCES_PATH: &str = ".agents/config/sources.json";
pub const SHARED_SKILLS_DIR: &str = ".agents/skills";
