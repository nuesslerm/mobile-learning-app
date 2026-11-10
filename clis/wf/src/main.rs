pub(crate) use cli_http as http;
mod external;
mod help_table;
mod log_emit;
mod routes;
mod usage_row;

use wf_kit::cli::{self, Recorder, Surface};

fn main() {
    pipeline::dead_refs::set_commands(routes::is_command);
    wf_harness::run::set_publisher(|dir, ran_on, record| pipeline::test_publish::publish(dir, ran_on, record).map(|bundle| bundle.base));
    cli::run(&Surface {
        version: env!("CARGO_PKG_VERSION"),
        commands: routes::COMMANDS,
        // wf_bucket::HELP carries the `wf cache` table (nx, sccache, pnpm, metro, status); clis/wf/tests/flow/surface.rs fails when a verb has no row in this table
        help: &[help_table::HELP, offload::HELP, wf_bench::HELP, knowledge::HELP, agent_config_sync::HELP, wf_bucket::HELP, wf_lib::HELP, parity::HELP, pipeline::HELP, wf_harness::HELP[0], wf_harness::HELP[1], harness_flow::HELP, wf_login::HELP, pipeline::HELP, secrets::HELP, deploy::HELP, wf_vmops::HELP],
        hooks: wf_harness::device::cli::help_hooks(),
        recorder: Some(Recorder { send_if_asked: usage_row::send_if_asked, record: usage_row::record }),
    })
}
