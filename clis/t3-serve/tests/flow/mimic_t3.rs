//! The imported goldens of the pinned server (`cassettes/t3-v0.1.6`, converted from `tooling/t3-conformance`) replayed by mimic against the
//! in-process router: the client side of the recorded session over real HTTP and WebSocket, the wire masked by the set's rules and diffed
//! under the imported allowlist. A divergence is fixed in the server or named in the allowlist with its reason; an unlisted one fails.
use super::support::mimic_t3::*;
use super::support::*;
use mimic_core::differ::Verdict;
use mimic_core::presets::t3_diff_options;
use mimic_core::usecase::replay;

/// The folder the recorded session makes its project on, which the in-process server is given as its root.
const WORKSPACE: &str = "/conformance/workspace";

fn replayed(set: &mimic_core::ports::CassetteSet, allow: &mimic_core::allowlist::Allowlist) -> Verdict {
    let h = Harness::start();
    *h.agent.root.lock().unwrap() = Some("/conformance".into());
    let pair = |scopes: Vec<String>| h.pair(PairingRequest { scopes, ..PairingRequest::default() }).code;
    let mut candidate = Candidate { addr: h.addr, pair: &pair, scopes: granted_scopes(set), workspace: WORKSPACE };
    replay(set, &mut candidate, allow, &t3_diff_options(), None).expect("the replay runs")
}

#[test]
fn the_imported_session_of_the_real_server_replays_through_the_router_with_no_difference_the_imported_allowlist_does_not_name() {
    // Given the set imported from the goldens of t3 v0.1.6 and its imported allowlist
    let (set, allow) = (load_set(SET), load_allowlist(SET));
    assert!(set.cassette.interactions.len() >= 3, "the set holds the descriptor, the 426 gate and the session");
    // When the client side of every interaction is replayed against this server
    let verdict = replayed(&set, &allow);
    // Then every masked answer equals the recorded one, apart from the divergences the allowlist names
    let found = differences(&verdict);
    assert!(found.is_empty(), "{} differences:\n{}", found.len(), found.join("\n"));
}

/// Values that equal the recorded ones on a host like the recording machine (linux x64), so removing their entry fails only elsewhere.
const HOST_DEPENDENT: [&str; 2] = ["platform.os", "platform.arch"];

type Section = fn(&mut mimic_core::allowlist::Allowlist) -> &mut Vec<mimic_core::allowlist::Entry>;

#[test]
fn removing_an_allowlist_entry_makes_the_replay_fail_naming_it() {
    // Given every entry of every section of the imported allowlist
    let (set, allow) = (load_set(SET), load_allowlist(SET));
    let sections: [(&str, Section); 3] = [("ignored", |a| &mut a.ignored), ("omitted", |a| &mut a.omitted), ("shapeOnly", |a| &mut a.shape_only)];
    for (name, section) in sections {
        let mut probe = allow.clone();
        let entries = section(&mut probe).clone();
        assert!(!entries.is_empty(), "{name} has entries");
        for entry in entries.iter().filter(|e| !HOST_DEPENDENT.contains(&e.path.as_str())) {
            // When that entry alone is removed and the session is replayed
            let mut without = allow.clone();
            section(&mut without).retain(|e| e.path != entry.path);
            let found = differences(&replayed(&set, &without));
            // Then the replay fails with a difference at that path
            assert!(found.iter().any(|d| d.contains(&entry.path)), "{name} entry {} is not needed: {found:?}", entry.path);
        }
    }
}
