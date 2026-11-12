use clap::Parser;
use secrecy::SecretString;
use std::path::PathBuf;
use vm_ops_api_core::application::health::Build;
use vm_ops_api_core::application::scripts::redeploy::RedeployEnv;
use vm_ops_api_core::domain::auth::AuthConfig;
use vm_ops_api_core::domain::js;

const UNKNOWN: &str = "unknown";
const DEFAULT_PORT: u16 = 8080;

/// Everything the service reads from its environment (or the same-named flags). A variable set to the empty string counts as
/// unset, as it does for Node's `env.X || default`.
#[derive(Parser, Default)]
#[command(name = "vm-ops-api-rs", about = "The bearer-gated control surface of the VM")]
pub struct Config {
    #[arg(long, env = "PORT")]
    port: Option<String>,

    #[arg(long, env = "VM_OPS_API_AUTH_TOKEN", hide_env_values = true)]
    auth_token: Option<String>,

    /// The SPKI PEM of the Ed25519 key that signs requests, with each newline written as the two characters `\n`.
    /// The second listener, the CI port: unset or empty leaves it off.
    #[arg(long, env = "CI_PORT")]
    ci_port: Option<String>,

    /// The CI port's own seed, never the admin one; a CI port without it refuses to start.
    #[arg(long, env = "VM_OPS_API_CI_AUTH_TOKEN", hide_env_values = true)]
    ci_auth_token: Option<String>,

    #[arg(long, env = "VM_GATEWAY_REQUEST_SIGNING_PUBLIC_KEY")]
    signing_public_key: Option<String>,

    /// `true` rejects the legacy raw token; any other value allows it.
    #[arg(long, env = "REQUIRE_SIGNED_AUTH")]
    require_signed_auth: Option<String>,

    #[arg(long, env = "HOST_LABEL")]
    host_label: Option<String>,

    /// `true` turns debug mode on; any other value leaves it off.
    #[arg(long, env = "DEBUG_MODE")]
    debug_mode: Option<String>,

    #[arg(long, env = "GIT_COMMIT")]
    git_commit: Option<String>,

    #[arg(long, env = "VM_OPS_API_IMAGE_TAG")]
    image_tag: Option<String>,

    /// Only read to keep it out of the logs.
    #[arg(long, env = "VM_PROVISION_GITHUB_TOKEN", hide_env_values = true)]
    provision_github_token: Option<String>,

    #[arg(long, env = "DOCKER_SOCKET")]
    docker_socket: Option<String>,

    #[arg(long, env = "PROC_ROOT")]
    proc_root: Option<String>,


    #[arg(long, env = "OPENCODE_LOG_STATE_DIR")]
    opencode_log_state_dir: Option<String>,

    #[arg(long, env = "IMAGE_NAMESPACE")]
    image_namespace: Option<String>,

    /// The compose dir of this service on the host, which the self-swap, history and status siblings mount.
    #[arg(long, env = "VM_DIR")]
    vm_dir: Option<String>,

    #[arg(long, env = "BETTERSTACK_OTLP_HOST")]
    betterstack_otlp_host: Option<String>,

    #[arg(long, env = "BETTERSTACK_OTLP_TOKEN", hide_env_values = true)]
    betterstack_otlp_token: Option<String>,
}

fn present(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|text| !text.is_empty())
}

fn path_or(value: &Option<String>, default: &str) -> PathBuf {
    PathBuf::from(present(value).unwrap_or(default))
}

fn port_from(variable: &str, value: &Option<String>, default: u16) -> Result<u16, String> {
    let Some(text) = present(value) else { return Ok(default) };
    let number = js::to_number(text);
    if number.fract() == 0.0 && (0.0..=f64::from(u16::MAX)).contains(&number) {
        Ok(number as u16)
    } else {
        Err(format!("{variable} is not a port number: {text}"))
    }
}

impl Config {
    /// The token that gates the service; none when it is unset or empty, which the service refuses to start on.
    pub fn auth_token(&self) -> Option<SecretString> {
        present(&self.auth_token).map(SecretString::from)
    }

    pub fn auth(&self, token: SecretString) -> AuthConfig {
        AuthConfig::new(token, &self.signing_pem(), present(&self.require_signed_auth) == Some("true"))
    }

    fn signing_pem(&self) -> String {
        present(&self.signing_public_key).unwrap_or_default().replace("\\n", "\n")
    }

    /// The CI port, when one is asked for: it must differ from the admin port and come with its own seed, and its seed must not be
    /// the admin one, or a CI token would open the admin port.
    pub fn ci_port(&self) -> Result<Option<u16>, String> {
        if present(&self.ci_port).is_none() {
            return Ok(None);
        }
        let port = port_from("CI_PORT", &self.ci_port, 0)?;
        if port == self.port()? {
            return Err(format!("CI_PORT is the admin port, {port}"));
        }
        let (ci, admin) = (present(&self.ci_auth_token), present(&self.auth_token));
        match ci {
            None => Err("CI_PORT is set but VM_OPS_API_CI_AUTH_TOKEN is not".to_string()),
            Some(token) if Some(token) == admin => Err("VM_OPS_API_CI_AUTH_TOKEN is the admin seed".to_string()),
            Some(_) => Ok(Some(port)),
        }
    }

    /// The CI port's credentials: signed requests only, since a CI job has no use for the raw seed.
    pub fn ci_auth(&self) -> Option<AuthConfig> {
        present(&self.ci_auth_token).map(|token| AuthConfig::new(SecretString::from(token), &self.signing_pem(), true))
    }

    /// Read as Node reads it, `Number(PORT)`: `0x4651` and `1e4` are ports, `18097.5` and `-1` are not.
    pub fn port(&self) -> Result<u16, String> {
        port_from("PORT", &self.port, DEFAULT_PORT)
    }

    pub fn host_label(&self) -> String {
        present(&self.host_label).unwrap_or(UNKNOWN).to_string()
    }

    pub fn build(&self, server_script_sha: String) -> Build {
        Build {
            commit: present(&self.git_commit).unwrap_or(UNKNOWN).to_string(),
            host: self.host_label(),
            debug_mode: present(&self.debug_mode) == Some("true"),
            server_script_sha,
            image_tag: present(&self.image_tag).unwrap_or(UNKNOWN).to_string(),
        }
    }

    pub fn docker_socket(&self) -> PathBuf {
        path_or(&self.docker_socket, "/var/run/docker.sock")
    }

    pub fn proc_root(&self) -> PathBuf {
        path_or(&self.proc_root, "/proc")
    }

    pub fn opencode_log_state_dir(&self) -> PathBuf {
        path_or(&self.opencode_log_state_dir, "/state/opencode-log")
    }

    /// The image this service runs, which its siblings run too: `<namespace>/vm-ops-api-rs:<tag>`, `latest` while the tag is unknown.
    pub fn sibling_image(&self) -> String {
        self.redeploy_env().sibling_image()
    }

    /// What every sibling script is rendered from, read as Node reads it: an empty variable is an unset one.
    pub fn redeploy_env(&self) -> RedeployEnv {
        RedeployEnv {
            host_label: self.host_label(),
            image_namespace: present(&self.image_namespace).unwrap_or("nuesslerm").to_string(),
            image_tag: present(&self.image_tag).map(str::to_string),
            provision_token: present(&self.provision_github_token).unwrap_or_default().to_string(),
            otlp_host: present(&self.betterstack_otlp_host).map(str::to_string),
            otlp_token: present(&self.betterstack_otlp_token).map(str::to_string),
            vm_dir: present(&self.vm_dir).unwrap_or("/home/ubuntu/vm-ops-api").to_string(),
        }
    }

    /// Values the log sink must scrub although the environment does not name them as secrets.
    pub fn secrets(&self) -> Vec<String> {
        [&self.auth_token, &self.ci_auth_token, &self.provision_github_token].into_iter().filter_map(present).map(str::to_string).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
    use ed25519_dalek::pkcs8::EncodePublicKey;
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    use vm_ops_api_core::domain::auth::{derive_rotating_token, is_authorized, Credentials, RequestLine};

    fn some(text: &str) -> Option<String> {
        Some(text.to_string())
    }

    #[test]
    fn given_nothing_set_when_read_then_the_defaults_are_nodes() {
        let config = Config::default();
        assert_eq!(config.port(), Ok(8080));
        assert_eq!(config.host_label(), "unknown");
        let build = config.build("abc".into());
        assert_eq!((build.commit.as_str(), build.image_tag.as_str(), build.debug_mode), ("unknown", "unknown", false));
        assert!(config.auth_token().is_none());
        assert!(config.secrets().is_empty());
    }

    #[test]
    fn given_the_image_when_read_then_siblings_run_the_rust_image_at_its_tag_and_latest_while_it_is_unknown() {
        let image = |namespace: Option<String>, tag: Option<String>| Config { image_namespace: namespace, image_tag: tag, ..Config::default() }.sibling_image();
        assert_eq!(image(None, None), "nuesslerm/vm-ops-api-rs:latest");
        assert_eq!(image(some("ns"), some("unknown")), "ns/vm-ops-api-rs:latest");
        assert_eq!(image(some("ns"), some("sha-1234567")), "ns/vm-ops-api-rs:sha-1234567");
    }

    #[test]
    fn given_empty_values_when_read_then_they_count_as_unset() {
        let config = Config { auth_token: some(""), port: some(""), host_label: some(""), git_commit: some(""), ..Config::default() };
        assert!(config.auth_token().is_none());
        assert_eq!(config.port(), Ok(8080));
        assert_eq!(config.host_label(), "unknown");
        assert_eq!(config.build("abc".into()).commit, "unknown");
    }

    #[test]
    fn given_a_port_when_read_then_a_number_in_range_is_taken_and_anything_else_is_refused() {
        assert_eq!(Config { port: some("18080"), ..Config::default() }.port(), Ok(18080));
        assert_eq!(Config { port: some("abc"), ..Config::default() }.port(), Err("PORT is not a port number: abc".to_string()));
        assert_eq!(Config { port: some("70000"), ..Config::default() }.port(), Err("PORT is not a port number: 70000".to_string()));
    }

    #[test]
    fn given_a_port_in_a_form_number_reads_when_read_then_it_is_the_port_node_listens_on() {
        let port = |text: &str| Config { port: some(text), ..Config::default() }.port();
        for (text, want) in [(" 18097", 18097), ("0x4651", 18001), ("1e4", 10_000), ("+18097", 18097), ("018097", 18097), ("65535", 65_535), ("0", 0), (" ", 0)] {
            assert_eq!(port(text), Ok(want), "PORT={text:?}");
        }
        for text in ["18097.5", "-1", "65536", "1e5", "0x", "NaN", "Infinity", "8080a"] {
            assert!(port(text).is_err(), "PORT={text:?}");
        }
    }

    #[test]
    fn given_debug_mode_values_when_read_then_only_the_text_true_turns_it_on() {
        let debug = |value: &str| Config { debug_mode: some(value), ..Config::default() }.build("a".into()).debug_mode;
        assert!(debug("true"));
        assert!(!debug("TRUE"));
        assert!(!debug("1"));
    }

    fn legacy_token_passes(config: &Config) -> bool {
        let auth = config.auth(SecretString::from("seed"));
        let credentials = Credentials { authorization: Some("Bearer seed"), ..Credentials::default() };
        is_authorized(&auth, 0, &credentials, &RequestLine { method: "GET", path_and_query: "/health" }, b"")
    }

    #[test]
    fn given_signed_auth_values_when_the_legacy_token_is_presented_then_only_the_text_true_refuses_it() {
        let with = |value: Option<String>| legacy_token_passes(&Config { require_signed_auth: value, ..Config::default() });
        assert!(with(None));
        assert!(with(some("false")));
        assert!(with(some("1")));
        assert!(!with(some("true")));
    }

    #[test]
    fn given_a_public_key_with_each_newline_written_as_backslash_n_when_a_signed_token_arrives_then_it_verifies() {
        // Given the key as compose passes it, on one line
        let key = SigningKey::from_bytes(&[7; 32]);
        let pem = key.verifying_key().to_public_key_pem(LineEnding::LF).expect("the test key encodes");
        let config = Config { signing_public_key: some(&pem.replace('\n', "\\n")), ..Config::default() };
        // When a request signed with its private half arrives with today's rotating token
        let now_ms = 1_791_115_200_000;
        let timestamp = now_ms.to_string();
        let canonical = format!("GET\n/health\n{timestamp}\n{}", hex::encode(Sha256::digest(b"")));
        let signature = STANDARD.encode(key.sign(canonical.as_bytes()).to_bytes());
        let authorization = format!("Bearer {}", derive_rotating_token("seed", "2026-10-04"));
        let credentials = Credentials { authorization: Some(&authorization), signature: Some(&signature), timestamp: Some(&timestamp) };
        let auth = config.auth(SecretString::from("seed"));
        // Then it is let in
        assert!(is_authorized(&auth, now_ms, &credentials, &RequestLine { method: "GET", path_and_query: "/health" }, b""));
    }

    #[test]
    fn given_the_token_and_the_github_token_when_the_secrets_are_listed_then_both_are_named() {
        let config = Config { auth_token: some("seed"), provision_github_token: some("ghp"), ..Config::default() };
        assert_eq!(config.secrets(), ["seed", "ghp"]);
    }

    #[test]
    fn given_a_ci_port_when_it_is_read_then_it_needs_its_own_seed_and_a_port_of_its_own() {
        let ci = |port: Option<String>, seed: Option<String>, admin_seed: Option<String>| Config { ci_port: port, ci_auth_token: seed, auth_token: admin_seed, ..Config::default() }.ci_port();
        assert_eq!(ci(None, None, some("admin")), Ok(None));
        assert_eq!(ci(some(""), None, some("admin")), Ok(None));
        assert_eq!(ci(some("8081"), some("ci"), some("admin")), Ok(Some(8081)));
        assert_eq!(ci(some("8081"), None, some("admin")), Err("CI_PORT is set but VM_OPS_API_CI_AUTH_TOKEN is not".to_string()));
        assert_eq!(ci(some("8081"), some("admin"), some("admin")), Err("VM_OPS_API_CI_AUTH_TOKEN is the admin seed".to_string()));
        assert_eq!(ci(some("8080"), some("ci"), some("admin")), Err("CI_PORT is the admin port, 8080".to_string()));
        assert_eq!(ci(some("nope"), some("ci"), some("admin")), Err("CI_PORT is not a port number: nope".to_string()));
    }

    #[test]
    fn given_the_ci_seed_when_the_ci_port_reads_a_raw_seed_then_it_is_refused_even_where_the_admin_port_would_take_it() {
        let config = Config { ci_auth_token: some("ci"), ..Config::default() };
        let ci = config.ci_auth().expect("a CI seed is set");
        let raw = Credentials { authorization: Some("Bearer ci"), ..Credentials::default() };
        assert!(!is_authorized(&ci, 0, &raw, &RequestLine { method: "GET", path_and_query: "/health" }, b""));
        assert!(Config::default().ci_auth().is_none());
    }

    #[test]
    fn given_both_seeds_when_the_secrets_are_listed_then_the_scrubber_knows_the_ci_seed_too() {
        let config = Config { auth_token: some("admin"), ci_auth_token: some("ci"), ..Config::default() };
        assert_eq!(config.secrets(), ["admin", "ci"]);
    }
}
