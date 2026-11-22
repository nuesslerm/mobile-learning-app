//! `wf mimic exec`: the launcher of a test that runs real agent CLIs. It resolves the chosen CLIs' credentials in memory, starts the command with the
//! environment the test reads (and nothing a test crate needs to resolve itself), then scans what the run wrote before anything is published.
use super::{fail, Done};
use crate::child_env::ChildEnv;
use crate::real_clis::{parse_mode, scan, Resolution, Resolved, MODE_ENV};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use wf_kit::args::Parsed;

pub fn exec_verb(p: &Parsed) -> Done {
    let Some((program, args)) = p.positional.split_first() else {
        return Err(fail("name the command to run after --", "wf mimic exec --clis real:claude -- cargo test ..."));
    };
    let mode = p.val("clis").map(String::from).or_else(|| std::env::var(MODE_ENV).ok());
    if mode.as_deref().is_none_or(|m| m.trim().is_empty()) {
        return Err(fail(format!("no mode: pass --clis or set {MODE_ENV} (scripted, real or real:<claude,codex,opencode>)"), "wf mimic exec --clis scripted|real|real:<names> -- <command>..."));
    }
    let clis = parse_mode(mode.as_deref()).map_err(|e| fail(e, "wf mimic exec --help"))?;
    let bin = p.val("cli-dir").map(PathBuf::from).or_else(|| std::env::var_os("MIMIC_REAL_CLI_DIR").filter(|v| !v.is_empty()).map(PathBuf::from));
    if !clis.is_empty() && !bin.as_deref().is_some_and(Path::is_dir) {
        return Err(fail("real CLIs need the directory of their binaries: --cli-dir or MIMIC_REAL_CLI_DIR names none that exists", "wf mimic capture record --provision <set> installs them, or pass --cli-dir <dir>"));
    }
    let home = private_home().map_err(|e| fail(e, "check TMPDIR is writable"))?;
    let resolution = if p.flag("skip-when-empty") { Resolved::load_unless_empty(&clis, &home) } else { Resolved::load(&clis, &home).map(Resolution::Ready) };
    let resolution = resolution.inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&home);
    });
    let resolved = match resolution.map_err(|e| fail(e, "make the key available (the vault, or the environment without one)"))? {
        Resolution::Ready(resolved) => resolved,
        Resolution::Empty(keys) => {
            let _ = std::fs::remove_dir_all(&home);
            let why = format!("skipped: {} holds no value, so the real {} did not run (--skip-when-empty); mint it with `wf secret login codex-ci`", keys.join(", "), clis.iter().map(|c| c.name).collect::<Vec<_>>().join(", "));
            let _ = std::io::Write::write_all(&mut std::io::stderr(), format!("{why}\n").as_bytes());
            return Ok((json!({"ok": true, "ran": false, "skipped": why, "real_clis": [], "command": program, "findings": [], "codex_login_rotated": false, "warnings": [], "next": "nothing ran: nothing to publish"}), 0));
        }
    };
    let logins: Vec<_> = resolved.logins().into_iter().map(|(cli, path)| {
        let before = std::fs::read(&path).ok();
        (cli, path, before)
    }).collect();

    let mut own: Vec<(String, String)> = Vec::new();
    for cli in &clis {
        own.extend(resolved.child_env(cli, ChildEnv::default()).pairs());
    }
    let listing = |names: Vec<String>| names.join(",");
    let mut vars = vec![
        ("T3_CHAIN_REAL_CLIS".to_string(), listing(resolved.names().iter().map(|n| n.to_string()).collect())),
        ("T3_CHAIN_EXECUTOR_ENV".to_string(), listing(own.iter().map(|(k, _)| k.clone()).collect())),
        ("T3_CHAIN_CLI_HOME".to_string(), home.display().to_string()),
        (MODE_ENV.to_string(), mode.clone().unwrap_or_default()),
    ];
    if let Some(bin) = bin.as_ref().filter(|_| !clis.is_empty()) {
        vars.push(("MIMIC_REAL_CLI_DIR".to_string(), std::path::absolute(bin).unwrap_or_else(|_| bin.clone()).display().to_string()));
    }
    let mut command = ChildEnv::base().with(vars).with(own).command(program);
    let ran = run_captured(command.args(args));
    let (status, said) = match ran {
        Ok((status, said)) => (Ok(status), said),
        Err(e) => (Err(e), Vec::new()),
    };
    let said = String::from_utf8_lossy(&said).into_owned();
    let mut shown = said.clone();
    let mut output_findings: Vec<String> = Vec::new();
    for (key, value) in resolved.secrets().iter().filter(|(_, v)| !v.is_empty()) {
        if said.contains(value.as_str()) {
            output_findings.push(format!("the command's own output holds the value of {key}"));
            shown = shown.replace(value.as_str(), &format!("[withheld: {key}]"));
        }
    }
    output_findings.dedup();
    let _ = std::io::Write::write_all(&mut std::io::stderr(), shown.as_bytes());

    let mut outputs: Vec<PathBuf> = p.val("scan").map(|v| v.split(',').filter(|s| !s.is_empty()).map(PathBuf::from).collect()).unwrap_or_default();
    if let Some(dir) = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|v| !v.is_empty()) {
        outputs.push(PathBuf::from(dir));
    }
    let (existing, absent): (Vec<_>, Vec<_>) = outputs.into_iter().partition(|d| d.is_dir());
    let dirs: Vec<&Path> = existing.iter().map(PathBuf::as_path).collect();
    let mut findings = if clis.is_empty() { Vec::new() } else { scan(&dirs, resolved.secrets()) };
    findings.extend(output_findings);
    // a file login a run refreshed: its refresh token is single-use, so the vault copy is dead until the new file is written back
    let rotated_logins: Vec<_> = logins.iter().filter(|(_, path, before)| before.is_some() && std::fs::read(path).ok() != *before).collect();
    let rotated = !rotated_logins.is_empty();
    let mut written_back = serde_json::Map::new();
    let mut writeback_failed: Vec<String> = Vec::new();
    for (cli, path, _) in rotated_logins.iter().filter(|(cli, ..)| cli.writes_back) {
        let outcome = match secrets::rotate_login(cli.key, &path.display().to_string()) {
            Ok(secrets::Rotation::Updated) => "updated".to_string(),
            Ok(secrets::Rotation::Unchanged) => "unchanged".to_string(),
            Ok(secrets::Rotation::Refused(why)) => {
                writeback_failed.push(format!("{}: {why}", cli.key));
                "refused".to_string()
            }
            Err(why) => {
                writeback_failed.push(format!("{}: {why}", cli.key));
                "failed".to_string()
            }
        };
        written_back.insert(cli.key.to_string(), json!(outcome));
    }
    let names = resolved.names();
    drop(resolved);
    let _ = std::fs::remove_dir_all(&home);

    let status = status.map_err(|e| fail(format!("the command {program}: {e}"), "fix the command after --"))?;
    let exit = status.code().unwrap_or(-1);
    let mut warnings: Vec<String> = absent.iter().map(|d| format!("{} does not exist: nothing was scanned there", d.display())).collect();
    for (cli, ..) in rotated_logins.iter().filter(|(cli, ..)| !cli.writes_back) {
        warnings.push(format!("the Codex login was rotated during the run and the vault still holds the old one: wf secret rotate-login {}", cli.name));
    }
    warnings.extend(writeback_failed.iter().map(|why| format!("the refreshed login was not written back and the vault's is dead until `wf secret login codex-ci` runs again ({why})")));
    let ok = exit == 0 && findings.is_empty() && writeback_failed.is_empty();
    let next = if !writeback_failed.is_empty() {
        "the refreshed login is lost to the vault: wf secret login codex-ci"
    } else if !findings.is_empty() {
        "hold the output back: delete the files named, find what wrote the value, then run it again"
    } else if exit != 0 {
        "the command failed: read its output above"
    } else {
        "wf test publish"
    };
    Ok((json!({"ok": ok, "ran": true, "real_clis": names, "command": program, "exit": exit, "scanned": dirs.len(), "findings": findings, "codex_login_rotated": rotated, "login_written_back": written_back, "warnings": warnings, "next": next}), if ok { 0 } else { 1 }))
}

/// The command's stdout and stderr, interleaved in one buffer, so they are scanned and redacted before they reach the log.
fn run_captured(command: &mut std::process::Command) -> std::io::Result<(std::process::ExitStatus, Vec<u8>)> {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let pump = |mut stream: Box<dyn Read + Send>| {
        let buffer = Arc::clone(&buffer);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = stream.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                buffer.lock().unwrap().extend_from_slice(&chunk[..n]);
            }
        })
    };
    let out = child.stdout.take().map(|s| pump(Box::new(s)));
    let err = child.stderr.take().map(|s| pump(Box::new(s)));
    let status = child.wait()?;
    for handle in [out, err].into_iter().flatten() {
        let _ = handle.join();
    }
    let said = std::mem::take(&mut *buffer.lock().unwrap());
    Ok((status, said))
}

/// A fresh owner-only directory under the temp dir; `create` fails rather than reuse a path someone else planted.
fn private_home() -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let home = std::env::temp_dir().join(format!("mimic-exec-{}-{nanos}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&home).map_err(|e| format!("the run's home {}: {}", home.display(), e.kind()))?;
    Ok(home)
}
