//! The bucket in `infra/oracle-cloud/build-cache.tf` expires each kind after the days `data/budget.json` declares, and stays private.
use blob_cache::domain::Kind;
use std::collections::BTreeMap;
use std::path::PathBuf;

type Fields = BTreeMap<String, String>;

fn terraform() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../infra/oracle-cloud/build-cache.tf");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is unreadable: {e}", path.display()))
}

/// The `key = value` lines of every block that opens with `header`, quotes stripped; nested blocks fold into the same map.
fn blocks(text: &str, header: &str) -> Vec<Fields> {
    let mut found = Vec::new();
    let mut open: Option<(Fields, i32)> = None;
    for line in text.lines().map(str::trim) {
        if let Some((fields, depth)) = open.as_mut() {
            *depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
            if let Some((key, value)) = line.split_once('=') {
                fields.insert(key.trim().to_string(), value.trim().trim_matches('"').to_string());
            }
            if *depth == 0 {
                found.push(open.take().map(|(f, _)| f).unwrap_or_default());
            }
        } else if line == header {
            open = Some((Fields::new(), 1));
        }
    }
    found
}

fn resource(kind: &str, name: &str) -> String {
    format!("resource \"{kind}\" \"{name}\" {{")
}

fn prefixes(rule: &Fields) -> Vec<String> {
    let list = rule.get("inclusion_prefixes").cloned().unwrap_or_default();
    list.trim_matches(|c| c == '[' || c == ']').split(',').map(|p| p.trim().trim_matches('"').to_string()).filter(|p| !p.is_empty()).collect()
}

#[test]
fn given_the_committed_budget_when_the_lifecycle_rules_are_read_then_each_kind_expires_after_its_ttl_days_and_nothing_else_is_deleted() {
    let rules = blocks(&terraform(), "rules {");

    for kind in Kind::ALL {
        let prefix = format!("{}/", kind.prefix());
        let expiring: Vec<&Fields> = rules
            .iter()
            .filter(|r| r.get("action").map(String::as_str) == Some("DELETE") && r.get("target").map(String::as_str) == Some("objects") && prefixes(r) == [prefix.clone()])
            .collect();
        assert_eq!(expiring.len(), 1, "build-cache.tf needs exactly one DELETE rule for {prefix}, found {}", expiring.len());
        let rule = expiring[0];
        assert_eq!(rule.get("is_enabled").map(String::as_str), Some("true"), "the {prefix} rule is disabled");
        assert_eq!(rule.get("time_unit").map(String::as_str), Some("DAYS"), "the {prefix} rule is not in days");
        assert_eq!(
            rule.get("time_amount").map(String::as_str),
            Some(kind.ttl_days().to_string().as_str()),
            "build-cache.tf expires {prefix} after {:?} days but clis/blob-cache/data/budget.json says {}",
            rule.get("time_amount"),
            kind.ttl_days()
        );
    }

    let kind_prefixes: Vec<String> = Kind::ALL.iter().map(|k| format!("{}/", k.prefix())).collect();
    for rule in rules.iter().filter(|r| r.get("action").map(String::as_str) == Some("DELETE")) {
        assert!(prefixes(rule).iter().all(|p| kind_prefixes.contains(p)), "a DELETE rule reaches a prefix that is not a kind: {:?}", prefixes(rule));
    }
}

#[test]
fn given_the_bucket_when_uploads_stall_then_multipart_uploads_are_aborted_after_one_day() {
    let rules = blocks(&terraform(), "rules {");

    let abort = rules.iter().find(|r| r.get("action").map(String::as_str) == Some("ABORT")).expect("build-cache.tf has no ABORT rule for multipart uploads");

    assert_eq!(abort.get("target").map(String::as_str), Some("multipart-uploads"));
    assert_eq!(abort.get("time_amount").map(String::as_str), Some("1"));
    assert_eq!(abort.get("time_unit").map(String::as_str), Some("DAYS"));
    assert_eq!(abort.get("is_enabled").map(String::as_str), Some("true"));
}

#[test]
fn given_the_bucket_when_its_access_is_read_then_it_is_private_and_the_two_pars_can_neither_list_nor_delete() {
    let text = terraform();

    let bucket = &blocks(&text, &resource("oci_objectstorage_bucket", "build_cache"))[0];
    let read = &blocks(&text, &resource("oci_objectstorage_preauthrequest", "build_cache_read"))[0];
    let write = &blocks(&text, &resource("oci_objectstorage_preauthrequest", "build_cache_write"))[0];

    assert_eq!(bucket.get("access_type").map(String::as_str), Some("NoPublicAccess"));
    assert_eq!(bucket.get("versioning").map(String::as_str), Some("Disabled"));
    assert_eq!(read.get("access_type").map(String::as_str), Some("AnyObjectRead"));
    // Deny is the default; setting it makes the provider plan a replacement on every run (the API returns it empty).
    assert!(read.get("bucket_listing_action").is_none(), "listing stays denied by default; an explicit action never converges");
    assert_eq!(write.get("access_type").map(String::as_str), Some("AnyObjectWrite"));
    assert!(read.get("object_name").is_none() && write.get("object_name").is_none(), "a PAR names an object: the cache writes under several prefixes");
    for output in ["build_cache_read_par_url", "build_cache_write_par_url"] {
        let block = &blocks(&text, &format!("output \"{output}\" {{"))[0];
        assert_eq!(block.get("sensitive").map(String::as_str), Some("true"), "{output} is not sensitive");
    }
}

#[test]
fn given_the_tenancy_quota_in_terraform_when_the_guard_reads_its_cap_then_both_name_the_same_number_of_bytes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../infra/oracle-cloud/always-free-quota.tf");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is unreadable: {e}", path.display()));
    let variable = &blocks(&text, "variable \"always_free_object_storage_cap_gb\" {")[0];
    let gb: u64 = variable.get("default").and_then(|v| v.parse().ok()).expect("a numeric default for the object storage cap");
    assert!(text.contains("storage-bytes to ${var.always_free_object_storage_cap_gb * 1000000000} in tenancy"), "the quota statement no longer multiplies the variable by 1e9");

    assert_eq!(blob_cache::domain::Budget::committed().tenancy_stored_bytes, gb * 1_000_000_000, "clis/blob-cache/data/budget.json tenancyStoredBytes drifted from always-free-quota.tf");
}
