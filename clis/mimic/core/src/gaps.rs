//! The known gaps of a candidate against a recorded set: the interactions where it differs from the reference and does not deliberately
//! (the allowlist does not name them), so each one is an adapter bug with its row until it is fixed. The file groups the interactions that
//! differ in the same way (`mimic-known-gaps/2`), so a thousand rows that fail alike are one entry; the older flat form (`/1`, one
//! entry per interaction) still loads.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SCHEMA: &str = "mimic-known-gaps/2";

/// What a replay found of one interaction: its id, its row, and one line per difference (`path kind severity`, or `error <why>`).
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub id: String,
    pub lines: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Group {
    pub class: String,
    pub severity: String,
    pub diffs: Vec<String>,
    /// The interactions that differ this way (each id names its row in the cassette).
    pub ids: Vec<String>,
}

/// Interaction id to the differences it was listed for, from either schema.
pub fn listed(file: &Value) -> Result<BTreeMap<String, Vec<String>>, String> {
    let strings = |v: &Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>()).ok_or("diffs is no list".to_string());
    let mut out = BTreeMap::new();
    for g in file["gaps"].as_array().into_iter().flatten() {
        out.insert(g["id"].as_str().ok_or("a gap has no id")?.to_string(), strings(&g["diffs"])?);
    }
    for g in file["groups"].as_array().into_iter().flatten() {
        let diffs = strings(&g["diffs"])?;
        for pair in g["ids"].as_array().ok_or("a group has no ids")? {
            out.insert(pair.as_str().ok_or("an id is no string")?.to_string(), diffs.clone());
        }
    }
    Ok(out)
}

fn severity_rank(line: &str) -> u8 {
    match line.rsplit(' ').next() {
        Some("breaking") => 2,
        Some("drift") => 1,
        _ => 3,
    }
}

/// `steps.ws:s2c:3.[0].payload.x` to `steps.ws:s2c:*.[*].payload.x`: the position of a frame or an element is not what differs.
fn shape(path: &str) -> String {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    path.split('.').map(|seg| match seg.rsplit_once(':') {
        Some((head, tail)) if digits(tail) => format!("{head}:*"),
        _ if seg.starts_with('[') && digits(seg.trim_matches(|c| c == '[' || c == ']')) => "[*]".to_string(),
        _ => seg.to_string(),
    }).collect::<Vec<_>>().join(".")
}

/// The class of a difference, derived from what differs and not hand-written: the worst line's kind and the shape of its path.
pub fn class_of(lines: &[String]) -> (String, String) {
    let worst = lines.iter().min_by_key(|l| (std::cmp::Reverse(severity_rank(l)), l.to_string())).expect("a gap has a line");
    if let Some(why) = worst.strip_prefix("error ") {
        return ("driver-error".to_string(), format!("the candidate failed to answer: {}", why.chars().take(80).collect::<String>()));
    }
    let mut parts = worst.rsplitn(3, ' ');
    let (severity, kind, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    (severity.to_string(), format!("{kind} at {}", shape(path)))
}

/// The groups of the interactions that differ alike, most numerous first.
pub fn group(found: &[Found]) -> Vec<Group> {
    let mut by: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for f in found.iter().filter(|f| !f.lines.is_empty()) {
        by.entry(f.lines.clone()).or_default().push(f.id.clone());
    }
    let mut out: Vec<Group> = by.into_iter().map(|(diffs, mut ids)| {
        ids.sort();
        let (severity, class) = class_of(&diffs);
        Group { class, severity, diffs, ids }
    }).collect();
    out.sort_by(|a, b| b.ids.len().cmp(&a.ids.len()).then(a.diffs.cmp(&b.diffs)));
    out
}

pub fn file(found: &[Found]) -> Value {
    serde_json::json!({"schema": SCHEMA, "groups": group(found)})
}

/// Interactions whose differences are not as listed, and listed ones that no longer differ as listed.
pub fn compare(found: &[Found], listed: &BTreeMap<String, Vec<String>>) -> (Vec<String>, Vec<String>) {
    let now: BTreeMap<&str, &Vec<String>> = found.iter().filter(|f| !f.lines.is_empty()).map(|f| (f.id.as_str(), &f.lines)).collect();
    let new = now.iter().filter(|(k, v)| listed.get(**k) != Some(**v)).map(|(k, v)| format!("{k}: {v:?}")).collect();
    let stale = listed.iter().filter(|(k, v)| now.get(k.as_str()).is_none_or(|n| *n != *v)).map(|(k, _)| k.clone()).collect();
    (new, stale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(id: &str, lines: &[&str]) -> Found {
        Found { id: id.into(), lines: lines.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn interactions_that_differ_in_the_same_way_are_one_group_with_all_their_ids_and_the_file_reloads_to_what_was_found() {
        // Given three interactions, two of which differ at the same path of different frames, and one that agrees
        let all = [found("a", &["steps.ws:s2c:1.[0].payload.code changed drift"]), found("b", &["steps.ws:s2c:1.[0].payload.code changed drift"]), found("c", &[])];
        // When the findings are written as a known-gaps file and read back
        let reread = listed(&file(&all)).unwrap();
        // Then the file has one group of the two, and what it lists is what was found
        assert_eq!(group(&all).len(), 1);
        assert_eq!(group(&all)[0].ids.len(), 2);
        assert_eq!(compare(&all, &reread), (vec![], vec![]));
    }

    #[test]
    fn a_new_difference_and_a_gap_that_no_longer_differs_as_listed_are_both_reported() {
        // Given a listing made of one finding
        let before = [found("a", &["x changed drift"])];
        let listing = listed(&file(&before)).unwrap();
        // When the interaction differs differently, and another one differs that was not listed
        let after = [found("a", &["x changed breaking"]), found("z", &["y missing breaking"])];
        // Then the changed one is both new and stale, the unlisted one new
        let (new, stale) = compare(&after, &listing);
        assert_eq!((new.len(), stale), (2, vec!["a".to_string()]));
    }

    #[test]
    fn the_class_names_the_worst_difference_by_kind_and_the_shape_of_its_path_not_the_frame_it_was_in() {
        // Given differences at different frame and element positions
        let lines = vec!["steps.ws:s2c:7.[3].payload.exit changed drift".to_string(), "steps.ws:s2c:7.[3].payload.defect missing breaking".to_string()];
        // When classed
        let (severity, class) = class_of(&lines);
        // Then the breaking one names it, without the positions
        assert_eq!((severity.as_str(), class.as_str()), ("breaking", "missing at steps.ws:s2c:*.[*].payload.defect"));
    }

    #[test]
    fn the_flat_file_of_the_first_schema_still_loads() {
        let old = serde_json::json!({"schema": "mimic-known-gaps/1", "gaps": [{"id": "a", "diffs": ["p changed drift"]}]});
        assert_eq!(listed(&old).unwrap()["a"], vec!["p changed drift".to_string()]);
    }
}
