//! The TypeScript `logfmt` line format (`lib/logfmt.ts`): `ts=<epoch s> level=<l> source=<s> msg="<m>" k=v ...`.
use crate::time::now_ms;

fn value(v: &str) -> String {
    if v.contains([' ', '"', '\\']) {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

pub fn line(level: &str, source: &str, msg: &str, fields: &[(&str, String)]) -> String {
    let msg = msg.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = format!("ts={} level={level} source={source} msg=\"{msg}\"", now_ms() / 1000);
    for (k, v) in fields {
        out.push_str(&format!(" {k}={}", value(v)));
    }
    out
}

/// `logfmt.mcp.*`: every level goes to stderr, because stdout may carry the protocol or the result.
pub fn mcp(level: &str, source: &str, msg: &str, fields: &[(&str, String)]) {
    eprintln!("{}", line(level, source, msg, fields));
}

/// One line, `info` on stdout and every other level on stderr.
pub fn write(level: &str, source: &str, msg: &str, fields: &[(&str, String)]) {
    let line = line(level, source, msg, fields);
    if level == "info" {
        println!("{line}");
    } else {
        eprintln!("{line}");
    }
}

pub fn info(source: &str, msg: &str, fields: &[(&str, String)]) {
    write("info", source, msg, fields);
}
