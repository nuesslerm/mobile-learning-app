//! The journeys of the device harness that cross into what only the full `wf` serves: the script API reference with the old spellings
//! refused (`wf run`, `flow run --help`) and a run published to the bucket (the publisher `wf` sets). The rest of the harness is `clis/wf-harness`. `adb` is an argv-recording stub whose screen is scripted through env vars.
use crate::support::{MockReply, MockServer, RunResult, SlotSandbox};
use serde_json::{json, Value};
use std::path::Path;
use wf_harness::testing::MockDaemon;

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

fn sandbox(prefix: &str) -> SlotSandbox {
    let s = SlotSandbox::new(prefix);
    s.stub("adb", STUB_ADB);
    s.file("ui.xml", UI_XML);
    s.file("shot.png", "PNG");
    s
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
fn an_android_script_reads_the_screen_drives_the_device_and_calls_an_agent() {
    // Given a claimed emulator whose screen is the dump fixture
    let s = sandbox("wf-flow-harness-android-");
    let android = |lua: &str, extra: &[&str], env: &[(&str, &str)]| script(&s, "android", lua, extra, env);

    // When a script finds a node, taps it, reads texts and the editor buffer, and looks for one that is not there
    let found = android("local e = device:get_element({id='TamilWriteCheck'})\nlog(json.encode({ found = e ~= nil, bottom = e.bounds.bottom, platform = device.platform }))\ndevice:tap_on({id='TamilWriteCheck'})", &[], &[]);
    let found_calls = s.calls("adb");
    let texts = android("log(json.encode({ badge = device:get_element({id='TamilWriteVerdictBadge'}).text, buffer = device:get_element({id='CodeEditorInput'}).text, missing = tostring(device:get_element({id='Nope'})) }))", &[], &[]);
    // Then the values come back, the tap lands on the node's centre, a missing view is nil and the buffer is unescaped
    assert_eq!((found.exit_code, logged(&found)), (0, json!({ "found": true, "bottom": 260, "platform": "android" })), "{}", out(&found));
    assert!(found_calls.iter().any(|c| c.ends_with("shell input tap 200 230")), "{found_calls:?}");
    assert_eq!(logged(&texts), json!({ "badge": "LOOKS RIGHT · 92", "buffer": "line one\n    line two", "missing": "nil" }));

    // When it reads the keyboard and the JS warnings, clears them, types, presses a key and reads its --env values
    let state = android("log(json.encode({ kb = device:get_device_state('keyboard').visible, warnings = device:get_logs() }))\ndevice:clear_logs()\ndevice:input_text('ab'); device:press_key('Enter')\nlog(env.problem .. ':' .. env.n)", &["--env", "problem=two-sum", "--env", "n=3"], &[("STUB_LOGCAT", "W/ReactNativeJS( 1): boom\\nI/ReactNativeJS( 1): fine\\n")]);
    let calls = s.calls("adb");
    // Then it saw the keyboard and exactly the warning line, the buffer was cleared, and the keys and args arrived
    assert_eq!(state.exit_code, 0, "{}", out(&state));
    assert_eq!((logged(&state)["kb"].clone(), logged(&state)["warnings"].as_array().unwrap().len()), (json!(true), 1));
    assert!(calls.iter().any(|c| c.contains("logcat") && c.contains("-c")) && calls.iter().any(|c| c.contains("input text 'ab'")) && calls.iter().any(|c| c.contains("input keyevent 66")), "{calls:?}");
    assert!(state.stdout.contains("two-sum:3"), "{}", state.stdout);

    // When it reads pixels on a 4x4 screen, and one off the screen
    let png = s.file("screen.png", base64::Engine::decode(&base64::engine::general_purpose::STANDARD, FOUR_BY_FOUR).unwrap());
    let pixels = android("local a, b = device:get_pixel('2,1'), device:get_pixel('0,0')\nlog(json.encode({ a = a, b = b }))", &[], &[("STUB_PNG", &png)]);
    let off = android("device:get_pixel('9,9')", &[], &[("STUB_PNG", &png)]);
    // Then the colours match the image and the off-screen read names the screen size
    assert_eq!(logged(&pixels), json!({ "a": { "r": 0, "g": 122, "b": 255 }, "b": { "r": 255, "g": 255, "b": 255 } }));
    assert_eq!(off.exit_code, 1);
    assert!(off.stderr.contains("outside the 4x4 screen"), "{}", off.stderr);

    // When a wait never succeeds, with a wait log, a wait budget and a record dir
    let waits = s.repo.join("waits.jsonl").display().to_string();
    let budget = [("WF_WAIT_LOG", waits.as_str()), ("WF_WAIT_BUDGET_MS", "1000"), ("WF_WAIT_DIAGNOSE_MS", "200"), ("STUB_PNG", png.as_str())];
    let logged_waits = android("device:assert_visible({id='TamilWriteCheck'})\npcall(function() device:assert_not_visible({id='TamilWriteCheck'}) end)\ndevice:extended_wait_until{visible={id='TamilWriteCheck'}, timeout=1000, optional=true}\ndevice:assert_visible({id='TamilWriteCheck'}, {timeout=1000})", &[], &budget);
    let never = android("log('start')\ndevice:assert_visible({id='Never'}, {timeout=1000})\nlog('unreachable')", &["--test-output-dir", "wait-rec"], &budget);
    // Then each wait is one log line under its span name, and the failing one stops the script at its line with a screenshot beside its report
    assert_eq!(logged_waits.exit_code, 0, "{}", out(&logged_waits));
    let lines = jsonl(&waits);
    let shape: Vec<Value> = lines.iter().filter(|l| l["sel"] == "{id='TamilWriteCheck'}").map(|l| json!([l["call"], l["ok"], l["budgetMs"], l["platform"]])).collect();
    assert_eq!(shape, vec![json!(["assert_visible", true, 1000, "android"]), json!(["assert_not_visible", false, 1000, "android"]), json!(["extended_wait_until:visible:optional", true, 1000, "android"]), json!(["assert_visible", true, 1000, "android"])]);
    assert!(lines.iter().all(|l| l["ms"].is_u64()), "{lines:?}");
    assert_eq!(never.exit_code, 1, "{}", out(&never));
    assert!(never.stderr.contains("t.lua:2: assert_visible{id='Never'}") && !never.stdout.contains("unreachable"), "{}", out(&never));
    let shot = never.stderr.split("screenshot: ").nth(1).and_then(|t| t.split_whitespace().next()).map(|p| p.trim_end_matches(';')).expect("a screenshot path").to_string();
    assert!(s.repo.join(&shot).exists() && s.repo.join(&shot).parent().is_some_and(|d| d.join("report.json").exists()), "{shot}: {}", never.stderr);
    // and the report names the screen with its density
    assert_eq!(report(&s.repo.join("wait-rec"))["screen"], "4x4@213");

    // When an assertion fails
    let failed = android("assert_true(1 == 2, 'one is not two')", &[], &[]);
    // Then the run fails with the message
    assert_eq!(failed.exit_code, 1);
    assert!(failed.stderr.contains("one is not two"), "{}", failed.stderr);

    // When an adbd running as the shell user, and then as root, gets an audio clip injected
    let wav = s.file("clip.wav", b"RIFF");
    let inject = format!("device:set_microphone_input('{wav}')");
    let as_shell = android(&inject, &[], &[("STUB_UID", "2000")]);
    let shell_calls = s.calls("adb");
    let as_root = android(&inject, &[], &[("STUB_UID", "0")]);
    // Then `adb root` runs before the first push only when it is needed
    assert_eq!((as_shell.exit_code, as_root.exit_code), (0, 0), "{}{}", out(&as_shell), out(&as_root));
    let root = shell_calls.iter().position(|c| c.ends_with(" root")).unwrap_or_else(|| panic!("no adb root in {shell_calls:?}"));
    assert!(root < shell_calls.iter().position(|c| c.contains(" push ")).expect("a push"), "{shell_calls:?}");
    assert!(!s.calls("adb").iter().any(|c| c.ends_with(" root")), "{:?}", s.calls("adb"));

    // When the script asks an agent for an edit, and an agent that hangs past its timeout
    let agent = s.file("agent.sh", "#!/bin/sh\necho \"thinking about: $1\"\necho '{\"edit\":\"add a loop\"}'\n");
    let slow = s.file("slow.sh", "#!/bin/sh\nsleep 30\n");
    for f in [&agent, &slow] {
        std::fs::set_permissions(f, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    let answered = android("local r = agent.run{ prompt = 'pick one edit', json = true, timeout = 20 }\nlog(json.encode({ ok = r.ok, edit = r.result.edit, saw = r.text:match('pick one edit') ~= nil }))", &[], &[("WF_AGENT_CMD", &agent)]);
    let hung = android("local r = agent.run{ prompt = 'x', timeout = 1 }\nlog(tostring(r.timed_out) .. ':' .. tostring(r.ok))", &[], &[("WF_AGENT_CMD", &slow)]);
    // Then the script gets the text and the parsed JSON, and the hung agent is killed and reported as timed out
    assert_eq!(logged(&answered), json!({ "ok": true, "edit": "add a loop", "saw": true }));
    assert_eq!(hung.exit_code, 0, "{}", out(&hung));
    assert!(hung.stdout.contains("true:false"), "{}", hung.stdout);

    // When a logpoint is asked for on Android, whose run loads the EAS Update and has no inspector, with a server answering where
    // the retired Metro was
    let metro = MockServer::start(None);
    let metro_url = format!("http://127.0.0.1:{}", metro.port);
    let debug = android("log('ran-the-script')", &["--logpoint", "src/a.ts:3", "--test-output-dir", "lp"], &[("WF_METRO_URL", metro_url.as_str())]);
    // Then the run is refused before the device is touched or the script runs, the report says why, and nothing asked the server
    assert_eq!(debug.exit_code, 1, "{}", out(&debug));
    assert!(out(&debug).contains("--logpoint is browser only: an Android run loads the EAS Update, which has no inspector"), "{}", out(&debug));
    assert!(!debug.stdout.contains("ran-the-script") && !debug.stdout.contains("debug.jsonl"), "{}", debug.stdout);
    assert!(s.calls("adb").is_empty() && metro.requests().is_empty(), "{:?} {:?}", s.calls("adb"), metro.requests());
    let refused = report(&s.repo.join("lp"));
    assert!(refused["verdict"] == "error" && refused["message"].as_str().unwrap_or_default().contains("--logpoint is browser only"), "{refused}");
    // and a script that reads the hits on Android is told the method is a browser's
    let reads = android("local h = device:get_debug_hits()", &[], &[]);
    assert!(reads.exit_code == 1 && out(&reads).contains("not on android"), "{}", out(&reads));

    // When the script stubs a backend route and lets the device call it
    let backend = MockServer::start(None);
    backend.routes(&[("GET /api/problems", MockReply::text(200, "from the backend"))]);
    let lua = format!("local r = device:add_network_stub('POST', '*/api/leetcode/run', {{ json_body = {{ ok = true }}, status = 201 }})\nlog('answer ' .. device:run_shell('wf-probe ' .. r.port))\nlog('forwarded ' .. device:run_shell('wf-forward ' .. r.port .. ' {}'))\nlog('hits ' .. #device:get_network_stub_hits() .. ' ' .. device:get_network_stub_hits()[1].url)", backend.port);
    let stubbed = android(&lua, &["--test-output-dir", "stub-rec"], &[]);
    let calls = s.calls("adb");
    // Then the device got the canned answer and the hit was kept under the backend's URL, the backend ports went to the stub server, and the rules were cleared
    assert_eq!(stubbed.exit_code, 0, "{}", out(&stubbed));
    assert!(stubbed.stdout.contains("answer {\"ok\":true}") && stubbed.stdout.contains("hits 1 http://100.64.0.9:8789/api/leetcode/run"), "{}", stubbed.stdout);
    // and a call no stub matches went on to the backend its Host header names, leaving no hit
    assert!(stubbed.stdout.contains("forwarded from the backend") && backend.requests().len() == 1, "{}", stubbed.stdout);
    let rules = calls.iter().find(|c| c.contains("wf-stub-rules-ok")).unwrap_or_else(|| panic!("no rules in {calls:?}"));
    assert!(rules.contains("--dport 8787 -m comment --comment wf-stub -j DNAT --to-destination 127.0.0.1:") && rules.contains("--dport 8789 "), "{rules}");
    assert!(rules.contains("--ctorigdstport 8789 ! --ctstate DNAT -m comment --comment wf-stub -j REJECT --reject-with tcp-reset"), "a connection opened before the rule bypasses the stub: {rules}");
    assert!(rules.contains("shell for t in nat filter;"), "{rules}");
    // When the device refuses a stub rule part-way
    s.forget("adb");
    let refused = android("device:add_network_stub('POST', '*/api/leetcode/run', { json_body = { ok = true } })", &[], &[("STUB_RULES_FAIL", "1")]);
    // Then the run fails naming it, and the rules it did add are cleared at once, so no later run's backend traffic points at a stub that is gone
    let refused_calls = s.calls("adb");
    let at = refused_calls.iter().position(|c| c.contains("wf-stub-rules-ok")).unwrap_or_else(|| panic!("{refused_calls:?}"));
    assert!(refused.exit_code != 0 && out(&refused).contains("can't initialize table"), "{}", out(&refused));
    assert!(refused_calls.get(at + 1).is_some_and(|c| c.contains("shell for t in nat filter;") && !c.contains("wf-stub-rules-ok")), "{refused_calls:?}");
    // and the report keeps each stub rule with its packet count, to tell a missed rule from a request that went elsewhere
    assert_eq!(report(&s.repo.join("stub-rec"))["stubs"]["routing"]["rules"], json!(["7   420 DNAT tcp -- * * 0.0.0.0/0 0.0.0.0/0 tcp dpt:8789 /* wf-stub */ to:127.0.0.1:1"]));
    assert!(calls.last().is_some_and(|c| c.contains("--comment wf-stub") && c.contains("-D ")), "{calls:?}");

    // When the reference and the help are asked for, and the old spellings are used
    let api = s.run(&["android", "test", "--api"], &[]);
    let help = s.run(&["android", "test", "--help"], &[]);
    let old = s.run(&["run", "t.lua", "--platform", "android"], &[]);
    let old_verb = s.run(&["android", "run", "--script", "t.lua"], &[]);
    let old_flags: Vec<RunResult> = [["--arg", "a=b"], ["--record-dir", "d"], ["--debug", "f.ts:1"], ["--goldens", "x"]].iter().map(|f| s.run(&["android", "test", "t.lua", f[0], f[1]], &[])).collect();
    let flow_help = s.run(&["flow", "run", "--help"], &[]);
    // Then the reference lists what an agent needs and no name from before the rename, the help names the flags, `wf run`
    // exits 2 naming what to use, and `wf android run` and run's renamed flags exit 2 as an unknown verb and unknown options
    assert_eq!(api.exit_code, 0);
    for needle in ["device:tap_on(", "device:get_element(", "device:assert_visible(", "device:extended_wait_until{", "device:input_text(", "device:get_device_state(", "device:get_logs(", "device:take_screenshot(", "device:assert_screenshot(", "--update-goldens", "{id="] {
        assert!(api.stdout.contains(needle), "missing {needle}: {}", api.stdout);
    }
    for gone in ["enamed", "device:tap(", "device:wait(", "assert_that", "json.decode"] {
        assert!(!api.stdout.contains(gone), "{gone} is still in the reference: {}", api.stdout);
    }
    for flag in ["--env", "--test-output-dir", "--update-goldens", "--cold-start"] {
        assert!(out(&help).contains(flag), "{flag}: {}", out(&help));
    }
    assert!(!out(&help).contains("[--logpoint") && !out(&help).contains("Hermes") && !out(&help).contains("WF_METRO_URL") && !api.stdout.contains("--logpoint file:line[:cond]]"), "{}", out(&help));
    assert_eq!(old.exit_code, 2, "{}", out(&old));
    let gone: Value = serde_json::from_str(old.stdout.trim()).unwrap();
    assert_eq!(gone["error"], "wf run is gone");
    assert!(gone["next"].as_str().unwrap().contains("wf android test <script.lua>"), "{gone}");
    let unknown: Value = serde_json::from_str(old_verb.stdout.trim()).unwrap();
    assert!(old_verb.exit_code == 2 && unknown["error"] == "unknown verb 'run'" && unknown["verbs"].as_array().unwrap().contains(&json!("test")), "{unknown}");
    let errors: Vec<(i32, Value, bool)> = old_flags.iter().map(|r| serde_json::from_str::<Value>(r.stdout.trim()).map(|v| (r.exit_code, v["error"].clone(), v["usage"].as_str().is_some_and(|u| u.starts_with("wf android test <script.lua>")))).unwrap_or((r.exit_code, Value::Null, false))).collect();
    assert_eq!(errors, vec![(2, json!("unexpected argument '--arg' found"), true), (2, json!("unexpected argument '--record-dir' found"), true), (2, json!("unexpected argument '--debug' found"), true), (2, json!("--goldens is gone: assert_screenshot compares with its golden on every run; --update-goldens writes them"), true)]);
    assert!(flow_help.exit_code == 0 && flow_help.stdout.contains("no --script"), "{}", flow_help.stdout);
}

#[test]
fn a_browser_run_told_to_publish_leaves_its_bundle_in_the_bucket_through_the_publisher_wf_sets() {
    // Given a browser session on a mock daemon, a bucket whose index lists no run, and a table that takes a row
    let s = sandbox("wf-flow-harness-publish-");
    let daemon = MockDaemon::start();
    s.seed(&json!({ "main/mla-tamagui/default": { "createdAt": "2026-01-01T00:00:00Z", "browserMcp": { "url": daemon.url, "startedAt": "2026-01-01T00:00:00Z" } } }));
    let bucket = MockServer::start(None);
    bucket.queue(vec![MockReply::json(200, json!({ "schema": 1, "runs": [] }))], true);
    let ords = MockServer::start(None);
    ords.queue(vec![MockReply::json(201, json!({}))], false);
    let par = format!("http://127.0.0.1:{}/p/", bucket.port);
    let read = format!("http://127.0.0.1:{}/o", bucket.port);
    let table = format!("http://127.0.0.1:{}/ords/health_rpt/test_runs/", ords.port);
    let env = [("VERIFICATION_ARTIFACTS_PAR_URL", par.as_str()), ("VERIFICATION_ARTIFACTS_READ_URL", read.as_str()), ("TEST_RUNS_ORDS_BASE_URL", table.as_str()), ("HEALTH_REPORTS_ORDS_PASSWORD", "pw"), ("WF_SITE", "sandbox")];

    // When a script runs with --publish
    let r = script(&s, "browser", "log('ran')", &["--test-output-dir", "pub-rec", "--no-record", "--publish"], &env);
    // Then the run passes, says where its bundle is, and the bundle and the index are in the bucket with the run's report
    assert_eq!(r.exit_code, 0, "{}", out(&r));
    let said = r.stdout.lines().find_map(|l| l.strip_prefix("bundle: ")).unwrap_or_else(|| panic!("no bundle line: {}", out(&r))).to_string();
    let puts: Vec<String> = bucket.requests().iter().filter(|q| q.method == "PUT").map(|q| q.url.clone()).collect();
    assert!(said.starts_with(&format!("{read}/verification/test-runs/run-")) && said.ends_with("/bundle.json"), "{said}");
    assert!(puts.iter().any(|u| u.ends_with("/report.json")) && puts.iter().any(|u| u.ends_with("/bundle.json")) && puts.last().is_some_and(|u| u.ends_with("/test-runs/index.json")), "{puts:?}");
    // and the row was posted to the table, and a run not told to publish touches neither
    assert_eq!(ords.requests().len(), 1);
    let before = bucket.requests().len();
    let quiet = script(&s, "browser", "log('ran')", &["--test-output-dir", "quiet-rec", "--no-record"], &env);
    assert_eq!((quiet.exit_code, bucket.requests().len()), (0, before), "{}", out(&quiet));
}
