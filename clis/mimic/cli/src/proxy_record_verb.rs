//! `wf mimic proxy record`: the loopback proxy as a recorder. It forwards what a client sends to the one upstream and, when the
//! run ends, writes what crossed as a replayable cassette set. Nothing is written before the sanitiser has seen all of it.
use super::proxy_verb::{listen_addr, serve_until, upstream_addr, upstream_public, Recorder};
use super::{fail, load_config, read_json, set_ref, tool, Done, Fail};
use crate::adapters::proxy::{Options, Proxy, Sink};
use crate::adapters::{conn, FsStore};
use mimic_core::cassette::Kind;
use mimic_core::ports::Store;
use mimic_core::rules::RuleSet;
use mimic_proxy_core::application::record::{default_rules, policy_for, record, Meta, RULES_FILE};
use mimic_proxy_core::domain::sanitize::Policy;
use mimic_proxy_core::domain::traffic::Event;
use serde_json::json;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use wf_kit::args::Parsed;

const NEXT: &str = "wf mimic proxy record --set <dir> --upstream http://127.0.0.1:<port>";
const SET_TOML: &str = "# Recorded through `wf mimic proxy record`. Add a [reference] with its pin to stamp a recording of a pinned server as recorded.\nrules = \"rules.json\"\n";
/// What the T3 app sends on some runs and not on others: a subscription names where it resumes from, and its fallback, only when the app holds a snapshot.
const T3_REPLAY_TOML: &str = "\n[proxy.replay]\nignore_keys = [\"httpSnapshotFallback\", \"afterSequence\"]\n";

fn rules_of(p: &Parsed, set_dir: &Path) -> Result<RuleSet, Fail> {
    let file = set_dir.join(RULES_FILE);
    if file.is_file() {
        return read_json(&file, "rules");
    }
    match p.val("preset") {
        None | Some("default") => Ok(default_rules()),
        Some("t3") => Ok(mimic_core::presets::t3()),
        Some(other) => Err(fail(format!("--preset takes default or t3, not {other:?}"), "wf mimic proxy record --help")),
    }
}

/// The values the caller declares must never be written, read from the variables it names. No refusal names a value.
fn declared(p: &Parsed) -> Result<Vec<(String, String)>, Fail> {
    let names = p.val("secret-env").unwrap_or_default().split(',').filter(|n| !n.is_empty());
    names
        .map(|name| match std::env::var(name) {
            Ok(value) if !value.is_empty() => Ok((name.to_string(), value)),
            _ => Err(fail(format!("--secret-env names {name}, which is not set"), "export the variable, or drop it from --secret-env")),
        })
        .collect()
}

/// What of this machine must not be in a published set: the home, the temp directory, the host and the user name (in any
/// case), and what `--mask` adds. A name inside a tailnet is replaced by the core whatever it is.
pub(super) fn masks(p: &Parsed) -> Result<Vec<(String, String)>, Fail> {
    let mut out = Vec::new();
    if let Some(home) = std::env::var("HOME").ok().filter(|h| h.len() > 1) {
        out.push((home, "/fixture/home".to_string()));
    }
    let temp = std::env::temp_dir();
    for dir in [std::fs::canonicalize(&temp).ok(), Some(temp)].into_iter().flatten() {
        let text = dir.display().to_string();
        let text = text.trim_end_matches('/');
        if text.len() > 1 {
            out.push((text.to_string(), "/fixture/tmp".to_string()));
        }
    }
    let host = std::process::Command::new("hostname").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    for (name, stand_in) in [(host.as_str(), "fixture-host"), (host.split('.').next().unwrap_or_default(), "fixture-host"), (user.as_str(), "fixture-user")] {
        if name.len() >= 4 && !out.iter().any(|(have, _)| have == name) {
            out.push((name.to_string(), stand_in.to_string()));
        }
    }
    for pair in p.val("mask").unwrap_or_default().split(',').filter(|m| !m.is_empty()) {
        let Some((value, stand_in)) = pair.split_once('=').filter(|(value, _)| !value.is_empty()) else {
            return Err(fail("--mask takes <value>=<stand-in> pairs", "wf mimic proxy record --help"));
        };
        out.push((value.to_string(), stand_in.to_string()));
    }
    Ok(out)
}

fn meta(p: &Parsed, set_dir: &Path) -> Result<Meta, Fail> {
    let toml = set_dir.join("mimic.toml");
    let reference = if toml.is_file() { load_config(&toml)?.reference } else { None };
    let target = reference.map(|r| r.provenance().map_err(|e| fail(e, "fix the [reference] pin in mimic.toml"))).transpose()?;
    let pinned = target.as_ref().is_some_and(|t| t.pin_hash.is_some());
    Ok(Meta {
        kind: if pinned { Kind::Recorded } else { Kind::Authored },
        target: target.filter(|_| pinned),
        tool: tool(),
        recorded_at: Some(wf_kit::time::iso(wf_kit::time::now_ms())),
        recorded_on: p.val("recorded-on").map(String::from),
        source: p.val("source").map(String::from),
        note: (!pinned).then(|| "observed through a proxy with no pinned reference named: a fixture, not evidence of a reference".to_string()),
    })
}

pub fn record_verb(p: &Parsed) -> Done {
    let Some(set_arg) = p.val("set") else { return Err(fail("--set <dir> is required", NEXT)) };
    let Some(upstream) = p.val("upstream") else { return Err(fail("--upstream <url> is required", NEXT)) };
    let set = set_ref(set_arg);
    if set.dir.join("cassette.json").exists() {
        return Err(fail(format!("{} holds a recording of its own", set.dir.display()), "name another --set directory: a recording is never written over"));
    }
    let upstream = upstream_addr(upstream)?;
    let listen = listen_addr(p.val("listen").unwrap_or("127.0.0.1:0"))?;
    let number = |name: &str| p.val(name).map(|v| v.parse::<u64>().map_err(|_| fail(format!("--{name} is not a number"), "wf mimic proxy record --help"))).transpose();
    let (duration, max_events) = (number("duration")?.filter(|d| *d > 0), number("max-events")?.unwrap_or(0));
    let rules = rules_of(p, &set.dir)?;
    let policy = policy_for(&rules, Policy { declared: declared(p)?, masks: masks(p)?, ..Policy::default() });
    let meta = meta(p, &set.dir)?;
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        let _ = signal_hook::flag::register(signal, stop.clone());
    }
    let trace = match p.val("trace") {
        None => None,
        Some(path) => {
            let file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|e| fail(format!("{path}: {e}"), "name a --trace file that does not exist yet"))?;
            Some(Arc::new(Mutex::new(Recorder::new(file, 0, stop.clone()))))
        }
    };
    let events: Arc<Mutex<Vec<Event>>> = Arc::default();
    let sink: Sink = {
        let (events, trace, stop) = (events.clone(), trace.clone(), stop.clone());
        Arc::new(move |event: Event| {
            if let Some(trace) = &trace {
                trace.lock().unwrap_or_else(|e| e.into_inner()).record(event.clone());
            }
            let mut events = events.lock().unwrap_or_else(|e| e.into_inner());
            events.push(event);
            if max_events > 0 && events.len() as u64 >= max_events {
                stop.store(true, Ordering::SeqCst);
            }
        })
    };
    let proxy = Proxy::listen_with(listen, upstream, sink, stop.clone(), Options { readable_bodies: true }).map_err(|e| fail(format!("the proxy did not listen on {listen}: {e}"), NEXT))?;
    let (bound, public) = (proxy.addr(), upstream_public(upstream));
    if let Some(trace) = &trace {
        trace.lock().unwrap_or_else(|e| e.into_inner()).header(&public, bound);
    }
    if let Some(ready) = p.val("ready") {
        let note = json!({"listen": bound.to_string(), "upstream": public, "set": set.dir.display().to_string()});
        std::fs::write(ready, format!("{note}\n")).map_err(|e| fail(format!("{ready}: {e}"), "name a writable --ready file"))?;
    }
    eprintln!("mimic proxy record: listening on {bound}, forwarding to {public}, recording into {}", set.dir.display());
    serve_until(&stop, duration, &|| proxy.live());
    drop(proxy);
    if let Some(trace) = &trace {
        trace.lock().unwrap_or_else(|e| e.into_inner()).footer();
    }
    let events = std::mem::take(&mut *events.lock().unwrap_or_else(|e| e.into_inner()));
    let recorded = match record(&events, rules, &policy, meta) {
        Ok(recorded) => recorded,
        Err(leak) => {
            let out = json!({
                "ok": false, "set": set.name, "wrote": [], "warnings": [],
                "error": format!("nothing was written: {leak}"),
                "next": "name the credential's key in a secret-key rule of the set's rules.json (or pass its value with --secret-env), then record again",
            });
            return Ok((out, 1));
        }
    };
    let mut store = FsStore::new(set.dir.parent().unwrap_or(Path::new(".")));
    store.write(&set.name, &recorded.set).map_err(|e| fail(e.to_string(), "make the set directory writable"))?;
    let mut wrote: Vec<String> = set.store.files_of(&set.name).iter().map(|f| f.display().to_string()).collect();
    wrote.push(set.dir.join(RULES_FILE).display().to_string());
    let toml = set.dir.join("mimic.toml");
    if !toml.exists() {
        let text = if p.val("preset") == Some("t3") { format!("{SET_TOML}{T3_REPLAY_TOML}") } else { SET_TOML.to_string() };
        std::fs::write(&toml, text).map_err(|e| fail(format!("{}: {e}", toml.display()), "make the set directory writable"))?;
        wrote.push(toml.display().to_string());
    }
    wrote.extend(p.val("trace").map(String::from));
    let cassette = &recorded.set.cassette;
    let count = |op: &str| cassette.interactions.iter().filter(|i| i.op.as_deref() == Some(op)).count();
    let ok = recorded.failures.is_empty();
    let out = json!({
        "ok": ok, "set": set.name, "kind": cassette.header.provenance.kind, "upstream": public, "listen": bound.to_string(),
        "interactions": cassette.interactions.len(), "http": count("http"), "ws": count("ws"), "upstream_connections": conn::dials(),
        "fixtures": recorded.sanitised.fixtures, "failures": recorded.failures, "gaps": [], "wrote": wrote, "warnings": [],
        "next": format!("wf mimic proxy replay {}", set.dir.display()),
    });
    Ok((out, i32::from(!ok)))
}
