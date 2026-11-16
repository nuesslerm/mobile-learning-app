//! The decode defects of the pinned server (the crawl set's `-invalid-` cases of each RPC group below) replayed against the router: the
//! payload that does not decode is a `Die` with the decoder's text, as recorded. The slice of one group at flow tier, in place of the 415 s
//! chain replay; the chain replay (`recorded_r1`) holds the same ids to `known-gaps.json`.
//! `CRAWL_PREFIX=<id substring>[,..]` replays that slice and prints what differs; with `CRAWL_WRITE_GAPS=1` it rewrites those ids in the gaps file.
use super::support::mimic_t3::*;
use super::support::*;
use mimic_core::differ::Severity;
use mimic_core::gaps;
use mimic_core::presets::t3_diff_options;
use mimic_core::usecase::replay;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/cassettes/t3-v0.1.9-crawl");

/// The groups whose invalid cases the router decodes as the real server does.
const GROUPS: [&str; 15] = ["t3-rpc-provider.install.", "AcpRegistry", "t3-rpc-vcs.", "t3-rpc-subscribeVcsBranch-", "t3-rpc-git.", "t3-rpc-review.", "t3-rpc-sourceControl.lookup", "t3-rpc-sourceControl.clone", "t3-rpc-sourceControl.signIn", "t3-rpc-sourceControl.appAccess", "t3-rpc-sourceControl.starred-", "t3-rpc-sourceControl.publish", "t3-rpc-provider.auth.", "t3-rpc-provider.chatgpt.", "t3-rpc-server.updateProvider-"];

/// Tags the router does not know yet (`scopes.rs` lacks them), so their requests never reach a decoder.
const NOT_ROUTED: [&str; 2] = ["t3-rpc-sourceControl.star-", "t3-rpc-provider.auth.removeMethod-"];

fn found(set: &mimic_core::ports::CassetteSet, allow: &mimic_core::allowlist::Allowlist, ids: &[String]) -> Vec<gaps::Found> {
    let mut out = Vec::new();
    for id in ids {
        let h = std::rc::Rc::new(Harness::start());
        *h.agent.root.lock().unwrap() = Some("/conformance".into());
        let (scopes, paired) = (granted_scopes(set), h.clone());
        let pair = std::rc::Rc::new(move || paired.pair(PairingRequest { scopes: scopes.clone(), ..PairingRequest::default() }).code);
        let mut candidate = driver(h.addr, "/conformance/workspace".to_string(), pair, prelude_of(set), true);
        let verdict = replay(set, &mut candidate, allow, &t3_diff_options(), Some(&[id.clone()])).expect("the replay runs");
        for r in verdict.results {
            let mut lines: Vec<String> = r.diffs.iter().filter(|d| d.severity != Severity::None).map(|d| format!("{} {} {}", d.path, format!("{:?}", d.kind).to_lowercase(), format!("{:?}", d.severity).to_lowercase())).collect();
            lines.extend(r.error.map(|e| format!("error {e}")));
            if !lines.is_empty() {
                out.push(gaps::Found { id: r.id, lines });
            }
        }
    }
    out
}

fn ids_of(set: &mimic_core::ports::CassetteSet, prefixes: &[String], invalid_only: bool) -> Vec<String> {
    set.cassette.interactions.iter().map(|i| i.id.clone()).filter(|id| prefixes.iter().any(|p| id.contains(p.as_str())) && !NOT_ROUTED.iter().any(|n| id.contains(n)) && (!invalid_only || id.contains("-invalid-"))).collect()
}

#[test]
fn the_invalid_payloads_of_the_decoded_groups_are_the_defects_the_real_server_answered() {
    // Given the crawl set of the pinned server and the groups decoded here
    let (set, allow) = (load_set(DIR), load_allowlist(DIR));
    let prefixes: Vec<String> = GROUPS.iter().map(|g| g.to_string()).collect();
    let ids = ids_of(&set, &prefixes, true);
    assert!(ids.len() >= 3, "{ids:?}");
    // When each invalid case is replayed against the router
    let differences = found(&set, &allow, &ids);
    // Then none differs from what the real server answered
    assert!(differences.is_empty(), "{differences:#?}");
}

/// A developer's slice: what differs for the invalid cases of `CRAWL_PREFIX` against `known-gaps.json` (a valid case answers from the agent API,
/// which the chain replay settles).
#[test]
fn a_slice_named_by_crawl_prefix_is_compared_with_the_known_gaps() {
    let Ok(prefix) = std::env::var("CRAWL_PREFIX") else { return };
    let (set, allow) = (load_set(DIR), load_allowlist(DIR));
    let ids = ids_of(&set, &prefix.split(',').map(String::from).collect::<Vec<_>>(), true);
    let differences = found(&set, &allow, &ids);
    let path = format!("{DIR}/known-gaps.json");
    let mut listed = gaps::listed(&serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()).unwrap();
    listed.retain(|id, _| ids.contains(id));
    let (new, stale) = gaps::compare(&differences, &listed);
    eprintln!("found:\n{differences:#?}");
    eprintln!("slice of {} ids: {} differ\nnot listed:\n{}\nlisted and no longer differing as listed:\n{stale:?}", ids.len(), differences.len(), new.join("\n"));
    if std::env::var("CRAWL_WRITE_GAPS").is_ok() {
        let all: std::collections::BTreeMap<String, Vec<String>> = gaps::listed(&serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()).unwrap().into_iter().filter(|(id, _)| !ids.contains(id)).chain(differences.iter().map(|f| (f.id.clone(), f.lines.clone()))).collect();
        let merged: Vec<gaps::Found> = all.into_iter().map(|(id, lines)| gaps::Found { id, lines }).collect();
        std::fs::write(&path, serde_json::to_string_pretty(&gaps::file(&merged)).unwrap() + "\n").unwrap();
    }
}
