//! Flows through the public API `wf offload ask` and `wf` build on, replaying `tests/flow/golden/rca-core.json`: the
//! `--stats` report, the `--savings` report, and the opencode process contract.
#[path = "flow/golden.rs"]
mod golden;

use golden::{golden, iso_or_null, norm, opt, s};
use rca_core::run_log::parse_run_log;
use rca_core::stats::{filter_runs, format_stats, js_json_pretty, notional_cost, parse_since, percentile, shape_stats, stats, to_fixed, ModelPricing, PricingTable, StatsFilter};
use rca_core::time::{date_ms, iso, parse, utc_date};
use serde_json::Value;
use std::collections::HashMap;

#[test]
fn given_a_run_log_when_the_stats_report_is_built_then_filters_groups_costs_and_both_renderings_match_the_typescript_byte_for_byte() {
    let g = golden();
    let rl = &g["runLog"];

    // Given the timestamps a run log carries
    // Then they read and print as JavaScript's Date does
    assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso(1_791_288_000_000), "2026-10-06T12:00:00.000Z");
    for (text, ms) in [
        ("2026-10-06T12:00:00.000Z", Some(1_791_288_000_000)),
        ("2026-10-06T14:00:00+02:00", Some(1_791_288_000_000)),
        ("2026-10-06T12:00:00.5Z", Some(1_791_288_000_500)),
        ("2026-10-06T12:00Z", None),
        ("2026-10-06T12:00", Some(1_791_288_000_000)),
        ("garbage", None),
    ] {
        assert_eq!(parse(text), ms, "{text}");
    }
    assert_eq!(parse(&iso(951_782_400_123)), Some(951_782_400_123));
    assert_eq!(utc_date(1_791_288_000_000), "2026-10-06");
    assert_eq!((date_ms("2026-10-06"), date_ms("2026-02-30")), (Some(1_791_244_800_000), None));

    // When the recorded run log is parsed
    let records = parse_run_log(s(&rl["text"]));
    // Then the rows are the recorded ones
    assert_eq!(norm(&serde_json::to_value(&records).unwrap()), norm(&rl["parsed"]));

    // When it is filtered by --since, session and caller, and per-shape stats are read
    let now = rl["now"].as_i64().unwrap();
    for c in rl["sinceSpecs"].as_array().unwrap() {
        assert_eq!(iso_or_null(parse_since(opt(&c["spec"]), now)), c["out"], "{c}");
    }
    assert_eq!(iso_or_null(parse_since(None, now)), rl["sinceUndefined"]);
    for c in rl["filters"].as_array().unwrap() {
        let f = &c["filter"];
        let filter = StatsFilter {
            since_ms: opt(&f["since"]).and_then(|spec| parse_since(Some(spec), now)),
            claude_session_id: opt(&f["claudeSessionId"]).map(String::from),
            caller: opt(&f["caller"]).map(String::from),
        };
        let want: Vec<String> = serde_json::from_value(c["out"].clone()).unwrap();
        assert_eq!(filter_runs(&records, &filter).into_iter().map(|r| r.ts).collect::<Vec<_>>(), want, "{f}");
    }
    for c in rl["shapes"].as_array().unwrap() {
        let got: serde_json::Map<String, Value> = shape_stats(&records, s(&c["shape"])).into_iter().map(|(m, st)| (m, serde_json::json!({ "okRate": st.ok_rate, "runs": st.runs }))).collect();
        let want: serde_json::Map<String, Value> = c["out"].as_object().unwrap().iter().map(|(m, v)| (m.clone(), serde_json::json!({ "okRate": v["okRate"], "runs": v["runs"] }))).collect();
        assert_eq!(norm(&Value::Object(got)), norm(&Value::Object(want)), "{c}");
    }

    // When the stats are computed, priced and unpriced, and rendered as the table and the full JSON
    let pricing: PricingTable = serde_json::from_value(rl["pricing"].clone()).unwrap();
    let priced = stats(&records, Some(&pricing));
    // Then each is the recorded one, empty logs included
    assert_eq!(norm(&serde_json::to_value(&priced).unwrap()), norm(&rl["stats"]));
    assert_eq!(norm(&serde_json::to_value(stats(&records, None)).unwrap()), norm(&rl["statsUnpriced"]));
    assert_eq!(format_stats(&priced), s(&rl["formatted"]));
    assert_eq!(format_stats(&stats(&[], None)), s(&rl["formattedEmpty"]));
    for (rows, key) in [(records.as_slice(), "statsFullJson"), (&[][..], "statsFullJsonEmpty")] {
        let mut doc = serde_json::Map::new();
        doc.insert("logPath".into(), Value::String("/tmp/runs.jsonl".into()));
        doc.extend(serde_json::to_value(stats(rows, None)).unwrap().as_object().unwrap().clone());
        assert_eq!(js_json_pretty(&Value::Object(doc)), s(&rl[key]), "{key}");
    }
    for c in rl["percentiles"].as_array().unwrap() {
        let values: Vec<f64> = serde_json::from_value(c["values"].clone()).unwrap();
        assert_eq!(percentile(&values, c["p"].as_f64().unwrap()), c["out"].as_f64().unwrap(), "{c}");
    }
    let rates: HashMap<String, ModelPricing> = serde_json::from_value(rl["pricing"].clone()).unwrap();
    for c in rl["notional"].as_array().unwrap() {
        let tokens: Option<rca_core::run_log::RunTokens> = (!c["tokens"].is_null()).then(|| serde_json::from_value(c["tokens"].clone()).unwrap());
        assert_eq!(notional_cost(tokens.as_ref(), rates.get(s(&c["model"]))), c["out"].as_f64(), "{c}");
    }

    // Given one caller per cost that sits on or near a rounding tie
    let cases = [
        (0.0625, "0.063"),
        (0.0124, "0.012"),
        (1.0, "1.000"),
        (0.0, "0.000"),
        (-0.0004, "-0.000"),
        (-2.0, "-2.000"),
        (-0.0625, "-0.063"),
        (1.0005, "1.000"),
        (12345.6785, "12345.678"),
        (0.0005, "0.001"),
        (1.4995, "1.500"),
    ];
    let log: String = cases.iter().enumerate().map(|(i, (cost, _))| format!("{{\"ts\":\"2026-10-06T10:00:00.000Z\",\"ok\":true,\"caller\":\"c{i}\",\"cost\":{cost}}}\n")).collect();
    // When the table is rendered
    let table = format_stats(&stats(&parse_run_log(&log), None));
    // Then each caller's cost is printed as JavaScript's toFixed(3) prints it: ties up, on the exact binary value
    for (i, (cost, want)) in cases.iter().enumerate() {
        let line = table.lines().find(|l| l.split_whitespace().next() == Some(&format!("c{i}"))).unwrap();
        assert_eq!(line.split_whitespace().nth(6), Some(format!("${want}").as_str()), "{cost}");
        assert_eq!(to_fixed(*cost, 3), *want, "{cost}");
    }
}

#[test]
fn given_a_run_log_and_a_delegation_report_when_savings_are_computed_then_each_ratio_variant_renders_as_the_typescript_did() {
    use rca_core::savings::{compute_savings, default_ratios, format_savings, parse_delegation_report, DelegationReportRow, SavingsRatios};
    let g = golden();
    let sv = &g["savings"];

    // Given the run log and the delegation report
    let records = parse_run_log(s(&sv["runLogText"]));
    let report = parse_delegation_report(s(&sv["reportText"]));
    assert_eq!(norm(&serde_json::to_value(&report).unwrap()), norm(&sv["reportRows"]));
    let custom: SavingsRatios = serde_json::from_value(sv["custom"].clone()).unwrap();

    // When savings are computed with the default, custom and negative ratios, with and without the report
    for v in sv["variants"].as_array().unwrap() {
        let ratios = match s(&v["name"]) {
            "default" => default_ratios(),
            "custom" => custom.clone(),
            "negative" => {
                let mut r = custom.clone();
                r.source = "negative savings".into();
                for size in [&mut r.sizes.small, &mut r.sizes.medium, &mut r.sizes.large] {
                    size.claude_usd = 0.0001;
                    size.n = 1.0;
                }
                (r.sizes.small.claude_seconds, r.sizes.medium.claude_seconds, r.sizes.large.claude_seconds) = (0.2, 0.5, 1000.5);
                r
            }
            other => panic!("unknown variant {other}"),
        };
        let rows: &[DelegationReportRow] = if v["withReport"].as_bool().unwrap() { &report } else { &[] };
        let got = compute_savings(&records, &ratios, rows);
        // Then the numbers, the text and the JSON document are the recorded ones
        assert_eq!(norm(&serde_json::to_value(&got).unwrap()), norm(&v["savings"]), "{}", v["name"]);
        assert_eq!(format_savings(&got), s(&v["formatted"]), "{}", v["name"]);
        let mut doc = serde_json::Map::new();
        doc.insert("logPath".into(), Value::String("/tmp/runs.jsonl".into()));
        doc.insert("reportPath".into(), Value::String("/tmp/report.jsonl".into()));
        doc.extend(serde_json::to_value(&got).unwrap().as_object().unwrap().clone());
        assert_eq!(js_json_pretty(&Value::Object(doc)), s(&v["json"]), "{}", v["name"]);
    }

    // When nothing was dispatched, then the empty report renders as recorded
    let empty = compute_savings(&[], &default_ratios(), &[]);
    assert_eq!(format_savings(&empty), s(&sv["emptyFormatted"]));
    assert_eq!(js_json_pretty(&serde_json::to_value(&empty).unwrap()), s(&sv["emptyJson"]));
}

#[test]
fn given_an_opencode_dispatch_when_its_command_environment_and_event_stream_are_handled_then_args_keys_tool_lines_and_summaries_match() {
    use rca_core::opencode::{build_opencode_args, format_dispatch_summary, has_completed_write, last_text_part, looks_like_cap_error, looks_like_transient_crash, opencode_env_overrides, parse_opencode_json_stream, tool_lines_from_events, SummaryInput};
    let g = golden();
    let oc = &g["opencode"];

    // Given each recorded launch
    // Then the opencode command line is the recorded one
    for c in oc["argCases"].as_array().unwrap() {
        let i = &c["input"];
        let got = build_opencode_args(i["auto"].as_bool().unwrap(), s(&i["format"]) == "json", opt(&i["continueSessionId"]), opt(&i["agent"]), s(&i["cwd"]), s(&i["model"]), s(&i["prompt"]));
        let want: Vec<String> = serde_json::from_value(c["out"].clone()).unwrap();
        assert_eq!(got, want, "{i}");
    }

    // Given an environment with the Go key, a provider key and an empty one
    let env = |k: &str| match k {
        "OPENCODE_GO_API_KEY" => Some("go".to_string()),
        "MINIMAX_API_KEY" => Some("mm".to_string()),
        "KIMI_API_KEY" => Some(String::new()),
        _ => None,
    };
    // Then the Go key is handed over as opencode's own, the provider key is copied, and nothing else is set
    assert_eq!(opencode_env_overrides(&env), vec![("OPENCODE_API_KEY".to_string(), "go".to_string()), ("MINIMAX_API_KEY".to_string(), "mm".to_string())]);
    assert!(opencode_env_overrides(&|_| None).is_empty());

    // When each recorded event stream is parsed
    for c in oc["streams"].as_array().unwrap() {
        let parsed = parse_opencode_json_stream(s(&c["raw"]));
        // Then the events, tool lines, write detection and final text are the recorded ones
        assert_eq!(norm(&serde_json::to_value(&parsed).unwrap()), norm(&c["parsed"]), "{}", c["raw"]);
        let lines: Vec<String> = serde_json::from_value(c["toolLines"].clone()).unwrap();
        assert_eq!(tool_lines_from_events(&parsed.tool_events), lines);
        assert_eq!(has_completed_write(&parsed.tool_events), c["hasWrite"].as_bool().unwrap());
        assert_eq!(last_text_part(&parsed.text_parts), s(&c["last"]));
    }
    for c in oc["texts"].as_array().unwrap() {
        assert_eq!(looks_like_cap_error(s(&c["text"])), c["cap"].as_bool().unwrap(), "cap: {}", c["text"]);
        assert_eq!(looks_like_transient_crash(s(&c["text"])), c["crash"].as_bool().unwrap(), "crash: {}", c["text"]);
    }

    // When a finished dispatch is summarized, brief and full
    // Then the summary is the recorded one
    for c in oc["summaries"].as_array().unwrap() {
        let input: SummaryInput = serde_json::from_value(c["result"].clone()).unwrap();
        assert_eq!(format_dispatch_summary(&input, s(&c["mode"]) == "brief"), s(&c["out"]), "{c}");
    }
}
