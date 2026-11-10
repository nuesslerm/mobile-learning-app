//! Flow tests of mimic-core: the use cases end to end over the in-memory doubles, and the T3 goldens as the read-only oracle of the
//! normalisation rules. Given/When/Then; case lists and placeholders are derived from the data under test.
use mimic_core::allowlist::{plain, Allowlist, Entry};
use mimic_core::cassette::{Cassette, CassetteError, CheckStatus, Current, Kind, Target, ToolInfo};
use mimic_core::corpus::{corpus_hash, expand, CorpusSpec, Gen, FieldSpec, GeneratorSpec, Mutator, SCHEMA as CORPUS_SCHEMA};
use mimic_core::differ::{DiffKind, Label, Severity};
use mimic_core::ports::{CassetteSet, Driver, DriverError, Hello, Observed, Request, Store};
use mimic_core::presets;
use mimic_core::rules::{Rule, RuleSet};
use mimic_core::testing::{driver_info, MemStore, ToyDriver, Tweak};
use mimic_core::usecase::{evidence, record, replay, verify, Meta};
use serde_json::{json, Value};

mod t3_oracle {
    use super::*;
    use regex::Regex;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    const UNMASKED_TS: &str = "2026-03-04T05:06:07.089Z";
    const UNMASKED_SIGNED: &str = "abcdefghijklmnopqrstuvwx.0123456789abcdef0";

    fn goldens() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tooling/t3-conformance/goldens")
    }

    // the one pinned version directory; the pin file names it
    fn golden_dir() -> PathBuf {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(goldens()).expect("goldens dir").map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        dirs.pop().expect("a golden version directory")
    }

    fn secret_keys(rules: &RuleSet) -> BTreeMap<String, String> {
        rules.rules.iter().filter_map(|r| if let Rule::SecretKey { keys } = r { Some(keys.clone()) } else { None }).flatten().collect()
    }

    fn number_keys(rules: &RuleSet) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for r in &rules.rules {
            match r {
                Rule::Sequence { keys, placeholder } => out.extend(keys.iter().map(|k| (k.clone(), placeholder.clone()))),
                Rule::VolatileNumber { keys } => out.extend(keys.clone()),
                _ => {}
            }
        }
        out
    }

    fn string_keys(rules: &RuleSet) -> BTreeMap<String, String> {
        rules.rules.iter().filter_map(|r| if let Rule::VolatileString { keys } = r { Some(keys.clone()) } else { None }).flatten().collect()
    }

    fn dropped(rules: &RuleSet) -> Vec<String> {
        rules.rules.iter().filter_map(|r| if let Rule::DropHeader { names } = r { Some(names.clone()) } else { None }).flatten().collect()
    }

    fn raw_secret(placeholder: &str) -> String {
        format!("raw-{}-0123456789", placeholder.trim_matches(|c| c == '<' || c == '>'))
    }

    fn fake_uuid(n: usize) -> String {
        format!("{:08x}-1111-4222-8333-{:012x}", 0xabcd_0000 + n, n)
    }

    /// What a reference would have said before masking: every placeholder of the golden put back as a made-up raw value.
    struct Unmask<'a> {
        rules: &'a RuleSet,
        uuid: Regex,
    }

    impl Unmask<'_> {
        fn text(&self, s: &str) -> String {
            let mut out = self.uuid.replace_all(s, |c: &regex::Captures| fake_uuid(c[1].parse().unwrap())).into_owned();
            for ph in secret_keys(self.rules).values() {
                out = out.replace(ph.as_str(), &raw_secret(ph));
            }
            for r in &self.rules.rules {
                match r {
                    Rule::Timestamp { placeholder, .. } => out = out.replace(placeholder.as_str(), UNMASKED_TS),
                    Rule::Token { placeholder, .. } => out = out.replace(placeholder.as_str(), UNMASKED_SIGNED),
                    _ => {}
                }
            }
            out
        }

        fn json(&self, v: &Value, key: Option<&str>) -> Value {
            match v {
                Value::String(s) => {
                    if let Some(k) = key {
                        if secret_keys(self.rules).get(k) == Some(s) {
                            return json!(raw_secret(s));
                        }
                        if string_keys(self.rules).get(k) == Some(s) {
                            return json!("0123abcd0123abcd");
                        }
                        if number_keys(self.rules).get(k) == Some(s) {
                            return json!(4242);
                        }
                    }
                    json!(self.text(s))
                }
                Value::Array(a) => Value::Array(a.iter().map(|x| self.json(x, key)).collect()),
                Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), self.json(x, Some(k)))).collect()),
                other => other.clone(),
            }
        }
    }

    fn form_encode(v: &Value) -> String {
        let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
        v.as_object().unwrap().iter().map(|(k, v)| format!("{}={}", enc(k), enc(v.as_str().unwrap()))).collect::<Vec<_>>().join("&")
    }

    fn headers_of(v: &Value, un: &Unmask, rules: &RuleSet) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = v.as_object().unwrap().iter().map(|(k, x)| (k.clone(), un.text(x.as_str().unwrap()))).collect();
        // what the machine adds to every request and response: the rules must drop it
        out.extend(dropped(rules).into_iter().map(|n| (n, "machine-specific".to_string())));
        out
    }

    fn raw_body(body: &Value, content_type: &str, un: &Unmask) -> String {
        match body {
            Value::Null => String::new(),
            b if content_type.contains("x-www-form-urlencoded") => form_encode(&un.json(b, None)),
            b if content_type.contains("json") => un.json(b, None).to_string(),
            Value::String(s) => un.text(s),
            other => un.json(other, None).to_string(),
        }
    }

    /// The recorder of the TS tool (golden.ts toLine) over the Rust normaliser: raw line in, golden line out.
    fn normalise_line(nz: &mut mimic_core::rules::Normalizer, golden: &Value, un: &Unmask, rules: &RuleSet) -> (Value, Value) {
        let mut raw = golden.clone();
        let mut out = golden.clone();
        let (r, o) = (raw.as_object_mut().unwrap(), out.as_object_mut().unwrap());
        match golden["kind"].as_str().unwrap() {
            "http" => {
                let ctype = |h: &str| golden[h]["content-type"].as_str().unwrap_or("").to_string();
                let (req_ct, res_ct) = (ctype("reqHeaders"), ctype("resHeaders"));
                let req_body = raw_body(&golden["reqBody"], &req_ct, un);
                let res_body = raw_body(&golden["resBody"], &res_ct, un);
                let raw_path = un.text(golden["path"].as_str().unwrap());
                let req_headers = headers_of(&golden["reqHeaders"], un, rules);
                let res_headers = headers_of(&golden["resHeaders"], un, rules);
                r.insert("reqBody".into(), json!(req_body));
                r.insert("resBody".into(), json!(res_body));
                r.insert("path".into(), json!(raw_path));
                r.insert("reqHeaders".into(), json!(req_headers));
                r.insert("resHeaders".into(), json!(res_headers));
                // bodies first: they teach the normaliser the secrets that headers and paths then carry
                o.insert("reqBody".into(), nz.body(&req_body, &req_ct));
                o.insert("resBody".into(), nz.body(&res_body, &res_ct));
                o.insert("path".into(), json!(nz.path(&raw_path)));
                o.insert("reqHeaders".into(), json!(nz.headers(&req_headers)));
                o.insert("resHeaders".into(), json!(nz.headers(&res_headers)));
            }
            "ws-open" => {
                let raw_path = un.text(golden["path"].as_str().unwrap());
                r.insert("path".into(), json!(raw_path));
                o.insert("path".into(), json!(nz.path(&raw_path)));
            }
            "ws" if golden["opcode"] == 1 => {
                let frame = &golden["frame"];
                let raw_frame = un.json(frame, None);
                r.insert("frame".into(), raw_frame.clone());
                let masked = match &raw_frame {
                    Value::String(s) => json!(nz.text(s)),
                    other => nz.json(other, None),
                };
                o.insert("frame".into(), masked);
            }
            _ => {}
        }
        (raw, out)
    }

    struct Recording {
        file: String,
        golden: Vec<Value>,
        raw: Vec<Value>,
        normalised: Vec<Value>,
    }

    fn run_all(rules_under_test: &RuleSet, truth: &RuleSet) -> Vec<Recording> {
        let compiled = rules_under_test.compile().expect("rules compile");
        let un = Unmask { rules: truth, uuid: Regex::new(r"<uuid:([0-9]+)>").unwrap() };
        let mut files: Vec<PathBuf> = std::fs::read_dir(golden_dir()).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "jsonl")).collect();
        files.sort();
        files
            .into_iter()
            // the variant-order file is an alternate recording of rpc-phase1, not a scenario
            .filter(|p| !p.file_name().unwrap().to_string_lossy().contains(".variant-"))
            .map(|path| {
                let text = std::fs::read_to_string(&path).unwrap();
                let golden: Vec<Value> = text.lines().filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect();
                let mut nz = compiled.normalizer();
                // a recording in which every uuid the file names has already appeared, in ordinal order
                let max = Regex::new(r"<uuid:([0-9]+)>").unwrap().captures_iter(&text).map(|c| c[1].parse::<usize>().unwrap()).max().unwrap_or(0);
                nz.text(&(1..=max).map(fake_uuid).collect::<Vec<_>>().join(" "));
                let (mut raw, mut normalised) = (Vec::new(), Vec::new());
                for line in &golden {
                    let (r, n) = normalise_line(&mut nz, line, &un, truth);
                    raw.push(r);
                    normalised.push(n);
                }
                Recording { file: path.file_name().unwrap().to_string_lossy().into_owned(), golden, raw, normalised }
            })
            .collect()
    }

    fn placeholders(rule: &Rule) -> Vec<String> {
        match rule {
            Rule::UuidOrdinal => vec!["<uuid:".into()],
            Rule::Timestamp { placeholder, .. } | Rule::Token { placeholder, .. } | Rule::Sequence { placeholder, .. } => vec![placeholder.clone()],
            Rule::SecretKey { keys } | Rule::VolatileNumber { keys } | Rule::VolatileString { keys } => keys.values().cloned().collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn given_the_t3_goldens_when_their_placeholders_are_put_back_as_raw_values_then_the_rust_rules_mask_every_line_as_the_ts_tool_did() {
        let rules = presets::t3();
        let recordings = run_all(&rules, &rules);
        assert!(!recordings.is_empty(), "no golden recordings found");
        for rec in &recordings {
            for (i, (golden, got)) in rec.golden.iter().zip(&rec.normalised).enumerate() {
                assert_eq!(got, golden, "{} line {i}: the masked raw line differs from the golden", rec.file);
            }
        }
    }

    #[test]
    fn given_the_golden_placeholders_when_unmasked_then_the_raw_lines_carry_none_of_them_so_the_oracle_is_not_vacuous() {
        let rules = presets::t3();
        let recordings = run_all(&rules, &rules);
        let all_golden: String = recordings.iter().flat_map(|r| &r.golden).map(Value::to_string).collect();
        let all_raw: String = recordings.iter().flat_map(|r| &r.raw).map(Value::to_string).collect();
        let present: Vec<String> = rules.rules.iter().flat_map(placeholders).filter(|p| all_golden.contains(p.as_str())).collect();
        assert!(present.len() >= 5, "the goldens exercise too few placeholders: {present:?}");
        for p in present {
            assert!(!all_raw.contains(&p), "placeholder {p} survived unmasking, so it is not tested");
        }
    }

    #[test]
    fn given_each_rule_the_goldens_exercise_when_it_is_removed_then_the_goldens_no_longer_match() {
        let truth = presets::t3();
        let all_golden: String = run_all(&truth, &truth).iter().flat_map(|r| &r.golden).map(Value::to_string).collect();
        for (i, rule) in truth.rules.iter().enumerate() {
            let exercised = placeholders(rule).iter().any(|p| all_golden.contains(p.as_str())) || matches!(rule, Rule::DropHeader { .. });
            if !exercised {
                continue;
            }
            let mut without = truth.clone();
            without.rules.remove(i);
            let broken = run_all(&without, &truth).iter().any(|r| r.normalised != r.golden);
            assert!(broken, "removing rule {rule:?} left the goldens matching: the rule is not load-bearing");
        }
    }
}

mod cassettes {
    use super::*;

    pub fn meta(kind: Kind, pin_hash: Option<&str>) -> Meta {
        Meta {
            kind,
            target: Some(Target { name: "toy".into(), pin_file: Some("toy-pin.json".into()), pin_hash: pin_hash.map(String::from), ..Target::default() }),
            tool: ToolInfo { mimic: "0.1.0".into(), git: None },
            seed: Some(7),
            recorded_at: Some("2026-11-05T00:00:00Z".into()),
            recorded_on: Some("sandbox".into()),
            source: None,
            rules_path: "rules.json".into(),
            corpus_path: Some("corpus.json".into()),
        }
    }

    pub fn toy_rules() -> RuleSet {
        RuleSet::new(vec![
            Rule::UuidOrdinal,
            Rule::Timestamp { keys: vec![], placeholder: "<ts>".into() },
            Rule::Token { pattern: r"\b[A-Za-z0-9]{32}\.[A-Za-z0-9]{32}\b".into(), placeholder: "<signed-token>".into() },
            Rule::Sequence { keys: vec!["sequence".into()], placeholder: "<seq>".into() },
        ])
    }

    pub fn toy_corpus() -> CorpusSpec {
        CorpusSpec {
            schema: CORPUS_SCHEMA.into(),
            seed: 7,
            generators: vec![
                GeneratorSpec::Corpus {
                    name: "hand".into(),
                    op: Some("echo".into()),
                    files: vec!["cases.json".into()],
                    cases: vec![mimic_core::corpus::CaseSpec { id: "echo-ada".into(), row: Some("toy:echo".into()), input: json!({"user": "ada", "n": 1}), tags: vec![] }],
                },
                GeneratorSpec::Seeded {
                    name: "login".into(),
                    op: Some("session".into()),
                    count: 3,
                    template: json!({"user": ""}),
                    fields: vec![FieldSpec { pointer: "/user".into(), gen: Gen::Text { min_len: 4, max_len: 8 } }],
                    mutators: vec![],
                },
            ],
        }
    }

    pub fn store_with_files() -> MemStore {
        let mut store = MemStore::default();
        store.files.insert(("toy".into(), "cases.json".into()), json!([{"id": "echo-grace", "input": {"user": "grace"}, "row": "toy:echo"}]));
        store
    }

    pub fn recorded(driver: &mut dyn Driver) -> CassetteSet {
        let store = store_with_files();
        let corpus = toy_corpus();
        let cases = expand(&corpus, &store, "toy").unwrap();
        let rec = record(driver, &cases, &toy_rules(), Some(&corpus), meta(Kind::Recorded, Some("sha256:pin-1"))).unwrap();
        assert_eq!(rec.nondeterministic, Vec::<String>::new());
        rec.set
    }

    pub fn toy() -> ToyDriver<mimic_core::rng::SplitMix64, mimic_core::testing::FixedClock> {
        ToyDriver::new(99, 1_788_000_000_000)
    }

    #[test]
    fn given_a_toy_that_invents_ids_and_times_when_recorded_then_the_cassette_masks_them_and_is_stable_across_runs() {
        let set = recorded(&mut toy());
        let sessions: Vec<_> = set.cassette.interactions.iter().filter(|i| i.op.as_deref() == Some("session")).collect();
        assert!(!sessions.is_empty());
        for i in sessions {
            let p = &i.steps[0].payload;
            assert_eq!(p["sessionId"], "<uuid:1>", "interaction {}", i.id);
            assert_eq!(p["startedAt"], "<ts>");
            assert_eq!(p["token"], "<signed-token>");
            assert_eq!(p["sequence"], "<seq>");
        }
        let again = recorded(&mut ToyDriver::new(12345, 1_700_000_000_000));
        assert_eq!(set.cassette.lines(), again.cassette.lines(), "a different seed and clock must give the same masked cassette");
    }

    #[test]
    fn given_a_recorded_set_when_replayed_against_the_same_toy_then_every_interaction_is_clean_and_the_input_reached_the_driver() {
        let set = recorded(&mut toy());
        let verdict = replay(&set, &mut ToyDriver::new(5, 1_600_000_000_000), &Allowlist::default(), &Default::default(), None).unwrap();
        assert!(verdict.ok(), "{verdict:?}");
        assert_eq!(verdict.summary.clean, set.cassette.interactions.len());
    }

    #[test]
    fn given_a_planted_change_when_replayed_then_a_dropped_key_is_breaking_an_extra_key_drift_and_each_names_its_path() {
        let set = recorded(&mut toy());
        let mut drop = toy();
        drop.tweak = Tweak::DropKey;
        let broken = replay(&set, &mut drop, &Allowlist::default(), &Default::default(), None).unwrap();
        assert_eq!(broken.severity, Severity::Breaking);
        let d = broken.results.iter().flat_map(|r| &r.diffs).find(|d| d.kind == DiffKind::Missing).expect("a missing-key diff");
        assert!(d.path.starts_with("steps.") && d.path.contains("payload."), "path {}", d.path);
        assert!(d.expected.is_some() && d.actual.is_none());

        let mut extra = toy();
        extra.tweak = Tweak::ExtraKey;
        let drift = replay(&set, &mut extra, &Allowlist::default(), &Default::default(), None).unwrap();
        assert_eq!(drift.severity, Severity::Drift);
        assert!(drift.results.iter().flat_map(|r| &r.diffs).all(|d| d.kind == DiffKind::Extra && d.path.ends_with(".extra")));
    }

    #[test]
    fn given_a_drift_when_its_path_is_allowlisted_then_replay_is_clean_with_the_reason_reported_and_without_the_entry_it_is_back() {
        let set = recorded(&mut toy());
        let mut changed = toy();
        changed.tweak = Tweak::ChangeValue;
        let opts = Default::default();
        let bare = replay(&set, &mut changed, &Allowlist::default(), &opts, None).unwrap();
        assert_eq!(bare.severity, Severity::Drift);
        let mut paths: Vec<String> = bare.results.iter().flat_map(|r| &r.diffs).map(|d| plain(&d.path)).collect();
        paths.sort();
        paths.dedup();
        let entries = paths.into_iter().map(|path| Entry { path, reason: "the toy renamed a value on purpose".into(), phase: "1".into(), pin_hash: None });
        let allow = Allowlist { ignored: entries.collect(), ..Allowlist::default() };

        let mut again = toy();
        again.tweak = Tweak::ChangeValue;
        let allowed = replay(&set, &mut again, &allow, &opts, None).unwrap();
        assert!(allowed.ok(), "{allowed:?}");
        assert!(allowed.summary.allowlisted > 0 && allowed.summary.drift == 0);
        let shown = allowed.results.iter().flat_map(|r| &r.diffs).find_map(|d| d.allowlisted.as_ref()).expect("the allowlisted diff is still reported");
        assert_eq!(shown.reason, "the toy renamed a value on purpose");
        assert!(allowed.unused_allowlist.is_empty());

        let mut third = toy();
        third.tweak = Tweak::ChangeValue;
        let without = replay(&set, &mut third, &Allowlist::default(), &opts, None).unwrap();
        assert_eq!(without.severity, Severity::Drift, "removing the entry brings the divergence back");
    }

    #[test]
    fn given_an_allowlist_entry_that_matches_nothing_when_replayed_then_it_is_reported_unused() {
        let set = recorded(&mut toy());
        let allow = Allowlist { omitted: vec![Entry { path: "no.such.key".into(), reason: "stale".into(), phase: "1".into(), pin_hash: None }], ..Allowlist::default() };
        let v = replay(&set, &mut toy(), &allow, &Default::default(), None).unwrap();
        assert_eq!(v.unused_allowlist, vec!["omitted:no.such.key".to_string()]);
    }

    #[test]
    fn given_a_driver_whose_output_differs_between_two_runs_when_recorded_then_the_unstable_ids_are_named() {
        struct Flaky(u32);
        impl Driver for Flaky {
            fn hello(&mut self) -> Result<Hello, DriverError> {
                Ok(Hello { driver: driver_info("flaky"), target_version: Some("1".into()), coverage: false })
            }
            fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
                self.0 += 1;
                let mut o = ToyDriver::new(1, 0).run(request)?;
                if request.id == "echo-ada" {
                    o.steps[0].payload = json!({"counter": self.0});
                }
                Ok(o)
            }
        }
        let store = store_with_files();
        let corpus = toy_corpus();
        let cases = expand(&corpus, &store, "toy").unwrap();
        let rec = record(&mut Flaky(0), &cases, &toy_rules(), Some(&corpus), meta(Kind::Recorded, Some("sha256:pin-1"))).unwrap();
        assert_eq!(rec.nondeterministic, vec!["echo-ada".to_string()]);
    }

    #[test]
    fn given_a_driver_that_fails_on_one_interaction_when_replayed_then_that_interaction_is_breaking_with_the_error_and_the_rest_still_run() {
        let set = recorded(&mut toy());
        struct Down;
        impl Driver for Down {
            fn hello(&mut self) -> Result<Hello, DriverError> {
                Ok(Hello { driver: driver_info("down"), target_version: None, coverage: false })
            }
            fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
                if request.id == "echo-ada" {
                    return Err(DriverError("connection refused".into()));
                }
                ToyDriver::new(3, 0).run(request)
            }
        }
        let v = replay(&set, &mut Down, &Allowlist::default(), &Default::default(), None).unwrap();
        let failed: Vec<_> = v.results.iter().filter(|r| r.error.is_some()).collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].id, "echo-ada");
        assert!(failed[0].error.as_ref().unwrap().contains("connection refused"));
        assert_eq!(v.summary.clean, set.cassette.interactions.len() - 1);
    }

    #[test]
    fn given_only_ids_when_replayed_then_just_those_interactions_run() {
        let set = recorded(&mut toy());
        let pick = vec![set.cassette.interactions[1].id.clone()];
        let mut driver = mimic_core::testing::ScriptedDriver::default();
        driver.replies.insert(pick[0].clone(), Ok(Observed { steps: set.cassette.interactions[1].steps.clone(), effects: set.cassette.interactions[1].effects.clone(), outcome: set.cassette.interactions[1].outcome.clone() }));
        let v = replay(&set, &mut driver, &Allowlist::default(), &Default::default(), Some(&pick)).unwrap();
        assert_eq!(v.summary.total, 1);
        assert_eq!(driver.asked.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), pick);
    }

    #[test]
    fn given_a_cassette_when_written_and_parsed_then_it_round_trips_and_a_hand_edit_is_refused() {
        let set = recorded(&mut toy());
        let c = &set.cassette;
        let parsed = Cassette::parse(&c.header_json(), &c.jsonl()).unwrap();
        assert_eq!(&parsed, c);
        let first = c.interactions[0].id.clone();
        let edited = c.jsonl().replacen(&first, "renamed", 1);
        assert!(matches!(Cassette::parse(&c.header_json(), &edited), Err(CassetteError::ContentHash { .. })));
        let fewer: String = c.jsonl().lines().skip(1).map(|l| format!("{l}\n")).collect();
        assert!(matches!(Cassette::parse(&c.header_json(), &fewer), Err(CassetteError::Count { .. })));
        let other_schema = c.header_json().replace("mimic-cassette/1", "mimic-cassette/9");
        assert!(matches!(Cassette::parse(&other_schema, &c.jsonl()), Err(CassetteError::Schema(_))));
    }

    #[test]
    fn given_two_recordings_that_differ_only_in_when_they_were_made_then_their_content_hashes_are_equal() {
        let a = recorded(&mut toy());
        let mut b = a.clone();
        b.cassette.header.provenance.recorded_at = Some("2030-01-01T00:00:00Z".into());
        assert_eq!(a.cassette.content_hash(), b.cassette.content_hash());
        b.cassette.header.provenance.seed = Some(8);
        assert_ne!(a.cassette.content_hash(), b.cassette.content_hash());
    }

    #[test]
    fn given_the_pin_moves_when_verified_then_the_pin_hash_is_stale_and_the_other_checks_stay_ok() {
        let set = recorded(&mut toy());
        let rules_hash = set.rules.hash();
        let corpus_hash = set.cassette.header.corpus.as_ref().map(|c| c.hash.clone());
        let fresh = Current { pin_hash: Some("sha256:pin-1".into()), rules_hash: rules_hash.clone(), corpus_hash: corpus_hash.clone() };
        assert!(verify(&set, &fresh).ok, "{:?}", verify(&set, &fresh));
        let moved = Current { pin_hash: Some("sha256:pin-2".into()), ..fresh };
        let report = verify(&set, &moved);
        assert!(!report.ok);
        let stale: Vec<_> = report.checks.iter().filter(|c| c.status == CheckStatus::Stale).map(|c| c.name.as_str()).collect();
        assert_eq!(stale, vec!["pin_hash"]);
    }

    #[test]
    fn given_a_corpus_that_changed_when_verified_then_the_corpus_hash_is_stale() {
        let set = recorded(&mut toy());
        let mut spec = toy_corpus();
        spec.seed += 1;
        let cases = expand(&spec, &store_with_files(), "toy").unwrap();
        let now = Current { pin_hash: Some("sha256:pin-1".into()), rules_hash: set.rules.hash(), corpus_hash: Some(corpus_hash(&cases)) };
        let report = verify(&set, &now);
        assert!(report.checks.iter().any(|c| c.name == "corpus_hash" && c.status == CheckStatus::Stale));
    }

    #[test]
    fn given_provenance_claims_that_do_not_fit_their_kind_when_verified_then_each_is_a_problem() {
        let store = store_with_files();
        let corpus = toy_corpus();
        let cases = expand(&corpus, &store, "toy").unwrap();
        let cases_of = |kind, pin: Option<&str>| record(&mut toy(), &cases, &toy_rules(), Some(&corpus), meta(kind, pin)).unwrap().set;
        let now = |set: &CassetteSet| Current { pin_hash: set.cassette.header.provenance.target.as_ref().and_then(|t| t.pin_hash.clone()), rules_hash: set.rules.hash(), corpus_hash: set.cassette.header.corpus.as_ref().map(|c| c.hash.clone()) };

        let unpinned = cases_of(Kind::Recorded, None);
        let fields: Vec<_> = verify(&unpinned, &now(&unpinned)).problems.into_iter().map(|p| p.field).collect();
        assert!(fields.contains(&"provenance.target.pin_hash".to_string()), "{fields:?}");

        let authored_with_pin = cases_of(Kind::Authored, Some("sha256:x"));
        assert!(!verify(&authored_with_pin, &now(&authored_with_pin)).ok);

        let imported = cases_of(Kind::Imported, None);
        let fields: Vec<_> = verify(&imported, &now(&imported)).problems.into_iter().map(|p| p.field).collect();
        assert!(fields.contains(&"provenance.source".to_string()));

        let authored = cases_of(Kind::Authored, None);
        assert!(verify(&authored, &now(&authored)).ok, "an authored cassette with no pin is honest and passes");
    }

    #[test]
    fn given_interactions_naming_rows_when_replayed_then_evidence_carries_the_label_and_the_kind_that_parity_filters_on() {
        let set = recorded(&mut toy());
        let clean = replay(&set, &mut toy(), &Allowlist::default(), &Default::default(), None).unwrap();
        let rows = evidence(&clean, &set.cassette);
        let named: Vec<_> = set.cassette.interactions.iter().filter(|i| i.row.is_some()).collect();
        assert_eq!(rows.len(), named.len());
        assert!(rows.iter().all(|r| r.diff == Label::Clean && r.provenance == Kind::Recorded));
        let mut drop = toy();
        drop.tweak = Tweak::DropKey;
        let broken = replay(&set, &mut drop, &Allowlist::default(), &Default::default(), None).unwrap();
        assert!(evidence(&broken, &set.cassette).iter().all(|r| r.diff == Label::Breaking));
    }

    #[test]
    fn given_a_reference_that_rejects_an_input_when_recorded_then_the_rejection_is_data_and_replays_clean() {
        let store = store_with_files();
        let corpus = CorpusSpec {
            schema: CORPUS_SCHEMA.into(),
            seed: 1,
            generators: vec![GeneratorSpec::Corpus {
                name: "probe".into(),
                op: Some("reject".into()),
                files: vec![],
                cases: vec![mimic_core::corpus::CaseSpec { id: "bad-shape".into(), row: None, input: json!({"x": null}), tags: vec![] }],
            }],
        };
        let cases = expand(&corpus, &store, "toy").unwrap();
        let rec = record(&mut toy(), &cases, &toy_rules(), Some(&corpus), meta(Kind::Recorded, Some("sha256:p"))).unwrap();
        assert_eq!(rec.set.cassette.interactions[0].outcome.reference_accepts, Some(false));
        let v = replay(&rec.set, &mut toy(), &Allowlist::default(), &Default::default(), None).unwrap();
        assert!(v.ok());
    }

    #[test]
    fn given_a_store_when_a_set_is_written_then_it_reads_back_and_a_missing_set_is_not_found() {
        let set = recorded(&mut toy());
        let mut store = MemStore::default();
        store.write("toy", &set).unwrap();
        assert_eq!(store.read("toy").unwrap(), set);
        assert_eq!(store.list().unwrap(), vec!["toy".to_string()]);
        assert!(matches!(store.read("nope"), Err(mimic_core::ports::StoreError::NotFound(_))));
    }
}

mod corpus {
    use super::cassettes::*;
    use super::*;

    fn seeded(mutators: Vec<Mutator>) -> CorpusSpec {
        CorpusSpec {
            schema: CORPUS_SCHEMA.into(),
            seed: 42,
            generators: vec![GeneratorSpec::Seeded {
                name: "frames".into(),
                op: None,
                count: 20,
                template: json!({"key": "", "n": 0, "kind": ""}),
                fields: vec![
                    FieldSpec { pointer: "/key".into(), gen: Gen::Hex { bytes: 8 } },
                    FieldSpec { pointer: "/n".into(), gen: Gen::Int { min: -3, max: 3 } },
                    FieldSpec { pointer: "/kind".into(), gen: Gen::Choice { values: vec![json!("a"), json!("b")] } },
                ],
                mutators,
            }],
        }
    }

    #[test]
    fn given_a_seed_when_expanded_twice_then_the_cases_are_identical_and_another_seed_gives_other_cases() {
        let store = MemStore::default();
        let a = expand(&seeded(vec![]), &store, "s").unwrap();
        let b = expand(&seeded(vec![]), &store, "s").unwrap();
        assert_eq!(a, b);
        assert_eq!(corpus_hash(&a), corpus_hash(&b));
        let mut other = seeded(vec![]);
        other.seed = 43;
        let c = expand(&other, &store, "s").unwrap();
        assert_ne!(corpus_hash(&a), corpus_hash(&c));
        assert_eq!(a.len(), 20);
    }

    #[test]
    fn given_drawn_fields_when_expanded_then_every_value_respects_its_generator() {
        for c in expand(&seeded(vec![]), &MemStore::default(), "s").unwrap() {
            let key = c.input["key"].as_str().unwrap();
            assert!(key.len() == 16 && key.chars().all(|ch| ch.is_ascii_hexdigit()), "{key}");
            assert!((-3..=3).contains(&c.input["n"].as_i64().unwrap()));
            assert!(["a", "b"].contains(&c.input["kind"].as_str().unwrap()));
        }
    }

    #[test]
    fn given_mutators_when_expanded_then_each_base_case_gains_derived_cases_with_unique_ids_and_one_field_changed() {
        let store = MemStore::default();
        let plain_cases = expand(&seeded(vec![]), &store, "s").unwrap();
        let cases = expand(&seeded(vec![Mutator::Empty, Mutator::Truncate, Mutator::ControlChars, Mutator::Boundary]), &store, "s").unwrap();
        let base: Vec<_> = cases.iter().filter(|c| !c.tags.contains(&"mutated".to_string())).collect();
        assert_eq!(base.len(), plain_cases.len());
        assert!(cases.len() > base.len());
        let mut ids: Vec<_> = cases.iter().map(|c| c.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cases.len());
        for m in cases.iter().filter(|c| c.tags.contains(&"mutated".to_string())) {
            let origin = m.id.split('.').next().unwrap();
            let b = base.iter().find(|c| c.id == origin).unwrap();
            let changed = ["key", "n", "kind"].iter().filter(|k| m.input[**k] != b.input[**k]).count();
            assert!(changed <= 1, "{} changed {changed} fields", m.id);
        }
        // boundary reaches both ends of the integer range
        assert!(cases.iter().any(|c| c.id.contains("boundary-min") && c.input["n"] == -3));
        assert!(cases.iter().any(|c| c.id.contains("boundary-max") && c.input["n"] == 3));
    }

    #[test]
    fn given_a_corpus_file_and_inline_cases_when_expanded_then_both_appear_in_order_and_a_duplicate_id_is_refused() {
        let store = store_with_files();
        let cases = expand(&toy_corpus(), &store, "toy").unwrap();
        assert_eq!(cases[0].id, "echo-ada");
        assert_eq!(cases[1].id, "echo-grace");
        assert_eq!(cases[0].op.as_deref(), Some("echo"));
        let mut dup = toy_corpus();
        if let GeneratorSpec::Corpus { cases, .. } = &mut dup.generators[0] {
            cases.push(cases[0].clone());
        }
        assert!(matches!(expand(&dup, &store, "toy"), Err(mimic_core::corpus::CorpusError::DuplicateId(id)) if id == "echo-ada"));
    }

    #[test]
    fn given_a_missing_corpus_file_or_wrong_schema_when_expanded_then_the_error_names_it() {
        let err = expand(&toy_corpus(), &MemStore::default(), "toy").unwrap_err().to_string();
        assert!(err.contains("cases.json"), "{err}");
        let mut bad = toy_corpus();
        bad.schema = "mimic-corpus/9".into();
        assert!(expand(&bad, &store_with_files(), "toy").unwrap_err().to_string().contains("mimic-corpus/9"));
    }

    fn property() -> CorpusSpec {
        let command = Gen::Record { fields: [("at".to_string(), Gen::Int { min: 0, max: 40 })].into() };
        CorpusSpec {
            schema: CORPUS_SCHEMA.into(),
            seed: 9,
            generators: vec![GeneratorSpec::Property {
                name: "lists".into(),
                op: Some("echo".into()),
                count: 12,
                template: json!({"label": "", "commands": []}),
                fields: vec![
                    FieldSpec { pointer: "/label".into(), gen: Gen::Choice { values: vec![json!("a"), json!("b")] } },
                    FieldSpec { pointer: "/commands".into(), gen: Gen::List { min_len: 2, max_len: 6, item: Box::new(command) } },
                ],
                shrink_steps: 4,
            }],
        }
    }

    #[test]
    fn given_a_property_generator_when_expanded_then_it_is_deterministic_and_every_ladder_case_is_smaller_than_its_base() {
        let store = MemStore::default();
        let cases = expand(&property(), &store, "s").unwrap();
        assert_eq!(cases, expand(&property(), &store, "s").unwrap());
        let size = |c: &mimic_core::corpus::Case| c.input.to_string().len();
        let ladder: Vec<_> = cases.iter().filter(|c| c.tags.contains(&"shrink".to_string())).collect();
        assert!(!ladder.is_empty());
        for s in ladder {
            let of = s.tags.iter().find_map(|t| t.strip_prefix("shrink-of:")).unwrap();
            let base = cases.iter().find(|c| c.id == of).unwrap();
            assert!(size(s) <= size(base) && s.input != base.input, "{} is not a smaller {of}", s.id);
            let n = s.input["commands"].as_array().unwrap().len();
            assert!(n >= 2, "{} shrank below the list minimum", s.id);
        }
        let bases = cases.iter().filter(|c| !c.tags.contains(&"shrink".to_string())).count();
        assert_eq!(bases, 12);
    }

    #[test]
    fn given_a_candidate_that_breaks_on_long_command_lists_when_replayed_then_the_report_names_the_smallest_stored_failing_case() {
        struct LongListsBreak;
        impl Driver for LongListsBreak {
            fn hello(&mut self) -> Result<Hello, DriverError> {
                Ok(Hello { driver: driver_info("long-lists"), target_version: Some("1".into()), coverage: false })
            }
            fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
                let mut o = ToyDriver::new(1, 0).run(request)?;
                if request.input["commands"].as_array().is_some_and(|l| l.len() > 2) {
                    o.steps[0].payload = json!({"label": "broken"});
                }
                Ok(o)
            }
        }
        let corpus = property();
        let cases = expand(&corpus, &MemStore::default(), "s").unwrap();
        let set = record(&mut ToyDriver::new(1, 0), &cases, &toy_rules(), Some(&corpus), meta(Kind::Recorded, Some("sha256:p"))).unwrap().set;
        let verdict = replay(&set, &mut LongListsBreak, &Allowlist::default(), &Default::default(), None).unwrap();
        assert!(verdict.summary.breaking > 0);
        assert!(!verdict.shrunk.is_empty());
        for s in &verdict.shrunk {
            let family: Vec<_> = verdict.results.iter().filter(|r| r.id == s.case || r.id.starts_with(&format!("{}.shrink.", s.case))).collect();
            let smallest_failing = family
                .iter()
                .filter(|r| r.label != Label::Clean)
                .map(|r| set.cassette.interactions.iter().find(|i| i.id == r.id).unwrap().input.to_string().len())
                .min()
                .unwrap();
            assert_eq!(s.input.to_string().len(), smallest_failing, "{} named {}", s.case, s.minimal);
            assert!(s.input["commands"].as_array().unwrap().len() > 2, "the minimal case must still fail");
        }
        let clean = replay(&set, &mut ToyDriver::new(1, 0), &Allowlist::default(), &Default::default(), None).unwrap();
        assert!(clean.shrunk.is_empty());
    }

    #[test]
    fn given_a_corpus_spec_when_serialised_then_it_parses_back_to_the_same_spec() {
        let spec = seeded(vec![Mutator::Boundary, Mutator::ControlChars]);
        let text = serde_json::to_string(&spec).unwrap();
        assert!(text.contains("\"kind\":\"seeded\"") && text.contains("control-chars"));
        assert_eq!(serde_json::from_str::<CorpusSpec>(&text).unwrap(), spec);
    }
}

mod crawl_use_case {
    use super::*;
    use mimic_core::corpus::CaseSpec;
    use mimic_core::crawl::{crawl, Catalog, CrawlError, Endpoint, Pin, Report, REPORT_SCHEMA};
    use mimic_core::testing::ScriptedCrawler;

    fn endpoint(id: &str) -> Endpoint {
        Endpoint { id: id.into(), kind: "rpc".into(), name: id.into(), transport: "rpc".into(), method: None, path: None, streaming: false, users: vec!["internal".into()], sources: vec!["schema".into()], access: None, input: "schema".into(), classes: [("valid".to_string(), 1), ("invalid".to_string(), 1)].into(), skipped: None }
    }

    fn case(row: &str, class: &str) -> CaseSpec {
        CaseSpec { id: format!("{row}-{class}"), row: Some(row.into()), input: json!({}), tags: vec![format!("class:{class}")] }
    }

    fn report(ids: &[&str]) -> Report {
        Report {
            schema: REPORT_SCHEMA.into(), app: "toy".into(), pin: Pin { version: None, commit: "c1".into() }, sources: vec![], cli_commands: vec![], blind: vec![],
            endpoints: ids.iter().map(|i| endpoint(i)).collect(),
            cases: ids.iter().flat_map(|i| [case(i, "valid"), case(i, "invalid")]).collect(),
        }
    }

    #[test]
    fn given_a_crawler_double_when_its_report_covers_the_catalog_then_there_are_no_problems_and_equal_reports_give_equal_files() {
        // Given a catalog of ids and a crawler that reports exactly them
        let ids = ["rpc:a", "rpc:b", "rpc:c"];
        let catalog = Catalog { ids: ids.iter().map(|s| s.to_string()).collect() };
        // When crawled twice
        let one = crawl(&mut ScriptedCrawler(Ok(report(&ids))), Some(&catalog), Some("c1")).unwrap();
        let two = crawl(&mut ScriptedCrawler(Ok(report(&ids))), Some(&catalog), Some("c1")).unwrap();
        // Then nothing is wrong, the three files are identical and the offline reading of the files finds the same report
        assert!(one.problems.is_empty() && one.warnings.is_empty(), "{:?}", one.problems);
        assert_eq!(one.outputs, two.outputs);
        let back = mimic_core::crawl::from_files(&one.outputs.inventory, &one.outputs.corpus).unwrap();
        assert_eq!(mimic_core::crawl::outputs(&back), one.outputs);
    }

    #[test]
    fn given_a_catalog_row_the_report_lacks_when_checked_then_exactly_that_row_lags_and_a_failing_crawler_is_an_error() {
        // Given a catalog with one id more than the report
        let ids = ["rpc:a", "rpc:b"];
        let mut catalog = Catalog { ids: ids.iter().map(|s| s.to_string()).collect() };
        catalog.ids.insert("rpc:missing".into());
        // When crawled, Then the lag names that id only
        let got = crawl(&mut ScriptedCrawler(Ok(report(&ids))), Some(&catalog), None).unwrap();
        assert_eq!(got.problems.iter().map(|p| (p.code, p.id.as_str())).collect::<Vec<_>>(), [("lags-source", "rpc:missing")]);
        // And a crawler that fails is the error of the use case
        let failed = crawl(&mut ScriptedCrawler(Err(CrawlError("no tree".into()))), None, None);
        assert_eq!(failed.unwrap_err(), CrawlError("no tree".into()));
    }
}
