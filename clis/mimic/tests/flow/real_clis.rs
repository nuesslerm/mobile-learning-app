//! The real agent CLIs of the integration chain: the mode, each credential reaching only its own CLI (fake CLIs that dump what they hold), a missing key
//! failing naming the key, the rotated login a second run starts from, and the exact-value scan of an output before it is published.
use crate::support::*;
use std::collections::HashMap;
use std::path::Path;
use wf_mimic::child_env::ChildEnv;
use wf_mimic::real_clis::{parse_mode, scan, Resolved, CLIS};

const CLAUDE: &str = "claude-token-value-0001";
const OPENCODE: &str = "opencode-key-value-0002";
const REFRESH: &str = "refresh-token-value-000003";

fn login(refresh: &str) -> String {
    let json = format!(r#"{{"tokens":{{"access_token":"access-token-value-0004","refresh_token":"{refresh}","account_id":"acct-1"}}}}"#);
    let out = std::process::Command::new("sh").arg("-c").arg(format!("printf %s '{json}' | base64 -w0")).output().unwrap();
    String::from_utf8(out.stdout).unwrap()
}

fn vault(codex: &str) -> impl Fn(&[&str]) -> HashMap<String, Option<String>> {
    let codex = codex.to_string();
    move |keys: &[&str]| {
        let all = [("CLAUDE_CODE_OAUTH_TOKEN", CLAUDE.to_string()), ("OPENCODE_GO_API_KEY", OPENCODE.to_string()), ("CODEX_OAUTH_JSON_B64", codex.clone())];
        keys.iter().map(|k| (k.to_string(), all.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()))).collect()
    }
}

/// A fake CLI of `name` that writes its environment, and the login file its `CODEX_HOME` names when it has one, beside itself.
fn fake_cli(dir: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nenv > \"{d}/{name}.env\"\n[ -n \"$CODEX_HOME\" ] && cp \"$CODEX_HOME/auth.json\" \"{d}/{name}.login\"\nexit 0\n", d = dir.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn base() -> ChildEnv {
    ChildEnv::base_from([("PATH".to_string(), "/usr/bin:/bin".to_string())])
}

#[test]
fn given_no_mode_or_scripted_when_it_is_read_then_no_cli_runs_for_real_and_real_names_all_or_some() {
    // Given/When the mode is read
    let names = |v: Option<&str>| parse_mode(v).unwrap().iter().map(|c| c.name).collect::<Vec<_>>();
    // Then the default and scripted run none, real all three, and a list the named ones
    assert!(names(None).is_empty() && names(Some("scripted")).is_empty() && names(Some("")).is_empty());
    assert_eq!(names(Some("real")), ["claude", "codex", "opencode"]);
    assert_eq!(names(Some("real:codex, claude")), ["codex", "claude"]);
    // And a name that is no CLI, a repeat and an empty list are refused
    for bad in ["real:gemini", "real:claude,claude", "real:", "live"] {
        assert!(parse_mode(Some(bad)).is_err(), "{bad}");
    }
}

#[test]
fn given_the_three_real_clis_when_each_is_run_then_it_holds_only_its_own_credential_and_codex_reads_a_login_file() {
    // Given the three credentials resolved and a fake CLI per name that dumps what it holds
    let work = tempfile::tempdir().unwrap();
    let all: Vec<_> = CLIS.iter().collect();
    let resolved = Resolved::resolve(&all, &work.path().join("home"), &vault(&login(REFRESH))).unwrap();
    let bin = work.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    // When each CLI is started with the environment built for it
    for cli in &CLIS {
        let status = resolved.child_env(cli, base()).command(fake_cli(&bin, cli.name)).status().unwrap();
        assert!(status.success());
    }
    let dump = |name: &str| std::fs::read_to_string(bin.join(format!("{name}.env"))).unwrap();
    // Then claude holds its token and no other value, opencode its key, and each dump names no other CLI's credential
    assert!(dump("claude").contains(&format!("CLAUDE_CODE_OAUTH_TOKEN={CLAUDE}")) && !dump("claude").contains(OPENCODE) && !dump("claude").contains(REFRESH));
    assert!(dump("opencode").contains(&format!("OPENCODE_API_KEY={OPENCODE}")) && !dump("opencode").contains(CLAUDE) && !dump("opencode").contains(REFRESH));
    // And codex holds no value in its environment, only the path of the directory with its login, and the login there is the decoded file
    let codex = dump("codex");
    assert!(!codex.contains(CLAUDE) && !codex.contains(OPENCODE) && !codex.contains(REFRESH) && !codex.contains("CODEX_OAUTH_JSON_B64"), "{codex}");
    assert!(codex.contains(&format!("CODEX_HOME={}", work.path().join("home/.codex").display())));
    assert!(std::fs::read_to_string(bin.join("codex.login")).unwrap().contains(REFRESH));
    // And the executor's variables are the two env credentials, never the login
    let executor: Vec<_> = resolved.executor_env().into_iter().map(|(k, _)| k).collect();
    assert_eq!(executor, ["CLAUDE_CODE_OAUTH_TOKEN", "OPENCODE_API_KEY"]);
}

#[test]
fn given_a_missing_or_broken_credential_when_it_is_resolved_then_it_fails_naming_the_key_and_never_a_value() {
    let work = tempfile::tempdir().unwrap();
    let claude = [&CLIS[0]];
    // Given a vault that lacks the Claude token
    let lacking = |keys: &[&str]| keys.iter().map(|k| (k.to_string(), None)).collect::<HashMap<_, _>>();
    let missing = Resolved::resolve(&claude, work.path(), &lacking).err().unwrap();
    // Then the error names the key and the CLI
    assert!(missing.contains("CLAUDE_CODE_OAUTH_TOKEN") && missing.contains("claude"), "{missing}");
    // And a Codex value that is not a login fails naming the key without echoing it
    let secret = "not-base64-secret-value!!";
    let broken = Resolved::resolve(&[&CLIS[1]], work.path(), &move |k: &[&str]| k.iter().map(|n| (n.to_string(), Some(secret.to_string()))).collect()).err().unwrap();
    assert!(broken.contains("CODEX_OAUTH_JSON_B64") && !broken.contains(secret), "{broken}");
}

#[test]
fn given_a_rotated_login_when_the_next_run_resolves_it_then_it_starts_from_the_rotated_one() {
    // Given the vault's login before and after a run's rotation
    let work = tempfile::tempdir().unwrap();
    let codex = [&CLIS[1]];
    // When each is resolved for a run
    let _first = Resolved::resolve(&codex, &work.path().join("one"), &vault(&login("refresh-before-000"))).unwrap();
    let _second = Resolved::resolve(&codex, &work.path().join("two"), &vault(&login("refresh-after-0000"))).unwrap();
    // Then each run's CODEX_HOME holds the login of its vault, so the second does not hold the dead refresh token
    let held = |d: &str| std::fs::read_to_string(work.path().join(d).join(".codex/auth.json")).unwrap();
    assert!(held("one").contains("refresh-before-000") && held("two").contains("refresh-after-0000") && !held("two").contains("refresh-before-000"));
}

#[test]
fn given_an_output_holding_a_credential_when_it_is_scanned_then_the_file_and_key_are_named_and_a_clean_output_passes() {
    // Given the resolved credentials and an output tree where one file holds the Claude token and another the Codex refresh token
    let work = tempfile::tempdir().unwrap();
    let all: Vec<_> = CLIS.iter().collect();
    let resolved = Resolved::resolve(&all, &work.path().join("home"), &vault(&login(REFRESH))).unwrap();
    let out = work.path().join("out");
    std::fs::create_dir_all(out.join("suite")).unwrap();
    std::fs::write(out.join("suite/trace.json"), format!("{{\"note\": \"{CLAUDE}\"}}")).unwrap();
    std::fs::write(out.join("log.txt"), format!("login {REFRESH}")).unwrap();
    std::fs::write(out.join("clean.json"), "{\"methods\": [\"turn.start\"]}").unwrap();
    // When the tree is scanned
    let found = scan(&[&out], resolved.secrets());
    // Then each leak is named by file and key, no value is printed, and the clean file is not named
    assert!(found.iter().any(|f| f.contains("suite/trace.json") && f.contains("CLAUDE_CODE_OAUTH_TOKEN")), "{found:?}");
    assert!(found.iter().any(|f| f.contains("log.txt") && f.contains("CODEX_OAUTH_JSON_B64")), "{found:?}");
    assert!(found.iter().all(|f| !f.contains(CLAUDE) && !f.contains(REFRESH) && !f.contains("clean.json")), "{found:?}");
    // And a tree without a value scans clean
    std::fs::remove_file(out.join("suite/trace.json")).unwrap();
    std::fs::remove_file(out.join("log.txt")).unwrap();
    assert!(scan(&[&out], resolved.secrets()).is_empty());
}

#[test]
fn given_a_login_with_identifying_fields_when_it_is_scanned_then_only_credential_fields_count_and_a_logged_account_id_passes() {
    // Given a Codex login whose account id and plan are long strings, and a log that holds only those
    let work = tempfile::tempdir().unwrap();
    let json = r#"{"tokens":{"access_token":"access-token-value-0004","refresh_token":"refresh-token-value-000003","account_id":"account-identifier-0000-1111"},"plan":"chatgpt-plus-plan-long-name"}"#;
    let b64 = String::from_utf8(std::process::Command::new("sh").arg("-c").arg(format!("printf %s '{json}' | base64 -w0")).output().unwrap().stdout).unwrap();
    let codex: Vec<_> = CLIS.iter().filter(|c| c.name == "codex").collect();
    let resolved = Resolved::resolve(&codex, &work.path().join("home"), &vault(&b64)).unwrap();
    let out = work.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("log.txt"), "account account-identifier-0000-1111 on chatgpt-plus-plan-long-name").unwrap();
    // When it is scanned, and then again with the refresh token in the log
    let clean = scan(&[&out], resolved.secrets());
    std::fs::write(out.join("log.txt"), "refresh-token-value-000003").unwrap();
    let leak = scan(&[&out], resolved.secrets());
    // Then the identifiers pass and the refresh token is still held back
    assert!(clean.is_empty(), "{clean:?}");
    assert!(leak.iter().any(|f| f.contains("CODEX_OAUTH_JSON_B64")), "{leak:?}");
}

#[test]
fn given_the_scan_verb_when_real_clis_ran_then_a_leak_holds_the_output_back_with_exit_1_and_a_scripted_run_is_skipped() {
    // Given an output directory that holds the Claude token, and the key in the environment (no vault)
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("trace.json"), format!("{{\"x\": \"{CLAUDE}\"}}")).unwrap();
    let dir = work.path().display().to_string();
    let key = [("CLAUDE_CODE_OAUTH_TOKEN", Path::new(CLAUDE))];
    // When it is scanned for the real claude
    let leak = mimic(&["scan", "--clis", "real:claude", &dir], &key);
    // Then it is held back naming the file and the key, exit 1, with no value printed
    assert_eq!(leak.code, 1, "{}", leak.json);
    assert_eq!(leak.json["schema"], json!("mimic-scan/1"));
    assert!(leak.json["findings"][0].as_str().unwrap().contains("trace.json holds the value of CLAUDE_CODE_OAUTH_TOKEN"), "{}", leak.json);
    assert!(!leak.json.to_string().contains(CLAUDE) && !leak.stderr.contains(CLAUDE));
    // And a run whose key is not available fails naming it, a clean output passes, and a scripted run is skipped
    let missing = mimic(&["scan", "--clis", "real:opencode", &dir], &[]);
    assert_eq!(missing.code, 2);
    assert!(missing.json["error"].as_str().unwrap().contains("OPENCODE_GO_API_KEY"), "{}", missing.json);
    std::fs::write(work.path().join("trace.json"), "{}").unwrap();
    assert_eq!(mimic(&["scan", "--clis", "real:claude", &dir], &key).code, 0);
    let skipped = mimic(&["scan", "--clis", "scripted", &dir], &[]);
    assert_eq!((skipped.code, skipped.json["skipped"].is_string()), (0, true), "{}", skipped.json);
    // And a scan told nothing about the chain's mode fails instead of passing a possibly real run through unscanned
    let unsaid = mimic(&["scan", &dir], &[]);
    assert_eq!(unsaid.code, 2, "{}", unsaid.json);
    assert!(unsaid.json["error"].as_str().unwrap().contains("no mode"), "{}", unsaid.json);
}
