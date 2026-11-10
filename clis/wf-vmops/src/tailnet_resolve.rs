//! `wf vm net resolve <name>...`: the name each tailnet host answers to now. A name drifts to `<name>-1` .. `-5` while a stale
//! record holds it (vm-ops-api's resolveTailnetHost.ts resolves its backends the same way).
use wf_kit::args::parse;
use std::net::ToSocketAddrs;

const MAX_SUFFIX: u32 = 5;

fn candidates(base: &str) -> Vec<String> {
    std::iter::once(base.to_string()).chain((1..=MAX_SUFFIX).map(|i| format!("{base}-{i}"))).collect()
}

/// The first candidate that resolves at all.
fn first_resolvable(base: &str, resolves: &dyn Fn(&str) -> bool) -> Option<String> {
    candidates(base).into_iter().find(|c| resolves(c))
}

/// The name `base` answers to now, or none when no candidate resolves.
pub fn answering(base: &str) -> Option<String> {
    first_resolvable(base, &resolves)
}

/// `WF_TEST_TAILNET_NAMES` (comma-separated) stands in for the tailnet's DNS: exactly those names resolve.
fn resolves(host: &str) -> bool {
    if let Ok(names) = std::env::var("WF_TEST_TAILNET_NAMES") {
        return names.split(',').any(|n| n.trim() == host);
    }
    (host, 0).to_socket_addrs().is_ok_and(|mut a| a.next().is_some())
}

/// The tailnet name a bundle baked in as `host` has since moved to, or none: it is still the first answer for its base name, it is
/// no tailnet name (a dotted one), or nothing answers at all (a resolver with nothing to say proves no drift).
pub fn drifted(host: &str) -> Option<String> {
    drifted_by(host, &resolves)
}

fn drifted_by(host: &str, resolves: &dyn Fn(&str) -> bool) -> Option<String> {
    if host.contains('.') {
        return None;
    }
    let base = host.rsplit_once('-').filter(|(_, n)| n.parse::<u32>().is_ok_and(|n| (1..=MAX_SUFFIX).contains(&n))).map_or(host, |(base, _)| base);
    first_resolvable(base, resolves).filter(|now| now != host)
}

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse(args, &[], &[])?;
    if p.positional.is_empty() {
        return None;
    }
    let mut found = serde_json::Map::new();
    let mut missing = Vec::new();
    for base in &p.positional {
        let name = first_resolvable(base, &resolves);
        if name.is_none() {
            missing.push(base.as_str());
        }
        found.insert(base.clone(), serde_json::json!(name));
    }
    println!("{}", serde_json::Value::Object(found));
    if missing.is_empty() {
        return Some(0);
    }
    wf_kit::wf_err!("wf: no name resolves for {} (tried each with -1 to -{MAX_SUFFIX}); `wf vm net device list` shows the tailnet's records", missing.join(", "));
    Some(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_name_held_by_a_stale_record_when_resolved_then_the_first_suffix_that_answers_is_the_name() {
        // Given only the -2 and -4 suffixes answer
        let answers = |h: &str| h == "backend-prod-2" || h == "backend-prod-4";
        // When the base name is resolved, Then the first that answers wins
        assert_eq!(first_resolvable("backend-prod", &answers).as_deref(), Some("backend-prod-2"));
        // And the base name wins when it answers itself
        assert_eq!(first_resolvable("backend-prod-2", &answers).as_deref(), Some("backend-prod-2"));
        // And nothing past -5 is tried
        let far = |h: &str| h == "backend-prod-6";
        assert_eq!(first_resolvable("backend-prod", &far), None);
    }

    #[test]
    fn given_a_baked_name_when_the_tailnet_answers_otherwise_then_it_has_drifted_and_silence_proves_nothing() {
        let live = |names: &'static [&'static str]| move |h: &str| names.contains(&h);
        // Given the bundle baked backend-prod-1 and the tailnet now answers the base name, Then the name has moved to it
        assert_eq!(drifted_by("backend-prod-1", &live(&["backend-prod"])).as_deref(), Some("backend-prod"));
        // and the base name baked while only -2 answers has moved to -2
        assert_eq!(drifted_by("backend-prod", &live(&["backend-prod-2"])).as_deref(), Some("backend-prod-2"));
        // But a name that is still the first answer, a dotted host and a tailnet that answers nothing prove no drift
        assert_eq!(drifted_by("backend-prod-1", &live(&["backend-prod-1", "backend-prod-3"])), None);
        assert_eq!(drifted_by("example.com", &live(&["example.com-1"])), None);
        assert_eq!(drifted_by("backend-prod-1", &live(&[])), None);
        // and a name whose last word is no suffix is its own base
        assert_eq!(drifted_by("remote-chat-agent-prod", &live(&["remote-chat-agent-prod"])), None);
    }
}
