use crate::support::*;
use serde_json::Value;
use std::collections::BTreeMap;

fn scan() -> BTreeMap<String, Value> {
    let path = repo_root().join("docs/plans/rca-parity/coverage/orca-methods.json");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    doc["rows"].as_array().unwrap().iter().filter(|r| r["kind"] == "rpc").map(|r| (r["name"].as_str().unwrap().to_string(), r.clone())).collect()
}

#[test]
fn given_the_assembled_table_when_read_against_the_reference_scan_then_every_served_method_is_a_phone_row_of_the_right_kind() {
    // Given the table the server is built with and the scan of the pinned reference
    let h = harness();
    let table = h.host.api();
    let scan = scan();
    let methods = table["methods"].as_array().unwrap();
    // Then there are served methods, each is a row the phone calls, and a stream is exactly a subscription row
    assert!(methods.iter().any(|m| m["state"] == "served"));
    for m in methods {
        let name = m["method"].as_str().unwrap();
        let row = scan.get(name).unwrap_or_else(|| panic!("{name} is no method of the pinned reference"));
        assert_eq!(row["phone"], true, "{name} is not a method the phone calls");
        if m["state"] == "served" {
            assert_eq!(m["streaming"] == true, row["subscription"] == true, "{name}: streaming does not match the scan");
        }
    }
}

#[test]
fn given_the_table_when_listed_then_no_method_is_claimed_twice_and_a_double_claim_is_refused() {
    // Given the table as printed
    let h = harness();
    let table = h.host.api();
    let mut names: Vec<&str> = table["methods"].as_array().unwrap().iter().map(|m| m["method"].as_str().unwrap()).collect();
    let before = names.len();
    names.dedup();
    // Then each name is listed once
    assert_eq!(names.len(), before);
    // And two areas that claim one method do not assemble
    let twice: Vec<Box<dyn orca_core::Area>> = vec![Box::new(orca_map_settings::SettingsArea), Box::new(orca_map_settings::SettingsArea)];
    let refused = orca_core::table::Table::assemble(&twice).unwrap_err();
    assert!(refused.contains("declared twice"), "{refused}");
}

#[test]
fn given_the_owner_decisions_when_compared_with_the_table_then_the_not_offered_rows_of_this_milestone_are_exactly_the_registered_ones() {
    // Given the decisions file and the table
    let h = harness();
    let table = h.host.api();
    let path = repo_root().join("docs/plans/rca-parity/coverage/orca-decisions.json");
    let decisions: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let decided: Vec<&str> = decisions["decisions"].as_object().unwrap().iter().filter(|(_, d)| d["state"] == "not-offered" && matches!(d["decidedBy"].as_str(), Some("M5" | "M6" | "M7"))).filter_map(|(id, _)| id.strip_prefix("rpc:")).collect();
    let registered: Vec<&str> = table["methods"].as_array().unwrap().iter().filter(|m| m["state"] == "not-offered").map(|m| m["method"].as_str().unwrap()).collect();
    // Then the two lists are the same set, and each decision names a reason the table repeats
    assert!(!decided.is_empty());
    let mut a = decided.clone();
    let mut b = registered.clone();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b);
    for m in table["methods"].as_array().unwrap().iter().filter(|m| m["state"] == "not-offered") {
        assert_eq!(m["reason"], decisions["decisions"][format!("rpc:{}", m["method"].as_str().unwrap())]["reason"]);
    }
}

#[test]
fn given_a_not_offered_method_when_called_then_the_answer_is_not_offered_with_the_reason_and_not_unknown() {
    // Given a paired phone
    let h = harness();
    let mut phone = paired(&h);
    // When it calls a row the owner has not offered, and a method nobody knows
    let refused = phone.call("pairing.provisionRelay", json!({}));
    let unknown = phone.call("pairing.nothing", json!({}));
    // Then the first is not_offered with the decision's reason and who decided, the second method_not_found
    assert_eq!((refused["ok"].clone(), refused["error"]["code"].clone()), (json!(false), json!("not_offered")));
    assert!(refused["error"]["message"].as_str().unwrap().contains("relay service"));
    assert_eq!(refused["error"]["data"], json!({"decidedBy": "M5"}));
    assert_eq!(unknown["error"]["code"], "method_not_found");
}
