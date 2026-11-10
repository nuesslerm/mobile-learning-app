//! `wf bench` measuring the app in a real, phone-framed Chromium. Integration tier: the flow tests (`flow/bench.rs`) double the browser.
use crate::lab::{free_port, median, readings, Lab, PAGE, WEB_SCRIPT};
use crate::support::RunResult;
use wf_harness::testing::require_browser;
use regex::Regex;
use serde_json::{json, Value};

fn leaky_page(leaky: bool) -> String {
    format!(
        "<!doctype html><html><body><div id=\"root\"></div><script>
  window.__kept = [];
  function render() {{
    var path = location.pathname.replace('/mla-tamagui', '');
    document.getElementById('root').textContent = 'route ' + path;
    if ({leaky} && path === '/b') {{
      for (var i = 0; i < 4000; i++) window.__kept.push({{ i: i, s: 'leak-' + i }});
      setInterval(function () {{ window.__kept.length; }}, 1000);
    }}
  }}
  addEventListener('popstate', render);
  render();
</script></body></html>"
    )
}

#[test]
fn repo_bench_measures_memory_runtime_and_startup_of_the_app_in_a_phone_framed_chromium() {
    require_browser();
    let lab = Lab::new();
    let export = lab.tmp.join("export");
    std::fs::create_dir_all(&export).unwrap();

    // Given an export with a route that adds nodes and a listener, When the memory suite runs, Then readings, shell offsets and loop growth are reported
    std::fs::write(export.join("index.html"), PAGE).unwrap();
    lab.scenario(
        "memory",
        json!({ "device": "iphone-15-pro", "port": free_port(), "baseUrl": "/mla-tamagui", "settleMs": 100, "start": "/", "routes": [{ "id": "a", "path": "/a" }, { "id": "b", "path": "/b" }], "loop": { "a": "/a", "b": "/b", "cycles": 2, "warmup": 1 }, "shellProbe": "data:text/html,<title>shell</title><p>no app</p>" }),
        1,
    );
    let r = lab.bench(&["memory", "--runs", "1"], &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let m = readings(&r.stdout);
    for name in ["coldStartHeapBytes", "harnessShellHeapBytes", "coldStartAppHeapBytes", "coldStartDomNodes", "coldStartListeners", "route.a.heapBytes", "route.a.appHeapBytes", "route.a.domNodes", "route.b.heapBytes", "route.b.appHeapBytes", "route.b.domNodes", "loopGrowthBytes", "loopGrowthDomNodes", "loopGrowthListeners", "loopSteadyGrowthBytes"] {
        assert!(m.contains_key(name), "{name}");
    }
    assert!(median(&m, "coldStartHeapBytes") > 100_000.0);
    assert_eq!(median(&m, "coldStartHeapBytes") - median(&m, "harnessShellHeapBytes"), median(&m, "coldStartAppHeapBytes"));
    assert!((200.0..215.0).contains(&median(&m, "loopGrowthDomNodes")) && median(&m, "loopGrowthListeners") == 2.0);
    assert!(median(&m, "route.b.domNodes") - median(&m, "coldStartDomNodes") >= 100.0);

    // Given a clean baseline and then a route that leaks on every visit, Then loop growth is large and the run fails
    let leak_lab = Lab::new();
    let leak_export = leak_lab.tmp.join("export");
    std::fs::create_dir_all(&leak_export).unwrap();
    let params = json!({ "device": "iphone-15-pro", "port": free_port(), "baseUrl": "/mla-tamagui", "settleMs": 200, "start": "/a", "routes": [{ "id": "b", "path": "/b" }], "loop": { "a": "/a", "b": "/b", "cycles": 5, "warmup": 1 } });
    std::fs::write(leak_lab.data.join("scenarios/memory.json"), json!({ "suite": "memory", "description": "planted leak", "runs": 1, "metrics": { "loopGrowthBytes": { "better": "lower", "tolerance": 0.5, "scope": "lab-only" } }, "params": params }).to_string()).unwrap();
    let pages = |leaky: bool| {
        for file in ["index.html", "a.html", "b.html"] {
            std::fs::write(leak_export.join(file), leaky_page(leaky)).unwrap();
        }
    };
    pages(false);
    let clean = leak_lab.bench(&["memory", "--runs", "1", "--record", "initial"], &[]);
    pages(true);
    let leaky = leak_lab.bench(&["memory", "--runs", "1"], &[]);
    assert_eq!(clean.exit_code, 0, "{}", clean.stderr);
    let growth = |r: &RunResult| readings(&r.stdout).get("loopGrowthBytes").map_or(f64::NAN, |l| l.median);
    assert!(growth(&leaky) > growth(&clean) + 300_000.0, "{} vs {}", growth(&leaky), growth(&clean));
    assert_eq!(leaky.exit_code, 1);
    assert!(Regex::new(r"(?m)^FAIL memory \(result vs current baseline\)$").unwrap().is_match(&leaky.stdout) && Regex::new(r"regressed\s+loopGrowthBytes").unwrap().is_match(&leaky.stdout), "{}", leaky.stdout);

    // Given a page with static test ids, When the runtime suite runs, Then tap, variant and mount timings and the long tasks are reported
    std::fs::write(export.join("tap.html"), PAGE).unwrap();
    lab.scenario(
        "runtime",
        json!({ "device": "iphone-15-pro", "port": free_port(), "baseUrl": "/mla-tamagui", "settleMs": 100, "tap": { "path": "/tap", "testId": "T1", "count": 3 }, "variant": { "path": "/v", "testIds": ["V1", "V2"], "count": 4 }, "mount": { "path": "/m", "readyTestId": "H1", "awayPath": "/away", "samples": 2 } }),
        1,
    );
    let r = lab.bench(&["runtime", "--runs", "1"], &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let m = readings(&r.stdout);
    assert!(m["tapLatencyMs"].unit == "ms" && (0.0..2000.0).contains(&median(&m, "tapLatencyMs")) && median(&m, "tapLatencyMs") > 0.0);
    assert!(median(&m, "variantUpdateMs") > 0.0 && median(&m, "deepTreeMountMs") > 0.0 && median(&m, "longTasks") >= 1.0 && median(&m, "longTaskMs") >= 50.0);

    // Given an export with source maps, When the startup suite runs, Then the executed bytes are attributed to packages and kept
    lab.scenario(
        "startup",
        json!({ "cwd": lab.app_rel, "platform": "web", "sourceMaps": true, "env": {}, "device": "iphone-15-pro", "port": free_port(), "baseUrl": "/mla-tamagui", "settleMs": 100, "start": "/", "scriptFilter": "/_expo/static/js/web/", "packages": ["zod"] }),
        1,
    );
    let r = lab.bench(&["startup", "--runs", "1"], &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let m = readings(&r.stdout);
    let lines: Vec<&str> = WEB_SCRIPT.split('\n').collect();
    let ran = (lines[0].len() + lines[2].len()) as f64;
    assert_eq!(
        [median(&m, "startupScriptBytes"), median(&m, "startup.zod.executedBytes"), median(&m, "startupExecutedBytes"), median(&m, "startup.first-party.executedBytes"), median(&m, "startup.other.executedBytes")],
        [WEB_SCRIPT.len() as f64, ran, ran, 0.0, 0.0]
    );
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(lab.tmp.join("startup-packages.json")).unwrap()).unwrap();
    assert_eq!((kept["byPackage"].clone(), kept["bySource"].clone()), (json!({ "zod": ran as u64 }), json!({ "../../node_modules/zod/index.js": ran as u64 })));
}
