//! The smoke gate of a deploy, between the build and the swap: the pulled image runs as a throwaway container on the spare port, beside the
//! live one and in its netns, and `wf` itself asks it for `/health` over the tailnet, the way a client would. The container is removed
//! whatever the answer was; a failure ends the deploy with the container's log or the probe's error, and nothing was swapped.
use std::time::{Duration, Instant};
use wf_vmops::services::{probe_self_smoke, smoke_app, smoke_health_url, smoke_self, smoke_stop_app, smoke_stop_self, VM_OPS_API};
use wf_vmops::vmops::tailnet_via;

const PROBE_TRIES: u32 = 3;
const PROBE_PAUSE: Duration = Duration::from_secs(2);

fn phase(name: &str, since: Instant) {
    eprintln!("phase {name}: {} s", since.elapsed().as_secs());
}

/// One answer of the throwaway container over the tailnet: vm-ops-api's is a signed `/health`, an app's its own `/health`; an app with
/// none (`Ok(None)`) has only the probe from inside the container.
fn probe(app: &str) -> Result<Option<String>, String> {
    let mut last = String::new();
    for attempt in 0..PROBE_TRIES {
        if attempt > 0 {
            std::thread::sleep(PROBE_PAUSE);
        }
        let answered = if app == VM_OPS_API {
            probe_self_smoke().map(Some)
        } else {
            let Some(url) = smoke_health_url(app) else { return Ok(None) };
            cli_http::Request::get(&url).via(tailnet_via()).timeout(Duration::from_secs(5)).send().and_then(|response| {
                if (200..300).contains(&response.status) { Ok(Some(response.text())) } else { Err(format!("HTTP {}", response.status)) }
            })
        };
        match answered {
            Ok(body) => return Ok(body),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// The one line every deploy ends its smoke gate with, so each app reads the same way: what ran inside the container and over the tailnet.
fn verdict(app: &str, inside: &str, tailnet: &str) {
    eprintln!("smoke: {}", serde_json::json!({ "app": app, "inside": inside, "tailnet": tailnet }));
}

/// Runs the gate for `app` at `commit` and `tag`.
pub(crate) fn gate(app: &str, commit: &str, tag: &str) -> Result<(), String> {
    let started = Instant::now();
    let up = if app == VM_OPS_API { smoke_self(commit, tag) } else { smoke_app(app, commit, tag) }?;
    phase("smoke test", started);
    if up["stdout"].as_str().is_some_and(|out| out.contains("has no smoke test")) {
        verdict(app, "none", "none");
        return Ok(());
    }
    let probed = Instant::now();
    let answer = probe(app);
    let stopped = if app == VM_OPS_API { smoke_stop_self() } else { smoke_stop_app(app) };
    if let Err(error) = stopped {
        wf_kit::wf_err!("wf: the smoke container of {app} was not removed: {error}");
    }
    phase("tailnet probe", probed);
    match answer {
        Ok(Some(_)) => verdict(app, "passed", "passed"),
        Ok(None) => verdict(app, "passed", "none"),
        Err(error) => {
            verdict(app, "passed", "failed");
            return Err(format!("the smoke container of {app} passed its probe inside but did not answer over the tailnet: {error}; nothing was swapped"));
        }
    }
    Ok(())
}
