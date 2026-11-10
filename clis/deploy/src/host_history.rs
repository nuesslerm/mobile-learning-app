//! `wf vm ops host [--window 1h|24h|7d|30d] [--gap-secs N]`: what the VM's host was doing over a window, from reporting-service's
//! stored samples: when it stopped sampling (a stall), and the worst moment of each stall signal (I/O and memory pressure, load, swap).
use serde_json::json;
use wf_kit::args::parse;
use wf_vmops::host_summary::{fetch, summarize, REPORTING_URL};

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse(args, &["window", "gap-secs"], &[]).filter(|p| p.positional.is_empty())?;
    let window = p.opts.get("window").map_or("24h", String::as_str);
    let gap_secs: i64 = p.opts.get("gap-secs").and_then(|g| g.parse().ok()).unwrap_or(180);
    match fetch(REPORTING_URL, window) {
        Ok(series) => {
            let points = series["points"].as_array().cloned().unwrap_or_default();
            let now = wf_kit::time::now_ms() as i64;
            let mut out = summarize(&points, 0, now, gap_secs * 1000);
            out["window"] = json!(window);
            println!("{}", serde_json::to_string_pretty(&out).expect("json"));
            Some(0)
        }
        Err(e) => {
            wf_kit::wf_err!("wf: {e}");
            Some(1)
        }
    }
}
