//! `wf knowledge rank|index|stale`: the docs/gaps and docs/solutions ledger. Anything the port cannot reproduce byte for
//! byte (YAML errors, odd flags, locale-sensitive sorts, impossible dates) goes to the TypeScript before any output.
mod help_table;
mod gem_split;
mod gem_drive;
mod knowledge_gemini;

pub use help_table::HELP;

use wf_kit::args::{parse_spec, Positional, Spec};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use yaml_rust2::{Yaml, YamlLoader};

const INDEX_MAX_LINES: usize = 200;
const STALE_DAYS: i64 = 30;
const DAY_MS: i64 = 86_400_000;

struct Fallback;
type Res<T> = Result<T, Fallback>;

struct Meta {
    slug: String,
    title: String,
    file: String,
}

enum Rec {
    Gap { meta: Meta, status: String, area: String, paths: Vec<String>, opened: String, last_verified: String, verify: Option<String> },
    Learning { meta: Meta, title: String, paths: Vec<String>, date: Option<String>, category: Option<String> },
}

impl Rec {
    fn paths(&self) -> &[String] {
        match self {
            Rec::Gap { paths, .. } | Rec::Learning { paths, .. } => paths,
        }
    }
    fn recency(&self) -> &str {
        match self {
            Rec::Gap { last_verified, .. } => last_verified,
            Rec::Learning { date, .. } => date.as_deref().unwrap_or(""),
        }
    }
}

fn key<'a>(h: &'a Yaml, k: &str) -> &'a Yaml {
    &h[k]
}

fn string_list(v: &Yaml) -> Option<Vec<String>> {
    match v {
        Yaml::BadValue | Yaml::Null => Some(vec![]),
        Yaml::String(s) => Some(vec![s.clone()]),
        Yaml::Array(a) => a.iter().map(|x| if let Yaml::String(s) = x { Some(s.clone()) } else { None }).collect(),
        _ => None,
    }
}

fn valid_date(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let shape = b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    shape.then(|| s.to_string())
}

fn date_string(v: &Yaml) -> Option<String> {
    if let Yaml::String(s) = v { valid_date(s) } else { None }
}

/// Epoch milliseconds of a calendar date, or None when JS `Date.parse` could disagree about it.
fn date_ms(s: &str) -> Option<i64> {
    rca_core::time::date_ms(s)
}

fn age_days(date: &str, now: i64) -> Res<i64> {
    let at = date_ms(date).ok_or(Fallback)?;
    Ok(((now - at).div_euclid(DAY_MS)).max(0))
}

fn split_frontmatter(text: &str) -> Res<(Option<Yaml>, String)> {
    let rest = if let Some(r) = text.strip_prefix("---\r\n") { r } else if let Some(r) = text.strip_prefix("---\n") { r } else { return Ok((None, text.to_string())) };
    let Some(at) = rest.find("\n---") else { return Ok((None, text.to_string())) };
    let raw = rest[..at].strip_suffix('\r').unwrap_or(&rest[..at]);
    // The opening fence's newline belongs to the block, so an empty block needs its own blank line.
    let after = &rest[at + 4..];
    let body = after.strip_prefix("\r\n").or_else(|| after.strip_prefix('\n')).unwrap_or(after);
    let docs = YamlLoader::load_from_str(raw).map_err(|_| Fallback)?;
    if docs.len() > 1 {
        return Err(Fallback);
    }
    Ok((Some(docs.into_iter().next().unwrap_or(Yaml::Null)), body.to_string()))
}

fn first_heading(body: &str) -> String {
    body.split(['\n', '\r', '\u{2028}', '\u{2029}'])
        .find_map(|l| l.strip_prefix("# ").filter(|r| !r.is_empty()))
        .map(|r| wf_kit::js::trim(r).to_string())
        .unwrap_or_default()
}

fn json_str(v: &Yaml) -> Res<String> {
    Ok(match v {
        Yaml::BadValue => "undefined".into(),
        Yaml::Null => "null".into(),
        Yaml::String(s) => serde_json::to_string(s).map_err(|_| Fallback)?,
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Integer(i) => i.to_string(),
        _ => return Err(Fallback),
    })
}

fn parse_gap(fm: &Yaml, meta: Meta) -> Res<Result<Rec, Vec<String>>> {
    let Yaml::Hash(_) = fm else { return Ok(Err(vec!["missing frontmatter".into()])) };
    let mut errors = vec![];
    let status = match key(fm, "status") {
        Yaml::String(s) if s == "open" || s == "blocked" => s.clone(),
        other => {
            errors.push(format!("status must be open|blocked, got {}", json_str(other)?));
            String::new()
        }
    };
    let area = if let Yaml::String(s) = key(fm, "area") { wf_kit::js::trim(s).to_string() } else { String::new() };
    if area.is_empty() {
        errors.push("area is required".into());
    }
    let paths = string_list(key(fm, "paths")).filter(|p| !p.is_empty());
    if paths.is_none() {
        errors.push("paths must list at least one glob".into());
    }
    let opened = date_string(key(fm, "opened"));
    if opened.is_none() {
        errors.push("opened must be YYYY-MM-DD".into());
    }
    let last_verified = date_string(key(fm, "last_verified"));
    if last_verified.is_none() {
        errors.push("last_verified must be YYYY-MM-DD".into());
    }
    if string_list(key(fm, "links")).is_none() {
        errors.push("links must be a list of strings".into());
    }
    let verify = match key(fm, "verify") {
        Yaml::BadValue => None,
        Yaml::String(s) => Some(s.clone()),
        _ => {
            errors.push("verify must be a command string".into());
            None
        }
    };
    if wf_kit::js::trim(&meta.title).is_empty() {
        errors.push("missing `# Title` heading".into());
    }
    if !errors.is_empty() {
        return Ok(Err(errors));
    }
    Ok(Ok(Rec::Gap { meta, status, area, paths: paths.unwrap_or_default(), opened: opened.unwrap_or_default(), last_verified: last_verified.unwrap_or_default(), verify }))
}

fn parse_learning(fm: &Yaml, meta: Meta) -> Option<Rec> {
    let Yaml::Hash(_) = fm else { return None };
    let paths = string_list(key(fm, "paths"))?;
    let title = match key(fm, "title") {
        Yaml::String(s) if !wf_kit::js::trim(s).is_empty() => s.clone(),
        _ => meta.title.clone(),
    };
    let date = date_string(key(fm, "date"));
    let category = if let Yaml::String(s) = key(fm, "category") { Some(s.clone()).filter(|s| !s.is_empty()) } else { None };
    Some(Rec::Learning { meta, title, paths, date, category })
}

fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn markdown_files(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if t.is_dir() && recursive {
            markdown_files(&p, true, out);
        } else if t.is_file() && name.ends_with(".md") && name != "README.md" {
            out.push(p);
        }
    }
}

fn sorted_files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut out = vec![];
    markdown_files(dir, recursive, &mut out);
    out.sort_by(|a, b| utf16_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
    out
}

struct Loaded {
    records: Vec<Rec>,
    gap_errors: Vec<String>,
}

fn meta_for(repo: &Path, file: &Path, body: &str) -> Meta {
    Meta {
        slug: file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        title: first_heading(body),
        file: file.strip_prefix(repo).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

fn load(repo: &Path) -> Res<Loaded> {
    let mut records = vec![];
    let mut gap_errors = vec![];
    for file in sorted_files(&repo.join("docs").join("gaps"), false) {
        let text = std::fs::read_to_string(&file).map_err(|_| Fallback)?;
        let Ok((fm, body)) = split_frontmatter(&text) else {
            let rel = file.strip_prefix(repo).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            gap_errors.push(format!("{rel}: frontmatter is not valid YAML"));
            continue;
        };
        let meta = meta_for(repo, &file, &body);
        let rel = meta.file.clone();
        match parse_gap(&fm.unwrap_or(Yaml::BadValue), meta)? {
            Ok(r) => records.push(r),
            Err(e) => gap_errors.push(format!("{rel}: {}", e.join("; "))),
        }
    }
    for file in sorted_files(&repo.join("docs").join("solutions"), true) {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let Ok((Some(fm), body)) = split_frontmatter(&text) else { continue };
        if let Some(r) = parse_learning(&fm, meta_for(repo, &file, &body)) {
            records.push(r);
        }
    }
    Ok(Loaded { records, gap_errors })
}

fn normalize(p: &str) -> String {
    let p = p.replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_string()
}

fn path_matches(file: &str, glob: &str) -> bool {
    let f = normalize(file);
    let g = normalize(glob);
    let g = g.trim_end_matches('/');
    if !g.contains(['*', '?']) {
        return f == g || f.starts_with(&format!("{g}/"));
    }
    let chars: Vec<char> = g.chars().collect();
    let mut re = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            let slash = chars.get(i + 2) == Some(&'/');
            re.push_str(if slash { "(?:.*/)?" } else { ".*" });
            i += if slash { 2 } else { 1 };
        } else if c == '*' {
            re.push_str("[^/]*");
        } else if c == '?' {
            re.push_str("[^/]");
        } else if ".+^${}()|[]\\".contains(c) {
            re.push('\\');
            re.push(c);
        } else {
            re.push(c);
        }
        i += 1;
    }
    Regex::new(&format!("^{re}$")).is_ok_and(|r| r.is_match(&f))
}

fn render_matches(hits: &[&Rec]) -> String {
    hits.iter()
        .map(|r| match r {
            Rec::Gap { meta, status, area, .. } => format!("- gap ({status}, {area}): [{}]({})", meta.title, meta.file),
            Rec::Learning { meta, title, category, .. } => {
                let cat = category.as_ref().map(|c| format!(" ({c})")).unwrap_or_default();
                format!("- learning{cat}: [{title}]({})", meta.file)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The TS worker host prints a failing worker's stderr once more behind `wf: `.
fn fail(msg: &str) {
    eprint!("{msg}");
    wf_kit::wf_err!("wf: {}", wf_kit::js::tail(msg, 4096));
}

fn report_gap_errors(l: &Loaded) -> bool {
    if l.gap_errors.is_empty() {
        return false;
    }
    let lines: Vec<String> = l.gap_errors.iter().map(|e| format!("  {e}")).collect();
    fail(&format!("invalid gap records:\n{}\n", lines.join("\n")));
    true
}

fn positive_int(raw: Option<&str>, dflt: i64) -> Res<i64> {
    match raw {
        None => Ok(dflt),
        Some(s) if !s.is_empty() && s.len() < 10 && s.bytes().all(|b| b.is_ascii_digit()) => s.parse::<i64>().ok().filter(|n| *n > 0).ok_or(Fallback),
        Some(_) => Err(Fallback),
    }
}

fn plain_area(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn go(args: &[String]) -> Res<i32> {
    let (sub, rest) = args.split_first().ok_or(Fallback)?;
    let spec = match sub.as_str() {
        "rank" => Spec { values: &["limit"], positional: Positional::Any, ..Spec::NONE },
        "stale" => Spec { values: &["days"], positional: Positional::None, ..Spec::NONE },
        "index" => Spec { flags: &["write"], positional: Positional::None, ..Spec::NONE },
        _ => return Err(Fallback),
    };
    let p = parse_spec(rest, &spec).ok_or(Fallback)?;
    let files: Vec<&str> = p.positional.iter().map(String::as_str).collect();
    let (limit, days, write) = (p.val("limit"), p.val("days"), p.flag("write"));
    if sub == "rank" && files.is_empty() {
        return Err(Fallback);
    }
    let top = Command::new("git").args(["rev-parse", "--show-toplevel"]).stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|_| Fallback)?;
    if !top.status.success() {
        return Err(Fallback);
    }
    let repo = PathBuf::from(wf_kit::js::trim(&String::from_utf8_lossy(&top.stdout)));
    let loaded = load(&repo)?;
    let now = now_ms();
    match sub.as_str() {
        "rank" => {
            let limit = positive_int(limit, 3)? as usize;
            let mut hits: Vec<&Rec> = loaded.records.iter().filter(|r| r.paths().iter().any(|g| files.iter().any(|f| path_matches(f, g)))).collect();
            let rank = |r: &Rec| if matches!(r, Rec::Gap { .. }) { 0 } else { 1 };
            hits.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| b.recency().cmp(a.recency())));
            hits.truncate(limit);
            if !hits.is_empty() {
                println!("{}", render_matches(&hits));
            }
            Ok(0)
        }
        "stale" => {
            let Ok(days) = positive_int(days, STALE_DAYS) else {
                eprintln!("--days must be a positive integer");
                return Ok(2);
            };
            if report_gap_errors(&loaded) {
                return Ok(1);
            }
            let mut stale = vec![];
            for r in &loaded.records {
                if let Rec::Gap { meta, last_verified, verify, .. } = r {
                    if age_days(last_verified, now)? > days {
                        stale.push((last_verified.clone(), format!("{}\t{last_verified}\t{}\n", meta.slug, verify.as_deref().unwrap_or("-"))));
                    }
                }
            }
            stale.sort_by(|a, b| a.0.cmp(&b.0));
            for (_, line) in stale {
                print!("{line}");
            }
            Ok(0)
        }
        _ => {
            if report_gap_errors(&loaded) {
                return Ok(1);
            }
            let mut gaps: Vec<(&str, &str, &Meta, &str)> = vec![];
            for r in &loaded.records {
                if let Rec::Gap { meta, area, opened, status, .. } = r {
                    if !plain_area(area) {
                        return Err(Fallback);
                    }
                    gaps.push((area, opened, meta, status));
                }
            }
            gaps.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(b.1)));
            let mut lines = vec![
                "# Known gaps".to_string(),
                String::new(),
                "Generated by `wf knowledge index --write` from `docs/gaps/*.md`; do not edit by hand.".into(),
                "One line per open gap: `area · title · age · link`. A gap closes only through `/cf-compound <slug>`.".into(),
                "Pre-2026-10 history: [archive](archive/README.md).".into(),
                String::new(),
            ];
            for (area, opened, meta, status) in &gaps {
                let blocked = if *status == "blocked" { " (blocked)" } else { "" };
                lines.push(format!("- {area} · {}{blocked} · {}d · [{}](gaps/{}.md)", meta.title, age_days(opened, now)?, meta.slug, meta.slug));
            }
            if gaps.is_empty() {
                lines.push("No open gaps.".into());
            }
            let text = format!("{}\n", lines.join("\n"));
            let line_count = text.matches('\n').count();
            if line_count > INDEX_MAX_LINES {
                fail(&format!("docs/known-gaps.md would be {line_count} lines (> {INDEX_MAX_LINES}): close or merge gaps\n"));
                return Ok(1);
            }
            if !write {
                print!("{text}");
                return Ok(0);
            }
            let docs = repo.join("docs");
            if !docs.is_dir() {
                return Err(Fallback);
            }
            std::fs::write(docs.join("known-gaps.md"), &text).map_err(|_| Fallback)?;
            println!("wrote docs/known-gaps.md: {} gaps, {line_count} lines", gaps.len());
            Ok(0)
        }
    }
}

/// What the thin binary mounts.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("knowledge", run)];

pub fn run(args: &[String]) -> Option<i32> {
    if args.first().is_some_and(|a| a == "index") && wf_kit::args::probe(&args[1..], &[&knowledge_gemini::SPEC]).is_some_and(|p| p.flag("gemini")) {
        return knowledge_gemini::run(&args[1..]);
    }
    go(args).ok()
}
