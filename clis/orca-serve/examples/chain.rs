//! The chain the integration tests replay against (`orca-serve serve` over the real rca-service and rca-exec, a scripted model CLI), as one program for
//! the launch of a mimic candidate: it prints one JSON line with the pairing link when the chain listens and serves until its stdin closes.
//! Flags name the binaries a runner built (`--orca-serve`, `--rca-service`, `--rca-exec`, `--scripted-cli`); without them the test kit builds what it needs.
#[allow(dead_code, unused_imports)]
#[path = "../tests/integration/rig.rs"]
mod rig;

use std::io::Read;

fn main() {
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let var = match flag.as_str() {
            "--orca-serve" => "ORCA_SERVE_BIN",
            "--rca-service" => "RCA_SERVICE_BIN",
            "--rca-exec" => "RCA_EXEC_BIN",
            "--scripted-cli" => "EXEC_TESTKIT_CLI",
            other => panic!("chain: {other} is no flag (--orca-serve, --rca-service, --rca-exec, --scripted-cli)"),
        };
        let value = args.next().unwrap_or_else(|| panic!("chain: {flag} needs a path"));
        std::env::set_var(var, std::fs::canonicalize(&value).unwrap_or_else(|e| panic!("chain: {value}: {e}")));
    }
    let chain = rig::Chain::start_recorded();
    let workspace = std::fs::canonicalize(chain.repo.path()).expect("the repository has a path");
    let said = serde_json::json!({"ok": true, "event": "listening", "addr": chain.addr.to_string(), "workspace": workspace.display().to_string(), "pairing": chain.pair_url(), "cli_dir": chain.cli.dir().display().to_string()});
    println!("{said}");
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
}
