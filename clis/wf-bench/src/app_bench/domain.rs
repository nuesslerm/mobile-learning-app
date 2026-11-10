//! The pure parts of the app benchmarks: statistics, baseline recording, export-log parsing, A/B summaries and
//! the attribution of executed script bytes to source files and packages through a source map.
use regex::Regex;
use serde_json::{json, Map, Value};
use sourcemap::SourceMap;

/// A JSON number that prints as JavaScript would: whole values without a fractional part.
pub fn jnum(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9.0e15 {
        Value::from(x as i64)
    } else {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

pub fn median(values: &[f64]) -> Result<f64, String> {
    if values.is_empty() {
        return Err("median of no values".into());
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mid = sorted.len() / 2;
    Ok(if sorted.len() % 2 == 1 { sorted[mid] } else { (sorted[mid - 1] + sorted[mid]) / 2.0 })
}

pub fn summarize(unit: &str, values: &[f64]) -> Result<Value, String> {
    let med = median(values)?;
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Ok(json!({ "unit": unit, "median": jnum(med), "min": jnum(min), "max": jnum(max), "values": values.iter().map(|v| jnum(*v)).collect::<Vec<_>>() }))
}

/// Relative spread of a metric: `(max - min) / median`, zero for a zero median.
pub fn spread_ratio(metric: &Value) -> f64 {
    let med = metric["median"].as_f64().unwrap_or(0.0);
    if med == 0.0 {
        0.0
    } else {
        (metric["max"].as_f64().unwrap_or(0.0) - metric["min"].as_f64().unwrap_or(0.0)) / med.abs()
    }
}

/// `initial` is the reference for "half the memory": written once, never replaced.
pub fn with_recorded(file: &Value, mode: &str, snapshot: &Value, suite: &str) -> Result<Value, String> {
    let has_initial = file["initial"].is_object();
    let new_metrics = |snap: &Value| -> Vec<String> {
        snap["metrics"]
            .as_object()
            .map(|m| m.keys().filter(|k| !file["initial"]["metrics"].as_object().is_some_and(|i| i.contains_key(*k))).cloned().collect())
            .unwrap_or_default()
    };
    match mode {
        "none" => Ok(file.clone()),
        "both" => {
            let base = if !has_initial || !new_metrics(snapshot).is_empty() { with_recorded(file, "initial", snapshot, suite)? } else { file.clone() };
            with_recorded(&base, "current", snapshot, suite)
        }
        "initial" => {
            let mut out = file.as_object().cloned().unwrap_or_default();
            if !has_initial {
                let current = if file["current"].is_object() { file["current"].clone() } else { snapshot.clone() };
                out.insert("initial".into(), snapshot.clone());
                out.insert("current".into(), current);
                return Ok(Value::Object(out));
            }
            let added = new_metrics(snapshot);
            if added.is_empty() {
                return Err(format!("baseline for {suite} already has an initial block; it is never overwritten"));
            }
            let mut initial = file["initial"].as_object().cloned().unwrap_or_default();
            let mut metrics = file["initial"]["metrics"].as_object().cloned().unwrap_or_default();
            for name in added {
                metrics.insert(name.clone(), snapshot["metrics"][&name].clone());
            }
            initial.insert("metrics".into(), Value::Object(metrics));
            out.insert("initial".into(), Value::Object(initial));
            Ok(Value::Object(out))
        }
        _ => {
            if !has_initial {
                return Err(format!("record the initial baseline for {suite} before a current one"));
            }
            let mut out = file.as_object().cloned().unwrap_or_default();
            out.insert("current".into(), snapshot.clone());
            Ok(Value::Object(out))
        }
    }
}

pub fn strip_ansi(text: &str) -> String {
    Regex::new("\x1b\\[[0-9;]*m").map_or_else(|_| text.to_string(), |re| re.replace_all(text, "").into_owned())
}

/// `|  🐥 [tamagui]  native charts-spike  ·  2 found  ·  0 opt  ·  0 flat  1577ms`: the plugin's time summed.
pub fn tamagui_plugin_ms(log: &str) -> u64 {
    let ms = Regex::new(r"(\d+)ms\s*$").unwrap_or_else(|_| unreachable!());
    strip_ansi(log)
        .split('\n')
        .filter(|l| l.contains("[tamagui]") && l.contains(" found"))
        .filter_map(|l| ms.captures(l).and_then(|c| c[1].parse::<u64>().ok()))
        .sum()
}

/// `Web Bundled 143489ms index.js (5682 modules)` and `λ Bundled 141439ms ...`, summed.
pub fn metro_bundle_ms(log: &str) -> u64 {
    let re = Regex::new(r"^(?:λ |Web |iOS |Android )?Bundled (\d+)ms").unwrap_or_else(|_| unreachable!());
    strip_ansi(log).split('\n').filter_map(|l| re.captures(js_trim(l)).and_then(|c| c[1].parse::<u64>().ok())).sum()
}

pub fn export_finished(log: &str) -> bool {
    strip_ansi(log).split('\n').any(|l| l.starts_with("Exported: "))
}

/// `iOS Bundled 34444ms frontend/mla-tamagui/index.js (4227 modules)`: (ms, module count) of one platform's bundle.
pub fn platform_bundle(log: &str, platform: &str) -> (u64, u64) {
    let label = match platform {
        "web" => "Web",
        "ios" => "iOS",
        "android" => "Android",
        other => other,
    };
    let Ok(re) = Regex::new(&format!(r"^{} Bundled (\d+)ms .*\((\d+) modules\)", regex::escape(label))) else { return (0, 0) };
    for line in strip_ansi(log).split('\n') {
        if let Some(c) = re.captures(js_trim(line)) {
            return (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
        }
    }
    (0, 0)
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

pub struct AbResult {
    pub baseline_median: f64,
    pub candidate_median: f64,
    /// candidate / baseline: below 1 is faster.
    pub ratio: f64,
    /// In how many pairs the candidate was faster than the baseline run next to it.
    pub candidate_wins: usize,
}

pub fn summarize_ab(baseline: &[f64], candidate: &[f64]) -> Result<AbResult, String> {
    let (b, c) = (median(baseline)?, median(candidate)?);
    Ok(AbResult {
        baseline_median: b,
        candidate_median: c,
        ratio: c / b,
        candidate_wins: candidate.iter().enumerate().filter(|(i, ms)| **ms < baseline.get(*i).copied().unwrap_or(f64::INFINITY)).count(),
    })
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Segment {
    pub column: i64,
    /// Source index, -1 for none.
    pub source: i64,
}

/// Per generated line (`line_count` of them), the segments with an absolute column and the source index they belong to (-1: none).
pub fn decode_lines(map: &SourceMap, line_count: usize) -> Vec<Vec<Segment>> {
    let mut lines: Vec<Vec<Segment>> = vec![Vec::new(); line_count];
    for token in map.tokens() {
        if let Some(line) = lines.get_mut(token.get_dst_line() as usize) {
            let source = if token.has_source() { i64::from(token.get_src_id()) } else { -1 };
            line.push(Segment { column: i64::from(token.get_dst_col()), source });
        }
    }
    lines
}

/// Package a source path belongs to: the segment after the last node_modules, scope included; else first-party.
pub fn package_of(source: &str) -> String {
    let marker = "node_modules/";
    let Some(at) = source.rfind(marker) else { return "first-party".into() };
    let rest: Vec<&str> = source[at + marker.len()..].split('/').collect();
    let first = rest.first().copied().unwrap_or("");
    if first.starts_with('@') {
        format!("{first}/{}", rest.get(1).copied().unwrap_or(""))
    } else {
        first.to_string()
    }
}

/// Characters of `code` that ran, per source file. Each mapping segment owns the text up to the next segment; text
/// before the first segment of a line, or in a segment without a source, counts as "unmapped". Offsets are UTF-16
/// code units, as V8 reports them and as source maps count columns.
pub fn executed_bytes_by_source(code: &str, map: &SourceMap, executed: &[(usize, usize)]) -> Vec<(String, u64)> {
    let mut ranges = executed.to_vec();
    ranges.sort_by_key(|r| r.0);
    let units: Vec<u16> = code.encode_utf16().collect();
    let lines = decode_lines(map, units.iter().filter(|u| **u == u16::from(b'\n')).count() + 1);
    let mut totals: Vec<(String, u64)> = Vec::new();
    let mut first = 0usize;
    let mut overlap = |from: usize, to: usize| -> u64 {
        while first < ranges.len() && ranges[first].1 <= from {
            first += 1;
        }
        let mut total = 0usize;
        for r in &ranges[first..] {
            if r.0 >= to {
                break;
            }
            total += r.1.min(to).saturating_sub(r.0.max(from));
        }
        total as u64
    };
    let mut add = |name: &str, bytes: u64| {
        if bytes > 0 {
            match totals.iter_mut().find(|(n, _)| n == name) {
                Some(entry) => entry.1 += bytes,
                None => totals.push((name.to_string(), bytes)),
            }
        }
    };
    let mut line_start = 0usize;
    for segments in &lines {
        let line_end = units[line_start.min(units.len())..].iter().position(|u| *u == b'\n' as u16).map_or(units.len(), |p| line_start + p);
        let mut cursor = line_start;
        let mut current = "unmapped".to_string();
        for seg in segments {
            let at = (line_start as i64 + seg.column).max(0) as usize;
            add(&current, overlap(cursor, at));
            cursor = at;
            current = u32::try_from(seg.source).ok().and_then(|i| map.get_source(i)).map_or_else(|| "unmapped".to_string(), str::to_string);
        }
        add(&current, overlap(cursor, line_end));
        line_start = line_end + 1;
    }
    totals
}

pub fn executed_bytes_by_package(by_source: &[(String, u64)]) -> Vec<(String, u64)> {
    let mut totals: Vec<(String, u64)> = Vec::new();
    for (source, bytes) in by_source {
        let name = if source == "unmapped" { source.clone() } else { package_of(source) };
        match totals.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 += bytes,
            None => totals.push((name, *bytes)),
        }
    }
    totals
}

pub fn merge_totals(parts: &[Vec<(String, u64)>]) -> Vec<(String, u64)> {
    let mut merged: Vec<(String, u64)> = Vec::new();
    for part in parts {
        for (name, bytes) in part {
            match merged.iter_mut().find(|(n, _)| n == name) {
                Some(entry) => entry.1 += bytes,
                None => merged.push((name.clone(), *bytes)),
            }
        }
    }
    merged
}

pub fn totals_json(totals: &[(String, u64)]) -> Value {
    let mut map = Map::new();
    for (name, bytes) in totals {
        map.insert(name.clone(), Value::from(*bytes));
    }
    Value::Object(map)
}
