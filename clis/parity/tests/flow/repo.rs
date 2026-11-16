//! The repository's own tables: what CI keeps honest. With the references checked out at their pins the check rescans them;
//! without, it rebuilds from the checked-in method lists.
use super::support::*;
use std::path::Path;

#[test]
fn the_repos_own_coverage_tables_pass_the_check_for_both_apps() {
    // Given the repository root
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    for app in ["t3", "orca"] {
        // When its table is checked
        let run = coverage(&root, &["--app", app, "--check"]);
        // Then nothing is stale, unassigned or untargeted, and every served row is one the checked-in evidence names and every row has one state
        assert_eq!(run.code, 0, "{app}: {}", run.stdout);
        assert_eq!(run.json["problems"], json!([]), "{app}");
        let evidence = root.join(format!("docs/plans/rca-parity/coverage/{app}-evidence.json"));
        let proved = std::fs::read_to_string(evidence).ok().map(|t| serde_json::from_str::<Value>(&t).unwrap()["rows"].as_object().unwrap().len()).unwrap_or(0);
        assert_eq!(run.json["summary"]["served"].as_u64(), Some(proved as u64), "{app}");
        let count = |k: &str| run.json["summary"][k].as_u64().unwrap();
        assert_eq!(count("rows"), count("served") + count("pending") + count("notOffered") + count("excluded") + count("flagged"), "{app}");
    }
}

#[test]
fn the_repos_own_oracle_tables_are_current_and_every_served_row_has_a_class() {
    // Given the repository root
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    for app in ["t3", "orca"] {
        // When the by-oracle table is checked
        let run = coverage(&root, &["--app", app, "--by-oracle", "--check"]);
        // Then it is what the generator builds and its classes add up to the served rows
        assert_eq!(run.code, 0, "{app}: {}", run.stdout);
        let sum: u64 = ["a", "b", "c", "d"].iter().map(|c| run.json["counts"][c].as_u64().unwrap()).sum();
        assert_eq!(Some(sum), run.json["served"].as_u64(), "{app}");
    }
}
