//! `wf android test` / `wf browser test <script.lua>` (one script API over two device drivers) and `wf flow run`'s goldens: what a
//! script sees, what reaches the device, and the report, recording, goldens and timing a run leaves behind. `adb` is an
//! argv-recording stub whose screen, keyboard, logcat and recordings are scripted through env vars; the browser daemon is a mock
//! that records each `/call` and answers it from rules.
use crate::support::{MockDaemon, RunResult, SlotSandbox};
use wf_vmops::testing::{answers, FakeVm, VM_SEED};
use serde_json::{json, Value};
use std::path::Path;

const STUB_ADB: &str = r#"echo "$*" >> "$0.log"
# the first $STUB_FLIP looks of a kind see the state before its change
before() { n=$(cat "$0.$1" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$0.$1"; [ "$n" -le "${STUB_FLIP:-0}" ]; }
case "$*" in
  *'id -u'*) echo "${STUB_UID:-0}";;
  *"wm density"*) printf 'Physical density: 320\nOverride density: 213\n';;
  *uiautomator*) cat "$STUB_UI_XML";;
  *exec-out*cat*) cat "$STUB_UI_XML";;
  *exec-out*screencap*) if [ -n "$STUB_PNG_BEFORE" ] && before px; then cat "$STUB_PNG_BEFORE"; elif [ -n "$STUB_PNG" ]; then cat "$STUB_PNG"; else printf 'PNGDATA'; fi;;
  *exec-out*) printf 'PNGDATA';;
  *dumpsys*input_method*) if before kb; then echo "  mInputShown=true"; else echo "  mInputShown=${STUB_IME:-true}"; fi;;
  *inj.marker*) before rec || echo /data/user/0/host.exp.exponent/cache/a.m4a.swapped;;
  *logcat*) printf '%b' "${STUB_LOGCAT:-}";;
  *'shell ls'*) printf '%b' "${STUB_SEGMENTS:-/sdcard/wf-record-1.mp4\n}";;
  *devices*) echo 'List of devices attached';;
  *'ip route'*) echo 'default via 127.0.0.1 dev eth0 table eth0 proto static';;
  *wf-stub-rules-ok*) [ -n "${STUB_RULES_FAIL:-}" ] && { echo "iptables: can't initialize table"; exit 3; }; echo wf-stub-rules-ok;;
  *'iptables -t nat -L OUTPUT'*) echo '    7   420 DNAT tcp -- * * 0.0.0.0/0 0.0.0.0/0 tcp dpt:8789 /* wf-stub */ to:127.0.0.1:1';;
  *wf-probe*) curl -s -X POST -H 'Host: 100.64.0.9:8789' "http://127.0.0.1:${*##* }/api/leetcode/run";;
  *wf-forward*) port=$(echo "$*" | awk '{print $(NF-1)}'); curl -s -H "Host: 127.0.0.1:${*##* }" "http://127.0.0.1:$port/api/problems";;
  *'shell sleep 2'*) sleep "${STUB_STOP_HANG:-0}";;
esac
exit "${STUB_ADB_EXIT:-0}""#;

const UI_XML: &str = r#"<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation="0">
<node index="0" text="" resource-id="TamilWriteCheck" class="android.view.ViewGroup" bounds="[100,200][300,260]" clickable="true" />
<node index="2" text="" resource-id="CodeEditorInput" class="android.webkit.WebView" bounds="[0,300][540,500]"><node index="0" text="line one&#10;    line two" resource-id="" class="android.widget.EditText" bounds="[0,300][540,500]" clickable="true" /></node>
<node index="1" text="LOOKS RIGHT · 92" resource-id="TamilWriteVerdictBadge" class="android.widget.TextView" bounds="[10,10][110,50]" clickable="false" />
</hierarchy>"#;

/// A 4x4 white screen with one blue pixel at (2,1).
const FOUR_BY_FOUR: &str = "iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAFElEQVR4nGP4jwQYoFQVEgdFBgIAq5UuTTkQ8tsAAAAASUVORK5CYII=";
/// The same screen all white.
const ALL_WHITE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAQAAAAECAIAAAAmkwkpAAAAD0lEQVR4nGP4jwQYiOMAANuQL9FuRbCyAAAAAElFTkSuQmCC";

fn sandbox(prefix: &str) -> SlotSandbox {
    let s = SlotSandbox::new(prefix);
    s.stub("adb", STUB_ADB);
    s.file("ui.xml", UI_XML);
    s.file("shot.png", "PNG");
    s
}

/// A page value as the daemon's `outerEval` and `shellEval` answer it: the value's JSON text.
fn evaluated(v: Value) -> Value {
    json!({ "result": v.to_string() })
}

/// The mock browser daemon, registered for the sandbox's slot with the default answers.
fn daemon(s: &SlotSandbox) -> MockDaemon {
    let d = MockDaemon::start();
    s.seed(&json!({ "main/mla-tamagui/default": { "createdAt": "2026-01-01T00:00:00Z", "browserMcp": { "url": d.url, "startedAt": "2026-01-01T00:00:00Z" } } }));
    defaults(&d, s);
    d
}

/// The page the mock daemon shows unless a test's earlier rules say otherwise: `Go` at 10,20-110,60 holding still, the keyboard up, one
/// warning, a recorder that took its clip, one stub hit and one logpoint hit, and the screenshot at shot.png.
fn defaults(d: &MockDaemon, s: &SlotSandbox) {
    let shot = s.repo.join("shot.png").display().to_string();
    let go = json!({ "bounds": { "left": 10, "top": 20, "right": 110, "bottom": 60 }, "text": "Go", "frame": "app", "path": ":root > body:nth-child(2) > div:nth-child(1)" });
    d.rule("outerEval", "return r.hit", vec![evaluated(go.clone())]);
    d.rule("outerEval", "return r.count", vec![evaluated(json!(1))]);
    d.rule("outerEval", ").tree", vec![evaluated(json!([]))]);
    d.rule("outerEval", "dpr: devicePixelRatio", vec![evaluated(json!({ "width": 393, "height": 852, "dpr": 3 }))]);
    d.rule("outerEval", "__mcpFakeMicState", vec![evaluated(json!({ "queued": [], "consumed": ["clip.wav"] }))]);
    d.rule("outerEval", "testids", vec![evaluated(json!({ "url": "http://x/", "title": "x", "testids": ["Go"], "text": "Go" }))]);
    d.rule("shellEval", "getStatus", vec![evaluated(json!({ "visible": true, "bounds": null }))]);
    d.rule("waitObserve", "setTimeout(look,100)", vec![json!({ "result": json!({ "hit": go }).to_string() })]);
    d.rule("waitObserve", "", vec![json!({ "result": "{\"seen\":true,\"ms\":3}" })]);
    d.rule("screenshot", "", vec![json!({ "path": shot })]);
    d.rule("consoleLogs", "", vec![json!({ "count": 1, "lines": ["warn: boom"] })]);
    d.rule("networkStubHits", "", vec![json!({ "hits": [{ "method": "POST", "url": "http://x/api/plan", "status": 201, "pattern": "*/api/plan", "at": 1 }] })]);
    d.rule("debugSet", "", vec![json!({ "id": 1, "resolved": true })]);
    d.rule("debugHits", "", vec![json!({ "count": 1, "hits": [{ "file": "src/a.ts", "line": 3, "captures": { "x": 1 } }] })]);
    d.rule("recordStop", "", vec![json!({ "video": { "path": "run/run.mp4", "format": "mp4", "bytes": 5 } })]);
}

/// Writes `lua` to t.lua and runs it on `platform` (`android` or `browser`), unrecorded unless `extra` names a record dir.
fn script(s: &SlotSandbox, platform: &str, lua: &str, extra: &[&str], env: &[(&str, &str)]) -> RunResult {
    let path = s.file("t.lua", lua);
    s.forget("adb");
    let xml = s.repo.join("ui.xml").display().to_string();
    let mut e: Vec<(&str, &str)> = vec![("STUB_UI_XML", xml.as_str()), ("WF_ANDROID_UIA", "dump"), ("WF_BROWSER_NO_START", "1")];
    e.extend_from_slice(env);
    let mut args = vec![platform, "test", path.as_str()];
    if !extra.contains(&"--test-output-dir") {
        args.push("--no-record");
    }
    args.extend_from_slice(extra);
    s.run(&args, &e)
}

fn out(r: &RunResult) -> String {
    format!("{}{}", r.stdout, r.stderr)
}

fn logged(r: &RunResult) -> Value {
    let line = r.stdout.lines().find(|l| l.starts_with('{')).unwrap_or_else(|| panic!("no JSON log line: {}", out(r)));
    serde_json::from_str(line).unwrap()
}

fn report(dir: &Path) -> Value {
    assert!(dir.join("report.html").is_file(), "no report.html in {}", dir.display());
    serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap()
}

fn jsonl(path: &str) -> Vec<Value> {
    std::fs::read_to_string(path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn the_wait_verbs_return_on_the_event() {
    let s = sandbox("wf-flow-harness-waits-");
    let png = |name: &str, b64: &str| s.file(name, base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).unwrap());
    let (white, blue) = (png("white.png", ALL_WHITE), png("blue.png", FOUR_BY_FOUR));
    let waits = "local blue = function(c) return c.b > 200 and c.r < 80 end\n\
        local function met(f, ms, label) return device:extended_wait_until{condition=f, timeout=ms, interval=50, optional=true, label=label} ~= nil end\n\
        log(json.encode({ pixel = met(function() return blue(device:get_pixel('62.5%,37.5%')) end, 5000), keyboard = met(function() return not device:get_device_state('keyboard').visible end, 5000),\n\
          recording = met(function() return device:get_device_state('microphone').consumed end, 5000), never = met(function() return device:get_device_state('keyboard').visible end, 300, 'keyboard back') }))";
    for platform in ["android", "browser"] {
        // Given a screen that turns blue at (2,1), a keyboard that goes down and a recorder that takes the clip, each on the
        // fourth look
        let log = s.repo.join(format!("waits-{platform}.jsonl")).display().to_string();
        let env = [("STUB_FLIP", "3"), ("STUB_PNG_BEFORE", white.as_str()), ("STUB_PNG", blue.as_str()), ("STUB_IME", "false"), ("WF_WAIT_LOG", log.as_str())];
        let browser = (platform == "browser").then(|| {
            let d = daemon(&s);
            d.clear_rules();
            let flip = |before: Value, after: Value| vec![before.clone(), before.clone(), before, after];
            d.rule("screenshot", "", flip(json!({ "path": white }), json!({ "path": blue })));
            d.rule("shellEval", "getStatus", flip(evaluated(json!({ "visible": true, "bounds": null })), evaluated(json!({ "visible": false, "bounds": null }))));
            d.rule("outerEval", "__mcpFakeMicState", flip(evaluated(json!({ "queued": ["c.wav"], "consumed": [] })), evaluated(json!({ "queued": [], "consumed": ["c.wav"] }))));
            defaults(&d, &s);
            d
        });
        // When a script waits for each
        let r = script(&s, platform, waits, &[], &env);
        // Then each wait returned true on the look that saw the change, and the keyboard that never comes back is false at the end
        assert_eq!(r.exit_code, 0, "{platform}: {}", out(&r));
        let line = r.stdout.lines().find(|l| l.contains("\"pixel\"")).unwrap_or_else(|| panic!("{platform}: {}", out(&r)));
        let v: Value = serde_json::from_str(line.split_once('{').map(|(_, rest)| format!("{{{rest}")).unwrap().trim()).unwrap();
        assert_eq!((v["pixel"].clone(), v["keyboard"].clone(), v["recording"].clone(), v["never"].clone()), (json!(true), json!(true), json!(true), json!(false)), "{platform}: {v}");
        let looks = match &browser {
            Some(d) => vec![("px", d.count("screenshot", "")), ("rec", d.count("outerEval", "__mcpFakeMicState"))],
            None => {
                let calls = s.calls("adb");
                ["screencap", "inj.marker"].iter().zip(["px", "rec"]).map(|(needle, kind)| (kind, calls.iter().filter(|c| c.contains(needle)).count())).collect()
            }
        };
        for (kind, n) in looks {
            assert_eq!(n, 4, "{platform} {kind}");
        }
        let logged: Vec<Value> = std::fs::read_to_string(&log).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let never = logged.iter().find(|w| w["call"] == "extended_wait_until:condition:optional" && w["label"] == "keyboard back").unwrap_or_else(|| panic!("{logged:?}"));
        assert!(never["ok"] == false && never["ms"].as_u64() >= Some(300), "{never}");
        for kind in ["px", "kb", "rec"] {
            let _ = std::fs::remove_file(s.bin_dir.join(format!("adb.{kind}")));
        }
    }
}

#[test]
fn an_android_run_is_recorded_timed_and_reported_pass_or_fail() {
    let s = sandbox("wf-flow-harness-report-");
    let android = |lua: &str, extra: &[&str], env: &[(&str, &str)]| script(&s, "android", lua, extra, env);

    // When a script runs with the default recording
    let recorded = android("log('hi')", &["--test-output-dir", "rec"], &[]);
    let calls = s.calls("adb");
    // Then recording started before the script and was stopped, pulled and reported after it, and the pass page keeps its shape
    assert_eq!(recorded.exit_code, 0, "{}", out(&recorded));
    let dir = "rec";
    assert!(recorded.stdout.contains(&format!("recording: {dir}/run.mp4")), "{}", recorded.stdout);
    assert!(calls.iter().position(|c| c.contains("screenrecord")).unwrap() < calls.iter().position(|c| c.contains(" pull ")).unwrap(), "{calls:?}");
    let pass = report(&s.repo.join("rec"));
    assert_eq!((pass["platform"].as_str(), pass["verdict"].as_str(), pass["appStart"].as_str()), (Some("android"), Some("pass"), Some("reused")));
    let page = std::fs::read_to_string(s.repo.join("rec/report.html")).unwrap();
    assert!(page.contains("class=\"verdict pass\"") && page.contains("JS warnings") && page.contains("Debugger hits") && page.matches("class=\"none\"").count() == 3 && page.contains("App logs") && page.contains("were not read"), "{page}");
    // and the Android debugger's status is gone from the report and its page: no key, no section
    assert!(pass.get("debugStatus").is_none() && !page.contains("<h2>Debugger</h2>") && !recorded.stdout.contains("debug status"), "{pass} {page}");

    // When a recorded script fails after a tap, with markup in its message
    let failed = android("device:tap_on({id='TamilWriteCheck'})\nassert_true(1 == 2, 'one is not two <b>&</b>')", &["--test-output-dir", "fail"], &[]);
    // Then it fails, the recording was still pulled, and the report carries the verdict, message, traceback and the tap, escaped
    assert_eq!(failed.exit_code, 1);
    assert!(failed.stdout.contains("report: fail/report.html"), "{}", failed.stdout);
    assert!(s.calls("adb").iter().any(|c| c.contains(" pull ")), "{:?}", s.calls("adb"));
    let v = report(&s.repo.join("fail"));
    assert_eq!((v["schema"].as_u64(), v["verdict"].as_str(), v["ok"].as_bool(), v["exitCode"].as_i64()), (Some(1), Some("fail"), Some(false), Some(1)), "{v}");
    assert_eq!(v["message"], json!("one is not two <b>&</b>"));
    let tb = v["traceback"].as_str().unwrap_or_default();
    assert!(tb.starts_with("stack traceback:") && tb.contains("t.lua:2"), "{v}");
    assert!(v["steps"].as_array().unwrap().iter().any(|st| st["kind"] == "tap"), "{v}");
    let page = std::fs::read_to_string(s.repo.join("fail/report.html")).unwrap();
    assert!(page.contains("one is not two &lt;b&gt;&amp;&lt;/b&gt;") && !page.contains("<b>&</b>") && page.contains("t.lua:2") && page.contains("class=\"verdict fail\""), "{page}");

    // When the script does not parse
    let syntax = android("local x = ", &["--test-output-dir", "syntax"], &[]);
    // Then the report says error with the parser's message
    assert_eq!(syntax.exit_code, 1);
    let v = report(&s.repo.join("syntax"));
    assert!(v["verdict"] == "error" && v["message"].as_str().unwrap_or_default().contains("syntax error"), "{v}");

    // When it runs cold, and when it runs on the running app with an open-route step
    let cold = android("device:tap_on({id='TamilWriteCheck'})", &["--cold-start", "--test-output-dir", "cold"], &[]);
    let cold_calls = s.calls("adb");
    let reused = android("run_flow('open route', function() device:tap_on({id='TamilWriteCheck'}) end)", &["--test-output-dir", "reused"], &[]);
    // Then Expo Go is force-stopped before the first call only when cold, and the report and its timing say how the app started
    assert_eq!((cold.exit_code, reused.exit_code), (0, 0), "{}{}", out(&cold), out(&reused));
    let stop = cold_calls.iter().position(|c| c.ends_with("am force-stop host.exp.exponent")).unwrap_or_else(|| panic!("{cold_calls:?}"));
    assert!(stop < cold_calls.iter().position(|c| c.ends_with("shell input tap 200 230")).unwrap(), "{cold_calls:?}");
    assert_eq!(report(&s.repo.join("cold"))["appStart"], "cold");
    assert!(!s.calls("adb").iter().any(|c| c.contains("force-stop")), "{:?}", s.calls("adb"));
    let v = report(&s.repo.join("reused"));
    assert_eq!((v["appStart"].as_str(), v["timing"]["appStart"].as_str()), (Some("reused"), Some("reused")), "{v}");
    assert!(v["timing"]["startupS"].is_number(), "{}", v["timing"]);

    // When a step finds an absent node and sleeps, with a timing file
    let timing = s.repo.join("timing.jsonl").display().to_string();
    let timed = android("run_flow('look', function()\n  device:get_element({id='Nope'})\n  sleep_ms(50)\nend)\ndevice:tap_on({id='TamilWriteCheck'})\ndevice:take_screenshot('/tmp/wf-flow-timing-shot.png')", &["--test-output-dir", "timed"], &[("WF_RUN_TIMING", timing.as_str())]);
    // Then every driver call is a span with its verb and milliseconds, the find is a miss at line 2 inside the step, the sleep sits
    // in the step and the find's adb work sits under it
    assert_eq!(timed.exit_code, 0, "{}", out(&timed));
    let spans = jsonl(&timing);
    let one = |kind: &str, name: &str| spans.iter().find(|v| v["kind"] == kind && v["name"] == name).unwrap_or_else(|| panic!("no {kind} {name} in {spans:?}")).clone();
    let (step, find, sleep, tap) = (one("step", "look"), one("call", "get_element"), one("sleep", "sleep"), one("call", "tap_on"));
    one("call", "take_screenshot");
    assert!(step["line"].as_str().unwrap().ends_with("t.lua:1"), "{step}");
    assert_eq!((find["parent"].clone(), find["outcome"].as_str(), find["line"].as_str().map(|l| l.ends_with("t.lua:2"))), (step["id"].clone(), Some("miss"), Some(true)), "{find}");
    assert!(sleep["parent"] == step["id"] && sleep["ms"].as_u64().unwrap() >= 50, "{sleep}");
    assert!(tap.get("parent").is_none_or(|p| p != &step["id"]), "{tap}");
    assert!(spans.iter().any(|v| v["kind"] == "inner" && v["parent"] == find["id"]), "no inner span under find: {spans:?}");
    assert!(spans.iter().all(|v| v["t0"].is_u64() && v["ms"].is_u64() && v["id"].is_string()), "{spans:?}");
    // and the report sums the time by verb, by step and asleep without counting the inner parts twice
    let t = &report(&s.repo.join("timed"))["timing"];
    let find_row = t["byVerb"].as_array().unwrap().iter().find(|v| v["verb"] == "get_element").unwrap_or_else(|| panic!("{t}")).clone();
    assert_eq!((find_row["calls"].clone(), find_row["misses"].clone()), (json!(1), json!(1)), "{t}");
    assert!(t["byStep"][0]["step"] == "look" && t["sleepS"].as_f64().unwrap() >= 0.05 && t["calls"].as_u64().unwrap() >= 3, "{t}");

    // When a run samples the VM every second from vm-ops-api, and when vm-ops-api cannot reach the Docker Engine
    let vm = FakeVm::start();
    let stats = answers::reply(200, &answers::stats(11927, 4100, [2.5, 1.0, 0.5], &[("metro-runner", 2292, 0.1), ("agent-shell-dind", 1310, 88.5)]));
    vm.replies_then_last(std::slice::from_ref(&stats));
    let (socks, key) = (vm.tailnet.address(), vm.private_one_line.clone());
    let vm_env = |secs: &'static str| vec![("WF_VM_SAMPLE_SECS", secs), ("WF_VM_CI_PORT", ""), ("WF_TAILNET_SOCKS", socks.as_str()), ("VM_OPS_API_AUTH_TOKEN", VM_SEED), ("VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", key.as_str())];
    let sampled = android("sleep_ms(1500)", &["--test-output-dir", "sampled"], &vm_env("1"));
    vm.replies_then_last(&[answers::reply(502, &answers::request_failed("connect ENOENT /var/run/docker.sock"))]);
    let unsampled = android("log('x')", &["--test-output-dir", "unsampled"], &vm_env("1"));
    // Then the report carries a sample at the start, each second and at the end, with the host's load and memory and the
    // device container's, and a failed sample is recorded without failing the run
    assert_eq!((sampled.exit_code, unsampled.exit_code), (0, 0), "{}{}", out(&sampled), out(&unsampled));
    let v = report(&s.repo.join("sampled"))["vm"].clone();
    let samples = v["samples"].as_array().unwrap_or_else(|| panic!("{v}"));
    assert!(samples.len() >= 3 && samples.iter().all(|x| x["load1"] == 2.5 && x["memAvailableMb"] == 4100 && x["device"]["memMb"] == 1310), "{v}");
    assert_eq!((v["everyS"].clone(), v["load1Max"].clone(), v["memAvailableMinMb"].clone(), v["deviceCpuMaxPct"].clone(), v["errors"].clone()), (json!(1), json!(2.5), json!(4100.0), json!(88.5), json!(0)), "{v}");
    assert!(vm.requests().iter().all(|r| r.url.ends_with("/stats")), "{:?}", vm.requests());
    let page = std::fs::read_to_string(s.repo.join("sampled/report.html")).unwrap();
    assert!(page.contains("<h2>VM") && page.contains("88.5"), "{page}");
    let failed = report(&s.repo.join("unsampled"))["vm"].clone();
    assert!(failed["errors"].as_u64() >= Some(1) && failed["samples"][0]["error"].as_str().is_some_and(|e| e.contains("502")), "{failed}");
    assert!(report(&s.repo.join("timed"))["vm"].is_null());
    // When vm-ops-api takes 1.5 s to answer and the script shells out while the first sample waits (Lua's os.execute is libc
    // system(), which holds each child's SIGCHLD back from the main thread), Then no sample was cut short (A6, 20 Oct)
    vm.replies_then_last(&[stats]);
    vm.server.delay(std::time::Duration::from_millis(1500));
    let busy = android("for _ = 1, 5 do os.execute('sleep 0.2') end", &["--test-output-dir", "busy"], &vm_env("20"));
    let v = report(&s.repo.join("busy"))["vm"].clone();
    assert_eq!((busy.exit_code, v["errors"].clone(), v["samples"].as_array().map(Vec::len)), (0, json!(0), Some(2)), "{v}");

    // When the harness verb names its platform and --platform contradicts it
    let refused = s.run(&["browser", "test", &s.repo.join("t.lua").display().to_string(), "--platform", "android"], &[]);
    // Then it is refused
    assert_eq!(refused.exit_code, 2);
    assert!(refused.stdout.contains("--platform is gone: the noun is the platform"), "{}", refused.stdout);
}

#[test]
fn a_run_told_to_stop_ends_at_once_and_still_leaves_its_report() {
    let s = sandbox("wf-flow-harness-stop-");

    // When SIGTERM reaches a run mid-sleep (as the queue's timeout sends it)
    let started = std::time::Instant::now();
    let slept = script(&s, "android", "os.execute('kill -TERM $PPID'); sleep_ms(30000); log('never')", &[], &[]);
    // Then it ends at once with 124, says why, and its report is written like after any failure
    assert_eq!(slept.exit_code, 124, "{}", out(&slept));
    assert!(started.elapsed().as_secs() < 10, "the sleep ended at the stop, not after 30 s");
    assert!(slept.stderr.contains("stopped: SIGTERM received"), "{}", slept.stderr);
    let dir = slept.stdout.lines().find_map(|l| l.strip_prefix("report: ")).map(|p| p.trim_end_matches("/report.html").to_string()).expect("a report line");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(s.repo.join(format!("{dir}/report.json"))).unwrap()).unwrap();
    assert!(v["verdict"] == "error" && v["message"].as_str().unwrap_or("").contains("stopped: SIGTERM received"), "{v}");

    // When a script retries device calls inside pcall as the stop arrives
    let started = std::time::Instant::now();
    let looped = script(&s, "android", "os.execute('kill -TERM $PPID'); for _ = 1, 200 do pcall(function() device:tap_on({point='1,1'}) end); pcall(sleep_ms, 500) end; log('never')", &[], &[]);
    // Then the stop is not swallowed: the loop ends at once
    assert_eq!(looped.exit_code, 124, "{}", out(&looped));
    assert!(started.elapsed().as_secs() < 10 && !looped.stdout.contains("never"), "the pcall loop ran on after the stop");
    assert!(s.calls("adb").iter().filter(|c| c.contains("input tap")).count() <= 1, "{:?}", s.calls("adb"));

    // When a recorded run is stopped and its recording takes 60 s to stop, so the caller kills it after its 15 s grace
    let killed = script(&s, "android", "os.execute('kill -TERM $PPID'); sleep_ms(30000)", &["--test-output-dir", "rec"], &[("STUB_STOP_HANG", "60")]);
    // Then the report written before the recording stop is already there, with the stop as its message
    assert!(killed.stdout.contains("report (before the recording stops):"), "{}", out(&killed));
    let v = report(&s.repo.join("rec"));
    assert!(v["verdict"] == "error" && v["message"].as_str().unwrap_or("").contains("stopped: SIGTERM received"), "{v}");
}

#[test]
fn a_browser_script_drives_the_daemon_waits_by_observer_and_reports() {
    let s = sandbox("wf-flow-harness-browser-");
    let d = daemon(&s);
    let browser = |lua: &str, extra: &[&str], env: &[(&str, &str)]| {
        d.clear();
        script(&s, "browser", lua, extra, env)
    };
    let of = |method: &str| d.calls().into_iter().filter(|(m, _)| m == method).map(|(_, a)| a).collect::<Vec<Value>>();

    // the daemon writes a session's video where open asked for it
    for dir in ["run", "shots", "dbg"] {
        s.file(&format!("{dir}/run.mp4"), "video");
    }

    // When a script stubs the planner before it opens the app, then opens, taps, types and presses
    let run = browser("device:add_network_stub('post', '**/api/plan', { status = 201, json_body = { edits = { 1 } }, delay = 50 }); device:open_link('http://x/y'); log('hits ' .. #device:get_network_stub_hits()); device:tap_on({id='Go'}); device:input_text('abc'); device:press_key('Backspace'); log(device.platform)", &["--test-output-dir", "run"], &[]);
    let c = d.calls();
    // Then each call is one daemon method in process: the session opened as the test's own and recording, the stub armed
    // before the first page, the tap a point inside the element the page's finder resolved, and the recording stopped
    assert_eq!(run.exit_code, 0, "{}", out(&run));
    let at = |method: &str| c.iter().position(|(m, _)| m == method).unwrap_or_else(|| panic!("no {method} in {c:?}"));
    let stub = &c[at("networkStub")].1;
    assert_eq!((stub["pattern"].clone(), stub["method"].clone(), stub["status"].clone(), stub["body"].clone(), stub["contentType"].clone(), stub["delayMs"].clone()), (json!("*/api/plan"), json!("POST"), json!(201), json!("{\"edits\":[1]}"), json!("application/json"), json!(50)), "{stub}");
    assert!(at("open") < at("networkStub") && at("networkStub") < at("navigate"), "{c:?}");
    let open = &c[at("open")].1;
    let session = open["name"].as_str().unwrap().to_string();
    assert!(session.starts_with("run-") && open["devicePreset"] == "iphone-15-pro" && open["owner"].is_u64() && open["video"] == true && open["videoPath"].as_str().unwrap().ends_with("run/run.mp4"), "{open}");
    assert!(c.iter().all(|(_, a)| a["name"] == session.as_str()), "every call names the test's session: {c:?}");
    assert_eq!(c[at("navigate")].1["url"], "http://x/y");
    let tap = &c[at("tap")].1;
    let (x, y) = (tap["x"].as_f64().unwrap(), tap["y"].as_f64().unwrap());
    assert!((30.0..=90.0).contains(&x) && (28.0..=52.0).contains(&y) && tap["ref"].is_null() && tap["selector"] == "{id='Go'}", "{tap}");
    let typed = &c[at("type")].1;
    assert_eq!((typed["ref"].clone(), typed["text"].clone(), typed["atCaret"].clone(), typed["human"].clone()), (json!(":focus"), json!("abc"), json!(true), json!(true)), "{typed}");
    assert_eq!(c[at("press")].1["key"], "Backspace");
    assert!(at("recordStop") < at("close"), "{c:?}");
    assert!(!c.iter().any(|(_, a)| a.to_string().contains("data-wf-text-target")), "a lookup wrote to the page: {c:?}");
    assert!(run.stdout.contains("hits 1") && run.stdout.contains("browser") && run.stdout.contains("recording: run/run.mp4"), "{}", run.stdout);
    let v = report(&s.repo.join("run"));
    assert_eq!((v["stubs"]["declared"].as_array().map(Vec::len), v["stubs"]["hits"].as_array().map(Vec::len)), (Some(1), Some(1)), "{v}");
    assert!(std::fs::read_to_string(s.repo.join("run/report.html")).unwrap().contains("Network stubs"));

    // When a page logs a warning while it loads, then an info line, a warning and an unhandled rejection after it opened, and
    // a script reads a rect and the warnings, then saves a screenshot into a folder beside the test output dir
    let (loading, rejection) = ("warn: while it loads", "error: Uncaught (in promise) Error: late");
    d.clear_rules();
    d.rule("consoleLogs", "\"warn\"", vec![json!({ "count": 3, "lines": [loading, "warn: boom", rejection] })]);
    d.rule("consoleLogs", "", vec![json!({ "count": 1, "lines": [loading] }), json!({ "count": 4, "lines": [loading, "info: loaded", "warn: boom", rejection] })]);
    defaults(&d, &s);
    let out_dir = s.repo.join("out").display().to_string();
    let env = format!("out={out_dir}");
    let saved = browser("device:open_link('http://x/'); local e = device:get_element({id='Go'}); log(json.encode({ bottom = e.bounds.bottom, w = device:get_logs() })); device:take_screenshot(env.out .. '/first shot.png')", &["--test-output-dir", "shots", "--env", &env], &[]);
    // Then the rect comes from the finder, the warnings are every warn and error line since the session opened, the one logged
    // while the page loaded and the unhandled rejection included, and the report links the screenshot with every warning
    let all = json!([loading, "warn: boom", rejection]);
    assert_eq!((saved.exit_code, logged(&saved)), (0, json!({ "bottom": 60, "w": all })), "{}", out(&saved));
    assert_eq!(of("consoleLogs").iter().map(|a| a["level"].clone()).collect::<Vec<_>>(), ["warn", "warn"], "the script's read, the run's own read");
    let v = report(&s.repo.join("shots"));
    assert_eq!(v["verdict"], "pass");
    assert_eq!(v["screenshots"].as_array().unwrap().iter().map(|x| x["href"].clone()).collect::<Vec<_>>(), vec![json!("../out/first%20shot.png")], "{v}");
    assert!(v["warnings"] == all && v["message"].is_null() && v["traceback"].is_null(), "{v}");
    let page = std::fs::read_to_string(s.repo.join("shots/report.html")).unwrap();
    assert!(page.contains("src=\"../out/first%20shot.png\"") && !page.contains("Failure") && !page.contains(&format!("src=\"{out_dir}")), "{page}");

    // When a script reads the logs below the default level and clears them, so the run's own read at its end finds none
    d.clear_rules();
    d.rule("consoleLogs", "", vec![json!({ "count": 2, "lines": ["info: loaded", "warn: boom"] }), json!({ "count": 0, "lines": [] })]);
    defaults(&d, &s);
    let cleared = browser("device:open_link('http://x/'); local w = device:get_logs{level='info'}; device:clear_logs(); log(#w)", &["--test-output-dir", "dbg"], &[]);
    // Then the warning it read is in the report all the same, and the info line is not
    assert_eq!((cleared.exit_code, report(&s.repo.join("dbg"))["warnings"].clone()), (0, json!(["warn: boom"])), "{}", out(&cleared));
    d.clear_rules();
    defaults(&d, &s);

    // When a script looks up a point, uses a command the browser has no equivalent for, or fails after opening
    let unsupported = browser("device:get_element({point='1,2'})", &[], &[]);
    let unbound = browser("log('before')\ndevice:run_shell('ls')\nlog('after')", &[], &[]);
    let failing = browser("device:open_link('http://x/'); error('boom')", &[], &[]);
    let closed = of("close");
    let cold = browser("log('x')", &["--cold-start"], &[]);
    // Then each fails at its line saying why, the session is still closed, and --cold-start is refused
    assert_eq!(unsupported.exit_code, 1);
    assert!(unsupported.stderr.contains("a point is tap_on's only") && unsupported.stderr.contains("t.lua:1"), "{}", unsupported.stderr);
    assert_eq!(unbound.exit_code, 1);
    assert!(unbound.stderr.contains("not on browser: a page has no OS shell") && unbound.stderr.contains("t.lua:2"), "{}", unbound.stderr);
    assert!(unbound.stdout.contains("before") && !unbound.stdout.contains("after"), "{}", unbound.stdout);
    assert_eq!(failing.exit_code, 1);
    assert!(closed.iter().any(|a| a["name"].as_str().is_some_and(|n| n.starts_with("run-"))), "{closed:?}");
    assert_eq!(cold.exit_code, 1);
    assert!(out(&cold).contains("--cold-start is for Android"), "{}", out(&cold));

    // When the script waits for an element: by default, with a 0.5 s budget, with its own longer timeout, and probing for one
    // that is absent
    let default = browser("device:open_link('http://x/'); device:assert_visible({id='Go'})", &[], &[]);
    let default_observers = of("waitObserve");
    let budget = browser("device:open_link('http://x/'); device:assert_visible({id='Go'})", &[], &[("WF_WAIT_BUDGET_MS", "500")]);
    let budget_observers = of("waitObserve");
    let slow = browser("device:open_link('http://x/'); device:assert_visible({id='Go'}, {timeout=30000})", &[], &[("WF_WAIT_BUDGET_MS", "500")]);
    let slow_observers = of("waitObserve");
    let absent = |d: &MockDaemon, answers: Vec<&str>| {
        d.clear_rules();
        d.rule("waitObserve", "", answers.into_iter().map(|a| json!({ "result": a })).collect());
        defaults(d, &s);
    };
    absent(&d, vec!["{\"seen\":false,\"ms\":1000}"]);
    let probed = browser("device:open_link('http://x/'); log('seen', tostring(device:extended_wait_until{visible={id='Go'}, timeout=1000, optional=true}))", &[], &[]);
    let probe_shots = of("screenshot");
    // Then one observer subscribes in the app frame for a slice of at most 1 s (so a stop is read between slices), the default
    // timeout is the budget, a timeout of its own is kept and its timer cleared, and an optional miss answers nil without
    // failing or taking a screenshot
    assert_eq!((default.exit_code, budget.exit_code, slow.exit_code, probed.exit_code), (0, 0, 0, 0), "{}{}{}", out(&default), out(&budget), out(&probed));
    let expr = |a: &Value| a["expression"].as_str().unwrap_or_default().to_string();
    assert!(default_observers.len() == 1 && default_observers[0]["timeout"] == 1000 && default_observers[0]["outer"] == true && expr(&default_observers[0]).contains("MutationObserver"), "{default_observers:?}");
    assert!(budget_observers.len() == 1 && budget_observers[0]["timeout"] == 500 && !expr(&budget_observers[0]).contains("30000"), "{budget_observers:?}");
    assert!(slow_observers.len() == 1 && slow_observers[0]["timeout"] == 1000 && expr(&slow_observers[0]).contains("clearTimeout(timer)"), "{slow_observers:?}");
    assert!(probed.stdout.contains("seen\tnil") && probe_shots.is_empty(), "{}", out(&probed));

    // When the element never appears, and when it appears only after the budget ran out (two slices of 1 s say not yet)
    let broken = browser("device:open_link('http://x/'); device:assert_visible({id='Go'})", &[], &[("WF_WAIT_BUDGET_MS", "2000"), ("WF_WAIT_DIAGNOSE_MS", "1000")]);
    let broken_shots = of("screenshot");
    let broken_observers = of("waitObserve");
    absent(&d, vec!["{\"seen\":false,\"ms\":1000}", "{\"seen\":false,\"ms\":1000}", "{\"seen\":true,\"ms\":40}"]);
    let late = browser("device:open_link('http://x/'); device:assert_visible({id='Go'})", &[], &[("WF_WAIT_BUDGET_MS", "2000")]);
    d.clear_rules();
    defaults(&d, &s);
    // Then both fail: the first as broken with the budget, its line and a final screenshot, the second as slow
    assert_eq!(broken.exit_code, 1);
    assert!(broken.stderr.contains("t.lua:1: assert_visible{id='Go'} timed out after 2000 ms -- broken"), "{}", broken.stderr);
    assert!(!broken_shots.is_empty(), "no final screenshot");
    let slices: Vec<u64> = broken_observers.iter().filter_map(|a| a["timeout"].as_u64()).collect();
    assert!(slices.len() == 3 && slices[0] == 1000 && slices.iter().all(|t| (900..=1000).contains(t)), "two slices of the budget, one of the diagnosis: {slices:?}");
    assert_eq!(late.exit_code, 1);
    assert!(late.stderr.contains("slow") && !late.stderr.contains("broken"), "{}", late.stderr);

    // When a logpoint is armed from the command line and the script reads its hits, and a script arms one itself
    let flagged = browser("device:open_link('http://x/'); local h = device:get_debug_hits(); log(#h .. ' ' .. h[1].file)", &["--test-output-dir", "dbg", "--logpoint", "src/a.ts:3:x > 1"], &[]);
    let flagged_points = of("debugSet");
    let scripted = browser("device:debug{ file = 'src/a.ts', line = 3 }", &[], &[]);
    // Then the point is armed deferred with its condition, the script reads the hit, and the hits are one JSON line each beside
    // the recording; a script cannot arm one: debug is no device method, and calling it fails at the script line
    assert_eq!(flagged.exit_code, 0, "{}", out(&flagged));
    assert!(flagged_points.iter().any(|a| a["file"] == "src/a.ts" && a["line"] == 3 && a["condition"] == "x > 1" && a["defer"] == true), "{flagged_points:?}");
    assert!(flagged.stdout.contains("1 src/a.ts") && flagged.stdout.contains("debug: dbg/debug.jsonl (1 hits)"), "{}", flagged.stdout);
    assert_eq!(std::fs::read_to_string(s.repo.join("dbg/debug.jsonl")).unwrap().lines().count(), 1);
    assert!(scripted.exit_code == 1 && scripted.stderr.contains("t.lua:1: attempt to call a nil value (method 'debug')"), "{}", out(&scripted));
}

#[test]
fn a_browser_tap_settles_its_element_in_one_page_evaluation_while_the_daemon_keeps_ticking() {
    // Given the mock page with Go holding still at 10,20-110,60
    let s = sandbox("wf-flow-harness-settle-");
    let d = daemon(&s);
    let tap = |env: &[(&str, &str)]| {
        d.clear();
        let run = script(&s, "browser", "device:open_link('http://x/'); device:tap_on({id='Go'})", &[], env);
        assert_eq!(run.exit_code, 0, "{}", out(&run));
        d.calls().into_iter().filter(|(m, _)| m == "waitObserve" || m == "outerEval" || m == "tap").collect::<Vec<_>>()
    };
    let expr = |a: &Value| a["expression"].as_str().unwrap_or_default().to_string();

    // When a script taps Go with the default settle bound, then with WF_SETTLE_MS=300
    let default = tap(&[]);
    let short = tap(&[("WF_SETTLE_MS", "300")]);
    // Then the lookup, the looks 100 ms apart until the box holds still, and the re-measure are one page evaluation that the
    // daemon runs while it keeps ticking (waitObserve), bounded by the settle, with no lookup around it; the tap lands inside Go
    let kinds = |c: &[(String, Value)]| c.iter().map(|(m, _)| m.clone()).collect::<Vec<_>>();
    assert_eq!((kinds(&default), kinds(&short)), (vec!["waitObserve".to_string(), "tap".into()], vec!["waitObserve".to_string(), "tap".into()]));
    let settle = &default[0].1;
    assert!(expr(settle).contains("setTimeout(look,100)") && expr(settle).contains("performance.now()+1000;") && expr(settle).contains("const from=null;") && settle["timeout"] == 1100 && settle["outer"] == true, "{settle}");
    assert!(expr(&short[0].1).contains("performance.now()+300;") && short[0].1["timeout"] == 400, "{}", short[0].1);
    let (x, y) = (default[1].1["x"].as_f64().unwrap(), default[1].1["y"].as_f64().unwrap());
    assert!((30.0..=90.0).contains(&x) && (28.0..=52.0).contains(&y), "{}", default[1].1);

    // When Go is not there at the first look and shows during the wait that follows
    d.clear_rules();
    let go = json!({ "bounds": { "left": 10, "top": 20, "right": 110, "bottom": 60 }, "full": { "left": 10, "top": 20, "right": 110, "bottom": 60 }, "text": "Go", "frame": "app", "path": ":root" });
    d.rule("waitObserve", "setTimeout(look,100)", vec![json!({ "result": "{\"hit\":null}" }), json!({ "result": json!({ "hit": go }).to_string() })]);
    defaults(&d, &s);
    let late = tap(&[]);
    // Then the wait's observer meets it, and the settle starts from the box the wait saw instead of a look of its own
    assert_eq!(kinds(&late), ["waitObserve", "waitObserve", "outerEval", "waitObserve", "tap"]);
    assert!(expr(&late[1].1).contains("MutationObserver") && expr(&late[2].1).contains("return r.hit"), "{late:?}");
    assert!(expr(&late[3].1).contains("const from={\"bounds\":{") && expr(&late[3].1).contains("\"full\":{"), "{}", expr(&late[3].1));
}

#[test]
fn a_browser_condition_is_checked_again_as_soon_as_the_page_changes_within_its_interval() {
    // Given a page whose Late button shows on the third look, and changes between each look and the next
    let s = sandbox("wf-flow-harness-condition-");
    let d = daemon(&s);
    d.clear_rules();
    let late = json!({ "bounds": { "left": 0, "top": 100, "right": 60, "bottom": 130 }, "text": "Late", "frame": "app", "path": ":root" });
    d.rule("outerEval", "Late", vec![evaluated(Value::Null), evaluated(Value::Null), evaluated(late)]);
    d.rule("waitObserve", "__wfChanges", vec![json!({ "result": "{\"changed\":true,\"ms\":4}" })]);
    defaults(&d, &s);

    // When a script waits for it by a condition checked every 500 ms
    let run = script(&s, "browser", "device:open_link('http://x/'); local e = device:extended_wait_until{condition=function() return device:get_element({id='Late'}) end, interval=500, timeout=5000}; log(json.encode({ text = e.text }))", &[], &[]);
    // Then between looks it watches the page's documents for their next change, at most the interval, and each change wakes
    // the next look instead of a 500 ms sleep, though never sooner than 50 ms after the last; a change since the look began
    // (or one a document watched only from now on may have had) counts, so one made while the watch was on its way wakes it
    assert_eq!((run.exit_code, logged(&run)), (0, json!({ "text": "Late" })), "{}", out(&run));
    let steps: Vec<(String, Value)> = d.calls().into_iter().filter(|(m, a)| (m == "outerEval" && a.to_string().contains("Late")) || m == "waitObserve").collect();
    assert_eq!(steps.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(), ["outerEval", "waitObserve", "outerEval", "waitObserve", "outerEval"], "{steps:?}");
    let watches: Vec<&Value> = steps.iter().filter(|(m, _)| m == "waitObserve").map(|(_, a)| a).collect();
    let number_after = |e: &str, at: &str| -> u64 { e.split(at).nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse().ok()).unwrap_or(u64::MAX) };
    for watch in watches {
        let e = watch["expression"].as_str().unwrap_or_default();
        let (floor, look) = (number_after(e, "const left="), number_after(e, "if(unknown||t0-w.at<="));
        assert!(watch["timeout"] == 500 && watch["outer"] == true && e.contains("for(const d of docs())") && e.contains("timer=setTimeout(()=>done(hit),500)"), "{watch}");
        assert!((40..=50).contains(&floor) && look < 500 && e.contains("w.obs.forEach((o)=>o.disconnect())"), "{watch}");
    }

    // When a condition is waited for before the script opens the app
    d.clear();
    let early = script(&s, "browser", "local k = 0; device:extended_wait_until{condition=function() k = k + 1; return k >= 2 end, interval=50, timeout=1000}; log(json.encode({ checks = k }))", &[], &[]);
    // Then it pauses between checks, with no page to watch
    assert_eq!((early.exit_code, logged(&early)), (0, json!({ "checks": 2 })), "{}", out(&early));
    assert_eq!(d.count("waitObserve", ""), 0, "{:?}", d.calls());
}

#[test]
fn a_browser_cli_verb_drives_an_open_session_but_not_a_running_tests() {
    // Given one session a running test opened (its owner is a live pid) and nothing else
    let s = sandbox("wf-flow-harness-browser-cli-");
    let d = daemon(&s);
    let owner = std::process::id();
    d.results(&[("list", json!([{ "name": "run-7", "devicePreset": "iphone-15-pro", "renderMode": "device-frame", "orientation": "portrait", "owner": owner }]))]);
    let cli = |args: &[&str]| {
        d.clear();
        let r = s.run(args, &[("WF_BROWSER_NO_START", "1")]);
        let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", out(&r)));
        (r.exit_code, v)
    };
    // When a verb names no device, and when it names the test's session
    let (refused, why) = cli(&["browser", "get-element", "--id", "Go"]);
    let refused_calls = d.calls();
    let (named, got) = cli(&["browser", "get-element", "--id", "Go", "--device", "run-7"]);
    let found = d.calls();
    let (_, screen) = cli(&["browser", "get-device-state", "screen", "--device", "run-7"]);
    let (tapped, tap) = cli(&["browser", "tap-on", "--point", "50%,10%", "--device", "run-7"]);
    let tap_call = d.calls().into_iter().find(|(m, _)| m == "tap").map(|(_, a)| a);
    // Then the first is refused before anything reaches the page, naming the session and the flag
    assert_eq!(refused, 1, "{why}");
    assert!(why["error"].as_str().unwrap().contains(&format!("'run-7' belongs to a running test (pid {owner}): pass --device run-7")), "{why}");
    assert!(refused_calls.iter().all(|(m, _)| m == "list"), "{refused_calls:?}");
    // and the named one reads the element through the page's finder, the screen from the preset, and taps a screen point
    assert_eq!((named, got["ok"].clone(), got["platform"].clone(), got["device"].clone(), got["result"]["bounds"]["bottom"].clone(), got["result"]["text"].clone()), (0, json!(true), json!("browser"), json!("run-7"), json!(60), json!("Go")), "{got}");
    let lookup = found.iter().find(|(m, _)| m == "outerEval").map(|(_, a)| a["expression"].as_str().unwrap_or_default().to_string()).unwrap_or_default();
    assert!(lookup.contains("\"id\":[\"(?<![\\\\s\\\\S])(?:(?:Go)|(?:Go))(?![\\\\s\\\\S])\",\"imsu\"]") && found.iter().all(|(_, a)| a.get("name").is_none_or(|n| n == "run-7")), "{lookup}");
    assert_eq!(screen["result"], json!({ "width": 393, "height": 852, "dpr": 3 }), "{screen}");
    assert_eq!((tapped, tap["result"]["x"].clone(), tap["result"]["y"].clone()), (0, json!(196.5), json!(85.2)), "{tap}");
    assert_eq!(tap_call.map(|a| (a["x"].clone(), a["y"].clone())), Some((json!(196.5), json!(85.2))));
}

#[test]
fn the_maestro_shaped_commands_wait_read_and_refuse_in_lua_and_on_the_cli() {
    // Given the dump fixture's screen, a wait log and a short diagnosis look
    let s = sandbox("wf-flow-harness-maestro-");
    let waits = s.repo.join("maestro-waits.jsonl").display().to_string();
    let env = [("WF_WAIT_LOG", waits.as_str()), ("WF_WAIT_DIAGNOSE_MS", "300")];

    // When a script waits, reads and checks with the new names, and a condition tries to act
    let lua = "local el = device:assert_visible({id='TamilWriteCheck'}, {label='check shown'})\n\
        local miss = device:extended_wait_until{visible={id='Nope'}, timeout=200, optional=true}\n\
        local badge = device:extended_wait_until{condition=function() local e = device:get_element({id='TamilWriteVerdictBadge'}) return e and e.text end, timeout=1000, label='badge read'}\n\
        local acted, why = pcall(function() device:extended_wait_until{condition=function() device:tap_on({id='TamilWriteCheck'}) return true end, timeout=500} end)\n\
        run_flow('read', function() log(json.encode({ bottom = el.bounds.bottom, miss = miss == nil, badge = badge, acted = acted, why = tostring(why), kb = device:get_device_state('keyboard').visible, gone = device:assert_not_visible({id='Nope'}) })) end)";
    let r = script(&s, "android", lua, &["--test-output-dir", "maestro"], &env);
    // Then each answers as the table says: the element, nil for the optional miss, the condition's value, and a raise for the act
    assert_eq!(r.exit_code, 0, "{}", out(&r));
    let v = logged(&r);
    assert_eq!((v["bottom"].clone(), v["miss"].clone(), v["badge"].clone(), v["acted"].clone(), v["kb"].clone(), v["gone"].clone()), (json!(260), json!(true), json!("LOOKS RIGHT · 92"), json!(false), json!(true), json!(true)), "{v}");
    assert!(v["why"].as_str().unwrap().contains("a condition must not act: tap_on"), "{v}");
    assert!(!s.calls("adb").iter().any(|c| c.contains("input tap")), "the condition's tap reached the device: {:?}", s.calls("adb"));
    // and the optional miss left no evidence, while every wait is a log line and a span under its mode's name
    assert!(!std::fs::read_dir(s.repo.join("maestro")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with("wait-timeout")));
    let lines = jsonl(&waits);
    let shape: Vec<Value> = lines.iter().map(|l| json!([l["call"], l["ok"]])).collect();
    assert_eq!(shape, vec![json!(["assert_visible", true]), json!(["extended_wait_until:visible:optional", false]), json!(["extended_wait_until:condition", true]), json!(["extended_wait_until:condition", false]), json!(["assert_not_visible", true])], "{lines:?}");
    assert_eq!((lines[0]["sel"].clone(), lines[2]["label"].clone(), lines[1]["timeoutMs"].clone()), (json!("{id='TamilWriteCheck'}"), json!("badge read"), json!(200)));
    let timing = &report(&s.repo.join("maestro"))["timing"];
    let row = |verb: &str| timing["byVerb"].as_array().unwrap().iter().find(|r| r["verb"] == verb).unwrap_or_else(|| panic!("no {verb} in {timing}")).clone();
    assert_eq!(row("extended_wait_until:visible:optional")["misses"], json!(1), "{timing}");
    row("assert_visible");

    // When a wait that is not optional runs out
    let failed = script(&s, "android", "device:assert_visible({id='Never'}, {timeout=500, label='never there'})", &["--test-output-dir", "never"], &env);
    // Then it fails at its line naming the call, the label, the verdict, the screenshot and the evidence it left
    assert_eq!(failed.exit_code, 1, "{}", out(&failed));
    assert!(failed.stderr.contains("t.lua:1: assert_visible{id='Never'} 'never there' timed out after 500 ms -- broken: never met; screenshot: never/wait-timeout-"), "{}", failed.stderr);
    let evidence = failed.stderr.split("evidence: ").nth(1).and_then(|t| t.split(';').next()).expect("an evidence path").trim().to_string();
    let e: Value = serde_json::from_str(&std::fs::read_to_string(s.repo.join(&evidence)).unwrap()).unwrap();
    assert_eq!((e["element"].clone(), e["matches"].clone(), e["keyboard_visible"].clone()), (json!("absent"), json!(0), json!(true)), "{e}");
    assert!(failed.stderr.contains("next: the selector matched nothing"), "{}", failed.stderr);
    // and the run's report lists that evidence, linked from the record dir
    let listed = report(&s.repo.join("never"))["evidence"].clone();
    assert!(listed.as_array().is_some_and(|l| l.len() == 1 && l[0]["href"].as_str().is_some_and(|h| evidence.ends_with(h) && h.starts_with("wait-timeout-"))), "{listed}");

    // When the same commands run as CLI verbs
    let xml = s.repo.join("ui.xml").display().to_string();
    let cli = |args: &[&str]| s.run(args, &[("STUB_UI_XML", xml.as_str()), ("WF_ANDROID_UIA", "dump"), ("WF_ANDROID_LEASE", "none")]);
    let read = cli(&["android", "get-element", "--id", "TamilWriteVerdictBadge"]);
    let shown = cli(&["android", "extended-wait-until", "--condition", "device:get_device_state('keyboard').visible", "--timeout", "0", "--json"]);
    let acting = cli(&["android", "extended-wait-until", "--condition", "device:tap_on('Go')", "--timeout", "0"]);
    let (bogus, css, old) = (cli(&["android", "tap-on", "--bogus"]), cli(&["android", "get-element", "--css", ".x"]), cli(&["android", "get-element", "id:Go"]));
    // Then each prints one envelope: the result, a failure with exit 1, or a usage error with exit 2
    let line = |r: &RunResult| -> Value { serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", out(r))) };
    let got = line(&read);
    assert_eq!((read.exit_code, got["ok"].clone(), got["command"].clone(), got["platform"].clone(), got["device"].clone(), got["result"]["text"].clone(), got["result"]["bounds"]["bottom"].clone()), (0, json!(true), json!("get-element"), json!("android"), json!("emulator-5554"), json!("LOOKS RIGHT · 92"), json!(50)), "{got}");
    let met = line(&shown);
    assert_eq!((shown.exit_code, met["result"]["met"].clone(), met["result"]["value"].clone(), met["result"]["checks"].clone()), (0, json!(true), json!(true), json!(1)), "{met}");
    assert_eq!((acting.exit_code, line(&acting)["ok"].clone()), (1, json!(false)), "{}", out(&acting));
    assert!(line(&acting)["error"].as_str().unwrap().contains("a condition must not act: tap_on"), "{}", acting.stdout);
    for (r, why) in [(&bogus, "unknown option --bogus"), (&css, "not on android"), (&old, "old selector form 'id:Go': use {id='Go'}")] {
        let v = line(r);
        assert!(r.exit_code == 2 && v["ok"] == false && v["error"].as_str().unwrap().contains(why) && v["usage"].is_string(), "{why}: {v}");
    }
}

/// A 720x400 grey screen, with a black square at the top left when `mark` is set.
fn screen_png(path: &Path, mark: bool) {
    let (w, h) = (720u32, 400u32);
    let data: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| { let v = if mark && x < 200 && y < 200 { 0 } else { 180 }; [v, v, v] })).collect();
    let mut bytes = Vec::new();
    let mut e = png::Encoder::new(&mut bytes, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&data).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn png_size(path: &Path) -> (u32, u32) {
    let r = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap())).read_info().unwrap();
    (r.info().width, r.info().height)
}

#[test]
fn goldens_are_kept_recorded_compared_and_shown_in_the_report() {
    let s = sandbox("wf-flow-harness-goldens-");
    let _daemon = daemon(&s);
    s.file("run/run.mp4", "video");
    screen_png(&s.repo.join("shot.png"), false);
    let (gdir, rdir) = (s.repo.join("goldens").display().to_string(), s.repo.join("run").display().to_string());
    let golden = |extra: &[&str]| {
        let mut args = vec!["--goldens-dir", gdir.as_str(), "--test-output-dir", rdir.as_str()];
        args.extend_from_slice(extra);
        script(&s, "browser", "device:open_link('http://x/'); device:tap_on({id='Go'}); local g = device:assert_screenshot('home'); log(json.encode({ verdict = g.verdict, pct = g.match_percentage }))", &args, &[])
    };

    // When assert_screenshot runs before any golden exists, without and then with --update-goldens
    let missing = golden(&[]);
    let recorded = golden(&["--update-goldens"]);
    // Then the first fails naming the flag, and the second writes a 360 px golden with a sidecar and a gallery entry
    assert_eq!(missing.exit_code, 1);
    assert!(missing.stderr.contains("no golden at") && missing.stderr.contains("--update-goldens"), "{}", missing.stderr);
    assert_eq!(recorded.exit_code, 0, "{}", out(&recorded));
    assert!(recorded.stdout.contains("\"verdict\":\"updated\"") && recorded.stdout.contains("goldens: ") && recorded.stdout.contains("1 updated"), "{}", recorded.stdout);
    assert_eq!(png_size(&s.repo.join("goldens/home.png")), (360, 200));
    let meta: Value = serde_json::from_str(&std::fs::read_to_string(s.repo.join("goldens/home.json")).unwrap()).unwrap();
    assert_eq!((meta["name"].clone(), meta["platform"].clone(), meta["last_action"].clone(), meta["step"].clone(), meta["warnings"].clone()), (json!("home"), json!("browser"), json!("tap_on{id='Go'}"), json!(2), json!(["warn: boom"])));
    assert!(meta["timings"]["capture_ms"].is_u64(), "{meta}");
    let gallery = std::fs::read_to_string(s.repo.join("run/goldens.html")).unwrap();
    assert!(gallery.contains("home") && gallery.contains("updated"), "{gallery}");

    // When the same screen runs again, with no flag
    let matched = golden(&[]);
    // Then it compares, matches fully and writes no diff
    assert_eq!(matched.exit_code, 0, "{}", out(&matched));
    assert!(matched.stdout.contains("\"verdict\":\"match\"") && matched.stdout.contains("\"pct\":100") && !s.repo.join("run/goldens/home.diff.png").exists(), "{}", matched.stdout);
    let summary: Value = serde_json::from_str(&std::fs::read_to_string(s.repo.join("run/goldens.json")).unwrap()).unwrap();
    assert_eq!((summary["ok"].clone(), summary["goldens"][0]["verdict"].clone()), (json!(true), json!("match")));

    // When the screen gains a black square
    screen_png(&s.repo.join("shot.png"), true);
    let differs = golden(&[]);
    // Then the run fails at the golden's line, and the actual, expected and diff images sit beside the run, in the gallery and report
    assert_eq!(differs.exit_code, 1);
    assert!(differs.stderr.contains("golden 'home' differs") && differs.stderr.contains("t.lua:1"), "{}", differs.stderr);
    for f in ["home.actual.png", "home.expected.png", "home.diff.png", "home.json"] {
        assert!(s.repo.join("run/goldens").join(f).exists(), "missing {f}");
    }
    let gallery = std::fs::read_to_string(s.repo.join("run/goldens.html")).unwrap();
    assert!(gallery.contains("mismatch") && gallery.contains("home.diff.png"), "{gallery}");
    let v = report(&s.repo.join("run"));
    let g = &v["goldens"][0];
    assert_eq!((v["verdict"].as_str(), g["name"].as_str(), g["verdict"].as_str()), (Some("fail"), Some("home"), Some("mismatch")), "{v}");
    assert_eq!((g["expected"]["href"].as_str(), g["actual"]["href"].as_str(), g["diff"]["href"].as_str()), (Some("goldens/home.expected.png"), Some("goldens/home.actual.png"), Some("goldens/home.diff.png")), "{v}");
    assert!(v["screenshots"].as_array().unwrap().is_empty(), "golden images are not screenshots: {v}");

    // When a golden name would escape the goldens dir
    let escape = script(&s, "browser", "device:assert_screenshot('../escape')", &["--update-goldens"], &[]);
    // Then the call is refused
    assert_eq!(escape.exit_code, 1);
    assert!(escape.stderr.contains("not a golden name"), "{}", escape.stderr);

    // When an Android script takes a golden of its 720 px screencap with --update-goldens and no --goldens-dir
    let screen = s.repo.join("screen.png");
    screen_png(&screen, false);
    let android = script(&s, "android", "device:assert_screenshot('android-home')", &["--update-goldens"], &[("STUB_PNG", &screen.display().to_string())]);
    // Then it lands in docs/runbooks/goldens/android/ of the repo with its sidecar
    assert_eq!(android.exit_code, 0, "{}", out(&android));
    let dir = s.repo.join("docs/runbooks/goldens/android");
    assert_eq!(png_size(&dir.join("android-home.png")), (360, 200));
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(dir.join("android-home.json")).unwrap()).unwrap()["platform"], "android");
}
