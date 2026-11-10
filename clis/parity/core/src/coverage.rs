//! The coverage table of one app: every phone row with its state (`pending`, `not-offered`, `excluded`, `flagged`, and `served` only for a
//! row the evidence names: a passing test and a trace hit, `evidence.rs`), its milestone and, for a T3 command, the target it maps to. The
//! decisions file is the hand-kept input; the methods list, the evidence and the mapper's own tables are generated or read.
use crate::evidence::Entry;
use crate::methods::{pretty, MethodsList, Row};
use crate::ports::Evidence;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const PIN_BUMP: &str = "parity: pin-bump";

#[derive(Debug, Default, Clone)]
pub struct Decisions {
    /// Row id, or a prefix ending in `*`, to milestone; the longest match wins.
    pub milestones: Vec<(String, String)>,
    /// Command row id to `agent:<method>`, `state:<key>` or `t3-local`.
    pub targets: BTreeMap<String, String>,
    /// Row id to `{state: not-offered|excluded, reason, decidedBy}`.
    pub rows: BTreeMap<String, Map<String, Value>>,
    /// Row id to why a trace hit does not serve it yet: the mapper only refuses it, or answers it without doing it. Such a row stays pending
    /// until the note is removed in the commit that makes the answer functional.
    pub reverify: BTreeMap<String, String>,
    /// Row id, or a prefix ending in `*`, to why a trace may name it though the phone does not reach it: the crawl drives the whole host surface.
    pub trace_noise: Vec<(String, String)>,
}

impl Decisions {
    pub fn parse(text: &str) -> Result<Decisions, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        let strings = |key: &str| -> Vec<(String, String)> {
            v[key].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str().filter(|v| !v.trim().is_empty())?.to_string()))).collect()).unwrap_or_default()
        };
        Ok(Decisions {
            milestones: strings("milestones"),
            targets: strings("targets").into_iter().collect(),
            rows: v["decisions"].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_object()?.clone()))).collect()).unwrap_or_default(),
            reverify: strings("reverify").into_iter().collect(),
            trace_noise: strings("traceNoise"),
        })
    }

    /// Whether the row's decision names the mimic oracle (`"oracle": "mimic"`): only a replay the oracle labelled clean can serve it.
    pub fn mimic(&self, id: &str) -> bool {
        self.rows.get(id).and_then(|d| d.get("oracle")).and_then(Value::as_str) == Some("mimic")
    }

    /// Whether a trace naming this row is explained by a `traceNoise` entry rather than a mistyped row.
    pub fn is_trace_noise(&self, id: &str) -> bool {
        self.trace_noise.iter().any(|(p, _)| p == id || p.strip_suffix('*').is_some_and(|prefix| id.starts_with(prefix)))
    }

    fn milestone(&self, id: &str) -> Option<&str> {
        self.milestones
            .iter()
            .filter(|(p, _)| p == id || p.strip_suffix('*').is_some_and(|prefix| id.starts_with(prefix)))
            .max_by_key(|(p, _)| (p == id, p.len()))
            .map(|(_, m)| m.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub rule: &'static str,
    pub message: String,
    pub rows: Vec<String>,
}

impl Problem {
    pub fn to_json(&self) -> Value {
        json!({"rule": self.rule, "message": self.message, "rows": self.rows})
    }
}

fn s(o: &Map<String, Value>, k: &str) -> Option<String> {
    o.get(k).and_then(Value::as_str).filter(|v| !v.trim().is_empty()).map(String::from)
}

fn word(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// `M<n>`: a milestone of plan section 6.
fn is_milestone(m: &str) -> bool {
    m.strip_prefix('M').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// `agent:<method>`, `state:<key>` or `t3-local`.
fn is_target(t: &str) -> bool {
    t == "t3-local" || t.strip_prefix("agent:").or_else(|| t.strip_prefix("state:")).is_some_and(word)
}

pub struct Coverage {
    pub json: Value,
    pub markdown: String,
    pub problems: Vec<Problem>,
    pub pending: usize,
}

/// Commands and project mutations are T3 rows that need a target.
fn needs_target(app: &str, row: &Row) -> bool {
    app == "t3" && (row.kind == "command" || row.kind == "mutation")
}

pub fn build(list: &MethodsList, decisions: &Decisions, answered: &BTreeMap<String, String>, evidence: &BTreeMap<String, Entry>, oracle: &dyn Evidence) -> Coverage {
    let app = list.app.as_str();
    let (mut out, mut problems) = (vec![], vec![]);
    let (mut unassigned, mut untargeted, mut incomplete, mut unproven, mut unreached) = (vec![], vec![], vec![], vec![], vec![]);
    for row in list.phone_rows() {
        let mut o = Map::new();
        o.insert("id".into(), row.id.clone().into());
        o.insert("kind".into(), row.kind.clone().into());
        o.insert("name".into(), row.name.clone().into());
        if row.subscription {
            o.insert("subscription".into(), true.into());
        }
        let decision = decisions.rows.get(&row.id);
        let (state, reason, decided) = match (row.class.as_deref(), decision) {
            (Some("flagged"), _) => ("flagged", Some(format!("behind {}: outside the parity target until the flag ships", row.extra.get("flag").and_then(Value::as_str).unwrap_or("a flag"))), None),
            (Some("offAllowlist"), _) => ("excluded", Some("not on the paired-phone allowlist (MOBILE_RPC_METHOD_ROUTES): the host refuses it to a phone".into()), None),
            (_, Some(d)) => match s(d, "state").as_deref() {
                Some("not-offered") => ("not-offered", s(d, "reason"), s(d, "decidedBy").filter(|m| is_milestone(m))),
                Some("excluded") => ("excluded", s(d, "reason"), s(d, "decidedBy").filter(|m| is_milestone(m))),
                Some("served") => {
                    if !evidence.contains_key(&row.id) {
                        unproven.push(row.id.clone());
                    }
                    ("pending", None, None)
                }
                _ => ("pending", None, s(d, "decidedBy").filter(|m| is_milestone(m))),
            },
            _ => ("pending", None, None),
        };
        // a pending row the evidence names is served, unless a reverify note holds it
        let held = decisions.reverify.get(&row.id);
        let proof = evidence.get(&row.id).filter(|e| state == "pending" && held.is_none() && (!decisions.mimic(&row.id) || (e.oracle.as_deref() == Some("mimic") && oracle.admits(e.cassette.as_deref(), e.diff.as_deref()))));
        if decision.and_then(|d| d.get("oracle")).is_some_and(|o| o.as_str() != Some("mimic")) {
            incomplete.push(row.id.clone());
        }
        let state = if proof.is_some() { "served" } else { state };
        o.insert("state".into(), state.into());
        let milestone = decisions.milestone(&row.id).filter(|m| is_milestone(m)).map(String::from);
        if let Some(m) = &milestone {
            o.insert("milestone".into(), m.clone().into());
        }
        if needs_target(app, row) {
            match decisions.targets.get(&row.id).filter(|t| is_target(t)) {
                Some(t) => {
                    o.insert("target".into(), t.clone().into());
                }
                None => untargeted.push(row.id.clone()),
            }
        }
        if let Some(r) = &reason {
            o.insert("reason".into(), r.clone().into());
        }
        if let Some(d) = &decided {
            o.insert("decidedBy".into(), d.clone().into());
        }
        if let Some(source) = answered.get(&row.id).filter(|_| state == "pending") {
            o.insert("note".into(), format!("answered by the mapper ({source}); no run-bundle evidence yet").into());
        }
        if let Some(entry) = proof {
            o.insert("evidence".into(), entry.to_json());
        }
        if let Some(why) = held.filter(|_| state == "pending") {
            o.insert("reverify".into(), why.clone().into());
        }
        match state {
            "not-offered" if reason.is_none() || decided.is_none() => incomplete.push(row.id.clone()),
            "excluded" if reason.is_none() => incomplete.push(row.id.clone()),
            "pending" | "not-offered" if milestone.is_none() && decided.is_none() => unassigned.push(row.id.clone()),
            _ => {}
        }
        out.push(Value::Object(o));
    }
    let ids: Vec<&str> = list.phone_rows().map(|r| r.id.as_str()).collect();
    let stale: Vec<String> = decisions.targets.keys().chain(decisions.rows.keys()).chain(decisions.reverify.keys()).chain(decisions.milestones.iter().map(|(p, _)| p).filter(|p| !p.ends_with('*'))).filter(|k| !ids.contains(&k.as_str())).cloned().collect();
    // evidence for a row the table does not serve (gone from the phone, decided not offered, or held) is evidence of nothing
    for id in evidence.keys() {
        let state = out.iter().find(|r| r["id"] == id.as_str()).and_then(|r| r["state"].as_str());
        if state != Some("served") {
            unreached.push(id.clone());
        }
    }
    for (rule, rows, message) in [
        ("unassigned", unassigned, "rows with neither a milestone nor a decidedBy (M<n>) in the decisions file"),
        ("untargeted", untargeted, "command rows without a target of the form agent:<method>, state:<key> or t3-local"),
        ("incomplete-decision", incomplete, "not-offered rows need a reason and a decidedBy, excluded rows a reason, and an oracle can only be mimic"),
        ("served-without-evidence", unproven, "a row is served only with run-bundle evidence (a passing test and a method-hit trace) in <app>-evidence.json: refresh it with --traces"),
        ("stale-evidence", unreached, "the evidence names rows the table does not serve: refresh it with --traces, or remove the not-offered decision or the reverify note"),
        ("stale-decision", stale, "decisions name rows the phone no longer reaches"),
    ] {
        if !rows.is_empty() {
            problems.push(Problem { rule, message: message.into(), rows });
        }
    }
    let count = |state: &str| out.iter().filter(|r| r["state"] == state).count();
    let pending = count("pending");
    // M3 before M10: by length, then by text.
    let mut by_milestone: BTreeMap<(usize, String), usize> = BTreeMap::new();
    for r in out.iter().filter(|r| r["state"] == "pending") {
        let m = r["milestone"].as_str().unwrap_or("none").to_string();
        *by_milestone.entry((m.len(), m)).or_default() += 1;
    }
    let by_milestone: Map<String, Value> = by_milestone.into_iter().map(|((_, m), n)| (m, n.into())).collect();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for r in &out {
        *by_kind.entry(r["kind"].as_str().unwrap_or("").to_string()).or_default() += 1;
    }
    let summary = json!({
        "rows": out.len(),
        "served": count("served"),
        "pending": pending,
        "notOffered": count("not-offered"),
        "excluded": count("excluded"),
        "flagged": count("flagged"),
        "answeredByMapper": out.iter().filter(|r| r.get("note").is_some()).count(),
        "byKind": by_kind,
        "pendingByMilestone": by_milestone,
    });
    let json = json!({
        "schema": 1,
        "app": app,
        "generatedBy": format!("wf parity coverage --app {app}"),
        "pin": list.pin,
        "summary": summary,
        "rows": out,
    });
    let markdown = markdown(app, &json);
    Coverage { json, markdown, problems, pending }
}

fn markdown(app: &str, doc: &Value) -> String {
    let title = if app == "t3" { "T3 Code" } else { "Orca" };
    let s = &doc["summary"];
    let mut md = format!(
        "# {title} parity coverage\n\nGenerated by `wf parity coverage --app {app}` from `{app}-methods.json` (the reference scan at {} {}), `{app}-decisions.json` and the mapper's own tables. Do not edit by hand.\n\n",
        doc["pin"]["version"].as_str().unwrap_or("?"),
        doc["pin"]["commit"].as_str().map_or("?", |c| &c[..c.len().min(8)])
    );
    md.push_str(&format!(
        "{} rows: {} served (a passing test of a chain run and its method-hit trace, `{app}-evidence.json`), {} pending, {} not offered, {} excluded, {} flagged.",
        s["rows"], s["served"], s["pending"], s["notOffered"], s["excluded"], s["flagged"]
    ));
    if s["pending"].as_u64().unwrap_or(0) > 0 {
        md.push_str(&format!(" {} of the pending rows are answered by the mapper today, with no run-bundle evidence yet.\n\n| Milestone | Pending |\n| --- | --- |\n", s["answeredByMapper"]));
        for (m, n) in s["pendingByMilestone"].as_object().into_iter().flatten() {
            md.push_str(&format!("| {m} | {n} |\n"));
        }
    } else {
        md.push('\n');
    }
    md.push_str("\n| Row | State | Milestone | Target | Note |\n| --- | --- | --- | --- | --- |\n");
    let cell = |v: &Value| v.as_str().unwrap_or("").replace('|', "\\|");
    for r in doc["rows"].as_array().into_iter().flatten() {
        let sub = if r["subscription"] == true { " (subscription)" } else { "" };
        let proof = r["evidence"]["test"].as_str().map(|t| format!("evidence: `{}` `{t}`{}", r["evidence"]["suite"].as_str().unwrap_or(""), mimic_note(&r["evidence"]))).unwrap_or_default();
        let held = r["reverify"].as_str().map(|w| format!("reverify: {}", w.replace('|', "\\|"))).unwrap_or_default();
        let note = [cell(&r["reason"]), cell(&r["note"]), proof, held, r["decidedBy"].as_str().map(|d| format!("decided by {d}")).unwrap_or_default()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("; ");
        md.push_str(&format!("| `{}`{sub} | {} | {} | {} | {note} |\n", cell(&r["id"]), cell(&r["state"]), cell(&r["milestone"]), cell(&r["target"])));
    }
    md
}

/// ` (mimic replay: <diff> on a <cassette> cassette)` for a row an oracle replay served.
fn mimic_note(evidence: &Value) -> String {
    match (evidence["oracle"].as_str(), evidence["diff"].as_str(), evidence["cassette"].as_str()) {
        (Some(oracle), Some(diff), Some(cassette)) => format!(" ({oracle} replay: {diff} diff on a {cassette} cassette)"),
        _ => String::new(),
    }
}

pub fn json_text(c: &Coverage) -> String {
    pretty(&c.json)
}

/// The pending count of a committed coverage table.
pub fn pending_of(text: &str) -> Option<usize> {
    serde_json::from_str::<Value>(text).ok()?["summary"]["pending"].as_u64().map(|n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::Row;

    fn list(rows: Vec<Row>) -> MethodsList {
        MethodsList { app: "t3".into(), pin: json!({"version": "0.1.6", "commit": "2359643c"}), counts: json!({}), rows, flags: json!({}) }
    }

    fn phone(kind: &str, name: &str) -> Row {
        let mut r = Row::new(kind, name);
        r.reached("a.ts:1".into());
        r
    }

    #[test]
    fn a_trace_row_is_noise_only_when_a_trace_noise_entry_names_it_exactly_or_by_prefix() {
        // Given an exact entry and a prefix entry
        let decisions = Decisions::parse(r#"{"traceNoise": {"rpc:sidebarSession.begin": "desktop only", "rpc:layout.*": "desktop only", "rpc:empty": " "}}"#).unwrap();
        // When rows are asked
        // Then the named ones are noise, a near miss and an entry without a reason are not
        assert!(decisions.is_trace_noise("rpc:sidebarSession.begin") && decisions.is_trace_noise("rpc:layout.apply"));
        assert!(!decisions.is_trace_noise("rpc:sidebarSession.beginx") && !decisions.is_trace_noise("rpc:layouts") && !decisions.is_trace_noise("rpc:empty"));
    }

    #[test]
    fn rows_take_the_longest_milestone_pattern_and_answered_rows_stay_pending_with_a_note() {
        // Given two terminal rows, a pattern for the group and an exact entry for one
        let decisions = Decisions::parse(r#"{"milestones": {"rpc:terminal.*": "M8", "rpc:terminal.open": "M3"}}"#).unwrap();
        let answered = BTreeMap::from([("rpc:terminal.open".to_string(), "rpc.rs IMPLEMENTED_RPCS".to_string())]);
        // When the table is built
        let c = build(&list(vec![phone("rpc", "terminal.open"), phone("rpc", "terminal.write"), Row::new("rpc", "server.not-phone")]), &decisions, &answered, &BTreeMap::new(), &crate::testing::AdmitOnly::default());
        // Then only phone rows are there, the exact entry wins, and the answered row is pending with its source named
        let rows = c.json["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0]["milestone"].as_str(), rows[1]["milestone"].as_str()), (Some("M3"), Some("M8")));
        assert_eq!(rows[0]["state"], "pending");
        assert!(rows[0]["note"].as_str().unwrap().contains("rpc.rs IMPLEMENTED_RPCS") && rows[0]["note"].as_str().unwrap().contains("no run-bundle evidence"));
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!(c.pending, 2);
    }

    #[test]
    fn unassigned_untargeted_incomplete_unproven_and_stale_rows_are_each_named() {
        // Given rows the decisions leave open in each way
        let decisions = Decisions::parse(
            r#"{"milestones": {"command:thread.archive": "M4"},
                "targets": {"command:gone": "agent:x"},
                "decisions": {"rpc:a.b": {"state": "not-offered", "reason": "no"}, "rpc:c.d": {"state": "served"}}}"#,
        )
        .unwrap();
        let c = build(&list(vec![phone("command", "thread.archive"), phone("rpc", "a.b"), phone("rpc", "c.d"), phone("rpc", "e.f")]), &decisions, &BTreeMap::new(), &BTreeMap::new(), &crate::testing::AdmitOnly::default());
        let named = |rule: &str| c.problems.iter().find(|p| p.rule == rule).map(|p| p.rows.clone()).unwrap_or_default();
        // Then each problem names exactly its rows
        assert_eq!(named("untargeted"), ["command:thread.archive"]);
        assert_eq!(named("incomplete-decision"), ["rpc:a.b"]);
        assert_eq!(named("served-without-evidence"), ["rpc:c.d"]);
        assert_eq!(named("unassigned"), ["rpc:c.d", "rpc:e.f"]);
        assert_eq!(named("stale-decision"), ["command:gone"]);
        // And no row became served
        assert!(c.json["rows"].as_array().unwrap().iter().all(|r| r["state"] != "served"));
    }

    #[test]
    fn an_empty_or_malformed_milestone_leaves_a_row_unassigned_and_a_malformed_target_leaves_it_untargeted() {
        // Given a command and an RPC row decided only by values not of the milestone or target form, and a well-formed pair
        let decisions = Decisions::parse(
            r#"{"milestones": {"rpc:*": "", "command:thread.archive": "soon", "command:thread.delete": "M4", "rpc:a.b": "M3"},
                "targets": {"command:thread.archive": "bogus", "command:thread.delete": "agent:threads.delete"}}"#,
        )
        .unwrap();
        // When the table is built
        let rows = vec![phone("command", "thread.archive"), phone("rpc", "e.f"), phone("command", "thread.delete"), phone("rpc", "a.b")];
        let c = build(&list(rows), &decisions, &BTreeMap::new(), &BTreeMap::new(), &crate::testing::AdmitOnly::default());
        let named = |rule: &str| c.problems.iter().find(|p| p.rule == rule).map(|p| p.rows.clone()).unwrap_or_default();
        // Then the malformed rows are named and carry neither value into the table, and the well-formed rows pass
        assert_eq!(named("unassigned"), ["command:thread.archive", "rpc:e.f"]);
        assert_eq!(named("untargeted"), ["command:thread.archive"]);
        let table = c.json["rows"].as_array().unwrap();
        assert!(table.iter().all(|r| r["milestone"] != "" && r["milestone"] != "soon" && r["target"] != "bogus"), "{table:?}");
        assert_eq!((table[2]["milestone"].as_str(), table[2]["target"].as_str()), (Some("M4"), Some("agent:threads.delete")));
    }

    #[test]
    fn flagged_and_off_allowlist_rows_are_outside_the_target_without_a_milestone() {
        let mut flagged = phone("rpc", "ui.setCollapsed");
        flagged.class = Some("flagged".into());
        flagged.extra.insert("flag".into(), "ORCA_SIDEBAR_INPUTS_IS_MAIN".into());
        let mut off = phone("rpc", "plugins.panelAction");
        off.class = Some("offAllowlist".into());
        let mut l = list(vec![flagged, off]);
        l.app = "orca".into();
        let c = build(&l, &Decisions::default(), &BTreeMap::new(), &BTreeMap::new(), &crate::testing::AdmitOnly::default());
        assert!(c.problems.is_empty(), "{:?}", c.problems);
        assert_eq!((c.json["summary"]["flagged"].as_u64(), c.json["summary"]["excluded"].as_u64(), c.pending), (Some(1), Some(1), 0));
        assert!(c.markdown.contains("ORCA_SIDEBAR_INPUTS_IS_MAIN"));
    }

    fn entry(test: &str) -> Entry {
        Entry { suite: "chain".into(), test: test.into(), agent: vec!["turn.start".into()], executor: vec![], oracle: None, diff: None, cassette: None }
    }

    #[test]
    fn a_pending_row_the_evidence_names_is_served_with_its_test_and_a_held_row_is_not() {
        // Given three rows, evidence for each, and a reverify note on one of them
        let decisions = Decisions::parse(r#"{"milestones": {"rpc:*": "M3"}, "reverify": {"rpc:held": "the answer is a canned one"}}"#).unwrap();
        let evidence = BTreeMap::from([("rpc:a".to_string(), entry("chain::a")), ("rpc:b".to_string(), entry("chain::b")), ("rpc:held".to_string(), entry("chain::held"))]);
        // When the table is built
        let c = build(&list(vec![phone("rpc", "a"), phone("rpc", "b"), phone("rpc", "held"), phone("rpc", "none")]), &decisions, &BTreeMap::new(), &evidence, &crate::testing::AdmitOnly::default());
        // Then the rows with evidence are served and name their test, the held one stays pending with its note, and pending counts the rest
        let rows = c.json["rows"].as_array().unwrap();
        let state = |id: &str| rows.iter().find(|r| r["id"] == id).map(|r| r["state"].as_str().unwrap().to_string()).unwrap();
        assert_eq!((state("rpc:a"), state("rpc:b"), state("rpc:held"), state("rpc:none")), ("served".into(), "served".into(), "pending".into(), "pending".into()));
        assert_eq!(rows[0]["evidence"]["test"], "chain::a");
        assert_eq!(rows[2]["reverify"], "the answer is a canned one");
        assert_eq!((c.pending, c.json["summary"]["served"].as_u64()), (2, Some(2)));
        assert!(c.markdown.contains("evidence: `chain` `chain::a`") && c.markdown.contains("reverify: the answer is a canned one"));
        // And the evidence of the held row is evidence of nothing, which the check names
        let rule = |name: &str| c.problems.iter().find(|p| p.rule == name).map(|p| p.rows.clone()).unwrap_or_default();
        assert_eq!(rule("stale-evidence"), ["rpc:held"]);
        assert!(rule("served-without-evidence").is_empty());
    }

    #[test]
    fn a_decision_that_claims_served_needs_evidence_and_evidence_for_a_decided_not_offered_row_is_stale() {
        // Given one row decided served with evidence and one without, and evidence for a row decided not offered and for one the phone does not reach
        let decisions = Decisions::parse(
            r#"{"milestones": {"rpc:*": "M3"}, "decisions": {"rpc:proved": {"state": "served"}, "rpc:claimed": {"state": "served"}, "rpc:no": {"state": "not-offered", "reason": "r", "decidedBy": "M5"}}}"#,
        )
        .unwrap();
        let evidence = BTreeMap::from([("rpc:proved".to_string(), entry("chain::p")), ("rpc:no".to_string(), entry("chain::n")), ("rpc:gone".to_string(), entry("chain::g"))]);
        let c = build(&list(vec![phone("rpc", "proved"), phone("rpc", "claimed"), phone("rpc", "no")]), &decisions, &BTreeMap::new(), &evidence, &crate::testing::AdmitOnly::default());
        let rule = |name: &str| c.problems.iter().find(|p| p.rule == name).map(|p| p.rows.clone()).unwrap_or_default();
        // Then only the claim without evidence is unproven, and the other two evidence entries are stale
        assert_eq!(rule("served-without-evidence"), ["rpc:claimed"]);
        assert_eq!(rule("stale-evidence"), ["rpc:gone", "rpc:no"]);
    }

    #[test]
    fn a_row_decided_oracle_mimic_is_served_only_by_oracle_evidence_and_an_unknown_oracle_is_incomplete() {
        // Given three rows decided oracle: mimic, one with chain evidence, one with mimic evidence, one with neither, and a row with another oracle
        let decisions = Decisions::parse(r#"{"milestones": {"rpc:*": "M3"}, "decisions": {"rpc:chain": {"oracle": "mimic"}, "rpc:replayed": {"oracle": "mimic"}, "rpc:none": {"oracle": "mimic"}, "rpc:other": {"oracle": "manual"}, "rpc:drift": {"oracle": "mimic"}, "rpc:authored": {"oracle": "mimic"}}}"#).unwrap();
        let replayed = Entry { oracle: Some("mimic".into()), diff: Some("clean".into()), cassette: Some("recorded".into()), ..entry("replay::set") };
        let stored = |diff: &str, cassette: &str| Entry { oracle: Some("mimic".into()), diff: Some(diff.into()), cassette: Some(cassette.into()), ..entry("replay::set") };
        let evidence = BTreeMap::from([("rpc:chain".to_string(), entry("chain::a")), ("rpc:replayed".to_string(), replayed), ("rpc:drift".to_string(), stored("drift", "recorded")), ("rpc:authored".to_string(), stored("clean", "authored"))]);
        // When the table is built
        let c = build(&list(vec![phone("rpc", "chain"), phone("rpc", "replayed"), phone("rpc", "none"), phone("rpc", "other"), phone("rpc", "drift"), phone("rpc", "authored")]), &decisions, &BTreeMap::new(), &evidence, &crate::testing::AdmitOnly::of(&[("recorded", "clean")]));
        // Then only the oracle's own evidence serves a row and names the diff, a chain entry for it is stale, and the unknown oracle is incomplete
        let state = |id: &str| c.json["rows"].as_array().unwrap().iter().find(|r| r["id"] == id).map(|r| r["state"].as_str().unwrap().to_string()).unwrap();
        assert_eq!((state("rpc:chain"), state("rpc:replayed"), state("rpc:none")), ("pending".into(), "served".into(), "pending".into()));
        // and a checked-in entry whose diff or cassette the oracle does not admit serves nothing, whatever the file says
        assert_eq!((state("rpc:drift"), state("rpc:authored")), ("pending".into(), "pending".into()));
        assert!(c.markdown.contains("(mimic replay: clean diff on a recorded cassette)"));
        let rule = |name: &str| c.problems.iter().find(|p| p.rule == name).map(|p| p.rows.clone()).unwrap_or_default();
        assert_eq!((rule("stale-evidence"), rule("incomplete-decision")), (vec!["rpc:authored".to_string(), "rpc:chain".to_string(), "rpc:drift".to_string()], vec!["rpc:other".to_string()]));
    }
}
