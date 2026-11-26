//! A run a proxy saw becomes a cassette set that is fit for a file: sanitised first, sealed after.
use crate::domain::interactions::interactions;
use crate::domain::sanitize::{sanitise, Leak, Policy, Sanitised};
use crate::domain::traffic::Event;
use mimic_core::cassette::{Cassette, DriverInfo, Header, Kind, Provenance, RefHash, Target, ToolInfo, SCHEMA};
use mimic_core::ports::CassetteSet;
use mimic_core::rules::{Rule, RuleSet};

pub const PROTOCOL: &str = "mimic-proxy-record/1";
pub const RULES_FILE: &str = "rules.json";

/// What the caller knows about the run. None of it comes from the traffic.
#[derive(Clone, Debug)]
pub struct Meta {
    pub kind: Kind,
    pub target: Option<Target>,
    pub tool: ToolInfo,
    pub recorded_at: Option<String>,
    pub recorded_on: Option<String>,
    pub source: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug)]
pub struct Recorded {
    pub set: CassetteSet,
    pub sanitised: Sanitised,
    pub failures: Vec<&'static str>,
}

/// The rules a set without a preset matches and diffs under: ids and times a client or server makes up per run.
pub fn default_rules() -> RuleSet {
    let trace = [("traceId", "<trace>"), ("spanId", "<trace>")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    RuleSet::new(vec![Rule::UuidOrdinal, Rule::Timestamp { keys: Vec::new(), placeholder: "<ts>".into() }, Rule::VolatileString { keys: trace }])
}

/// The credential names a rule set declares, added to the policy's own.
pub fn policy_for(rules: &RuleSet, mut policy: Policy) -> Policy {
    for rule in &rules.rules {
        if let Rule::SecretKey { keys } = rule {
            policy.secret_keys.extend(keys.keys().filter(|k| !policy.secret(k)).cloned().collect::<Vec<_>>());
        }
    }
    policy
}

pub fn record(events: &[Event], rules: RuleSet, policy: &Policy, meta: Meta) -> Result<Recorded, Leak> {
    let mut its = interactions(events);
    let sanitised = sanitise(&mut its, policy)?;
    let failures = events.iter().filter_map(|e| if let Event::Failed { failure } = e { Some(failure.label()) } else { None }).collect();
    let rules_hash = rules.hash();
    let provenance = Provenance {
        kind: meta.kind,
        target: meta.target,
        driver: Some(DriverInfo { name: "mimic-proxy".into(), version: meta.tool.mimic.clone(), protocol: PROTOCOL.into(), toolchain_lock_hash: None }),
        tool: meta.tool,
        seed: None,
        rules_hash: rules_hash.clone(),
        corpus_hash: None,
        recorded_at: meta.recorded_at,
        recorded_on: meta.recorded_on,
        source: meta.source,
        note: meta.note,
        imported_from: Vec::new(),
    };
    let header = Header { schema: SCHEMA.into(), provenance, rules: RefHash { path: RULES_FILE.into(), hash: rules_hash }, corpus: None, interactions: 0, content_hash: String::new() };
    let mut cassette = Cassette { header, interactions: its };
    cassette.seal();
    Ok(Recorded { set: CassetteSet { cassette, rules, corpus: None }, sanitised, failures })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::traffic::Failure;

    fn meta() -> Meta {
        Meta { kind: Kind::Authored, target: None, tool: ToolInfo { mimic: "0.1.0".into(), git: None }, recorded_at: None, recorded_on: None, source: Some("toy upstream".into()), note: None }
    }

    #[test]
    fn given_a_run_with_a_credential_when_recorded_then_the_set_is_sealed_parses_back_and_holds_no_credential() {
        // Given
        let events = vec![
            Event::HttpRequest { exchange: 1, method: "GET".into(), target: "/me".into(), headers: vec![("authorization".into(), "Bearer LIVE-TOKEN-CANARY".into())], body: Vec::new() },
            Event::HttpResponse { exchange: 1, status: 200, headers: Vec::new(), body: br#"{"name":"fixture user"}"#.to_vec() },
            Event::Failed { failure: Failure::NoAnswer },
        ];

        // When
        let recorded = record(&events, default_rules(), &Policy::default(), meta()).expect("recorded");

        // Then
        let cassette = &recorded.set.cassette;
        let parsed = Cassette::parse(&cassette.header_json(), &cassette.jsonl()).expect("the set parses back");
        assert_eq!(parsed.interactions.len(), 1);
        assert!(cassette.problems().is_empty(), "{:?}", cassette.problems());
        assert!(!cassette.jsonl().contains("CANARY"));
        assert_eq!(recorded.failures, ["the server did not answer"]);
    }

    #[test]
    fn given_rules_that_name_a_credential_key_when_the_policy_is_made_then_the_key_is_one_of_its_own() {
        // Given
        let rules = RuleSet::new(vec![Rule::SecretKey { keys: [("launchSecret".to_string(), "<s>".to_string())].into_iter().collect() }]);

        // When
        let policy = policy_for(&rules, Policy::default());

        // Then
        assert!(policy.secret("launch_secret"));
    }
}
