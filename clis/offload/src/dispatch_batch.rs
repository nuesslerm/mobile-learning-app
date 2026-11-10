//! `wf offload ask --batch <glob...> -t <template>`: one dispatch per file, a bounded number in flight.
use wf_kit::args::{parse_spec, Positional, Spec};
use crate::dispatch_run::{dispatch_for_batch, repo_root_from, BatchItem};
use wf_kit::js::js_num;
use wf_kit::time::now_ms;
use rca_core::stats::to_fixed;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

struct Options {
    globs: Vec<String>,
    template: Option<String>,
    concurrency: String,
    cwd: Option<String>,
    idle_ms: Option<u64>,
    start_model: Option<String>,
}

/// What `dispatch --batch` takes.
pub const SPEC: Spec = Spec {
    variadic: &["batch"],
    values: &["prompt-template", "concurrency", "cwd", "idle-timeout-ms", "start-model"],
    short: &[('t', "prompt-template")],
    positional: Positional::None,
    ..Spec::NONE
};

fn parse_options(args: &[String]) -> Option<Options> {
    let p = parse_spec(args, &SPEC)?;
    let text = |name: &str| p.val(name).map(str::to_string);
    let idle_ms = match p.val("idle-timeout-ms") {
        Some(v) => Some(v.parse().ok()?),
        None => None,
    };
    let globs: Vec<String> = p.values("batch").into_iter().map(str::to_string).collect();
    (!globs.is_empty()).then(|| Options { globs, template: text("prompt-template"), concurrency: p.val("concurrency").unwrap_or("2").to_string(), cwd: text("cwd"), idle_ms, start_model: text("start-model") })
}

fn wildcard(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => (0..=name.len()).any(|k| wildcard(&pattern[1..], &name[k..])),
        Some('?') => !name.is_empty() && wildcard(&pattern[1..], &name[1..]),
        Some(c) => name.first() == Some(c) && wildcard(&pattern[1..], &name[1..]),
    }
}

fn matches_segment(segment: &str, name: &str) -> bool {
    if name.starts_with('.') && !segment.starts_with('.') {
        return false;
    }
    wildcard(&segment.chars().collect::<Vec<_>>(), &name.chars().collect::<Vec<_>>())
}

fn entries(dir: &Path) -> Vec<(String, bool)> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| Some((e.file_name().into_string().ok()?, e.file_type().ok()?.is_dir())))
                .collect()
        })
        .unwrap_or_default()
}

fn walk(base: &Path, rel: &str, segments: &[&str], out: &mut Vec<String>) {
    let Some((first, rest)) = segments.split_first() else { return };
    let join = |name: &str| if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") };
    if *first == "**" {
        walk(base, rel, rest, out);
        for (name, is_dir) in entries(&base.join(rel)) {
            if is_dir && !name.starts_with('.') {
                walk(base, &join(&name), segments, out);
            }
        }
        return;
    }
    for (name, is_dir) in entries(&base.join(rel)) {
        if !matches_segment(first, &name) {
            continue;
        }
        if rest.is_empty() {
            out.push(join(&name));
        } else if is_dir {
            walk(base, &join(&name), rest, out);
        }
    }
}

/// `None` for pattern syntax this matcher does not cover, which is then unsupported.
fn expand(pattern: &str, cwd: &Path) -> Option<Vec<String>> {
    if pattern.starts_with('/') || pattern.starts_with("./") || pattern.contains("..") || pattern.contains(['{', '}', '[', ']', '(', ')', '!', '\\']) {
        return None;
    }
    let segments: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    if segments.iter().any(|s| s.contains(['*', '?'])) {
        walk(cwd, "", &segments, &mut out);
    } else if cwd.join(pattern).exists() {
        out.push(segments.join("/"));
    }
    out.sort();
    out.dedup();
    Some(out)
}

fn render(template: &str, file: &str, content: &str) -> String {
    template.replace("{{file}}", file).replace("{{content}}", content)
}

fn transcript_name(file: &str) -> String {
    let mut out: String = file
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c.to_string() } else { "_".repeat(c.len_utf16()) })
        .collect();
    out.push_str(".md");
    out
}

fn table(items: &[(String, BatchItem)]) -> String {
    let mut lines: Vec<String> = items
        .iter()
        .map(|(file, it)| {
            let status = if it.ok { "ok" } else { "FAIL" };
            let secs = ((it.duration_ms / 1000.0) + 0.5).floor();
            format!("{:<12} {file}", format!("{status:<6} {secs}s"))
        })
        .collect();
    let ok = items.iter().filter(|(_, i)| i.ok).count();
    let cost: f64 = items.iter().map(|(_, i)| i.cost).sum();
    let tokens: f64 = items.iter().map(|(_, i)| i.tokens).sum();
    lines.push(String::new());
    lines.push(format!("{ok}/{} changed | tokens={} | cost=${}", items.len(), js_num(tokens), to_fixed(cost, 4)));
    lines.join("\n")
}

pub fn run(args: &[String]) -> Option<i32> {
    let o = parse_options(args)?;
    let Some(template_path) = o.template.clone() else {
        wf_kit::wf_err!("wf: --batch needs -t, --prompt-template <file>");
        return Some(1);
    };
    let cwd = PathBuf::from(o.cwd.clone().unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".into())));
    let mut files = Vec::new();
    for g in &o.globs {
        files.extend(expand(g, &cwd)?);
    }
    files.sort();
    if files.is_empty() {
        wf_kit::wf_err!("wf: no files matched: {}", o.globs.join(" "));
        return Some(1);
    }
    let template = std::fs::read_to_string(&template_path).ok()?;
    let concurrency: usize = o.concurrency.parse().ok()?;
    let transcript_dir = format!("{}/.agents/tmp/batch-{}", repo_root_from(&cwd).display(), now_ms());
    eprintln!("[batch: {} file(s), concurrency {}, transcripts in {transcript_dir}]", files.len(), o.concurrency);
    secrets::load_all_into_process_env();

    let wants_content = template.contains("{{content}}");
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<BatchItem>>> = Mutex::new((0..files.len()).map(|_| None).collect());
    let workers = concurrency.min(files.len()).max(1);
    let cwd_text = cwd.display().to_string();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::SeqCst);
                let Some(file) = files.get(index) else { break };
                let content = if wants_content { std::fs::read_to_string(file).unwrap_or_default() } else { String::new() };
                let item = dispatch_for_batch(
                    render(&template, file, &content),
                    &cwd_text,
                    file,
                    format!("{transcript_dir}/{}", transcript_name(file)),
                    o.idle_ms,
                    o.start_model.clone(),
                );
                let secs = ((item.duration_ms / 1000.0) + 0.5).floor();
                eprintln!("[batch {}/{}] {} {file} ({secs}s)", index + 1, files.len(), if item.ok { "ok" } else { "FAIL" });
                results.lock().expect("results")[index] = Some(item);
            });
        }
    });
    let items: Vec<(String, BatchItem)> = files.iter().cloned().zip(results.into_inner().expect("results").into_iter().flatten()).collect();
    println!("{}", table(&items));
    if let Some(stat) = items.last().and_then(|(_, i)| i.diff_stat.as_deref()).filter(|s| !s.is_empty()) {
        println!("\ngit diff --stat:\n{stat}");
    }
    Some(i32::from(items.iter().any(|(_, i)| !i.ok)))
}
