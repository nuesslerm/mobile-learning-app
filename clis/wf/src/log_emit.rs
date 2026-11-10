//! `wf log emit`: the shell scripts' logger (`telemetry::cli`, also the static `logger` binary) under wf's argument parser.
use wf_kit::args::{parse_spec, Positional, Spec};
use ::telemetry::cli::{self, Emit};

pub fn run(args: &[String]) -> Option<i32> {
    let spec = Spec { values: &["service", "message", "level", "host-label", "attr"], required: &["service", "message"], positional: Positional::None, ..Spec::NONE };
    let p = parse_spec(args, &spec)?;
    let attrs = p.values("attr").iter().map(|pair| pair.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))).collect::<Option<_>>()?;
    let level = match cli::level(p.val("level").unwrap_or("info")) {
        Ok(level) => level,
        Err(why) => {
            eprintln!("[logger] {why}");
            return Some(1);
        }
    };
    Some(cli::run(Emit {
        service: p.val("service")?.to_string(),
        message: p.val("message")?.to_string(),
        level,
        host_label: p.val("host-label").map(str::to_string),
        attrs,
    }))
}
