//! Rule sets and differ options of the references mimic already knows. They are the data X5 writes to `rules.json` when the T3 goldens
//! are imported; until then they let the oracle test hold the Rust rules to the TS normaliser's goldens.
use crate::cassette::Dir;
use crate::differ::{DiffOptions, Fold};
use crate::rules::{Rule, RuleSet};
use std::collections::BTreeMap;

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// The T3 conformance normaliser (the old TS recorder's normalize.ts, retired in X5) as data.
pub fn t3() -> RuleSet {
    RuleSet::new(vec![
        Rule::Token { pattern: r"\b[A-Za-z0-9_-]{24,}\.[A-Za-z0-9_-]{16,}\b".into(), placeholder: "<signed-token>".into() },
        Rule::UuidOrdinal,
        Rule::Timestamp { keys: Vec::new(), placeholder: "<ts>".into() },
        Rule::SecretKey {
            keys: map(&[
                ("access_token", "<access-token>"),
                ("subject_token", "<pairing-credential>"),
                ("credential", "<pairing-credential>"),
                ("ticket", "<ws-ticket>"),
                ("wsTicket", "<ws-ticket>"),
                ("token", "<pairing-credential>"),
                ("csrfToken", "<csrf>"),
            ]),
        },
        Rule::Sequence { keys: strings(&["sequence", "snapshotSequence", "afterSequence", "latestSequence"]), placeholder: "<seq>".into() },
        Rule::VolatileNumber { keys: map(&[("expires_in", "<expires-in>"), ("pid", "<pid>"), ("fiberId", "<fiber>")]) },
        Rule::VolatileString { keys: map(&[("traceId", "<trace>"), ("spanId", "<trace>")]) },
        Rule::DropHeader {
            names: strings(&[
                "date",
                "host",
                "connection",
                "keep-alive",
                "content-length",
                "etag",
                "last-modified",
                "transfer-encoding",
                "user-agent",
                "accept-encoding",
                "accept-language",
                "sec-websocket-key",
                "sec-websocket-accept",
                "sec-websocket-version",
                "sec-fetch-mode",
                "sec-fetch-site",
                "sec-fetch-dest",
                "upgrade",
            ]),
        },
    ])
}

/// The differ's T3 semantics (the old TS differ, retired in X5): the status of an HTTP answer or an upgrade is behaviour, a recorded
/// capability flag an advertisement. The paths are where the wire dialect's steps keep them.
pub const T3_BREAKING_PATHS: [&str; 2] = ["steps.http:ret:_.payload.status", "steps.ws:open:s2c:_.payload.status"];

/// The real server splits a subscription's values into chunks as it likes, re-sends a shell snapshot after the first project (racing the update,
/// docs/plans/t3-parity/phase0-report.md item 12) and lists one Interrupt per fiber it stops: the values and their order are what count.
pub fn t3_fold() -> Fold {
    Fold {
        channel: "ws".into(),
        tag: "/_tag".into(),
        chunk: "Chunk".into(),
        values: "/values".into(),
        once: vec![("/kind".into(), "snapshot".into())],
        squash: strings(&["/exit/cause"]),
    }
}

pub fn t3_diff_options() -> DiffOptions {
    DiffOptions { ignore_dirs: vec![Dir::C2s], fold: Some(t3_fold()), breaking_paths: strings(&T3_BREAKING_PATHS), advertised_prefixes: strings(&["capabilities."]) }
}
