//! The device command table the two CLIs and the Lua binder share: `wf browser` and `wf android` list, document and refuse the
//! same verbs, and a script finds the same methods on either platform.
use crate::support::{run_wf, RunOpts};
use serde_json::{json, Value};

fn only_json_line(stdout: &str) -> Value {
    let lines: Vec<&str> = stdout.trim().lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "one JSON line expected: {stdout}");
    serde_json::from_str(lines[0]).unwrap_or_else(|e| panic!("{e}: {stdout}"))
}

#[test]
fn both_device_clis_and_the_lua_binder_offer_the_same_commands_from_one_table() {
    // Given the command table as each CLI prints it
    let api = |noun: &str| {
        let r = run_wf(&[noun, "test", "--api", "--json"], &RunOpts::default().clean_env());
        assert_eq!(r.exit_code, 0, "{noun}: {}{}", r.stdout, r.stderr);
        only_json_line(&r.stdout)
    };
    let (browser, android) = (api("browser"), api("android"));
    let names = |v: &Value| v["commands"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    // Then both serve the one table, told apart only by their platform (the drivers' answers to each shared verb are compared
    // in maestro.rs)
    let table = |v: &Value| v.as_object().unwrap().iter().filter(|(k, _)| k.as_str() != "platform").map(|(k, v)| (k.clone(), v.clone())).collect::<serde_json::Map<_, _>>();
    assert_eq!(table(&browser), table(&android));
    assert_eq!(browser["platform"], json!("browser"));
    assert!(names(&browser).len() >= 25 && names(&browser).iter().any(|n| n == "extended_wait_until"), "{:?}", names(&browser));

    // When each noun is asked for its help, and each CLI verb for its own and with a flag it does not take
    let commands = browser["commands"].as_array().unwrap();
    for noun in ["browser", "android"] {
        let r = run_wf(&[noun, "--help"], &RunOpts::default().clean_env());
        let listed = r.stdout.split_once("\nDevice commands").map(|(_, l)| l.to_string()).unwrap_or_default();
        // Then the noun lists every CLI verb of the table
        for c in commands.iter().filter(|c| c["verb"].is_string()) {
            let verb = c["verb"].as_str().unwrap();
            assert!(listed.lines().any(|l| l.starts_with(&format!("  {verb} "))), "wf {noun} --help lists no {verb}: {listed}");
        }
    }
    for c in commands.iter().filter(|c| c["verb"].is_string()) {
        let verb = c["verb"].as_str().unwrap();
        for noun in ["browser", "android"] {
            let r = run_wf(&[noun, verb, "--help"], &RunOpts::default().clean_env());
            // and each verb's help is generated from the same row: usage, every parameter and the result schema
            assert_eq!(r.exit_code, 0, "wf {noun} {verb} --help: {}", r.stderr);
            assert!(r.stdout.starts_with(&format!("Usage: wf {noun} {verb}")) && r.stdout.contains(&format!("Result: {}", c["result"].as_str().unwrap())), "wf {noun} {verb}: {}", r.stdout);
            for p in c["params"].as_array().unwrap() {
                assert!(r.stdout.contains(p["doc"].as_str().unwrap()), "wf {noun} {verb}: no {} in {}", p["name"], r.stdout);
            }
            // and the verb reaches the table's front end: a flag it does not take is a usage error carrying the help's usage line
            let bogus = run_wf(&[noun, verb, "--bogus"], &RunOpts::default().clean_env());
            let usage = r.stdout.lines().next().and_then(|l| l.strip_prefix("Usage: ")).unwrap_or_default();
            let refused = only_json_line(&bogus.stdout);
            assert!(bogus.exit_code == 2 && refused["error"] == "unknown option --bogus" && refused["usage"] == usage, "wf {noun} {verb} --bogus: {refused}");
        }
    }
    // and the help is served wherever --help or -h stands after the verb, each noun's own verbs included
    for (args, usage) in [
        (vec!["android", "tap-on", "--id", "Go", "--help"], "Usage: wf android tap-on "),
        (vec!["browser", "get-element", "-h", "--id", "Go"], "Usage: wf browser get-element "),
        (vec!["browser", "swipe", "--help"], "Usage: wf browser swipe <start> <end>"),
        (vec!["android", "swipe", "--help"], "Usage: wf android swipe <start> <end>"),
        (vec!["browser", "start-device", "--device", "a", "--help"], "Usage: wf browser start-device --device N"),
        (vec!["android", "start-device", "--name", "a", "--help"], "Usage: wf android start-device [--name N]"),
    ] {
        let r = run_wf(&args, &RunOpts::default().clean_env());
        assert!(r.exit_code == 0 && r.stdout.starts_with(usage), "{args:?}: {}{}", r.stdout, r.stderr);
    }
    // and a Lua-only or CLI-only command says why in its doc
    for c in commands.iter().filter(|c| c["surface"] != "both") {
        assert!(c["doc"].as_str().unwrap().contains(if c["surface"] == "lua" { "Lua only" } else { "CLI only" }), "{c}");
    }

    // When the reference is asked for, and by the verb from before the rename
    let run_api = run_wf(&["android", "run", "--api"], &RunOpts::default().clean_env());
    let test_api = run_wf(&["android", "test", "--api"], &RunOpts::default().clean_env());
    // Then `run` is an unknown verb, and `test --api` names every Lua method of the table with its result schema
    assert_eq!((run_api.exit_code, only_json_line(&run_api.stdout)["error"].clone()), (2, json!("unknown verb 'run'")), "{}", run_api.stdout);
    for c in commands.iter().filter(|c| c["lua"].is_string()) {
        assert!(test_api.stdout.contains(&format!("{}\n      -> {}.", c["lua"].as_str().unwrap(), c["result"].as_str().unwrap())), "{} missing from test --api", c["lua"]);
        // and what kind each of its options is and what it means
        for p in c["params"].as_array().unwrap().iter().filter(|p| !matches!(p["name"].as_str(), Some("optional" | "label"))) {
            let line = format!("      {} ({}): {}", p["name"].as_str().unwrap(), p["kind"].as_str().unwrap(), p["doc"].as_str().unwrap());
            assert!(test_api.stdout.contains(&line), "{line:?} missing from test --api");
        }
    }

    // When a script on each platform asks the binder for every Lua method of the table, for the CLI-only ones and the names
    // from before the rename, and calls each method with an option no command takes
    let dir = tempfile::tempdir().unwrap();
    let lua_names: Vec<String> = commands.iter().filter(|c| c["lua"].is_string()).map(|c| format!("'{}'", c["name"].as_str().unwrap())).collect();
    let cli_only: Vec<String> = commands.iter().filter(|c| c["lua"].is_null()).map(|c| format!("'{}'", c["name"].as_str().unwrap())).collect();
    let old_names = ["open", "tap", "type_text", "press", "find", "text", "wait", "wait_gone", "appears", "screenshot", "trace", "shell", "stub", "step", "span", "golden", "debug", "stroke", "unstub", "record_start", "record_stop"];
    assert!(browser.get("renamed").is_none() && browser.get("removed").is_none(), "test --api --json lists no old names");
    let placeholder = |p: &Value| match p["kind"].as_str().unwrap() {
        "selector" => "'Go'".to_string(),
        "point" => "'1,2'".to_string(),
        "int" | "number" => "1".to_string(),
        "bool" => "true".to_string(),
        "json" => "{}".to_string(),
        k if k.contains('|') => format!("'{}'", k.split('|').next().unwrap()),
        _ => "'x'".to_string(),
    };
    let calls: String = commands
        .iter()
        .filter(|c| c["lua"].is_string())
        .map(|c| {
            let params = c["params"].as_array().unwrap();
            let mut args: Vec<String> = params.iter().filter(|p| p["positional"] == true).map(placeholder).collect();
            args.push("{bogus=true}".into());
            let takes: Vec<&str> = params.iter().map(|p| p["name"].as_str().unwrap()).collect();
            format!("refuses('{n}', function() return device:{n}({}) end, {})\n", args.join(", "), json!(takes.join(", ")), n = c["name"].as_str().unwrap())
        })
        .collect();
    let script = dir.path().join("t.lua");
    std::fs::write(
        &script,
        format!(
            "for _, n in ipairs({{{}}}) do assert_true(type(device[n]) == 'function', 'no device:' .. n) end\n\
             for _, n in ipairs({{{}}}) do local ok, v = pcall(function() return device[n] end) assert_true(not ok or v == nil, 'CLI-only device:' .. n .. ' is bound') end\n\
             for _, g in ipairs({{'run_flow', 'assert_true', 'sleep_ms', 'add_timing_span', 'env', 'now_ms', 'log', 'agent'}}) do assert_true(rawget(_G, g) ~= nil, 'no global ' .. g) end\n\
             local bad = {{}}\n\
             for _, n in ipairs({{{}}}) do if device[n] ~= nil then bad[#bad + 1] = 'device:' .. n .. ' is bound' end end\n\
             for _, g in ipairs({{'args', 'sleep', 'assert_that'}}) do if _G[g] ~= nil then bad[#bad + 1] = g .. ' is set' end end\n\
             if json.decode ~= nil then bad[#bad + 1] = 'json.decode is set' end\n\
             local ok, e = pcall(function() device:tap('Go') end)\n\
             if ok or not tostring(e):find(\"t.lua:8: attempt to call a nil value (method 'tap')\", 1, true) then bad[#bad + 1] = 'device:tap: ' .. tostring(e) end\n\
             local function refuses(n, f, takes) local ok, e = pcall(f) if ok or not tostring(e):find(\"unknown parameter 'bogus' (takes: \" .. takes .. \")\", 1, true) then bad[#bad + 1] = n .. ': ' .. tostring(e) end end\n\
             {calls}log('bound ' .. #bad .. ' ' .. table.concat(bad, ' | '))",
            lua_names.join(", "),
            cli_only.join(", "),
            old_names.iter().map(|n| format!("'{n}'")).collect::<Vec<_>>().join(", ")
        ),
    )
    .unwrap();
    for platform in ["browser", "android"] {
        let bound = run_wf(&[platform, "test", &script.display().to_string(), "--no-record", "--test-output-dir", &dir.path().join(platform).display().to_string()], &RunOpts::default().cwd(dir.path()).env("WF_ANDROID_LEASE", "none"));
        // Then each table method is a method there and checks its arguments against the same row, no CLI-only one is bound,
        // no old name or global is set, and calling an old method fails with Lua's own error at the script line
        assert!(bound.exit_code == 0 && bound.stdout.contains("bound 0 "), "{platform}: {}{}", bound.stdout, bound.stderr);
    }

    // When a verb one platform lacks runs there
    let back = run_wf(&["browser", "back"], &RunOpts::default().clean_env());
    // Then it exits 2 naming why
    assert_eq!(back.exit_code, 2, "{}{}", back.stdout, back.stderr);
    assert!(only_json_line(&back.stdout)["error"].as_str().unwrap().starts_with("not on browser: a page has no system Back key"), "{}", back.stdout);
}
