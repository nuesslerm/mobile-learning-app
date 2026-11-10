//! The environment a provider CLI runs with. Each provider keeps its login and settings in a directory of its own on the service's volume
//! (`/data/providers/<provider>`), named to the CLI through the variables it reads, and sees only the variables that are its own or shared and
//! not secret: a second provider cannot reach the first one's tokens, and no CLI sees the service's own secret.
use super::driver::Driver;

/// Where the providers' directories live on the service's volume.
pub const PROVIDERS_ROOT: &str = "/data/providers";

/// Variables every provider may inherit: the shell's basics, the locale and the directories of the tools a CLI shells out to.
const SHARED: [&str; 21] = [
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TERM", "TMPDIR", "TZ", "LANG", "LANGUAGE", "PNPM_HOME", "CARGO_HOME", "RUSTUP_HOME", "NPM_CONFIG_PREFIX", "VOLTA_HOME", "NVM_DIR", "GOPATH", "GOROOT", "SSL_CERT_FILE", "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
];
const SHARED_PREFIXES: [&str; 1] = ["LC_"];

/// The prefixes of the variables only `driver` inherits: the credentials and settings of its own provider and of the model APIs it reaches.
pub fn prefixes(driver: Driver) -> &'static [&'static str] {
    match driver {
        Driver::ClaudeAgent => &["ANTHROPIC_", "CLAUDE_CODE_"],
        Driver::Codex => &["OPENAI_", "CODEX_"],
        Driver::Opencode => &["OPENCODE_", "MINIMAX_", "KIMI_", "DEEPSEEK_", "OPENROUTER_"],
    }
}

/// The variables that point `driver`'s CLI at its own directory under `root`.
fn directories(driver: Driver, root: &str) -> Vec<(String, String)> {
    let own = format!("{root}/{}", driver.program());
    match driver {
        Driver::ClaudeAgent => vec![("CLAUDE_CONFIG_DIR".into(), own)],
        Driver::Codex => vec![("CODEX_HOME".into(), own)],
        Driver::Opencode => vec![("XDG_DATA_HOME".into(), format!("{own}/data")), ("XDG_CONFIG_HOME".into(), format!("{own}/config"))],
    }
}

fn inherits(driver: Driver, name: &str) -> bool {
    SHARED.contains(&name) || SHARED_PREFIXES.iter().chain(prefixes(driver)).any(|prefix| name.starts_with(prefix))
}

/// The environment of `driver`'s process given the one the service `inherited`, by name: the shared variables, the driver's own prefixed ones
/// and its directory variables under `root` (which replace any inherited value of the same name). Everything else is dropped.
pub fn provider_env(driver: Driver, root: &str, inherited: &[(String, String)]) -> Vec<(String, String)> {
    let own = directories(driver, root);
    let mut env: Vec<(String, String)> = inherited.iter().filter(|(name, _)| inherits(driver, name) && !own.iter().any(|(set, _)| set == name)).cloned().collect();
    env.extend(own);
    env.sort();
    env
}

#[cfg(test)]
mod tests {
    use super::super::driver::DRIVERS;
    use super::*;

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_string(), value.to_string())
    }

    fn value<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
        env.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The environment of a service whose every provider has a credential, beside the shared variables and a secret of its own.
    fn service_env() -> Vec<(String, String)> {
        let credentials = DRIVERS.iter().flat_map(|d| prefixes(*d).iter().map(move |p| pair(&format!("{p}KEY"), d.id())));
        let shared = [pair("PATH", "/usr/bin"), pair("HOME", "/home/svc"), pair("LANG", "C.UTF-8"), pair("LC_ALL", "C"), pair("PNPM_HOME", "/pnpm")];
        credentials.chain(shared).chain([pair("CHAT_AGENT_API_SECRET", "s3cret"), pair("GH_TOKEN", "ghp"), pair("UNRELATED", "x")]).collect()
    }

    #[test]
    fn given_each_provider_when_its_environment_is_built_then_it_points_the_cli_at_its_own_directory_on_the_volume() {
        // Given the providers' root on the volume and an environment that names no directory
        // When each provider's environment is built
        let of = |driver| provider_env(driver, PROVIDERS_ROOT, &[]);
        // Then Claude Code reads CLAUDE_CONFIG_DIR, Codex CODEX_HOME and opencode its XDG data and config homes, each under /data/providers
        assert_eq!(of(Driver::ClaudeAgent), [pair("CLAUDE_CONFIG_DIR", "/data/providers/claude")]);
        assert_eq!(of(Driver::Codex), [pair("CODEX_HOME", "/data/providers/codex")]);
        assert_eq!(of(Driver::Opencode), [pair("XDG_CONFIG_HOME", "/data/providers/opencode/config"), pair("XDG_DATA_HOME", "/data/providers/opencode/data")]);
        // And another root moves them all
        assert_eq!(value(&provider_env(Driver::Codex, "/x", &[]), "CODEX_HOME"), Some("/x/codex"));
    }

    #[test]
    fn given_an_environment_with_every_providers_credentials_when_built_for_one_then_only_its_own_survive() {
        let inherited = service_env();
        for driver in DRIVERS {
            // When the environment is built for one provider
            let env = provider_env(driver, PROVIDERS_ROOT, &inherited);
            // Then its credentials are there, and no other provider's is
            for other in DRIVERS {
                for prefix in prefixes(other) {
                    let name = format!("{prefix}KEY");
                    let seen = value(&env, &name);
                    assert_eq!(seen.is_some(), other == driver, "{} sees {name}", driver.id());
                }
            }
        }
    }

    #[test]
    fn given_the_services_own_secret_and_unrelated_variables_when_built_then_no_provider_sees_them_and_the_shared_ones_stay() {
        for driver in DRIVERS {
            // Given/When the environment of each provider is built from the service's
            let env = provider_env(driver, PROVIDERS_ROOT, &service_env());
            // Then neither the service secret, a GitHub token nor an unrelated variable is passed on, under any name or value
            for dropped in ["CHAT_AGENT_API_SECRET", "GH_TOKEN", "UNRELATED"] {
                assert_eq!(value(&env, dropped), None, "{} sees {dropped}", driver.id());
            }
            assert!(env.iter().all(|(_, v)| v != "s3cret" && v != "ghp"), "{}", driver.id());
            // And the path, home, locale and tool directories are
            for (name, want) in [("PATH", "/usr/bin"), ("HOME", "/home/svc"), ("LANG", "C.UTF-8"), ("LC_ALL", "C"), ("PNPM_HOME", "/pnpm")] {
                assert_eq!(value(&env, name), Some(want), "{} lacks {name}", driver.id());
            }
        }
    }

    #[test]
    fn given_the_service_with_its_own_provider_directories_when_built_then_the_providers_directory_replaces_them_and_another_providers_are_dropped() {
        // Given a service whose environment already names a Codex home, a Claude config directory and an XDG config home
        let inherited = [pair("CODEX_HOME", "/root/.codex"), pair("CLAUDE_CONFIG_DIR", "/root/.claude"), pair("XDG_CONFIG_HOME", "/root/.config")];
        // When each provider's environment is built
        let codex = provider_env(Driver::Codex, PROVIDERS_ROOT, &inherited);
        let claude = provider_env(Driver::ClaudeAgent, PROVIDERS_ROOT, &inherited);
        let opencode = provider_env(Driver::Opencode, PROVIDERS_ROOT, &inherited);
        // Then each reads its own directory under the volume, once, and none sees the others' directory variables
        assert_eq!(value(&codex, "CODEX_HOME"), Some("/data/providers/codex"));
        assert_eq!(codex.iter().filter(|(n, _)| n == "CODEX_HOME").count(), 1);
        assert_eq!(value(&claude, "CLAUDE_CONFIG_DIR"), Some("/data/providers/claude"));
        assert_eq!((value(&claude, "CODEX_HOME"), value(&claude, "XDG_CONFIG_HOME")), (None, None));
        assert_eq!(value(&opencode, "XDG_CONFIG_HOME"), Some("/data/providers/opencode/config"));
        assert_eq!((value(&opencode, "CODEX_HOME"), value(&opencode, "CLAUDE_CONFIG_DIR")), (None, None));
        assert_eq!((value(&codex, "XDG_CONFIG_HOME"), value(&codex, "CLAUDE_CONFIG_DIR")), (None, None));
    }
}
