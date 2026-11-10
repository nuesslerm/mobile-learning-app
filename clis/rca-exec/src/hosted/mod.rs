//! The pure side of the hosted family: which programs a request starts (`plan`) and what their output means (`shape`). The adapter
//! (`adapters::hosted`) runs the programs; nothing here touches a file, a process or the environment.
pub mod plan;
pub mod shape;
pub mod time;

/// The environment a hosted CLI starts with: the variables of `INHERITED_ENV` from the executor's own environment (its configuration
/// directories among them, so its own sign-in is used), what keeps the CLI from prompting, paging or phoning home, and `extra`. A token
/// variable of the executor's environment is not passed on.
pub fn child_env(parent: impl Iterator<Item = (String, String)>, extra: &[(String, String)]) -> Vec<(String, String)> {
    const FIXED: [(&str, &str); 11] = [
        ("GH_PROMPT_DISABLED", "1"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
        ("GH_SPINNER_DISABLED", "1"),
        ("GH_PAGER", "cat"),
        ("PAGER", "cat"),
        ("NO_COLOR", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GLAB_SEND_TELEMETRY", "false"),
        ("GLAB_CHECK_UPDATE", "false"),
        ("NO_PROMPT", "1"),
        ("GIT_PAGER", "cat"),
    ];
    let mut env: Vec<(String, String)> = parent.filter(|(k, _)| exec_hosted::INHERITED_ENV.contains(&k.as_str())).collect();
    env.extend(FIXED.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    env.extend(extra.iter().cloned());
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_an_executor_environment_with_tokens_when_a_cli_environment_is_made_then_only_its_own_configuration_and_the_network_settings_pass() {
        // Given an environment with the machine's own config directory, a proxy, and every kind of secret a machine may hold
        let parent = [("PATH", "/bin"), ("HOME", "/home/x"), ("GH_CONFIG_DIR", "/cfg/gh"), ("GLAB_CONFIG_DIR", "/cfg/glab"), ("HTTPS_PROXY", "http://p:1"), ("GH_TOKEN", "t1"), ("GITHUB_TOKEN", "t2"), ("GITLAB_TOKEN", "t3"), ("ANTHROPIC_API_KEY", "t4"), ("RCA_SERVICE_SECRET", "t5"), ("GH_HOST", "evil.example")]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()));
        // When the CLI's environment is made for an instance
        let env = child_env(parent, &[("GH_HOST".into(), "git.example.com".into())]);
        let get = |k: &str| env.iter().filter(|(n, _)| n == k).map(|(_, v)| v.as_str()).collect::<Vec<_>>();
        // Then the config directories and the proxy pass, no secret does, and the instance named by the request is the only GH_HOST
        assert_eq!((get("GH_CONFIG_DIR"), get("GLAB_CONFIG_DIR"), get("HTTPS_PROXY"), get("HOME")), (vec!["/cfg/gh"], vec!["/cfg/glab"], vec!["http://p:1"], vec!["/home/x"]));
        for secret in ["GH_TOKEN", "GITHUB_TOKEN", "GITLAB_TOKEN", "ANTHROPIC_API_KEY", "RCA_SERVICE_SECRET"] {
            assert!(get(secret).is_empty(), "{secret} reached the CLI");
        }
        assert_eq!(get("GH_HOST"), vec!["git.example.com"]);
        // And the CLI is kept from prompting
        assert_eq!(get("GH_PROMPT_DISABLED"), vec!["1"]);
    }
}
