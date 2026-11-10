//! The Orca oracle on mimic (plan X6): the phone's own transport (the pinned `direct-rpc-client.ts` under Node, the `orca` driver of
//! `tooling/mimic-drivers`) as the scripted client of two targets. The reference is orcad at the pin on the VM; here it is a scripted host
//! double, so the set it records is `authored`: its diff can be clean and it still serves no parity row. The candidate is the real
//! `orca-serve` of the chain, paired by the link `orca-serve pair` prints.
use crate::mimic_x2::{clis, executable};
use crate::rig::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

const SUITE: &str = "wf-mimic-orca-files";

pub(crate) fn root() -> PathBuf {
    clis().join("..")
}

fn set_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("cassettes/orca-files")
}

fn drivers() -> PathBuf {
    root().join("tooling/mimic-drivers/src")
}

pub(crate) fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

fn wf_mimic() -> PathBuf {
    std::env::var_os("WF_MIMIC_BIN").map(PathBuf::from).unwrap_or_else(|| executable(&["build", "--locked", "-p", "wf-mimic", "--bin", "wf-mimic"]))
}

/// The scripted host: `host-double.ts` answering from a method table, paired by the link it prints, until it is dropped.
struct Double {
    child: Child,
    stdin: Option<ChildStdin>,
    url: String,
}

impl Double {
    fn start(methods: &Value) -> Double {
        let mut child = Command::new("node")
            .arg(drivers().join("orca/host-double.ts"))
            .env("ORCA_DOUBLE_METHODS", methods.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("node starts the host double");
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut line).unwrap();
        let ready: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("the double printed {line:?} ({e})"));
        let url = ready["pairing"]["url"].as_str().expect("the ready line carries the pairing link").to_string();
        Double { stdin: child.stdin.take(), child, url }
    }
}

impl Drop for Double {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

pub(crate) struct Run {
    pub(crate) code: Option<i32>,
    pub(crate) json: Value,
}

/// `wf-mimic mimic <args>` with the host the scripted phone pairs with named by `pairing`, as a person's environment would.
pub(crate) fn mimic(args: &[&str], pairing: &str, out_dir: &Path) -> Run {
    let out = Command::new(wf_mimic())
        .arg("mimic")
        .args(args)
        .env("ORCA_PAIRING_URI", pairing)
        .env("WF_TEST_OUTPUT_DIR", out_dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("wf-mimic printed no JSON ({e}): {}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    Run { code: out.status.code(), json }
}

pub(crate) fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("orca-serve-x6-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(crate) fn out_dir(scratch: &Path) -> PathBuf {
    std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| scratch.join("out"))
}

/// The files the set's fixture says the host holds, written into the chain's repository.
fn hold_fixture(chain: &Chain) {
    for (path, text) in read_json(&set_dir().join("fixture.json")).as_object().unwrap() {
        chain.repo.write(path, text.as_str().unwrap());
    }
}

#[test]
fn given_the_double_as_reference_when_orca_serve_is_the_candidate_then_each_row_is_clean_and_the_double_serves_none() {
    // Given the chain's host holding the set's files, and the set recorded from the double (kind authored, no pin claimed)
    let set = set_dir();
    let header = read_json(&set.join("cassette.json"));
    assert_eq!((header["provenance"]["kind"].clone(), header["provenance"]["target"].get("pin_hash").cloned()), (json!("authored"), None), "{header}");
    let chain = Chain::start(None);
    hold_fixture(&chain);
    let scratch = scratch("clean");
    let out = out_dir(&scratch);

    // When wf-mimic replays the set against orca-serve as the candidate, the phone's transport driven by the link orca-serve pair printed
    let run = mimic(&["replay", set.to_str().unwrap(), "--candidate", "orca-serve", "--suite", SUITE], &chain.pair_url(), &out);

    // Then every row is clean or allowlisted (the allowlist is bound to the pin), and the trace says the cassette is authored, so no row is served by it
    assert_eq!((run.code, run.json["severity"].clone()), (Some(0), json!("none")), "{}\n{}", run.json, chain.evidence());
    let trace = read_json(&out.join(SUITE).join("trace.json"));
    let calls: Vec<Value> = trace["tests"].as_array().unwrap().iter().flat_map(|t| t["calls"].as_array().unwrap().clone()).collect();
    let rows: Vec<String> = read_json(&set.join("corpus.json"))["generators"][0]["cases"].as_array().unwrap().iter().map(|c| c["row"].as_str().unwrap().to_string()).collect();
    assert_eq!(calls.len(), rows.len());
    assert!(calls.iter().all(|c| c["diff"] == "clean" || c["diff"] == "allowlisted"), "{trace}");
    assert_eq!((trace["tests"][0]["cassette"].clone(), trace["app"].clone()), (json!("authored"), json!("orca")));
    let allow = read_json(&set.join("allowlist.json"));
    let pin = Command::new("sha256sum").arg(root().join("tooling/orca-conformance/orca-pin.json")).output().unwrap();
    let pin = format!("sha256:{}", String::from_utf8_lossy(&pin.stdout).split_whitespace().next().unwrap());
    assert!(allow["ignored"].as_array().unwrap().iter().all(|e| e["pin_hash"] == json!(pin)), "an allowlist entry is not bound to the current pin {pin}: {allow}");
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn given_a_candidate_that_answers_otherwise_when_the_set_is_replayed_then_the_difference_is_breaking_and_names_the_row() {
    // Given the set and a second double as the candidate whose stat of README.md reports another size
    let mut methods = read_json(&set_dir().join("double.json"));
    methods["files.stat"][0]["result"]["size"] = json!(15);
    let candidate = Double::start(&methods);
    let scratch = scratch("break");

    // When the set is replayed against it
    let run = mimic(&["replay", set_dir().to_str().unwrap(), "--candidate", "orca-serve"], &candidate.url, &out_dir(&scratch));

    // Then the replay fails on that interaction and no other
    let broken: Vec<&str> = run.json["results"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!((run.code, broken), (Some(1), vec!["files-stat-file"]), "{}", run.json);
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn given_the_double_when_the_set_is_recorded_again_then_the_interactions_are_the_committed_ones() {
    // Given a copy of the set whose paths are absolute, and the double named as a stand-in
    let scratch = scratch("record");
    let copy = scratch.join("orca-files");
    std::fs::create_dir_all(&copy).unwrap();
    for name in ["rules.json", "corpus.json", "allowlist.json", "double.json"] {
        std::fs::copy(set_dir().join(name), copy.join(name)).unwrap();
    }
    let toml = std::fs::read_to_string(set_dir().join("mimic.toml")).unwrap().replace("../../../../", &format!("{}/", root().canonicalize().unwrap().display()));
    std::fs::write(copy.join("mimic.toml"), toml).unwrap();
    let double = Double::start(&read_json(&set_dir().join("double.json")));

    // When it is recorded
    let run = mimic(&["record", copy.to_str().unwrap(), "--stand-in", "scripted Orca host double"], &double.url, &out_dir(&scratch));

    // Then the double is a stand-in (authored, no pin hash), and what it recorded is the committed interactions byte for byte
    assert_eq!((run.code, run.json["provenance"]["kind"].clone()), (Some(0), json!("authored")), "{}", run.json);
    assert_eq!(std::fs::read_to_string(copy.join("interactions.jsonl")).unwrap(), std::fs::read_to_string(set_dir().join("interactions.jsonl")).unwrap());
    std::fs::remove_dir_all(&scratch).ok();
}

/// A `mimic-driver/1` client over the `orca` driver's process: the cases it is given, the replies it sends back.
fn drive(pairing: &str, cases: &[Value]) -> Vec<Value> {
    let mut child = Command::new("node")
        .arg(drivers().join("cli.ts"))
        .args(["serve", "orca"])
        .env("ORCA_PAIRING_URI", pairing)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("node starts the orca driver");
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    writeln!(stdin, "{}", json!({"hello": true})).unwrap();
    let hello: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert!(hello["hello"]["driver"]["name"].is_string(), "{hello}");
    let replies = cases
        .iter()
        .map(|c| {
            writeln!(stdin, "{}", json!({"id": c["id"], "op": c["op"], "input": c["input"]})).unwrap();
            serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
        })
        .collect();
    drop(stdin);
    let _ = child.wait();
    replies
}

fn envelope_clean(p: &Value) -> bool {
    match p["ok"].as_bool() {
        Some(true) => p.get("result").is_some(),
        Some(false) => p["error"]["code"].is_string() && p["error"]["message"].is_string(),
        None => false,
    }
}

#[test]
fn given_payloads_derived_from_the_pinned_zod_schemas_when_orca_serve_is_asked_the_files_rows_then_every_answer_is_a_well_formed_response() {
    // Given the derived payload of every phone row of `files` (the driver's schema verb over the pinned tree), as sent and with the workspace selector grounded
    let derived = Command::new("node").arg(drivers().join("cli.ts")).args(["schema", "orca", "files", "--cases"]).output().unwrap();
    assert!(derived.status.success(), "{}", String::from_utf8_lossy(&derived.stderr));
    let cases: Vec<Value> = serde_json::from_slice(&derived.stdout).unwrap();
    let ground = |case: &Value| {
        let mut c = case.clone();
        c["id"] = json!(format!("{}-grounded", case["id"].as_str().unwrap()));
        let params = &mut c["input"]["requests"][0]["params"];
        params["worktree"] = json!("id:w1::");
        c["input"]["requests"][0]["timeout_ms"] = json!(15_000);
        c
    };
    let all: Vec<Value> = cases.iter().map(ground).chain(cases.iter().cloned().map(|mut c| { c["input"]["requests"][0]["timeout_ms"] = json!(15_000); c })).collect();
    let chain = Chain::start(None);
    hold_fixture(&chain);

    // When the phone's transport sends each to orca-serve
    let replies = drive(&chain.pair_url(), &all);

    // Then every request is answered over a connected transport with a well-formed response, and the report counts the rest
    let mut report = Vec::new();
    for (case, reply) in all.iter().zip(&replies) {
        assert_eq!(reply["outcome"]["status"], "ok", "{case}: {reply}\n{}", chain.evidence());
        let answer = reply["steps"].as_array().unwrap().iter().find(|s| s["dir"] == "s2c").map(|s| s["payload"].clone()).unwrap_or(Value::Null);
        report.push(json!({"id": case["id"], "row": case["row"], "clean": envelope_clean(&answer), "ok": answer["ok"], "code": answer["error"]["code"]}));
    }
    let count = |f: &dyn Fn(&Value) -> bool, part: &dyn Fn(&str) -> bool| report.iter().filter(|r| part(r["id"].as_str().unwrap()) && f(r)).count();
    let grounded = |id: &str| id.ends_with("-grounded");
    let ungrounded = |id: &str| !id.ends_with("-grounded");
    let codes = |part: &dyn Fn(&str) -> bool| {
        let mut by: std::collections::BTreeMap<String, usize> = Default::default();
        for r in report.iter().filter(|r| part(r["id"].as_str().unwrap())) {
            *by.entry(r["code"].as_str().unwrap_or("ok").to_string()).or_default() += 1;
        }
        by
    };
    let summary = json!({
        "rows": cases.len(),
        "grounded": {"clean": count(&|r| r["clean"] == true, &grounded), "answered_ok": count(&|r| r["ok"] == true, &grounded), "refused_invalid_argument": count(&|r| r["code"] == "invalid_argument", &grounded), "by_code": codes(&grounded)},
        "as_derived": {"clean": count(&|r| r["clean"] == true, &ungrounded), "answered_ok": count(&|r| r["ok"] == true, &ungrounded), "refused_invalid_argument": count(&|r| r["code"] == "invalid_argument", &ungrounded), "by_code": codes(&ungrounded)},
        "report": report,
    });
    let scratch = scratch("spike");
    let dir = out_dir(&scratch);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("orca-files-schema-spike.json"), serde_json::to_string_pretty(&summary).unwrap()).unwrap();
    eprintln!("SCHEMA SPIKE {summary}");
    assert_eq!((summary["grounded"]["clean"].clone(), summary["as_derived"]["clean"].clone()), (json!(cases.len()), json!(cases.len())), "{summary}");
    std::fs::remove_dir_all(&scratch).ok();
}
