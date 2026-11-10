//! The VM state backup bucket (`infra/oracle-cloud/vm-state.tf`), one restic repository: the storage guard counts it and never deletes from
//! it, it is versioned and private, no lifecycle rule expires its live objects, and its budget is twice the sum of the `maxBytes` the state
//! volumes declare in vm-apps.json and fits beside the cache under the tenancy cap.
use blob_cache::domain::Budget;
use blob_cache::storage_policy::{candidates, Pins, Role};
use blob_cache::ports::ObjectInfo;
use std::path::PathBuf;

fn text(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../infra/oracle-cloud").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is unreadable: {e}", path.display()))
}

#[test]
fn given_a_full_tenancy_when_the_guard_chooses_what_to_evict_then_no_backup_is_ever_a_candidate() {
    // Given the state bucket as terraform names it, holding backups far older than any cache entry
    let bucket = "mobile-learning-vm-state";
    let old = |name: &str| ObjectInfo { name: name.into(), bytes: 90_000_000, created_ms: 0, etag: None };
    let listed = vec![(bucket.to_string(), vec![old("data/ab/abcdef0123456789"), old("snapshots/0123456789abcdef"), old("verification/test-runs/x/bundle.json")])];

    // When the guard asks which objects may go
    let found = candidates(&listed, 1_800_000_000_000, &Pins::none());

    // Then the bucket is counted under its own role and nothing in it is a candidate, whatever its name or age
    assert_eq!(Role::of(bucket), Role::State);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn given_the_terraform_when_the_bucket_is_read_then_it_is_versioned_private_and_never_expires_a_live_object_of_the_repository() {
    let tf = text("vm-state.tf");
    assert!(tf.contains("name           = \"${var.name_prefix}-vm-state\"") && tf.contains("versioning     = \"Enabled\"") && tf.contains("access_type    = \"NoPublicAccess\""));
    assert!(!tf.contains("oci_objectstorage_preauthrequest"), "a PAR would make the bucket readable without the admin key");
    for rule in ["target      = \"previous-object-versions\"", "target      = \"multipart-uploads\""] {
        assert!(tf.contains(rule), "{rule}");
    }
    // a rule on current objects would delete the packs of snapshots that are still kept
    assert!(!tf.contains("target      = \"objects\""), "a rule that expires current objects destroys the repository's old packs");
}

fn tf_bytes(tf: &str, name: &str) -> u64 {
    tf.lines().find_map(|l| l.trim().strip_prefix(name).filter(|r| r.trim_start().starts_with('='))).and_then(|r| r.trim_start_matches([' ', '=']).trim().parse().ok()).unwrap_or_else(|| panic!("{name} in vm-state.tf"))
}

#[test]
fn given_the_budget_when_the_terraform_caps_are_read_then_both_name_the_same_bytes_and_fit_beside_the_cache() {
    let tf = text("vm-state.tf");
    let budget = Budget::committed();
    assert_eq!(budget.vm_state_bytes, tf_bytes(&tf, "vm_state_budget_bytes"), "budget.json vmStateBytes drifted from vm-state.tf");
    assert_eq!(budget.vm_state_repository_bytes, tf_bytes(&tf, "vm_state_repository_bytes"), "budget.json vmStateRepositoryBytes drifted from vm-state.tf");
    assert!(budget.stored_bytes + budget.vm_state_bytes < budget.tenancy_stored_bytes, "the cache and the state bucket must leave room for the evidence bucket");
}

#[test]
fn given_the_volumes_vm_apps_declares_when_their_caps_are_summed_then_the_repository_budget_is_twice_and_the_bucket_budget_four_times_the_sum() {
    // Given the maxBytes of every state volume, read from the file the backup reads
    let apps: serde_json::Value = serde_json::from_str(&text("vm-apps.json")).unwrap();
    let caps: Vec<u64> = apps.as_array().unwrap().iter().flat_map(|a| a["stateVolumes"].as_array().into_iter().flatten()).map(|v| v["maxBytes"].as_u64().expect("every state volume declares its maxBytes")).collect();
    assert!(caps.len() >= 4, "{caps:?}");
    let sum: u64 = caps.iter().sum();

    // When the budgets terraform and the guard hold are compared with the sum
    let tf = text("vm-state.tf");

    // Then all of them agree: the live repository may hold twice the sum and the bucket, with the previous versions a prune leaves behind, four times
    assert_eq!(tf_bytes(&tf, "vm_state_repository_bytes"), 2 * sum);
    assert_eq!(tf_bytes(&tf, "vm_state_budget_bytes"), 4 * sum);
    assert_eq!(Budget::committed().vm_state_repository_bytes, 2 * sum);
    assert_eq!(Budget::committed().vm_state_bytes, 4 * sum);
}

#[test]
fn given_a_prune_that_deletes_packs_when_the_previous_versions_are_kept_then_they_are_kept_for_one_day_and_the_bucket_budget_has_room_for_them() {
    // Given the lifecycle rule for previous object versions and the budgets
    let tf = text("vm-state.tf");
    let rule = tf.split("target      = \"previous-object-versions\"").next().unwrap().rsplit("rules {").next().unwrap().to_string();
    let days: u64 = rule.lines().find_map(|l| l.trim().strip_prefix("time_amount")).and_then(|r| r.trim_start_matches([' ', '=']).trim().parse().ok()).expect("time_amount of the previous-versions rule");

    // When a prune deletes everything the live repository holds, Then those packs stay billed for at most a day (the shortest rule OCI takes) and the bucket budget leaves them a whole repository
    assert_eq!(days, 1, "deleted packs stay billed as previous versions for this many days");
    assert!(tf_bytes(&tf, "vm_state_budget_bytes") >= 2 * tf_bytes(&tf, "vm_state_repository_bytes"));
}
