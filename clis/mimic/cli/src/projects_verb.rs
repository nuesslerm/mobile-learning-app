//! `wf mimic record-projects`: the live recording of the `hosted_project_*` actions of rca-exec against a throwaway GitHub Projects v2 board.
//! Setup creates the board (and a text field, a number field and a draft item) with `gh api graphql`; the run asks the real executor
//! (`LocalExecutor`) for each action with a recording `gh` on its PATH, so the argv, stdin, stdout and exit code of every call are what the real gh did;
//! teardown deletes the board whatever the run did, and a guard deletes boards of an earlier killed run. The token is read in memory
//! (`GH_PROJECTS_RECORD_TOKEN`: the environment, else the vault), reaches gh only as `GH_TOKEN`, and a recorded value that holds it stops the write.
use super::{fail, Done, Fail};
use mimic_core::cassette::{Cassette, Dir, DriverInfo, Effect, Header, Interaction, Kind, Outcome, Provenance, RefHash, Step, StepKind, Target, ToolInfo, SCHEMA};
use mimic_core::json::sha256_label;
use mimic_core::ports::{CassetteSet, Store};
use mimic_core::rules::RuleSet;
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::execute;
use serde_json::{json, Map, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use wf_kit::args::Parsed;

pub const TOKEN_KEY: &str = "GH_PROJECTS_RECORD_TOKEN";
pub const BOARD_PREFIX: &str = "mla-recording-test-";
const SCOPES: [&str; 2] = ["project", "read:org"];
const GH_ENV: [&str; 14] = ["PATH", "HOME", "GH_CONFIG_DIR", "TMPDIR", "SSL_CERT_FILE", "SSL_CERT_DIR", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy", "XDG_CONFIG_HOME", "LANG"];
const CASE_COUNT: usize = 8;
const DEFAULT_STALE_MINUTES: i64 = 60;

struct Gh {
    program: PathBuf,
    token: String,
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Gh {
    fn scrub(&self, text: &str) -> String {
        text.replace(&self.token, "<token>")
    }

    fn run(&self, args: &[&str]) -> Result<Output, String> {
        let mut cmd = Command::new(&self.program);
        cmd.args(args).env_clear();
        for name in GH_ENV {
            if let Some(v) = std::env::var_os(name) {
                cmd.env(name, v);
            }
        }
        let out = cmd
            .env("GH_TOKEN", &self.token)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("{} did not start: {e}", self.program.display()))?;
        Ok(Output { code: out.status.code().unwrap_or(-1), stdout: self.scrub(&String::from_utf8_lossy(&out.stdout)), stderr: self.scrub(&String::from_utf8_lossy(&out.stderr)) })
    }

    fn graphql(&self, query: &str, vars: &[(&str, &str)]) -> Result<Value, String> {
        let query_arg = format!("query={query}");
        let var_args: Vec<String> = vars.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let mut args = vec!["api", "graphql", "-f", query_arg.as_str()];
        for v in &var_args {
            args.push("-f");
            args.push(v);
        }
        let out = self.run(&args)?;
        if out.code != 0 {
            return Err(format!("gh api graphql exited {}: {}", out.code, brief(&out.stderr, &out.stdout)));
        }
        let body: Value = serde_json::from_str(&out.stdout).map_err(|e| format!("gh api graphql printed no JSON: {e}"))?;
        match body.get("errors").and_then(Value::as_array).and_then(|e| e.first()) {
            Some(first) => Err(format!("gh api graphql: {}", first["message"].as_str().unwrap_or("refused"))),
            None => Ok(body),
        }
    }
}

fn brief(stderr: &str, stdout: &str) -> String {
    let said = if stderr.trim().is_empty() { stdout } else { stderr };
    said.trim().chars().take(300).collect()
}

/// The scopes `gh api -i user` reports in `X-OAuth-Scopes`; none for a token that sends no such header (a fine-grained one).
fn granted_scopes(headers: &str) -> Vec<String> {
    headers
        .lines()
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("x-oauth-scopes")).map(|(_, v)| v))
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// `2026-11-21T10:00:00Z` in milliseconds since the epoch.
fn parse_iso_ms(text: &str) -> Option<i64> {
    let (date, time) = text.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|n| n.parse::<i64>().ok());
    let mut t = time.split('.').next()?.split(':').map(|n| n.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let (h, mi, s) = (t.next()??, t.next()??, t.next()??);
    Some(((days_from_civil(y, mo, da) * 24 + h) * 60 + mi) * 60_000 + s * 1000)
}

fn find_gh(override_path: Option<&str>) -> Result<PathBuf, Fail> {
    if let Some(p) = override_path {
        return Ok(PathBuf::from(p));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("gh"))
        .find(|c| c.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
        .ok_or_else(|| fail("gh is not on PATH", "install the GitHub CLI (the hosted runner has it), or pass --gh <path>"))
}

fn token() -> Result<String, Fail> {
    secrets::load_values(&[TOKEN_KEY]).remove(TOKEN_KEY).flatten().filter(|v| !v.is_empty()).ok_or_else(|| {
        fail(format!("{TOKEN_KEY} is neither in the environment nor in the vault"), format!("put a classic token with the scopes project and read:org in the vault as {TOKEN_KEY} (scope ci-deploy), then run the step with SECRETS_BACKEND=bitwarden and the vault token"))
    })
}

fn check_scopes(gh: &Gh) -> Result<Vec<String>, Fail> {
    let out = gh.run(&["api", "-i", "user"]).map_err(|e| fail(e, "check gh"))?;
    if out.code != 0 {
        return Err(fail(format!("the token was refused by GitHub (gh exited {}: {})", out.code, brief(&out.stderr, &out.stdout)), format!("mint a new classic token with the scopes project and read:org and store it as {TOKEN_KEY}")));
    }
    let granted = granted_scopes(&out.stdout);
    let has = |s: &str| granted.iter().any(|g| g == s || (s == "read:org" && (g == "write:org" || g == "admin:org")));
    let missing: Vec<&str> = SCOPES.iter().copied().filter(|s| !has(s)).collect();
    if missing.is_empty() {
        return Ok(granted);
    }
    Err(Fail {
        error: format!("the token lacks the scope {}: it has {}", missing.join(", "), if granted.is_empty() { "none (a fine-grained token sends no X-OAuth-Scopes: use a classic token)".to_string() } else { granted.join(", ") }),
        next: format!("mint a classic token with the scopes {} and store it as {TOKEN_KEY}", SCOPES.join(" and ")),
        evidence: json!({"missing_scopes": missing, "granted_scopes": granted}),
    })
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Board {
    id: String,
    title: String,
    number: u64,
}

fn read_state(path: &Path) -> Option<Board> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn delete_board(gh: &Gh, id: &str) -> Result<(), String> {
    gh.graphql("mutation($project:ID!){deleteProjectV2(input:{projectId:$project}){projectV2{id}}}", &[("project", id)]).map(|_| ())
}

/// Deletes the boards named `mla-recording-test-*` that are older than `stale_ms`, and none other; returns (deleted titles, kept count).
fn stale_guard(gh: &Gh, stale_ms: i64, now_ms: i64) -> Result<(Vec<String>, usize), String> {
    let listing = gh.graphql("query{viewer{projectsV2(first:100){nodes{id number title createdAt}}}}", &[])?;
    let nodes = listing["data"]["viewer"]["projectsV2"]["nodes"].as_array().cloned().unwrap_or_default();
    let (mut deleted, mut kept) = (Vec::new(), 0);
    for n in &nodes {
        let (Some(id), Some(title)) = (n["id"].as_str(), n["title"].as_str()) else { continue };
        let ours = title.len() > BOARD_PREFIX.len() && title.starts_with(BOARD_PREFIX);
        let old = n["createdAt"].as_str().and_then(parse_iso_ms).is_some_and(|created| now_ms - created > stale_ms);
        if ours && old {
            delete_board(gh, id)?;
            deleted.push(title.to_string());
        } else {
            kept += 1;
        }
    }
    Ok((deleted, kept))
}

struct Setup {
    board: Board,
    login: String,
    notes_field: String,
    estimate_field: String,
    item: String,
}

fn id_at(v: &Value, pointer: &str, what: &str) -> Result<String, String> {
    v.pointer(pointer).and_then(Value::as_str).map(String::from).ok_or_else(|| format!("GitHub's answer to {what} has no id"))
}

fn setup(gh: &Gh, title: &str, state: &Path) -> Result<Setup, String> {
    let viewer = gh.graphql("query{viewer{id login}}", &[])?;
    let (owner, login) = (id_at(&viewer, "/data/viewer/id", "the viewer")?, id_at(&viewer, "/data/viewer/login", "the viewer")?);
    let created = gh.graphql("mutation($owner:ID!,$title:String!){createProjectV2(input:{ownerId:$owner,title:$title}){projectV2{id number url}}}", &[("owner", &owner), ("title", title)])?;
    let board = Board { id: id_at(&created, "/data/createProjectV2/projectV2/id", "createProjectV2")?, title: title.to_string(), number: created.pointer("/data/createProjectV2/projectV2/number").and_then(Value::as_u64).ok_or("createProjectV2 answered no number")? };
    std::fs::write(state, serde_json::to_string(&board).expect("prints")).map_err(|e| format!("{}: {e}", state.display()))?;
    let field = |kind: &str, name: &str| -> Result<String, String> {
        let query = format!("mutation($project:ID!,$name:String!){{createProjectV2Field(input:{{projectId:$project,dataType:{kind},name:$name}}){{projectV2Field{{... on ProjectV2Field{{id name}}}}}}}}");
        id_at(&gh.graphql(&query, &[("project", &board.id), ("name", name)])?, "/data/createProjectV2Field/projectV2Field/id", "createProjectV2Field")
    };
    let notes_field = field("TEXT", "notes")?;
    let estimate_field = field("NUMBER", "estimate")?;
    let draft = gh.graphql("mutation($project:ID!,$title:String!){addProjectV2DraftIssue(input:{projectId:$project,title:$title}){projectItem{id}}}", &[("project", &board.id), ("title", "recorded item")])?;
    let item = id_at(&draft, "/data/addProjectV2DraftIssue/projectItem/id", "addProjectV2DraftIssue")?;
    Ok(Setup { board, login, notes_field, estimate_field, item })
}

struct Case {
    name: &'static str,
    tool: &'static str,
    args: Value,
    succeeds: bool,
}

fn cases(s: &Setup) -> Vec<Case> {
    let (owner, number) = (s.login.as_str(), s.board.number);
    let (project, item) = (s.board.id.as_str(), s.item.as_str());
    vec![
        Case { name: "lists the viewer's projects and those of the organizations", tool: "hosted_project_list", args: json!({"provider": "github"}), succeeds: true },
        Case { name: "only the owner when no number is given", tool: "hosted_project_resolve", args: json!({"provider": "github", "owner": owner, "owner_type": "user"}), succeeds: true },
        Case { name: "a user's project by number", tool: "hosted_project_resolve", args: json!({"provider": "github", "owner": owner, "owner_type": "user", "project_number": number}), succeeds: true },
        Case { name: "a project that is not there", tool: "hosted_project_resolve", args: json!({"provider": "github", "owner": owner, "owner_type": "user", "project_number": 999_999}), succeeds: false },
        Case { name: "lists a project's views", tool: "hosted_project_views", args: json!({"provider": "github", "owner": owner, "owner_type": "user", "project_number": number}), succeeds: true },
        Case { name: "a text field", tool: "hosted_project_field_set", args: json!({"provider": "github", "project": project, "item": item, "field": s.notes_field, "kind": "text", "value": "recorded by wf mimic record-projects"}), succeeds: true },
        Case { name: "a number field", tool: "hosted_project_field_set", args: json!({"provider": "github", "project": project, "item": item, "field": s.estimate_field, "kind": "number", "number_value": 3}), succeeds: true },
        Case { name: "clears the value of a field", tool: "hosted_project_field_clear", args: json!({"provider": "github", "project": project, "item": item, "field": s.notes_field}), succeeds: true },
    ]
}

/// A `gh` that logs every call to `<rec>/<n>/{argv,stdin,stdout,stderr,code}` and then runs the real one with the token in `GH_TOKEN`
/// (the executor never passes a token variable on, so the token is read from a file only this process can read).
fn shim_script(rec: &Path, token_file: &Path, real: &Path) -> Result<String, String> {
    for p in [rec, token_file, real] {
        if p.to_string_lossy().contains('\'') {
            return Err(format!("{} holds a quote", p.display()));
        }
    }
    Ok(format!(
        "#!/bin/sh\nPATH=/usr/local/bin:/usr/bin:/bin\nn=0\nwhile ! mkdir '{rec}/'\"$n\" 2>/dev/null; do n=$((n+1)); done\nd='{rec}/'\"$n\"\nprintf '%s\\0' \"$@\" > \"$d/argv\"\ncat > \"$d/stdin\"\nGH_TOKEN=\"$(cat '{tok}')\"\nexport GH_TOKEN\n'{real}' \"$@\" < \"$d/stdin\" > \"$d/stdout\" 2> \"$d/stderr\"\ncode=$?\nprintf '%s' \"$code\" > \"$d/code\"\ncat \"$d/stdout\"\ncat \"$d/stderr\" >&2\nexit \"$code\"\n",
        rec = rec.display(),
        tok = token_file.display(),
        real = real.display()
    ))
}

struct Call {
    argv: Vec<String>,
    stdin: String,
    stdout: String,
    stderr: String,
    code: i64,
}

fn read_calls(rec: &Path) -> Vec<Call> {
    let mut dirs: Vec<(u64, PathBuf)> = std::fs::read_dir(rec).into_iter().flatten().flatten().filter_map(|e| Some((e.file_name().to_str()?.parse().ok()?, e.path()))).collect();
    dirs.sort();
    let text = |d: &Path, f: &str| std::fs::read_to_string(d.join(f)).unwrap_or_default();
    dirs.iter()
        .map(|(_, d)| Call {
            argv: text(d, "argv").split('\0').filter(|a| !a.is_empty()).map(String::from).collect(),
            stdin: text(d, "stdin"),
            stdout: text(d, "stdout"),
            stderr: text(d, "stderr"),
            code: text(d, "code").trim().parse().unwrap_or(-1),
        })
        .collect()
}

fn step(dir: Dir, channel: Option<&str>, kind: StepKind, payload: Value) -> Step {
    Step { dir, channel: channel.map(String::from), kind, rid: None, len: None, payload }
}

struct Ran {
    case: Case,
    id: String,
    ok: bool,
    code: Option<String>,
    interaction: Interaction,
}

fn run_cases(gh: &Gh, set: &Setup, work: &Path) -> Result<Vec<Ran>, String> {
    let (rec, shim_dir, token_file) = (work.join("rec"), work.join("shim"), work.join("token"));
    for d in [&rec, &shim_dir] {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&token_file, &gh.token).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    let shim = shim_dir.join("gh");
    std::fs::write(&shim, shim_script(&rec, &token_file, &gh.program)?).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    let root = work.join("root");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let ex = LocalExecutor::new(&root)?.with_hosted_path(&shim_dir.display().to_string());
    let mut ran: Vec<Ran> = Vec::new();
    let mut seen = 0;
    for (n, case) in cases(set).into_iter().enumerate() {
        let id = format!("r{:02}", n + 1);
        let result = execute(&ex, &id, case.tool, &case.args, &mut |_, _| {}, &mut |_| {});
        let (ok, code, frame) = match &result {
            Ok(v) => (true, None, json!({"id": id, "ok": true, "result": v})),
            Err(e) => (false, Some(e.code.as_str().to_string()), json!({"id": id, "ok": false, "error": {"code": e.code.as_str(), "message": e.message}})),
        };
        let calls = read_calls(&rec);
        let mut steps = Vec::new();
        let mut starts = Vec::new();
        for c in calls.iter().skip(seen) {
            steps.push(step(Dir::Call, Some("gh"), StepKind::Json, json!({"args": c.argv})));
            if !c.stdin.is_empty() {
                steps.push(step(Dir::Stdin, Some("gh"), StepKind::Text, json!(c.stdin)));
            }
            steps.push(step(Dir::Stdout, Some("gh"), StepKind::Text, json!(c.stdout)));
            steps.push(step(Dir::Stderr, Some("gh"), StepKind::Text, json!(c.stderr)));
            steps.push(step(Dir::Exit, Some("gh"), StepKind::Exit, json!(c.code)));
            starts.push(json!(std::iter::once("gh".to_string()).chain(c.argv.iter().cloned()).collect::<Vec<_>>()));
        }
        seen = calls.len();
        let mut response = step(Dir::S2c, None, StepKind::Json, frame);
        response.rid = Some(id.clone());
        steps.push(response);
        let index = ran.iter().filter(|r| r.case.tool == case.tool).count();
        let interaction = Interaction {
            id: format!("{}/{index:02}", case.tool),
            row: None,
            op: Some(case.tool.to_string()),
            input: json!({"name": case.name, "request": {"args": case.args, "id": id, "tool": case.tool}}),
            steps,
            effects: vec![Effect { kind: "cli-starts".into(), data: Map::from_iter([("starts".to_string(), Value::Array(starts))]) }],
            outcome: Outcome::default(),
            tags: Vec::new(),
        };
        let failed = ok != case.succeeds;
        ran.push(Ran { case, id, ok, code, interaction });
        if failed {
            break;
        }
    }
    Ok(ran)
}

fn gh_version(gh: &Gh) -> Option<String> {
    let out = gh.run(&["--version"]).ok()?;
    out.stdout.lines().next()?.split_whitespace().nth(2).map(String::from)
}

fn sealed(interactions: Vec<Interaction>, version: Option<String>, board: &Board) -> CassetteSet {
    let rules = RuleSet::new(Vec::new());
    let on = std::env::var("GITHUB_RUN_ID").map_or_else(|_| "a local run".to_string(), |id| format!("GitHub Actions run {id}"));
    let provenance = Provenance {
        kind: Kind::Recorded,
        target: Some(Target { name: "gh".into(), pin_hash: Some(sha256_label(pin_text(version.as_deref()).as_bytes())), version, pin_file: Some("pin.json".into()), ..Target::default() }),
        driver: Some(DriverInfo { name: "record-projects".into(), version: env!("CARGO_PKG_VERSION").into(), protocol: "mimic-driver/1".into(), toolchain_lock_hash: None }),
        tool: ToolInfo { mimic: env!("CARGO_PKG_VERSION").into(), git: option_env!("WF_BUILD_SHA").map(String::from) },
        seed: None,
        rules_hash: rules.hash(),
        corpus_hash: None,
        recorded_at: Some(wf_kit::time::iso(wf_kit::time::now_ms())),
        recorded_on: Some(on),
        source: Some("the hosted_project_* actions of rca-exec run against a throwaway Projects v2 board of a real GitHub account by wf mimic record-projects: every gh call's argv, stdin, stdout and exit code is what the real gh did".into()),
        note: Some(format!("recorded; the board {} was deleted afterwards, so its node ids are real but name nothing that still exists. The hosted_project_list reply holds every project and organization the account can see (titles, URLs, organization logins), so the account must hold only test boards. Only the project actions are recorded; the repository-bound actions (comments, issue update and types, pull request and issue reads) need a repository the token may write and stay authored doubles (docs/gaps/orca-github-project-rows.md)", board.title)),
        imported_from: Vec::new(),
    };
    let header = Header { schema: SCHEMA.into(), provenance, rules: RefHash { path: "rules.json".into(), hash: rules.hash() }, corpus: None, interactions: 0, content_hash: String::new() };
    let mut cassette = Cassette { header, interactions };
    cassette.seal();
    CassetteSet { cassette, rules, corpus: None }
}

/// The pin of a recording is the gh version it ran; `verify` hashes this file. The [reference] only carries the pin: the set is made by `wf mimic record-projects`, not by `wf mimic record`.
const MIMIC_TOML: &str = "# Recorded by wf mimic record-projects, which needs a token and a real gh: wf mimic record does not apply.\nrules = \"rules.json\"\n[reference]\ndriver = \"stdio\"\ncommand = [\"wf\", \"mimic\", \"record-projects\"]\npin = \"pin.json\"\n";

fn pin_text(version: Option<&str>) -> String {
    format!("{}\n", json!({"cli": "gh", "version": version.unwrap_or("unknown")}))
}

fn write_set(dir: &Path, set: &CassetteSet, version: Option<&str>) -> Result<Vec<String>, String> {
    let root = dir.parent().ok_or("the set directory has no parent")?;
    let name = dir.file_name().and_then(|n| n.to_str()).ok_or("the set directory has no name")?;
    mimic_store(root).write(name, set).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("mimic.toml"), MIMIC_TOML).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("pin.json"), pin_text(version)).map_err(|e| e.to_string())?;
    Ok(["cassette.json", "interactions.jsonl", "rules.json", "mimic.toml", "pin.json"].iter().map(|f| dir.join(f).display().to_string()).collect())
}

fn mimic_store(root: &Path) -> impl Store {
    crate::adapters::FsStore::new(root)
}

fn holds_token(set: &CassetteSet, token: &str) -> bool {
    set.cassette.jsonl().contains(token) || set.cassette.header_json().contains(token)
}

fn clean_id(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

pub fn record_projects(p: &Parsed) -> Done {
    let dir = std::path::absolute(&p.positional[0]).map_err(|e| fail(e.to_string(), "wf mimic record-projects --help"))?;
    let teardown_only = p.flag("teardown");
    let stale_ms = p.val("stale-minutes").map_or(Ok(DEFAULT_STALE_MINUTES), |m| m.parse::<i64>().map_err(|_| fail(format!("--stale-minutes {m} is not a number"), "wf mimic record-projects --help")))? * 60_000;
    let run_id = clean_id(&p.val("run-id").map(String::from).or_else(|| std::env::var("GITHUB_RUN_ID").ok()).unwrap_or_else(|| std::process::id().to_string()));
    let state = p.val("state").map_or_else(|| std::env::temp_dir().join("mimic-record-projects.state.json"), PathBuf::from);
    let gh = Gh { program: find_gh(p.val("gh"))?, token: token()? };
    let scopes = if teardown_only { Vec::new() } else { check_scopes(&gh)? };

    let mut problems: Vec<String> = Vec::new();
    let guard = match stale_guard(&gh, stale_ms, wf_kit::time::now_ms()) {
        Ok((deleted, kept)) => json!({"deleted": deleted, "kept": kept}),
        Err(e) => {
            problems.push(format!("the stale-board guard failed: {e}"));
            json!({"deleted": [], "kept": 0, "error": e})
        }
    };

    let title = format!("{BOARD_PREFIX}{run_id}");
    let work = std::env::temp_dir().join(format!("mimic-record-projects-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| fail(format!("{}: {e}", work.display()), "check TMPDIR is writable"))?;
    let _ = std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700));

    let mut ran: Vec<Ran> = Vec::new();
    let mut set_up: Option<Setup> = None;
    let mut run_error: Option<String> = None;
    if !teardown_only {
        match setup(&gh, &title, &state) {
            Ok(s) => set_up = Some(s),
            Err(e) => run_error = Some(format!("setup: {e}")),
        }
        if let Some(s) = &set_up {
            match run_cases(&gh, s, &work) {
                Ok(r) => ran = r,
                Err(e) => run_error = Some(format!("run: {e}")),
            }
        }
    }

    // teardown runs whatever the run did: the board of this run (known from the state file even when setup died half way)
    let board = set_up.as_ref().map(|s| s.board.clone()).or_else(|| read_state(&state));
    let mut deleted = json!(null);
    if let Some(b) = &board {
        match delete_board(&gh, &b.id) {
            Ok(()) => {
                let _ = std::fs::remove_file(&state);
                deleted = json!(true);
            }
            Err(e) => {
                problems.push(format!("teardown could not delete the board {} ({}): {e}", b.title, b.id));
                deleted = json!(false);
            }
        }
    }
    let _ = std::fs::remove_dir_all(work.join("shim"));
    let _ = std::fs::remove_file(work.join("token"));

    let mismatched = ran.iter().find(|r| r.ok != r.case.succeeds);
    if let Some(r) = mismatched {
        run_error = Some(format!("{} ({}) {}", r.case.tool, r.case.name, if r.ok { "succeeded where a refusal was expected" } else { "failed" }));
    }
    let actions: Vec<Value> = ran.iter().map(|r| json!({"id": r.id, "tool": r.case.tool, "name": r.case.name, "ok": r.ok, "expected_ok": r.case.succeeds, "code": r.code})).collect();
    let run_ok = !teardown_only && run_error.is_none() && ran.len() == CASE_COUNT;
    let mut files: Vec<String> = Vec::new();
    if run_ok {
        let version = gh_version(&gh);
        let set = sealed(ran.iter().map(|r| r.interaction.clone()).collect(), version.clone(), board.as_ref().expect("a run has a board"));
        if holds_token(&set, &gh.token) {
            problems.push("a recorded value holds the token: nothing was written".to_string());
        } else {
            match write_set(&dir, &set, version.as_deref()) {
                Ok(f) => files = f,
                Err(e) => problems.push(format!("writing the set: {e}")),
            }
        }
    }
    let _ = std::fs::remove_dir_all(&work);

    let ok = (teardown_only || run_ok) && problems.is_empty();
    let error = run_error.clone().or_else(|| problems.first().cloned());
    let next = if ok { "upload the set as the artifact mimic-set-<set>".to_string() } else { "read error and problems; a board left behind is deleted by the next run's guard after an hour".to_string() };
    let out = json!({
        "ok": ok,
        "set": dir.file_name().and_then(|n| n.to_str()),
        "dir": dir.display().to_string(),
        "mode": if teardown_only { "teardown" } else { "record" },
        "scopes": scopes,
        "board": board.as_ref().map(|b| json!({"title": b.title, "number": b.number, "deleted": deleted})),
        "stale": guard,
        "actions": actions,
        "recorded": if files.is_empty() { 0 } else { ran.len() },
        "files": files,
        "error": error,
        "problems": problems,
        "warnings": [],
        "next": next,
    });
    Ok((out, i32::from(!ok)))
}

