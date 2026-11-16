//! The decode defects of the pinned server (the crawl set's cases of each RPC group below that the real server answered with the decoder's defect) replayed against the router: the
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

/// The groups whose decoder-defect cases the router decodes as the real server does.
const GROUPS: [&str; 78] = ["t3-rpc-provider.install.", "AcpRegistry", "t3-rpc-vcs.", "t3-rpc-subscribeVcsBranch-", "t3-rpc-git.", "t3-rpc-review.", "t3-rpc-sourceControl.lookup", "t3-rpc-sourceControl.clone", "t3-rpc-sourceControl.signIn", "t3-rpc-sourceControl.appAccess", "t3-rpc-sourceControl.starred-", "t3-rpc-sourceControl.publish", "t3-rpc-provider.auth.", "t3-rpc-provider.chatgpt.", "t3-rpc-server.updateProvider-", "t3-rpc-projects.searchEntries-", "t3-rpc-projects.searchContents-", "t3-rpc-projects.listIgnoredFiles-", "t3-rpc-projects.writeFile-", "t3-rpc-projects.createNew-", "t3-rpc-projects.readFile-", "t3-rpc-projects.mutate-", "t3-rpc-pullRequests.routingIdentity-", "t3-rpc-pullRequests.linkedThreads-", "t3-rpc-pullRequests.generateReviewGuide-", "t3-rpc-pullRequests.comment-", "t3-rpc-pullRequests.update-", "t3-rpc-pullRequests.updateComment-", "t3-rpc-pullRequests.submitReview-", "t3-rpc-pullRequests.replyToThread-", "t3-rpc-pullRequests.setThreadResolution-", "t3-rpc-pullRequests.setReaction-", "t3-rpc-pullRequests.runAction-", "t3-rpc-server.getProcessResourceHistory-", "t3-rpc-server.getResourceTelemetryHistory-", "t3-rpc-server.signalProcess-", "t3-rpc-server.checkOtlpEndpoint-", "t3-rpc-server.getNpmRelease-", "t3-rpc-server.getUsageSummary-", "t3-rpc-shell.openInEditor-", "t3-rpc-peerLinks.unlink-", "t3-rpc-peerLinks.link-", "t3-rpc-peerLinks.answerRequest-", "t3-rpc-preview.clearProfile-", "t3-rpc-preview.stopLocalServer-", "t3-rpc-preview.reportProfiles-", "t3-rpc-preview.reportStatus-", "t3-rpc-preview.resize-", "t3-rpc-preview.adjust-", "t3-rpc-preview.open-", "t3-rpc-scheduledTasks.listWebhookDeliveries-", "t3-rpc-scheduledTasks.getWebhookDelivery-", "t3-rpc-attachments.delete-", "t3-rpc-mcpApps.", "t3-rpc-assets.", "t3-command-thread.auto-settle-", "t3-command-thread.model-selection.set-", "t3-command-thread.pull-request.link-", "t3-command-thread.pull-request.unlink-", "t3-command-thread.pull-request.watch-", "t3-command-thread.pull-request-link.sync-", "t3-command-thread.create-", "t3-command-thread.fork-", "t3-command-thread.merge-back-", "t3-command-thread.tag.set-", "t3-command-thread.settle-", "t3-rpc-server.removeKeybinding-", "t3-rpc-server.upsertKeybinding-", "t3-rpc-server.reportClientActivity-", "t3-rpc-server.listTextGeneratorModels-", "t3-rpc-server.prepareTriage-", "t3-rpc-server.updateSettings-", "t3-rpc-orchestration.", "t3-rpc-device.", "t3-command-provider.switch-", "t3-rpc-provider.consumeResetCredit-", "t3-rpc-provider.uploadFeedback-", "t3-rpc-provider.auth.removeMethod-"];

/// Tags the router does not know yet (`scopes.rs` lacks them), so their requests never reach a decoder.
const NOT_ROUTED: [&str; 1] = ["t3-rpc-sourceControl.star-"];

/// The ids replayed on four threads, each against routers of its own.
fn found(set: &mimic_core::ports::CassetteSet, allow: &mimic_core::allowlist::Allowlist, ids: &[String]) -> Vec<gaps::Found> {
    let chunk = ids.len().div_ceil(4).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = ids.chunks(chunk).map(|part| scope.spawn(move || found_in_turn(set, allow, part))).collect();
        workers.into_iter().flat_map(|w| w.join().expect("a replay thread")).collect()
    })
}

fn found_in_turn(set: &mimic_core::ports::CassetteSet, allow: &mimic_core::allowlist::Allowlist, ids: &[String]) -> Vec<gaps::Found> {
    let mut out = Vec::new();
    for id in ids {
        let h = std::rc::Rc::new(Harness::start());
        *h.agent.root.lock().unwrap() = Some("/conformance".into());
        let (scopes, paired) = (granted_scopes(set), h.clone());
        let pair = std::rc::Rc::new(move || paired.pair(PairingRequest { scopes: scopes.clone(), ..PairingRequest::default() }).code);
        let mut candidate = driver(h.addr, "/conformance/workspace".to_string(), pair, prelude_of(set), true);
        let verdict = replay(set, &mut candidate, allow, &t3_diff_options(), Some(std::slice::from_ref(id))).expect("the replay runs");
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

/// Whether the real server answered the case with the decoder's defect, whatever the case was called (a "valid" input can miss a pattern).
fn decoded_to_a_defect(i: &mimic_core::cassette::Interaction) -> bool {
    i.steps.iter().filter_map(|s| s.payload["exit"]["cause"].as_array()).flatten().any(|c| c["_tag"] == "Die" && c["defect"].as_str().is_some_and(|d| d.starts_with("Missing key") || d.starts_with("Expected ")))
}

fn ids_of(set: &mimic_core::ports::CassetteSet, prefixes: &[String]) -> Vec<String> {
    set.cassette.interactions.iter().filter(|i| decoded_to_a_defect(i)).map(|i| i.id.clone()).filter(|id| prefixes.iter().any(|p| id.contains(p.as_str())) && !NOT_ROUTED.iter().any(|n| id.contains(n))).collect()
}

fn listed_for(ids: &[String]) -> std::collections::BTreeMap<String, Vec<String>> {
    let path = format!("{DIR}/known-gaps.json");
    let mut listed = gaps::listed(&serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()).unwrap();
    listed.retain(|id, _| ids.contains(id));
    listed
}

#[test]
fn the_payloads_the_decoder_refuses_in_the_decoded_groups_are_the_defects_the_real_server_answered_but_for_the_listed_gaps() {
    // Given the crawl set of the pinned server and the groups decoded here
    let (set, allow) = (load_set(DIR), load_allowlist(DIR));
    let prefixes: Vec<String> = GROUPS.iter().map(|g| g.to_string()).collect();
    let ids = ids_of(&set, &prefixes);
    assert!(ids.len() > 200, "{}", ids.len());
    // When each of those cases is replayed against the router
    let differences = found(&set, &allow, &ids);
    // Then what differs from the real server's answer is exactly what known-gaps.json lists for them (the nested profiles the chatgpt RPCs decode)
    let (new, stale) = gaps::compare(&differences, &listed_for(&ids));
    assert!((new.is_empty(), stale.is_empty()) == (true, true), "not listed: {new:#?}\nlisted and no longer so: {stale:#?}");
}

/// A developer's slice: what differs for the decoder-defect cases of `CRAWL_PREFIX` against `known-gaps.json` (any other case answers from the
/// agent API, which the chain replay settles).
#[test]
fn a_slice_named_by_crawl_prefix_is_compared_with_the_known_gaps() {
    let Ok(prefix) = std::env::var("CRAWL_PREFIX") else { return };
    let (set, allow) = (load_set(DIR), load_allowlist(DIR));
    let ids = ids_of(&set, &prefix.split(',').map(String::from).collect::<Vec<_>>());
    let differences = found(&set, &allow, &ids);
    let path = format!("{DIR}/known-gaps.json");
    let (new, stale) = gaps::compare(&differences, &listed_for(&ids));
    eprintln!("found:\n{differences:#?}");
    eprintln!("slice of {} ids: {} differ\nnot listed:\n{}\nlisted and no longer differing as listed:\n{stale:?}", ids.len(), differences.len(), new.join("\n"));
    if std::env::var("CRAWL_WRITE_GAPS").is_ok() {
        let all: std::collections::BTreeMap<String, Vec<String>> = gaps::listed(&serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()).unwrap().into_iter().filter(|(id, _)| !ids.contains(id)).chain(differences.iter().map(|f| (f.id.clone(), f.lines.clone()))).collect();
        let merged: Vec<gaps::Found> = all.into_iter().map(|(id, lines)| gaps::Found { id, lines }).collect();
        std::fs::write(&path, serde_json::to_string_pretty(&gaps::file(&merged)).unwrap() + "\n").unwrap();
    }
}
