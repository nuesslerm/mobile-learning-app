//! The command layer both drivers share (`clis/harness-core/src/device`): a selector pattern as the three regex engines behind it read
//! it (the Android UI server's Java, the page's JavaScript, the dump fallback's Rust), and the commands every script leans on
//! reaching each driver alike. The device is the argv-recording `adb`, the fake UI server and the mock browser daemon.
use crate::android::{sandbox, FakeUiServer};
use crate::support::{node_path, MockDaemon, RunResult, SlotSandbox};
use serde_json::{json, Value};
use std::process::{Command, Stdio};

const PATTERNS: [&str; 9] = ["Continue", ".*Continue.*", "Run (local)", "Hello World", "\\d+", "a|ab", "a.b", "[^0-9]+", "a\\sb"];
const VALUES: [&str; 10] = ["Continue", "continue later", "Tap Continue now", "Run (local)", "run local", "Hello\nWorld", "42", "௪௨", "ab", "a\nb"];

/// Maestro's rule, pattern by value: case-insensitive, a full match or literal equality, a newline read as a space, a dot
/// across lines, ASCII digits.
const EXPECTED: [[u8; 10]; 9] = [
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 1, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 1, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
    [1, 1, 1, 1, 1, 1, 0, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
];

/// UiSelector.textMatches as the UI server runs it: the pattern compiled with DOTALL, then `matches()` on the whole value.
const MATCH_JAVA: &str = r#"import java.io.*; import java.util.Base64; import java.util.regex.Pattern;
public class Match { public static void main(String[] a) throws Exception {
  BufferedReader in = new BufferedReader(new InputStreamReader(System.in, "UTF-8")); Base64.Decoder d = Base64.getDecoder(); String l;
  while ((l = in.readLine()) != null) { String[] f = l.split(" ", -1);
    System.out.println(Pattern.compile(new String(d.decode(f[0]), "UTF-8"), Pattern.DOTALL).matcher(new String(d.decode(f[1]), "UTF-8")).matches() ? 1 : 0); } } }"#;

fn out(r: &RunResult) -> String {
    format!("{}{}", r.stdout, r.stderr)
}

fn logged(r: &RunResult) -> Value {
    let line = r.stdout.lines().find(|l| l.starts_with('{') || l.starts_with('[')).unwrap_or_else(|| panic!("no JSON line: {}", out(r)));
    serde_json::from_str(line).unwrap()
}

fn xml_text(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;").replace('\n', "&#10;")
}

/// A Lua script over the patterns: `body` runs once per pattern `p` (index `i`).
fn per_pattern(body: &str) -> String {
    let list: Vec<String> = PATTERNS.iter().map(|p| format!("[==[{p}]==]")).collect();
    format!("local P = {{ {} }}\nlocal out = {{}}\nfor i, p in ipairs(P) do\n{body}\nend\nlog(json.encode(out))", list.join(", "))
}

/// The mock daemon registered as the slot's browser, answering every page lookup with nothing found.
fn browser_daemon(s: &SlotSandbox) -> MockDaemon {
    let d = MockDaemon::start();
    s.seed(&json!({ "main/mla-tamagui/default": { "createdAt": "2026-01-01T00:00:00Z", "browserMcp": { "url": d.url, "startedAt": "2026-01-01T00:00:00Z" } } }));
    d
}

fn b64(s: &str) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, s.as_bytes())
}

#[test]
fn a_selector_pattern_reads_alike_on_the_java_javascript_and_rust_engines() {
    // Given a screen whose nodes v1..v10 hold the values, as the dump fallback reads it
    let s = sandbox("wf-flow-maestro-regex-");
    let nodes: String = VALUES.iter().enumerate().map(|(i, v)| format!("<node index=\"{i}\" text=\"{}\" resource-id=\"v{}\" class=\"android.widget.TextView\" bounds=\"[0,{}][400,{}]\" />", xml_text(v), i + 1, i * 50, i * 50 + 40)).collect();
    let xml = s.file("ui.xml", format!("<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation=\"0\">{nodes}</hierarchy>"));

    // and a screen whose nodes carry the values as their resource ids, each after the package's prefix
    let id_nodes: String = VALUES.iter().enumerate().map(|(i, v)| format!("<node index=\"{i}\" text=\"t{}\" resource-id=\"com.app:id/{}\" class=\"android.widget.TextView\" bounds=\"[0,{}][400,{}]\" />", i + 1, xml_text(v), i * 50, i * 50 + 40)).collect();
    let id_xml = s.file("ids.xml", format!("<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation=\"0\">{id_nodes}</hierarchy>"));
    let script = |name: &str, lua: &str| s.file(name, lua).to_string();

    // When a script asks for each value by its node and each pattern, through the Rust engine of the dump fallback, as text
    // and as a resource id
    let rust_lua = script("rust.lua", &per_pattern(&format!("  local row = {{}}\n  for j = 1, {} do row[j] = device:get_element({{id='v' .. j, text=p}}) and 1 or 0 end\n  out[i] = row", VALUES.len())));
    let rust = s.run(&["android", "test", &rust_lua, "--no-record"], &[("STUB_UI_XML", xml.as_str()), ("WF_ANDROID_UIA", "dump")]);
    let rust_id_lua = script("rust-id.lua", &per_pattern(&format!("  local row = {{}}\n  for j = 1, {} do row[j] = device:get_element({{id=p, text='t' .. j}}) and 1 or 0 end\n  out[i] = row", VALUES.len())));
    let rust_id = s.run(&["android", "test", &rust_id_lua, "--no-record"], &[("STUB_UI_XML", id_xml.as_str()), ("WF_ANDROID_UIA", "dump")]);
    // and once per pattern through the UI server, whose selectors carry the Java form, and through the browser's page finder
    let tmp = s.repo.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let marker = tmp.join("u2-started");
    let server = FakeUiServer::start(Some(marker.clone()), false);
    let (port, mark, tmp_dir) = (server.port.to_string(), marker.display().to_string(), tmp.display().to_string());
    let look = script("look.lua", &per_pattern("  out[i] = { device:get_element({text=p}) ~= nil, device:get_element({id=p}) ~= nil }"));
    let java_run = s.run(&["android", "test", &look, "--no-record"], &[("STUB_U2_PORT", port.as_str()), ("STUB_U2_MARK", mark.as_str()), ("TMPDIR", tmp_dir.as_str())]);
    let d = browser_daemon(&s);
    let js_run = s.run(&["browser", "test", &look, "--no-record"], &[("WF_BROWSER_NO_START", "1")]);
    assert_eq!((rust.exit_code, rust_id.exit_code, java_run.exit_code, js_run.exit_code), (0, 0, 0, 0), "{}{}{}{}", out(&rust), out(&rust_id), out(&java_run), out(&js_run));

    // Then the Rust engine finds each value exactly where Maestro's rule says, as text and as the id after its prefix
    let expected: Vec<Vec<u8>> = EXPECTED.iter().map(|r| r.to_vec()).collect();
    assert_eq!(serde_json::from_value::<Vec<Vec<u8>>>(logged(&rust)).unwrap(), expected, "rust");
    assert_eq!(serde_json::from_value::<Vec<Vec<u8>>>(logged(&rust_id)).unwrap(), expected, "rust, ids");

    // and the JavaScript form each lookup sent the page, by text and by test id, answers the same through node's RegExp
    let specs: Vec<Value> = d
        .calls()
        .into_iter()
        .filter(|(m, a)| m == "outerEval" && a["expression"].as_str().is_some_and(|e| e.ends_with("return r.hit; })()")))
        .map(|(_, a)| {
            let e = a["expression"].as_str().unwrap().to_string();
            serde_json::from_str(&e[e.rfind("})({").unwrap() + 3..e.rfind("); return r.hit").unwrap()]).unwrap()
        })
        .collect();
    let node = node_path().expect("node on PATH");
    for field in ["text", "id"] {
        let forms: Vec<Value> = specs.iter().map(|spec| spec[field].clone()).filter(|f| !f.is_null()).collect();
        assert_eq!(forms.len(), PATTERNS.len(), "{field}: {specs:?}");
        let js = Command::new(&node)
            .args(["-e", "const [cases, values] = JSON.parse(process.argv[1]); console.log(JSON.stringify(cases.map(([src, flags]) => values.map((v) => (new RegExp(src, flags).test(v) ? 1 : 0)))))", &json!([forms, VALUES]).to_string()])
            .env_remove("FORCE_COLOR")
            .output()
            .unwrap();
        assert_eq!(serde_json::from_slice::<Vec<Vec<u8>>>(&js.stdout).unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&js.stderr))), expected, "javascript, {field}");
    }

    // and the Java form each UI server selector carried answers the same as UiSelector's textMatches, and as its
    // resourceIdMatches on the bare id and on the id after the package's prefix (CI has a JDK; elsewhere it may be skipped)
    let asked = server.asked();
    let forms = |key: &str| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in asked.iter().filter_map(|sel| sel[key].as_str()) {
            if !out.iter().any(|o| o == f) {
                out.push(f.to_string());
            }
        }
        out
    };
    let (text_forms, id_forms) = (forms("textMatches"), forms("resourceIdMatches"));
    assert_eq!((text_forms.len(), id_forms.len()), (PATTERNS.len(), PATTERNS.len()), "{text_forms:?} {id_forms:?}");
    match which("java") {
        Some(java) => {
            let src = tmp.join("Match.java");
            std::fs::write(&src, MATCH_JAVA).unwrap();
            let cases = |forms: &[String], prefix: &str| -> String { forms.iter().flat_map(|p| VALUES.iter().map(move |v| format!("{} {}\n", b64(p), b64(&format!("{prefix}{v}"))))).collect() };
            let lines = [cases(&text_forms, ""), cases(&id_forms, ""), cases(&id_forms, "com.app:id/")].concat();
            let mut child = Command::new(java).arg(&src).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
            std::io::Write::write_all(&mut child.stdin.take().unwrap(), lines.as_bytes()).unwrap();
            let o = child.wait_with_output().unwrap();
            let flat: Vec<u8> = String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect();
            let rows: Vec<Vec<u8>> = flat.chunks(VALUES.len()).map(<[u8]>::to_vec).collect();
            assert_eq!(rows.len(), 3 * PATTERNS.len(), "java: {}", String::from_utf8_lossy(&o.stderr));
            for (part, name) in rows.chunks(PATTERNS.len()).zip(["textMatches", "resourceIdMatches, bare", "resourceIdMatches, prefixed"]) {
                assert_eq!(part, expected.as_slice(), "java {name}: {}", String::from_utf8_lossy(&o.stderr));
            }
        }
        None if std::env::var_os("CI").is_some() => panic!("no java on PATH: CI runs the Java engine's check"),
        None => eprintln!("no java on PATH: the Java engine's check is skipped"),
    }

    // Given a pattern Rust's regex accepts and JavaScript refuses (an inline flag)
    let bad = "(?i)run";
    assert!(regex::Regex::new(bad).is_ok());
    let refused_js = Command::new(&node).args(["-e", &format!("try {{ new RegExp({}, 'imsu'); console.log('accepted') }} catch (e) {{ console.log('refused') }}", json!(bad))]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&refused_js.stdout).trim(), "refused");
    // When it is given to each platform's CLI and to a script
    d.clear();
    server.asked();
    s.forget("adb");
    let browser = s.run(&["browser", "get-element", "--text", bad, "--device", "t"], &[("WF_BROWSER_NO_START", "1")]);
    let android = s.run(&["android", "get-element", "--text", bad], &[("WF_ANDROID_UIA", "dump"), ("STUB_UI_XML", xml.as_str())]);
    let lua = script("bad.lua", &format!("log('before')\ndevice:get_element({{text={}}})\nlog('after')", json!(bad)));
    let scripted = s.run(&["android", "test", &lua, "--no-record"], &[("WF_ANDROID_UIA", "dump"), ("STUB_UI_XML", xml.as_str())]);
    // Then each refuses it as outside the shared grammar before anything reaches a page or the device
    for (r, platform) in [(&browser, "browser"), (&android, "android")] {
        let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", out(r)));
        assert!(r.exit_code == 2 && v["error"].as_str().unwrap().contains("pattern '(?i)run': an inline flag such as (?i) is outside the regex grammar"), "{platform}: {v}");
    }
    assert!(!d.calls().iter().any(|(m, _)| m == "outerEval") && server.asked().is_empty(), "{:?}", d.calls());
    assert!(!s.calls("adb").iter().any(|c| c.contains("uiautomator")), "{:?}", s.calls("adb"));
    assert!(scripted.exit_code == 1 && scripted.stderr.contains("bad.lua:2") && scripted.stderr.contains("inline flag") && !scripted.stdout.contains("after"), "{}", out(&scripted));
}

#[test]
fn nested_matches_an_element_without_text_and_a_match_count_read_on_android_as_on_the_browser_finder() {
    // Given a screen where ChainA holds ChainB (which holds ChainC) and ChainD, a box with no text or label, and a node whose
    // text and label both read Twice, as the dump fallback and the UI server see it (browser.rs holds the page finder's twin)
    let s = sandbox("wf-flow-maestro-nested-");
    let xml = s.file("ui.xml", "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation=\"0\"><node index=\"0\" text=\"\" resource-id=\"ChainA\" class=\"android.view.ViewGroup\" bounds=\"[0,200][400,600]\"><node index=\"0\" text=\"\" resource-id=\"ChainB\" class=\"android.view.ViewGroup\" bounds=\"[0,200][400,400]\"><node index=\"0\" text=\"c\" resource-id=\"ChainC\" class=\"android.widget.TextView\" bounds=\"[0,200][200,300]\" /></node><node index=\"1\" text=\"d\" resource-id=\"ChainD\" class=\"android.widget.TextView\" bounds=\"[0,400][400,600]\" /></node><node index=\"1\" text=\"\" resource-id=\"Plain\" class=\"android.view.View\" bounds=\"[0,700][40,710]\" /><node index=\"2\" text=\"Twice\" content-desc=\"Twice\" resource-id=\"\" class=\"android.widget.TextView\" bounds=\"[0,800][100,840]\" /></hierarchy>");
    let lua = s.file("t.lua", "log(json.encode({ nested = device:get_element({id='Chain.*'}).text, plain = device:get_element({id='Plain'}).text == nil }))");
    let tmp = s.repo.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let marker = tmp.join("u2-started");
    let server = FakeUiServer::start(Some(marker.clone()), false);
    let (port, mark, tmp_dir) = (server.port.to_string(), marker.display().to_string(), tmp.display().to_string());
    let dump = [("STUB_UI_XML", xml.as_str()), ("WF_ANDROID_UIA", "dump")];
    let u2 = [("STUB_U2_PORT", port.as_str()), ("STUB_U2_MARK", mark.as_str()), ("TMPDIR", tmp_dir.as_str())];

    // When each path looks up the chain and the box, and a wait for Twice to go runs out
    let mut seen = Vec::new();
    for env in [&dump[..], &u2[..]] {
        let run = s.run(&["android", "test", &lua, "--no-record"], env);
        assert_eq!(run.exit_code, 0, "{}", out(&run));
        let stayed = s.run(&["android", "extended-wait-until", "--not-visible", "--text", "Twice", "--timeout", "0"], env);
        let failure: Value = serde_json::from_str(stayed.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", out(&stayed)));
        seen.push((logged(&run), stayed.exit_code, failure["evidence"]["matches"].clone()));
    }

    // Then both pick ChainC, the first match with no match inside it (not ChainD, the last one inside the first), read the box's
    // text as nil, and count the node that matches by its text and by its label once
    let want = (json!({ "nested": "c", "plain": true }), 1, json!(1));
    assert_eq!(seen, vec![want.clone(), want], "dump, then the UI server");
}

/// An envelope with its timings and file paths left out: what the same command on the same screen must answer alike.
fn shape(v: &Value) -> Value {
    let mut v = v.clone();
    if let Some(m) = v.as_object_mut() {
        m.retain(|k, _| !matches!(k.as_str(), "ms" | "platform" | "device"));
        if let Some(r) = m.get_mut("result").and_then(Value::as_object_mut) {
            r.remove("ms");
        }
        if let Some(e) = m.get_mut("evidence").and_then(Value::as_object_mut) {
            for (k, v) in e.iter_mut() {
                if matches!(k.as_str(), "screenshot" | "hierarchy" | "path") {
                    *v = json!(v.is_string());
                }
            }
        }
    }
    v
}

#[test]
fn the_shared_verbs_answer_alike_from_both_drivers_on_the_same_screen() {
    // Given one screen as each driver reads it: Go at 10,20-110,60 with the keyboard down, from a uiautomator dump on Android
    // and from the mock daemon's page on the browser
    let s = sandbox("wf-flow-maestro-parity-");
    let xml = s.file("ui.xml", "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation=\"0\"><node index=\"0\" text=\"Go\" resource-id=\"Go\" class=\"android.widget.Button\" bounds=\"[10,20][110,60]\" /></hierarchy>");
    let d = browser_daemon(&s);
    let shot = s.file("shot.png", "PNGDATA");
    let go = json!({ "bounds": { "left": 10, "top": 20, "right": 110, "bottom": 60 }, "full": { "left": 10, "top": 20, "right": 110, "bottom": 60 }, "text": "Go", "frame": "app", "path": ":root" });
    d.rule("outerEval", "Nope", vec![json!({ "result": "null" })]);
    d.rule("outerEval", "return r.hit", vec![json!({ "result": go.to_string() })]);
    d.rule("outerEval", "return r.count", vec![json!({ "result": "1" })]);
    d.rule("outerEval", ").tree", vec![json!({ "result": json!([{ "id": "Go", "text": "Go" }]).to_string() })]);
    d.rule("shellEval", "getStatus", vec![json!({ "result": json!({ "visible": false, "bounds": null }).to_string() })]);
    d.rule("screenshot", "", vec![json!({ "path": shot })]);
    d.rule("consoleLogs", "", vec![json!({ "count": 0, "lines": [] })]);
    let cases: [&[&str]; 8] = [
        &["get-element", "--id", "Go"],
        &["get-element", "--id", "Nope"],
        &["extended-wait-until", "--visible", "--id", "Go", "--timeout", "0"],
        &["assert-not-visible", "--id", "Go", "--timeout", "0"],
        &["press-key", "Enter"],
        &["input-text", "x"],
        &["get-device-state", "keyboard"],
        &["tap-on", "--point", "10,20"],
    ];

    // When each shared verb runs on both CLIs
    for args in cases {
        let android = s.run(&[&["android"], args].concat(), &[("STUB_UI_XML", xml.as_str()), ("WF_ANDROID_UIA", "dump"), ("STUB_IME", "false")]);
        let browser = s.run(&[&["browser"], args, &["--device", "t"]].concat(), &[("WF_BROWSER_NO_START", "1")]);
        let (a, b): (Value, Value) = (serde_json::from_str(android.stdout.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {}", out(&android))), serde_json::from_str(browser.stdout.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {}", out(&browser))));
        // Then each answers with the same exit code and the same envelope: the same result, or the same error, verdict, next
        // step and evidence fields
        assert_eq!((android.exit_code, shape(&a)), (browser.exit_code, shape(&b)), "{args:?}\nandroid: {a}\nbrowser: {b}");
    }
}

fn which(tool: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(tool)).find(|p| p.is_file())
}

#[test]
fn a_swipe_a_clipped_tap_and_the_editors_buffer_reach_both_drivers_alike() {
    // Given an Android screen with the code editor's web view and a 480x1040 display, and a browser page whose button is clipped
    // by its scrolling parent to its left 40 px
    let s = sandbox("wf-flow-maestro-commands-");
    let xml = s.file("ui.xml", "<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation=\"0\"><node index=\"0\" text=\"\" resource-id=\"CodeEditorInput\" class=\"android.webkit.WebView\" bounds=\"[0,300][480,900]\"><node index=\"0\" text=\"def f():&#10;    pass\" resource-id=\"\" class=\"android.widget.EditText\" bounds=\"[0,300][480,900]\" /></node></hierarchy>");
    let d = browser_daemon(&s);
    let clipped = json!({ "bounds": { "left": 80, "top": 0, "right": 120, "bottom": 40 }, "full": { "left": 80, "top": 0, "right": 180, "bottom": 40 }, "text": "Clip", "frame": "app", "path": ":root > body:nth-child(2) > button:nth-child(1)" });
    let below = |top: i64| json!({ "result": json!({ "bounds": { "left": 0, "top": top, "right": 100, "bottom": (top + 100).min(852) }, "full": { "left": 0, "top": top, "right": 100, "bottom": top + 100 }, "text": "Below", "frame": "app", "path": ":root" }).to_string() });
    d.rule("outerEval", "Below", vec![below(800), below(700)]);
    d.rule("outerEval", "return r.hit", vec![json!({ "result": clipped.to_string() })]);
    d.rule("waitObserve", "setTimeout(look,100)", vec![json!({ "result": json!({ "hit": clipped }).to_string() })]);
    d.rule("outerEval", "dpr: devicePixelRatio", vec![json!({ "result": json!({ "width": 393, "height": 852, "dpr": 3 }).to_string() })]);
    let lua = s.file("t.lua", "device:swipe('200,600', '200,200', {duration=600})\ndevice:swipe('50%,90%', '50%,10%')\nlocal r = {}\nif device.platform == 'android' then r.editor = device:get_element({id='CodeEditorInput'}).text else r.tap = device:tap_on({id='Clipped'}); r.scrolled = device:scroll_until_visible{element={id='Below'}} end\nlog(json.encode(r))");

    // When the script swipes by points and by percentages, and reads the editor on Android and taps the clipped button on the browser
    let android = s.run(&["android", "test", &lua, "--no-record"], &[("STUB_UI_XML", xml.as_str()), ("WF_ANDROID_UIA", "dump")]);
    let browser = s.run(&["browser", "test", &lua, "--no-record"], &[("WF_BROWSER_NO_START", "1")]);
    assert_eq!((android.exit_code, browser.exit_code), (0, 0), "{}{}", out(&android), out(&browser));

    // Then Android swipes with input swipe, a percentage taken of its display, the default 400 ms where none was given
    let swipes: Vec<String> = s.calls("adb").into_iter().filter(|c| c.contains("input swipe")).collect();
    assert_eq!(swipes, ["-s emulator-5554 shell input swipe 200 600 200 200 600", "-s emulator-5554 shell input swipe 240 936 240 104 400"]);
    // and the editor's web view reads as its buffer
    assert_eq!(logged(&android)["editor"], "def f():\n    pass");
    // and the browser swipes the same strokes over the screen in its tap units, plus one scroll for the element half below
    // the fold, which counts as shown only once all of it is
    let strokes: Vec<Vec<f64>> = d.calls().into_iter().filter(|(m, a)| m == "swipe" && a["screen"] == true).map(|(_, a)| ["x", "y"].iter().map(|k| a["from"][*k].as_f64().unwrap()).chain(["dx", "dy", "durationMs"].iter().map(|k| a[*k].as_f64().unwrap())).collect()).collect();
    let near = |got: &[f64], want: [f64; 5]| got.len() == 5 && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6);
    assert!(strokes.len() == 3 && near(&strokes[0], [200.0, 600.0, 0.0, -400.0, 600.0]) && near(&strokes[1], [196.5, 766.8, 0.0, -681.6, 400.0]), "{strokes:?}");
    assert!(near(&strokes[2], [196.5, 426.0, 0.0, -340.8, 600.0]), "a drag from the screen's centre to 10% inside its top: {strokes:?}");
    assert_eq!((logged(&browser)["scrolled"]["swipes"].clone(), logged(&browser)["scrolled"]["bounds"]["top"].clone()), (json!(1), json!(700)));
    // and the tap lands inside the button's on-screen part, never in the clipped-off rest of its box
    let tap = d.calls().into_iter().find(|(m, _)| m == "tap").map(|(_, a)| a).expect("a tap");
    let (x, y) = (tap["x"].as_f64().unwrap(), tap["y"].as_f64().unwrap());
    assert!((88.0..=112.0).contains(&x) && (8.0..=32.0).contains(&y), "{tap}");
    assert_eq!(logged(&browser)["tap"]["bounds"], json!({ "left": 80, "top": 0, "right": 120, "bottom": 40, "cx": 100, "cy": 20 }));
}

#[test]
fn at_speed_two_a_user_swipes_and_types_twice_as_fast_on_both_drivers_and_a_drag_stays_a_drag() {
    // Given a sideways row whose End key shows only after a swipe: on Android a 480 px row at y 600-660 in the dump, on the
    // browser a 200 tap-unit row at y 300-340 whose key the page finds on the second look; and a keyboard that is up
    let s = sandbox("wf-flow-maestro-speed-");
    let screen = |nodes: &str| format!("<?xml version='1.0' encoding='UTF-8' standalone='yes' ?><hierarchy rotation=\"0\">{nodes}</hierarchy>");
    let row = "<node index=\"0\" text=\"\" resource-id=\"SpeedRow\" class=\"android.widget.HorizontalScrollView\" bounds=\"[0,600][480,660]\" />";
    let before = s.file("before.xml", screen(row));
    let after = s.file("after.xml", screen(&format!("{row}<node index=\"1\" text=\"End\" resource-id=\"SpeedEnd\" class=\"android.widget.Button\" bounds=\"[300,610][380,650]\" />")));
    let d = browser_daemon(&s);
    let hit = |id: &str, (l, t, r, b): (i64, i64, i64, i64)| json!({ "bounds": { "left": l, "top": t, "right": r, "bottom": b }, "full": { "left": l, "top": t, "right": r, "bottom": b }, "text": id, "frame": "app", "path": ":root" });
    let row = hit("SpeedRow", (0, 300, 200, 340));
    let look = |end: Value, row: Value| json!({ "result": json!([end, row]).to_string() });
    d.rule("outerEval", "SpeedEnd", vec![look(Value::Null, row), look(hit("SpeedEnd", (120, 305, 160, 335)), json!(false))]);
    d.rule("outerEval", "dpr: devicePixelRatio", vec![json!({ "result": json!({ "width": 393, "height": 852, "dpr": 3 }).to_string() })]);
    d.rule("shellEval", "getStatus", vec![json!({ "result": json!({ "visible": true, "bounds": { "left": 0, "top": 582, "right": 393, "bottom": 852 } }).to_string() })]);
    let lua = s.file("t.lua", "local r = device:scroll_until_visible{element={id='SpeedEnd'}, direction='RIGHT', from={id='SpeedRow'}}\ndevice:swipe('10,20', '10,200')\ndevice:swipe('10,20', '10,200', {duration=600})\ndevice:input_text('ab')\nlog(json.encode({ swipes = r.swipes }))");

    // When the script scrolls the row, swipes with the default and with 600 ms, and types, at WF_TEST_SPEED=2
    let android = s.run(&["android", "test", &lua, "--no-record"], &[("STUB_UI_XML", before.as_str()), ("STUB_UI_XML_AFTER", after.as_str()), ("WF_ANDROID_UIA", "dump"), ("WF_TEST_SPEED", "2")]);
    let browser = s.run(&["browser", "test", &lua, "--no-record"], &[("WF_BROWSER_NO_START", "1"), ("WF_TEST_SPEED", "2")]);
    assert_eq!((android.exit_code, browser.exit_code), (0, 0), "{}{}", out(&android), out(&browser));
    assert_eq!((logged(&android)["swipes"].clone(), logged(&browser)["swipes"].clone()), (json!(1), json!(1)));

    // Then Android's scroll crosses the row from 90% to 10% of its width (432 to 48 of 480) in 300 ms, not 600, the swipe
    // verb's default 400 ms becomes the 250 ms floor while the script's 600 stays 600, and adb types the text as before
    let swipes: Vec<String> = s.calls("adb").into_iter().filter(|c| c.contains("input swipe")).collect();
    assert_eq!(swipes, ["-s emulator-5554 shell input swipe 432 630 48 630 300", "-s emulator-5554 shell input swipe 10 20 10 200 250", "-s emulator-5554 shell input swipe 10 20 10 200 600"]);
    assert!(s.calls("adb").iter().any(|c| c.ends_with("shell input text 'ab'")), "{:?}", s.calls("adb"));
    // and the browser's scroll does the same over its row (180 to 20 of 200, so dx -160) in 300 ms, with the same swipe
    // durations, and its type call carries speed 2 for the daemon to shorten the pauses between keys
    let strokes: Vec<Vec<f64>> = d.calls().into_iter().filter(|(m, a)| m == "swipe" && a["screen"] == true).map(|(_, a)| ["x", "y"].iter().map(|k| a["from"][*k].as_f64().unwrap()).chain(["dx", "dy", "durationMs"].iter().map(|k| a[*k].as_f64().unwrap())).collect()).collect();
    assert_eq!(strokes, [vec![180.0, 320.0, -160.0, 0.0, 300.0], vec![10.0, 20.0, 0.0, 180.0, 250.0], vec![10.0, 20.0, 0.0, 180.0, 600.0]]);
    // and each of its looks reads the key, and the row it swipes while the key is not all on screen, in one page evaluation:
    // two looks for one swipe
    let looks: Vec<String> = d.calls().into_iter().filter(|(m, a)| m == "outerEval" && a.to_string().contains("Speed")).map(|(_, a)| a.to_string()).collect();
    assert!(looks.len() == 2 && looks.iter().all(|l| l.contains("SpeedEnd") && l.contains("SpeedRow")), "{looks:?}");
    let typed: Vec<Value> = d.calls().into_iter().filter(|(m, _)| m == "type").map(|(_, a)| a).collect();
    assert!(typed.len() == 1 && typed[0]["speed"].as_f64() == Some(2.0) && typed[0]["text"] == "ab" && typed[0]["human"] == true, "{typed:?}");
}

#[test]
fn a_scroll_whose_combined_look_meets_a_reload_looks_up_its_row_on_its_own() {
    // Given a browser page that is between documents at the first look for a key and its row, then shows the row, and the key
    // once the row was swiped
    let s = sandbox("wf-flow-maestro-reload-");
    let d = browser_daemon(&s);
    let hit = |id: &str, (l, t, r, b): (i64, i64, i64, i64)| json!({ "bounds": { "left": l, "top": t, "right": r, "bottom": b }, "full": { "left": l, "top": t, "right": r, "bottom": b }, "text": id, "frame": "app", "path": ":root" });
    let both = |end: Value, row: Value| json!({ "result": json!([end, row]).to_string() });
    d.rule("outerEval", "ReloadEnd", vec![json!({ "error": "Execution context was destroyed." }), both(hit("ReloadEnd", (120, 305, 160, 335)), json!(false))]);
    d.rule("outerEval", "ReloadRow", vec![json!({ "result": hit("ReloadRow", (0, 300, 200, 340)).to_string() })]);
    d.rule("outerEval", "dpr: devicePixelRatio", vec![json!({ "result": json!({ "width": 393, "height": 852, "dpr": 3 }).to_string() })]);
    let lua = s.file("t.lua", "local r = device:scroll_until_visible{element={id='ReloadEnd'}, direction='RIGHT', from={id='ReloadRow'}}\nlog(json.encode({ swipes = r.swipes }))");

    // When the script scrolls the row until the key shows
    let browser = s.run(&["browser", "test", &lua, "--no-record"], &[("WF_BROWSER_NO_START", "1")]);

    // Then the row the combined look could not read is looked up by a look of its own, swiped once, and the key is found
    assert_eq!((browser.exit_code, logged(&browser)["swipes"].clone()), (0, json!(1)), "{}", out(&browser));
    let looks: Vec<String> = d.calls().into_iter().filter(|(m, a)| m == "outerEval" && a.to_string().contains("Reload")).map(|(_, a)| if a.to_string().contains("ReloadEnd") { "both" } else { "row" }.to_string()).collect();
    assert_eq!(looks, ["both", "row", "both"]);
    assert_eq!(d.calls().into_iter().filter(|(m, a)| m == "swipe" && a["screen"] == true).count(), 1);
}

#[test]
fn a_speed_outside_one_to_four_stops_the_test_before_it_starts_and_the_report_records_the_speed_used() {
    // Given a script that logs, and the mock daemon as the browser
    let s = sandbox("wf-flow-maestro-speed-range-");
    let d = browser_daemon(&s);
    let lua = s.file("t.lua", "log('ran')");

    // When a test starts with WF_TEST_SPEED=9 or abc, or --speed 0.5, on either harness
    let mut refused = Vec::new();
    for platform in ["browser", "android"] {
        for (args, env) in [(vec![], "9"), (vec![], "abc"), (vec!["--speed", "0.5"], "")] {
            let argv = [&[platform, "test", lua.as_str(), "--no-record"], &args[..]].concat();
            let r = s.run(&argv, &[("WF_BROWSER_NO_START", "1"), ("WF_TEST_SPEED", env)]);
            refused.push((platform, env, r));
        }
    }
    // Then each exits 2 with a usage error naming the allowed range as its only output (the script never logged), and the
    // browser was never asked anything
    for (platform, env, r) in &refused {
        let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{platform} {env}: {e}: {}", out(r)));
        let error = v["error"].as_str().unwrap_or_default();
        assert!(r.exit_code == 2 && error.contains("a number from 1 to 4") && v["usage"].as_str().is_some_and(|u| u.contains("[--speed N]")), "{platform} {env}: {v}");
        assert!(error.contains(if env.is_empty() { "--speed" } else { "WF_TEST_SPEED" }), "{platform} {env}: {v}");
    }
    assert!(!d.calls().iter().any(|(m, _)| m == "outerEval" || m == "type"), "{:?}", d.calls());
    // and the synopsis `test --api` prints names the option on both harnesses
    for platform in ["browser", "android"] {
        let api = s.run(&[platform, "test", "--api"], &[]);
        assert!(api.exit_code == 0 && api.stdout.lines().take(2).any(|l| l.contains("[--speed N]")), "{platform}: {}", out(&api));
    }

    // When a test runs with --speed 3, and one runs with neither
    let fast = s.run(&["browser", "test", &lua, "--no-record", "--speed", "3", "--test-output-dir", "fast"], &[("WF_BROWSER_NO_START", "1")]);
    let xml = s.repo.join("ui.xml").display().to_string();
    let plain = s.run(&["android", "test", &lua, "--no-record", "--test-output-dir", "plain"], &[("STUB_UI_XML", xml.as_str()), ("WF_ANDROID_UIA", "dump")]);
    // Then each report.json records the speed it ran at, and the page shows it
    assert_eq!((fast.exit_code, plain.exit_code), (0, 0), "{}{}", out(&fast), out(&plain));
    let report = |dir: &str| -> Value { serde_json::from_str(&std::fs::read_to_string(s.repo.join(dir).join("report.json")).unwrap()).unwrap() };
    assert_eq!((report("fast")["speed"].as_f64(), report("plain")["speed"].as_f64()), (Some(3.0), Some(1.0)));
    assert!(std::fs::read_to_string(s.repo.join("fast/report.html")).unwrap().contains("<dt>speed</dt><dd>3.0</dd>"));
}
