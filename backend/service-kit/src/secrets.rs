//! The registered secrets (`data/registry.json`, kept equal to `clis/secrets/src/data/registry.json` by a flow test: a published crate holds only its own files): which environment variables the registry names, so the ones a
//! deployed container was not given can be asked of the vault through `wf`, and so the log scrubber knows every secret value by what it
//! is, not only by what its variable is called.

use std::sync::OnceLock;
use std::time::Duration;

const REGISTRY: &str = include_str!("../data/registry.json");

/// The environment variable of every registry entry.
pub fn env_vars() -> &'static [String] {
    static VARS: OnceLock<Vec<String>> = OnceLock::new();
    VARS.get_or_init(|| {
        let parsed: serde_json::Value = serde_json::from_str(REGISTRY).unwrap_or_default();
        let mut vars: Vec<String> = parsed.as_object().into_iter().flat_map(|entries| entries.values()).filter_map(|e| e.get("envVar").and_then(|v| v.as_str()).map(str::to_string)).collect();
        vars.sort();
        vars.dedup();
        vars
    })
}

/// `NAME="value"` lines as the vault command writes them; a line that is not one, or has no value, is skipped.
fn parse_env(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=').filter(|(name, _)| !name.is_empty())?;
            let value: String = serde_json::from_str(value).ok()?;
            (!value.is_empty()).then(|| (name.to_string(), value))
        })
        .collect()
}

fn is_unset(name: &str) -> bool {
    std::env::var(name).map_or(true, |v| v.is_empty())
}

/// Fills the environment from the vault for every registered variable that is not set. Best effort by construction: a deployed
/// container gets its environment before this runs, so when nothing is missing no process is started, and an unusable vault warns once
/// on stderr and the existing environment stands.
pub async fn load() {
    let missing: Vec<&str> = env_vars().iter().map(String::as_str).filter(|name| is_unset(name)).collect();
    if missing.is_empty() {
        return;
    }
    let tool = std::env::var("WF_BIN").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "wf".into());
    let keys = missing.join(",");
    let run = tokio::process::Command::new(&tool).args(["secret", "env", "--keys", &keys]).stdin(std::process::Stdio::null()).kill_on_drop(true).output();
    let why = match tokio::time::timeout(Duration::from_secs(60), run).await {
        Ok(Ok(out)) if out.status.success() => {
            for (name, value) in parse_env(&String::from_utf8_lossy(&out.stdout)) {
                if is_unset(&name) {
                    std::env::set_var(name, value);
                }
            }
            return;
        }
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if stderr.is_empty() { format!("exit {}", out.status) } else { stderr }
        }
        Ok(Err(error)) => error.to_string(),
        Err(_) => "timed out after 60 s".to_string(),
    };
    eprintln!("[secrets] the vault is unavailable through \"{tool}\", continuing with the existing environment — {why}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_registry_when_its_variables_are_listed_then_they_are_sorted_and_include_the_ones_the_services_read() {
        let vars = env_vars();

        assert!(vars.windows(2).all(|w| w[0] < w[1]));
        for needed in ["CHAT_AGENT_API_SECRET", "BETTERSTACK_OTLP_TOKEN", "HEALTH_REPORTS_ORDS_PASSWORD", "LEETCODE_BACKEND_ADMIN_SECRET", "TELEGRAM_NOTIFY_SECRET"] {
            assert!(vars.iter().any(|v| v == needed), "{needed}");
        }
    }

    #[test]
    fn given_vault_output_when_parsed_then_only_well_formed_non_empty_lines_are_taken() {
        let parsed = parse_env("A=\"one\"\nB=\"\"\nnot a line\n=\"x\"\nC=\"two \\\"quoted\\\"\"\nD=notjson\n");

        assert_eq!(parsed, vec![("A".to_string(), "one".to_string()), ("C".to_string(), "two \"quoted\"".to_string())]);
    }
}
