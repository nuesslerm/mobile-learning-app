//! The lab of the bench journeys: scenarios, baselines and run files in throwaway directories, the stub export and the page it serves.
use crate::support::{repo_root, run_wf, RunOpts, RunResult};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const WEB_SCRIPT: &str = "function a(){return 1}\nfunction b(){return 2}\na();\n";
pub const MAP: &str = r#"{"version":3,"sources":["../../node_modules/zod/index.js","src/app.ts","../../node_modules/zod/index.js"],"mappings":"AAAA;ACCA;ADCA"}"#;
pub const LOG_WEB: &str = "|  🐥 [tamagui]  native charts-spike  ·  2 found  ·  0 opt  ·  0 flat  1577ms\n|  🐥 [tamagui]  web charts-spike  ·  2 found  ·  0 opt  ·  0 flat  23ms\nWeb Bundled 143ms index.js (120 modules)\n";

pub const PAGE: &str = r#"<!doctype html><title>app</title><head><script>const t=Date.now();while(Date.now()-t<120);</script></head>
<body><button data-testid="T1" onclick="this.dataset.n=(+this.dataset.n||0)+1">tap</button>
<button data-testid="V1">v1</button><button data-testid="V2">v2</button><h1 data-testid="H1">ready</h1>
<script>addEventListener('popstate',()=>{if(location.pathname.endsWith('/b')){for(let i=0;i<100;i++)document.body.appendChild(document.createElement('i'));document.body.addEventListener('click',()=>{});}});</script></body>"#;

/// `pnpm ... expo export --platform <p> --output-dir <d>`: a tiny export and the log Expo prints, then a hang the bench must cut short.
pub const EXPORT: &str = r#"#!/bin/bash
platform=""; out=""
while [ $# -gt 0 ]; do
  case "$1" in --platform) platform=$2; shift;; --output-dir) out=$2; shift;; esac
  shift
done
mkdir -p "$out/_expo/static/js/$platform"
if [ "$platform" = web ]; then
  cp "$STUB_DIR/entry.js" "$out/_expo/static/js/web/entry.js"
  cp "$STUB_DIR/entry.js.map" "$out/_expo/static/js/web/entry.js.map"
  { cat "$STUB_DIR/page.html"; echo '<script src="/mla-tamagui/_expo/static/js/web/entry.js"></script>'; } > "$out/index.html"
  cp "$STUB_DIR/page.html" "$out/tap.html"
  cat "$STUB_DIR/log-web.txt"
else
  head -c 4096 /dev/zero > "$out/_expo/static/js/$platform/entry.hbc"
  echo 'iOS Bundled 800ms frontend/mla-tamagui/index.js (90 modules)'
fi
if [ -n "$STUB_CANDIDATE" ] && [ "$CANDIDATE_SLOW" = 1 ]; then sleep 0.5; fi
echo "Exported: $out"
sleep 60
"#;

pub fn committed(dir: &str, suite: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(repo_root().join("clis/app-bench").join(dir).join(format!("{suite}.json"))).unwrap()).unwrap()
}

/// Scenarios, baselines and run files in throwaway directories, and the stub export.
pub struct Lab {
    pub root: tempfile::TempDir,
    pub data: PathBuf,
    pub tmp: PathBuf,
    pub app_rel: String,
}

pub fn relative(from: &Path, to: &Path) -> String {
    let (a, b): (Vec<Component>, Vec<Component>) = (from.components().collect(), to.components().collect());
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".into(); a.len() - common];
    parts.extend(b[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    parts.join("/")
}

#[allow(dead_code)]
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

impl Lab {
    pub fn new() -> Self {
        let root = tempfile::Builder::new().prefix("wf-flow-app-bench-").tempdir().unwrap();
        let (data, tmp, bin, app) = (root.path().join("data"), root.path().join("tmp"), root.path().join("bin"), root.path().join("app"));
        for dir in [data.join("scenarios"), data.join("baselines"), tmp.clone(), bin.clone(), app.clone()] {
            std::fs::create_dir_all(dir).unwrap();
        }
        for (name, text) in [("page.html", PAGE), ("entry.js", WEB_SCRIPT), ("entry.js.map", MAP), ("log-web.txt", LOG_WEB)] {
            std::fs::write(root.path().join(name), text).unwrap();
        }
        std::fs::write(bin.join("pnpm"), EXPORT).unwrap();
        std::fs::set_permissions(bin.join("pnpm"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let app_rel = relative(&repo_root(), &app.canonicalize().unwrap());
        Self { root, data, tmp, app_rel }
    }

    /// The committed scenario of `suite` with these params and runs.
    pub fn scenario(&self, suite: &str, params: Value, runs: u64) {
        let mut file = committed("scenarios", suite);
        file["runs"] = json!(runs);
        file["params"] = params;
        std::fs::write(self.data.join("scenarios").join(format!("{suite}.json")), serde_json::to_string_pretty(&file).unwrap()).unwrap();
    }

    pub fn baseline(&self, suite: &str) -> PathBuf {
        self.data.join("baselines").join(format!("{suite}.json"))
    }

    pub fn bench(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut full = vec!["bench"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default()
            .timeout(Duration::from_secs(300))
            .bin_dir(&self.root.path().join("bin"))
            .env("WF_APP_BENCH_DIR", &self.data.display().to_string())
            .env("WF_APP_BENCH_TMP", &self.tmp.display().to_string())
            .env("STUB_DIR", &self.root.path().display().to_string());
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    /// Sets one metric of both baseline blocks to `value`.
    pub fn pin(&self, suite: &str, metric: &str, value: f64) {
        let mut baseline: Value = serde_json::from_str(&std::fs::read_to_string(self.baseline(suite)).unwrap()).unwrap();
        for block in ["initial", "current"] {
            for field in ["median", "min", "max"] {
                baseline[block]["metrics"][metric][field] = json!(value);
            }
            baseline[block]["metrics"][metric]["values"] = json!([value]);
        }
        std::fs::write(self.baseline(suite), baseline.to_string()).unwrap();
    }
}

pub struct Reading {
    pub median: f64,
    pub unit: String,
}

pub fn readings(stdout: &str) -> HashMap<String, Reading> {
    let re = Regex::new(r"^ {2}(\S+)\s+median (\S+) (\S+) \(min (\S+), max (\S+), spread [\d.]+%\)").unwrap();
    stdout.lines().filter_map(|line| re.captures(line)).map(|c| (c[1].to_string(), Reading { median: c[2].parse().unwrap_or(f64::NAN), unit: c[3].to_string() })).collect()
}

pub fn median(m: &HashMap<String, Reading>, name: &str) -> f64 {
    m.get(name).unwrap_or_else(|| panic!("metric {name} missing")).median
}
