//! The `android` suite: the lab APK on a slim redroid, cold-started a few times, with the app's own memory read from
//! `dumpsys meminfo`. Lab-only: a 1.5-CPU ARM container without a GPU, not a phone.
use super::domain::summarize;
use serde_json::{json, Map, Value};

type Res<T> = Result<T, String>;

/// What one cold start left behind, in kilobytes (PSS and RSS of the whole app, then its parts) and milliseconds.
#[derive(Debug, PartialEq, Default)]
pub struct Sample {
    pub start_ms: f64,
    pub pss_kb: f64,
    pub rss_kb: f64,
    pub java_kb: f64,
    pub native_kb: f64,
    pub code_kb: f64,
    pub device_used_kb: f64,
    pub crashes: f64,
}

fn first_number(text: &str) -> Option<f64> {
    text.split_whitespace().find_map(|t| t.replace(',', "").trim_end_matches('K').parse::<f64>().ok())
}

/// The number after `key` on the first line that carries it (`TotalTime: 11808` gives 11808).
fn after(out: &str, key: &str) -> Option<f64> {
    out.lines().find_map(|l| l.split_once(key).and_then(|(_, rest)| first_number(rest)))
}

/// The first log line that proves the app's JS is running. `am start -W` is not used: it reports a timeout on this slow device.
pub const MARKER: &str = "Running \"main\"";

/// Milliseconds from the launch stamp (`@@T0 <epoch ms>`, taken just before `am start`) to the marker's own `logcat -v epoch`
/// time; `None` when either is missing or the marker predates the launch.
pub fn parse_start_ms(out: &str) -> Option<f64> {
    let t0 = after(out, "@@T0")?;
    let seen: f64 = out.lines().find(|l| l.contains(MARKER))?.split_whitespace().next()?.parse().ok()?;
    let ms = (seen * 1000.0).round() - t0;
    (ms > 0.0).then_some(ms)
}

/// The summary block of `dumpsys meminfo <package>`: `Java Heap:   13480   47828` is PSS then RSS.
pub fn parse_meminfo(out: &str) -> Option<(f64, f64, f64, f64, f64)> {
    let summary = |label: &str| out.lines().find(|l| l.trim_start().starts_with(label)).and_then(|l| first_number(l.split_once(':')?.1));
    let pss = after(out, "TOTAL PSS:")?;
    let rss = after(out, "TOTAL RSS:")?;
    Some((pss, rss, summary("Java Heap:")?, summary("Native Heap:")?, summary("Code:")?))
}

/// The device's `Used RAM` line from `dumpsys meminfo` (`Used RAM: 1,224,882K (...)`).
pub fn parse_used_ram_kb(out: &str) -> Option<f64> {
    after(out, "Used RAM:")
}

/// The VM script prints `RUN` blocks (output the job keeps is a tail, so the APK size is printed first and last); each holds the start output, the meminfo, the device total and the crash count.
pub fn parse_runs(out: &str) -> Res<Vec<Sample>> {
    let mut samples = Vec::new();
    for block in out.split("@@RUN").skip(1) {
        let start_ms = parse_start_ms(block).ok_or_else(|| format!("the app did not log {MARKER} after launch:\n{}", tail(block)))?;
        let (pss_kb, rss_kb, java_kb, native_kb, code_kb) = parse_meminfo(block).ok_or_else(|| format!("no meminfo for the app:\n{}", tail(block)))?;
        let device_used_kb = parse_used_ram_kb(block).unwrap_or(0.0);
        let crashes = after(block, "@@CRASHES").unwrap_or(0.0);
        samples.push(Sample { start_ms, pss_kb, rss_kb, java_kb, native_kb, code_kb, device_used_kb, crashes });
    }
    if samples.is_empty() {
        return Err(format!("the device script produced no runs:\n{}", tail(out)));
    }
    Ok(samples)
}

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(12)..].join("\n")
}

pub fn apk_bytes(out: &str) -> Option<f64> {
    after(out, "@@APK")
}

/// `432x1200@192` from the `@@SCREEN Physical size: 432x1200 Physical density: 192` line the script prints.
pub fn screen_in(out: &str) -> Option<String> {
    let line = out.lines().find_map(|l| l.strip_prefix("@@SCREEN"))?;
    let size = line.split_whitespace().find(|t| t.split_once('x').is_some_and(|(w, h)| w.parse::<u32>().is_ok() && h.parse::<u32>().is_ok()))?;
    let dpi = line.split_whitespace().rev().find_map(|t| t.parse::<u32>().ok())?;
    Some(format!("{size}@{dpi}"))
}

/// The device script: install the APK (cached on the VM by URL hash), then cold-start the app `runs` times. Each run
/// stamps the clock, starts the activity and waits (90 s at most) for logcat's first `MARKER` line; memory is read `settle_s` after it.
pub fn script(container: &str, package: &str, apk_url: &str, settle_s: u64, runs: u64) -> String {
    format!(
        "set -u\n\
D='docker exec {container}'\n\
apk=/tmp/lab-$(echo '{apk_url}' | md5sum | cut -c1-12).apk\n\
[ -s $apk ] || curl -sSL -o $apk '{apk_url}' || exit 1\n\
echo \"@@APK $(stat -c %s $apk)\"\n\
echo \"@@SCREEN $($D wm size | tail -1) $($D wm density | tail -1)\"\n\
docker cp $apk {container}:/data/local/tmp/lab.apk\n\
$D pm install -r /data/local/tmp/lab.apk >/dev/null 2>&1 || {{ echo 'install failed'; exit 1; }}\n\
activity=$($D cmd package resolve-activity --brief {package} | tail -1)\n\
for i in $(seq 1 {runs}); do\n\
  echo '@@RUN'\n\
  $D am force-stop {package}; $D logcat -c; $D sh -c 'echo 3 > /proc/sys/vm/drop_caches' 2>/dev/null; sleep 3\n\
  echo \"@@T0 $(date +%s%3N)\"\n\
  $D am start -n \"$activity\" >/dev/null 2>&1\n\
  timeout 90 $D logcat -v epoch -e 'Running \"main\"' -m 1 2>&1\n\
  sleep {settle_s}\n\
  $D dumpsys meminfo {package} | grep -E 'Java Heap:|Native Heap:|Code:|TOTAL PSS'\n\
  $D dumpsys meminfo | grep 'Used RAM'\n\
  echo \"@@CRASHES $($D logcat -d -b crash 2>/dev/null | grep -c {package})\"\n\
done\n\
echo \"@@APK $(stat -c %s $apk)\"\n"
    )
}

pub fn run(scenario: &Value, runs: f64) -> Res<Value> {
    let params = &scenario["params"];
    // The VM may have no room for a second device, so the shared one can be used: APP_BENCH_ANDROID_CONTAINER=rdl
    // APP_BENCH_ANDROID_PORT=5555 APP_BENCH_ANDROID_TEARDOWN=0 (a device the suite did not boot is not removed).
    let env_or = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let container = env_or("APP_BENCH_ANDROID_CONTAINER").unwrap_or_else(|| params["container"].as_str().unwrap_or("rdl-bench").to_string());
    let container = container.as_str();
    let port = env_or("APP_BENCH_ANDROID_PORT").unwrap_or_else(|| params["port"].as_u64().unwrap_or(5556).to_string());
    let package = params["package"].as_str().ok_or("scenario params lack a package")?;
    let apk_url = std::env::var("APP_BENCH_APK_URL").ok().or_else(|| params["apkUrl"].as_str().map(str::to_string)).ok_or("scenario params lack an apkUrl")?;
    let settle = params["settleSeconds"].as_u64().unwrap_or(15);
    let up = wf_harness::android_lab::up(None, container, Some(&port), None, false, false, false)?;
    wf_harness::android_lab::on_vm("start-device", &wf_harness::android_lab::up_script(&up)).map_err(|e| format!("the bench device did not start: {e}"))?;
    let out = wf_harness::android_lab::capture(&script(container, package, &apk_url, settle, runs as u64));
    let teardown = env_or("APP_BENCH_ANDROID_TEARDOWN").map_or(params["teardown"].as_bool().unwrap_or(true), |v| v != "0");
    if teardown {
        let _ = wf_harness::android_lab::on_vm("stop-device", &wf_harness::android_lab::down_script(container));
    }
    let out = out?;
    let samples = parse_runs(&out)?;
    let series = |f: &dyn Fn(&Sample) -> f64| samples.iter().map(f).collect::<Vec<f64>>();
    let mut metrics = Map::new();
    metrics.insert("apkBytes".into(), summarize("bytes", &[apk_bytes(&out).ok_or("no APK size")?])?);
    for (name, unit, values) in [
        ("coldStartMs", "ms", series(&|s| s.start_ms)),
        ("pssBytes", "bytes", series(&|s| s.pss_kb * 1024.0)),
        ("rssBytes", "bytes", series(&|s| s.rss_kb * 1024.0)),
        ("javaHeapBytes", "bytes", series(&|s| s.java_kb * 1024.0)),
        ("nativeHeapBytes", "bytes", series(&|s| s.native_kb * 1024.0)),
        ("codeBytes", "bytes", series(&|s| s.code_kb * 1024.0)),
        ("deviceUsedBytes", "bytes", series(&|s| s.device_used_kb * 1024.0)),
        ("crashes", "count", series(&|s| s.crashes)),
    ] {
        metrics.insert(name.into(), summarize(unit, &values)?);
    }
    let env = json!({ "container": container, "package": package, "apkUrl": apk_url, "screen": screen_in(&out).unwrap_or_else(|| "unknown".into()), "device": "redroid 14, 1.5 CPU, no GPU, slim", "coldStartMarker": format!("launch to first ReactNativeJS {MARKER}") });
    Ok(json!({ "suite": "android", "metrics": Value::Object(metrics), "env": env }))
}
