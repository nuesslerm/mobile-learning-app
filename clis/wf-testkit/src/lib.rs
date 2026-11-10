//! The doubles the flow tests of the `wf` crates share: run the binary under test with stub tools first on its PATH, and stand up
//! the services it talks to. The code lives in `src` so a change to a double reruns the tests of every crate that uses it.
mod aux;
mod mock_server;
mod mock_tailnet;
pub mod otlp;
pub mod pki;
mod run;
mod slot_sandbox;
mod stubs;
#[cfg(feature = "tls")]
mod mock_https;

#[cfg(feature = "tls")]
pub use mock_https::{HttpsRequest, MockHttps, Rule};
pub use aux::Aux;
pub use mock_server::{MockReply, MockRequest, MockServer};
pub use mock_tailnet::{MockTailnet, Upstream};
pub use run::{node_path, node_script, run_wf, spawn_bin, spawn_wf, wait_until, wf_binary, RunOpts, RunResult, Spawned};
pub use slot_sandbox::SlotSandbox;
pub use stubs::{create_sh_stub, create_stub, with_stubs};

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// The repository root: two directories above this crate, which is the same for every crate beside it.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

/// A fresh ed25519 private key as one line (`\\n` for newlines), the form `VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY` is given in.
pub fn ed25519_private_key_one_line() -> String {
    pki::ed25519_pem_pair().0.trim_end().replace('\n', "\\n")
}

/// Milliseconds since the Unix epoch, now.
pub fn now_ms() -> i128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i128
}

/// Milliseconds since the Unix epoch as a JavaScript `toISOString()` text (`2026-10-10T08:41:09.618Z`).
pub fn iso_from_ms(ms: i128) -> String {
    let (secs, millis) = (ms.div_euclid(1000), ms.rem_euclid(1000));
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i128::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// The last stdout line that parses as a JSON object (the `--json` envelope or a summary line), or panics with the output.
pub fn last_json(r: &RunResult) -> serde_json::Value {
    r.stdout
        .lines()
        .rev()
        .filter(|l| l.trim_start().starts_with('{'))
        .find_map(|l| serde_json::from_str(l).ok())
        .unwrap_or_else(|| panic!("no JSON line in stdout {:?} (stderr {:?})", r.stdout, r.stderr))
}
