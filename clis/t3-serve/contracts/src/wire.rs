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

/// `OrchestrationV2DispatchCommandError`: `detail` repeats `message`, and `cause` is the failure the pin's dispatcher wrapped.
fn dispatch_failure(command_id: &str, command_type: &str, message: &str, cause: Value) -> Value {
    json!({"_tag": "OrchestrationV2DispatchCommandError", "commandId": command_id, "commandType": command_type, "detail": message, "message": message, "cause": cause})
}

pub fn dispatch_error(command_id: &str, command_type: &str, message: &str) -> Value {
    if command_type == "queued-run.edit" {
        let cause = json!({"cause": message, "message": format!("Orchestration command {command_type} ({command_id}) was rejected before commit."), "name": "OrchestratorCommandRejectedError"});
        return dispatch_failure(command_id, command_type, message, cause);
    }
    let cause = json!({"cause": message, "message": format!("Failed to dispatch orchestration command {command_type} ({command_id})."), "name": "OrchestratorDispatchError"});
    dispatch_failure(command_id, command_type, message, cause)
}

/// What the pin's projection store fails with for a thread it holds no projection of: the cause every thread-bound error nests.
pub fn projection_missing_cause(thread_id: &str) -> Value {
    json!({"cause": {"message": format!("No orchestration projection exists for thread {thread_id}."), "name": "ProjectionStoreThreadNotFoundError"}, "message": projection_load_failed(thread_id), "name": "OrchestratorProjectionError"})
}

/// The error `tag` declares for a thread that has no projection: the missing cause, with `extra`'s fields beside it.
pub fn thread_missing_error(tag: &str, thread_id: &str, extra: Value) -> Value {
    let mut error = json!({"_tag": tag, "cause": projection_missing_cause(thread_id)});
    if let (Some(into), Some(from)) = (error.as_object_mut(), extra.as_object()) {
        into.extend(from.clone());
    }
    error
}

/// `OrchestrationV2GetThreadProjectionError`: what reading or subscribing to a thread's projection fails with when it has none.
pub fn thread_projection_error(thread_id: &str) -> Value {
    thread_missing_error("OrchestrationV2GetThreadProjectionError", thread_id, json!({"message": format!("Failed to load orchestration V2 thread {thread_id}"), "threadId": thread_id}))
}

/// What the pin's dispatcher fails with for a command of a thread it holds no projection of.
pub fn dispatch_thread_missing(command_id: &str, command_type: &str, thread_id: &str) -> Value {
    let message = format!("No orchestration projection exists for thread {thread_id}.");
    dispatch_failure(command_id, command_type, &message, projection_missing_cause(thread_id))
}

/// The outer message of `dispatch_thread_missing`'s cause, which a later dispatch of the same command id quotes.
pub fn projection_load_failed(thread_id: &str) -> String {
    format!("Failed to load orchestration projection for thread {thread_id}.")
}

/// A command id whose first dispatch was refused is refused again with the first refusal's message (`reason`).
pub fn dispatch_previously_rejected(command_id: &str, command_type: &str, reason: &str) -> Value {
    let message = format!("Command {command_id} was previously rejected: {reason}");
    dispatch_failure(command_id, command_type, &message, json!({"message": message, "name": "OrchestratorCommandPreviouslyRejectedError"}))
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

/// The refusal of a delete of a project that holds threads: the message is the cause's, as the pin passes it through.
pub fn project_not_empty_error(command_id: &str, project_id: &str) -> Value {
    let message = project_not_empty(project_id);
    json!({"_tag": "ProjectMutationError", "commandId": command_id, "message": message, "cause": {"message": message, "name": "ProjectNotEmptyError"}})
}

/// The refusal of a mutation of a project the pin holds none of: the generic message, with the cause that names the project.
pub fn project_not_found_error(command_id: &str, project_id: &str) -> Value {
    json!({"_tag": "ProjectMutationError", "commandId": command_id, "message": PROJECT_MUTATION_FAILED, "cause": {"message": format!("Project {project_id} was not found."), "name": "ProjectNotFoundError"}})
}

/// What a file RPC says for a workspace folder that does not exist: the RPC's own error `tag` with the pin's failure code, the folder as given
/// and the cause that names it. `verb` is how the pin words the operation ("list", "search"); `extra` holds the fields only that RPC has.
pub fn workspace_root_missing(tag: &str, verb: &str, cwd: &str, extra: Value) -> Value {
    let mut error = json!({
        "_tag": tag,
        "cwd": cwd,
        "failure": "workspace_root_not_found",
        "message": format!("Failed to {verb} workspace entries in '{cwd}'."),
        "normalizedCwd": cwd,
        "cause": {"message": format!("Workspace root does not exist: {cwd}"), "name": "WorkspaceRootNotExistsError"},
    });
    if let (Some(into), Some(from)) = (error.as_object_mut(), extra.as_object()) {
        into.extend(from.clone());
    }
    error
}

/// `projects.readFile` in a folder that does not exist: the operation that failed is the resolution of the folder itself.
pub fn read_file_root_missing(cwd: &str, relative_path: &str) -> Value {
    let resolved = format!("{}/{relative_path}", cwd.trim_end_matches('/'));
    json!({
        "_tag": "ProjectReadFileError",
        "cwd": cwd,
        "failure": "operation_failed",
        "message": format!("Failed to read workspace file '{relative_path}' in '{cwd}'."),
        "operation": "realpath-workspace-root",
        "operationPath": relative_path,
        "relativePath": relative_path,
        "resolvedPath": resolved,
        "cause": {
            "name": "WorkspaceFileSystemOperationError",
            "message": format!("Workspace file operation 'realpath-workspace-root' failed at '{relative_path}' for resolved path '{resolved}' (requested as '{relative_path}' in '{cwd}')."),
            "cause": {"name": "Error", "message": format!("ENOENT: no such file or directory, realpath '{cwd}'")},
        },
    })
}

/// `projects.writeFile` in a folder that is no project, managed worktree or thread worktree of this server.
pub fn write_file_unknown_workspace(cwd: &str, relative_path: &str, missing: bool) -> Value {
    let mut cause = json!({"name": "UnknownWorkspaceError", "message": format!("'{cwd}' is not a registered project, a managed worktree, or a thread's worktree.")});
    if missing {
        cause["cause"] = json!({"name": "PlatformError", "message": format!("NotFound: FileSystem.realPath ({cwd})"), "cause": {"name": "Error", "message": format!("ENOENT: no such file or directory, lstat '{cwd}'")}});
    }
    json!({"_tag": "ProjectWriteFileError", "cwd": cwd, "failure": "workspace_path_outside_root", "message": format!("Failed to write workspace file '{relative_path}' in '{cwd}'."), "relativePath": relative_path, "cause": cause})
}

/// A file or turn operation the execution host refused or could not do.
pub fn host_error(message: &str) -> Value {
    json!({"_tag": "ExecutionHostError", "message": message})
}

pub fn not_found_error(what: &str, id: &str) -> Value {
    json!({"_tag": "NotFoundError", "message": format!("{what} {id} was not found."), "id": id})
}

/// The version of the relay client the pin installs, which `cloud.getRelayClientStatus` names while none is installed.
pub const RELAY_CLIENT_VERSION: &str = "2026.10.4";

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
pub fn apply_provider_logins(list: &mut [Value], logins: &[(String, String)]) {
    for provider in list {
        let status = logins.iter().find(|(instance, _)| provider["instanceId"] == instance.as_str()).map(|(_, status)| status.as_str());
        provider["setup"]["canAuthenticate"] = json!(true);
        if let Some(status) = status {
            provider["auth"]["status"] = json!(if status == "signed_in" { "authenticated" } else { "unauthenticated" });
        }
    }
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
