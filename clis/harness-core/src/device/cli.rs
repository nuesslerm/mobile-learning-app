//! `wf browser|android <verb>` for the verbs of the command table: flags read from the table, the device picked (`--device`,
//! or the only one), `commands::call` (which runs the Android lease guard for guarded verbs), and one JSON line.
use super::api::{self, Command, Kind};
use super::commands::{self, Args, Ctx};
use super::golden::Goldens;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::Instant;

/// Whether `wf browser|android <verb>` is a table verb this front end serves.
pub fn is_verb(verb: &str) -> bool {
    api::by_verb(verb).is_some()
}

/// The device part of the help engine: the verb list of a device noun's page and the page of one verb of the command table.
pub fn help_hooks() -> wf_kit::help::Hooks {
    wf_kit::help::Hooks {
        noun_section: |noun| matches!(noun, "browser" | "android").then(|| api::noun_section(noun)),
        verb_help: |noun, verb| {
            let cmd = api::by_verb(verb).filter(|_| matches!(noun, "browser" | "android"))?;
            Some(api::verb_help(noun, cmd))
        },
    }
}

struct Parsed {
    args: Args,
    device: Option<String>,
    condition: Option<String>,
}

fn take(rest: &[String], i: &mut usize, inline: &Option<String>, flag: &str) -> Result<String, String> {
    if let Some(v) = inline {
        return Ok(v.clone());
    }
    let v = rest.get(*i).filter(|v| !v.starts_with("--")).cloned().ok_or_else(|| format!("--{flag} needs a value"))?;
    *i += 1;
    Ok(v)
}

fn number(flag: &str, v: &str) -> Result<Value, String> {
    serde_json::from_str::<Value>(v).ok().filter(Value::is_number).ok_or_else(|| format!("--{flag} wants a number, got '{v}'"))
}

/// The selector param a prefixed flag names: `--element-id` is `element`'s id, `--from-text` is `from`'s text.
fn prefixed(cmd: &Command, flag: &str) -> Option<(&'static str, &'static str)> {
    cmd.params.iter().filter(|p| p.kind == Kind::Sel).find_map(|p| {
        let field = flag.strip_prefix(&format!("{}-", p.name.replace('_', "-")))?;
        ["id", "text", "css"].into_iter().find(|f| *f == field).map(|f| (p.name, f))
    })
}

fn parse(cmd: &Command, rest: &[String]) -> Result<Parsed, String> {
    let wait = cmd.name == "extended_wait_until";
    let own = cmd.param("sel").map(|p| p.name);
    let (mut args, mut sels, mut words) = (Args::new(), Map::new(), Vec::new());
    let (mut device, mut condition, mut mode): (Option<String>, Option<String>, Option<&str>) = (None, None, None);
    let mut i = 0;
    while i < rest.len() {
        let raw = rest[i].as_str();
        i += 1;
        if raw == "--json" {
            continue;
        }
        let Some(flag) = raw.strip_prefix("--") else {
            words.push(raw.to_string());
            continue;
        };
        let (flag, inline) = match flag.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (flag, None),
        };
        match flag {
            "device" => device = Some(take(rest, &mut i, &inline, flag)?),
            "visible" | "not-visible" if wait => {
                if mode.replace(if flag == "visible" { "visible" } else { "not_visible" }).is_some() {
                    return Err("give one of --visible and --not-visible".into());
                }
            }
            "condition" if wait => condition = Some(take(rest, &mut i, &inline, flag)?),
            // a selector field, unless the verb has no selector and a param of that name (input-text --text)
            "id" | "text" | "css" | "point" if wait || own.is_some() || cmd.param(flag).is_none() => {
                let target = if wait { "visible" } else { own.ok_or_else(|| format!("{} takes no --{flag}", cmd.verb()))? };
                let entry = sels.entry(target).or_insert_with(|| json!({}));
                if !entry.is_object() {
                    return Err("give the selector as flags or one string, not both".into());
                }
                entry[flag] = json!(take(rest, &mut i, &inline, flag)?);
            }
            other => {
                if let Some((param, field)) = prefixed(cmd, other) {
                    sels.entry(param).or_insert_with(|| json!({}))[field] = json!(take(rest, &mut i, &inline, other)?);
                    continue;
                }
                let name = other.replace('-', "_");
                let p = cmd.param(&name).filter(|p| p.kind != Kind::Sel && p.kind != Kind::Lua).ok_or_else(|| format!("unknown option --{other}"))?;
                let v = match p.kind {
                    Kind::Bool => json!(inline.as_deref() != Some("false")),
                    Kind::Int | Kind::Num => number(other, &take(rest, &mut i, &inline, other)?)?,
                    Kind::Json => {
                        let v = take(rest, &mut i, &inline, other)?;
                        serde_json::from_str(&v).map_err(|e| format!("--{other} wants JSON: {e}"))?
                    }
                    Kind::Rects => {
                        let v = take(rest, &mut i, &inline, other)?;
                        let r: Vec<i64> = v.split(',').map(|n| n.trim().parse::<i64>()).collect::<Result<_, _>>().map_err(|_| format!("--{other} wants L,T,R,B, got '{v}'"))?;
                        if r.len() != 4 {
                            return Err(format!("--{other} wants L,T,R,B, got '{v}'"));
                        }
                        let mut all = args.remove(&name).unwrap_or_else(|| json!([]));
                        if let Some(a) = all.as_array_mut() {
                            a.push(json!(r));
                        }
                        all
                    }
                    _ => json!(take(rest, &mut i, &inline, other)?),
                };
                args.insert(name, v);
            }
        }
    }
    let mut words = words.into_iter();
    let positional: Vec<&api::Param> = if wait { vec![] } else { cmd.params.iter().filter(|p| p.positional).collect() };
    if wait {
        if let Some(w) = words.next() {
            if sels.contains_key("visible") {
                return Err("give the selector as flags or one string, not both".into());
            }
            sels.insert("visible".into(), json!(w));
        }
    }
    for p in positional {
        let Some(w) = words.next() else { break };
        if p.kind == Kind::Sel {
            if sels.contains_key(p.name) {
                return Err("give the selector as flags or one string, not both".into());
            }
            sels.insert(p.name.into(), json!(w));
        } else if matches!(p.kind, Kind::Int | Kind::Num) {
            args.insert(p.name.into(), number(p.name, &w)?);
        } else {
            args.insert(p.name.into(), json!(w));
        }
    }
    if let Some(extra) = words.next() {
        return Err(format!("unexpected argument '{extra}'"));
    }
    for (k, v) in sels {
        match (wait, k.as_str()) {
            (true, "visible") => {
                let m = mode.ok_or("give --visible or --not-visible with the selector")?;
                args.insert(m.into(), v);
            }
            _ => {
                args.insert(k, v);
            }
        }
    }
    match (mode, &condition) {
        (Some(_), Some(_)) => return Err("extended-wait-until takes exactly one of --visible, --not-visible and --condition".into()),
        (Some(m), None) if !args.contains_key(m) => return Err(format!("--{} needs a selector: <text> | --id RE | --text RE | --css S", m.replace('_', "-"))),
        _ => {}
    }
    if let Some(c) = &condition {
        args.insert("condition".into(), json!(c));
    }
    Ok(Parsed { args, device, condition })
}

fn emit(v: Value) {
    println!("{v}");
}

fn usage_error(platform: &str, cmd: &Command, error: &str) -> i32 {
    emit(json!({ "ok": false, "command": cmd.verb(), "platform": platform, "error": error, "usage": api::usage(platform, cmd) }));
    2
}

/// `wf <platform> <verb>` for a verb neither the table nor the noun serves: a usage error that lists the verbs there are.
pub fn unknown_verb(platform: &str, verb: &str, own: &[&str]) -> i32 {
    let table = api::COMMANDS.iter().filter(|c| c.surface != api::Surface::LuaOnly && c.absent_on(platform).is_none()).map(|c| c.verb());
    let mut verbs: Vec<String> = table.chain(own.iter().map(|v| v.to_string())).collect();
    verbs.sort();
    emit(json!({ "ok": false, "command": verb, "platform": platform, "error": format!("unknown verb '{verb}'"), "verbs": verbs, "usage": format!("wf {platform} <verb> [options]"), "next": format!("wf {platform} --help lists each verb; wf {platform} <verb> --help") }));
    2
}

/// The only device of `all`: exit 2 when several are listed, 1 when none is.
pub fn one(all: Vec<String>, none: &str) -> Result<String, (i32, String)> {
    match all.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err((1, none.to_string())),
        many => Err((2, format!("several devices: pass --device, one of {}", many.join(", ")))),
    }
}

/// `wf <platform> <verb> ...`: one JSON line, exit 0 (done), 1 (failed), 2 (usage) or 3 (Android lease held).
pub fn run(target: &dyn crate::platform::Platform, verb: &str, rest: &[String]) -> i32 {
    let platform = target.name();
    let Some(cmd) = api::by_verb(verb) else { return 2 };
    let started = Instant::now();
    let parsed = match parse(cmd, rest) {
        Ok(p) => p,
        Err(e) => return usage_error(platform, cmd, &e),
    };
    if let Some(why) = cmd.absent_on(platform) {
        return usage_error(platform, cmd, &format!("not on {platform}: {why}"));
    }
    let (dev, name) = match target.pick(parsed.device.as_deref()) {
        Ok(d) => d,
        Err((2, e)) => return usage_error(platform, cmd, &e),
        Err((code, e)) => {
            emit(json!({ "ok": false, "command": verb, "platform": platform, "device": null, "ms": started.elapsed().as_millis() as u64, "error": e, "next": format!("open a device, or pass --device") }));
            return code;
        }
    };
    let out_dir = std::env::var_os("WF_TEST_OUTPUT_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("wf-evidence"));
    let goldens = Goldens::new(wf_kit::root::repo_root().join("docs/runbooks/goldens").join(platform), crate::golden::Mode::Compare, out_dir.clone());
    let cond = match &parsed.condition {
        Some(expr) => match super::lua::condition(dev.clone(), expr) {
            Ok(c) => Some(c),
            Err(e) => return usage_error(platform, cmd, &e),
        },
        None => None,
    };
    let ctx = Ctx { out_dir: Some(out_dir), goldens: Some(&goldens), ..Ctx::default() };
    let out = {
        let _span = crate::spans::begin("call", cmd.name, None);
        commands::call(&*dev, &ctx, cmd.name, &parsed.args, cond.as_ref().map(|c| c as &dyn Fn() -> Result<Option<Value>, String>))
    };
    let ms = started.elapsed().as_millis() as u64;
    match out {
        Ok(v) => {
            let result = if cmd.is_wait() { json!({ "met": !v.is_null(), "value": v, "ms": ms, "checks": ctx.checks.get() }) } else { v };
            emit(json!({ "ok": true, "command": verb, "platform": platform, "device": name, "ms": ms, "result": result }));
            0
        }
        Err(f) if f.usage => usage_error(platform, cmd, &f.error),
        Err(f) => {
            emit(json!({ "ok": false, "command": verb, "platform": platform, "device": name, "ms": ms, "error": f.error, "evidence": f.evidence, "verdict": f.verdict, "met_after_ms": f.met_after_ms, "next": f.next }));
            f.code.unwrap_or(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(verb: &str, argv: &[&str]) -> Result<Value, String> {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        parse(api::by_verb(verb).unwrap(), &argv).map(|p| json!({ "args": p.args, "device": p.device, "condition": p.condition }))
    }

    #[test]
    fn given_cli_flags_when_parsed_from_the_table_then_they_become_the_commands_args() {
        assert_eq!(parsed("tap-on", &["Continue", "--device", "s1", "--json"]).unwrap(), json!({ "args": { "sel": "Continue" }, "device": "s1", "condition": null }));
        assert_eq!(parsed("tap-on", &["--id", "Go", "--optional"]).unwrap()["args"], json!({ "optional": true, "sel": { "id": "Go" } }));
        assert_eq!(parsed("extended-wait-until", &["--not-visible", "--id", "Go", "--timeout", "500"]).unwrap()["args"], json!({ "timeout": 500, "not_visible": { "id": "Go" } }));
        assert_eq!(parsed("extended-wait-until", &["--visible", "Continue"]).unwrap()["args"], json!({ "visible": "Continue" }));
        let cond = parsed("extended-wait-until", &["--condition", "device:get_device_state('keyboard').visible", "--timeout=0"]).unwrap();
        assert_eq!((cond["args"]["timeout"].clone(), cond["condition"].clone()), (json!(0), json!("device:get_device_state('keyboard').visible")));
        assert_eq!(parsed("scroll-until-visible", &["--element-text", "Done", "--from-id", "EditorActionToolbar", "--direction", "RIGHT"]).unwrap()["args"], json!({ "direction": "RIGHT", "element": { "text": "Done" }, "from": { "id": "EditorActionToolbar" } }));
        assert_eq!(parsed("assert-screenshot", &["home", "--ignore", "0,0,10,10", "--ignore", "5,5,6,6", "--threshold-percentage", "99.5"]).unwrap()["args"], json!({ "path": "home", "ignore": [[0, 0, 10, 10], [5, 5, 6, 6]], "threshold_percentage": 99.5 }));
        assert_eq!(parsed("get-device-state", &["keyboard"]).unwrap()["args"], json!({ "part": "keyboard" }));
        assert_eq!(parsed("input-text", &["--text", "hello"]).unwrap()["args"], json!({ "text": "hello" }), "a param named like a selector field");
    }

    #[test]
    fn given_bad_cli_flags_when_parsed_then_each_is_named() {
        for (verb, argv, why) in [
            ("tap-on", vec!["--bogus"], "unknown option --bogus"),
            ("tap-on", vec!["a", "b"], "unexpected argument 'b'"),
            ("tap-on", vec!["Go", "--id", "Go"], "not both"),
            ("extended-wait-until", vec!["--id", "Go"], "--visible or --not-visible"),
            ("extended-wait-until", vec!["--visible"], "needs a selector"),
            ("get-pixel", vec!["--device"], "needs a value"),
            ("input-text", vec!["--id", "x"], "takes no --id"),
            ("extended-wait-until", vec!["--visible", "--condition", "true"], "exactly one of"),
            ("extended-wait-until", vec!["--not-visible", "--id", "Go", "--condition", "true"], "exactly one of"),
        ] {
            let e = parsed(verb, &argv).unwrap_err();
            assert!(e.contains(why), "{verb} {argv:?}: {e}");
        }
        for old in ["--name", "--serial", "--identifier"] {
            assert_eq!(parsed("tap-on", &["Go", old, "e1"]).unwrap_err(), format!("unknown option {old}"), "an old flag is an unknown one");
        }
        assert!(is_verb("tap-on") && is_verb("swipe") && !is_verb("add-network-stub") && !is_verb("tap"));
    }
}
