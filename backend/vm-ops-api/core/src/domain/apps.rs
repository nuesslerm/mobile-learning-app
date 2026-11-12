//! The apps this service manages and what it knows about each: its containers, compose file, repo path and the compose services a
//! plain redeploy may restart alone. Every lookup is by exact name, so `toString` or `__proto__` from a request is simply no app.

/// In the order `knownApps` is listed in a reply.
pub const KNOWN_APPS: [&str; 9] = [
    "leetcode-backend",
    "remote-chat-agent",
    "leetcode-auth-cookie-capture",
    "agent-shell",
    "ci-runner",
    "vnc-auth-session",
    "reporting-service",
    "t3-serve",
    "orca-serve",
];

/// The apps whose logs are readable: the managed ones, then the control plane itself, which is no managed app.
pub const LOG_APPS: [&str; 10] = [
    "leetcode-backend",
    "remote-chat-agent",
    "leetcode-auth-cookie-capture",
    "agent-shell",
    "ci-runner",
    "vnc-auth-session",
    "reporting-service",
    "t3-serve",
    "orca-serve",
    "vm-ops-api",
];

/// Retiring these would take the control plane down, tailnet state included.
pub const UNRETIRABLE_PROJECTS: [&str; 2] = ["vm-ops-api", "watchdog"];

pub fn is_known_app(app: &str) -> bool {
    KNOWN_APPS.contains(&app)
}

pub fn is_unretirable_project(name: &str) -> bool {
    UNRETIRABLE_PROJECTS.contains(&name)
}

/// A name `POST /services/<app>/retire` may tear down: `^[a-z][a-z0-9-]{1,40}$`, and neither a managed app nor the control plane.
pub fn is_retirable(app: &str) -> bool {
    let name = app.as_bytes();
    let shaped = (2..=41).contains(&name.len()) && name[0].is_ascii_lowercase() && name[1..].iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-');
    shaped && !is_known_app(app) && !is_unretirable_project(app)
}

/// The apps a host may deploy, keyed by `HOST_LABEL`; a blank or unrecognised label has none, so it fails closed.
pub fn host_allowed_apps(host: &str) -> Option<&'static [&'static str]> {
    (host == "main").then_some(&KNOWN_APPS[..])
}

/// The explicit `container_name` values of an app's compose file; the first is the replaceable app container.
pub fn container_names(app: &str) -> Option<&'static [&'static str]> {
    Some(match app {
        "leetcode-backend" => &["leetcode-backend", "leetcode-backend-tailscale"],
        "remote-chat-agent" => &["remote-chat-agent", "remote-chat-agent-tailscale", "remote-chat-agent-opencode"],
        "leetcode-auth-cookie-capture" => &["leetcode-capture", "leetcode-capture-tailscale"],
        "agent-shell" => &["agent-shell", "agent-shell-sessions", "agent-shell-dind", "agent-shell-droid", "agent-shell-tailscale"],
        "ci-runner" => &["ci-runner-tailscale", "ci-runner-builder", "ci-runner-droid", "ci-runner-gh-runner"],
        "vnc-auth-session" => &["vnc-auth", "vnc-auth-tailscale"],
        "reporting-service" => &["reporting-service", "reporting-service-tailscale"],
        "t3-serve" => &["t3-serve", "t3-serve-tailscale"],
        "orca-serve" => &["orca-serve", "orca-serve-tailscale"],
        "vm-ops-api" => &["vm-ops-api-ops-api-1", "vm-ops-api-tailscale", "watchdog"],
        _ => return None,
    })
}

pub fn compose_file(app: &str) -> Option<&'static str> {
    Some(match app {
        "leetcode-backend" | "remote-chat-agent" | "agent-shell" | "ci-runner" | "reporting-service" | "t3-serve" | "orca-serve" => "docker-compose.prod.yml",
        "leetcode-auth-cookie-capture" => "docker-compose.capture.yml",
        "vnc-auth-session" => "docker-compose.auth.yml",
        _ => return None,
    })
}

/// Repo files an app's compose file reaches by a relative path (`extends: file: ../dind.yml`); provisioning copies each one to the same relative spot beside the app's directory.
pub fn shared_files(app: &str) -> &'static [&'static str] {
    match app {
        "agent-shell" | "ci-runner" => &["infra/oracle-cloud/dind.yml"],
        _ => &[],
    }
}

pub fn repo_path(app: &str) -> String {
    match app {
        "agent-shell" => "infra/oracle-cloud/agent-shell".to_string(),
        "ci-runner" => "infra/oracle-cloud/ci-runner".to_string(),
        "vnc-auth-session" => "clis/vnc-auth-capture".to_string(),
        "t3-serve" => "clis/t3-serve".to_string(),
        "orca-serve" => "clis/orca-serve".to_string(),
        _ => format!("backend/{app}"),
    }
}

/// The compose services a plain redeploy restarts alone with `--no-deps`, leaving sessions and sidecars up.
pub fn replaceable_services(app: &str) -> Option<&'static [&'static str]> {
    match app {
        "agent-shell" => Some(&["app"]),
        "remote-chat-agent" => Some(&["agent"]),
        _ => None,
    }
}

/// The env variable names a redeploy request body may set for `app`: a per-app declaration, none for an app that does not list one.
pub fn request_env_names(app: &str) -> &'static [&'static str] {
    match app {
        "ci-runner" => &["RUNNER_TOKEN", "TS_CI_RUNNER_AUTH_KEY"],
        _ => &[],
    }
}

/// How a redeploy proves a built image before it swaps anything in: a throwaway container of `service` runs on `SMOKE_PORT` in the
/// netns of the running one, and `probe` (a command inside it, with `{port}` the spare port) must pass. The apps without an HTTP
/// health have none.
pub struct Smoke {
    pub service: &'static str,
    pub probe: &'static str,
    /// The variable the app reads its listening port from.
    pub port_env: &'static str,
    /// Further env of the throwaway container, for an app whose start would otherwise touch what it shares with the live one.
    pub env: &'static [(&'static str, &'static str)],
}

/// The spare port every smoke test listens on, clear of every app's own.
pub const SMOKE_PORT: u16 = 18999;

/// The spare port of the smoke container's CI listener, beside the live one's: both share one network namespace.
pub const SMOKE_CI_PORT: u16 = 18998;

/// The apps a deploy cannot smoke-test, each with why: a second copy would not be proof, or would disturb the live one.
pub const SMOKE_EXEMPT: [(&str, &str); 2] = [
    ("agent-shell", "deployed by hand: the pipeline must not restart the shell sessions"),
    ("ci-runner", "deployed by hand, and a second runner would register a second runner with the same name and take a job, so a copy is no proof"),
];

pub fn smoke(app: &str) -> Option<Smoke> {
    match app {
        "leetcode-backend" => Some(Smoke { service: "app", probe: "leetcode-backend-rs --healthcheck", port_env: "PORT", env: &[] }),
        "remote-chat-agent" => Some(Smoke { service: "agent", probe: "curl -fsS -o /dev/null http://127.0.0.1:{port}/health", port_env: "PORT", env: &[] }),
        "reporting-service" => Some(Smoke { service: "app", probe: "reporting-service-rs --healthcheck", port_env: "PORT", env: &[] }),
        "t3-serve" => Some(Smoke { service: "app", probe: "t3-serve t3 serve --healthcheck", port_env: "T3_PORT", env: &[] }),
        "orca-serve" => Some(Smoke { service: "app", probe: "orca-serve serve --healthcheck", port_env: "ORCA_PORT", env: &[] }),
        "leetcode-auth-cookie-capture" => Some(Smoke { service: "app", probe: "leetcode-auth-cookie-capture-rs --healthcheck", port_env: "CAPTURE_LISTENER_PORT", env: &[] }),
        // no /health route: any HTTP answer of the listener proves it started; smoke mode keeps it off the live tailnet route and gate port
        "vnc-auth-session" => Some(Smoke { service: "app", probe: "curl -s -o /dev/null http://127.0.0.1:{port}/", port_env: "VNC_AUTH_LISTENER_PORT", env: &[("VNC_AUTH_SMOKE", "1"), ("VNC_AUTH_GATE_PORT", "18998")] }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILT_IN_NAMES: [&str; 6] = ["toString", "constructor", "__proto__", "hasOwnProperty", "valueOf", "prototype"];

    #[test]
    fn given_the_managed_apps_when_listed_then_they_are_the_ten_in_order_and_metro_runner_is_gone() {
        assert_eq!(
            KNOWN_APPS,
            ["leetcode-backend", "remote-chat-agent", "leetcode-auth-cookie-capture", "agent-shell", "ci-runner", "vnc-auth-session", "reporting-service", "t3-serve", "orca-serve"]
        );
        assert!(!is_known_app("metro-runner"));
        assert!(!is_known_app("vm-ops-api"));
        assert!(KNOWN_APPS.iter().all(|app| is_known_app(app)));
    }

    #[test]
    fn given_the_apps_whose_logs_are_readable_when_listed_then_they_are_the_known_apps_and_the_control_plane() {
        assert_eq!(LOG_APPS[..9], KNOWN_APPS);
        assert_eq!(LOG_APPS[9], "vm-ops-api");
        assert!(container_names("vm-ops-api").is_some());
    }

    #[test]
    fn given_names_when_asked_whether_they_may_be_retired_then_only_the_control_plane_is_refused_here() {
        assert!(is_unretirable_project("vm-ops-api"));
        assert!(is_unretirable_project("watchdog"));
        assert!(!is_unretirable_project("metro-runner"));
        assert!(!is_unretirable_project("leetcode-backend"));
    }

    #[test]
    fn given_names_when_asked_whether_they_may_be_retired_then_the_pattern_decides_and_the_managed_apps_and_the_control_plane_are_refused() {
        let (shortest, longest) = ("ab".to_string(), format!("a{}", "b".repeat(40)));
        for name in ["old-app", "metro-runner", "vm-ops-api-old", "watchdog2", "old-watchdog", shortest.as_str(), longest.as_str()] {
            assert!(is_retirable(name), "{name}");
        }
        let too_long = format!("a{}", "b".repeat(41));
        for name in ["a", "Old-app", "1old", "-old", "old_app", "old%2Dapp", "old-app\n", "", "vm-ops-api", "watchdog", "leetcode-backend", too_long.as_str()] {
            assert!(!is_retirable(name), "{name:?}");
        }
    }

    #[test]
    fn given_the_host_label_when_its_allowed_apps_are_read_then_main_has_all_ten_and_any_other_label_has_none() {
        assert_eq!(host_allowed_apps("main"), Some(&KNOWN_APPS[..]));
        for label in ["", "unknown", "Main", "main ", "light", "heavy"] {
            assert_eq!(host_allowed_apps(label), None, "HOST_LABEL {label:?}");
        }
    }

    #[test]
    fn given_the_apps_that_extend_the_shared_dind_file_when_their_shared_files_are_read_then_it_is_in_the_repo_and_the_others_have_none() {
        for app in ["agent-shell", "ci-runner"] {
            assert_eq!(shared_files(app), ["infra/oracle-cloud/dind.yml"], "{app}");
            let path = format!("{}/../../../{}", env!("CARGO_MANIFEST_DIR"), shared_files(app)[0]);
            assert!(std::path::Path::new(&path).is_file(), "{path}");
        }
        assert!(KNOWN_APPS.iter().filter(|app| !["agent-shell", "ci-runner"].contains(app)).all(|app| shared_files(app).is_empty()));
    }

    #[test]
    fn given_the_apps_when_their_container_names_are_read_then_each_is_the_compose_files_explicit_list() {
        let expected: [(&str, &[&str]); 10] = [
            ("leetcode-backend", &["leetcode-backend", "leetcode-backend-tailscale"]),
            ("remote-chat-agent", &["remote-chat-agent", "remote-chat-agent-tailscale", "remote-chat-agent-opencode"]),
            ("leetcode-auth-cookie-capture", &["leetcode-capture", "leetcode-capture-tailscale"]),
            ("agent-shell", &["agent-shell", "agent-shell-sessions", "agent-shell-dind", "agent-shell-droid", "agent-shell-tailscale"]),
            ("ci-runner", &["ci-runner-tailscale", "ci-runner-builder", "ci-runner-droid", "ci-runner-gh-runner"]),
            ("vnc-auth-session", &["vnc-auth", "vnc-auth-tailscale"]),
            ("reporting-service", &["reporting-service", "reporting-service-tailscale"]),
            ("t3-serve", &["t3-serve", "t3-serve-tailscale"]),
            ("orca-serve", &["orca-serve", "orca-serve-tailscale"]),
            ("vm-ops-api", &["vm-ops-api-ops-api-1", "vm-ops-api-tailscale", "watchdog"]),
        ];
        for (app, names) in expected {
            assert_eq!(container_names(app), Some(names), "{app}");
        }
        assert_eq!(container_names("metro-runner"), None);
    }

    #[test]
    fn given_two_apps_when_their_container_names_are_compared_then_no_container_belongs_to_both() {
        let mut seen = std::collections::HashSet::new();
        for app in LOG_APPS {
            for name in container_names(app).expect("every readable app has containers") {
                assert!(seen.insert(*name), "{name} is listed for two apps");
            }
        }
    }

    #[test]
    fn given_the_managed_apps_when_their_compose_files_are_read_then_each_has_one_and_vm_ops_api_has_none() {
        let expected = [
            ("leetcode-backend", "docker-compose.prod.yml"),
            ("remote-chat-agent", "docker-compose.prod.yml"),
            ("leetcode-auth-cookie-capture", "docker-compose.capture.yml"),
            ("agent-shell", "docker-compose.prod.yml"),
            ("ci-runner", "docker-compose.prod.yml"),
            ("vnc-auth-session", "docker-compose.auth.yml"),
            ("reporting-service", "docker-compose.prod.yml"),
            ("t3-serve", "docker-compose.prod.yml"),
            ("orca-serve", "docker-compose.prod.yml"),
        ];
        for (app, file) in expected {
            assert_eq!(compose_file(app), Some(file), "{app}");
        }
        assert!(KNOWN_APPS.iter().all(|app| compose_file(app).is_some()));
        assert_eq!(compose_file("vm-ops-api"), None);
        assert_eq!(compose_file("metro-runner"), None);
    }

    #[test]
    fn given_an_app_when_its_repo_path_is_read_then_the_apps_outside_backend_live_elsewhere_and_the_rest_under_backend() {
        assert_eq!(repo_path("agent-shell"), "infra/oracle-cloud/agent-shell");
        assert_eq!(repo_path("ci-runner"), "infra/oracle-cloud/ci-runner");
        assert_eq!(repo_path("vnc-auth-session"), "clis/vnc-auth-capture");
        assert_eq!(repo_path("t3-serve"), "clis/t3-serve");
        assert_eq!(repo_path("orca-serve"), "clis/orca-serve");
        for app in ["leetcode-backend", "remote-chat-agent", "leetcode-auth-cookie-capture", "reporting-service"] {
            assert_eq!(repo_path(app), format!("backend/{app}"));
        }
    }

    #[test]
    fn given_the_apps_when_their_replaceable_services_are_read_then_only_agent_shell_and_remote_chat_agent_have_them() {
        assert_eq!(replaceable_services("agent-shell"), Some(&["app"][..]));
        assert_eq!(replaceable_services("remote-chat-agent"), Some(&["agent"][..]));
        for app in ["leetcode-backend", "leetcode-auth-cookie-capture", "vnc-auth-session", "reporting-service", "t3-serve", "orca-serve", "vm-ops-api", "ci-runner"] {
            assert_eq!(replaceable_services(app), None, "{app}");
        }
    }

    #[test]
    fn given_every_managed_app_when_its_smoke_test_is_read_then_each_has_one_or_a_written_exemption_and_never_both() {
        for app in KNOWN_APPS {
            let exempt = SMOKE_EXEMPT.iter().any(|(name, why)| *name == app && !why.is_empty());
            assert!(smoke(app).is_some() != exempt, "{app}: needs a smoke test or an entry in SMOKE_EXEMPT, not both");
        }
        assert!(SMOKE_EXEMPT.iter().all(|(name, _)| is_known_app(name)));
    }

    #[test]
    fn given_the_apps_with_a_smoke_test_when_their_service_is_read_then_their_compose_file_defines_it() {
        for app in KNOWN_APPS {
            let Some(check) = smoke(app) else { continue };
            let path = format!("{}/../../../{}/{}", env!("CARGO_MANIFEST_DIR"), repo_path(app), compose_file(app).expect("a compose file"));
            let compose = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(compose.contains(&format!("\n  {}:\n", check.service)), "{app}: compose has no service {}", check.service);
        }
    }

    #[test]
    fn given_a_name_that_is_a_built_in_of_a_plain_object_when_looked_up_then_it_is_no_app_anywhere() {
        for name in BUILT_IN_NAMES {
            assert!(!is_known_app(name), "{name}");
            assert!(!is_unretirable_project(name), "{name}");
            assert_eq!(host_allowed_apps(name), None, "{name}");
            assert_eq!(container_names(name), None, "{name}");
            assert_eq!(compose_file(name), None, "{name}");
            assert_eq!(replaceable_services(name), None, "{name}");
            assert_eq!(repo_path(name), format!("backend/{name}"), "{name}");
        }
    }
}
