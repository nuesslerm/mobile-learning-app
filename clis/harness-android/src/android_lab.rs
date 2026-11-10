//! `wf android start-device|stop-device|list-devices`: the on-demand redroid on the VM. Idle cost is zero because nothing
//! stays resident between runs; the script runs in agent-shell's dind through `wf vm exec`. `--dry-run` shows the script
//! without running it. A fresh device boots slim (see `android_features`); `--with <features>` keeps some back, `--stock`
//! keeps everything.
use crate::android_features::{catalog, parse_with, plan, report, restore_all};
use wf_vmops::services::ANDROID_IMAGE;
use serde_json::{json, Value};
use std::collections::HashSet;

/// A Galaxy S25 in dp (360 x 780, 19.5:9) at 213 dpi: 499,200 pixels, under the first 540x960 at 240 (518,400), so rendering
/// costs no more. The width stays 360 dp, the width the scripts' positions were measured at; scripts read the size from the device.
const DEFAULT_SCREEN: &str = "480x1040@213";
/// Build.PRODUCT with "emulator" in it is what Expo Go's emulator check reads: from a device it takes for real hardware it
/// asks EAS as EXPO_DEVICE, and EAS answers an anonymous EXPO_DEVICE 403 for an update of a project that requires sign-in.
const PRODUCT: &str = "wf_redroid_emulator";
const EXPO_GO_URL: &str = "https://github.com/expo/expo-go-releases/releases/download/Expo-Go-57.0.9/Expo-Go-57.0.9.apk";

fn parse_screen(s: &str) -> Option<(u32, u32, u32)> {
    let (size, dpi) = s.split_once('@')?;
    let (w, h) = size.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?, dpi.parse().ok()?))
}

pub struct Up {
    w: u32,
    h: u32,
    dpi: u32,
    name: String,
    port: u16,
    /// Packages to disable and to enable once booted; `None` leaves the device as it is.
    apply: Option<(Vec<String>, Vec<String>)>,
    /// Whether an already running device is changed too (an explicit `--slim`, `--with` or `--stock`).
    explicit: bool,
    /// Install Expo Go once the device has booted (the EAS Update opens in it).
    expo_go: bool,
}

fn expo_go_script(name: &str) -> String {
    format!(
        "apk=/tmp/expo-go-57.0.9.apk\n\
[ -s $apk ] || curl -sSL -o $apk '{EXPO_GO_URL}'\n\
docker cp $apk {name}:/data/local/tmp/expogo.apk\n\
if docker exec {name} pm install -r /data/local/tmp/expogo.apk >/dev/null 2>&1; then echo 'Expo Go installed'; else echo 'Expo Go install failed'; exit 1; fi\n"
    )
}

fn apply_script(name: &str, disable: &[String], enable: &[String]) -> String {
    format!(
        "dis=\"{}\"; ena=\"{}\"; n=0\n\
for p in $dis; do docker exec {name} pm disable-user --user 0 $p >/dev/null 2>&1 && n=$((n+1)); done\n\
for p in $ena; do docker exec {name} pm enable $p >/dev/null 2>&1; done\n\
echo \"packages: $n of {} disabled, {} enabled\"\n\
docker restart {name} >/dev/null 2>&1\n\
ok=0; for i in $(seq 1 60); do [ \"$(docker exec {name} getprop sys.boot_completed 2>/dev/null | tr -d '\\r')\" = 1 ] && docker exec {name} service check activity 2>/dev/null | grep -q found && ok=1 && break; sleep 4; done\n\
if [ $ok = 0 ]; then docker exec {name} logcat -d -b all 2>/dev/null | grep -v SystemServerTiming | grep -E 'FATAL|AndroidRuntime|ActivityTaskManager|PackageManager|Home|Launcher' | tail -20 | cut -c1-200; echo \"{name}: the package set did not boot after a restart; recreating the device from a fresh volume\"; docker rm -f {name} >/dev/null 2>&1; docker volume rm {name}-data >/dev/null 2>&1; boot_fresh || exit 1; echo \"{name}: rolled back to a clean stock device\"; exit 1; fi\n\
[ $n -gt 0 ] && docker exec {name} sh -c 'echo ok > /data/local/wf-slim-proof'\n\
echo 'package set boots after a restart'\n",
        disable.join(" "),
        enable.join(" "),
        disable.len(),
        enable.len()
    )
}

/// The buildx builder keeps about 1.4 GB while idle; with a device up the VM had 198 MB left (docs/gaps/vm-memory-saturation.md).
const BUILDER: &str = "buildx_buildkit_agent-shell-builder0";

pub fn up_script(u: &Up) -> String {
    let Up { w, h, dpi, name, port, .. } = u;
    let apply = u.apply.as_ref().map(|(d, e)| apply_script(name, d, e)).unwrap_or_default();
    let apps = if u.expo_go { expo_go_script(name) } else { String::new() };
    let on_running = if u.explicit { format!("{apply}{apps}") } else { apps.clone() };
    format!(
        "set -u\n\
boot_fresh() {{\n\
docker rm -f {name} >/dev/null 2>&1\n\
docker run -d --name {name} --privileged --cpus 1.5 -p {port}:5555 -v {name}-data:/data {ANDROID_IMAGE} androidboot.use_memfd=1 androidboot.redroid_width={w} androidboot.redroid_height={h} androidboot.redroid_dpi={dpi} ro.product.name={PRODUCT} androidboot.redroid_fps=30 androidboot.redroid_gpu_mode=guest androidboot.redroid_net_ndns=1 androidboot.redroid_net_dns1=100.100.100.100 >/dev/null || return 1\n\
for i in $(seq 1 60); do [ \"$(docker exec {name} getprop sys.boot_completed 2>/dev/null | tr -d '\\r')\" = 1 ] && break; sleep 3; done\n\
[ \"$(docker exec {name} getprop sys.boot_completed 2>/dev/null | tr -d '\\r')\" = 1 ] || {{ echo '{name} boot timeout'; docker rm -f {name} >/dev/null 2>&1; return 1; }}\n\
}}\n\
if [ \"$(docker inspect -f '{{{{.State.Running}}}}' {name} 2>/dev/null)\" = true ]; then\n\
  if docker inspect -f '{{{{join .Args \" \"}}}}' {name} | grep -q 'redroid_width={w} androidboot.redroid_height={h} androidboot.redroid_dpi={dpi} ro.product.name={PRODUCT} '; then echo '{name} already up'; {on_running}exit 0; fi\n\
  echo '{name} runs another screen: stopping it to boot at {w}x{h}@{dpi}'; docker stop -t 30 {name} >/dev/null 2>&1\n\
fi\n\
before=$(awk '/MemAvailable/{{print int($2/1024)}}' /proc/meminfo)\n\
avail=$(awk '/MemAvailable/{{print int($2/1024)}}' /proc/meminfo)\n\
if [ \"$avail\" -lt 700 ] && ! pgrep -f 'docker buildx build' >/dev/null && [ \"$(docker inspect -f '{{{{.State.Running}}}}' {BUILDER} 2>/dev/null)\" = true ]; then\n\
  echo \"{name}: $avail MB available: stopping the idle image builder ({BUILDER}); the next build starts it again\"; docker stop -t 10 {BUILDER} >/dev/null 2>&1; sleep 2\n\
  avail=$(awk '/MemAvailable/{{print int($2/1024)}}' /proc/meminfo)\n\
fi\n\
if [ \"$avail\" -lt 700 ]; then echo \"{name}: only $avail MB of memory is available and a device needs about 700 MB; the biggest holders:\"; docker stats --no-stream --format '{{{{.Name}}}} {{{{.MemUsage}}}}' | sort -k2 -rn | head -5; exit 1; fi\n\
boot_fresh || exit 1\n\
{apply}{apps}after=$(awk '/MemAvailable/{{print int($2/1024)}}' /proc/meminfo)\n\
echo \"{name} up {w}x{h}@{dpi} MemAvailable_mb before=$before after=$after\"\n"
    )
}

pub fn down_script(name: &str) -> String {
    format!(
        "set -u\n\
before=$(awk '/MemAvailable/{{print int($2/1024)}}' /proc/meminfo)\n\
docker stop -t 30 {name} >/dev/null 2>&1\n\
docker rm -f {name} >/dev/null 2>&1\n\
sleep 2\n\
after=$(awk '/MemAvailable/{{print int($2/1024)}}' /proc/meminfo)\n\
echo \"{name} down MemAvailable_mb before=$before after=$after\"\n"
    )
}

/// Runs a script on the VM and returns its output once the job ends.
pub fn capture(script: &str) -> Result<String, String> {
    match wf_vmops::vm_exec::capture_code(script)? {
        (0, out) => Ok(out),
        (c, out) => Err(format!("vm exec exit {c}: {}", out.trim())),
    }
}

/// What `start-device` boots: the screen, the container and its adb port, and the package set; a malformed option is the
/// error, before anything runs.
pub fn up(screen: Option<&str>, name: &str, port: Option<&str>, with: Option<&str>, slim: bool, stock: bool, expo_go: bool) -> Result<Up, String> {
    let raw = screen.unwrap_or(DEFAULT_SCREEN);
    let (w, h, dpi) = parse_screen(raw).ok_or_else(|| format!("--screen must look like 480x1040@213, got '{raw}'"))?;
    let port: u16 = port.unwrap_or("5555").parse().map_err(|_| "--port must be a number".to_string())?;
    let cat = catalog();
    let with = parse_with(with.unwrap_or(""), &cat)?;
    if stock && (slim || !with.is_empty()) {
        return Err("--stock cannot be combined with --slim or --with".into());
    }
    let explicit = stock || slim || !with.is_empty();
    let apply = if stock { Some((Vec::new(), restore_all(&cat))) } else { Some(plan(&cat, &with)) };
    Ok(Up { w, h, dpi, name: name.to_string(), port, apply, explicit, expo_go })
}

impl Up {
    pub(crate) fn summary(&self) -> Value {
        json!({ "name": self.name, "screen": format!("{}x{}@{}", self.w, self.h, self.dpi), "port": self.port, "expoGo": self.expo_go })
    }
}

/// Runs a lifecycle script on the VM, its output streamed as it comes; a non-zero exit is the error.
pub fn on_vm(verb: &str, script: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("wf-android-{verb}-{}.sh", std::process::id()));
    std::fs::write(&path, script).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let code = wf_vmops::vm_exec::run(&[path.display().to_string()]);
    let _ = std::fs::remove_file(&path);
    match code {
        Some(0) => Ok(()),
        Some(c) => Err(format!("the {verb} script exited {c} (its output is above, on stderr)")),
        None => Err(format!("the {verb} script did not run")),
    }
}

/// The device `name` as `list-devices` reports it: booted, the slim set proved across a restart, each feature on, off or
/// partial, and its lease where the lease state is local. None when no such container runs.
pub(crate) fn device(name: &str) -> Result<Option<Value>, String> {
    let script = format!(
        "[ \"$(docker inspect -f '{{{{.State.Running}}}}' {name} 2>/dev/null)\" = true ] || {{ echo @@ABSENT; exit 0; }}\n\
docker exec {name} getprop sys.boot_completed\n\
docker exec {name} service check activity\n\
echo \"@@PROOF $(docker exec {name} cat /data/local/wf-slim-proof 2>/dev/null)\"\n\
echo @@DISABLED\n\
docker exec {name} pm list packages -d\n"
    );
    let out = capture(&script)?;
    if out.lines().any(|l| l.trim() == "@@ABSENT") {
        return Ok(None);
    }
    let disabled: HashSet<String> = out.lines().filter_map(|l| l.trim().strip_prefix("package:")).map(str::to_string).collect();
    let mut r = report(&catalog(), &disabled);
    r["name"] = json!(name);
    r["serial"] = json!(crate::android::serial_or_env(None));
    r["boot"] = json!({
        "completed": out.lines().any(|l| l.trim() == "1"),
        "activityService": out.lines().any(|l| l.contains("Service activity: found")),
    });
    r["restartProven"] = json!(out.lines().any(|l| l.trim() == "@@PROOF ok"));
    r["lease"] = crate::android_lease::status_of(name).unwrap_or(Value::Null);
    Ok(Some(r))
}
