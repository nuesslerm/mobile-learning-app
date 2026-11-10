//! `callsite-check --contract <json> --impl <name> --source <file> [--repo-root <dir>]`, records as JSON lines on stdin: prints the
//! verdict of the call-site contract as one JSON line and exits 1 when it is not ok.

use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            println!("{}", serde_json::json!({ "ok": false, "error": message }));
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone());
    let need = |name: &str| flag(name).ok_or_else(|| format!("missing {name} (usage: callsite-check --contract <json> --impl <name> --source <file> [--repo-root <dir>])"));
    let contract: Value = serde_json::from_str(&std::fs::read_to_string(need("--contract")?).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let implementation = need("--impl")?;
    let source = PathBuf::from(need("--source")?);
    let repo_root = PathBuf::from(flag("--repo-root").unwrap_or_else(|| ".".into()));
    let root = contract["implementations"][&implementation]["root"].as_str().ok_or_else(|| format!("the contract has no implementation `{implementation}`"))?;
    let file = telemetry::callsite::file_of(&repo_root, root, &source)?;
    let text = std::fs::read_to_string(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin).map_err(|e| e.to_string())?;
    let records = stdin
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).map_err(|e| format!("a record line is not JSON ({e}): {l}")))
        .collect::<Result<Vec<_>, _>>()?;
    let verdict = telemetry::callsite::check(&contract, &implementation, &text, &file, &records)?;
    println!("{verdict}");
    Ok(verdict["ok"] == true)
}
