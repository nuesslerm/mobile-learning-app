//! The environment a provider CLI runs with. Each provider keeps its login and settings in a directory of its own on the service's volume
//! (`/data/providers/<provider>`), named to the CLI through the variables it reads, and sees only the variables that are its own or shared and
//! not secret: a second provider cannot reach the first one's tokens, and no CLI sees the service's own secret.
use super::driver::{Command, Driver};

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

/// Where a turn's CLI reaches the model gateway, and the token that opens it for this turn and this provider. The token is no provider key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRoute {
    /// The gateway's id of the provider (`anthropic`, `openai`, `opencode-go`, ...), which names the provider block opencode is given.
    pub provider: String,
    /// `<service>/gateway/<provider>`: what the CLI appends its own route to.
    pub base_url: String,
    pub token: String,
}

/// `command` pointed at the gateway: opencode through the `baseURL` of the provider block its config gets. Only a turn the service decided to route goes
/// through here; a CLI that keeps its own subscription login is launched as `command` has it. Claude Code and Codex live as processes that outlast a turn, so they
/// have no command to point (`docs/gaps/codex-session-not-through-gateway.md`, `docs/gaps/claude-session-not-through-gateway.md`).
pub fn through_gateway(driver: Driver, mut command: Command, route: &GatewayRoute) -> Command {
    match driver {
        Driver::ClaudeAgent | Driver::Codex => command,
        Driver::Opencode => {
            const PREFIX: &str = "OPENCODE_CONFIG_CONTENT=";
            let block = serde_json::json!({"options": {"baseURL": route.base_url, "apiKey": route.token}});
            let at = command.args.iter().position(|a| a.starts_with(PREFIX));
            let mut config = at.and_then(|i| serde_json::from_str::<serde_json::Value>(&command.args[i][PREFIX.len()..]).ok()).unwrap_or_else(|| serde_json::json!({}));
            config["provider"][route.provider.as_str()] = block;
            let setting = format!("{PREFIX}{config}");
            match at {
                Some(i) => command.args[i] = setting,
                None => {
                    let mut wrapped = vec![setting, command.program];
                    wrapped.extend(command.args);
                    return Command { program: "env".into(), args: wrapped, cwd: command.cwd };
                }
            }
            command
        }
    }
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

    fn route(provider: &str) -> GatewayRoute {
        GatewayRoute { provider: provider.into(), base_url: format!("http://svc:8787/gateway/{provider}"), token: "rcag_turn".into() }
    }

    fn request(driver: Driver, model: Option<&str>) -> super::super::driver::TurnRequest {
        super::super::driver::TurnRequest { thread_id: "t".into(), run_id: "r".into(), instance_id: driver.id().into(), model: model.map(str::to_string), cwd: "/w".into(), prompt: "hi".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: None, tools: None }
    }

    #[test]
    fn given_a_turn_routed_through_the_gateway_when_each_cli_is_built_then_it_is_pointed_at_the_gateway_with_the_turn_token_and_runs_with_the_provider_environment() {
        for (driver, provider, model) in [(Driver::Opencode, "opencode-go", Some("opencode-go/m"))] {
            // Given the host command of a turn of each CLI
            let host = super::super::driver::command_for_host(driver, &request(driver, model), "http://svc:8787/mcp/t", "mcp-tok").unwrap();
            // When it is pointed at the gateway
            let routed = through_gateway(driver, host.clone(), &route(provider));
            let line = format!("{} {}", routed.program, routed.args.join(" "));
            // Then the gateway's address and the turn's token are in it, the CLI's own program and prompt are still there and the prompt is last
            assert!(line.contains(&format!("http://svc:8787/gateway/{provider}")) && line.contains("rcag_turn"), "{line}");
            assert_eq!(routed.args.last(), host.args.last(), "{}", driver.id());
            // And the command is still known as that CLI's, so the service's own environment is stripped from it
            assert_eq!(super::super::login::driver_of_command(&routed.program, &routed.args), Some(driver), "{line}");
        }
    }

    #[test]
    fn given_a_command_of_a_cli_that_lives_when_routed_then_it_is_left_as_it_is() {
        for (driver, args) in [(Driver::ClaudeAgent, "-p"), (Driver::Codex, "app-server")] {
            let untouched = Command { program: driver.program().into(), args: vec![args.into()], cwd: ".".into() };
            assert_eq!(through_gateway(driver, untouched.clone(), &route("anthropic")), untouched, "{}", driver.id());
        }
    }

    #[test]
    fn given_an_opencode_turn_with_a_tool_server_when_routed_then_the_provider_block_joins_the_config_it_already_had() {
        // Given the host command of an opencode turn, whose config names the service's tool server
        let host = super::super::driver::command_for_host(Driver::Opencode, &request(Driver::Opencode, Some("kimi-for-coding/k3")), "http://svc:8787/mcp/t", "mcp-tok").unwrap();
        // When it is routed
        let routed = through_gateway(Driver::Opencode, host, &route("kimi"));
        // Then the single config setting has both the tool server and the provider's baseURL and token
        let settings: Vec<&String> = routed.args.iter().filter(|a| a.starts_with("OPENCODE_CONFIG_CONTENT=")).collect();
        assert_eq!(settings.len(), 1);
        let config: serde_json::Value = serde_json::from_str(&settings[0]["OPENCODE_CONFIG_CONTENT=".len()..]).unwrap();
        assert_eq!(config["mcp"]["executor"]["url"], "http://svc:8787/mcp/t");
        assert_eq!(config["provider"]["kimi"]["options"]["baseURL"], "http://svc:8787/gateway/kimi");
        assert_eq!(config["provider"]["kimi"]["options"]["apiKey"], "rcag_turn");
    }
}
