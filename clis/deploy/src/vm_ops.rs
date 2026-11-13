//! `wf vm ops health|stats|host|logs|probe|pair|ci-port|runner-check|token-permissions|cancel-run|install-clis` and `wf vm ledger cat`: the commands that
//! talk to vm-ops-api.
use wf_kit::args::parse;
use wf_vmops::services::health_url;
use wf_vmops::vmops::{encode_uri_component, request_json, request_raw, Opts};
use serde_json::{json, Value};

/// The TypeScript prints a WfError as `wf: <message>` and exits 1.
fn fail(message: &str) -> Option<i32> {
    wf_kit::wf_err!("wf: {message}");
    Some(1)
}

/// `String(Number(s))` for a `--tail` value.
fn js_number_string(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return "0".into();
    }
    match t.parse::<f64>() {
        Ok(n) if n.is_finite() && n.fract() == 0.0 => format!("{}", n as i64),
        Ok(n) if n.is_finite() => format!("{n}"),
        _ => "NaN".into(),
    }
}

fn health() -> Option<i32> {
    match request_json("/health", &Opts::get()) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).expect("json"));
            Some(0)
        }
        Err(e) => fail(&e),
    }
}

/// What holds the VM's memory and CPU: the host's numbers and each running container's working set, biggest first.
fn stats() -> Option<i32> {
    match request_json("/stats", &Opts { timeout_sec: 60, ..Opts::get() }) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).expect("json"));
            Some(0)
        }
        Err(e) => fail(&e),
    }
}

fn logs(app: &str, tail: &str) -> Option<i32> {
    let path = format!(
        "/services/{app}/logs?tail={}",
        encode_uri_component(&js_number_string(tail))
    );
    let result = match request_json(
        &path,
        &Opts {
            timeout_sec: 60,
            ..Opts::get()
        },
    ) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    // `logs` holds each of the app's containers by name, its tailscale sidecar included, so every text gets a header
    let Some(by_name) = result["logs"].as_object() else {
        return fail(&format!("vm-ops-api {path} answered without a logs object: {result}"));
    };
    let sections: Vec<String> = by_name
        .iter()
        .map(|(name, text)| format!("==> {name} <==\n{}", text.as_str().unwrap_or_default().trim_end()))
        .collect();
    println!("{}", sections.join("\n\n"));
    Some(0)
}

struct Probed {
    app: String,
    ms: u128,
    error: Option<String>,
}

const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const PROBE_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// One GET of the app's /health at the tailnet name it answers to now, the way a test's baked-in host reaches it.
fn probe_app(app: &str, health_url: &str) -> Probed {
    let started = std::time::Instant::now();
    let (host, tail) = health_url.strip_prefix("http://").and_then(|rest| rest.split_once(':')).unwrap_or_default();
    let reached = match wf_vmops::tailnet_resolve::answering(host) {
        None => Err(format!("no tailnet name answers for {host}")),
        Some(name) => {
            let url = format!("http://{name}:{tail}");
            match cli_http::Request::get(&url).via(wf_vmops::vmops::tailnet_via()).timeout(PROBE_TIMEOUT).send() {
                Ok(response) if response.ok() => Ok(()),
                Ok(response) => Err(format!("{url} answered HTTP {}", response.status)),
                Err(e) => Err(format!("{url}: {e}")),
            }
        }
    };
    Probed { app: app.into(), ms: started.elapsed().as_millis(), error: reached.err() }
}

/// What the app's containers are doing, from vm-ops-api: the evidence for an app that does not answer.
fn container_states(app: &str) -> String {
    match wf_vmops::services::ps(app) {
        Ok(containers) if containers.is_empty() => "none".into(),
        Ok(containers) => containers.iter().map(|c| format!("{}={}", c["name"].as_str().unwrap_or("?"), c["state"].as_str().unwrap_or("?"))).collect::<Vec<_>>().join(", "),
        Err(e) => format!("unreadable ({e})"),
    }
}

/// `wf vm ops probe [--wait <s>] <app>...`: every app's /health answers 2xx on the tailnet, or exit 1 naming the ones that do not.
fn probe(apps: &[String], wait: u64) -> Option<i32> {
    let mut urls = Vec::new();
    for app in apps {
        match health_url(app) {
            Some(url) => urls.push(url),
            None => return fail(&format!("{app} has no /health on the tailnet to probe")),
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
    let mut probed: Vec<Probed> = apps.iter().zip(&urls).map(|(app, url)| probe_app(app, url)).collect();
    while probed.iter().any(|p| p.error.is_some()) && std::time::Instant::now() + PROBE_EVERY <= deadline {
        std::thread::sleep(PROBE_EVERY);
        for (p, url) in probed.iter_mut().zip(&urls).filter(|(p, _)| p.error.is_some()) {
            *p = probe_app(&p.app, url);
        }
    }
    let down: Vec<(&Probed, String)> = probed.iter().filter(|p| p.error.is_some()).map(|p| (p, container_states(&p.app))).collect();
    let rows: Vec<Value> = probed
        .iter()
        .map(|p| match down.iter().find(|(d, _)| d.app == p.app) {
            None => json!({ "app": p.app, "ok": true, "ms": p.ms }),
            Some((_, containers)) => json!({ "app": p.app, "ok": false, "error": p.error, "containers": containers, "ms": p.ms }),
        })
        .collect();
    let mut report = json!({ "ok": down.is_empty(), "apps": rows });
    if let Some((first, _)) = down.first() {
        let names: Vec<&str> = down.iter().map(|(p, _)| p.app.as_str()).collect();
        report["unhealthy"] = json!(names);
        report["next"] = json!(format!("wf ship --status {} (container states), wf vm ops logs {} (why); a push to dev redeploys", names.join(" "), first.app));
    }
    println!("{report}");
    if down.is_empty() {
        return Some(0);
    }
    let detail: Vec<String> = down.iter().map(|(p, containers)| format!("{} ({}; containers: {containers})", p.app, p.error.as_deref().unwrap_or_default())).collect();
    fail(&format!("not answering /health: {}", detail.join("; ")))
}

/// The route the check asks the CI port for: one only the admin port serves, read-only.
const ADMIN_ONLY_ROUTE: &str = "/services/agent-shell/logs?tail=1";

/// `wf vm ops ci-port`: as a CI job (`WF_VM_CI_PORT` set), `/health` answers 200 on the CI port with the CI seed and an admin-only route
/// answers 404 there. One JSON line; exit 1 when either differs or the port is not set.
fn ci_port_check() -> Option<i32> {
    use wf_vmops::vmops::{ci_port, send_on, Lane};
    let Some(port) = ci_port() else {
        wf_kit::wf_err!("{}", json!({ "ok": false, "error": "WF_VM_CI_PORT is not set, so every call of this process goes to the admin port", "next": "WF_VM_CI_PORT=8081 wf vm ops ci-port" }));
        return Some(1);
    };
    let timeout = std::time::Duration::from_secs(10);
    let checks: Vec<Value> = [("/health", 200), (ADMIN_ONLY_ROUTE, 404)]
        .iter()
        .map(|(path, expected)| match send_on(Lane::Ci, port, "GET", path, None, timeout) {
            Ok(reply) => json!({ "route": format!("GET {path}"), "expects": expected, "status": reply.status, "ok": reply.status == *expected }),
            Err(e) => json!({ "route": format!("GET {path}"), "expects": expected, "status": null, "ok": false, "error": e }),
        })
        .collect();
    let ok = checks.iter().all(|c| c["ok"] == json!(true));
    println!("{}", json!({ "ok": ok, "port": port, "checks": checks }));
    Some(if ok { 0 } else { 1 })
}

/// The one app vm-ops-api mints a pairing link for.
const PAIRABLE_APP: &str = "orca-serve";

/// A failed pair as `{ok:false,error,next?}` on stderr and exit 1. `error` never carries a reply body: a reply may hold the link.
fn pair_fail(error: &str, next: Option<&str>) -> Option<i32> {
    let mut report = json!({ "ok": false, "error": error });
    if let Some(next) = next {
        report["next"] = json!(next);
    }
    wf_kit::wf_err!("{report}");
    Some(1)
}

/// `wf vm ops pair orca-serve`: the pairing link on stdout and nothing else there, as it is a credential to be piped.
fn pair(app: &str) -> Option<i32> {
    if app != PAIRABLE_APP {
        return pair_fail(&format!("{app} cannot be paired; only {PAIRABLE_APP}"), Some("wf vm ops pair orca-serve"));
    }
    let timeout = std::time::Duration::from_secs(60);
    let reply = match wf_vmops::vmops::send(wf_vmops::vmops::PORT, "POST", &format!("/services/{PAIRABLE_APP}/pair"), None, timeout) {
        Ok(reply) => reply,
        Err(e) => return pair_fail(&e, Some("wf vm net up, then wf vm ops health")),
    };
    let body: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
    let url = body["url"].as_str().filter(|url| reply.status == 200 && body["ok"] == json!(true) && url.starts_with("orca://pair"));
    let Some(url) = url else {
        let why = body["error"].as_str().filter(|_| reply.status != 200).unwrap_or("the reply carried no pairing link");
        return pair_fail(&format!("vm-ops-api answered HTTP {}: {why}", reply.status), Some("wf vm ops logs orca-serve, wf vm ops probe orca-serve"));
    };
    println!("{url}");
    eprintln!("wf: minted an orca pairing link for {PAIRABLE_APP} (a credential: stdout only, pipe it to wf secret update)");
    Some(0)
}

/// `args` follow `vm ops`.
pub fn run(args: &[String]) -> Option<i32> {
    let (sub, rest) = args.split_first()?;
    match sub.as_str() {
        "health" => parse(rest, &[], &[])
            .filter(|p| p.positional.is_empty())
            .and_then(|_| health()),
        "stats" => parse(rest, &[], &[]).filter(|p| p.positional.is_empty()).and_then(|_| stats()),
        "logs" => {
            let p = parse(rest, &["tail"], &[]).filter(|p| p.positional.len() == 1)?;
            logs(
                &p.positional[0],
                p.opts.get("tail").map_or("200", String::as_str),
            )
        }
        "probe" => {
            let p = parse(rest, &["wait"], &[]).filter(|p| !p.positional.is_empty())?;
            probe(&p.positional, p.opts.get("wait").and_then(|w| w.parse().ok()).unwrap_or(0))
        }
        "pair" => parse(rest, &[], &[]).filter(|p| p.positional.len() == 1).and_then(|p| pair(&p.positional[0])),
        "ci-port" => parse(rest, &[], &[]).filter(|p| p.positional.is_empty()).and_then(|_| ci_port_check()),
        "runner-check" => {
            let p = parse(rest, &["proc-root", "docker-sock-path"], &["json"]).filter(|p| p.positional.is_empty())?;
            crate::runner_check::run(p.opts.get("proc-root").map(String::as_str), p.opts.get("docker-sock-path").map(String::as_str))
        }
        "token-permissions" => parse(rest, &[], &[]).filter(|p| p.positional.is_empty()).and_then(|_| crate::token_permissions::run()),
        "cancel-run" => {
            let p = parse(rest, &["run"], &["queued", "all"]).filter(|p| p.positional.is_empty())?;
            let (queued, all) = (p.flags.iter().any(|f| f == "queued"), p.flags.iter().any(|f| f == "all"));
            match (p.opts.get("run"), queued, all) {
                (Some(n), false, false) => n.parse::<u64>().ok().and_then(|n| crate::cancel_run::run(crate::cancel_run::Target::Run(n))),
                (None, true, false) => crate::cancel_run::run(crate::cancel_run::Target::Queued),
                (None, false, true) => crate::cancel_run::run(crate::cancel_run::Target::All),
                _ => None,
            }
        }
        "install-clis" => {
            let p = parse(rest, &["sha256"], &[]).filter(|p| p.positional.is_empty())?;
            crate::install_clis::run(p.opts.get("sha256")?)
        }
        "host" => crate::host_history::run(rest),
        "drill" => crate::vm_drill::run(rest),
        _ => None,
    }
}

/// `wf vm ledger cat <name>`: a file of the agent-shell ledger, verbatim with one trailing newline. `args` follow `ledger`.
pub fn ledger(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    if verb != "cat" {
        return None;
    }
    let p = parse(rest, &[], &[]).filter(|p| p.positional.len() == 1)?;
    let path = format!("/services/agent-shell/vm-ledger/{}", encode_uri_component(&p.positional[0]));
    match request_raw(&path, &Opts::get()) {
        Ok(body) => {
            print!("{body}{}", if body.ends_with('\n') { "" } else { "\n" });
            Some(0)
        }
        Err(e) => fail(&e),
    }
}
