//! JSON bodies the server writes: descriptor, auth, errors, `ServerConfig`. Builders take plain values so the core stays free of
//! serde details. Field sets follow protocol.md 2-4; defaults the real decoders fill are left out.
use serde_json::{json, Map, Value};

pub const TOKEN_EXCHANGE_GRANT: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
pub const ACCESS_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:access_token";
pub const BEARER_TTL_SECONDS: i64 = 30 * 24 * 3600;
pub const TICKET_TTL_SECONDS: i64 = 5 * 60;
pub const PAIRING_TTL_SECONDS: i64 = 5 * 60;
pub const PAIRING_CODE_LENGTH: usize = 12;
pub const PAIRING_ALPHABET: &str = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

pub struct Environment<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub os: &'a str,
    pub arch: &'a str,
    pub server_version: &'a str,
}

const CAPABILITIES: &str = include_str!("../data/capabilities.json");
const CONFIG_TEMPLATE: &str = include_str!("../data/server-config.json");
pub const DATABASE_MIGRATION: u32 = 76;

/// The capability flags this server advertises as true: only the ones whose feature a flow test proves (plan.md section 8).
pub const SUPPORTED_CAPABILITIES: [&str; 5] = ["connectionProbe", "threadPinning", "threadVisitedTracking", "threadStop", "threadRestartContinuation"];

/// Every flag the real server lists, true for the supported ones. `fileAttachments` is an object, so it is left out when unsupported.
pub fn capabilities() -> Value {
    let all: Map<String, Value> = serde_json::from_str(CAPABILITIES).unwrap_or_default();
    let flags = all.into_iter().filter(|(k, v)| v.is_boolean() || SUPPORTED_CAPABILITIES.contains(&k.as_str())).map(|(k, _)| {
        let on = SUPPORTED_CAPABILITIES.contains(&k.as_str());
        (k, Value::Bool(on))
    });
    Value::Object(flags.collect())
}

/// `GET /.well-known/t3/environment`.
pub fn descriptor(env: &Environment) -> Value {
    json!({
        "environmentId": env.id,
        "label": env.label,
        "platform": {"os": env.os, "arch": env.arch},
        "serverVersion": env.server_version,
        "orchestrationProtocolVersion": crate::ORCHESTRATION_PROTOCOL_VERSION,
        "databaseMigration": DATABASE_MIGRATION,
        "capabilities": capabilities(),
    })
}

pub fn token_response(access_token: &str, permissions: &[String]) -> Value {
    json!({
        "access_token": access_token,
        "issued_token_type": ACCESS_TOKEN_TYPE,
        "token_type": "Bearer",
        "expires_in": BEARER_TTL_SECONDS - 1,
        "scope": permissions.join(" "),
    })
}

/// A refused credential. The reason (unknown, expired, used) stays in the server's log: the body is the same for all of them.
pub fn auth_invalid(trace_id: &str) -> Value {
    json!({"_tag": "EnvironmentAuthInvalidError", "code": "auth_invalid", "reason": "invalid_credential", "traceId": trace_id})
}

pub fn bad_request(message: &str) -> Value {
    json!({"error": "invalid_request", "code": "bad_request", "message": message})
}

pub fn unauthorized(message: &str) -> Value {
    json!({"code": "unauthorized", "message": message})
}

pub fn protocol_incompatible() -> Value {
    json!({
        "code": "orchestration_protocol_incompatible",
        "message": "Update this client to one that supports orchestration protocol 2.",
        "orchestrationProtocolVersion": crate::ORCHESTRATION_PROTOCOL_VERSION,
    })
}

pub fn ticket_response(ticket: &str, expires_at: &str) -> Value {
    json!({"ticket": ticket, "expiresAt": expires_at})
}

/// The `auth` block of the session and of `ServerConfig`: what this server accepts (bearer tokens only, no cookie or DPoP).
pub fn auth_block() -> Value {
    json!({
        "policy": "remote-reachable",
        "bootstrapMethods": ["one-time-token"],
        "sessionMethods": ["bearer-access-token"],
        "sessionCookieName": "t3_session",
        "serverUpdateScope": "environment:maintain",
    })
}

pub fn session_state(session: Option<(&[String], &[String], &str)>) -> Value {
    match session {
        Some((permissions, legacy, expires_at)) => json!({
            "authenticated": true,
            "auth": auth_block(),
            "scopes": legacy,
            "permissions": permissions,
            "sessionMethod": "bearer-access-token",
            "expiresAt": expires_at,
        }),
        None => json!({"authenticated": false, "auth": auth_block()}),
    }
}

/// The tagged error every RPC may fail with: `EnvironmentAuthorizationError` (protocol.md 2.2, 3).
pub fn authorization_error(permission: &str) -> Value {
    json!({
        "_tag": "EnvironmentAuthorizationError",
        "message": format!("This session lacks the {permission} permission."),
        "requiredScope": crate::scopes::legacy_parent(permission),
        "requiredPermission": permission,
    })
}

pub fn dispatch_error(command_id: &str, command_type: &str, message: &str) -> Value {
    json!({"_tag": "OrchestrationV2DispatchCommandError", "commandId": command_id, "commandType": command_type, "message": message})
}

/// `orchestration.launchThread`'s declared error.
pub fn launch_error(command_id: &str, project_id: &str, message: &str) -> Value {
    json!({"_tag": "OrchestrationV2ThreadLaunchError", "commandId": command_id, "projectId": project_id, "message": message})
}

/// A launch's answer; `sequence` is the initial message's, absent for a launch without one.
pub fn launch_result(thread_id: &str, projection: Value, resumed: bool, sequence: Option<u64>) -> Value {
    let mut result = json!({"threadId": thread_id, "projection": projection, "resumed": resumed});
    if let Some(sequence) = sequence {
        result["sequence"] = sequence.into();
    }
    result
}

/// What `projects.mutate` says for every refusal but a non-empty delete; the reason stays in the server's log, as the pin does.
pub const PROJECT_MUTATION_FAILED: &str = "Failed to mutate project.";

pub fn project_mutation_error(command_id: &str, message: &str) -> Value {
    json!({"_tag": "ProjectMutationError", "commandId": command_id, "message": message})
}

/// `ProjectNotEmptyError`'s message, which `projects.mutate` passes through.
pub fn project_not_empty(project_id: &str) -> String {
    format!("Project {project_id} is not empty.")
}

/// A file or turn operation the execution host refused or could not do.
pub fn host_error(message: &str) -> Value {
    json!({"_tag": "ExecutionHostError", "message": message})
}

pub fn not_found_error(what: &str, id: &str) -> Value {
    json!({"_tag": "NotFoundError", "message": format!("{what} {id} was not found."), "id": id})
}

/// What every RPC this server does not build answers: the method's declared error is not known to us, so one generic tagged error.
pub fn unsupported_error(tag: &str) -> Value {
    json!({"_tag": "UnsupportedOperationError", "message": format!("{tag} is not supported by this server."), "method": tag})
}

pub struct Model<'a> {
    pub slug: &'a str,
    pub name: &'a str,
}

fn provider(instance: &str, display: &str, enabled: bool, message: Option<&str>, models: &[Model], checked_at: &str) -> Value {
    let models: Vec<Value> = models.iter().map(|m| json!({"slug": m.slug, "name": m.name, "isCustom": false, "capabilities": null})).collect();
    let mut p = json!({
        "instanceId": instance,
        "driver": instance,
        "displayName": display,
        "continuation": {"groupKey": format!("{instance}:instance:{instance}")},
        "showInteractionModeToggle": false,
        "setup": {"canAuthenticate": false, "canInstall": false},
        "enabled": enabled,
        "installed": enabled,
        "version": null,
        "status": if enabled { "ready" } else { "disabled" },
        "auth": {"status": if enabled { "authenticated" } else { "unknown" }},
        "checkedAt": checked_at,
        "models": models,
        "slashCommands": [],
        "skills": [],
        "usageLimits": {"checkedAt": checked_at, "windows": [], "unavailable": {"reason": "unsupported"}},
    });
    if let Some(m) = message {
        p["message"] = json!(m);
    }
    p
}

/// The provider instances: `opencode` (served by rca, with the ladder's models) is on; `claudeAgent` and `codex` wait for the provider image.
pub fn providers(models: &[Model], checked_at: &str) -> Vec<Value> {
    vec![
        provider("opencode", "OpenCode", true, None, models, checked_at),
        provider("claudeAgent", "Claude", false, Some("Claude Code is not installed in the rca image yet."), &[], checked_at),
        provider("codex", "Codex", false, Some("Codex is not installed in the rca image yet."), &[], checked_at),
    ]
}

/// The provider instances as `server.refreshProviders` shows them once the accounts group has said which are signed in: the login can be
/// started on each, and `auth.status` follows the login (`status` is the accounts group's `signed_in`, `signed_out` or `authenticating`).
pub fn providers_with_logins(models: &[Model], checked_at: &str, logins: &[(String, String)]) -> Vec<Value> {
    let mut list = providers(models, checked_at);
    for provider in &mut list {
        let status = logins.iter().find(|(instance, _)| provider["instanceId"] == instance.as_str()).map(|(_, status)| status.as_str());
        provider["setup"]["canAuthenticate"] = json!(true);
        if let Some(status) = status {
            provider["auth"]["status"] = json!(if status == "signed_in" { "authenticated" } else { "unauthenticated" });
        }
    }
    list
}

/// `ServerSettings` before anything was changed: the template's.
pub fn default_settings() -> Map<String, Value> {
    let template: Map<String, Value> = serde_json::from_str(CONFIG_TEMPLATE).unwrap_or_default();
    template.get("settings").and_then(Value::as_object).cloned().unwrap_or_default()
}

pub struct ConfigInput<'a> {
    pub environment: Value,
    pub cwd: &'a str,
    pub models: Vec<Model<'a>>,
    pub now: &'a str,
}

/// `ServerConfig`: the recorded template (keybindings, settings, flags) with this server's environment, paths and providers.
pub fn server_config(input: &ConfigInput) -> Value {
    let mut config: Map<String, Value> = serde_json::from_str(CONFIG_TEMPLATE).unwrap_or_default();
    let home = format!("{}/.t3", input.cwd);
    for (key, value) in [
        ("environment", input.environment.clone()),
        ("auth", auth_block()),
        ("cwd", json!(input.cwd)),
        ("keybindingsConfigPath", json!(format!("{home}/userdata/keybindings.json"))),
        ("providers", Value::Array(providers(&input.models, input.now))),
        ("observability", json!({"logsDirectoryPath": format!("{home}/userdata/logs"), "localTracingEnabled": false, "otlpTracesEnabled": false, "otlpMetricsEnabled": false, "otlpLogsEnabled": false})),
        ("scratchWorkspaceRoot", json!(format!("{home}/scratch"))),
        ("newProjectsRoot", json!(format!("{home}/projects"))),
    ] {
        config.insert(key.into(), value);
    }
    Value::Object(config)
}
