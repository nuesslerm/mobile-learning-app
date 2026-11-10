//! `wf vm logs [--service <name>]`: a one-shot BetterStack read through the ClickHouse HTTP interface (basic auth, SQL as
//! the raw body). BetterStack keeps only the newest rows in `remote(<source>_logs)` and moves older ones to
//! `s3Cluster(primary, <source>_s3)`, so every read is the union of both.
use wf_kit::args::parse;
use regex::Regex;
use serde_json::{Map, Value};

const SOURCES: &[&str] = &["backend", "mobile"];

/// A failure: message, exit code, and whether the TypeScript raised a plain `Error` (no `wf:` prefix) or a WfError.
pub(crate) struct Fail(pub(crate) String, pub(crate) i32, pub(crate) bool);

fn wf_error(message: impl Into<String>) -> Fail {
    Fail(message.into(), 1, false)
}

pub(crate) use wf_vmops::betterstack::{rows_source, Config};

pub(crate) fn load_config(source: &str) -> Result<Config, Fail> {
    wf_vmops::betterstack::load_config(source).map_err(wf_error)
}

pub(crate) fn query(cfg: &Config, sql: &str) -> Result<Vec<Vec<String>>, Fail> {
    wf_vmops::betterstack::query(cfg, sql).map_err(wf_error)
}

/// `--since 5m` into a ClickHouse INTERVAL; a bare `m` would silently widen the window, so units are spelled out.
fn parse_since(s: &str) -> Result<String, String> {
    let re = Regex::new(r"^(\d+)(s|m|h|d)$").unwrap();
    let Some(c) = re.captures(s.trim()) else {
        return Err(format!(
            "Invalid --since \"{s}\" (expected a duration like \"5m\", \"1h\", \"30s\", \"2d\")"
        ));
    };
    let unit = match &c[2] {
        "s" => "SECOND",
        "m" => "MINUTE",
        "h" => "HOUR",
        _ => "DAY",
    };
    Ok(format!("INTERVAL {} {unit}", &c[1]))
}

fn level_filter(level: Option<&str>) -> Result<String, String> {
    let Some(level) = level.filter(|l| !l.is_empty()) else {
        return Ok(String::new());
    };
    let ranks = ["debug", "info", "warn", "error"];
    let Some(min) = ranks.iter().position(|r| *r == level) else {
        return Err(format!(
            "Unknown --level \"{level}\" (expected debug|info|warn|error)"
        ));
    };
    let allowed: Vec<String> = ranks[min..].iter().map(|l| format!("'{l}'")).collect();
    Ok(format!(
        " AND lower(JSONExtractString(raw,'level')) IN ({})",
        allowed.join(", ")
    ))
}

// Must mirror extract_module below, or --module filters out what the column displays.
const MODULE_SQL: &str = "coalesce(nullIf(JSONExtractString(raw,'module'), ''), nullIf(JSONExtractString(raw,'attributes','source'), ''), nullIf(JSONExtractString(raw,'source'), ''), nullIf(JSONExtractString(raw,'scope','name'), ''), nullIf(JSONExtractString(raw,'resource','attributes','service.name'), ''), '')";
const APP_SQL: &str = "coalesce(nullIf(JSONExtractString(raw,'app'), ''), nullIf(JSONExtractString(raw,'resource','attributes','service.name'), ''), '')";

fn quote_escape(s: &str) -> String {
    s.replace('\'', "\\'")
}

/// `String(Number(s))`.
fn js_number(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return "0".into();
    }
    match t.parse::<f64>() {
        Ok(n) if n.is_finite() && n.fract() == 0.0 => format!("{}", n as i64),
        Ok(n) if n.is_finite() => format!("{n}"),
        _ => "NaN".into(),
    }
}

struct Query<'a> {
    since: &'a str,
    level: Option<&'a str>,
    module: Option<&'a str>,
    grep: Option<&'a str>,
    limit: String,
    app: Option<&'a str>,
}

fn build_sql(table: &str, q: &Query) -> Result<String, String> {
    let mut filter = format!("dt > now() - {}", parse_since(q.since)?);
    if let Some(app) = q.app.filter(|a| !a.is_empty()) {
        filter.push_str(&format!(" AND {APP_SQL} = '{}'", quote_escape(app)));
    }
    filter.push_str(&level_filter(q.level)?);
    if let Some(m) = q.module.filter(|m| !m.is_empty()) {
        filter.push_str(&format!(" AND {MODULE_SQL} = '{}'", quote_escape(m)));
    }
    if let Some(g) = q.grep.filter(|g| !g.is_empty()) {
        let escaped = quote_escape(&g.replace('\\', "\\\\"));
        filter.push_str(&format!(
            " AND lowerUTF8(CAST(JSONExtractString(raw,'message') AS String)) LIKE lowerUTF8('%{escaped}%')"
        ));
    }
    Ok(format!(
        "SELECT formatDateTime(dt, '%Y-%m-%dT%H:%i:%S.%fZ') AS ts, raw FROM {} ORDER BY dt DESC LIMIT {} FORMAT TabSeparatedWithNames",
        rows_source(table, "dt, raw", &filter),
        q.limit
    ))
}


fn extract_module(obj: &Map<String, Value>) -> String {
    let non_empty = |v: Option<&Value>| {
        v.and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    non_empty(obj.get("module"))
        .or_else(|| {
            non_empty(
                obj.get("attributes")
                    .and_then(Value::as_object)
                    .and_then(|a| a.get("source")),
            )
        })
        .or_else(|| non_empty(obj.get("source")))
        .or_else(|| {
            non_empty(
                obj.get("scope")
                    .and_then(Value::as_object)
                    .and_then(|s| s.get("name")),
            )
        })
        .or_else(|| {
            non_empty(
                obj.get("resource")
                    .and_then(Value::as_object)
                    .and_then(|r| r.get("attributes"))
                    .and_then(Value::as_object)
                    .and_then(|a| a.get("service.name")),
            )
        })
        .unwrap_or_else(|| "?".into())
}

const ENVELOPE: &[&str] = &[
    "dt",
    "level",
    "module",
    "message",
    "app",
    "platform",
    "attributes",
    "scope",
    "resource",
    "severity_text",
    "severity_number",
];

/// One log line: `<ts>  <LEVEL> <module> <message>  key=value ...`.
fn format_row(raw: &str, ts: &str) -> String {
    let (level, module, message, attrs) = match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(obj)) => {
            let mut attrs: Map<String, Value> = Map::new();
            for (k, v) in &obj {
                if !ENVELOPE.contains(&k.as_str()) {
                    attrs.insert(k.clone(), v.clone());
                }
            }
            // OTLP rows nest their structured metadata inside `attributes`.
            if let Some(Value::Object(nested)) = obj.get("attributes") {
                for (k, v) in nested {
                    attrs.insert(k.clone(), v.clone());
                }
            }
            (
                obj.get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string(),
                extract_module(&obj),
                obj.get("message")
                    .and_then(Value::as_str)
                    .map_or_else(|| raw.to_string(), str::to_string),
                attrs,
            )
        }
        _ => ("?".into(), "?".into(), raw.to_string(), Map::new()),
    };
    let ts = ts.replacen('T', " ", 1).replacen('Z', "", 1);
    let base = format!(
        "{ts}  {} {} {message}",
        pad(&level.to_uppercase(), 5),
        pad(&module, 24)
    );
    if attrs.is_empty() {
        return base;
    }
    let attrs: Vec<String> = attrs
        .iter()
        .map(|(k, v)| {
            format!(
                "{k}={}",
                v.as_str().map_or_else(|| v.to_string(), str::to_string)
            )
        })
        .collect();
    format!("{base}  {}", attrs.join(" "))
}

fn pad(s: &str, width: usize) -> String {
    format!("{s}{}", " ".repeat(width.saturating_sub(s.chars().count())))
}

fn run_inner(args: &[String]) -> Result<Option<i32>, Fail> {
    let Some(p) = parse(
        args,
        &["since", "level", "module", "grep", "limit", "source", "service"],
        &[],
    ) else {
        return Ok(None);
    };
    if !p.positional.is_empty() {
        return Ok(None);
    }
    let source = match p.opts.get("source") {
        Some(s) if !SOURCES.contains(&s.as_str()) => {
            return Err(wf_error(format!(
                "Unknown --source \"{s}\". Valid sources: {}.",
                SOURCES.join(", ")
            )));
        }
        Some(s) => s.as_str(),
        None => "backend",
    };
    let cfg = load_config(source)?;
    let q = Query {
        since: p.opts.get("since").map_or("10m", String::as_str),
        level: p.opts.get("level").map(String::as_str),
        module: p.opts.get("module").map(String::as_str),
        grep: p.opts.get("grep").map(String::as_str),
        limit: p
            .opts
            .get("limit")
            .map_or("200".to_string(), |l| js_number(l)),
        app: p.opts.get("service").map(String::as_str),
    };
    let sql = build_sql(&cfg.table, &q).map_err(|m| Fail(m, 1, true))?;
    for row in query(&cfg, &sql)? {
        println!(
            "{}",
            format_row(
                row.get(1).map_or("", String::as_str),
                row.first().map_or("", String::as_str)
            )
        );
    }
    Ok(Some(0))
}

pub fn run(args: &[String]) -> Option<i32> {
    match run_inner(args) {
        Ok(code) => code,
        // A plain `Error` in the TypeScript (bad --since or --level) is reported without the `wf:` prefix.
        Err(Fail(message, code, plain)) => {
            if plain {
                eprintln!("Error: {message}");
            } else {
                wf_kit::wf_err!("wf: {message}");
            }
            Some(code)
        }
    }
}
