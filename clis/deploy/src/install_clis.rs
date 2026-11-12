//! `wf vm ops install-clis --sha256 <hex>`: asks vm-ops-api's CI port to install the `wf` build CI published (`wf-latest`, the arm64 tarball)
//! on the VM's `clis` volume, if it is the build with that sha256. A runner that cannot write to the VM's disk (a GitHub-hosted one) calls this
//! after publishing; one JSON line is the answer.
use serde_json::{json, Value};
use std::time::Duration;
use wf_vmops::vmops::send_routed;

const TIMEOUT: Duration = Duration::from_secs(180);

fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn run(sha256: &str) -> Option<i32> {
    if !valid_sha256(sha256) {
        return None;
    }
    match send_routed("POST", &format!("/clis/install?sha256={sha256}"), None, TIMEOUT) {
        Ok(reply) if reply.status == 200 => {
            let body: Value = serde_json::from_str(&reply.text()).unwrap_or(Value::Null);
            println!("{}", json!({ "ok": true, "tree": body["tree"] }));
            Some(0)
        }
        Ok(reply) => {
            let body: Value = serde_json::from_str(&reply.text()).unwrap_or(Value::Null);
            println!("{}", json!({ "ok": false, "status": reply.status, "error": body["error"] }));
            Some(1)
        }
        Err(e) => {
            wf_kit::wf_err!("wf: {e}");
            Some(1)
        }
    }
}
