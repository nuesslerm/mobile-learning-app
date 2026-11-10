//! The helper commands of plan section 3 ("Helper commands"): `provision`, `appendEnvLocal` and `verifyEnv`, which the
//! app-redeploy and self-swap siblings run as `vm-ops-api-rs <subcommand>` (plan section 4, deliberate difference 3). The
//! `docker` double never runs a sibling's script, so these cases run each helper line under `sh -c` themselves, with the
//! service's own program on PATH and `provision` pointed at
//! a local double of raw.githubusercontent.com; the last case runs the helper lines exactly as the recorded scripts emit
//! them, so a line that names a command the image lacks fails here.
use crate::self_redeploy::{swap_world, wait_swap, BUILT_COMMIT, BUILT_TAG};
use crate::{expect_json, post, world};
use regex::Regex;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use vm_ops_api_integration::{case, helper_command, Exit, GithubRaw, ToolRule, World, GITHUB_RAW};

const TOKEN: [(&str, &str); 1] = [("GITHUB_RAW_TOKEN", "tok")];
const WITHIN: Duration = Duration::from_secs(30);
const REPO: &str = "/nuesslerm/mobile-learning-app/dev";
const REFUSED: &str = "[verify-env] Refusing to restart containers — the running ones are left untouched.\n";

fn quoted(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// The helper `name` called with `args`, each single-quoted, as a sibling script's line calls it.
fn line(name: &str, args: &[&str]) -> String {
    std::iter::once(helper_command(name)).chain(args.iter().map(|a| quoted(a))).collect::<Vec<_>>().join(" ")
}

/// A file in the world's `vm/`, the self-swap's host dir.
fn vm(w: &World, file: &str) -> PathBuf {
    w.dir().join("vm").join(file)
}

fn write(path: &PathBuf, text: &str) {
    std::fs::create_dir_all(path.parent().expect("a dir")).expect("the host dir");
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn read(path: &PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn get(path: &str, authorization: Option<&str>) -> Value {
    json!({ "method": "GET", "path": path, "authorization": authorization })
}

fn outcome(exit: &Exit) -> (Option<i32>, &str, &str) {
    (exit.code, exit.stdout.as_str(), exit.stderr.as_str())
}

case!(provision_fetches_each_url_with_the_token_and_writes_its_bytes, {
    // Given a raw double serving a compose file and a .env.defaults that holds a byte that is no UTF-8
    let compose: &[u8] = b"services:\n  app:\n    image: ns/app:${IMAGE_TAG}\n";
    let defaults: &[u8] = b"PORT=8080\nNOTE=caf\xc3\xa9 \xff\n";
    let (compose_path, defaults_path) = (format!("{REPO}/backend/app/docker-compose.yml"), format!("{REPO}/backend/app/.env.defaults"));
    let raw = GithubRaw::serve(&[(&compose_path, compose), (&defaults_path, defaults)]);
    let w = world();
    let (compose_dest, defaults_dest) = (vm(&w, "docker-compose.yml"), vm(&w, ".env.defaults"));
    let args = [
        format!("{}{compose_path}", raw.base()),
        compose_dest.display().to_string(),
        format!("{}{defaults_path}", raw.base()),
        defaults_dest.display().to_string(),
    ];
    let provision = line("provision", &args.each_ref().map(String::as_str));
    // When provision runs with the two url and dest pairs and the provisioning token, as the sibling has it
    let exit = w.run_helper(&provision, &TOKEN, WITHIN);
    // Then it exits 0 with no output, each file holds the bytes served, and each GET carried `Authorization: token tok`
    assert_eq!(outcome(&exit), (Some(0), "", ""));
    assert_eq!((std::fs::read(&compose_dest).unwrap(), std::fs::read(&defaults_dest).unwrap()), (compose.to_vec(), defaults.to_vec()));
    assert_eq!(raw.requests(), [get(&compose_path, Some("token tok")), get(&defaults_path, Some("token tok"))]);
    // When it runs with the token empty, as the sibling has it on a VM with no provisioning token
    let exit = w.run_helper(&provision, &[("GITHUB_RAW_TOKEN", "")], WITHIN);
    // Then it exits 0, and its GETs carry no Authorization header
    assert_eq!(outcome(&exit), (Some(0), "", ""));
    assert_eq!(raw.requests()[2..], [get(&compose_path, None), get(&defaults_path, None)]);
});

case!(provision_stops_at_the_first_file_it_cannot_get_and_exits_1, {
    // Given a raw double serving the first and the third of three files, not the second
    let paths = ["a", "b", "c"].map(|f| format!("{REPO}/{f}.txt"));
    let raw = GithubRaw::serve(&[(&paths[0], b"first\n"), (&paths[2], b"third\n")]);
    let w = world();
    let urls = paths.each_ref().map(|p| format!("{}{p}", raw.base()));
    let dests = ["a", "b", "c"].map(|f| vm(&w, &format!("{f}.txt")));
    let args: Vec<String> = urls.iter().zip(&dests).flat_map(|(u, d)| [u.clone(), d.display().to_string()]).collect();
    // When provision runs for the three
    let exit = w.run_helper(&line("provision", &args.iter().map(String::as_str).collect::<Vec<_>>()), &TOKEN, WITHIN);
    // Then it exits 1 with exactly `[provision] GET <url> -> 404` on stderr
    let message = format!("[provision] GET {} -> 404\n", urls[1]);
    assert_eq!(outcome(&exit), (Some(1), "", message.as_str()));
    // And the first file is written, and the third is neither fetched nor written
    assert_eq!(read(&dests[0]), "first\n");
    assert!(!dests[1].exists() && !dests[2].exists());
    let fetched: Vec<Value> = raw.requests().iter().map(|r| r["path"].clone()).collect();
    assert_eq!(fetched, [json!(paths[0]), json!(paths[1])]);
});

case!(provision_without_url_and_dest_pairs_exits_2_and_fetches_nothing, {
    // Given a raw double serving one file
    let path = format!("{REPO}/a.txt");
    let raw = GithubRaw::serve(&[(&path, b"a\n")]);
    let w = world();
    let (url, dest) = (format!("{}{path}", raw.base()), vm(&w, "a.txt").display().to_string());
    // When provision runs with no argument, a url alone, or a pair and a url
    for args in [vec![], vec![url.as_str()], vec![url.as_str(), dest.as_str(), url.as_str()]] {
        let exit = w.run_helper(&line("provision", &args), &TOKEN, WITHIN);
        // Then each exits 2 with its usage on stderr
        assert_eq!(exit.code, Some(2), "{args:?}: {}", exit.stderr);
        assert!(exit.stderr.starts_with("usage: "), "{args:?}: {}", exit.stderr);
    }
    // And nothing was fetched or written
    assert_eq!(raw.requests(), Vec::<Value>::new());
    assert!(!PathBuf::from(dest).exists());
});

case!(append_env_local_drops_an_empty_override_of_a_resolved_key_and_appends_the_rest, {
    // Given a .env as sync-env resolved it (two keys with values, one empty) and a .env.local that empties both resolved
    // keys (one as ""), empties the key that was empty anyway, sets a new one, and has a comment and a blank line
    let w = world();
    let (env, local) = (vm(&w, ".env"), vm(&w, ".env.local"));
    let resolved = "API_KEY=resolved-key\nDB_URL=postgres://u:pw@h/db\nOPTIONAL=\nHOST_LABEL=main\n";
    write(&env, resolved);
    write(&local, "API_KEY=\nOPTIONAL=\nNEW_FLAG=1\n# set by hand\n\nDB_URL=\"\"\n");
    // When appendEnvLocal runs on the two
    let exit = w.run_helper(&line("appendEnvLocal", &[&env.display().to_string(), &local.display().to_string()]), &[], WITHIN);
    // Then it exits 0, naming the two dropped keys and counting the lines it appended, on stdout
    let printed = format!(
        "[append-env-local] ignored 2 empty override(s) that would have shadowed a resolved value: API_KEY, DB_URL\n\
         [append-env-local] appended 3 line(s) from {}\n",
        local.display()
    );
    assert_eq!(outcome(&exit), (Some(0), printed.as_str(), ""));
    // And the .env ends with the rest of the local lines in order, the blank one left out
    assert_eq!(read(&env), format!("{resolved}OPTIONAL=\nNEW_FLAG=1\n# set by hand\n"));
});

case!(append_env_local_without_a_local_file_changes_nothing, {
    // Given a .env and no .env.local
    let w = world();
    let (env, local) = (vm(&w, ".env"), vm(&w, ".env.local"));
    write(&env, "API_KEY=resolved-key\n");
    // When appendEnvLocal runs on the two, Then it exits 0, prints nothing, and the .env is unchanged
    let exit = w.run_helper(&line("appendEnvLocal", &[&env.display().to_string(), &local.display().to_string()]), &[], WITHIN);
    assert_eq!(outcome(&exit), (Some(0), "", ""));
    assert_eq!(read(&env), "API_KEY=resolved-key\n");
    // When it runs with the .env alone, Then it exits 2 with its usage on stdout, and the .env is unchanged
    let exit = w.run_helper(&line("appendEnvLocal", &[&env.display().to_string()]), &[], WITHIN);
    assert_eq!(exit.code, Some(2), "{}{}", exit.stdout, exit.stderr);
    assert!(exit.stdout.starts_with("usage: ") && exit.stderr.is_empty(), "{}{}", exit.stdout, exit.stderr);
    assert_eq!(read(&env), "API_KEY=resolved-key\n");
});

case!(verify_env_refuses_a_resolved_key_that_ends_missing_empty_or_quoted_twice_naming_only_the_key, {
    // Given what sync-env resolved: two secrets and a key it left empty
    let w = world();
    let (synced, env) = (vm(&w, ".env.synced"), vm(&w, ".env"));
    let resolved = "API_KEY=s3cret-value\nDB_URL=postgres://u:pw@h/db\nEMPTY=\n";
    write(&synced, resolved);
    let clobbered = "[verify-env] FAILED: 1 key(s) resolved by sync-env would reach the container empty: API_KEY\n\
        [verify-env] Something appended after sync-env (HOST_LABEL, .env.local) overrode them. Compose takes the LAST occurrence.\n";
    let mangled = "[verify-env] FAILED: 1 key(s) are JSON-quoted twice, so the app would receive the quotes as part of the secret: API_KEY\n\
        [verify-env] Check this .env for a hand-appended pre-quoted line, .env.local, and any ambient env var of the same name.\n";
    let finals = [
        ("missing", "DB_URL=postgres://u:pw@h/db\nEMPTY=\nHOST_LABEL=main\n".to_string(), clobbered),
        ("emptied last", format!("{resolved}HOST_LABEL=main\nAPI_KEY=\n"), clobbered),
        ("quoted twice", format!("{resolved}API_KEY=\"\\\"s3cret-value\\\"\"\n"), mangled),
        ("emptied, and another quoted twice", format!("{resolved}API_KEY=\nDB_URL=\"\\\"postgres://u:pw@h/db\\\"\"\n"), clobbered),
    ];
    for (how, text, message) in finals {
        // When the final .env has API_KEY missing, emptied by a later line, or JSON-quoted twice, or has it emptied and
        // DB_URL quoted twice, which reports the emptied key alone
        write(&env, &text);
        let exit = w.run_helper(&line("verifyEnv", &[&synced.display().to_string(), &env.display().to_string()]), &[], WITHIN);
        // Then verifyEnv exits 1, naming the key and refusing the restart on stderr, and prints no value
        let expected = format!("{message}{REFUSED}");
        assert_eq!(outcome(&exit), (Some(1), "", expected.as_str()), "{how}");
        assert!(!exit.stderr.contains("s3cret") && !exit.stderr.contains("postgres"), "{how}: {}", exit.stderr);
    }
});

case!(verify_env_with_one_file_exits_2_with_its_usage_on_stderr, {
    // Given what sync-env resolved
    let w = world();
    let synced = vm(&w, ".env.synced");
    write(&synced, "API_KEY=s3cret-value\n");
    // When verifyEnv runs with the snapshot alone
    let exit = w.run_helper(&line("verifyEnv", &[&synced.display().to_string()]), &[], WITHIN);
    // Then it exits 2 with its usage on stderr and nothing on stdout
    assert_eq!((exit.code, exit.stdout.as_str()), (Some(2), ""), "{}", exit.stderr);
    assert!(exit.stderr.starts_with("usage: "), "{}", exit.stderr);
});

case!(verify_env_passes_a_final_env_that_keeps_every_resolved_key, {
    // Given what sync-env resolved, and a final .env with HOST_LABEL, a local line and a non-empty override appended
    let w = world();
    let (synced, env) = (vm(&w, ".env.synced"), vm(&w, ".env"));
    let resolved = "API_KEY=s3cret-value\nDB_URL=postgres://u:pw@h/db\nEMPTY=\n";
    write(&synced, resolved);
    write(&env, &format!("{resolved}HOST_LABEL=main\nLOCAL_ONLY=1\nAPI_KEY=local-override\n"));
    // When verifyEnv runs on the two
    let exit = w.run_helper(&line("verifyEnv", &[&synced.display().to_string(), &env.display().to_string()]), &[], WITHIN);
    // Then it exits 0, counting the synced keys on stderr
    assert_eq!(outcome(&exit), (Some(0), "", "[verify-env] ok: 3 synced key(s) survive into the final .env\n"));
});

/// The single-quoted arguments of a line, unquoted: a provision line's URLs and dests.
fn quoted_args(line: &str) -> Vec<String> {
    Regex::new(r"'([^']*)'").expect("a regex").captures_iter(line).map(|c| c[1].to_string()).collect()
}

/// What the raw double serves for `path`.
fn served(path: &str) -> String {
    format!("# served from {path}\n")
}

/// The lines of a sibling script that call a helper, with each helper's name.
fn helper_lines(script: &str) -> Vec<(&'static str, String)> {
    let call = Regex::new(r"^vm-ops-api-rs (provision|append-env-local|verify-env) ").expect("a regex");
    let named = |name: &str| match name {
        "provision" => "provision",
        "append-env-local" => "appendEnvLocal",
        _ => "verifyEnv",
    };
    let each = script.lines().filter_map(|l| {
        let found = call.captures(l)?;
        // a line of an `&&` chain ends with its operator, which is the script's, not the helper's
        Some((named(found.get(1)?.as_str()), l.strip_suffix(" &&").unwrap_or(l).to_string()))
    });
    each.collect()
}

case!(the_helper_lines_of_the_recorded_scripts_run_with_this_implementations_program, [
    "POST /services/:app/redeploy",
    "POST /redeploy"
], {
    // Given the service, whose app-redeploy and smoke-test siblings exit 0
    let mut w = swap_world();
    w.rule(ToolRule::docker(&["run", "--rm"]).rest(r">> redeploy-history\.jsonl").answer("", "", 0))
        .rule(ToolRule::docker(&["run", "--pull", "missing"]).answer("", "", 0));
    let svc = w.start();
    // When leetcode-backend is redeployed, and vm-ops-api redeploys itself
    assert_eq!(post(&svc, "/services/leetcode-backend/redeploy", b"").status, 200);
    expect_json(&post(&svc, &format!("/redeploy?ref={BUILT_COMMIT}&tag={BUILT_TAG}"), b""), 202, &json!({ "started": true, "swapping": BUILT_COMMIT, "tag": BUILT_TAG }));
    let swap = wait_swap(&svc, Duration::from_secs(20));
    let app = svc.world().docker_calls()[1].clone();
    let scripts = [("the app redeploy", &app, true), ("the self-swap", &swap, false)];
    // Then each script calls provision, appendEnvLocal and verifyEnv, in order, the way this implementation's image has them
    let mut paths = Vec::new();
    for (sibling, argv, _) in scripts {
        let lines = helper_lines(argv.last().map(String::as_str).unwrap_or(""));
        let names: Vec<&str> = lines.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["provision", "appendEnvLocal", "verifyEnv"], "{sibling}: {lines:?}");
        for (name, line) in &lines {
            assert!(line.starts_with(&format!("{} ", helper_command(name))), "{sibling}: {line}");
        }
        let urls = quoted_args(&lines[0].1).into_iter().step_by(2);
        paths.extend(urls.map(|u| format!("/{}", u.strip_prefix(GITHUB_RAW).unwrap_or_else(|| panic!("{sibling}: provision from {u}")))));
    }
    // Given a raw double serving each file the provision lines name, and each host dir as its sibling finds it: a .env as
    // sync-env left it, a .env.local that empties the resolved key and adds one, and the sync-env snapshot
    let bodies: Vec<(String, Vec<u8>)> = paths.iter().map(|p| (p.clone(), served(p).into_bytes())).collect();
    let raw = GithubRaw::serve(&bodies.iter().map(|(p, b)| (p.as_str(), b.as_slice())).collect::<Vec<_>>());
    let world_dir = svc.world().dir().display().to_string();
    for (sibling, argv, on_host) in scripts {
        let token = argv.iter().find_map(|a| a.strip_prefix("GITHUB_RAW_TOKEN=")).expect("the sibling's provisioning token");
        // The provision URLs point at the raw double instead of GitHub, and the app redeploy's /home/ubuntu into the case
        let local = |line: String| {
            let line = line.replace(GITHUB_RAW, &format!("{}/", raw.base()));
            if on_host { line.replace("/home/ubuntu/", &format!("{world_dir}/home/ubuntu/")) } else { line }
        };
        let lines: Vec<String> = helper_lines(argv.last().map(String::as_str).unwrap_or("")).into_iter().map(|(_, l)| local(l)).collect();
        // the files as the lines name them: the self-swap stages its .env and .env.synced and reads the host's .env.local
        let words = |line: &str| line.split_whitespace().map(String::from).collect::<Vec<_>>();
        let (append, verify) = (words(&lines[1]), words(&lines[2]));
        let env = PathBuf::from(&append[append.len() - 2]);
        write(&env, "API_KEY=resolved\nHOST_LABEL=main\n");
        write(&PathBuf::from(&append[append.len() - 1]), "API_KEY=\nLOCAL_ONLY=1\n");
        write(&PathBuf::from(&verify[verify.len() - 2]), "API_KEY=resolved\n");
        // When each helper line runs in order, as the sibling runs it, with the sibling's token
        let exits: Vec<Exit> = lines.iter().map(|l| svc.world().run_helper(l, &[("GITHUB_RAW_TOKEN", token)], WITHIN)).collect();
        // Then each exits 0: provision wrote each file it fetched, appendEnvLocal appended the local line and dropped the
        // empty one, and verifyEnv found the resolved key intact
        let codes: Vec<Option<i32>> = exits.iter().map(|e| e.code).collect();
        assert_eq!(codes, [Some(0); 3], "{sibling}: {:?}", exits.iter().map(|e| &e.stderr).collect::<Vec<_>>());
        let pairs = quoted_args(&lines[0]);
        assert_eq!(pairs.len(), 4, "{sibling}: {}", lines[0]);
        for pair in pairs.chunks(2) {
            let path = pair[0].strip_prefix(raw.base()).unwrap_or_default();
            assert_eq!(read(&PathBuf::from(&pair[1])), served(path), "{sibling}: {}", pair[1]);
        }
        assert_eq!(read(&env), "API_KEY=resolved\nHOST_LABEL=main\nLOCAL_ONLY=1\n", "{sibling}");
        assert_eq!(exits[2].stderr, "[verify-env] ok: 1 synced key(s) survive into the final .env\n", "{sibling}");
    }
    // And every file was fetched once, with the sibling's token
    let fetched: Vec<Value> = paths.iter().map(|p| get(p, Some("token tok"))).collect();
    assert_eq!(raw.requests(), fetched);
});
