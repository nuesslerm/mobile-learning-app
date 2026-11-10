//! `wf android`: the command table's verbs on the device (by uiautomator dump and by the on-device UI server), raw `adb`,
//! the device lease, the redroid lifecycle scripts, the dropped verbs and unknown ones. `adb` is an argv-recording stub, vm-ops-api is
//! a fake behind a mock tailnet, the UI server is a fake HTTP JSON-RPC server, and the lease clock is moved with WF_LEASE_CLOCK_OFFSET_MS.
use crate::support::{fixtures_dir, last_json, repo_root, spawn_wf, wait_until, RunOpts, RunResult, SlotSandbox};
use wf_vmops::testing::{answers, FakeVm};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One `adb` for every journey: it logs its argv and answers the boot polls, the screen, the keyboard, logcat and recordings;
/// with STUB_UI_XML_AFTER the screen is that one from the first `input swipe` on, with STUB_UI_SEQ (lines `<argv part>|<xml>`,
/// taken in order) the screen is a line's xml from the first call naming its part on, with STUB_TYPE_LOWERS typed text lowers
/// the keyboard, and with STUB_DEVICE_BIN the recorder's stop runs its device script with that directory's tools alone.
const STUB_ADB: &str = r#"echo "$*" >> "$0.log"
case "$*" in *"input swipe"*) touch "$0.swiped";; esac
case "$*" in *"input text"*) [ -n "${STUB_TYPE_LOWERS:-}" ] && echo false > "$0.ime";; esac
[ -n "${STUB_UI_XML_AFTER:-}" ] && [ -e "$0.swiped" ] && STUB_UI_XML="$STUB_UI_XML_AFTER"
if [ -n "${STUB_UI_SEQ:-}" ]; then
  i=$(cat "$0.seq" 2>/dev/null || echo 1); next=$(sed -n "${i}p" "$STUB_UI_SEQ")
  [ -n "$next" ] && case "$*" in *"${next%%|*}"*) echo $((i + 1)) > "$0.seq"; echo "${next#*|}" > "$0.seqxml";; esac
  [ -e "$0.seqxml" ] && STUB_UI_XML=$(cat "$0.seqxml")
fi
case "$*" in
  *"id -u"*) echo 0;;
  *"[ -e /sdcard/wf-record.on ]"*) [ -n "${STUB_RECORDING:-}" ] && echo "wf-recording=$STUB_RECORDING"; true;;
  *"pkill -INT screenrecord"*) [ -z "${STUB_DEVICE_BIN:-}" ] || PATH="$STUB_DEVICE_BIN" /bin/sh -c "$4";;
  *get-state*) n=$(cat "$0.n" 2>/dev/null || echo 0); n=$((n + 1)); echo "$n" > "$0.n"; [ "$n" -gt "${STUB_OFFLINE_POLLS:-0}" ] && echo device; exit 0;;
  *"getprop sys.boot_completed"*) echo "${STUB_BOOT:-1}"; exit 0;;
  *"getprop init.svc.bootanim"*) echo stopped; exit 0;;
  *exec-out*cat*) cat "$STUB_UI_XML";;
  *exec-out*screencap*) if [ -n "${STUB_PNG:-}" ]; then cat "$STUB_PNG"; else printf 'PNGDATA'; fi;;
  *exec-out*) printf 'PNGDATA';;
  *dumpsys*input_method*) echo "  mInputShown=$(cat "$0.ime" 2>/dev/null || echo "${STUB_IME:-true}")";;
  *"dumpsys window"*) printf '  Window #3 Window{a u0 InputMethod}:\n    mAttrs={(0,0)(fillxfill) gr=BOTTOM ty=INPUT_METHOD}\n    frame=[0,32][480,1040] last=[0,0][0,0]\n  InsetsSource type=ime frame=[0,610][480,1040] visible=true\n';;
  *"wm size"*) printf 'Physical size: 1080x2400\nOverride size: 480x1040\n';;
  *inj.marker*) printf 'wf-input=/tmp/clip.wav\n/data/user/0/host.exp.exponent/cache/a.m4a.swapped\n';;
  *logcat*) printf '%s' "${STUB_LOGCAT:-}";;
  *"forward tcp:0 tcp:9008"*) echo "$STUB_U2_PORT";;
  *"shell ls /data/local/tmp/u2.jar"*) if [ -f "$STUB_U2_MARK.jar" ]; then echo /data/local/tmp/u2.jar; else echo 'ls: /data/local/tmp/u2.jar: No such file or directory'; fi;;
  *" push "*u2.jar*) touch "$STUB_U2_MARK.jar";;
  *com.wetest.uia2.Main*) touch "$STUB_U2_MARK";;
  *"pkill -f com.wetest.uia2"*) rm -f "$STUB_U2_MARK";;
  *'shell ls'*) printf '%b' "${STUB_SEGMENTS:-/sdcard/wf-record-1.mp4\n}";;
  *devices*) echo 'List of devices attached';;
esac
exit "${STUB_ADB_EXIT:-0}""#;

const UI_XML: &str = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation="0">
<node index="0" text="" resource-id="com.app:id/TamilWriteCheck" class="android.view.ViewGroup" bounds="[100,200][300,260]" clickable="true" />
<node index="2" text="" resource-id="TamilPracticeCanvasSurface" class="android.view.ViewGroup" bounds="[20,100][80,200]" clickable="true" />
<node index="1" text="LOOKS RIGHT · 92" resource-id="TamilWriteVerdictBadge" class="android.widget.TextView" bounds="[10,10][110,50]" clickable="false" />
<node index="3" text="Hi &amp; bye" resource-id="" class="android.widget.TextView" bounds="[0,0][10,10]" clickable="false" />
</hierarchy>"#;

const DUMP: &str = "-s emulator-5554 shell uiautomator dump /sdcard/wf-ui.xml";
const READ: &str = "-s emulator-5554 exec-out cat /sdcard/wf-ui.xml";
const STOP_RECORDER: &str = "-s emulator-5554 shell rm -f /sdcard/wf-record.on; pkill -INT screenrecord; command -v pidof >/dev/null || sleep 2; i=0; while pidof screenrecord >/dev/null && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done; true";

pub(crate) fn sandbox(prefix: &str) -> SlotSandbox {
    let s = SlotSandbox::new(prefix);
    s.stub("adb", STUB_ADB);
    s.stub("kill", "echo \"$@\" >> \"$0.log\"");
    s.stub("lsof", "exit 1");
    s.file("ui.xml", UI_XML);
    s
}

/// `wf android <args>` with the dump path (no UI server), the metro pid dir and the evidence dir in the sandbox, and `extra`
/// on top.
fn android(s: &SlotSandbox, args: &[&str], extra: &[(&str, &str)]) -> RunResult {
    let (xml, pids, evidence) = (s.repo.join("ui.xml").display().to_string(), s.reg_dir.display().to_string(), s.repo.join("evidence").display().to_string());
    let mut env = vec![("STUB_UI_XML", xml.as_str()), ("WF_ANDROID_UIA", "dump"), ("WF_METRO_PID_DIR", pids.as_str()), ("WF_TEST_OUTPUT_DIR", evidence.as_str())];
    env.extend_from_slice(extra);
    let mut full = vec!["android"];
    full.extend_from_slice(args);
    s.run(&full, &env)
}

fn out(r: &RunResult) -> String {
    format!("{}{}", r.stdout, r.stderr)
}

#[test]
fn an_agent_drives_the_device_with_the_table_verbs_and_raw_adb() {
    // Given no device named anywhere, and adb listing none
    let s = sandbox("wf-flow-android-adb-");
    let none = android(&s, &["tap-on", "--point", "1,2"], &[("ANDROID_SERIAL", "")]);
    let forwarded = android(&s, &["adb", "devices"], &[("ANDROID_SERIAL", "")]);
    // Then a device verb names the fix in its envelope after asking adb, and a raw adb call goes through unchanged
    let refused = json_out(&none);
    assert_eq!((none.exit_code, refused["ok"].clone()), (1, json!(false)), "{}", out(&none));
    assert!(refused["error"].as_str().unwrap().contains("no Android device: pass --device or set ANDROID_SERIAL") && refused["next"].is_string(), "{refused}");
    assert_eq!((forwarded.exit_code, s.calls("adb")), (0, vec!["devices".to_string(), "devices".to_string()]), "{}", out(&forwarded));
    s.forget("adb");

    // When the agent drives the device ANDROID_SERIAL names (or the one --device gives) with the table's verbs
    let shot = s.repo.join("shots/home.png").display().to_string();
    let png = s.file("screen.png", png_rgb(4, 8, |_, _| [255, 255, 255]));
    let mut answers = Vec::new();
    for args in [
        vec!["open-link", "exp://u.expo.dev/p/group/g"],
        vec!["tap-on", "--point", "120,340"],
        vec!["swipe", "100,800", "100,200", "--duration", "250"],
        vec!["swipe", "1,2", "3,4"],
        vec!["input-text", "two sum", "--json"],
        vec!["run-shell", "getprop ro.build.version.sdk"],
        vec!["tap-on", "--point", "1,2", "--device", "emulator-5556"],
        vec!["take-screenshot", shot.as_str()],
        vec!["adb", "devices"],
    ] {
        let r = android(&s, &args, &[("STUB_PNG", png.as_str())]);
        assert_eq!(r.exit_code, 0, "{args:?}: {}", out(&r));
        answers.push(r);
    }
    let failing = android(&s, &["adb", "get-serialno"], &[("STUB_ADB_EXIT", "7")]);
    // Then each verb answered one JSON envelope naming its device, and became its adb command lines with that serial; the
    // screenshot landed at its path; raw adb's output is its own, and adb's exit code is the exit code
    for (r, device) in answers[..8].iter().zip(["emulator-5554", "emulator-5554", "emulator-5554", "emulator-5554", "emulator-5554", "emulator-5554", "emulator-5556", "emulator-5554"]) {
        let v = json_out(r);
        assert_eq!((v["ok"].clone(), v["platform"].clone(), v["device"].clone()), (json!(true), json!("android"), json!(device)), "{v}");
    }
    assert_eq!(
        s.calls("adb"),
        [
            "-s emulator-5554 shell am start -a android.intent.action.VIEW -d exp://u.expo.dev/p/group/g",
            "-s emulator-5554 shell input tap 120 340",
            "-s emulator-5554 shell input swipe 100 800 100 200 250",
            "-s emulator-5554 shell input swipe 1 2 3 4 400",
            "-s emulator-5554 shell dumpsys input_method",
            "-s emulator-5554 shell input text 'two%ssum'",
            "-s emulator-5554 shell getprop ro.build.version.sdk",
            "-s emulator-5556 shell input tap 1 2",
            "-s emulator-5554 exec-out screencap -p",
            "-s emulator-5554 devices",
            "-s emulator-5554 get-serialno",
        ]
    );
    assert_eq!(json_out(&answers[7])["result"], json!({ "path": shot, "width": 4, "height": 8 }));
    assert_eq!(std::fs::read(&shot).unwrap(), std::fs::read(&png).unwrap());
    assert_eq!((answers[8].stdout.as_str(), failing.exit_code), ("List of devices attached\n", 7));

    // When a verb or an option is used under its name from before the rename, or a dropped verb runs
    s.forget("adb");
    let old = |args: &[&str]| {
        let r = android(&s, args, &[]);
        (r.exit_code, json_out(&r))
    };
    let unknown: Vec<(i32, Value)> = [vec!["run", "t.lua"], vec!["tap", "1", "2"], vec!["type", "x"], vec!["press", "back"], vec!["open", "exp://x"], vec!["trace", "--id", "Pad"], vec!["screenshot"], vec!["keyboard", "--require"], vec!["edit-text"], vec!["warnings", "--fail"], vec!["record", "stop"], vec!["up", "--print"], vec!["down"], vec!["features", "--json"]]
        .iter()
        .map(|a| old(a))
        .collect();
    let dropped: Vec<(i32, Value)> = ["text", "find", "wait", "stroke", "launch", "install", "help", "start", "stop", "status"].iter().map(|v| old(&[v])).collect();
    let serial = old(&["tap-on", "Go", "--serial", "e1"]);
    let name = old(&["get-element", "--id", "Go", "--name", "rdl"]);
    let print = old(&["stop-device", "--print"]);
    let lease_device = old(&["lease", "status", "--device", "rdl"]);
    // Then an old verb is an unknown one: exit 2 before reaching the device, listing the verbs there are
    for (code, v) in &unknown {
        let verb = v["command"].as_str().unwrap();
        let verbs: Vec<&str> = v["verbs"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(*code == 2 && v["error"] == format!("unknown verb '{verb}'") && v["use"].is_null(), "{v}");
        assert!(verbs.contains(&"tap-on") && verbs.contains(&"start-device") && verbs.contains(&"test") && !verbs.contains(&verb), "{v}");
    }
    // and a dropped one still says why and what covers it
    for (code, v) in &dropped {
        assert!(*code == 2 && v["error"] == "dropped" && v["why"].is_string() && v["use"].as_str().is_some_and(|u| u.starts_with("wf android ")), "{v}");
    }
    // and an old option is an unknown one
    for ((code, v), flag) in [(&serial, "--serial"), (&name, "--name"), (&print, "--print"), (&lease_device, "--device")] {
        assert!(*code == 2 && v["error"].as_str().unwrap().contains(flag) && !v["error"].as_str().unwrap().contains(" is now ") && v["usage"].is_string(), "{v}");
    }
    assert!(s.calls("adb").is_empty(), "{:?}", s.calls("adb"));
    // and verbs that never existed here are refused as unsupported
    for verb in ["cleanup", "nuke", "build"] {
        assert_eq!(android(&s, &[verb], &[]).exit_code, 2, "{verb}");
    }
}

#[test]
fn the_device_lease_hands_the_device_to_one_sandbox_at_a_time_in_order() {
    // Given a claimed emulator, a local lease directory and a clock that tests can move
    let s = sandbox("wf-flow-android-lease-");
    let dir = s.repo.join("leases").display().to_string();
    let at = |offset_ms: u64| offset_ms.to_string();
    let lease = |args: &[&str], offset: &str| {
        let mut full = vec!["android", "lease", "--local"];
        full.extend_from_slice(args);
        s.run(&full, &[("WF_ANDROID_LEASE_DIR", dir.as_str()), ("WF_LEASE_CLOCK_OFFSET_MS", offset)])
    };
    let result = |r: &RunResult| last_json(r)["result"].clone();

    // When sandbox-a takes the device and sandbox-b asks without waiting
    let first = lease(&["--who", "sandbox-a"], &at(0));
    let second = lease(&["--who", "sandbox-b", "--no-wait", "--json"], &at(0));
    let a = result(&first)["leaseId"].as_str().expect("a lease id").to_string();
    // Then a holds it, b is refused with exit 3 naming the holder and how to take a turn, and b did not stay in the line
    assert_eq!((first.exit_code, result(&first)["granted"].clone(), last_json(&first)["device"].clone()), (0, json!(true), json!("rdl")), "{}", out(&first));
    assert_eq!((second.exit_code, last_json(&second)["ok"].clone()), (3, json!(false)), "{}", out(&second));
    assert!(last_json(&second)["error"].as_str().unwrap().contains("sandbox-a") && last_json(&second)["next"].as_str().unwrap().contains("wf android lease release"), "{}", out(&second));
    let status = result(&lease(&["status"], &at(1000)));
    assert_eq!((status["holder"]["who"].clone(), status["queue"].clone()), (json!("sandbox-a"), json!([])), "{status}");

    // When the container is named with the old --device, and another container's lease is asked for
    let old_flag = lease(&["status", "--device", "rdl"], &at(1000));
    let other = last_json(&lease(&["status", "--name", "rdl-bench"], &at(1000)));
    // Then --device is an unknown option there, and the other container has a line of its own
    assert_eq!(old_flag.exit_code, 2, "{}", out(&old_flag));
    assert!(last_json(&old_flag)["error"].as_str().unwrap().contains("'--device'") && last_json(&old_flag)["usage"].as_str().unwrap().contains("lease"), "{}", out(&old_flag));
    assert_eq!((other["device"].clone(), other["result"]["holder"].clone()), (json!("rdl-bench"), Value::Null), "{other}");

    // When raw adb runs without the lease, with the check skipped, and by the holder
    let env = |extra: &[(&'static str, String)]| {
        let mut e = vec![("WF_ANDROID_LEASE_DIR", dir.clone()), ("WF_ANDROID_UIA", "dump".to_string())];
        e.extend_from_slice(extra);
        e
    };
    let refs = |e: &[(&'static str, String)]| e.iter().map(|(k, v)| (*k, v.clone())).collect::<Vec<_>>();
    let run = |extra: &[(&'static str, String)]| {
        let e = refs(&env(extra));
        let e: Vec<(&str, &str)> = e.iter().map(|(k, v)| (*k, v.as_str())).collect();
        s.run(&["android", "adb", "devices"], &e)
    };
    let stranger = run(&[]);
    let skipped = run(&[("WF_ANDROID_LEASE", "none".to_string())]);
    let holder = run(&[("WF_ANDROID_LEASE_ID", a.clone())]);
    // Then only the stranger is refused, with exit 3 and the holder's name, before adb runs
    assert_eq!(stranger.exit_code, 3, "{}", out(&stranger));
    assert!(stranger.stderr.contains("sandbox-a") && stranger.stderr.contains("wf android lease"), "{}", stranger.stderr);
    assert_eq!((skipped.exit_code, holder.exit_code), (0, 0), "{}{}", out(&skipped), out(&holder));
    assert_eq!(s.calls("adb"), ["-s emulator-5554 devices", "-s emulator-5554 devices"]);

    // When the device commands run without the lease: one that acts, one that only reads, and the act by the holder
    s.forget("adb");
    let command = |args: &[&str], extra: &[(&'static str, String)]| {
        let e = refs(&env(extra));
        let e: Vec<(&str, &str)> = e.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let mut full = vec!["android"];
        full.extend_from_slice(args);
        let r = s.run(&full, &e);
        let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|err| panic!("{err}: {}", out(&r)));
        (r.exit_code, v)
    };
    let (acted, refused) = command(&["tap-on", "--point", "10,10"], &[]);
    let (read, _) = command(&["get-device-state", "keyboard"], &[]);
    let (held, _) = command(&["tap-on", "--point", "10,10"], &[("WF_ANDROID_LEASE_ID", a.clone())]);
    // Then only the act is refused, before anything reaches the device, with exit 3 naming the holder
    assert_eq!((acted, refused["ok"].clone()), (3, json!(false)), "{refused}");
    assert!(refused["error"].as_str().unwrap().contains("leased by sandbox-a") && refused["next"].as_str().unwrap().contains("wf android lease"), "{refused}");
    assert_eq!((read, held), (0, 0));
    assert_eq!(s.calls("adb").iter().filter(|c| c.contains("input tap")).collect::<Vec<_>>(), ["-s emulator-5554 shell input tap 10 10"]);

    // When someone who holds nothing releases or heartbeats
    let stray_release = lease(&["release", "--id", "zzz"], &at(1000));
    let stray_beat = lease(&["heartbeat", "--id", "zzz"], &at(1000));
    // Then both are refused and the holder keeps the device
    assert_eq!(stray_release.exit_code, 1);
    assert!(out(&stray_release).contains("lease zzz is neither the holder nor waiting"), "{}", out(&stray_release));
    assert_eq!(stray_beat.exit_code, 1);
    assert!(out(&stray_beat).contains("is not the holder; sandbox-a holds the device"), "{}", out(&stray_beat));

    // When sandbox-c waits in line and the holder releases
    let mut opts = RunOpts::default().cwd(&s.repo).env("WF_ANDROID_LEASE_DIR", &dir).env("WF_REGISTRY_DIR", &s.reg_dir.display().to_string());
    opts.timeout = None;
    let mut waiter = spawn_wf(&["android", "lease", "--local", "--who", "sandbox-c", "--wait", "60"], &opts);
    wait_until(Duration::from_secs(20), "sandbox-c in the line", || result(&lease(&["status"], &at(1000)))["queue"][0]["who"] == "sandbox-c");
    let line = result(&lease(&["status"], &at(1000)));
    let released = lease(&["release", "--id", &a], &at(1000));
    // Then the line shows c first, and c gets the device on its next ask
    assert_eq!(line["queue"][0]["position"], 1, "{line}");
    assert_eq!((released.exit_code, result(&released)), (0, json!({ "action": "release", "leaseId": a })), "{}", out(&released));
    assert_eq!(waiter.wait(Duration::from_secs(20)), Some(0), "{}", waiter.out());
    let granted: Value = serde_json::from_str(waiter.out().lines().find(|l| l.starts_with('{')).unwrap()).unwrap();
    assert_eq!((granted["result"]["granted"].clone(), granted["result"]["who"].clone()), (json!(true), json!("sandbox-c")), "{granted}");
    let c = granted["result"]["leaseId"].as_str().unwrap().to_string();

    // When c heartbeats once, and then nobody hears from it past its 600 s TTL
    assert_eq!(lease(&["heartbeat", "--id", &c], &at(1000)).exit_code, 0);
    let taken_over = lease(&["--who", "sandbox-d", "--ttl", "10", "--no-wait"], &at(602_000));
    // Then the next asker gets the device
    assert_eq!(taken_over.exit_code, 0, "{}", out(&taken_over));
    let d = result(&taken_over)["leaseId"].as_str().unwrap().to_string();

    // When d heartbeats within its 10 s TTL and e asks after the original TTL would have run out
    assert_eq!(lease(&["heartbeat", "--id", &d], &at(609_000)).exit_code, 0);
    let refused = lease(&["--who", "sandbox-e", "--no-wait"], &at(613_000));
    // Then the heartbeat kept d's turn
    assert_eq!(refused.exit_code, 3, "a heartbeat keeps the holder: {}", out(&refused));

    // When a waiter crashes in line and d releases
    let mut ghost_opts = opts.clone().env("WF_LEASE_CLOCK_OFFSET_MS", "613000");
    ghost_opts.timeout = None;
    let mut ghost = spawn_wf(&["android", "lease", "--local", "--who", "ghost", "--wait", "60"], &ghost_opts);
    wait_until(Duration::from_secs(20), "the ghost in the line", || result(&lease(&["status"], &at(613_000)))["queue"][0]["who"] == "ghost");
    ghost.kill();
    assert_eq!(lease(&["release", "--id", &d], &at(613_000)).exit_code, 0);
    let behind_ghost = lease(&["--who", "sandbox-f", "--no-wait"], &at(613_000));
    let after_ghost = lease(&["--who", "sandbox-f", "--no-wait"], &at(648_000));
    // Then the next asker waits behind the ghost only until the ghost has stopped asking for 30 s
    assert_eq!(behind_ghost.exit_code, 3, "{}", out(&behind_ghost));
    assert!(out(&behind_ghost).contains("held by nobody"), "{}", out(&behind_ghost));
    assert_eq!(after_ghost.exit_code, 0, "{}", out(&after_ghost));
}

#[test]
fn a_lease_asked_from_a_sandbox_is_relayed_to_the_vm_where_its_state_lives() {
    // Given the VM answering the relayed lease commands with its own envelopes: a status, then a busy device
    let s = sandbox("wf-flow-android-relay-");
    let vm = FakeVm::start();
    let on_vm = |n: u16, envelope: Value, code: i64| {
        let job = answers::job_id(n);
        [answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(code), &format!("progress\n{envelope}\n")))]
    };
    vm.replies(&on_vm(1, json!({ "ok": true, "command": "lease", "platform": "android", "device": "rdl", "ms": 3, "result": { "holder": { "who": "ci" }, "queue": [] } }), 0));
    // When the status is asked without --local
    let status = crate::support::run_wf(&["android", "lease", "status", "--name", "rdl"], &vm.opts().cwd(&s.repo));
    // Then the VM ran the same command with --local and --json, and its result is this envelope's result
    assert_eq!((status.exit_code, last_json(&status)["result"].clone(), last_json(&status)["device"].clone()), (0, json!({ "holder": { "who": "ci" }, "queue": [] }), json!("rdl")), "{}", out(&status));
    let script = vm.requests()[0].body.clone();
    assert!(script.contains("'android' 'lease' 'status' '--name' 'rdl' '--json' --local"), "{script}");

    // When the device is busy on the VM
    vm.replies(&on_vm(2, json!({ "ok": false, "command": "lease", "platform": "android", "device": "rdl", "ms": 3, "error": "device busy: held by ci (position 1)", "next": "retry with --wait <seconds>" }), 3));
    let busy = crate::support::run_wf(&["android", "lease", "--who", "sandbox", "--no-wait"], &vm.opts().cwd(&s.repo));
    // Then the refusal, its next step and exit 3 come back as the VM gave them
    assert_eq!((busy.exit_code, last_json(&busy)["error"].clone(), last_json(&busy)["next"].clone()), (3, json!("device busy: held by ci (position 1)"), json!("retry with --wait <seconds>")), "{}", out(&busy));
}

#[test]
fn a_run_keeps_its_lease_through_a_read_only_wait_longer_than_the_ttl() {
    // Given a lease with a 2 s TTL, held by the run that is about to start
    let s = sandbox("wf-flow-android-renew-");
    let dir = s.repo.join("leases").display().to_string();
    let lease = |args: &[&str], offset: &str| {
        let mut full = vec!["android", "lease", "--local"];
        full.extend_from_slice(args);
        s.run(&full, &[("WF_ANDROID_LEASE_DIR", dir.as_str()), ("WF_LEASE_CLOCK_OFFSET_MS", offset)])
    };
    let granted = lease(&["--who", "the-run", "--ttl", "2"], "0");
    let id = last_json(&granted)["result"]["leaseId"].as_str().expect("a lease id").to_string();
    let lua = s.file("renew.lua", "device:extended_wait_until{condition=function() return not device:get_device_state('keyboard').visible end, timeout=4000, interval=500, optional=true}\ndevice:tap_on({point='10,10'})\nlog('tapped')");
    let xml = s.repo.join("ui.xml").display().to_string();
    let mut opts = RunOpts::default().bin_dir(&s.bin_dir).cwd(&s.repo).env("WF_REGISTRY_DIR", &s.reg_dir.display().to_string()).env("ANDROID_SERIAL", "emulator-5554");
    for (k, v) in [("WF_ANDROID_LEASE_DIR", dir.as_str()), ("WF_ANDROID_LEASE_ID", id.as_str()), ("WF_ANDROID_UIA", "dump"), ("STUB_UI_XML", xml.as_str())] {
        opts = opts.env(k, v);
    }
    opts.timeout = None;

    // When the run waits 4 s on a condition that only reads, and another sandbox asks for the device 3 s in
    let mut run = spawn_wf(&["android", "test", &lua, "--no-record"], &opts);
    std::thread::sleep(Duration::from_secs(3));
    let intruder = lease(&["--who", "intruder", "--no-wait"], "0");
    // Then the intruder is refused, because the run renewed its lease every third of the TTL, and the run's tap went through
    assert_eq!(intruder.exit_code, 3, "{}", out(&intruder));
    assert!(out(&intruder).contains("the-run"), "{}", out(&intruder));
    assert_eq!(run.wait(Duration::from_secs(30)), Some(0), "{}", run.out());
    assert!(run.out().contains("tapped"), "{}", run.out());
    assert!(s.calls("adb").iter().any(|c| c.ends_with("shell input tap 10 10")), "{:?}", s.calls("adb"));
    // and once the run is over nothing renews it: 3 s later the device is free for the next asker
    assert_eq!(lease(&["--who", "next", "--no-wait"], "3000").exit_code, 0);

    // When a run without that lease reaches a command that acts
    s.forget("adb");
    let script = s.file("refused.lua", "log('before')\ndevice:tap_on({point='10,10'})\nlog('after')");
    let refused = s.run(&["android", "test", &script, "--no-record"], &[("WF_ANDROID_LEASE_DIR", dir.as_str()), ("WF_ANDROID_UIA", "dump"), ("STUB_UI_XML", xml.as_str())]);
    // Then the script fails at that line naming the holder, and nothing reached the device
    assert_eq!(refused.exit_code, 1, "{}", out(&refused));
    assert!(refused.stderr.contains("refused.lua:2") && refused.stderr.contains("leased by next") && !refused.stdout.contains("after"), "{}", out(&refused));
    assert!(!s.calls("adb").iter().any(|c| c.contains("input tap")), "{:?}", s.calls("adb"));
}

#[test]
fn the_device_state_reads_only_the_parts_asked_for() {
    // Given a device with an overridden screen size, the keyboard up (its IME window taller than the keys, as on the CI device),
    // and a clip the recorder took
    let s = sandbox("wf-flow-android-state-");
    let state = |args: &[&str], extra: &[(&str, &str)]| -> (Value, Vec<String>) {
        s.forget("adb");
        let mut full = vec!["get-device-state"];
        full.extend_from_slice(args);
        let r = android(&s, &full, extra);
        assert_eq!(r.exit_code, 0, "{}", out(&r));
        (json_out(&r)["result"].clone(), s.calls("adb"))
    };
    // When each part is read, the keyboard also while it is down, and then the whole state
    let (screen, screen_calls) = state(&["screen"], &[]);
    let (keyboard, keyboard_calls) = state(&["keyboard"], &[]);
    let (down, down_calls) = state(&["keyboard"], &[("STUB_IME", "false")]);
    let (microphone, microphone_calls) = state(&["microphone"], &[]);
    let (all, all_calls) = state(&[], &[]);
    // Then the screen is the override size at dpr 1, the keyboard its IME frame (none while down), the microphone the clip
    // set last and taken, and each part costs only its own reads
    assert_eq!(screen, json!({ "width": 480, "height": 1040, "dpr": 1 }));
    assert_eq!(screen_calls, ["-s emulator-5554 shell wm size"]);
    assert_eq!(keyboard, json!({ "visible": true, "bounds": { "left": 0, "top": 610, "right": 480, "bottom": 1040 } }));
    assert_eq!(keyboard_calls, ["-s emulator-5554 shell dumpsys input_method", "-s emulator-5554 shell dumpsys window"]);
    assert_eq!((down, down_calls.len()), (json!({ "visible": false, "bounds": null }), 1));
    assert_eq!(microphone, json!({ "input": "/tmp/clip.wav", "consumed": true }));
    assert!(microphone_calls.len() == 1 && microphone_calls[0].contains("-newer /data/local/tmp/inj.marker"), "{microphone_calls:?}");
    assert_eq!((all["screen"].clone(), all["keyboard"].clone(), all["microphone"].clone()), (screen, keyboard, microphone));
    assert_eq!(all_calls.len(), 4, "{all_calls:?}");
}

/// The redroid packages a stock image has, and the catalog's core list.
fn stock_and_core() -> (HashSet<String>, HashSet<String>) {
    let src = repo_root().join("clis/harness-android/src");
    let stock = std::fs::read_to_string(src.join("android_stock_packages.txt")).unwrap().lines().map(str::to_string).collect();
    let catalog: Value = serde_json::from_str(&std::fs::read_to_string(src.join("android_features.json")).unwrap()).unwrap();
    let core = catalog["core"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect();
    (stock, core)
}

/// The `dis="..."` and `ena="..."` package lists of a printed up script.
fn package_lists(script: &str) -> (Vec<String>, Vec<String>) {
    let line = script.lines().find(|l| l.starts_with("dis=")).unwrap_or_else(|| panic!("no dis= line: {script}"));
    let list = |key: &str| line.split(&format!("{key}=\"")).nth(1).and_then(|t| t.split('"').next()).unwrap_or("").split_whitespace().map(str::to_string).collect();
    (list("dis"), list("ena"))
}

#[test]
fn the_lab_scripts_boot_a_slim_redroid_safely_and_report_its_features() {
    let s = sandbox("wf-flow-android-lab-");
    let lab = |args: &[&str]| android(&s, args, &[]);
    let script_of = |r: &RunResult| -> String { json_out(r)["result"]["script"].as_str().unwrap_or_else(|| panic!("no script: {}", out(r))).to_string() };

    // When the default start script is printed
    let up = lab(&["start-device", "--dry-run"]);
    let script = script_of(&up);
    // Then it boots redroid with the adopted screen, the tailnet DNS, a 1.5 CPU cap and a product name Expo Go takes for an
    // emulator (EAS refuses an anonymous real device an update that requires sign-in), and keeps a running device as it is
    assert_eq!((up.exit_code, json_out(&up)["device"].clone(), json_out(&up)["result"]["dryRun"].clone()), (0, json!("rdl"), json!(true)), "{}", out(&up));
    for needle in ["docker run -d --name rdl --privileged --cpus 1.5 -p 5555:5555", "androidboot.redroid_width=480", "androidboot.redroid_height=1040", "androidboot.redroid_dpi=213", "androidboot.redroid_net_dns1=100.100.100.100", "ro.product.name=wf_redroid_emulator", "sys.boot_completed", "docker inspect -f '{{.State.Running}}' rdl", "already up'; exit 0"] {
        assert!(script.contains(needle), "missing {needle}: {script}");
    }
    // and a running device at another size or product is stopped gracefully before booting at this one
    let keep = script.find("redroid_width=480 androidboot.redroid_height=1040 androidboot.redroid_dpi=213 ro.product.name=wf_redroid_emulator '").expect("compares the running screen");
    let restop = script.find("runs another screen: stopping it").expect("stops a device at another size");
    assert!(keep < restop && script[restop..].contains("docker stop -t 30 rdl"), "{script}");
    // and memory is checked before a fresh boot, an idle image builder is stopped first, and the biggest holders are named
    let (builder, gate) = (script.find("docker stop -t 10 buildx_buildkit_agent-shell-builder0"), script.find("a device needs about 700 MB"));
    assert!(builder.is_some() && gate.is_some() && builder < gate && script.contains("! pgrep -f 'docker buildx build'"), "{script}");
    assert!(script.find("-lt 700").expect("a memory guard") < script.find("boot_fresh || exit 1").expect("a fresh boot") && script.contains("docker stats --no-stream"), "{script}");
    // and the slim set is disabled, proved to boot after a restart, and rolled back from a fresh volume when it does not
    assert!(script.contains("pm disable-user --user 0") && script.contains("docker restart rdl") && script.contains("service check activity") && script.contains("/data/local/wf-slim-proof"), "{script}");
    let tail = &script[script.find("recreating the device from a fresh volume").expect("a rollback branch")..];
    let (rm, vol, boot) = (tail.find("docker rm -f rdl").unwrap(), tail.find("docker volume rm rdl-data").unwrap(), tail.find("boot_fresh").unwrap());
    assert!(rm < vol && vol < boot, "remove the container before the state, then boot: {tail}");
    assert!(!script.contains("rm -f /data/system") && !script.contains("expogo.apk"), "{script}");
    // and the core packages (launcher, input method, webview, system UI...) are never disabled
    let (stock, core) = stock_and_core();
    let (disabled, _) = package_lists(&script);
    assert!(disabled.iter().any(|p| p == "com.android.contacts"), "{disabled:?}");
    for keep in ["com.android.inputmethod.latin", "com.android.webview", "com.android.systemui", "com.android.permissioncontroller", "com.android.sdksandbox", "com.android.launcher3", "com.android.settings"] {
        assert!(core.contains(keep) && !disabled.iter().any(|p| p == keep), "{keep} must be core and never disabled");
    }

    // When the stock image is asked for
    let stock_up = script_of(&lab(&["start-device", "--dry-run", "--stock"]));
    // Then nothing is disabled and exactly the stock image's non-core packages are enabled: every package is classified
    let (dis, ena) = package_lists(&stock_up);
    assert!(dis.is_empty() && stock_up.contains("pm enable"), "{stock_up}");
    let listed = ena.len();
    let ena: HashSet<String> = ena.into_iter().collect();
    assert_eq!(listed, ena.len(), "a package is classified twice");
    let non_core: HashSet<String> = stock.difference(&core).cloned().collect();
    assert_eq!(ena, non_core, "classify every stock package in android_features.json, and only those");
    assert!(core.is_subset(&stock), "core lists a package the stock image does not have: {:?}", core.difference(&stock).collect::<Vec<_>>());

    // When a feature, a screen, a second device's name and port, and Expo Go are asked for (with the equals form and a repeat)
    let camera = script_of(&lab(&["start-device", "--dry-run", "--with", "camera"]));
    let screen = script_of(&lab(&["start-device", "--dry-run", "--screen", "1x1@1", "--screen", "720x1280@320"]));
    let second = lab(&["start-device", "--dry-run", "--name=rdl-bench", "--port", "5556"]);
    let expo = script_of(&lab(&["start-device", "--expo-go", "--dry-run"]));
    // Then camera packages are enabled while the rest stay off, and an explicit --with applies to a running device
    let (dis, ena) = package_lists(&camera);
    assert!(ena.iter().any(|p| p == "com.android.camera2") && !dis.iter().any(|p| p == "com.android.camera2") && dis.iter().any(|p| p == "com.android.contacts"), "{camera}");
    assert!(camera.contains("already up'; dis="), "{camera}");
    // and the last --screen wins, the equals form names the second device, and Expo Go goes in after the boot
    assert!(screen.contains("androidboot.redroid_width=720") && screen.contains("androidboot.redroid_dpi=320"), "{screen}");
    assert_eq!(json_out(&second)["device"], json!("rdl-bench"));
    assert!(script_of(&second).contains("docker run -d --name rdl-bench --privileged --cpus 1.5 -p 5556:5555 -v rdl-bench-data:/data"), "{}", out(&second));
    assert!(expo.contains("Expo-Go-57.0.9.apk") && expo.contains("pm install -r /data/local/tmp/expogo.apk"), "{expo}");
    assert!(expo.rfind("expogo.apk").unwrap() > expo.find("boot_fresh || exit 1").unwrap(), "Expo Go goes in after the device booted");
    // and WF_ANDROID_NAME names the container when --name does not
    assert!(script_of(&android(&s, &["start-device", "--dry-run"], &[("WF_ANDROID_NAME", "rdl-2")])).contains("docker run -d --name rdl-2 "));

    // When the screen is malformed, the feature unknown, or the script is asked for with the old --print
    let bad_screen = lab(&["start-device", "--dry-run", "--screen", "wide"]);
    let bad_feature = lab(&["start-device", "--dry-run", "--with", "camera,teleport"]);
    let print = lab(&["stop-device", "--print"]);
    // Then each is a usage error with no script, saying what is wrong
    for (r, why) in [(&bad_screen, "--screen must look like 480x1040@213"), (&bad_feature, "unknown feature 'teleport'"), (&print, "unexpected argument '--print'")] {
        let v = json_out(r);
        assert!(r.exit_code == 2 && v["error"].as_str().unwrap().contains(why) && v["result"].is_null() && v["usage"].as_str().unwrap().starts_with("wf android "), "{v}");
    }
    assert!(json_out(&bad_feature)["error"].as_str().unwrap().contains("camera"), "{}", out(&bad_feature));

    // When the stop script is printed
    let down = script_of(&lab(&["stop-device", "--dry-run"]));
    // Then Android is stopped gracefully before the container is removed, and the memory freed is reported
    let (stop, rm) = (down.find("docker stop -t 30 rdl"), down.find("docker rm -f rdl"));
    assert!(stop.is_some() && rm.is_some() && stop < rm && down.contains("MemAvailable"), "{down}");

    // Given the VM answers the device probe: booted, proven, the camera set and contacts disabled; and a lease held on it
    let vm = FakeVm::start();
    let catalog: Value = serde_json::from_str(&std::fs::read_to_string(repo_root().join("clis/harness-android/src/android_features.json")).unwrap()).unwrap();
    let camera_packages: Vec<String> = catalog["features"].as_array().unwrap().iter().find(|f| f["name"] == "camera").unwrap()["packages"].as_array().unwrap().iter().map(|p| format!("package:{}", p.as_str().unwrap())).collect();
    let output = format!("1\nService activity: found\n@@PROOF ok\n@@DISABLED\n{}\npackage:com.android.contacts\n", camera_packages.join("\n"));
    let job = answers::job_id(1);
    vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(0), &output))]);
    let leases = s.repo.join("leases").display().to_string();
    assert_eq!(android(&s, &["lease", "--local", "--who", "ci"], &[("WF_ANDROID_LEASE_DIR", leases.as_str())]).exit_code, 0);
    // When the devices are listed
    let listed = crate::support::run_wf(&["android", "list-devices"], &vm.opts().cwd(&s.repo).env("WF_ANDROID_LEASE_DIR", &leases));
    // Then the one device has each feature on, off or partial, its boot, the restart proof and its lease
    assert_eq!(listed.exit_code, 0, "{}", out(&listed));
    let devices = last_json(&listed)["result"]["devices"].clone();
    let device = &devices[0];
    let state = |n: &str| device["features"].as_array().unwrap().iter().find(|f| f["feature"] == n).unwrap()["state"].as_str().unwrap().to_string();
    assert_eq!((devices.as_array().unwrap().len(), device["name"].clone()), (1, json!("rdl")), "{devices}");
    assert_eq!((state("camera"), state("contacts"), state("search")), ("off".to_string(), "partial".to_string(), "on".to_string()), "{device}");
    assert_eq!((device["boot"]["completed"].clone(), device["boot"]["activityService"].clone(), device["restartProven"].clone()), (json!(true), json!(true), json!(true)));
    assert!(device["disabledTotal"].as_u64().unwrap() > 1 && device["lease"]["holder"]["who"] == "ci", "{device}");
    assert!(vm.requests()[0].body.contains("pm list packages -d"), "{:?}", vm.requests());

    // When no such container runs
    let job = answers::job_id(2);
    vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(0), "@@ABSENT\n"))]);
    let absent = crate::support::run_wf(&["android", "list-devices", "--name", "rdl-gone"], &vm.opts().cwd(&s.repo));
    // Then the answer is an empty list, not a failure
    assert_eq!((absent.exit_code, last_json(&absent)["result"].clone(), last_json(&absent)["device"].clone()), (0, json!({ "devices": [] }), json!("rdl-gone")), "{}", out(&absent));
    assert!(vm.requests()[0].body.contains("docker inspect -f") && vm.requests()[0].body.contains("rdl-gone"), "{:?}", vm.requests());
}

/// A 100 by 220 screen: white, a grey outline block and a green vertical guide at x 48..52, y 120..180 inside the canvas.
fn guide_screen() -> Vec<u8> {
    png_rgb(100, 220, |x, y| {
        if (48..=52).contains(&x) && (120..=180).contains(&y) {
            [165, 205, 185]
        } else if (35..=65).contains(&x) && (110..=190).contains(&y) {
            [225, 225, 225]
        } else {
            [255, 255, 255]
        }
    })
}

fn png_rgb(w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let data: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (y, x))).flat_map(|(y, x)| pixel(x, y)).collect();
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&data).unwrap();
    out
}

fn png_size(path: &Path) -> (u32, u32) {
    let d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let r = d.read_info().unwrap();
    (r.info().width, r.info().height)
}

fn json_out(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", out(r)))
}

#[test]
fn a_runbook_looks_up_taps_types_draws_and_records_through_the_uiautomator_dump() {
    // Given a claimed emulator whose screen is the dump fixture
    let s = sandbox("wf-flow-android-input-");
    let input = |args: &[&str], extra: &[(&str, &str)]| {
        s.forget("adb");
        android(&s, args, extra)
    };
    let result = |r: &RunResult| json_out(r)["result"].clone();

    // When elements are looked up by id (package prefix ignored), by a text pattern, by text with an entity, and one is missing
    let by_id = input(&["get-element", "--id", "TamilWriteCheck"], &[]);
    let calls = s.calls("adb");
    let by_text = input(&["get-element", "--text", "looks right.*"], &[]);
    let decoded = input(&["get-element", "Hi & bye"], &[]);
    let missing = input(&["get-element", "--id", "Nope"], &[]);
    // Then the bounds and centre come from one dump, the text is matched whole and case-insensitively, and a miss is a null
    // answer, not a failure
    assert_eq!(result(&by_id)["bounds"], json!({ "left": 100, "top": 200, "right": 300, "bottom": 260, "cx": 200, "cy": 230 }), "{}", out(&by_id));
    assert_eq!(calls, [DUMP, READ]);
    assert_eq!(result(&by_text)["text"], json!("LOOKS RIGHT · 92"), "{}", out(&by_text));
    assert_eq!(result(&decoded)["bounds"]["right"], json!(10), "{}", out(&decoded));
    assert_eq!((missing.exit_code, result(&missing)), (0, Value::Null), "{}", out(&missing));

    // When a node is tapped by id, waited for while present, and asserted while absent
    let tap = input(&["tap-on", "--id", "TamilWriteCheck"], &[]);
    let tap_calls = s.calls("adb");
    let present = input(&["assert-visible", "--id", "TamilWriteVerdictBadge", "--timeout", "2000"], &[]);
    let present_calls = s.calls("adb");
    let absent = input(&["assert-visible", "--id", "Never", "--timeout", "0", "--label", "never there"], &[]);
    // Then the tap lands on the centre once a second look shows the node holding still, the wait answers the element its one
    // look saw (no second dump), and the miss fails at once with its evidence
    assert_eq!((tap.exit_code, tap_calls), (0, vec![DUMP.to_string(), READ.to_string(), DUMP.to_string(), READ.to_string(), "-s emulator-5554 shell input tap 200 230".to_string()]));
    assert_eq!((present.exit_code, result(&present)["value"]["text"].clone(), present_calls), (0, json!("LOOKS RIGHT · 92"), vec![DUMP.to_string(), READ.to_string()]), "{}", out(&present));
    let failure = json_out(&absent);
    assert_eq!((absent.exit_code, failure["error"].clone()), (1, json!("timed out after 0 ms")), "{failure}");
    assert!(std::path::Path::new(failure["evidence"]["hierarchy"].as_str().unwrap()).exists() && failure["next"].is_string(), "{failure}");

    // When a point is tapped and text typed with an input log, the keyboard up; then with it down; then a character the
    // shell cannot carry
    let log = s.repo.join("input.log").display().to_string();
    let logged = [("WF_ANDROID_INPUT_LOG", log.as_str())];
    assert_eq!(input(&["tap-on", "--point", "5,6"], &logged).exit_code, 0);
    let typed = input(&["input-text", "a_b c\nd'e\tf"], &logged);
    let script = s.calls("adb").into_iter().find(|c| c.contains("shell input")).expect("a shell input call");
    let no_keyboard = input(&["input-text", "abc"], &[("STUB_IME", "false")]);
    let no_keyboard_calls = s.calls("adb");
    let percent = input(&["input-text", "50%"], &[]);
    // Then one shell call sends each run of characters as one input and newline, quote and tab as keys; the others are refused
    assert_eq!(typed.exit_code, 0, "{}", out(&typed));
    let sent: Vec<&str> = script.split_once("shell ").unwrap().1.split("; ").filter(|c| c.starts_with("input")).collect();
    assert_eq!(sent, ["input text 'a_b%sc'", "input keyevent 66", "input text 'd'", "input keyevent 75", "input text 'e'", "input keyevent 61", "input text 'f'"], "{script}");
    assert_eq!(no_keyboard.exit_code, 1);
    assert!(json_out(&no_keyboard)["error"].as_str().unwrap().contains("keyboard") && !no_keyboard_calls.iter().any(|c| c.contains("input text") || c.contains("input keyevent")), "{}", out(&no_keyboard));
    assert_eq!(percent.exit_code, 1);
    assert!(json_out(&percent)["error"].as_str().unwrap().contains('%') && !s.calls("adb").iter().any(|c| c.contains("input text")), "{}", out(&percent));

    // When named keys are pressed, and Back with the keyboard up and with it down
    let backspace = input(&["press-key", "Backspace"], &logged);
    let backspace_calls = s.calls("adb");
    let left = input(&["press-key", "Remote Dpad Left"], &[]);
    let left_calls = s.calls("adb");
    let ctrl_end = input(&["press-key", "Ctrl+End"], &[]);
    let ctrl_end_calls = s.calls("adb");
    let back_up = input(&["back"], &[]);
    let back = input(&["back"], &[("STUB_IME", "false")]);
    let back_calls = s.calls("adb");
    let unknown = input(&["press-key", "hyper"], &[]);
    // Then each key is one keyevent and Ctrl+End a key combination, Back refuses while the keyboard is up (a user taps Done),
    // an unknown name is a usage error listing Maestro's names, and the tap, the text and the key are logged without the screen
    assert_eq!((backspace.exit_code, left.exit_code, ctrl_end.exit_code, back.exit_code, unknown.exit_code), (0, 0, 0, 0, 2), "{}", out(&unknown));
    assert!(backspace_calls.iter().any(|c| c.ends_with("shell input keyevent 67")), "{backspace_calls:?}");
    assert!(left_calls.iter().any(|c| c.ends_with("shell input keyevent 21")), "{left_calls:?}");
    assert!(ctrl_end_calls.iter().any(|c| c.ends_with("shell input keycombination 113 123")), "{ctrl_end_calls:?}");
    assert_eq!(back_up.exit_code, 1);
    assert!(json_out(&back_up)["error"].as_str().unwrap().contains("tap Done"), "{}", out(&back_up));
    assert!(back_calls.iter().any(|c| c.ends_with("shell input keyevent 4")), "{back_calls:?}");
    assert!(json_out(&unknown)["error"].as_str().unwrap().contains("Remote Dpad Left"), "{}", out(&unknown));
    let lines: Vec<Value> = std::fs::read_to_string(&log).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.iter().map(|l| l["verb"].as_str().unwrap()).collect::<Vec<_>>(), ["tap", "type", "press"]);
    assert_eq!((lines[0]["args"].clone(), lines[1]["args"].clone(), lines[2]["args"].clone()), (json!(["5", "6"]), json!(["a_b c\nd'e\tf"]), json!(["backspace"])));
    assert!(lines[0]["at"].as_i64().unwrap() > 0 && lines[2]["at"].as_i64().unwrap() >= lines[0]["at"].as_i64().unwrap());

    // When the JS logs are read from logcat, checked for none (the old warnings --fail), and cleared
    let noisy = "W/ReactNativeJS( 1234): Response.blob() is using React Native's blob\nI/ReactNativeJS( 1234): fine\nE/ReactNativeJS( 1234): boom\n";
    let listed = input(&["get-logs"], &[("STUB_LOGCAT", noisy)]);
    let listed_calls = s.calls("adb");
    let info = input(&["get-logs", "--level", "info"], &[("STUB_LOGCAT", noisy)]);
    let none = ["extended-wait-until", "--condition", "#device:get_logs() == 0", "--timeout", "0"];
    let failed = input(&none, &[("STUB_LOGCAT", noisy)]);
    let clean = input(&none, &[("STUB_LOGCAT", "I/ReactNativeJS( 1): ok\n")]);
    let cleared = input(&["clear-logs"], &[]);
    // Then the warning and the error are listed by level (the info line only when asked for) from a bounded logcat, the check
    // for none fails on them and passes on a clean log, and clearing runs logcat -c
    assert_eq!(result(&listed), json!(["warn: Response.blob() is using React Native's blob", "error: boom"]), "{}", out(&listed));
    assert_eq!(result(&info).as_array().map(Vec::len), Some(3));
    assert!(listed_calls.iter().any(|c| c.contains("logcat") && c.contains("-d -t 5000")), "a bare logcat -d never returns on the emulator: {listed_calls:?}");
    assert_eq!((failed.exit_code, clean.exit_code), (1, 0), "{}{}", out(&failed), out(&clean));
    assert!(cleared.exit_code == 0 && s.calls("adb").iter().any(|c| c.contains("logcat -c")), "{:?}", s.calls("adb"));

    // When a recording starts, and stops from another process with one segment on the device, then with three listed out of order
    let video = s.repo.join("run.mp4").display().to_string();
    let started = input(&["start-recording", &video], &[]);
    let start_call = s.calls("adb").into_iter().find(|c| c.contains("screenrecord")).expect("the recorder's loop");
    let stopped = input(&["stop-recording"], &[("STUB_RECORDING", &video)]);
    let stop_calls = s.calls("adb");
    let many = input(&["stop-recording"], &[("STUB_RECORDING", &video), ("STUB_SEGMENTS", "/sdcard/wf-record-10.mp4\\n/sdcard/wf-record-2.mp4\\n/sdcard/wf-record-1.mp4\\n")]);
    // Then screenrecord runs detached in 170 s segments noting the path, stop reads that path off the device, ends the loop and
    // waits for screenrecord's exit in one shell call, pulls the file, and segments are pulled in numeric order
    assert_eq!((started.exit_code, result(&started)), (0, json!({ "path": video })), "{}", out(&started));
    assert!(start_call.contains("screenrecord --time-limit 170 /sdcard/wf-record-$i.mp4") && start_call.contains("nohup") && start_call.contains(&video), "{start_call}");
    assert_eq!(stopped.exit_code, 0, "{}", out(&stopped));
    assert!(stop_calls[0].contains("wf-record.on"), "{stop_calls:?}");
    assert_eq!(stop_calls[1..], [STOP_RECORDER, "-s emulator-5554 shell ls /sdcard/wf-record-*.mp4", &format!("-s emulator-5554 pull /sdcard/wf-record-1.mp4 {video}")]);
    assert_eq!(result(&stopped), json!({ "files": [video] }));
    let base = video.trim_end_matches(".mp4");
    let pulls: Vec<String> = s.calls("adb").into_iter().filter(|c| c.contains(" pull ")).collect();
    assert_eq!(pulls, [format!("-s emulator-5554 pull /sdcard/wf-record-1.mp4 {video}"), format!("-s emulator-5554 pull /sdcard/wf-record-2.mp4 {base}-2.mp4"), format!("-s emulator-5554 pull /sdcard/wf-record-10.mp4 {base}-3.mp4")]);
    assert_eq!(result(&many)["files"].as_array().map(Vec::len), Some(3));

    // When the green guide on the practice pad is drawn evenly, as a hand would, and from a source wf does not know
    let screen = s.file("screen.png", guide_screen());
    let even = input(&["draw-strokes-on", "--id", "TamilPracticeCanvasSurface", "--source", "guide", "--human=false"], &[("STUB_PNG", screen.as_str())]);
    let even_calls = s.calls("adb");
    let human = input(&["draw-strokes-on", "--id", "TamilPracticeCanvasSurface", "--source", "guide"], &[("STUB_PNG", screen.as_str()), ("WF_ANDROID_INPUT_LOG", log.as_str())]);
    let human_calls = s.calls("adb");
    let svg = input(&["draw-strokes-on", "--id", "TamilPracticeCanvasSurface", "--source", "svg"], &[]);
    // Then the pad is looked up (twice, to see it hold still), its guide read off a screenshot into one stroke and drawn from
    // its top in one adb shell script (a frame apart when even, re-timed with pauses as a hand), the stroke is logged, and svg
    // is a usage error
    assert_eq!((even.exit_code, result(&even)["strokes"].clone()), (0, json!(1)), "{}", out(&even));
    assert_eq!((&even_calls[..5], even_calls.len()), (&[DUMP.to_string(), READ.to_string(), DUMP.to_string(), READ.to_string(), "-s emulator-5554 exec-out screencap -p".to_string()][..], 6), "{even_calls:?}");
    let down = even_calls[5].split("input motionevent DOWN ").nth(1).unwrap();
    let mut xy = down.split([' ', ';']).filter(|t| !t.is_empty()).take(2).map(|t| t.parse::<f64>().unwrap());
    let (x, y) = (xy.next().unwrap(), xy.next().unwrap());
    assert!((x - 50.0).abs() <= 3.0 && (118.0..=126.0).contains(&y) && even_calls[5].contains("input motionevent UP"), "{x} {y} in {}", even_calls[5]);
    assert!(even_calls[5].contains("; sleep 0.016; ") && even_calls[5].contains("input motionevent MOVE"), "{}", even_calls[5]);
    let stroke = human_calls.last().unwrap();
    assert!(human.exit_code == 0 && stroke.contains("input motionevent DOWN") && stroke.contains("sleep ") && stroke.matches("input motionevent MOVE").count() > 3 && stroke.contains("input motionevent UP"), "{stroke}");
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().last().map(|l| serde_json::from_str::<Value>(l).unwrap()["verb"].clone()), Some(json!("stroke")));
    assert_eq!(svg.exit_code, 2, "{}", out(&svg));
}

#[test]
fn the_lesson_pads_as_redroid_shows_them_are_traced_along_the_whole_guide() {
    // Given each real lesson pad as the whole screen, the canvas covering it
    for (pad, (w, h), start, end, min_length, bowl_y) in [("tamil-guide-pad.png", (644, 596), (333.0, 132.0), (557.0, 115.0), 1500.0, 380.0), ("tamil-guide-pad-aa.png", (644, 558), (282.0, 89.0), (459.0, 98.0), 1100.0, 290.0)] {
        let s = sandbox("wf-flow-android-pad-");
        let png = fixtures_dir().join("trace").join(pad);
        assert_eq!(png_size(&png), (w, h));
        s.file("ui.xml", format!(r#"<hierarchy><node resource-id="TamilPracticeCanvasSurface" text="" bounds="[0,0][{w},{h}]" /></hierarchy>"#));
        // When the guide is drawn evenly
        let r = android(&s, &["draw-strokes-on", "--id", "TamilPracticeCanvasSurface", "--source", "guide", "--human=false"], &[("STUB_PNG", &png.display().to_string())]);
        // Then one stroke starts at the start dot, ends at the arrow, and tours the ring, the bowl and the vertical
        assert_eq!((r.exit_code, json_out(&r)["result"]["strokes"].clone()), (0, json!(1)), "{pad}: {}", out(&r));
        let script = s.calls("adb").into_iter().find(|c| c.contains("input motionevent DOWN")).expect("the stroke's script");
        let path: Vec<(f64, f64)> = script
            .split("; ")
            .filter_map(|part| part.split("input motionevent ").nth(1))
            .map(|ev| {
                let w: Vec<f64> = ev.split(' ').skip(1).take(2).map(|t| t.parse().unwrap()).collect();
                (w[0], w[1])
            })
            .collect();
        let (first, last) = (path[0], path[path.len() - 1]);
        assert!((first.0 - start.0).abs() < 8.0 && (first.1 - start.1).abs() < 8.0, "{pad}: starts at {first:?}");
        assert!((last.0 - end.0).abs() < 8.0 && (last.1 - end.1).abs() < 12.0, "{pad}: ends at {last:?}");
        let length: f64 = path.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
        assert!(length > min_length && path.iter().any(|p| p.1 > bowl_y), "{pad}: the walk must cover the ring, the bowl and the vertical ({length} px)");
    }
}

#[test]
fn a_traced_letter_goes_on_to_its_next_stroke_as_soon_as_that_strokes_guide_shows() {
    // Given the Tamil writing test case and a lesson pad that opens on a three-stroke letter: each stroke brings the next
    // stroke's guide with the coach's "Stroke n of 3", the last one the score badge
    let s = sandbox("wf-flow-android-tamil-");
    s.file("e2e/tamil-writing.lua", std::fs::read(repo_root().join("e2e/tamil-writing.lua")).unwrap());
    s.file("ui.xml", r#"<hierarchy><node resource-id="" text="Expo Go" bounds="[0,0][480,80]" /></hierarchy>"#);
    let badge = r#"<node resource-id="TamilPracticeStrokeScore" text="100% ON THE LINE" bounds="[320,620][470,660]" />"#;
    let screens: Vec<String> = [("Stroke 1 of 3", ""), ("Stroke 2 of 3", ""), ("Stroke 3 of 3", ""), ("All 3 strokes written", badge)]
        .iter()
        .enumerate()
        .map(|(n, (coach, extra))| s.file(&format!("pad-{n}.xml"), format!(r#"<hierarchy><node resource-id="TamilPracticeCanvasSurface" text="" bounds="[0,0][644,596]" /><node resource-id="TamilPracticeStrokeStep" text="{coach}" bounds="[20,620][300,660]" />{extra}</hierarchy>"#)))
        .collect();
    let seq = s.file("screens", format!("/tamil/uyirmei-k|{}\ninput motionevent UP|{}\ninput motionevent UP|{}\ninput motionevent UP|{}\n", screens[0], screens[1], screens[2], screens[3]));
    let png = fixtures_dir().join("trace/tamil-guide-pad.png").display().to_string();

    // When the test case traces the letter
    let out_dir = s.repo.join("out").display().to_string();
    let r = android(&s, &["test", "e2e/tamil-writing.lua", "--no-record", "--test-output-dir", "rec", "--update-goldens", "--goldens-dir", "goldens", "--env", "part=consonant", "--env", "base=exp://127.0.0.1:20000/--", "--env", &format!("out={out_dir}")], &[("STUB_PNG", &png), ("STUB_UI_SEQ", &seq)]);

    // Then each stroke is drawn once, and the wait after it ends when the next stroke's guide shows rather than after its
    // 2 s bound; the last one ends at the badge, whose score is read
    let text = out(&r);
    assert_eq!(r.exit_code, 0, "{text}");
    let report: Value = serde_json::from_str(&std::fs::read_to_string(s.repo.join("rec/report.json")).unwrap()).unwrap();
    let (waits, gaps) = crate::support::after_each_draw(&report);
    assert_eq!(waits, ["hit", "hit", "hit"], "{gaps:?}");
    assert!(gaps.iter().all(|&ms| ms < 2000), "ms from one stroke to the next: {gaps:?}");
    assert_eq!(s.calls("adb").iter().filter(|c| c.contains("input motionevent UP")).count(), 3);
    assert!(text.contains("uyirmei-k letter 1 attempt 2: drew=true, next guide: Stroke 3 of 3") && text.contains("uyirmei-k/1\t100% ON THE LINE\tok"), "{text}");
}

/// A stand-in for the on-device UI server: answers `/ping` while the started-marker file exists (or always without one), and the
/// JSON-RPC methods `wf` uses with canned replies, logging each (and every lookup's selector). With `dead_once`, every selector
/// misses until a `deviceInfo` fails. Its screen: TamilWriteCheck (labelled "Check my writing"), a Card with a matching node
/// inside it, the practice pad (100 by 220, as `guide_screen`), and the code editor's web view hosting an EditText.
pub(crate) struct FakeUiServer {
    pub(crate) url: String,
    pub(crate) port: u16,
    seen: Arc<Mutex<Vec<String>>>,
    selectors: Arc<Mutex<Vec<Value>>>,
}

impl FakeUiServer {
    pub(crate) fn start(alive_marker: Option<std::path::PathBuf>, dead_once: bool) -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let selectors = Arc::new(Mutex::new(Vec::new()));
        let (log, asked) = (seen.clone(), selectors.clone());
        std::thread::spawn(move || {
            let mut dead = dead_once;
            for mut request in server.incoming_requests() {
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(request.as_reader(), &mut body);
                let params = || serde_json::from_str::<Value>(&body).map(|v| v["params"].to_string()).unwrap_or_default();
                let method = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["method"].as_str().map(str::to_string)).unwrap_or_default();
                let reply = if request.url() == "/ping" {
                    if alive_marker.as_ref().is_none_or(|m| m.exists()) { "pong".to_string() } else { "down".to_string() }
                } else {
                    let ok = |result: Value| json!({ "jsonrpc": "2.0", "id": 1, "result": result }).to_string();
                    let miss = json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32001, "message": "UiSelector[RESOURCE_ID_REGEX=x]", "data": "UiSelector[RESOURCE_ID_REGEX=x]" } }).to_string();
                    let mut log = log.lock().unwrap();
                    match method.as_str() {
                        "deviceInfo" if dead => {
                            log.push("deviceInfo dead".into());
                            dead = false;
                            json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "java.lang.NullPointerException" } }).to_string()
                        }
                        "objInfoOfAllInstances" | "count" => {
                            let sel = serde_json::from_str::<Value>(&body).map(|v| v["params"][0].clone()).unwrap_or_default();
                            asked.lock().unwrap().push(sel.clone());
                            let check = json!({ "bounds": { "left": 100, "top": 200, "right": 300, "bottom": 260 }, "text": null, "contentDescription": "Check my writing", "className": "android.view.ViewGroup" });
                            let found: Vec<Value> = if dead {
                                vec![]
                            } else if body.contains("TamilWriteCheck") || (sel["descriptionMatches"].is_string() && body.contains("writing")) {
                                vec![check]
                            } else if body.contains("Card") {
                                let card = |b: [i64; 4], visible: [i64; 4]| json!({ "bounds": { "left": b[0], "top": b[1], "right": b[2], "bottom": b[3] }, "visibleBounds": { "left": visible[0], "top": visible[1], "right": visible[2], "bottom": visible[3] }, "text": "Card" });
                                vec![card([0, 0, 400, 400], [0, 0, 400, 400]), card([100, 200, 300, 260], [100, 200, 300, 230]), card([0, 900, 400, 1000], [0, 0, 0, 0])]
                            } else if body.contains("Chain") {
                                let n = |id: &str, b: [i64; 4], text: &str| json!({ "bounds": { "left": b[0], "top": b[1], "right": b[2], "bottom": b[3] }, "text": text, "resourceName": id, "className": "android.view.ViewGroup" });
                                vec![n("ChainA", [0, 200, 400, 600], ""), n("ChainB", [0, 200, 400, 400], ""), n("ChainC", [0, 200, 200, 300], "c"), n("ChainD", [0, 400, 400, 600], "d")]
                            } else if body.contains("Plain") {
                                vec![json!({ "bounds": { "left": 0, "top": 700, "right": 40, "bottom": 710 }, "text": null, "resourceName": "Plain", "className": "android.view.View" })]
                            } else if body.contains("Twice") {
                                vec![json!({ "bounds": { "left": 0, "top": 800, "right": 100, "bottom": 840 }, "text": "Twice", "contentDescription": "Twice", "className": "android.widget.TextView" })]
                            } else if body.contains("TamilPracticeCanvasSurface") {
                                vec![json!({ "bounds": { "left": 0, "top": 0, "right": 100, "bottom": 220 }, "text": null, "resourceName": "TamilPracticeCanvasSurface", "className": "android.view.ViewGroup" })]
                            } else if body.contains("CodeEditorInput") {
                                vec![json!({ "bounds": { "left": 0, "top": 300, "right": 540, "bottom": 900 }, "text": null, "className": "android.webkit.WebView" })]
                            } else if body.contains("Unacked") {
                                vec![json!({ "bounds": { "left": 433, "top": 675, "right": 480, "bottom": 734 }, "text": null, "resourceName": "Unacked", "className": "android.view.ViewGroup" })]
                            } else if body.contains("EditorWrap") {
                                vec![json!({ "bounds": { "left": 17, "top": 186, "right": 463, "bottom": 524 }, "text": null, "resourceName": "EditorWrap", "className": "android.widget.FrameLayout" })]
                            } else {
                                vec![]
                            };
                            let what = if found.is_empty() { "miss" } else { "hit" };
                            if method == "count" {
                                log.push(format!("count {what}"));
                                ok(json!(found.len()))
                            } else {
                                log.push(format!("objInfoOfAllInstances {}", if body.contains("TamilWriteCheck") && !found.is_empty() { "TamilWriteCheck" } else { what }));
                                ok(json!(found))
                            }
                        }
                        "objInfo" if body.contains("\"child\"") && body.contains("android.widget.EditText") => {
                            log.push("objInfo child EditText".into());
                            ok(json!({ "bounds": { "left": 0, "top": 300, "right": 540, "bottom": 900 }, "text": "def f():\n    pass", "className": "android.widget.EditText" }))
                        }
                        "objInfo" if body.contains("android.widget.EditText") => {
                            log.push("objInfo EditText".into());
                            ok(json!({ "bounds": { "left": 54, "top": 170, "right": 465, "bottom": 525 }, "text": "class Solution:", "className": "android.widget.EditText" }))
                        }
                        "objInfo" => {
                            log.push("objInfo miss".into());
                            miss
                        }
                        "getConfigurator" => {
                            log.push("getConfigurator".into());
                            ok(json!({ "actionAcknowledgmentTimeout": 500, "keyInjectionDelay": 0, "scrollAcknowledgmentTimeout": 200, "waitForIdleTimeout": 10000, "waitForSelectorTimeout": 10000 }))
                        }
                        "click" if body.contains("Unacked") => {
                            log.push("click Unacked".into());
                            ok(json!(false))
                        }
                        "click" | "swipePoints" | "pressKeyCode" | "setConfigurator" => {
                            log.push(format!("{method} {}", params()));
                            ok(json!(true))
                        }
                        _ => ok(Value::Null),
                    }
                };
                let _ = request.respond(tiny_http::Response::from_string(reply).with_header(tiny_http::Header::from_bytes("content-type", "application/json").unwrap()));
            }
        });
        Self { url: format!("http://127.0.0.1:{port}"), port, seen, selectors }
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.seen.lock().unwrap())
    }

    pub(crate) fn asked(&self) -> Vec<Value> {
        std::mem::take(&mut *self.selectors.lock().unwrap())
    }
}

#[test]
fn a_runbook_drives_the_screen_through_the_on_device_ui_server() {
    // Given a claimed emulator whose UI server is not running yet, and no remembered server port
    let s = sandbox("wf-flow-android-u2-");
    let tmp = s.repo.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let marker = tmp.join("u2-started");
    let server = FakeUiServer::start(Some(marker.clone()), false);
    let (port, mark, tmp_dir) = (server.port.to_string(), marker.display().to_string(), tmp.display().to_string());
    let base = [("STUB_U2_PORT", port.as_str()), ("STUB_U2_MARK", mark.as_str()), ("TMPDIR", tmp_dir.as_str())];
    let u2 = |args: &[&str], extra: &[(&str, &str)]| {
        s.forget("adb");
        let mut env = base.to_vec();
        env.extend_from_slice(extra);
        let mut full = vec!["android"];
        full.extend_from_slice(args);
        s.run(&full, &env)
    };

    // When the first lookup runs
    let first = u2(&["get-element", "--id", "TamilWriteCheck"], &[]);
    let setup = s.calls("adb");
    // Then the server port is forwarded, the jar pushed and started, the idle wait switched off, and the bounds come from the server
    assert_eq!(first.exit_code, 0, "{}", out(&first));
    assert_eq!(json_out(&first)["result"]["bounds"]["cx"], json!(200));
    let has = |needle: &str| setup.iter().any(|c| c.contains(needle));
    assert!(has("forward tcp:0 tcp:9008") && has("shell ls /data/local/tmp/u2.jar") && has("push") && has("app_process / com.wetest.uia2.Main"), "{setup:?}");
    assert!(!has("uiautomator dump"), "{setup:?}");
    let seen = server.take();
    let configured: Value = serde_json::from_str(seen.iter().find_map(|l| l.strip_prefix("setConfigurator ")).expect("a setConfigurator call")).unwrap();
    assert_eq!(configured, json!([{ "actionAcknowledgmentTimeout": 500, "keyInjectionDelay": 0, "scrollAcknowledgmentTimeout": 200, "waitForIdleTimeout": 0, "waitForSelectorTimeout": 10000 }]));
    assert_eq!(seen.last().map(String::as_str), Some("objInfoOfAllInstances TamilWriteCheck"));

    // When the next lookup runs in a new process
    let second = u2(&["get-element", "--id", "Nope"], &[]);
    // Then the remembered port is used without any adb call, and a miss is a null answer
    assert_eq!((second.exit_code, json_out(&second)["result"].clone()), (0, Value::Null), "{}", out(&second));
    assert!(s.calls("adb").is_empty(), "{:?}", s.calls("adb"));
    assert_eq!(server.take(), ["objInfoOfAllInstances miss"]);

    // Given a running server reached directly (WF_U2_URL)
    let direct = |args: &[&str]| u2(args, &[("WF_U2_URL", server.url.as_str())]);
    // When a wait, keys, a tap, a swipe and a tap by test id run
    let wait = direct(&["assert-visible", "--id", "TamilWriteCheck", "--timeout", "7000"]);
    let wait_seen = server.take();
    let wrapped_wait = direct(&["assert-visible", "--id", "EditorWrap", "--timeout", "7000"]);
    let wrapped_wait_seen = server.take();
    let left = direct(&["press-key", "Remote Dpad Left"]);
    let backspace = direct(&["press-key", "Backspace"]);
    let keys_seen = server.take();
    let tap = direct(&["tap-on", "--point", "120,340"]);
    let swipe = direct(&["swipe", "400,500", "40,500", "--duration", "400"]);
    let gestures = server.take();
    let by_id = direct(&["tap-on", "--id", "TamilWriteCheck"]);
    let by_id_seen = server.take();
    // Then the wait is answered by a lookup on the device, never a blocking server-side wait, and the look that met it is
    // the answer: no lookup after it, and a wrapper adds only the read of its field
    assert_eq!((wait.exit_code, wait_seen), (0, vec!["objInfoOfAllInstances TamilWriteCheck".to_string()]), "{}", out(&wait));
    assert_eq!(json_out(&wrapped_wait)["result"]["value"]["text"], json!("class Solution:"), "{}", out(&wrapped_wait));
    assert_eq!(wrapped_wait_seen, ["objInfoOfAllInstances hit", "objInfo EditText"]);
    // and keys are server keycodes, a tap a click and a swipe a two-point swipePoints, with no `input` process on the device
    assert_eq!((left.exit_code, backspace.exit_code, keys_seen), (0, 0, vec!["pressKeyCode [21]".to_string(), "pressKeyCode [67]".to_string()]));
    assert_eq!((tap.exit_code, swipe.exit_code, gestures.len()), (0, 0, 2), "{gestures:?}");
    assert!(gestures[0].starts_with("click [120,340") && gestures[1].starts_with("swipePoints [[400,500,40,500],"), "{gestures:?}");
    // and a tap by test id finds and presses its view in one call on the device, so a layout that moves cannot make it miss
    assert_eq!(by_id.exit_code, 0, "{}", out(&by_id));
    assert!(by_id_seen.last().is_some_and(|c| c.starts_with("click [{") && c.contains("TamilWriteCheck")), "{by_id_seen:?}");

    // When the practice pad's guide is drawn
    let screen = s.file("screen.png", guide_screen());
    let drawn = u2(&["draw-strokes-on", "--id", "TamilPracticeCanvasSurface", "--source", "guide"], &[("WF_U2_URL", server.url.as_str()), ("STUB_PNG", screen.as_str())]);
    let strokes: Vec<String> = server.take().into_iter().filter(|l| l.starts_with("swipePoints")).collect();
    // Then the stroke is one swipePoints call of at most 60 points at two steps each, with no segment over 30 px, resting on its end
    assert_eq!((drawn.exit_code, strokes.len()), (0, 1), "{}", out(&drawn));
    let params: Value = serde_json::from_str(strokes[0].trim_start_matches("swipePoints ")).unwrap();
    let flat: Vec<f64> = params[0].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let points: Vec<(f64, f64)> = flat.chunks(2).map(|c| (c[0], c[1])).collect();
    assert!((8..=60).contains(&points.len()) && params[1] == 2, "{}", strokes[0]);
    assert!(points.windows(2).all(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1) <= 30.01), "{points:?}");
    assert_eq!(points[points.len() - 1], points[points.len() - 2]);
    assert!(!s.calls("adb").iter().any(|c| c.contains("shell input") || c.contains("motionevent")), "{:?}", s.calls("adb"));

    // When the device commands look elements up by a text, by a test id, in a card that matches three times, and in the
    // editor's web view
    let cli = |args: &[&str]| -> Value {
        let r = direct(args);
        serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", out(&r)))
    };
    server.asked();
    let labelled = cli(&["get-element", "--text", "Check my writing"]);
    let text_selectors = server.asked();
    let by_test_id = cli(&["get-element", "--id", "TamilWriteCheck"]);
    let id_selectors = server.asked();
    server.take();
    let card = cli(&["tap-on", "--id", "Card"]);
    let card_seen = server.take();
    s.forget("adb");
    let unacked = cli(&["tap-on", "--id", "Unacked"]);
    let unacked_adb = s.calls("adb");
    let editor = cli(&["get-element", "--id", "CodeEditorInput"]);
    server.take();
    let wrapped = cli(&["get-element", "--id", "EditorWrap"]);
    let wrapped_seen = server.take();
    // Then the text is matched whole and case-insensitively by the server's Java engine, on the text before the label
    let java = json!("(?ismu)(?:Check[ \\n]my[ \\n]writing)|(?:Check[ \\n]my[ \\n]writing)");
    let shapes: Vec<(Value, Value, Value)> = text_selectors.iter().map(|s| (s["mask"].clone(), s["textMatches"].clone(), s["descriptionMatches"].clone())).collect();
    assert_eq!(shapes, [(json!(4), java.clone(), Value::Null), (json!(256), Value::Null, java)], "{text_selectors:?}");
    assert_eq!((labelled["ok"].clone(), labelled["result"]["text"].clone()), (json!(true), json!("Check my writing")), "{labelled}");
    // and a test id matches the whole resource id or the part after its last '/'
    assert_eq!(id_selectors, [json!({ "mask": 0x40_0000, "childOrSibling": [], "childOrSiblingSelector": [], "resourceIdMatches": "(?ismu)(?:.*/(?=[^/]*\\z))?(?:(?:TamilWriteCheck)|(?:TamilWriteCheck))" })]);
    assert_eq!(by_test_id["result"]["bounds"]["cx"], json!(200), "{by_test_id}");
    // and of the card's matches the deepest one on screen wins: its on-screen part, tapped on the device as that same match
    assert_eq!((card["result"]["bounds"].clone(), card["result"]["x"].clone(), card["result"]["y"].clone()), (json!({ "left": 100, "top": 200, "right": 300, "bottom": 230, "cx": 200, "cy": 215 }), json!(200.0), json!(215.0)), "{card}");
    assert!(card_seen.last().is_some_and(|c| c.starts_with("click [{") && c.contains("\"instance\":1") && c.contains("Card")), "{card_seen:?}");
    // and a press the server makes but sees no screen change for within its acknowledgment is still one press: no input tap
    // presses it again (a mic pressed twice started a second recording)
    assert_eq!(unacked["ok"], json!(true), "{unacked}");
    assert!(!unacked_adb.iter().any(|c| c.contains("input tap")), "{unacked_adb:?}");
    // and the editor's web view reads as the EditText inside it: the buffer (the old edit-text verb)
    assert_eq!(editor["result"]["text"], json!("def f():\n    pass"), "{editor}");
    // and on a device the testID sits on a FrameLayout around the web view: the first EditText drawn inside it is its text,
    // read with one objInfo call, not a scan of every EditText
    assert_eq!((wrapped["result"]["text"].clone(), wrapped_seen), (json!("class Solution:"), vec!["objInfoOfAllInstances hit".to_string(), "objInfo EditText".to_string()]), "{wrapped}");

    // Given a server whose automation connection died: it answers /ping but misses every selector until restarted
    let dead = FakeUiServer::start(None, true);
    // When a lookup asks for a view that is on screen
    let revived = u2(&["get-element", "--id", "TamilWriteCheck"], &[("WF_U2_URL", dead.url.as_str())]);
    // Then the miss is checked with deviceInfo, the server is killed for a restart, and the second ask finds the view
    assert_eq!((revived.exit_code, json_out(&revived)["result"]["bounds"]["cx"].clone()), (0, json!(200)), "{}", out(&revived));
    assert_eq!(dead.take(), ["objInfoOfAllInstances miss", "deviceInfo dead", "objInfoOfAllInstances TamilWriteCheck"]);
    assert!(s.calls("adb").iter().any(|c| c.ends_with("shell pkill -f com.wetest.uia2")), "{:?}", s.calls("adb"));
}

#[test]
fn the_cold_start_ends_at_once_on_expo_gos_error_screen_with_what_its_error_log_says() {
    // Given Expo Go showing its error screen with the error log's line beside it, as it did for an update it could not open
    let s = sandbox("wf-flow-android-expo-error-");
    s.file("e2e/warm-app.lua", std::fs::read(crate::support::repo_root().join("e2e/warm-app.lua")).unwrap());
    s.file("ui.xml", r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation="0">
<node index="0" text="Something went wrong." resource-id="" class="android.widget.TextView" bounds="[20,300][460,340]" clickable="false" />
<node index="1" text="Sorry about that. You can go back to Expo home or try to reload the project." resource-id="" class="android.widget.TextView" bounds="[20,350][460,400]" clickable="false" />
<node index="2" text="View error log" resource-id="" class="android.widget.TextView" bounds="[180,900][300,930]" clickable="true" />
<node index="3" text="Error: Failed to fetch the update manifest: 404" resource-id="" class="android.widget.TextView" bounds="[20,120][460,160]" clickable="false" />
</hierarchy>"#);
    let out_dir = s.repo.join("warm");
    let started = std::time::Instant::now();

    // When the cold start opens the update
    let logcat = "1792588000.100  4242  4300 E expo-updates: Failed to download asset\n1792588000.101  4242  4300 E expo-updates: Caused by: java.io.IOException: Unexpected response 403\n";
    let r = android(&s, &["test", "e2e/warm-app.lua", "--no-record", "--env", "base=exp://u.expo.dev/p/group/g/--", "--env", &format!("out={}", out_dir.display())], &[("STUB_LOGCAT", logcat)]);

    // Then it fails within seconds naming the link, the log's line and logcat's cause, taps View error log rather than pressing
    // back on the screen's "reload the project", and keeps both screenshots and the loader's logcat lines
    let text = out(&r);
    assert_eq!(r.exit_code, 1, "{text}");
    assert!(text.contains("Expo Go could not open exp://u.expo.dev/p/group/g/--/: Error: Failed to fetch the update manifest: 404 (logcat: java.io.IOException: Unexpected response 403)"), "{text}");
    assert_eq!(std::fs::read_to_string(out_dir.join("expo-go-logcat.txt")).unwrap(), logcat);
    assert!(started.elapsed() < std::time::Duration::from_secs(60), "{:?}", started.elapsed());
    let adb = s.calls("adb");
    assert!(adb.iter().any(|c| c.ends_with("shell input tap 240 915")) && !adb.iter().any(|c| c.contains("keyevent 4")), "{adb:?}");
    assert!(out_dir.join("expo-go-error.png").exists() && out_dir.join("expo-go-error-log.png").exists());
}

#[test]
fn the_cold_start_opens_the_link_it_is_told_and_an_update_is_told_it_bare() {
    // Given Expo Go showing the app's first frame (its debug button), and an update's base with its link without /--
    let s = sandbox("wf-flow-android-bare-open-");
    s.file("e2e/warm-app.lua", std::fs::read(crate::support::repo_root().join("e2e/warm-app.lua")).unwrap());
    s.file("ui.xml", r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation="0">
<node index="0" text="" resource-id="DebugFab" class="android.widget.Button" bounds="[400,900][460,960]" clickable="true" />
</hierarchy>"#);
    let (update, out_dir) = ("exp://u.expo.dev/p/group/g", s.repo.join("warm").display().to_string());
    let open = |extra: &[&str]| {
        s.forget("adb");
        let mut args = vec!["test", "e2e/warm-app.lua", "--no-record", "--env", "base=exp://u.expo.dev/p/group/g/--", "--env"];
        let out_env = format!("out={out_dir}");
        args.push(out_env.as_str());
        args.extend_from_slice(extra);
        let r = android(&s, &args, &[]);
        let opened: Vec<String> = s.calls("adb").into_iter().filter(|c| c.contains("android.intent.action.VIEW")).collect();
        (r, opened)
    };

    // When the cold start is told the update's bare link, as `wf test run` does, and when it is told only the base
    let (told, opened) = open(&["--env", &format!("open={update}")]);
    let (untold, opened_untold) = open(&[]);
    // Then the first opens the bare link (an update's manifest answers 404 under a /--/ path) and reaches the first frame, and the
    // second opens the base and a slash
    assert_eq!(told.exit_code, 0, "{}", out(&told));
    assert!(opened.len() == 1 && opened[0].contains(&format!("{update}?wfRun=")), "{opened:?}");
    assert!(std::path::Path::new(&out_dir).join("startup.json").exists());
    assert!(opened_untold.len() == 1 && opened_untold[0].contains(&format!("{update}/--/?wfRun=")), "{opened_untold:?} {}", out(&untold));
}

#[test]
fn an_android_test_case_run_without_a_base_stops_before_it_opens_any_link() {
    // Given the cold start and the stop probe as committed, which open the link a caller hands them, and Expo Go on the app's first frame
    let s = sandbox("wf-flow-android-no-base-");
    for script in ["warm-app", "stop-probe"] {
        s.file(&format!("e2e/{script}.lua"), std::fs::read(repo_root().join(format!("e2e/{script}.lua"))).unwrap());
    }
    s.file("ui.xml", r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation="0">
<node index="0" text="" resource-id="DebugFab" class="android.widget.Button" bounds="[400,900][460,960]" clickable="true" />
</hierarchy>"#);
    let out_env = format!("out={}", s.repo.join("warm").display());
    let run = |script: &str, extra: &[&str]| {
        s.forget("adb");
        let mut args = vec!["test", script, "--no-record", "--env", out_env.as_str()];
        args.extend_from_slice(extra);
        let r = android(&s, &args, &[]);
        let opened: Vec<String> = s.calls("adb").into_iter().filter(|c| c.contains("android.intent.action.VIEW")).collect();
        (r, opened)
    };

    // When each runs with no base, and the cold start once with one
    let (warm, warm_opened) = run("e2e/warm-app.lua", &[]);
    let (probe, probe_opened) = run("e2e/stop-probe.lua", &[]);
    let (control, control_opened) = run("e2e/warm-app.lua", &["--env", "base=exp://u.expo.dev/p/group/g/--"]);
    // Then both without fail at once saying the caller passes the update link, and opened nothing on the device; the same device
    // double does see the link the run with a base opens
    for (r, opened) in [(&warm, &warm_opened), (&probe, &probe_opened)] {
        assert_eq!(r.exit_code, 1, "{}", out(r));
        assert!(out(r).contains("pass --env base=<update link>; wf test run does"), "{}", out(r));
        assert!(opened.is_empty(), "{opened:?}");
    }
    assert_eq!(control.exit_code, 0, "{}", out(&control));
    assert_eq!(control_opened.len(), 1, "{control_opened:?}");
}

/// Logging stand-ins for the device shell's tools, the only ones the stub adb runs a device script with (STUB_DEVICE_BIN):
/// pidof finds screenrecord for STUB_SCREENRECORD_POLLS asks, then not; sleep, rm and pkill only log. Shell builtins only.
fn device_tools(s: &SlotSandbox) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = s.repo.join("device-bin");
    std::fs::create_dir_all(&dir).unwrap();
    let log = r#"echo "${0##*/} $*" >> "${0%/*}/device.log""#;
    let pidof = format!("{log}\nn=0; [ -e \"$0.n\" ] && read -r n < \"$0.n\"; n=$((n + 1)); echo \"$n\" > \"$0.n\"\n[ \"$n\" -le \"${{STUB_SCREENRECORD_POLLS:-0}}\" ] && echo 4242");
    for (tool, body) in [("pidof", pidof.as_str()), ("sleep", log), ("rm", log), ("pkill", log)] {
        let path = dir.join(tool);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

#[test]
fn the_recording_stops_once_screenrecord_has_written_its_file_and_exited_waiting_at_most_5_s() {
    // Given a recording whose screenrecord takes three polls to exit after SIGINT, one whose screenrecord never exits, and
    // one on an image without pidof
    let s = sandbox("wf-flow-android-record-stop-");
    let tools = device_tools(&s);
    let (video, bin) = (s.repo.join("run.mp4").display().to_string(), tools.display().to_string());
    let stop = |polls: &str| {
        s.forget("adb");
        let _ = std::fs::remove_file(tools.join("device.log"));
        let _ = std::fs::remove_file(tools.join("pidof.n"));
        let r = android(&s, &["stop-recording"], &[("STUB_RECORDING", &video), ("STUB_DEVICE_BIN", &bin), ("STUB_SCREENRECORD_POLLS", polls)]);
        let device: Vec<String> = std::fs::read_to_string(tools.join("device.log")).unwrap_or_default().lines().map(str::to_string).collect();
        (r, device, s.calls("adb"))
    };

    // When each is stopped
    let (quick, quick_device, quick_adb) = stop("3");
    let (stuck, stuck_device, stuck_adb) = stop("1000");
    std::fs::remove_file(tools.join("pidof")).unwrap();
    let (blind, blind_device, blind_adb) = stop("1000");

    // Then the loop's flag goes, screenrecord gets SIGINT, and one shell call asks for it every 0.1 s until it is gone; only
    // then is the file pulled
    assert_eq!((quick.exit_code, json_out(&quick)["result"].clone()), (0, json!({ "files": [video] })), "{}", out(&quick));
    let mut want = vec!["rm -f /sdcard/wf-record.on", "pkill -INT screenrecord"];
    want.extend(["pidof screenrecord", "sleep 0.1"].repeat(3));
    want.push("pidof screenrecord");
    assert_eq!(quick_device, want);
    assert_eq!(quick_adb[1..], [STOP_RECORDER, "-s emulator-5554 shell ls /sdcard/wf-record-*.mp4", &format!("-s emulator-5554 pull /sdcard/wf-record-1.mp4 {video}")]);
    // and one that never exits is given up on after 50 pauses of 0.1 s, its file still pulled
    assert_eq!((stuck.exit_code, stuck_device.iter().filter(|c| *c == "sleep 0.1").count()), (0, 50), "{}", out(&stuck));
    assert!(stuck_adb.last().is_some_and(|c| c.ends_with(&format!("pull /sdcard/wf-record-1.mp4 {video}"))), "{stuck_adb:?}");
    // and with no pidof to ask, the stop waits the fixed 2 s it waited before, then pulls
    assert_eq!((blind.exit_code, blind_device), (0, vec!["rm -f /sdcard/wf-record.on".to_string(), "pkill -INT screenrecord".into(), "sleep 2".into()]), "{}", out(&blind));
    assert!(blind_adb.last().is_some_and(|c| c.ends_with(&format!("pull /sdcard/wf-record-1.mp4 {video}"))), "{blind_adb:?}");
}

#[test]
fn a_key_reads_the_keyboard_right_before_it_so_one_the_app_lowered_by_itself_is_refused() {
    // Given a script that types twice and presses a key, on a device whose app lowers the keyboard once text is typed (a
    // Keyboard.dismiss after the last digit), with no command of the script in between
    let s = sandbox("wf-flow-android-keyboard-read-");
    s.file(
        "keys.lua",
        r#"device:input_text('ab')
assert_true(device:input_text('c', {optional=true}) == nil, "text with the keyboard down is refused")
assert_true(device:press_key('Backspace', {optional=true}) == nil, "a key with the keyboard down is refused")
"#,
    );

    // When it runs
    let r = android(&s, &["test", "keys.lua", "--no-record"], &[("STUB_TYPE_LOWERS", "1")]);

    // Then each key and text reads the keyboard just before it is sent: the first text goes, and the text and key after the
    // app lowered it are refused and never sent
    assert_eq!(r.exit_code, 0, "{}", out(&r));
    let sent: Vec<String> = s
        .calls("adb")
        .iter()
        .filter_map(|c| c.strip_prefix("-s emulator-5554 shell "))
        .filter(|c| c.starts_with("dumpsys input_method") || c.starts_with("input "))
        .map(|c| c.replace("dumpsys input_method", "READ"))
        .collect();
    assert_eq!(sent, ["READ", "input text 'ab'", "READ", "READ"]);
}

#[test]
fn stack_maps_a_logged_hermes_stack_through_the_groups_source_map_from_the_bucket() {
    // Given a bucket holding an update group's summary and its android source map (one mapping: bundle 1:1 is boom, features/a.ts:1:1)
    let bucket = crate::support::MockServer::start(None);
    let map = json!({ "version": 3, "sources": ["features/a.ts"], "names": ["boom"], "mappings": "AAAAA" });
    bucket.routes(&[
        ("GET /verification/eas-updates/g-1/summary.json", crate::support::MockReply::json(200, json!({ "maps": { "files": ["_expo/static/js/ios/e.hbc.map", "_expo/static/js/android/e.hbc.map"] } }))),
        ("GET /verification/eas-updates/g-1/_expo/static/js/android/e.hbc.map", crate::support::MockReply::json(200, map)),
    ]);
    let stack = "Error: x\n    at boom (address at index.android.bundle:1:1)\n    at native (native)";
    let opts = RunOpts::default().clean_env().env("VERIFICATION_ARTIFACTS_READ_URL", &format!("http://127.0.0.1:{}", bucket.port)).input(stack);

    // When the stack is mapped for that group, and for a group the bucket does not have
    let mapped = last_json(&crate::support::run_wf(&["android", "stack", "g-1"], &opts));
    let missing = crate::support::run_wf(&["android", "stack", "g-2"], &opts);

    // Then the located frame names its source line, the others stay as logged, and the unknown group fails with the next step
    assert_eq!(mapped["result"]["frames"][1]["source"], "features/a.ts");
    assert_eq!(mapped["result"]["text"], "Error: x\nboom (features/a.ts:1:1)\nat native (native)");
    assert_eq!(mapped["result"]["frames"][2]["mapped"], false);
    assert_eq!(missing.exit_code, 1);
    assert!(last_json(&missing)["error"].as_str().unwrap().contains("no summary for update group g-2"));
}
