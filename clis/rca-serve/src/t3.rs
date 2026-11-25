use rca_mobile_core::Failure;
use serde_json::json;
use std::{env, path::Path};
use t3_serve::wiring;

fn setting(name: &str) -> Result<String, Failure> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Failure::new("CONFIG", format!("{name} is required")))
}

pub fn serve(dir: &Path) -> Result<(), Failure> {
    let upstream = setting("RCA_MOBILE_UPSTREAM")?;
    let authority = upstream
        .strip_prefix("ws://")
        .map(|s| s.trim_end_matches('/'))
        .filter(|s| !s.is_empty() && !s.contains(['/', '@', '?', '#']))
        .ok_or_else(|| Failure::new("CONFIG", "Expected a ws:// RCA host and port"))?;
    let settings = wiring::Settings {
        state_dir: dir.join("t3"),
        label: env::var("RCA_MOBILE_LABEL").unwrap_or_else(|_| "rca".into()),
        catalog_file: None,
        catalog_inline: None,
        rca_url: Some(format!("http://{authority}")),
        rca_bearer: Some(setting("RCA_MOBILE_BEARER")?),
        rca_workspace: Some(setting("RCA_MOBILE_WORKSPACE")?),
        guest_host: None,
    };
    let address = env::var("RCA_MOBILE_BIND").unwrap_or_else(|_| "127.0.0.1:3775".into());
    let (listener, environment) =
        wiring::listen(&settings, &address).map_err(|e| Failure::new("SERVE", e))?;
    println!(
        "{}",
        json!({"ready":true,"address":listener.addr().to_string(),
        "environmentId":environment,"orchestrationProtocolVersion":2})
    );
    listener.wait();
    Ok(())
}

pub fn pair(dir: &Path) -> Result<(), Failure> {
    let base = setting("RCA_MOBILE_PUBLIC_URL")?;
    let link = wiring::pair_link(
        &dir.join("t3"),
        base.trim_end_matches('/'),
        Default::default(),
    )
    .map_err(|e| Failure::new("PAIR", e))?;
    println!("{link}");
    Ok(())
}
