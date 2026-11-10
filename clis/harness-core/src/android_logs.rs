//! Logs instead of breakpoints on the device: an EAS Update has no Metro to attach a debugger through, so an Android run carries a
//! run id into the app (the launch URL's `wfRun` query), the app puts it on every line it forwards to BetterStack, and after the
//! run this module reads those lines back, maps the stacks of crashes and errors through the update's source maps, and hands the
//! report one section. `wf android stack <group>` is the stack mapping on its own.
use regex::Regex;
use serde_json::{json, Map, Value};
use sourcemap::SourceMap;
use std::time::{Duration, Instant};

/// The launch-URL query the app reads its run id from (`frontend/mla-tamagui/app-shell/runId.ts`).
pub const RUN_QUERY: &str = "wfRun";
/// The env var the harness sets for the run, which `open_link` reads.
const RUN_ENV: &str = "WF_APP_RUN_ID";
const MAPS_PREFIX: &str = "verification/eas-updates";
/// How long to wait for BetterStack to ingest the run's lines, in seconds.
const WAIT_ENV: &str = "WF_ANDROID_LOGS_WAIT_S";
const DEFAULT_WAIT_S: u64 = 30;
const POLL: Duration = Duration::from_secs(4);
const SHOWN_INFO: usize = 200;

/// A run id the app can carry in a URL and a SQL string: letters, digits and dashes.
pub fn run_id_for(stem: &str, now_ms: i64) -> String {
    let clean: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    format!("{clean}-{now_ms}")
}

pub fn start_run(id: &str) {
    std::env::set_var(RUN_ENV, id);
}

pub fn run_id() -> Option<String> {
    std::env::var(RUN_ENV).ok().filter(|v| !v.is_empty())
}

/// The link with the run id in its query, before any fragment.
pub fn link_with_run_id(link: &str) -> String {
    let Some(id) = run_id() else { return link.to_string() };
    let (head, fragment) = link.split_once('#').map_or((link, ""), |(h, f)| (h, f));
    let joined = format!("{head}{}{RUN_QUERY}={id}", if head.contains('?') { '&' } else { '?' });
    if fragment.is_empty() {
        joined
    } else {
        format!("{joined}#{fragment}")
    }
}

/// The update group a link or `--env base=` names: `exp://u.expo.dev/<project>/group/<group>/--`.
pub fn group_of(link: &str) -> Option<String> {
    Regex::new(r"/group/([A-Za-z0-9-]+)").unwrap().captures(link).map(|c| c[1].to_string())
}

fn valid_group(group: &str) -> bool {
    !group.is_empty() && group.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The group's source map for a platform, from the bucket `wf ship eas` uploaded it to (the group's summary.json lists its files).
fn fetch_map(group: &str, platform: &str) -> Result<(SourceMap, String), String> {
    if !valid_group(group) {
        return Err(format!("'{group}' is not an update group id (letters, digits and dashes)"));
    }
    let base = format!("{}/{MAPS_PREFIX}/{group}", wf_bucket::artifacts::public_base());
    let summary = wf_bucket::artifacts::read_public(&format!("{base}/summary.json"))?.ok_or_else(|| format!("the bucket has no summary for update group {group}: it was published before source maps were stored, or by a build that stored none"))?;
    let summary: Value = serde_json::from_str(&summary).map_err(|e| format!("summary.json of group {group} is not JSON: {e}"))?;
    let files = summary["maps"]["files"].as_array().ok_or_else(|| format!("group {group} stored no source maps: {}", summary["maps"]["error"].as_str().unwrap_or("its summary lists none")))?;
    let rel = files
        .iter()
        .filter_map(Value::as_str)
        .find(|f| f.ends_with(".map") && f.contains(&format!("/{platform}/")))
        .ok_or_else(|| format!("group {group} has no {platform} source map among {} files", files.len()))?;
    let text = wf_bucket::artifacts::read_public(&format!("{base}/{rel}"))?.ok_or_else(|| format!("{base}/{rel} is listed in the summary but not in the bucket"))?;
    let map = crate::browser_debug::parse_map(text.as_bytes()).ok_or_else(|| format!("{rel} is not a source map"))?;
    Ok((map, rel.to_string()))
}

/// One stack line mapped to its source: Hermes prints `at name (address at bundle:LINE:COL)`, columns counted from 1.
fn map_frame(map: &SourceMap, raw: &str) -> Value {
    let located = Regex::new(r":(\d+):(\d+)\)?\s*$").unwrap().captures(raw.trim_end());
    let token = located.and_then(|c| {
        let (line, col): (u32, u32) = (c[1].parse().ok()?, c[2].parse().ok()?);
        map.lookup_token(line.checked_sub(1)?, col.saturating_sub(1))
    });
    match token.and_then(|t| Some((t.get_source()?.to_string(), t.get_src_line() + 1, t.get_src_col() + 1, t.get_name().map(str::to_string)))) {
        Some((source, line, column, name)) => json!({ "raw": raw, "mapped": true, "source": source, "line": line, "column": column, "name": name }),
        None => json!({ "raw": raw, "mapped": false }),
    }
}

fn map_stack(map: &SourceMap, stack: &str) -> Vec<Value> {
    stack.lines().filter(|l| !l.trim().is_empty()).map(|l| map_frame(map, l)).collect()
}

/// A mapped frame as one text line: `name (source:line:column)`; an unmapped one stays as printed.
fn frame_text(frame: &Value) -> String {
    match (frame["mapped"].as_bool(), frame["source"].as_str()) {
        (Some(true), Some(source)) => format!("{} ({source}:{}:{})", frame["name"].as_str().unwrap_or("?"), frame["line"], frame["column"]),
        _ => frame["raw"].as_str().unwrap_or_default().trim().to_string(),
    }
}

/// `wf android stack <group> [--platform P] [--file F]`: the stack on stdin (or in F) mapped through the group's source map.
pub fn stack(group: &str, platform: &str, file: Option<&str>) -> Result<Value, String> {
    use std::io::{IsTerminal, Read};
    let text = match file {
        Some(f) => std::fs::read_to_string(f).map_err(|e| format!("cannot read {f}: {e}"))?,
        None if std::io::stdin().is_terminal() => return Err("give the stack on stdin or with --file".to_string()),
        None => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).map_err(|e| format!("cannot read stdin: {e}"))?;
            text
        }
    };
    let (map, rel) = fetch_map(group, platform)?;
    let frames = map_stack(&map, &text);
    let text = frames.iter().map(frame_text).collect::<Vec<_>>().join("\n");
    Ok(json!({ "group": group, "platform": platform, "map": rel, "frames": frames, "text": text }))
}

/// The stack a log line carries: a `stack` field, or the `stack` of its `error` field (an Error serialized) or the error text itself.
fn stack_of(fields: &Map<String, Value>) -> Option<String> {
    let stack = fields.get("stack").and_then(Value::as_str).or_else(|| fields.get("error").and_then(|e| e["stack"].as_str().or_else(|| e.as_str())));
    stack.filter(|s| s.lines().count() > 1 || s.contains(" at ")).map(str::to_string)
}

/// One BetterStack row `[ts, raw JSON]` as a log line; the run's own envelope fields stay out of `fields`.
fn line_of(row: &[String]) -> Value {
    let raw = row.get(1).map_or("", String::as_str);
    let parsed: Map<String, Value> = serde_json::from_str(raw).unwrap_or_default();
    let text = |k: &str| parsed.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let message = if parsed.is_empty() { raw.to_string() } else { text("message") };
    let level = text("level").to_lowercase();
    let fields: Map<String, Value> = parsed.iter().filter(|(k, _)| !["dt", "level", "message", "app", "platform", "runId"].contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
    json!({ "ts": row.first().cloned().unwrap_or_default(), "level": level, "message": message, "crash": message == "app.crash", "stack": stack_of(&fields), "fields": fields })
}

fn rank(line: &Value) -> u8 {
    match (line["crash"].as_bool(), line["level"].as_str()) {
        (Some(true), _) => 0,
        (_, Some("error")) => 1,
        (_, Some("warn")) => 2,
        _ => 3,
    }
}

/// The run's lines from BetterStack with crashes first, then errors, warnings and the rest in time order; crash and error
/// stacks mapped through the update's source map when the run loaded an update group. Waits a bounded time for ingest: a
/// forwarder posts once a second and BetterStack takes a moment, so the read repeats until two reads in a row agree.
pub fn collect(run_id: &str, group: Option<&str>, platform: &str) -> Value {
    let wait = Duration::from_secs(std::env::var(WAIT_ENV).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(DEFAULT_WAIT_S));
    let base = json!({ "runId": run_id, "group": group });
    let merge = |extra: Value| -> Value {
        let mut out = base.clone();
        out.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());
        out
    };
    let cfg = match wf_vmops::betterstack::load_config("mobile") {
        Ok(c) => c,
        Err(e) => return merge(json!({ "lines": [], "note": format!("the run's log lines were not read: {}", e.lines().next().unwrap_or_default()) })),
    };
    let (started, mut last) = (Instant::now(), None::<usize>);
    let (rows, settled) = loop {
        let rows = match wf_vmops::betterstack::run_lines(&cfg, run_id, 6 * 3600) {
            Ok(r) => r,
            Err(e) => return merge(json!({ "lines": [], "note": format!("the run's log lines were not read: {}", e.lines().next().unwrap_or_default()) })),
        };
        let stable = !rows.is_empty() && last == Some(rows.len());
        if stable || started.elapsed() >= wait {
            break (rows, stable);
        }
        last = Some(rows.len());
        std::thread::sleep(POLL);
    };
    let mut lines: Vec<Value> = rows.iter().map(|r| line_of(r)).collect();
    let mut map_note = None;
    if let Some(group) = group {
        if lines.iter().any(|l| l["stack"].is_string()) {
            match fetch_map(group, platform) {
                Ok((map, _)) => {
                    for l in lines.iter_mut().filter(|l| l["stack"].is_string()) {
                        let frames = map_stack(&map, l["stack"].as_str().unwrap_or_default());
                        l["mappedStack"] = json!(frames.iter().map(frame_text).collect::<Vec<_>>().join("\n"));
                    }
                }
                Err(e) => map_note = Some(format!("stacks stay as logged: {e}")),
            }
        }
    } else if lines.iter().any(|l| l["stack"].is_string()) {
        map_note = Some("stacks stay as logged: the run did not load an update group (no --env base=exp://.../group/<group>)".to_string());
    }
    lines.sort_by_key(rank);
    let note = match (rows.len(), settled) {
        (0, _) => format!("no line carried this run's id after {}s: the app may not have started with the id, the build may carry no BetterStack ingest pair, or ingest is slow, so lines may be missing", wait.as_secs()),
        (n, true) => format!("{n} lines, the same on two reads in a row; a line that arrives later would be missing"),
        (n, false) => format!("{n} lines when the {}s wait for ingest ended with the count still changing: later lines may be missing", wait.as_secs()),
    };
    merge(json!({ "lines": lines, "note": note, "mapNote": map_note }))
}

/// The report's section: the note, then crashes with mapped stacks, errors and warnings, then up to `SHOWN_INFO` other lines.
pub fn html(logs: &Value) -> String {
    let esc = crate::golden::esc;
    let lines = logs["lines"].as_array().map_or(&[][..], Vec::as_slice);
    let mut body = format!("<p>run <code>{}</code>{}</p><p class=\"none\">{}</p>", esc(logs["runId"].as_str().unwrap_or_default()), logs["group"].as_str().map(|g| format!(", update group <code>{}</code>", esc(g))).unwrap_or_default(), esc(logs["note"].as_str().unwrap_or_default()));
    if let Some(n) = logs["mapNote"].as_str() {
        body.push_str(&format!("<p class=\"none\">{}</p>", esc(n)));
    }
    let (mut info_shown, mut hidden) = (0, 0);
    let mut items = String::new();
    for l in lines {
        if rank(l) == 3 {
            if info_shown >= SHOWN_INFO {
                hidden += 1;
                continue;
            }
            info_shown += 1;
        }
        let fields = l["fields"].as_object().map(|o| o.iter().map(|(k, v)| format!("{k}={}", v.as_str().map_or_else(|| v.to_string(), str::to_string))).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        let stack = l["mappedStack"].as_str().or_else(|| l["stack"].as_str()).map(|s| format!("<pre>{}</pre>", esc(s))).unwrap_or_default();
        let class = if rank(l) <= 1 { "bad" } else if rank(l) == 2 { "warn" } else { "" };
        items.push_str(&format!("<li class=\"{class}\"><code>{} {} {}</code> {}{stack}</li>", esc(l["ts"].as_str().unwrap_or_default()), esc(&l["level"].as_str().unwrap_or_default().to_uppercase()), esc(l["message"].as_str().unwrap_or_default()), esc(&fields)));
    }
    if hidden > 0 {
        items.push_str(&format!("<li>{hidden} more info and debug lines are in report.json</li>"));
    }
    body.push_str(&format!("<ul class=\"warn\">{items}</ul>"));
    format!("<section><h2>App logs <span class=\"count\">{}</span></h2>{body}</section>", lines.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_with_run_id_goes_before_the_fragment_and_joins_an_existing_query() {
        // Given a run id and links with no query, a query and a fragment
        start_run("g1-5");
        // When the harness opens them
        // Then the id is one more query parameter, ahead of the fragment
        assert_eq!(link_with_run_id("exp://h/g/--/leetcode"), "exp://h/g/--/leetcode?wfRun=g1-5");
        assert_eq!(link_with_run_id("exp://h/g/--/x?a=1#top"), "exp://h/g/--/x?a=1&wfRun=g1-5#top");
        std::env::remove_var(RUN_ENV);
    }

    #[test]
    fn report_section_lists_crashes_first_with_the_mapped_stack_and_says_when_lines_may_be_missing() {
        // Given a run whose info line came first and whose crash has a mapped stack
        let logs = json!({ "runId": "r1", "group": "g", "note": "2 lines when the 30s wait for ingest ended with the count still changing: later lines may be missing", "lines": [
            { "ts": "t2", "level": "error", "message": "app.crash", "crash": true, "fields": {}, "stack": "raw", "mappedStack": "boom (features/a.ts:3:9)" },
            { "ts": "t1", "level": "info", "message": "app.boot", "crash": false, "fields": { "source": "AppShell" } } ] });
        // When the section renders
        let page = html(&logs);
        // Then the crash is listed before the info line, with its mapped stack, and the ingest caveat is shown
        assert!(page.find("app.crash").unwrap() < page.find("app.boot").unwrap());
        assert!(page.contains("boom (features/a.ts:3:9)") && page.contains("later lines may be missing"));
    }
}
