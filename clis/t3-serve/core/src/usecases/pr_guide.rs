//! `pullRequests.reviewGuide` and `pullRequests.generateReviewGuide`: a guided tour of a pull request that a model writes from its diff. The
//! guide is a model turn of the agent API on a thread of its own (the executor's own CLI, with the machine's own sign-in) whose prompt holds the
//! pull request's title, description, settled review remarks and diff; its JSON answer is checked against the diff (a stop may only show lines
//! the change touches, a note only a line inside one of its excerpts) and kept in the agent API's `state`, so a restart keeps it. A run is in
//! this process only: `Running` while the turn goes, `Failed` with its reason when it did not give a guide.
use super::pr::Target;
use super::queries::QueryFailure;
use super::service::Service;
use crate::ports::{AgentError, AgentEventKind, AgentThread, AgentTurn, Finish, Op};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use t3_contracts::time::iso;

const SCOPE: &str = "t3";
const OPERATION: &str = "PullRequestOperationError";
/// How long the model may take before its turn is stopped.
const TURN_LIMIT: Duration = Duration::from_secs(600);
const POLL: Duration = Duration::from_millis(200);
const MAX_EXCERPT_LINES: i64 = 25;
const ATTENTION_KINDS: [&str; 5] = ["tradeoff", "contract", "product", "risk", "accessibility"];

static GUIDE_THREADS: AtomicU64 = AtomicU64::new(0);

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

fn rejected(operation: &str, detail: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": OPERATION, "operation": operation, "detail": detail}))
}

/// An inclusive range of lines.
type Lines = (i64, i64);

/// The lines of a file a change touches, on each side: inclusive ranges.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileHunks {
    pub new: Vec<Lines>,
    pub old: Vec<Lines>,
}

/// The files of a pull request's diff and the lines each hunk covers.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Change {
    pub files: Vec<String>,
    pub hunks: HashMap<String, FileHunks>,
}

fn hunk_range(start: &str, count: Option<&str>) -> Option<Lines> {
    let start: i64 = start.parse().ok()?;
    let lines: i64 = count.map_or(Some(1), |c| c.parse().ok())?;
    (lines > 0).then_some((start, start + lines - 1))
}

/// `@@ -a,b +c,d @@`: the old and the new range.
fn hunk_header(line: &str) -> Option<(Option<Lines>, Option<Lines>)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let split = |s: &str| match s.split_once(',') {
        Some((start, count)) => hunk_range(start, Some(count)),
        None => hunk_range(s, None),
    };
    Some((split(old), split(new)))
}

/// What a unified diff changes: each file by the name it has after the change, with the line ranges of its hunks.
pub fn parse_change(patch: &str) -> Change {
    let mut change = Change::default();
    let mut current: Option<String> = None;
    for line in patch.lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            current = header.rsplit_once(" b/").map(|(_, path)| path.to_string()).filter(|p| !p.contains('"'));
            if let Some(path) = &current {
                if !change.files.contains(path) {
                    change.files.push(path.clone());
                }
                change.hunks.entry(path.clone()).or_default();
            }
        } else if let (Some(path), Some((old, new))) = (&current, hunk_header(line)) {
            let hunks = change.hunks.entry(path.clone()).or_default();
            hunks.old.extend(old);
            hunks.new.extend(new);
        }
    }
    change
}

#[derive(Debug, Clone, PartialEq)]
struct Excerpt {
    path: String,
    side: &'static str,
    start: i64,
    end: i64,
    caption: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Note {
    path: String,
    line: i64,
    title: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Stop {
    title: String,
    summary: String,
    attention: Option<(String, String)>,
    excerpts: Vec<Excerpt>,
    files: Vec<(String, Option<String>)>,
    notes: Vec<Note>,
    questions: Vec<String>,
}

fn int(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64).filter(|n| *n > 0)
}

fn items<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// A stop as the model wrote it, keeping only what is well formed: a stop needs its title; an excerpt, a note or a file with a missing part is
/// not kept.
fn read_stop(raw: &Value) -> Option<Stop> {
    let title = text(raw, "title")?.trim().to_string();
    let attention = raw.get("attention").filter(|a| a.is_object()).and_then(|a| Some((text(a, "kind").filter(|k| ATTENTION_KINDS.contains(k))?.to_string(), text(a, "note")?.to_string())));
    let excerpts = items(raw, "excerpts")
        .iter()
        .filter_map(|e| {
            let side = match text(e, "side")? {
                "old" => "old",
                "new" => "new",
                _ => return None,
            };
            Some(Excerpt { path: text(e, "path")?.to_string(), side, start: int(e, "start")?, end: int(e, "end")?, caption: e["caption"].as_str().unwrap_or_default().to_string() })
        })
        .collect();
    let files = items(raw, "files").iter().filter_map(|f| Some((text(f, "path")?.to_string(), text(f, "note").map(str::to_string)))).collect();
    let notes = items(raw, "notes").iter().filter_map(|n| Some(Note { path: text(n, "path")?.to_string(), line: int(n, "line")?, title: text(n, "title")?.to_string(), body: n["body"].as_str().unwrap_or_default().to_string() })).collect();
    let questions = items(raw, "questions").iter().filter_map(|q| q.as_str().filter(|s| !s.trim().is_empty()).map(str::to_string)).collect();
    Some(Stop { title, summary: raw["summary"].as_str().unwrap_or_default().to_string(), attention, excerpts, files, notes, questions })
}

/// The guide's stops out of what the model answered: the JSON object between the first `{` and the last `}` (a model may wrap it in prose or a
/// fence), with a `stops` array.
fn read_stops(answer: &str) -> Result<Vec<Stop>, String> {
    let (start, end) = (answer.find('{'), answer.rfind('}'));
    let object: Value = match (start, end) {
        (Some(s), Some(e)) if s < e => serde_json::from_str(&answer[s..=e]).map_err(|e| format!("The model's answer is not JSON ({e})."))?,
        _ => return Err("The model's answer holds no JSON object.".into()),
    };
    let stops = object.get("stops").and_then(Value::as_array).ok_or("The model's answer has no `stops`.")?;
    let read: Vec<Stop> = stops.iter().filter_map(read_stop).collect();
    if read.is_empty() {
        return Err("The model's answer has no usable stop.".into());
    }
    Ok(read)
}

/// An excerpt cut to the hunks of its file on its side: none when no hunk overlaps it.
fn clamp(excerpt: &Excerpt, change: &Change) -> Option<Excerpt> {
    let (start, end) = (excerpt.start.min(excerpt.end), excerpt.start.max(excerpt.end));
    let hunks = change.hunks.get(&excerpt.path)?;
    let side = if excerpt.side == "old" { &hunks.old } else { &hunks.new };
    let overlapping: Vec<&Lines> = side.iter().filter(|(hs, he)| *hs <= end && start <= *he).collect();
    let (first, last) = (overlapping.first()?, overlapping.last()?);
    Some(Excerpt { start: start.max(first.0), end: end.min(last.1), ..excerpt.clone() })
}

fn merge(excerpts: Vec<Excerpt>) -> Vec<Excerpt> {
    let mut merged: Vec<Excerpt> = Vec::new();
    for excerpt in excerpts {
        match merged.iter_mut().find(|o| o.path == excerpt.path && o.side == excerpt.side && o.start <= excerpt.end + 1 && excerpt.start <= o.end + 1) {
            Some(other) => {
                other.start = other.start.min(excerpt.start);
                other.end = other.end.max(excerpt.end);
                if other.caption.is_empty() {
                    other.caption = excerpt.caption;
                }
            }
            None => merged.push(excerpt),
        }
    }
    merged.into_iter().map(|e| Excerpt { end: e.end.min(e.start + MAX_EXCERPT_LINES - 1), ..e }).collect()
}

/// The stops as the phone may be shown them: only changed files and lines the change touches, notes only inside an excerpt, each file shown once,
/// and a last stop for the changed files no stop covers.
fn normalize(stops: Vec<Stop>, change: &Change) -> Vec<Stop> {
    let changed = |path: &str| change.files.iter().any(|f| f == path);
    let shaped: Vec<Stop> = stops
        .into_iter()
        .map(|stop| {
            let excerpts = merge(stop.excerpts.iter().filter(|e| changed(&e.path)).filter_map(|e| clamp(e, change)).collect());
            let notes = stop.notes.into_iter().filter(|n| excerpts.iter().any(|e| e.path == n.path && e.start <= n.line && n.line <= e.end)).collect();
            Stop { files: stop.files.into_iter().filter(|(p, _)| changed(p)).collect(), excerpts, notes, ..stop }
        })
        .collect();
    let mut placed: Vec<String> = shaped.iter().flat_map(|s| s.excerpts.iter().map(|e| e.path.clone())).collect();
    let mut stops: Vec<Stop> = shaped
        .into_iter()
        .map(|stop| {
            let files: Vec<(String, Option<String>)> = stop.files.into_iter().filter(|(p, _)| !placed.contains(p)).collect();
            placed.extend(files.iter().map(|(p, _)| p.clone()));
            Stop { files, ..stop }
        })
        .collect();
    let unplaced: Vec<(String, Option<String>)> = change.files.iter().filter(|p| !placed.contains(p)).map(|p| (p.clone(), None)).collect();
    if !unplaced.is_empty() {
        stops.push(Stop { title: "Other files".into(), summary: "Changed files the tour does not otherwise cover.".into(), attention: None, excerpts: vec![], files: unplaced, notes: vec![], questions: vec![] });
    }
    stops
}

fn stop_json(stop: &Stop) -> Value {
    json!({
        "title": stop.title, "summary": stop.summary,
        "attention": stop.attention.as_ref().map(|(kind, note)| json!({"kind": kind, "note": note})),
        "excerpts": stop.excerpts.iter().map(|e| json!({"path": e.path, "side": e.side, "start": e.start, "end": e.end, "caption": e.caption})).collect::<Vec<_>>(),
        "files": stop.files.iter().map(|(path, note)| note.as_ref().map_or_else(|| json!({"path": path}), |n| json!({"path": path, "note": n}))).collect::<Vec<_>>(),
        "notes": stop.notes.iter().map(|n| json!({"path": n.path, "line": n.line, "title": n.title, "body": n.body})).collect::<Vec<_>>(),
        "questions": stop.questions,
    })
}

fn one_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 200 {
        format!("{}…", flat.chars().take(200).collect::<String>())
    } else {
        flat
    }
}

/// What the model is asked: the pull request, what its reviewers already settled, the project's own instructions, and the diff.
fn prompt(pr: &Value, number: u64, settled: &str, instructions: &str, patch: &str, truncated: bool) -> String {
    let mut out = String::from("You are writing a guided tour of a pull request for the person who will review it. Read the change below and answer with one JSON object and nothing else: no prose, no code fence.\n\n");
    out.push_str(&format!("Pull request #{number}: {}\nBase branch: {}\n\n{}\n", text(pr, "title").unwrap_or("(untitled)"), text(pr, "base_ref").unwrap_or("(unknown)"), pr["body"].as_str().unwrap_or_default()));
    if !settled.is_empty() {
        out.push_str(&format!("\nReview remarks already resolved or outdated (do not raise them again):\n{settled}\n"));
    }
    if !instructions.trim().is_empty() {
        out.push_str(&format!("\nInstructions from the project:\n{}\n", instructions.trim()));
    }
    out.push_str(concat!(
        "\nThe answer is {\"stops\": [...]}. A stop is one idea of the change, in the order a reviewer should read it:\n",
        "{\"title\": \"short\", \"summary\": \"what it does and why\", \"attention\": null or {\"kind\": \"tradeoff\"|\"contract\"|\"product\"|\"risk\"|\"accessibility\", \"note\": \"what to look at\"},\n",
        " \"excerpts\": [{\"path\": \"a changed file\", \"side\": \"new\"|\"old\", \"start\": line, \"end\": line, \"caption\": \"why this code\"}],\n",
        " \"files\": [{\"path\": \"a changed file\", \"note\": \"optional\"}], \"notes\": [{\"path\": \"a changed file\", \"line\": line inside one of the excerpts, \"title\": \"short\", \"body\": \"detail\"}],\n",
        " \"questions\": [\"what the reviewer should ask the author\"]}\n",
        "Use only paths and line numbers the diff shows, at most 25 lines to an excerpt.\n",
    ));
    if truncated {
        out.push_str("\nThe diff below was cut where it was too long: tour only what it shows.\n");
    }
    out.push_str("\nThe diff:\n");
    out.push_str(patch);
    out
}

/// The resolved and outdated review threads as the prompt lists them: one line each, the opener's first words.
fn settled_summary(threads: &Value) -> String {
    threads["threads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["resolved"] == true || t["outdated"] == true)
        .filter_map(|t| {
            let opener = t["comments"].as_array()?.first()?;
            let state = if t["resolved"] == true { "resolved" } else { "outdated" };
            Some(format!("- {} ({state}, {}): {}", text(t, "path")?, opener["author"].as_str().unwrap_or("ghost"), one_line(opener["body"].as_str().unwrap_or_default())))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Service {
    fn guide_key(target: &Target, number: u64) -> String {
        format!("pr.guide:{}:{}:{number}", target.project.id, target.repo.clone().unwrap_or_default())
    }

    fn stored_guide(&self, key: &str) -> Option<Value> {
        let stored = self.agent().run(&Op::StateGet { scope: SCOPE.into(), key: key.into() }).ok()?;
        stored.get("value").filter(|v| v.is_object()).cloned()
    }

    fn run_of(&self, key: &str) -> Option<Value> {
        self.2.guide_runs.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    /// `pullRequests.reviewGuide`: the guide kept for the pull request, the run that is writing or failed to write one, and the head the host has now.
    pub fn pr_review_guide(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("reviewGuide", payload)?;
        let number = Self::number(payload)?;
        let key = Self::guide_key(&target, number);
        let pr = self.view("reviewGuide", &target, number)?;
        Ok(json!({"guide": self.stored_guide(&key), "run": self.run_of(&key), "currentHeadSha": text(&pr, "head_sha")}))
    }

    /// `pullRequests.generateReviewGuide`: starts the run (a second ask for the same head while it goes starts none) and answers at once.
    pub fn pr_generate_review_guide(&self, payload: &Value) -> Result<Value, QueryFailure> {
        const OP: &str = "generateReviewGuide";
        let settings = self.effective_settings();
        if settings["reviewGuidesEnabled"] != json!(true) {
            return Err(rejected(OP, "Review guides are off. Turn them on in Settings → Source Control."));
        }
        let selection = [&settings["reviewGuideModelSelection"], &settings["textGenerationModelSelection"], &settings["defaultModelSelection"]]
            .into_iter()
            .find(|s| text(s, "instanceId").is_some() && text(s, "model").is_some())
            .cloned()
            .ok_or_else(|| rejected(OP, "Pick a model for review guides in Settings → Source Control."))?;
        let target = self.target(OP, payload)?;
        let number = Self::number(payload)?;
        let key = Self::guide_key(&target, number);
        let pr = self.view(OP, &target, number)?;
        let head = text(&pr, "head_sha").ok_or_else(|| rejected(OP, "This host did not report the pull request's head commit."))?.to_string();
        let stored = self.stored_guide(&key);
        let running = json!({"_tag": "Running", "headSha": head, "startedAt": iso(self.now_ms())});
        {
            let mut runs = self.2.guide_runs.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = runs.get(&key).filter(|r| r["_tag"] == "Running" && r["headSha"] == json!(head)) {
                return Ok(json!({"guide": stored, "run": existing, "currentHeadSha": head, "started": false}));
            }
            runs.insert(key.clone(), running.clone());
        }
        let (service, instructions) = (self.clone(), settings["reviewGuideInstructions"].as_str().unwrap_or_default().to_string());
        let (run_key, run_head) = (key.clone(), head.clone());
        std::thread::spawn(move || {
            let outcome = service.write_review_guide(&target, number, &pr, &selection, &instructions);
            let mut runs = service.2.guide_runs.lock().unwrap_or_else(|e| e.into_inner());
            if runs.get(&run_key).is_some_and(|r| r["headSha"] != json!(run_head)) {
                return;
            }
            match outcome {
                Ok(guide) => match service.agent().run(&Op::StateSet { scope: SCOPE.into(), key: run_key.clone(), value: guide }) {
                    Ok(_) => {
                        runs.remove(&run_key);
                    }
                    Err(e) => {
                        runs.insert(run_key.clone(), json!({"_tag": "Failed", "headSha": run_head, "failedAt": iso(service.now_ms()), "detail": format!("The review guide could not be kept: {}", e.message())}));
                    }
                },
                Err(detail) => {
                    service.log("review_guide.failed", &[("detail", detail.clone())]);
                    runs.insert(run_key.clone(), json!({"_tag": "Failed", "headSha": run_head, "failedAt": iso(service.now_ms()), "detail": detail}));
                }
            }
        });
        Ok(json!({"guide": stored, "run": running, "currentHeadSha": head, "started": true}))
    }

    /// The guide for the head `pr` has: the diff and the settled remarks read through the hosted group, the model's turn, the answer checked.
    fn write_review_guide(&self, target: &Target, number: u64, pr: &Value, selection: &Value, instructions: &str) -> Result<Value, String> {
        let hosted = |method: &'static str| self.ask_hosted("reviewGuide", target, method, json!({"number": number})).map_err(|e| match e {
            QueryFailure::Error(v) => v["detail"].as_str().or(v["requirement"].as_str()).unwrap_or("the host refused").to_string(),
            QueryFailure::Defect(m) => m,
        });
        let diff = hosted("pr.diff")?;
        let patch = diff["diff"].as_str().unwrap_or_default();
        if patch.trim().is_empty() {
            return Err("The host gave no diff for this pull request.".into());
        }
        let settled = hosted("pr.threads").map(|t| settled_summary(&t)).unwrap_or_default();
        let ask = prompt(pr, number, &settled, instructions, patch, diff["truncated"] == true);
        let answer = self.run_model_turn(selection, &ask)?;
        let stops = normalize(read_stops(&answer)?, &parse_change(patch));
        let mut chosen = Map::new();
        for key in ["instanceId", "model", "options"] {
            if let Some(v) = selection.get(key).filter(|v| !v.is_null()) {
                chosen.insert(key.into(), v.clone());
            }
        }
        Ok(json!({"headSha": text(pr, "head_sha"), "generatedAt": iso(self.now_ms()), "modelSelection": chosen, "stops": stops.iter().map(stop_json).collect::<Vec<_>>()}))
    }

    /// One turn of the model on a thread of its own, which is deleted afterwards: what it said in the turn, as one text.
    fn run_model_turn(&self, selection: &Value, prompt: &str) -> Result<String, String> {
        let thread = format!("review-guide-{}-{}", self.now_ms(), GUIDE_THREADS.fetch_add(1, Ordering::SeqCst));
        let (instance, model) = (text(selection, "instanceId").unwrap_or_default().to_string(), text(selection, "model").map(str::to_string));
        let outcome = self.turn_text(&thread, &instance, model, prompt);
        if let Err(e) = self.agent().run(&Op::ThreadDelete { thread: thread.clone(), only_if_unused: false }) {
            self.log("review_guide.thread_kept", &[("detail", e.message())]);
        }
        outcome
    }

    fn turn_text(&self, thread: &str, instance: &str, model: Option<String>, prompt: &str) -> Result<String, String> {
        let agent_error = |e: AgentError| e.message();
        let mut after = self.agent().ensure_thread(&AgentThread { thread_id: thread.into(), instance_id: instance.into(), model: model.clone() }).map_err(agent_error)?;
        let turn = self.agent().start_turn(&AgentTurn { thread_id: thread.into(), instance_id: instance.into(), model, prompt: prompt.into(), runtime_mode: "approval-required".into(), interaction_mode: "default".into(), attachments: vec![] }).map_err(agent_error)?;
        let deadline = Instant::now() + TURN_LIMIT;
        let (mut said, mut failed) = (String::new(), None);
        loop {
            let page = self.agent().history(thread, after, 500).map_err(agent_error)?;
            for event in page.events.iter().filter(|e| e.turn.as_deref() == Some(turn.as_str())) {
                match &event.kind {
                    AgentEventKind::Text(t) => said.push_str(t),
                    AgentEventKind::Failure(why) => failed = Some(why.clone()),
                    AgentEventKind::TurnFinished(Finish::Completed) => return if said.trim().is_empty() { Err("The model gave no answer.".into()) } else { Ok(said) },
                    AgentEventKind::TurnFinished(finish) => return Err(failed.unwrap_or_else(|| format!("The model's turn ended {}.", if *finish == Finish::Interrupted { "interrupted" } else { "in failure" }))),
                    _ => {}
                }
            }
            after = page.last_seq.max(after);
            if Instant::now() >= deadline {
                let _ = self.agent().interrupt(thread);
                return Err("The model did not answer in time.".into());
            }
            if !page.more {
                std::thread::sleep(POLL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/up.rs b/src/up.rs\nindex 1..2 100644\n--- a/src/up.rs\n+++ b/src/up.rs\n@@ -3,2 +3,4 @@ fn a() {\n keep\n-old\n+new\n+more\n@@ -20 +22,0 @@\n-gone\ndiff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-a\n+b\ndiff --git a/src/new.rs b/src/new.rs\nnew file mode 100644\n--- /dev/null\n+++ b/src/new.rs\n@@ -0,0 +1,30 @@\n+x\n";

    fn stop(excerpts: Vec<Excerpt>, notes: Vec<Note>, files: &[&str]) -> Stop {
        Stop { title: "t".into(), summary: "s".into(), attention: None, excerpts, files: files.iter().map(|p| (p.to_string(), None)).collect(), notes, questions: vec![] }
    }

    fn excerpt(path: &str, side: &'static str, start: i64, end: i64) -> Excerpt {
        Excerpt { path: path.into(), side, start, end, caption: String::new() }
    }

    #[test]
    fn given_a_diff_when_its_change_is_parsed_then_each_file_has_the_line_ranges_of_its_hunks_on_each_side_and_an_empty_side_has_none() {
        // Given a diff with a modified file (two hunks, the second only deleting), a one-line change and a new file
        let change = parse_change(PATCH);
        // Then the files are in the diff's order and each side holds its ranges: a count of zero is no range
        assert_eq!(change.files, ["src/up.rs", "README.md", "src/new.rs"]);
        assert_eq!(change.hunks["src/up.rs"], FileHunks { old: vec![(3, 4), (20, 20)], new: vec![(3, 6)] });
        assert_eq!(change.hunks["README.md"], FileHunks { old: vec![(1, 1)], new: vec![(1, 1)] });
        assert_eq!(change.hunks["src/new.rs"], FileHunks { old: vec![], new: vec![(1, 30)] });
    }

    #[test]
    fn given_an_answer_in_prose_or_a_fence_when_read_then_the_json_object_gives_its_well_formed_stops_and_anything_else_is_an_error() {
        let answer = "Here is the tour:\n```json\n{\"stops\": [{\"title\": \"Retry\", \"summary\": \"s\", \"attention\": {\"kind\": \"risk\", \"note\": \"n\"}, \"excerpts\": [{\"path\": \"src/up.rs\", \"side\": \"new\", \"start\": 3, \"end\": 5, \"caption\": \"c\"}, {\"path\": \"x\", \"side\": \"left\", \"start\": 1, \"end\": 2}, {\"path\": \"y\", \"side\": \"new\", \"start\": 0, \"end\": 2}], \"files\": [{\"path\": \"README.md\", \"note\": \"docs\"}, {\"note\": \"no path\"}], \"notes\": [{\"path\": \"src/up.rs\", \"line\": 4, \"title\": \"why\", \"body\": \"b\"}], \"questions\": [\"q\", \" \", 3]}, {\"summary\": \"no title\"}]}\n```";
        let stops = read_stops(answer).unwrap();
        // Then the titled stop is kept with its valid excerpt, file, note and question only, and the attention kind is one T3 knows
        assert_eq!(stops.len(), 1);
        assert_eq!((stops[0].excerpts.len(), stops[0].files.clone(), stops[0].notes.len(), stops[0].questions.clone(), stops[0].attention.clone()), (1, vec![("README.md".to_string(), Some("docs".to_string()))], 1, vec!["q".to_string()], Some(("risk".to_string(), "n".to_string()))));
        for bad in ["", "no json here", "{\"stops\": []}", "{\"stops\": [{\"summary\": \"x\"}]}", "{\"other\": 1}", "{not json}"] {
            assert!(read_stops(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn given_stops_that_show_lines_the_change_does_not_touch_when_normalized_then_only_changed_files_and_hunk_lines_remain_and_the_rest_of_the_files_get_a_last_stop() {
        // Given the diff's change, and a stop whose excerpts reach past a hunk, into an unchanged file, and onto the wrong side, with notes in and out of them
        let change = parse_change(PATCH);
        let wide = stop(
            vec![excerpt("src/up.rs", "new", 1, 40), excerpt("src/up.rs", "new", 5, 6), excerpt("Cargo.toml", "new", 1, 2), excerpt("src/new.rs", "old", 1, 5), excerpt("src/new.rs", "new", 1, 200)],
            vec![Note { path: "src/up.rs".into(), line: 4, title: "in".into(), body: "b".into() }, Note { path: "src/up.rs".into(), line: 9, title: "out".into(), body: "b".into() }, Note { path: "Cargo.toml".into(), line: 1, title: "elsewhere".into(), body: "b".into() }],
            &["src/up.rs", "Cargo.toml", "README.md"],
        );
        // When normalized
        let stops = normalize(vec![wide], &change);
        // Then the excerpt is cut to the hunk and merged with the one inside it, the unchanged file and the old side of a new file are gone, a long excerpt stops at 25 lines
        assert_eq!(stops[0].excerpts, vec![excerpt("src/up.rs", "new", 3, 6), excerpt("src/new.rs", "new", 1, 25)]);
        // And only the note inside an excerpt stays, the file an excerpt placed is not listed again, and README.md (changed, unplaced) is shown once in the stop and not again
        assert_eq!(stops[0].notes.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["in"]);
        assert_eq!(stops[0].files, vec![("README.md".to_string(), None)]);
        assert_eq!(stops.len(), 1);
        // When a stop covers nothing of README.md, Then the changed file no stop covers gets the last stop
        let stops = normalize(vec![stop(vec![excerpt("src/up.rs", "new", 3, 4)], vec![], &[])], &change);
        assert_eq!((stops.len(), stops[1].title.as_str(), stops[1].files.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>()), (2, "Other files", vec!["README.md", "src/new.rs"]));
    }

    #[test]
    fn given_the_threads_of_a_pull_request_when_summarized_then_only_resolved_and_outdated_ones_are_listed_one_line_each() {
        let threads = json!({"threads": [
            {"path": "a.rs", "resolved": true, "outdated": false, "comments": [{"author": "hubot", "body": "Why\n\nretry   here?"}]},
            {"path": "b.rs", "resolved": false, "outdated": true, "comments": [{"author": null, "body": "x"}]},
            {"path": "c.rs", "resolved": false, "outdated": false, "comments": [{"author": "me", "body": "open"}]}]});
        assert_eq!(settled_summary(&threads), "- a.rs (resolved, hubot): Why retry here?\n- b.rs (outdated, ghost): x");
    }

    #[test]
    fn given_a_pull_request_when_the_prompt_is_made_then_it_holds_the_description_the_diff_and_the_instructions_and_says_so_when_the_diff_was_cut() {
        let pr = json!({"title": "Add retry", "base_ref": "main", "body": "Retries the upload."});
        let full = prompt(&pr, 12, "- a.rs (resolved, hubot): fine", "Be brief.", "diff --git a/x b/x\n", false);
        let cut = prompt(&pr, 12, "", "", "diff --git a/x b/x\n", true);
        assert!(["Pull request #12: Add retry", "Base branch: main", "Retries the upload.", "- a.rs (resolved, hubot): fine", "Be brief.", "The diff:\ndiff --git a/x b/x"].iter().all(|part| full.contains(part)), "{full}");
        assert!(!full.contains("was cut") && cut.contains("was cut") && !cut.contains("Instructions from the project"));
    }
}
