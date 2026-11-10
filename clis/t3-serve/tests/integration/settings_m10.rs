//! The settings and accounts rows of M10 through the chain: each call goes from the phone through `t3-serve serve` to the real rca-service, whose
//! login commands and turns are the scripted CLI (which keeps its token in the directory its environment names, as a real CLI does). A call
//! counts for its row only when it was answered and the service did what the row says, and each test reads the service's side itself (the
//! provider directories on its volume, the environment and files each process saw, the device state): the state the service holds is the
//! evidence, not the answer. Rows that only pass by being refused are called here too, as refusals, so the trace shows why they stay pending.
use crate::chain::{hit, last_window, writes};
use crate::rig::*;
use crate::support::*;
use exec_testkit::script::Turn;
use exec_testkit::Invocation;
use std::path::Path;
use std::time::{Duration, Instant};

const URL_LINE: &str = "Open https://login.example.test/device to sign in.";
const CODE_LINE: &str = "Enter the code ABCD-1234 there.";
const PASTE_LINE: &str = "Open https://login.example.test/paste and paste the code.";
/// What a scripted process keeps the value of: every provider variable and the service's own secrets.
const WATCHED: [&str; 9] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "KIMI_API_KEY", "CHAT_AGENT_API_SECRET", "GH_TOKEN"];
const CODEX_TOKEN: &str = "codex-device-token-4711";
const CLAUDE_TOKEN: &str = "claude-paste-token-8123";

fn all_scopes() -> Vec<String> {
    t3_contracts::scopes::ALL_SCOPES.iter().map(|s| s.to_string()).collect()
}

fn asked(agent: &[(String, bool)], method: &str) -> bool {
    agent.iter().any(|(m, ok)| m == method && *ok)
}

fn codex_login() -> Turn {
    Turn::default().then_line(URL_LINE).then_line(CODE_LINE).then_sleeping(500).then_writing_credential("CODEX_HOME", "auth.json", &format!("{{\"access_token\":\"{CODEX_TOKEN}\"}}")).logging_env(&WATCHED).listing_env(&["CODEX_HOME"])
}

fn claude_login() -> Turn {
    Turn::default().then_line(PASTE_LINE).then_reading_a_line().then_writing_credential("CLAUDE_CONFIG_DIR", ".credentials.json", "{\"access_token\":\"{paste}\"}").logging_env(&WATCHED).listing_env(&["CLAUDE_CONFIG_DIR"])
}

fn providers_dir(chain: &Chain) -> std::path::PathBuf {
    chain.service.home().join("providers")
}

fn files_below(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                walk(&entry.path(), &format!("{name}/"), out);
            } else {
                out.push(name);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out.sort();
    out
}

fn starts(chain: &Chain, program: &str, count: usize) -> Vec<Invocation> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let runs = chain.cli.invocations(program);
        if runs.len() >= count || Instant::now() > deadline {
            return runs;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The frames of the watch socket's stream `id` until a state in `phase` arrives, in order: the phase of each.
fn phases_until(phone: &mut Phone, id: &Value, phase: &str) -> Vec<Value> {
    let mut phases: Vec<Value> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while phases.last() != Some(&json!(phase)) {
        assert!(Instant::now() < deadline, "the stream said {phases:?}");
        let Some(frame) = phone.watch.try_recv(Duration::from_millis(300)) else { continue };
        if frame["_tag"] != "Chunk" || frame["requestId"] != *id {
            continue;
        }
        phone.watch.ack(id);
        phases.extend(frame["values"].as_array().unwrap().iter().map(|state| state["phase"].clone()));
    }
    phases
}

#[test]
fn two_providers_sign_in_by_a_device_code_and_a_pasted_code_each_in_its_own_directory_and_a_turn_then_runs_in_the_signed_in_ones_alone() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a codex whose login prints a device code and keeps its token, a claude whose login asks for a pasted code, and a phone
        let chain = Chain::start();
        chain.cli.script("codex", &exec_testkit::script::Script::of(vec![codex_login(), codex_login()]));
        chain.cli.script("claude", &exec_testkit::script::Script::of(vec![claude_login(), writes("notes/a.txt", "hello from the model").logging_env(&WATCHED).listing_env(&["CLAUDE_CONFIG_DIR", "CODEX_HOME"])]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let mut said = Vec::<String>::new();
        // When the phone starts codex's login, and then waits for it
        let started = phone.call(case, &chain, &["rpc:provider.auth.start"], "provider.auth.start", json!({"instanceId": "codex"}));
        let (agent, _) = last_window(case);
        said.push(started.to_string());
        // Then the service started the CLI's login and the phone is shown the address and the code the CLI printed
        assert!(asked(&agent, "provider.auth.start"), "{agent:?}");
        let state = success(&started);
        assert_eq!((state["phase"].clone(), state["interaction"]["type"].clone(), state["interaction"]["userCode"].clone()), (json!("waiting"), json!("deviceCode"), json!("ABCD-1234")));
        assert_eq!(state["interaction"]["url"], json!("https://login.example.test/device"));
        let flow = state["flowId"].as_str().unwrap().to_string();
        // When the phone completes it (a device code has nothing to hand over)
        let done = phone.call(case, &chain, &["rpc:provider.auth.complete"], "provider.auth.complete", json!({"instanceId": "codex", "flowId": flow, "callbackUrl": "https://login.example.test/unused"}));
        let (agent, _) = last_window(case);
        said.push(done.to_string());
        // Then the login ended signed in, and the CLI wrote its token in codex's own directory on the service's volume
        assert!(asked(&agent, "provider.auth.complete"), "{agent:?}");
        assert_eq!(success(&done)["phase"], json!("succeeded"));
        assert_eq!(files_below(&providers_dir(&chain).join("codex")), ["auth.json"]);
        assert!(std::fs::read_to_string(providers_dir(&chain).join("codex/auth.json")).unwrap().contains(CODEX_TOKEN));
        // When the phone signs claude in by a pasted code: the login asks for it, the phone answers the terminal prompt, and the stream says the end
        let asked_paste = phone.call(case, &chain, &["rpc:provider.auth.start"], "provider.auth.start", json!({"instanceId": "claudeAgent", "methodId": "paste"}));
        said.push(asked_paste.to_string());
        let paste = success(&asked_paste);
        assert_eq!((paste["phase"].clone(), paste["interaction"]["type"].clone()), (json!("waiting"), json!("terminal")));
        assert!(paste["interaction"]["output"].as_str().unwrap().contains("https://login.example.test/paste"));
        let (stream, first) = phone.stream(case, &chain, &["rpc:provider.auth.subscribe"], "provider.auth.subscribe", json!({"instanceId": "claudeAgent"}));
        assert_eq!(first[0]["flowId"], paste["flowId"]);
        let pasted = phone.call(case, &chain, &["rpc:provider.auth.respond"], "provider.auth.respond", json!({"instanceId": "claudeAgent", "flowId": paste["flowId"], "interactionId": paste["flowId"], "response": {"type": "terminal", "data": format!("{CLAUDE_TOKEN}\n")}}));
        let (agent, _) = last_window(case);
        said.push(pasted.to_string());
        assert!(asked(&agent, "provider.auth.respond"), "{agent:?}");
        let phases = phases_until(&mut phone, &stream, "succeeded");
        said.extend(phases.iter().map(Value::to_string));
        // Then claude's own directory holds its login, which is the pasted code, and codex's still holds only its own
        assert_eq!(files_below(&providers_dir(&chain).join("claude")), [".credentials.json"]);
        assert!(std::fs::read_to_string(providers_dir(&chain).join("claude/.credentials.json")).unwrap().contains(CLAUDE_TOKEN));
        assert_eq!(files_below(&providers_dir(&chain).join("codex")), ["auth.json"]);
        // When codex signs in again now that claude holds a login
        let again = phone.call(case, &chain, &["rpc:provider.auth.start"], "provider.auth.start", json!({"instanceId": "codex"}));
        said.push(again.to_string());
        let second = phone.call(case, &chain, &["rpc:provider.auth.complete"], "provider.auth.complete", json!({"instanceId": "codex", "flowId": success(&again)["flowId"], "callbackUrl": "https://login.example.test/unused"}));
        said.push(second.to_string());
        assert_eq!(success(&second)["phase"], json!("succeeded"));
        // When a turn runs on a claude thread
        phone.seed(case, &chain);
        let turn = phone.message(case, &chain, "t1", "c-m1", "make a note");
        said.push(turn.to_string());
        let (_, executor) = last_window(case);
        assert!(hit(&executor, "tool", "write_file").is_some_and(|h| h["ok"] == true), "the signed-in turn did not write through rca-exec: {executor}");
        // Then claude's process was started in claude's directory with only its own login: no codex variable or file, and no secret of the service
        let runs = starts(&chain, "claude", 2);
        let (login, run) = (&runs[0], &runs[1]);
        let claude_dir = providers_dir(&chain).join("claude").to_string_lossy().into_owned();
        assert_eq!(run.env["CLAUDE_CONFIG_DIR"], Some(claude_dir.clone()), "{run:?}");
        assert_eq!(login.env["CLAUDE_CONFIG_DIR"], Some(claude_dir.clone()), "{login:?}");
        assert_eq!(run.listing["CLAUDE_CONFIG_DIR"], [".credentials.json"]);
        assert_eq!((run.env["CODEX_HOME"].clone(), run.listing["CODEX_HOME"].clone()), (None, Vec::<String>::new()));
        for name in ["CHAT_AGENT_API_SECRET", "GH_TOKEN"] {
            assert_eq!((run.env[name].clone(), login.env[name].clone()), (None, None), "{name}");
        }
        // And codex's login process saw only codex's directory: none of claude's variables, and nothing of claude's in it
        let codex_runs = starts(&chain, "codex", 2);
        let codex = &codex_runs[0];
        let codex_dir = providers_dir(&chain).join("codex").to_string_lossy().into_owned();
        assert_eq!(codex.env["CODEX_HOME"], Some(codex_dir), "{codex:?}");
        assert_eq!((codex.env["CLAUDE_CONFIG_DIR"].clone(), codex.env["XDG_DATA_HOME"].clone()), (None, None));
        assert!(codex.listing["CODEX_HOME"].is_empty(), "{codex:?}");
        // And codex's login after claude held a login listed only codex's own file in its directory and saw none of claude's variables
        let later = &codex_runs[1];
        assert_eq!(later.listing["CODEX_HOME"], ["auth.json"], "{later:?}");
        assert_eq!((later.env["CLAUDE_CONFIG_DIR"].clone(), later.env["XDG_DATA_HOME"].clone()), (None, None));
        assert_eq!(later.env["CODEX_HOME"], codex.env["CODEX_HOME"]);
        // And the file the turn wrote is in the repository: the turn ran
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/a.txt")).unwrap(), "hello from the model");
        // And no frame the phone saw carries either token
        assert!(said.iter().all(|s| !s.contains(CODEX_TOKEN) && !s.contains(CLAUDE_TOKEN)), "a token reached the phone");
        // When the phone signs codex out, then refreshes the providers and asks for the usage
        let out = phone.call(case, &chain, &["rpc:provider.auth.logout"], "provider.auth.logout", json!({"instanceId": "codex"}));
        let (agent, _) = last_window(case);
        // Then codex's login is gone from its directory and claude's is untouched
        assert!(asked(&agent, "provider.logout"), "{agent:?}");
        assert_eq!(success(&out)["phase"], json!("idle"));
        assert!(files_below(&providers_dir(&chain).join("codex")).is_empty());
        assert_eq!(files_below(&providers_dir(&chain).join("claude")), [".credentials.json"]);
        let refreshed = phone.call(case, &chain, &["rpc:server.refreshProviders"], "server.refreshProviders", json!({}));
        let (agent, _) = last_window(case);
        assert!(asked(&agent, "provider.refresh") && asked(&agent, "provider.list"), "{agent:?}");
        let auth: Vec<(String, String)> = success(&refreshed)["providers"].as_array().unwrap().iter().map(|p| (p["instanceId"].as_str().unwrap().to_string(), p["auth"]["status"].as_str().unwrap().to_string())).collect();
        assert!(auth.contains(&("claudeAgent".into(), "authenticated".into())) && auth.contains(&("codex".into(), "unauthenticated".into())), "{auth:?}");
        let usage = phone.call(case, &chain, &["rpc:server.getUsageSummary"], "server.getUsageSummary", json!({"sinceDay": "2026-11-01", "untilDay": "2026-11-04", "timeZone": "UTC"}));
        let (agent, _) = last_window(case);
        assert!(asked(&agent, "usage.summary"), "{agent:?}");
        let sources: Vec<(String, String)> = success(&usage)["sources"].as_array().unwrap().iter().map(|s| (s["fingerprint"]["provider"].as_str().unwrap().to_string(), s["status"].as_str().unwrap().to_string())).collect();
        assert!(sources.contains(&("claude".into(), "ok".into())) && sources.contains(&("codex".into(), "missing".into())), "{sources:?}");
    });
}

#[test]
fn claude_signs_in_by_a_pasted_code_when_the_phone_names_no_method_and_refuses_a_device_code() {
    exec_testkit::case!(SUITE, |case| {
        // Given a claude whose login asks for a pasted code
        let chain = Chain::start();
        chain.cli.script("claude", &exec_testkit::script::Script::of(vec![claude_login()]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        // When the phone starts the login with no method, and then asks for a device code
        let started = phone.call(case, &chain, &["rpc:provider.auth.start"], "provider.auth.start", json!({"instanceId": "claudeAgent"}));
        let device = phone.calls.call("provider.auth.start", json!({"instanceId": "claudeAgent", "methodId": "device_code"}));
        // Then the default is the paste flow, which the phone answers and which ends signed in with the code in claude's directory
        let flow = success(&started);
        assert_eq!((flow["phase"].clone(), flow["interaction"]["type"].clone()), (json!("waiting"), json!("terminal")));
        assert!(failure(&device)["_tag"].is_string(), "{device}");
        let (stream, _) = phone.stream(case, &chain, &["rpc:provider.auth.subscribe"], "provider.auth.subscribe", json!({"instanceId": "claudeAgent"}));
        phone.call(case, &chain, &["rpc:provider.auth.respond"], "provider.auth.respond", json!({"instanceId": "claudeAgent", "flowId": flow["flowId"], "interactionId": flow["flowId"], "response": {"type": "terminal", "data": format!("{CLAUDE_TOKEN}\n")}}));
        phases_until(&mut phone, &stream, "succeeded");
        assert!(std::fs::read_to_string(providers_dir(&chain).join("claude/.credentials.json")).unwrap().contains(CLAUDE_TOKEN));
    });
}

#[test]
fn a_login_the_phone_cancels_ends_cancelled_and_leaves_no_login_in_the_providers_directory() {
    exec_testkit::case!(SUITE, |case| {
        // Given an opencode whose login waits for a pasted code that never comes
        let chain = Chain::start();
        chain.cli.script("opencode", &exec_testkit::script::Script::of(vec![Turn::default().then_line(PASTE_LINE).hanging().logging_env(&WATCHED)]));
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let started = phone.call(case, &chain, &["rpc:provider.auth.start"], "provider.auth.start", json!({"instanceId": "opencode"}));
        let flow = success(&started)["flowId"].as_str().unwrap().to_string();
        assert_eq!(success(&started)["phase"], json!("waiting"));
        // When the phone cancels it
        let cancelled = phone.call(case, &chain, &["rpc:provider.auth.cancel"], "provider.auth.cancel", json!({"instanceId": "opencode", "flowId": flow}));
        let (agent, _) = last_window(case);
        // Then the service stopped the login, the phone sees it cancelled, and nothing was written for opencode
        assert!(asked(&agent, "provider.auth.cancel"), "{agent:?}");
        assert_eq!(success(&cancelled)["phase"], json!("cancelled"));
        assert!(files_below(&providers_dir(&chain).join("opencode")).is_empty());
        // And a second login of the provider can start
        let again = phone.call(case, &chain, &["rpc:provider.auth.start"], "provider.auth.start", json!({"instanceId": "opencode"}));
        assert_eq!(success(&again)["phase"], json!("waiting"));
    });
}

#[test]
fn updated_settings_are_kept_by_the_service_as_device_state_under_the_t3_scope_and_read_back_in_the_config() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a boolean setting of the defaults
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let (key, flipped) = t3_contracts::wire::default_settings().into_iter().find_map(|(k, v)| Some((k, json!(!v.as_bool()?)))).unwrap();
        // When the phone changes it
        let updated = phone.call(case, &chain, &["rpc:server.updateSettings"], "server.updateSettings", json!({"patch": {key.clone(): flipped}}));
        let (agent, _) = last_window(case);
        // Then the service holds it as device state, and the answer shows it
        assert!(asked(&agent, "state.get") && asked(&agent, "state.set"), "{agent:?}");
        assert_eq!(success(&updated)[&key], flipped);
        let held = chain.direct().call("state", "state.get", json!({"scope": "t3", "key": "server.settings"})).unwrap();
        assert_eq!(held["value"], json!({key.clone(): flipped}));
        // And the config a phone reads now has it
        let config = phone.calls.call("server.getConfig", json!({}));
        assert_eq!(success(&config)["settings"][&key], flipped);
        // And the operations on the model lists and the thread mode key, which are patch keys of T3 and no keys of the defaults, are kept too
        let ops = phone.calls.call("server.updateSettings", json!({"patch": {
            "setModelFavorites": [{"provider": "codex", "model": "m1", "favorite": true}],
            "setModelsHidden": [{"provider": "codex", "model": "m2", "hidden": true}],
            "setProviderModelOrder": {"provider": "claudeAgent", "modelOrder": ["o1", "o2"]},
            "defaultThreadEnvMode": "worktree",
        }}));
        let (agent, _) = last_window(case);
        assert!(asked(&agent, "state.set"), "{agent:?}");
        let settings = success(&ops);
        assert_eq!(settings["favorites"], json!([{"provider": "codex", "model": "m1"}]));
        assert_eq!(settings["providerModelPreferences"]["codex"]["hiddenModels"], json!(["m2"]));
        assert_eq!(settings["providerModelPreferences"]["claudeAgent"]["modelOrder"], json!(["o1", "o2"]));
        assert_eq!(settings["defaultThreadEnvMode"], json!("worktree"));
        let unfavorited = phone.calls.call("server.updateSettings", json!({"patch": {"setModelFavorites": [{"provider": "codex", "model": "m1", "favorite": false}]}}));
        assert_eq!(success(&unfavorited)["favorites"], json!([]));
        assert_eq!(success(&phone.calls.call("server.getConfig", json!({})))["settings"]["providerModelPreferences"]["codex"]["hiddenModels"], json!(["m2"]));
        // And a setting this server does not have is refused and the state is left as it was
        let refused = phone.calls.call("server.updateSettings", json!({"patch": {"noSuchSetting": true}}));
        assert_eq!(failure(&refused)["_tag"], json!("ServerSettingsError"));
        assert_eq!(chain.direct().call("state", "state.get", json!({"scope": "t3", "key": "server.settings"})).unwrap()["value"]["noSuchSetting"], Value::Null);
    });
}

#[test]
fn the_database_storage_stream_measures_the_state_database_serve_keeps_in_its_state_directory() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, whose serve keeps its database in the chain's state directory
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        // When the phone opens the stream
        let (_, values) = phone.stream(case, &chain, &["rpc:server.getDatabaseStorage"], "server.getDatabaseStorage", json!({}));
        // Then the status measures a database with pages in it, and names the file in the state directory in the command that compacts it
        let status = &values[0];
        assert!(status["databaseBytes"].as_u64().unwrap() > 0, "{status}");
        assert!(status["reclaimableBytes"].as_u64().unwrap() <= status["databaseBytes"].as_u64().unwrap());
        assert_eq!(status["compactCommand"], json!(format!("sqlite3 {} VACUUM", chain.state.path().join("state.sqlite").display())));
    });
}

#[test]
fn the_rows_that_need_an_account_flow_or_an_image_are_refused_through_the_chain_with_their_reason_and_serve_no_row() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain and a paired phone
        let chain = Chain::start();
        let mut phone = Phone::connect_with(case, &chain, &all_scopes());
        let tap = &chain.tap;
        // When the phone consumes a reset credit (the service's model API resets none), updates a provider, imports a ChatGPT profile and opens a handoff
        let credit = case.call(tap, &rows(&["rpc:provider.consumeResetCredit"]), || {
            let exit = phone.calls.call("provider.consumeResetCredit", json!({"instanceId": "codex"}));
            (false, exit)
        });
        let (agent, _) = last_window(case);
        // Then the service was asked and refused (NOT_SUPPORTED), which the phone sees as the usage source error
        assert!(agent.iter().any(|(m, ok)| m == "provider.resetCredit" && !ok), "{agent:?}");
        assert_eq!(failure(&credit)["_tag"], json!("UsageLimitSourceError"));
        let update = case.call(tap, &rows(&["rpc:server.updateProvider"]), || (false, phone.calls.call("server.updateProvider", json!({"provider": "codex"}))));
        assert_eq!(failure(&update)["_tag"], json!("ServerProviderUpdateError"));
        let import = case.call(tap, &rows(&["rpc:provider.chatgpt.import-profile"]), || (false, phone.calls.call("provider.chatgpt.import-profile", json!({"instanceId": "codex", "profile": {}}))));
        assert_eq!(failure(&import)["_tag"], json!("ProviderSetupError"));
        let handoff = case.call(tap, &rows(&["rpc:provider.chatgpt.handoff.subscribe"]), || (false, phone.calls.call("provider.chatgpt.handoff.subscribe", json!({"instanceId": "codex", "environmentId": "e", "attemptId": "a", "returnUrl": "https://x.test", "profile": null}))));
        assert_eq!(failure(&handoff)["_tag"], json!("ProviderSetupError"));
        let reconnect = case.call(tap, &rows(&["rpc:provider.chatgpt.reconnect-profile"]), || (false, phone.calls.call("provider.chatgpt.reconnect-profile", json!({"instanceId": "codex", "methodId": "chatgpt"}))));
        assert_eq!(success(&reconnect), &Value::Null);
        let rates = case.call(tap, &rows(&["rpc:server.refreshUsageRates"]), || (false, phone.calls.call("server.refreshUsageRates", json!({}))));
        assert_eq!(success(&rates)["status"], json!("unavailable"));
        // And none of the five reached the service but the credit (which it refused), so no call did what its row says
        assert!(case.calls().iter().rev().take(5).all(|c| c["ok"] == false));
        assert!(hit(&case.calls().last().unwrap()["agent"], "method", "usage.summary").is_none());
    });
}
