//! The Linear and Jira rows of a host with no tracker account connected, which is every host of this server (rca holds no tracker credential): the status
//! says nothing is connected, a list or a search is empty, a lookup finds nothing and a write says Linear is not connected. A row that needs an account to
//! answer (`linear.connect`, which asks Linear to check a key) is not mapped.
use orca_core::wire::{Failure, RUNTIME_ERROR};
use serde_json::{json, Value};

const JIRA_FILTERS: &[&str] = &["assigned", "reported", "all", "done"];

fn linear_status() -> Value {
    json!({"activeWorkspaceId": null, "connected": false, "credentialProtection": null, "selectedWorkspaceId": null, "viewer": null, "workspaces": []})
}

fn jira_status() -> Value {
    json!({"activeSiteId": null, "connected": false, "credentialProtection": null, "selectedSiteId": null, "sites": [], "supportsScopedTokens": true, "viewer": null})
}

/// The text the phone sent under `key`, or the refusal that names what is missing.
fn need(params: &Value, key: &str, said: &str) -> Result<(), Failure> {
    match params.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => Ok(()),
        _ => Err(Failure::invalid(said)),
    }
}

fn not_connected() -> Value {
    json!({"error": "Not connected to Linear", "errorCode": "linear.notConnected", "ok": false})
}

/// A Jira read that asks to be verified against a connected site has none to ask.
fn jira_read(params: &Value, found: Value) -> Result<Value, Failure> {
    if params.get("strict") == Some(&Value::Bool(true)) {
        return Err(Failure::new(RUNTIME_ERROR, "Verified Jira reads require one connected site ID."));
    }
    Ok(found)
}

pub(crate) fn call(method: &str, params: &Value) -> Option<Result<Value, Failure>> {
    Some(match method {
        "linear.status" => Ok(linear_status()),
        "linear.selectWorkspace" => need(params, "workspaceId", "Workspace ID is required").map(|_| linear_status()),
        "linear.listTeams" => Ok(json!([])),
        "linear.teamStates" => need(params, "teamId", "Team ID is required").map(|_| json!([])),
        "linear.searchIssues" => need(params, "query", "Missing query").map(|_| json!([])),
        "linear.issueComments" => need(params, "issueId", "Issue ID is required").map(|_| json!([])),
        "linear.getIssue" => need(params, "id", "Issue ID is required").map(|_| Value::Null),
        "linear.listIssues" if !(params.is_null() || params.is_object()) => Err(Failure::invalid("Invalid input")),
        "linear.listIssues" => Ok(json!({"items": []})),
        "linear.createIssue" => need(params, "teamId", "Team ID is required").map(|_| not_connected()),
        "linear.updateIssue" => need(params, "id", "Issue ID is required").map(|_| not_connected()),
        "linear.addIssueComment" => need(params, "issueId", "Issue ID is required").map(|_| not_connected()),
        "jira.status" => Ok(jira_status()),
        "jira.getIssue" => need(params, "key", "Issue key is required").and_then(|_| jira_read(params, Value::Null)),
        "jira.issueComments" => need(params, "key", "Issue key is required").and_then(|_| jira_read(params, json!([]))),
        "jira.searchIssues" => need(params, "jql", "Missing JQL").and_then(|_| jira_read(params, json!([]))),
        "jira.listIssues" => match params.get("filter") {
            Some(f) if !f.as_str().is_some_and(|f| JIRA_FILTERS.contains(&f)) => Err(Failure::invalid("Invalid option: expected one of \"assigned\"|\"reported\"|\"all\"|\"done\"")),
            _ => jira_read(params, json!([])),
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(method: &str, params: Value) -> Result<Value, Failure> {
        call(method, &params).unwrap()
    }

    #[test]
    fn given_no_tracker_account_when_the_phone_reads_then_nothing_is_connected_and_every_list_is_empty() {
        // Given a host with no Linear or Jira account, When the phone asks for the status, Then neither is connected
        assert_eq!(answer("linear.status", Value::Null).unwrap()["connected"], false);
        assert_eq!(answer("jira.status", json!({})).unwrap()["sites"], json!([]));
        // When it lists, searches or looks one up, Then lists are empty and a lookup finds nothing
        assert_eq!(answer("linear.searchIssues", json!({"query": "q"})).unwrap(), json!([]));
        assert_eq!(answer("jira.listIssues", Value::Null).unwrap(), json!([]));
        assert_eq!(answer("linear.getIssue", json!({"id": "a"})).unwrap(), Value::Null);
    }

    #[test]
    fn given_no_tracker_account_when_the_phone_writes_then_it_is_told_linear_is_not_connected_and_a_missing_id_is_refused_first() {
        assert_eq!(answer("linear.createIssue", json!({"teamId": "t", "title": "x"})).unwrap()["errorCode"], "linear.notConnected");
        assert_eq!(answer("linear.updateIssue", json!({"id": "i", "updates": {}})).unwrap()["ok"], false);
        assert_eq!(answer("linear.addIssueComment", json!({"issueId": 7, "body": "b"})).unwrap_err().message, "Issue ID is required");
        assert_eq!(answer("jira.searchIssues", json!({})).unwrap_err().message, "Missing JQL");
    }

    #[test]
    fn given_a_jira_read_that_must_be_verified_when_no_site_is_connected_then_it_is_refused_and_a_bad_filter_is_named() {
        assert_eq!(answer("jira.issueComments", json!({"key": "K-1", "strict": true})).unwrap_err().message, "Verified Jira reads require one connected site ID.");
        assert!(answer("jira.listIssues", json!({"filter": 7})).unwrap_err().message.starts_with("Invalid option"));
    }
}
