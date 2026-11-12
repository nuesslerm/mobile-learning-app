//! `wf android`: the command table's device verbs (`device::cli`) on an adb serial (`--device`, else `ANDROID_SERIAL` or
//! `WF_ANDROID_SERIAL`, else the only device), the one redroid's lifecycle on the VM (`start-device`, `stop-device`,
//! `list-devices`: android_lab), `lease`, `hierarchy`, `adb` (raw, lease-guarded) and the script runner. Every verb but
//! `adb` and `test` prints one JSON envelope; a dropped verb exits 2 saying why and what covers it.
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::Instant;

/// The device an adb call is for: `ANDROID_SERIAL`, else `WF_ANDROID_SERIAL` (rdl's adb address on the VM).
fn env_serial() -> Option<String> {
    ["ANDROID_SERIAL", "WF_ANDROID_SERIAL"].iter().find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty()))
}

/// The serial a call is for: the one given, else `ANDROID_SERIAL`, else `WF_ANDROID_SERIAL`.
pub(crate) fn serial_or_env(given: Option<&str>) -> Option<String> {
    given.filter(|s| !s.is_empty()).map(str::to_string).or_else(env_serial)
}

/// The container a lifecycle or lease verb names: `--name`, else `WF_ANDROID_NAME`, else rdl. The device verbs take the adb
/// serial instead (`--device`), as the container is not its serial.
pub fn container(given: Option<&str>) -> String {
    let env = std::env::var("WF_ANDROID_NAME").ok().filter(|s| !s.is_empty());
    given.filter(|s| !s.is_empty()).map(str::to_string).or(env).unwrap_or_else(|| "rdl".into())
}

/// `wf android adb <args>`: everything after `adb` goes to adb verbatim, with the device's serial in front when one is set.
fn adb_passthrough(rest: &[String]) -> i32 {
    if let Some(code) = crate::android_lease::guard() {
        return code;
    }
    let mut args: Vec<String> = env_serial().map(|s| vec!["-s".to_string(), s]).unwrap_or_default();
    args.extend(rest.iter().cloned());
    match Command::new("adb").args(&args).stdin(Stdio::null()).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            wf_kit::wf_err!("wf: cannot run adb: {e}");
            1
        }
    }
}

/// `wf android`'s own verbs beside the command table's (`harness_core::device::api`): `wf check --dead-refs` checks each
/// `wf android <verb>` it reads against both.
pub const VERBS: &[&str] = &["adb", "hierarchy", "lease", "list-devices", "stack", "start-device", "stop-device", "test"];

/// Verbs that are gone, each with why and what covers it (docs/plans/harness-api/spec.md, Dropped).
pub(crate) const DROPPED: &[(&str, &str, &str)] = &[
    ("find", "get-element reads one element and hierarchy the screen", "wf android get-element; wf android hierarchy"),
    ("help", "every verb takes --help", "wf android --help"),
    ("install", "no caller beyond its own test", "wf android adb install -r <apk>"),
    ("launch", "scripts open screens by deep link", "wf android open-link"),
    ("start", "the SDK emulator's verb, listed but never served", "wf android start-device"),
    ("status", "the SDK emulator's verb, listed but never served", "wf android list-devices"),
    ("stop", "the SDK emulator's verb, listed but never served", "wf android stop-device"),
    ("stroke", "no script drew raw strokes", "wf android draw-strokes-on"),
    ("text", "it typed without the keyboard check input-text makes", "wf android input-text"),
    ("wait", "extended-wait-until is the one wait", "wf android extended-wait-until"),
];

fn emit(v: Value) {
    println!("{v}");
}

/// A verb's usage line: the first line of its committed help.
fn usage_of(verb: &str) -> String {
    wf_kit::help::text(&format!("android {verb}")).and_then(|t| t.lines().next()).and_then(|l| l.strip_prefix("Usage: ")).map_or_else(|| format!("wf android {verb}"), str::to_string)
}

fn usage_error(verb: &str, error: &str) -> i32 {
    emit(json!({ "ok": false, "command": verb, "platform": "android", "error": error, "usage": usage_of(verb) }));
    2
}

/// Why a verb did not answer: a usage error (exit 2) or a failure with its exit code and the next step.
enum Out {
    Usage(String),
    Failed { code: i32, error: String, next: Option<String> },
}

fn failed(error: impl Into<String>, next: &str) -> Out {
    Out::Failed { code: 1, error: error.into(), next: Some(next.into()) }
}

fn spec(verb: &str) -> Spec {
    let (values, flags, positional): (&'static [&'static str], &'static [&'static str], Positional) = match verb {
        "start-device" => (&["name", "screen", "port", "with"], &["dry-run", "slim", "stock", "expo-go", "local"], Positional::None),
        "stop-device" => (&["name"], &["dry-run", "local"], Positional::None),
        "list-devices" => (&["name"], &["local"], Positional::None),
        "stack" => (&["platform", "file"], &[], Positional::Exactly(1)),
        "lease" => (&["name", "wait", "ttl", "who", "id", "clean-package"], &["local", "no-wait"], Positional::AtMost(1)),
        _ => (&["device"], &[], Positional::None),
    };
    Spec { values, flags, positional, ..Spec::NONE }
}

fn start_device(name: &str, p: &Parsed) -> Result<Value, Out> {
    if crate::android_emulator::selected().map_err(Out::Usage)? {
        let script = crate::android_emulator::up_script(480, 1040, 213, p.flag("expo-go"));
        if p.flag("dry-run") {
            return Ok(json!({ "dryRun": true, "script": script }));
        }
        wf_kit::json_mode::stdout_to_stderr(|| crate::android_emulator::run_local("start-device", &script)).map_err(|e| failed(e, "the emulator log is emulator.log in RUNNER_TEMP; adb devices shows whether it came up"))?;
        return Ok(json!({ "name": crate::android_emulator::SERIAL, "provider": "emulator", "expoGo": p.flag("expo-go") }));
    }
    let up = crate::android_lab::up(p.val("screen"), name, p.val("port"), p.val("with"), p.flag("slim"), p.flag("stock"), p.flag("expo-go")).map_err(Out::Usage)?;
    let script = crate::android_lab::up_script(&up);
    if p.flag("dry-run") {
        return Ok(json!({ "dryRun": true, "script": script }));
    }
    let run = |script: &str| if p.flag("local") { crate::android_lab::run_local("start-device", script) } else { crate::android_lab::on_vm("start-device", script) };
    wf_kit::json_mode::stdout_to_stderr(|| run(&script)).map_err(|e| failed(e, "wf vm ops stats shows what holds the VM's memory; wf android stop-device, then start-device again"))?;
    Ok(up.summary())
}

fn stop_device(name: &str, p: &Parsed) -> Result<Value, Out> {
    if crate::android_emulator::selected().map_err(Out::Usage)? {
        let script = crate::android_emulator::down_script();
        if p.flag("dry-run") {
            return Ok(json!({ "dryRun": true, "script": script }));
        }
        wf_kit::json_mode::stdout_to_stderr(|| crate::android_emulator::run_local("stop-device", &script)).map_err(|e| failed(e, "adb devices shows whether the emulator is still there"))?;
        return Ok(json!({ "name": crate::android_emulator::SERIAL, "stopped": true }));
    }
    let script = crate::android_lab::down_script(name);
    if p.flag("dry-run") {
        return Ok(json!({ "dryRun": true, "script": script }));
    }
    let run = |script: &str| if p.flag("local") { crate::android_lab::run_local("stop-device", script) } else { crate::android_lab::on_vm("stop-device", script) };
    wf_kit::json_mode::stdout_to_stderr(|| run(&script)).map_err(|e| failed(e, "wf vm exec 'docker ps -a' shows whether the container is still there"))?;
    Ok(json!({ "name": name, "stopped": true }))
}

/// The devices as the provider reports them.
fn list_devices(name: &str, p: &Parsed) -> Result<Value, Out> {
    let found = if crate::android_emulator::selected().map_err(Out::Usage)? { crate::android_emulator::device() } else { crate::android_lab::device(name, p.flag("local")) };
    found.map(|d| json!({ "devices": d.into_iter().collect::<Vec<_>>() })).map_err(|e| failed(e, "wf vm ops health"))
}

fn hierarchy(given: Option<&str>) -> Result<(String, Value), Out> {
    use harness_core::device::Device;
    let serial = crate::driver::android_serial(given).map_err(|(code, e)| if code == 2 { Out::Usage(e) } else { failed(e, "wf android list-devices; or pass --device") })?;
    let tree = crate::driver::AndroidDevice::new(Some(serial.clone()), None).hierarchy().map_err(|e| failed(e, "wf android adb devices"))?;
    Ok((serial, tree))
}

/// `wf android <own verb>`: one JSON line, `{ok, command, platform, device, ms, result}` (exit 0), a failure with the next
/// step (exit 1, or 3 when the lease is held) or a usage error (exit 2).
fn verb(verb: &str, rest: &[String]) -> i32 {
    let started = Instant::now();
    if rest.iter().any(|a| a == "--help" || a == "-h") {
        return wf_kit::help::text(&format!("android {verb}")).map_or(2, |t| {
            print!("{t}");
            0
        });
    }
    let Some(p) = parse_spec(rest, &spec(verb)) else {
        let e = wf_kit::args::take_error().unwrap_or_default();
        let said: Vec<&str> = e.lines().map(str::trim).take_while(|l| !l.is_empty() && !l.starts_with("Usage:")).collect();
        return usage_error(verb, said.join(" ").trim_start_matches("error: "));
    };
    let name = container(p.val("name"));
    let (device, out) = match verb {
        "start-device" => (Some(name.clone()), start_device(&name, &p)),
        "stop-device" => (Some(name.clone()), stop_device(&name, &p)),
        "list-devices" => (Some(name.clone()), list_devices(&name, &p)),
        "stack" => (None, harness_core::android_logs::stack(&p.positional[0], p.val("platform").unwrap_or("android"), p.val("file")).map_err(|e| failed(e, "wf android stack --help; the group id is in the EAS Update link and in the run summary"))),
        "lease" => {
            let action = p.positional.first().map_or("acquire", String::as_str);
            if !["acquire", "heartbeat", "release", "status"].contains(&action) {
                return usage_error(verb, &format!("the action is acquire, heartbeat, release or status, got '{action}'"));
            }
            (Some(name.clone()), crate::android_lease::run(&name, &p, rest).map_err(|(code, error, next)| Out::Failed { code, error, next }))
        }
        _ => match hierarchy(p.val("device")) {
            Ok((serial, tree)) => (Some(serial), Ok(tree)),
            Err(e) => (p.val("device").map(str::to_string), Err(e)),
        },
    };
    let ms = started.elapsed().as_millis() as u64;
    match out {
        Ok(result) => {
            emit(json!({ "ok": true, "command": verb, "platform": "android", "device": device, "ms": ms, "result": result }));
            0
        }
        Err(Out::Usage(e)) => usage_error(verb, &e),
        Err(Out::Failed { code, error, next }) => {
            emit(json!({ "ok": false, "command": verb, "platform": "android", "device": device, "ms": ms, "error": error, "next": next }));
            code
        }
    }
}

pub fn run(args: &[String]) -> Option<i32> {
    let (cmd, rest) = args.split_first()?;
    match cmd.as_str() {
        "test" => return harness_core::run::test_for(&crate::driver::PLATFORM, rest),
        "adb" => return Some(adb_passthrough(rest)),
        v if harness_core::device::cli::is_verb(v) => return Some(harness_core::device::cli::run(&crate::driver::PLATFORM, v, rest)),
        v if VERBS.contains(&v) => return Some(verb(v, rest)),
        v if v.starts_with('-') => return None,
        _ => {}
    }
    let Some((old, why, instead)) = DROPPED.iter().find(|(old, _, _)| old == cmd) else { return Some(harness_core::device::cli::unknown_verb("android", cmd, VERBS)) };
    emit(json!({ "ok": false, "command": old, "platform": "android", "error": "dropped", "why": why, "use": instead }));
    Some(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_verbs_when_read_then_each_name_is_served_once_and_every_dropped_one_points_at_a_verb_that_exists() {
        let mut all: Vec<&str> = VERBS.iter().copied().chain(DROPPED.iter().map(|d| d.0)).collect();
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), n, "a verb is both kept and dropped");
        assert!(all.iter().all(|v| harness_core::device::api::by_verb(v).is_none()), "a dropped verb shadows a table verb");
        let lives = |v: &str| VERBS.contains(&v) || harness_core::device::api::by_verb(v).is_some();
        for (old, _, instead) in DROPPED {
            for v in instead.split("; ").filter_map(|u| u.strip_prefix("wf android ")).map(|u| u.split(' ').next().unwrap_or(u)) {
                assert!(lives(v) || v == "--help", "{old}: {v}");
            }
        }
        crate::register_help();
        for v in VERBS.iter().filter(|v| **v != "test") {
            assert!(wf_kit::help::text(&format!("android {v}")).is_some_and(|t| t.starts_with(&format!("Usage: wf android {v}"))), "wf android {v} has no help");
        }
    }
}
