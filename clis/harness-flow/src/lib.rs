//! The flow harness: `wf flow run` (vitest over the app's flow tests, harness doubles, jsdom) and `wf flow goldens` (the frozen
//! DOMs painted in a headless Chromium and checked against the committed goldens), plus the headless browser both use and that
//! `wf check --visual` drives. It builds on `harness-core` and stays below the pipeline: a published run goes through the
//! publisher the binary set there.
pub mod browser;
mod flow_goldens;
mod flow_run;
mod help_table;

pub use help_table::HELP;

pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("flow", flow_run::run)];
