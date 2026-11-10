//! `WF_VM_SAMPLE_SECS=<n>`: a device run samples the VM from vm-ops-api's `/stats` every n seconds (host load and MemAvailable,
//! the device container's memory and CPU) and its report carries the samples, so a slow unit can be laid beside the VM's state.
use crate::vmops::{request_json, Opts};
use nix::sys::signal::{pthread_sigmask, SigSet, SigmaskHow, Signal};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct Sampler {
    every_s: u64,
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<Value>>>,
    thread: Option<JoinHandle<()>>,
    started: Instant,
    container: String,
}

/// Starts sampling when `WF_VM_SAMPLE_SECS` is a positive number. The device is counted by `WF_ANDROID_CONTAINER`, default
/// `agent-shell-dind`: on the VM the device runs inside that Docker host, which `/stats` sees (image builds wait while it is up).
pub fn start_from_env() -> Option<Sampler> {
    let every_s = std::env::var("WF_VM_SAMPLE_SECS").ok()?.trim().parse::<u64>().ok().filter(|n| *n > 0)?;
    let container = std::env::var("WF_ANDROID_CONTAINER").unwrap_or_else(|_| "agent-shell-dind".into());
    let (stop, samples, started) = (Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(Vec::new())), Instant::now());
    let thread = {
        let (stop, samples, container) = (stop.clone(), samples.clone(), container.clone());
        std::thread::spawn(move || {
            // a child's SIGCHLD that the main thread holds back (libc system() for Lua's os.execute, posix_spawn) is handed to a
            // thread that takes it, and here it cut the sample's /stats call short with EINTR (A6, 20 Oct)
            let quiet = SigSet::from_iter([Signal::SIGCHLD, Signal::SIGINT, Signal::SIGTERM]);
            let _ = pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&quiet), None);
            while !stop.load(Ordering::SeqCst) {
                let s = sample(started, &container);
                if let Ok(mut v) = samples.lock() {
                    v.push(s);
                }
                let next = Instant::now() + Duration::from_secs(every_s);
                while Instant::now() < next && !stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        })
    };
    Some(Sampler { every_s, stop, samples, thread: Some(thread), started, container })
}

fn sample(started: Instant, container: &str) -> Value {
    let t_ms = started.elapsed().as_millis() as u64;
    match request_json("/stats", &Opts { timeout_sec: 10, ..Opts::get() }) {
        Ok(v) => {
            let device = v["containers"].as_array().and_then(|cs| cs.iter().find(|c| c["name"] == container)).map(|c| json!({ "memMb": c["memMb"], "cpuPct": c["cpuPct"] }));
            json!({ "tMs": t_ms, "load1": v["host"]["load"][0], "memAvailableMb": v["host"]["memAvailableMb"], "device": device })
        }
        Err(e) => json!({ "tMs": t_ms, "error": e }),
    }
}

impl Sampler {
    /// Stops the sampler, takes one last sample so the run's end is covered, and returns the samples with their extremes.
    pub fn finish(mut self) -> Value {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let mut samples = self.samples.lock().map(|v| v.clone()).unwrap_or_default();
        samples.push(sample(self.started, &self.container));
        let nums = |f: &dyn Fn(&Value) -> Option<f64>| samples.iter().filter_map(f).collect::<Vec<f64>>();
        let max = |xs: Vec<f64>| xs.into_iter().reduce(f64::max);
        let min = |xs: Vec<f64>| xs.into_iter().reduce(f64::min);
        json!({
            "everyS": self.every_s,
            "container": self.container,
            "load1Max": max(nums(&|s| s["load1"].as_f64())),
            "memAvailableMinMb": min(nums(&|s| s["memAvailableMb"].as_f64())),
            "deviceMemMaxMb": max(nums(&|s| s["device"]["memMb"].as_f64())),
            "deviceCpuMaxPct": max(nums(&|s| s["device"]["cpuPct"].as_f64())),
            "errors": samples.iter().filter(|s| s.get("error").is_some()).count(),
            "samples": samples,
        })
    }
}
