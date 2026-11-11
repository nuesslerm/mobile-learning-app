//! The `emulator` device provider (`WF_ANDROID_PROVIDER=emulator`): the SDK emulator as a local process on a GitHub-hosted runner,
//! booted by `start-device` and killed by `stop-device`, instead of the redroid container the VM's agent-shell runs. The runner
//! reaches the guest at `emulator-5554` and the guest reaches the runner at 10.0.2.2, so the stub host is that address.
use serde_json::{json, Value};
use std::process::{Command, Stdio};

pub const SERIAL: &str = "emulator-5554";
pub const STUB_HOST: &str = "10.0.2.2";
const IMAGE: &str = "system-images;android-34;google_apis;x86_64";
const EXPO_GO_URL: &str = "https://github.com/expo/expo-go-releases/releases/download/Expo-Go-57.0.9/Expo-Go-57.0.9.apk";

/// How the device is reached: the redroid on the VM through `wf vm exec` (the default), the redroid through the CI port's device lab
/// (a CI job that has no admin port), or the SDK emulator as a local process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Redroid,
    RedroidLab,
    Emulator,
}

/// The provider named by `WF_ANDROID_PROVIDER`: `redroid` (the default), `redroid-lab` or `emulator`.
pub fn provider() -> Result<Provider, String> {
    match std::env::var("WF_ANDROID_PROVIDER").unwrap_or_default().as_str() {
        "" | "redroid" => Ok(Provider::Redroid),
        "redroid-lab" => Ok(Provider::RedroidLab),
        "emulator" => Ok(Provider::Emulator),
        other => Err(format!("WF_ANDROID_PROVIDER is redroid, redroid-lab or emulator, got '{other}'")),
    }
}

/// Whether the provider is the local emulator.
pub fn selected() -> Result<bool, String> {
    provider().map(|p| p == Provider::Emulator)
}

pub fn up_script(w: u32, h: u32, dpi: u32, expo_go: bool) -> String {
    let apps = if expo_go {
        format!(
            "apk=\"${{RUNNER_TEMP:-/tmp}}/expo-go.apk\"\n\
[ -s \"$apk\" ] || curl -fsSL -o \"$apk\" '{EXPO_GO_URL}'\n\
adb -s {SERIAL} install -r \"$apk\" >/dev/null && echo 'Expo Go installed'\n"
        )
    } else {
        String::new()
    };
    format!(
        "set -eu\n\
SDK=\"${{ANDROID_HOME:-${{ANDROID_SDK_ROOT:-}}}}\"; [ -n \"$SDK\" ] || {{ echo 'ANDROID_HOME is not set'; exit 1; }}\n\
export PATH=\"$SDK/cmdline-tools/latest/bin:$SDK/platform-tools:$SDK/emulator:$PATH\"\n\
if adb -s {SERIAL} get-state >/dev/null 2>&1; then echo '{SERIAL} already up'; {apps}exit 0; fi\n\
[ -w /dev/kvm ] || {{ echo 'KVM is not writable here'; ls -l /dev/kvm; exit 1; }}\n\
if ! command -v emulator >/dev/null || [ ! -d \"$SDK/system-images/android-34\" ]; then\n\
  yes | sdkmanager --licenses >/dev/null 2>&1 || true\n\
  sdkmanager --install emulator platform-tools '{IMAGE}' >/dev/null\n\
fi\n\
export ANDROID_AVD_HOME=\"$HOME/.android/avd\"; mkdir -p \"$ANDROID_AVD_HOME\"\n\
echo no | avdmanager create avd -n wf -k '{IMAGE}' -d pixel_2 --force >/dev/null\n\
ini=\"$ANDROID_AVD_HOME/wf.avd/config.ini\"\n\
for kv in hw.lcd.width={w} hw.lcd.height={h} hw.lcd.density={dpi} hw.ramSize=3072 disk.dataPartition.size=2G; do key=\"${{kv%%=*}}\"; sed -i \"/^$key=/d\" \"$ini\"; echo \"$kv\" >> \"$ini\"; done\n\
log=\"${{RUNNER_TEMP:-/tmp}}/emulator.log\"\n\
setsid nohup emulator -avd wf -no-window -no-snapshot -no-boot-anim -no-audio -gpu swiftshader_indirect -accel on -cores 2 -memory 3072 > \"$log\" 2>&1 &\n\
echo $! > \"${{RUNNER_TEMP:-/tmp}}/emulator.pid\"\n\
t0=$(date +%s)\n\
adb wait-for-device\n\
until [ \"$(adb -s {SERIAL} shell getprop sys.boot_completed 2>/dev/null | tr -d '\\r')\" = 1 ]; do\n\
  [ $(( $(date +%s) - t0 )) -gt 900 ] && {{ echo 'no boot after 900 s'; tail -30 \"$log\"; exit 1; }}\n\
  sleep 3\n\
done\n\
adb -s {SERIAL} root >/dev/null; sleep 3; adb -s {SERIAL} wait-for-device\n\
adb -s {SERIAL} shell wm size {w}x{h}; adb -s {SERIAL} shell wm density {dpi}\n\
echo \"{SERIAL} up {w}x{h}@{dpi} after $(( $(date +%s) - t0 )) s\"\n\
{apps}"
    )
}

pub fn down_script() -> String {
    format!(
        "adb -s {SERIAL} emu kill >/dev/null 2>&1 || true\n\
pid=$(cat \"${{RUNNER_TEMP:-/tmp}}/emulator.pid\" 2>/dev/null || true)\n\
[ -n \"$pid\" ] && kill \"$pid\" >/dev/null 2>&1 || true\n\
echo '{SERIAL} down'\n"
    )
}

/// Runs a lifecycle script here, its output on stderr.
pub fn run_local(verb: &str, script: &str) -> Result<(), String> {
    let path = std::env::temp_dir().join(format!("wf-android-emulator-{verb}-{}.sh", std::process::id()));
    std::fs::write(&path, script).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let status = Command::new("bash").arg(&path).stdout(Stdio::inherit()).status();
    let _ = std::fs::remove_file(&path);
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("the {verb} script exited {} (its output is above, on stderr)", s.code().unwrap_or(1))),
        Err(e) => Err(format!("the {verb} script did not run: {e}")),
    }
}

/// `list-devices` for this provider, in the neutral shape the CI health step reads: booted, the activity service, the provider.
pub fn device() -> Result<Option<Value>, String> {
    let adb = |args: &[&str]| Command::new("adb").args(["-s", SERIAL]).args(args).stdin(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let state = adb(&["get-state"]).map_err(|e| format!("cannot run adb: {e}"))?;
    if state.trim() != "device" {
        return Ok(None);
    }
    let booted = adb(&["shell", "getprop", "sys.boot_completed"]).unwrap_or_default().trim() == "1";
    let activity = adb(&["shell", "service", "check", "activity"]).unwrap_or_default().contains("found");
    Ok(Some(json!({ "name": SERIAL, "provider": "emulator", "serial": SERIAL, "boot": { "completed": booted, "activityService": activity } })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_emulator_provider_when_the_start_script_is_built_then_it_boots_the_one_serial_at_the_requested_screen() {
        let s = up_script(480, 1040, 213, true);
        assert!(s.contains("hw.lcd.width=480") && s.contains("wm size 480x1040"), "{s}");
        assert!(s.contains("emulator-5554") && s.contains("Expo Go installed"), "{s}");
    }

    #[test]
    fn given_no_expo_go_when_the_start_script_is_built_then_it_installs_no_app() {
        assert!(!up_script(480, 1040, 213, false).contains("expo-go"));
    }

    #[test]
    fn given_the_stop_script_when_read_then_it_kills_the_emulator_by_serial_and_pid() {
        let s = down_script();
        assert!(s.contains("emu kill") && s.contains("emulator.pid"));
    }
}
