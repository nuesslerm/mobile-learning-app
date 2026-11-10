//! The deployer reads behind `wf ship --status` and `wf ship retire` (`ship.rs` routes them here); image builds are in `deploy_run`,
//! and the planning step CI runs is in `pipeline`.
use wf_vmops::services::{ps, RECOVER_APPS};
use wf_vmops::vmops::{encode_uri_component, request_json, Opts};
use serde_json::{json, Map, Value};

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("json")
}

pub(crate) fn status(apps: &[String]) -> Result<Option<i32>, String> {
    let list: Vec<String> = if apps.is_empty() {
        RECOVER_APPS.iter().map(|s| s.to_string()).collect()
    } else {
        apps.to_vec()
    };
    let mut services = Map::new();
    for app in &list {
        let entry = match ps(app) {
            Ok(containers) => Value::Array(containers),
            Err(e) => json!({ "error": e }),
        };
        services.insert(app.clone(), entry);
    }
    let health = request_json("/health", &Opts::get())?;
    let redeploy = request_json(
        "/redeploy/status",
        &Opts {
            timeout_sec: 30,
            ..Opts::get()
        },
    )?;
    let report =
        json!({ "health": health, "redeploy": redeploy, "services": Value::Object(services) });
    println!("{}", pretty(&report));
    Ok(Some(0))
}

pub(crate) fn retire(app: &str) -> Result<Option<i32>, String> {
    let v = request_json(
        &format!("/services/{}/retire", encode_uri_component(app)),
        &Opts {
            timeout_sec: 180,
            ..Opts::post()
        },
    )?;
    println!("{}", pretty(&v));
    Ok(Some(0))
}
