mod devices;
mod http;
mod rca;

use devices::SqlDevices;
use rca_mobile_core::Failure;
use serde_json::json;
use std::{env, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!(
            "{}",
            json!({"ok":false,"code":error.code,"error":error.message})
        );
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Failure> {
    let mut args: Vec<_> = env::args().skip(1).collect();
    if args.first().is_some_and(|v| v == "rca-serve") {
        args.remove(0);
    }
    if args.is_empty()
        || args
            .iter()
            .any(|v| matches!(v.as_str(), "help" | "--help" | "-h"))
    {
        println!(
            "{}",
            json!({"usage":"wf rca-serve <serve|pair> [--state-dir DIR]","serve":"RCA_MOBILE_UPSTREAM, RCA_MOBILE_BEARER and optional RCA_MOBILE_BIND (default 127.0.0.1:3775)","pair":"Print a one-use pairing code, valid for five minutes"})
        );
        return Ok(());
    }
    let dir = match &args[1..] {
        [] => env::var_os("RCA_MOBILE_STATE_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state/rca-serve"))
            }),
        [key, path] if key == "--state-dir" => Some(PathBuf::from(path)),
        _ => None,
    }
    .ok_or_else(|| Failure::new("USAGE", "Expected --state-dir DIR"))?;
    let devices = Arc::new(rca_mobile_core::pairing::Pairing {
        store: SqlDevices::open(&dir)?,
        secrets: devices::OsSecrets,
    });
    match args[0].as_str() {
        "pair" => {
            println!("{}", json!({"code":devices.invite()?,"expiresIn":300}));
            Ok(())
        }
        "serve" => {
            let required = |name: &str| {
                env::var(name).map_err(|_| Failure::new("CONFIG", format!("{name} is required")))
            };
            let agent = Arc::new(rca::Rca {
                upstream: required("RCA_MOBILE_UPSTREAM")?,
                bearer: required("RCA_MOBILE_BEARER")?,
            });
            let address = env::var("RCA_MOBILE_BIND").unwrap_or_else(|_| "127.0.0.1:3775".into());
            let listener = tokio::net::TcpListener::bind(&address)
                .await
                .map_err(|e| Failure::new("BIND", e.to_string()))?;
            let bound = listener
                .local_addr()
                .map_err(|e| Failure::new("BIND", e.to_string()))?;
            println!("{}", json!({"ready":true,"address":bound.to_string()}));
            axum::serve(listener, http::router(devices, agent))
                .await
                .map_err(|e| Failure::new("SERVE", e.to_string()))
        }
        _ => Err(Failure::new("USAGE", "Expected serve or pair")),
    }
}
