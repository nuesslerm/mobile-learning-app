//! The `docker run` argv of every sibling that deploys: the app redeploy, the self-swap, the history append and the retire. Each
//! script is pinned by its snapshot (`golden`).
use crate::application::scripts::{HELPER_APPEND_ENV_LOCAL, HELPER_PROVISION, HELPER_VERIFY_ENV, IMAGE_REPO};
use crate::domain::apps::{repo_path, replaceable_services, shared_files, smoke, SMOKE_CI_PORT, SMOKE_PORT};
use crate::domain::js;
use crate::domain::provision_ref::raw_url;
use serde_json::{json, Map, Value};

pub const REDEPLOY_LOG: &str = "last-redeploy.log";
pub const REDEPLOY_HISTORY_FILE: &str = "redeploy-history.jsonl";
const HISTORY_MAX_LINES: u32 = 200;
const DOCKER_SOCKET: &str = "/var/run/docker.sock:/var/run/docker.sock";
const BWS: &str = "/usr/local/bin/bws:/usr/local/bin/bws:ro";
const CLIS: &str = "clis:/opt/clis:ro";
/// The files a swap replaces, in the order they are swapped in: compose reads only the last two.
const SWAP_FILES: [&str; 3] = [".env.defaults", ".env", "docker-compose.yml"];
const UP_WAIT_SECONDS: u32 = 120;
/// A smoke test waits for its container to answer this many times two seconds.
const SMOKE_TRIES: u32 = 30;
const VOLUMES: &str = "docker volume create agent-workspace >/dev/null 2>&1 || true ; docker volume create clis >/dev/null 2>&1 || true";

/// What every sibling script is rendered from: where this service is, what it runs and the credentials it hands on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RedeployEnv {
    pub host_label: String,
    pub image_namespace: String,
    /// `VM_OPS_API_IMAGE_TAG`: the tag this service runs, none when unset.
    pub image_tag: Option<String>,
    pub provision_token: String,
    pub otlp_host: Option<String>,
    pub otlp_token: Option<String>,
    /// The compose dir of this service on the host, which the self-swap, the history and the status siblings mount.
    pub vm_dir: String,
}

impl RedeployEnv {
    /// The image every sibling runs: this service's own, `latest` while its tag is unknown.
    pub fn sibling_image(&self) -> String {
        let tag = self.image_tag.as_deref().filter(|tag| !tag.is_empty() && *tag != "unknown").unwrap_or("latest");
        format!("{}/{IMAGE_REPO}:{tag}", self.image_namespace)
    }

    /// The tag this service runs when it is one a swap may restore: `^[A-Za-z0-9._-]{1,128}$`, and not `unknown`.
    fn running_tag(&self) -> &str {
        let is_tag = |tag: &&str| (1..=128).contains(&tag.len()) && tag.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        self.image_tag.as_deref().filter(is_tag).filter(|tag| *tag != "unknown").unwrap_or("")
    }
}

/// An app redeploy, validated.
pub struct AppRedeploy<'a> {
    pub app: &'a str,
    pub host_dir: &'a str,
    pub compose_file: &'a str,
    pub tag: &'a str,
    pub reference: &'a str,
    pub wipe_state: bool,
    pub recreate_all: bool,
    /// Variables a request body set, appended to the app's `.env` last. Values travel in the sibling's env, never the script text.
    pub request_env: &'a [(String, String)],
}

/// One line of the redeploy history.
pub struct HistoryEntry<'a> {
    pub key: &'a str,
    pub requested_by: &'a str,
    pub tag: &'a str,
    pub reference: &'a str,
    pub outcome: &'a str,
    pub exit_code: Option<i32>,
}

fn history_cap() -> String {
    format!("tail -n {HISTORY_MAX_LINES} {REDEPLOY_HISTORY_FILE} > {REDEPLOY_HISTORY_FILE}.tmp && mv {REDEPLOY_HISTORY_FILE}.tmp {REDEPLOY_HISTORY_FILE}")
}

/// Seconds-resolution markers in the redeploy's own output, so a slow step shows up in the response and the history.
fn stamp(label: &str) -> String {
    format!("echo \"[redeploy] {label} $(date +%s)\"")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

/// The sibling runs this image, so the helper reads the token from its own env, never the script text.
fn provision_lines(label: &str, reference: &str, files: &[(String, String)]) -> Vec<String> {
    let args: Vec<String> = files.iter().flat_map(|(url, dest)| [shell_quote(url), shell_quote(dest)]).collect();
    vec![format!("echo '[provision] refreshing {label} from {reference}'"), format!("{HELPER_PROVISION} {}", args.join(" "))]
}

/// Sources only the vault bootstrap credential: any other ambient key would shadow its vault copy in sync-env. `local_dir` holds
/// the `.env.local` when `dir` is a staging directory.
fn sync_env_lines(dir: &str, for_app: &str, host_label: &str, local_dir: &str) -> Vec<String> {
    let mut lines = vec![
        // `.` is a special builtin: a syntax error in the sourced file would end the whole shell, outcome unrecorded.
        format!("if [ -f {local_dir}/.env.local ]; then grep -E '^(BWS_[A-Z0-9_]+|SECRETS_BACKEND)=' {local_dir}/.env.local > {dir}/.env.bootstrap || true; if sh -n {dir}/.env.bootstrap; then set -a; . {dir}/.env.bootstrap; set +a; rm -f {dir}/.env.bootstrap; else rm -f {dir}/.env.bootstrap; echo '[env] .env.local has a BWS_ or SECRETS_BACKEND line that does not parse' >&2; false; fi; fi"),
        "unset BETTERSTACK_OTLP_TOKEN BETTERSTACK_OTLP_HOST EXPO_PUBLIC_BETTERSTACK_TOKEN".to_string(),
        format!("( cd {dir} && wf secret sync-env --for {for_app} --backend bitwarden --base .env.defaults --out .env )"),
        format!("cp {dir}/.env {dir}/.env.synced"),
        format!("echo \"HOST_LABEL={host_label}\" >> {dir}/.env"),
        format!("{HELPER_APPEND_ENV_LOCAL} {dir}/.env {local_dir}/.env.local"),
    ];
    lines.push(format!("{HELPER_VERIFY_ENV} {dir}/.env.synced {dir}/.env"));
    lines
}

/// A watchdog recreate reads the project's .env, so the running tag goes in it, through a temp file that a failure removes.
/// Compose takes the last of two IMAGE_TAG lines, so the pin wins over an `.env.defaults` default.
fn pin_image_tag(dir: &str, tag: &str) -> String {
    format!("if cp -p {dir}/.env {dir}/.env.tmp && echo \"IMAGE_TAG={tag}\" >> {dir}/.env.tmp && mv -f {dir}/.env.tmp {dir}/.env; then :; else rm -f {dir}/.env.tmp; false; fi")
}

/// Every sibling mounts this directory, so a flock on it is one lock for the whole host: pulls share it, a prune takes it alone.
/// Without it a prune that ends beside another leg's pull deletes that pull's leases and layers ("failed to lease content").
const DOCKER_LOCK: &str = "/home/ubuntu/vm-ops-api";

/// The pull of a redeploy, last in its own line so a failed pull ends the script.
fn compose_pull_command(request: &AppRedeploy) -> String {
    let compose = format!("docker compose -f {}", request.compose_file);
    let pull = match replaceable_services(request.app).filter(|_| !request.recreate_all && !request.wipe_state) {
        Some(replaceable) => format!("cd {} && {compose} pull {}", request.host_dir, replaceable.join(" ")),
        None => format!("cd {} && {compose} pull", request.host_dir),
    };
    format!("flock -s {DOCKER_LOCK} sh -c '{pull}'")
}

/// The swap: replaceable apps restart only their app service.
fn compose_up_command(request: &AppRedeploy) -> String {
    let (host_dir, app) = (request.host_dir, request.app);
    let compose = format!("docker compose -f {}", request.compose_file);
    let replaceable = replaceable_services(app).filter(|_| !request.recreate_all && !request.wipe_state);
    let Some(replaceable) = replaceable else {
        let wipe = if request.wipe_state { format!("(docker volume ls -q | grep -E '^{app}_.*tailscale-state$' | xargs -r docker volume rm || true) ; ") } else { String::new() };
        return format!("cd {host_dir} && {compose} down ; {VOLUMES} ; {wipe}{compose} up -d");
    };
    let narrow = replaceable.join(" ");
    let narrow_pattern = replaceable.join("|");
    // Creates missing sidecars and recreates any whose resolved config changed (a rotated secret in .env reaches its sidecar);
    // an unchanged container keeps running. The replaceable service is left to the second command: one `up` that recreates it
    // followed at once by another found the old container still being removed ("removal ... is already in progress").
    format!("{VOLUMES} ; cd {host_dir} && {compose} up -d --no-deps $({compose} config --services | grep -vxE '{narrow_pattern}') ; {compose} up -d --no-deps {narrow}")
}

/// Runs `service` of the pulled image as a throwaway container on the spare port, in the netns of the running one, until `probe`
/// passes inside it. A container that dies or never answers ends the script with its own log, before anything is swapped.
fn smoke_lines(cd: &str, compose: &str, name: &str, service: &str, probe: &str, port_env: &str, env: &[(&str, &str)]) -> Vec<String> {
    let container = format!("{name}-smoke");
    let extra: String = env.iter().map(|(key, value)| format!(" -e {key}={value}")).collect();
    let probe = shell_quote(&probe.replace("{port}", &SMOKE_PORT.to_string()));
    let mut lines = vec![
        stamp("smoke start"),
        format!("docker rm -f {container} >/dev/null 2>&1 || true"),
        format!("{cd} && {compose} run -d --no-deps --name {container} -e {port_env}={SMOKE_PORT}{extra} {service} >/dev/null"),
        "SMOKE_OK=0".to_string(),
        format!("for i in $(seq 1 {SMOKE_TRIES}); do [ \"$(docker inspect -f '{{{{.State.Running}}}}' {container} 2>/dev/null)\" = true ] || break; if docker exec {container} sh -c {probe} >/dev/null 2>&1; then SMOKE_OK=1; break; fi; sleep 2; done"),
        format!("if [ \"$SMOKE_OK\" != 1 ]; then echo '[smoke] {name} did not answer on port {SMOKE_PORT}; nothing was swapped. Its log:' >&2; docker logs --tail 80 {container} >&2; docker rm -f {container} >/dev/null 2>&1; exit 1; fi"),
    ];
    lines.push(format!("echo '[smoke] {name} healthy on port {SMOKE_PORT}'"));
    lines.push(stamp("smoke done"));
    lines
}

fn sibling_env_args(tag: &str, env: &RedeployEnv) -> Vec<String> {
    ["-e".to_string(), format!("IMAGE_TAG={tag}"), "-e".to_string(), format!("IMAGE_NAMESPACE={}", env.image_namespace), "-e".to_string(), format!("GITHUB_RAW_TOKEN={}", env.provision_token)].to_vec()
}

/// Each variable is written from the sibling's own env, after the generated `.env` and its local overrides.
fn request_env_lines(dir: &str, request_env: &[(String, String)]) -> Vec<String> {
    request_env.iter().map(|(name, _)| format!("printf '%s=%s\\n' {name} \"${name}\" >> {dir}/.env")).collect()
}

/// Provision, env sync and pull: what a smoke test and a redeploy of an app both start with.
fn app_front_lines(env: &RedeployEnv, request: &AppRedeploy) -> Vec<String> {
    let (host_dir, reference) = (request.host_dir, request.reference);
    let repo_path = repo_path(request.app);
    let mut files = vec![
        (raw_url(&format!("{repo_path}/{}", request.compose_file), reference), format!("{host_dir}/{}", request.compose_file)),
        (raw_url(&format!("{repo_path}/.env.defaults"), reference), format!("{host_dir}/.env.defaults")),
    ];
    files.extend(shared_files(request.app).iter().map(|path| (raw_url(path, reference), format!("{host_dir}/../{}", path.rsplit('/').next().unwrap_or(path)))));
    let mut lines = vec!["set -e".to_string(), stamp("start"), format!("mkdir -p {host_dir}")];
    lines.extend(provision_lines(&format!("{}'\"'\"'s compose file and .env.defaults", request.app), reference, &files));
    lines.push(stamp("provisioned"));
    lines.push(format!("if [ ! -f {host_dir}/.env.local ] && [ -f /home/ubuntu/vm-ops-api/.env.local ]; then cp /home/ubuntu/vm-ops-api/.env.local {host_dir}/.env.local; fi"));
    lines.extend(sync_env_lines(host_dir, request.app, &env.host_label, host_dir));
    lines.extend(request_env_lines(host_dir, request.request_env));
    lines.push(stamp("env synced"));
    lines.push(compose_pull_command(request));
    lines.push(stamp("pulled"));
    lines
}

/// The `docker run` argv of a foreground sibling that runs `lines` in the app's compose dir.
fn app_sibling_argv(env: &RedeployEnv, request: &AppRedeploy, lines: &[String]) -> Vec<String> {
    let host_dir = request.host_dir;
    let mut args = strings(&["run", "--pull", "missing", "--rm", "-v", DOCKER_SOCKET, "-v"]);
    args.push(format!("{host_dir}:{host_dir}"));
    args.extend(strings(&["-v", "/home/ubuntu/vm-ops-api:/home/ubuntu/vm-ops-api:ro", "-v", BWS]));
    // the image's /usr/local/bin/wf links into this volume (the VM's one wf build); without it `wf secret sync-env` is not found
    args.extend(strings(&["-v", CLIS, "-w", host_dir]));
    args.extend(sibling_env_args(request.tag, env));
    for (name, value) in request.request_env {
        args.extend(["-e".to_string(), format!("{name}={value}")]);
    }
    args.push(env.sibling_image());
    args.extend(strings(&["sh", "-c"]));
    args.push(lines.join("\n"));
    args
}

/// Foreground sibling: another app is redeployed, so the caller can await the real exit code.
pub fn app_redeploy_sibling_args(env: &RedeployEnv, request: &AppRedeploy) -> Vec<String> {
    let host_dir = request.host_dir;
    let mut lines = app_front_lines(env, request);
    lines.push(compose_up_command(request));
    lines.push(pin_image_tag(host_dir, request.tag));
    lines.push(stamp("compose done"));
    // Every deploy leaves the previous tag's images unused, its build layers cached and its smoke containers stopped; without this the
    // host disk filled up (95%). Only above 70% used, because the prune itself takes tens of seconds that the redeploy would otherwise
    // wait for. The build cache is kept to 6 GB, the newest layers.
    lines.push(format!(r#"flock -x {DOCKER_LOCK} sh -c '[ "$(df --output=pcent / | tail -1 | tr -dc 0-9)" -lt 70 ] || {{ docker image prune -af --filter until=1h; docker builder prune -f --keep-storage 6GB; docker container prune -f --filter until=1h; }}' >/dev/null 2>&1 || true"#));
    app_sibling_argv(env, request, &lines)
}

/// Foreground sibling: the pulled image of an app runs as a throwaway container on the spare port, in the netns of the running one,
/// passes its probe from inside and is left running, so the caller can reach it over the tailnet. `None` for an app with no smoke test.
pub fn app_smoke_sibling_args(env: &RedeployEnv, request: &AppRedeploy) -> Option<Vec<String>> {
    let check = smoke(request.app)?;
    let mut lines = app_front_lines(env, request);
    // A first deploy has no sidecar yet, and the throwaway container joins the sidecar's network namespace.
    let compose = format!("docker compose -f {}", request.compose_file);
    lines.push(format!("cd {} && {compose} up -d --no-deps $({compose} config --services | grep -vx {})", request.host_dir, check.service));
    lines.extend(smoke_lines(&format!("cd {}", request.host_dir), &format!("docker compose -f {}", request.compose_file), request.app, check.service, check.probe, check.port_env, check.env));
    Some(app_sibling_argv(env, request, &lines))
}

/// Foreground sibling: removes the throwaway container `app_smoke_sibling_args` left running.
pub fn smoke_stop_sibling_args(env: &RedeployEnv, name: &str) -> Vec<String> {
    let mut args = strings(&["run", "--pull", "missing", "--rm", "-v", DOCKER_SOCKET]);
    args.push(env.sibling_image());
    args.extend(strings(&["sh", "-c"]));
    args.push(format!("docker rm -f {name}-smoke >/dev/null 2>&1 || true; echo '[smoke] {name} stopped'"));
    args
}

/// Foreground sibling: the pulled image of this service runs once as a throwaway container on the spare port, in the netns of the
/// running one with its env and mounts, and must answer `--healthcheck`. Its exit code is the answer;
/// nothing is swapped.
pub fn self_smoke_sibling_args(env: &RedeployEnv, tag: &str, reference: &str) -> Vec<String> {
    let vm_dir = env.vm_dir.as_str();
    let stage = format!("{vm_dir}/.smoke-staging");
    let files = [
        (raw_url("infra/oracle-cloud/vm-ops-api/docker-compose.yml", reference), format!("{stage}/docker-compose.yml")),
        (raw_url("infra/oracle-cloud/vm-ops-api/.env.defaults", reference), format!("{stage}/.env.defaults")),
    ];
    let mut lines = vec!["set -e".to_string(), stamp("start"), format!("trap 'rm -rf {stage}' EXIT"), format!("rm -rf {stage}"), format!("mkdir -p {stage}")];
    lines.extend(provision_lines("vm-ops-api'\"'\"'s own compose file and .env.defaults", reference, &files));
    lines.extend(sync_env_lines(&stage, "vm-ops-api", &env.host_label, vm_dir));
    lines.push(format!("( cd {stage} && docker compose config -q )"));
    lines.push(format!("( cd {stage} && docker compose pull ops-api )"));
    lines.push(stamp("pulled"));
    lines.extend(smoke_lines(&format!("cd {stage}"), "docker compose", "vm-ops-api", "ops-api", "vm-ops-api-rs --healthcheck", "PORT", &[("CI_PORT", &SMOKE_CI_PORT.to_string())]));
    let mut args = strings(&["run", "--pull", "missing", "--rm", "-v", DOCKER_SOCKET, "-v"]);
    args.push(format!("{vm_dir}:{vm_dir}"));
    args.extend(strings(&["-v", BWS]));
    args.extend(strings(&["-v", CLIS, "-w", vm_dir]));
    args.extend(sibling_env_args(tag, env));
    args.push(env.sibling_image());
    args.extend(strings(&["sh", "-c"]));
    args.push(lines.join("\n"));
    args
}

/// Detached sibling: `compose up` replaces this very container, so it reports the outcome itself. New files are staged and checked
/// first, a failed swap puts the `.prev` copies back, and a marker lets the next run undo a swap killed mid-rename.
pub fn self_redeploy_sibling_args(env: &RedeployEnv, tag: &str, reference: &str, requested_by: &str) -> Vec<String> {
    let vm_dir = env.vm_dir.as_str();
    let emit = format!("wf log emit --service vm-ops-api --host-label \"{}\" --attr tag={tag}", env.host_label);
    let (stage, marker) = (format!("{vm_dir}/.staging"), format!("{vm_dir}/.swap-pending"));
    let running = env.running_tag();
    let mut staged = vec![
        format!("{{ [ ! -f {marker} ] || {{ echo 'a swap was interrupted: putting the previous files back'; rollback && rm -f {marker}; }}; }}"),
        format!("mkdir -p {stage}"),
    ];
    let files = [
        (raw_url("infra/oracle-cloud/vm-ops-api/docker-compose.yml", reference), format!("{stage}/docker-compose.yml")),
        (raw_url("infra/oracle-cloud/vm-ops-api/.env.defaults", reference), format!("{stage}/.env.defaults")),
    ];
    staged.extend(provision_lines("vm-ops-api'\"'\"'s own compose file and .env.defaults", reference, &files));
    staged.extend(sync_env_lines(&stage, "vm-ops-api", &env.host_label, vm_dir));
    staged.push(format!("( cd {stage} && docker compose config -q )"));
    staged.push(format!("( cd {stage} && docker compose pull ops-api )"));
    let mut swapped = vec![format!(": > {marker}")];
    swapped.extend(SWAP_FILES.iter().map(|file| format!("mv -f {stage}/{file} {vm_dir}/{file}")));
    swapped.push(format!("rm -f {marker}"));
    swapped.push(format!("( cd {vm_dir} && docker compose up -d --wait --wait-timeout {UP_WAIT_SECONDS} )"));
    swapped.push(pin_image_tag(vm_dir, tag));
    let swap_paths = SWAP_FILES.iter().map(|file| format!("{vm_dir}/{file}")).collect::<Vec<_>>().join(" ");
    let keeps = SWAP_FILES.iter().map(|file| format!("keep {vm_dir}/{file}")).collect::<Vec<_>>().join(" && ");
    // Restores the stack that was running: its tag when this service knows it, else the old .env's pin or `latest`.
    let restore_up = if running.is_empty() { "unset IMAGE_TAG && docker compose up -d".to_string() } else { format!("IMAGE_TAG={running} docker compose up -d") };
    let requested = js::stringify(&Value::from(requested_by));
    let cmd = [
        "date -u +%FT%TZ".to_string(),
        format!("echo 'pulling tag={tag}'"),
        // the env sync unsets the OTLP variables so compose reads the new .env; the outcome event is emitted after that
        "OTLP_HOST=${BETTERSTACK_OTLP_HOST:-}; OTLP_TOKEN=${BETTERSTACK_OTLP_TOKEN:-}".to_string(),
        format!("{emit} --level info --message redeploy.started || true"),
        format!("trap 'rm -rf {stage}' EXIT"),
        format!("rm -rf {stage}"),
        "REDEPLOY_EXIT=0; RESTORED=none".to_string(),
        format!("rollback() {{ for f in {swap_paths}; do [ ! -f \"$f.prev\" ] || mv -f \"$f.prev\" \"$f\" || return 1; done; }}"),
        r#"keep() { [ ! -f "$1" ] || { cp -p "$1" "$1.prev.tmp" && mv -f "$1.prev.tmp" "$1.prev"; } || { rm -f "$1.prev.tmp"; return 1; }; }"#.to_string(),
        format!("{{\n{}\n}} || REDEPLOY_EXIT=$?", staged.join(" &&\n")),
        format!("if [ \"$REDEPLOY_EXIT\" = 0 ]; then {{ {keeps}; }} || REDEPLOY_EXIT=$?; fi"),
        format!("if [ \"$REDEPLOY_EXIT\" = 0 ]; then\n{{\n{}\n}} || REDEPLOY_EXIT=$?", swapped.join(" &&\n")),
        "  if [ \"$REDEPLOY_EXIT\" != 0 ]; then".to_string(),
        format!("    if rollback; then rm -f {marker}; if ( cd {vm_dir} && {restore_up} ); then RESTORED=ok; else RESTORED=failed; echo \"restore: docker compose up -d failed\"; fi; else RESTORED=failed; echo \"restore: moving the .prev files back failed\"; fi"),
        "  fi".to_string(),
        "fi".to_string(),
        r#"echo "exit=$REDEPLOY_EXIT restored=$RESTORED""#.to_string(),
        r#"if [ "$REDEPLOY_EXIT" = "0" ]; then LOGGER_LEVEL=info; LOGGER_MSG=redeploy.succeeded; else LOGGER_LEVEL=error; LOGGER_MSG=redeploy.failed; fi"#.to_string(),
        format!("BETTERSTACK_OTLP_HOST=\"$OTLP_HOST\" BETTERSTACK_OTLP_TOKEN=\"$OTLP_TOKEN\" {emit} --level \"$LOGGER_LEVEL\" --message \"$LOGGER_MSG\" --attr exitCode=\"$REDEPLOY_EXIT\" --attr restored=\"$RESTORED\" || true"),
        format!("printf '{{\"ts\":\"%s\",\"key\":\"vm-ops-api\",\"requestedBy\":{requested},\"tag\":\"{tag}\",\"ref\":\"{reference}\",\"outcome\":\"%s\",\"exitCode\":%s,\"restored\":\"%s\"}}\\n' \"$(date -u +%FT%TZ)\" \"$LOGGER_MSG\" \"$REDEPLOY_EXIT\" \"$RESTORED\" >> {REDEPLOY_HISTORY_FILE}"),
        history_cap(),
        r#"exit "$REDEPLOY_EXIT""#.to_string(),
    ]
    .join("\n");
    let mut args = strings(&["run", "--pull", "always", "--rm", "-d", "-v", DOCKER_SOCKET, "-v"]);
    args.push(format!("{vm_dir}:{vm_dir}"));
    args.extend(strings(&["-v", BWS]));
    // the image's /usr/local/bin/wf links into this volume (the VM's one wf build); without it `wf secret sync-env` is not found
    args.extend(strings(&["-v", CLIS, "-w", vm_dir]));
    args.extend(sibling_env_args(tag, env));
    args.extend(["-e".to_string(), format!("BETTERSTACK_OTLP_HOST={}", env.otlp_host.as_deref().unwrap_or("")), "-e".to_string(), format!("BETTERSTACK_OTLP_TOKEN={}", env.otlp_token.as_deref().unwrap_or(""))]);
    args.push(env.sibling_image());
    args.extend(strings(&["sh", "-c"]));
    args.push(format!("{{\n{cmd}\n}} > {REDEPLOY_LOG} 2>&1"));
    args
}

/// Appends one line to the history and keeps its last 200, from a sibling that mounts the compose dir.
pub fn append_history_args(vm_dir: &str, image: &str, entry: &HistoryEntry, now_ms: i64) -> Vec<String> {
    let mut line = Map::new();
    line.insert("ts".to_string(), json!(js::iso_string(now_ms)));
    line.insert("key".to_string(), json!(entry.key));
    line.insert("requestedBy".to_string(), json!(entry.requested_by));
    line.insert("tag".to_string(), json!(entry.tag));
    line.insert("ref".to_string(), json!(entry.reference));
    line.insert("outcome".to_string(), json!(entry.outcome));
    line.insert("exitCode".to_string(), entry.exit_code.map_or(Value::Null, Value::from));
    let line = js::stringify(&Value::Object(line)).replace('\'', "'\\''");
    let cmd = format!("printf '%s\\n' '{line}' >> {REDEPLOY_HISTORY_FILE} && {}", history_cap());
    let mut args = strings(&["run", "--rm", "-v"]);
    args.push(format!("{vm_dir}:{vm_dir}"));
    args.extend(strings(&["-w", vm_dir, image, "sh", "-c"]));
    args.push(cmd);
    args
}

/// The pairing command of the deployed orca-serve container, as the one fixed script of `POST /services/orca-serve/pair`: it takes
/// no argument from a request, and the link it prints is a credential.
pub const PAIR_SCRIPT: &str = "docker exec orca-serve orca-serve pair --endpoint ws://orca-serve:6768";

pub fn pair_sibling_args(env: &RedeployEnv) -> Vec<String> {
    let mut args = strings(&["run", "--pull", "missing", "--rm", "-v", DOCKER_SOCKET]);
    args.push(env.sibling_image());
    args.extend(strings(&["sh", "-c"]));
    args.push(PAIR_SCRIPT.to_string());
    args
}

/// Takes down a compose project by name for an app that is no longer in the app list (its compose file is gone from the repo, so
/// nothing else can). `--volumes` also drops its tailscale state, which retires the tailnet node's key.
pub fn retire_sibling_args(env: &RedeployEnv, app: &str) -> Vec<String> {
    let mut args = strings(&["run", "--pull", "missing", "--rm", "-v", DOCKER_SOCKET]);
    args.push(env.sibling_image());
    args.extend(strings(&["sh", "-c"]));
    args.push(format!("docker compose -p {app} down --volumes --remove-orphans"));
    args
}
