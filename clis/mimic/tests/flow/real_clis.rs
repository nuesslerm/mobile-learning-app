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
    std::fs::write(&path, format!("env > \"{d}/{name}.env\"\n[ -n \"$CODEX_HOME\" ] && cp \"$CODEX_HOME/auth.json\" \"{d}/{name}.login\"\nexit 0\n", d = dir.display())).unwrap();
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
        // run through `sh`, never the file itself: executing a file another test thread's forked child still holds open for writing fails with ETXTBSY
        let status = resolved.child_env(cli, base()).command("/bin/sh").arg(fake_cli(&bin, cli.name)).status().unwrap();
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

#[test]
fn given_real_clis_when_a_command_is_launched_then_it_gets_the_resolved_environment_only_and_stdout_stays_one_json_object() {
    // Given a fake vault (the caller's environment) holding the Claude token and the Codex login, a stray credential no CLI was asked for, and a directory of CLIs
    let work = tempfile::tempdir().unwrap();
    let (out, bins, dump) = (work.path().join("out"), work.path().join("bins"), work.path().join("dump"));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(&bins).unwrap();
    let login_b64 = login(REFRESH);
    let env = [("CLAUDE_CODE_OAUTH_TOKEN", Path::new(CLAUDE)), ("CODEX_OAUTH_JSON_B64", Path::new(&login_b64)), ("OPENCODE_API_KEY", Path::new("stray-key-value-0009"))];
    let script = format!("echo noise; env > {d}; cat \"$CODEX_HOME/auth.json\" > {d}.login; ls \"$CODEX_HOME\" > {d}.home; echo \"$CODEX_HOME\" > {d}.path", d = dump.display());
    let (out_s, bins_s, dump_s) = (out.display().to_string(), bins.display().to_string(), dump.display().to_string());
    // When it is launched for the real claude and codex through the verb
    let run = mimic(&["exec", "--clis", "real:claude,codex", "--cli-dir", &bins_s, "--scan", &out_s, "--", "sh", "-c", &script], &env);
    // Then it passed, clean, naming the CLIs and no value
    assert_eq!(run.code, 0, "{}\n{}", run.json, run.stderr);
    assert_eq!((run.json["schema"].clone(), run.json["ok"].clone(), run.json["exit"].clone()), (json!("mimic-exec/1"), json!(true), json!(0)), "{}", run.json);
    assert_eq!(run.json["real_clis"], json!(["claude", "codex"]));
    assert!(!run.json.to_string().contains(CLAUDE) && !run.json.to_string().contains(REFRESH) && !run.stderr.contains(CLAUDE) && !run.stderr.contains(REFRESH));
    // And the command's own stdout went to the verb's stderr
    assert!(run.stderr.contains("noise"), "{}", run.stderr);
    // And the command held the Claude token, the names the rig reads, the CLI directory and a Codex home with the login, and neither the stray key nor the login's value as a variable
    let held = std::fs::read_to_string(&dump).unwrap();
    assert!(held.contains(&format!("CLAUDE_CODE_OAUTH_TOKEN={CLAUDE}")) && held.contains("T3_CHAIN_REAL_CLIS=claude,codex"), "{held}");
    assert!(held.contains("T3_CHAIN_EXECUTOR_ENV=CLAUDE_CODE_OAUTH_TOKEN,CODEX_HOME") && held.contains(&format!("MIMIC_REAL_CLI_DIR={bins_s}")), "{held}");
    assert!(!held.contains("stray-key-value-0009") && !held.contains("CODEX_OAUTH_JSON_B64") && !held.contains(REFRESH), "{held}");
    assert!(std::fs::read_to_string(format!("{dump_s}.login")).unwrap().contains(REFRESH));
    // And the home it was given is gone once the run ended
    let home = std::fs::read_to_string(format!("{dump_s}.path")).unwrap();
    assert!(!Path::new(home.trim()).exists(), "{home}");
}

#[test]
fn given_a_launched_run_when_it_leaks_or_fails_then_the_output_is_held_back_and_the_exit_is_reported() {
    // Given the Claude token in the environment and a command that writes it into the output directory
    let work = tempfile::tempdir().unwrap();
    let (out, bins) = (work.path().join("out"), work.path().join("bins"));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(&bins).unwrap();
    let (out_s, bins_s) = (out.display().to_string(), bins.display().to_string());
    let env = [("CLAUDE_CODE_OAUTH_TOKEN", Path::new(CLAUDE))];
    let leak = mimic(&["exec", "--clis", "real:claude", "--cli-dir", &bins_s, "--scan", &out_s, "--", "sh", "-c", "echo \"$CLAUDE_CODE_OAUTH_TOKEN\" > \"$0/trace.json\"", &out_s], &env);
    // Then it is held back naming the file and key, exit 1, no value printed
    assert_eq!((leak.code, leak.json["ok"].clone()), (1, json!(false)), "{}", leak.json);
    assert!(leak.json["findings"][0].as_str().unwrap().contains("trace.json holds the value of CLAUDE_CODE_OAUTH_TOKEN"), "{}", leak.json);
    assert!(!leak.json.to_string().contains(CLAUDE) && !leak.stderr.contains(CLAUDE));
    // And a failing command is exit 1 with its own exit code reported and a clean scan
    std::fs::remove_file(out.join("trace.json")).unwrap();
    let failed = mimic(&["exec", "--clis", "real:claude", "--cli-dir", &bins_s, "--scan", &out_s, "--", "sh", "-c", "exit 7"], &env);
    assert_eq!((failed.code, failed.json["exit"].clone(), failed.json["findings"].clone()), (1, json!(7), json!([])), "{}", failed.json);
}

#[test]
fn given_a_launcher_that_cannot_start_when_the_key_mode_command_or_directory_is_missing_then_it_fails_with_exit_2_before_running_anything() {
    // Given a marker the command would leave if it ran
    let work = tempfile::tempdir().unwrap();
    let marker = work.path().join("ran");
    let touch = format!("touch {}", marker.display());
    let bins = work.path().display().to_string();
    // When the vault lacks the key, then the error names it and the command never ran
    let no_key = mimic(&["exec", "--clis", "real:opencode", "--cli-dir", &bins, "--", "sh", "-c", &touch], &[]);
    assert_eq!(no_key.code, 2);
    assert!(no_key.json["error"].as_str().unwrap().contains("OPENCODE_GO_API_KEY"), "{}", no_key.json);
    // And no mode, no command and no directory of binaries are each refused
    let no_mode = mimic(&["exec", "--", "sh", "-c", &touch], &[]);
    assert!(no_mode.code == 2 && no_mode.json["error"].as_str().unwrap().contains("no mode"), "{}", no_mode.json);
    let no_command = mimic(&["exec", "--clis", "scripted"], &[]);
    assert_eq!(no_command.code, 2, "{}", no_command.json);
    let no_dir = mimic(&["exec", "--clis", "real:claude", "--", "sh", "-c", &touch], &[("CLAUDE_CODE_OAUTH_TOKEN", Path::new(CLAUDE))]);
    assert!(no_dir.code == 2 && no_dir.json["error"].as_str().unwrap().contains("directory"), "{}", no_dir.json);
    assert!(!marker.exists());
}

#[test]
fn given_the_scripted_mode_when_a_command_is_launched_then_nothing_is_resolved_and_the_rig_is_told_no_real_cli() {
    // Given a stale real-CLI list in the caller's environment and no credentials anywhere
    let work = tempfile::tempdir().unwrap();
    let dump = work.path().join("dump");
    let out = work.path().display().to_string();
    // When the command is launched as scripted
    let run = mimic(&["exec", "--clis", "scripted", "--scan", &out, "--", "sh", "-c", &format!("echo \"[$T3_CHAIN_REAL_CLIS][$T3_CHAIN_EXECUTOR_ENV]\" > {}", dump.display())], &[("T3_CHAIN_REAL_CLIS", Path::new("claude"))]);
    // Then it passes without a key, the rig sees an empty list, and nothing was scanned for
    assert_eq!((run.code, run.json["real_clis"].clone(), run.json["findings"].clone()), (0, json!([]), json!([])), "{}", run.json);
    assert_eq!(std::fs::read_to_string(&dump).unwrap().trim(), "[][]");
}

#[test]
fn given_a_run_that_rotates_the_codex_login_when_it_ends_then_the_rotation_is_reported_without_a_value() {
    // Given the Codex login in the environment and a command that rewrites it as Codex does on a refresh
    let work = tempfile::tempdir().unwrap();
    let bins = work.path().display().to_string();
    let login_b64 = login(REFRESH);
    let env = [("CODEX_OAUTH_JSON_B64", Path::new(&login_b64))];
    // When it runs
    let run = mimic(&["exec", "--clis", "real:codex", "--cli-dir", &bins, "--", "sh", "-c", "echo '{\"tokens\":{}}' > \"$CODEX_HOME/auth.json\""], &env);
    // Then the rotation is flagged with the step to write it back, and no value is printed
    assert_eq!((run.code, run.json["codex_login_rotated"].clone()), (0, json!(true)), "{}", run.json);
    assert!(run.json["warnings"][0].as_str().unwrap().contains("rotate-login codex"), "{}", run.json);
    assert!(!run.json.to_string().contains(REFRESH) && !run.stderr.contains(REFRESH));
}

#[test]
fn given_a_command_that_prints_a_credential_when_it_is_launched_then_the_output_is_withheld_and_the_run_held_back() {
    // Given the Claude token and a command that echoes it to stdout and stderr
    let work = tempfile::tempdir().unwrap();
    let bins = work.path().join("bins");
    std::fs::create_dir_all(&bins).unwrap();
    let bins_s = bins.display().to_string();
    let env = [("CLAUDE_CODE_OAUTH_TOKEN", Path::new(CLAUDE))];
    // When it is launched for the real claude
    let run = mimic(&["exec", "--clis", "real:claude", "--cli-dir", &bins_s, "--", "sh", "-c", "echo \"out $CLAUDE_CODE_OAUTH_TOKEN\"; echo \"err $CLAUDE_CODE_OAUTH_TOKEN\" >&2"], &env);
    // Then it is held back naming the key, and neither the verb's stdout nor its stderr carries the value
    assert_eq!((run.code, run.json["ok"].clone()), (1, json!(false)), "{}", run.json);
    assert!(run.json["findings"][0].as_str().unwrap().contains("CLAUDE_CODE_OAUTH_TOKEN"), "{}", run.json);
    assert!(!run.json.to_string().contains(CLAUDE) && !run.stderr.contains(CLAUDE) && run.stderr.contains("[withheld: CLAUDE_CODE_OAUTH_TOKEN]"), "{}", run.stderr);
}

#[test]
fn given_a_codex_login_when_it_is_resolved_then_its_directory_and_file_are_owner_only() {
    // Given a vault holding a login
    use std::os::unix::fs::PermissionsExt;
    let work = tempfile::tempdir().unwrap();
    // When it is resolved
    let _held = Resolved::resolve(&[&CLIS[1]], &work.path().join("home"), &vault(&login(REFRESH))).unwrap();
    // Then the directory is 0700 and the login 0600
    let mode = |p: &str| std::fs::metadata(work.path().join(p)).unwrap().permissions().mode() & 0o777;
    assert_eq!((mode("home/.codex"), mode("home/.codex/auth.json")), (0o700, 0o600));
}

const CI_KEY: &str = "CODEX_CI_OAUTH_JSON_B64";
const CI_REFRESH: &str = "ci-refresh-token-value-0010";
const CI_ROTATED: &str = "ci-rotated-refresh-token-0011";

fn ci_login(refresh: &str, account: &str) -> String {
    let json = format!(r#"{{"tokens":{{"access_token":"ci-access-token-value-0012","refresh_token":"{refresh}","account_id":"{account}"}}}}"#);
    let out = std::process::Command::new("sh").arg("-c").arg(format!("printf %s '{json}' | base64 -w0")).output().unwrap();
    String::from_utf8(out.stdout).unwrap()
}

/// The vault as `bws` serves it for the CI deploy credential: the listing it was given, every call logged beside it.
struct Vault {
    dir: tempfile::TempDir,
}

impl Vault {
    fn holding(login: Option<&str>) -> Vault {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        let list = login.map_or_else(|| "[]".to_string(), |l| format!(r#"[{{"id":"cx","key":"{CI_KEY}","value":"{l}"}}]"#));
        std::fs::write(dir.path().join("list.json"), list).unwrap();
        let stub = dir.path().join("bin/bws");
        std::fs::write(&stub, "#!/bin/sh\necho \"$* | token=$BWS_ACCESS_TOKEN\" >> \"$STUB_DIR/bws.log\"\nif [ \"$1 $2\" = \"secret list\" ]; then cat \"$STUB_DIR/list.json\"; fi\nexit 0\n").unwrap();
        std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        Vault { dir }
    }

    /// `wf mimic exec --clis real:codex-ci` for `script`, with this vault and the CI deploy credential in the verb's environment alone.
    fn exec(&self, extra: &[&str], script: &str) -> Run {
        self.launch(extra, script, None)
    }

    /// The same as a GitHub Actions step runs it: `GITHUB_ACTIONS=true` and the verb's stderr redirected into `log`.
    fn exec_on_a_runner(&self, extra: &[&str], script: &str, log: &Path) -> Run {
        self.launch(extra, script, Some(log))
    }

    fn launch(&self, extra: &[&str], script: &str, runner_log: Option<&Path>) -> Run {
        let bins = self.dir.path().join("cli");
        std::fs::create_dir_all(&bins).unwrap();
        let (bins_s, d) = (bins.display().to_string(), self.dir.path().display().to_string());
        let path = format!("{d}/bin:{}", std::env::var("PATH").unwrap_or_default());
        let state = self.dir.path().join("state");
        let mut env = vec![("PATH", Path::new(&path)), ("SECRETS_BACKEND", Path::new("bitwarden")), ("BWS_CI_DEPLOY_ACCESS_TOKEN", Path::new("ci-token")), ("STUB_DIR", Path::new(&d)), ("XDG_STATE_HOME", state.as_path())];
        let mut args = vec!["exec", "--clis", "real:codex-ci", "--cli-dir", &bins_s];
        args.extend_from_slice(extra);
        args.extend(["--", "sh", "-c", script]);
        match runner_log {
            Some(log) => {
                env.push(("GITHUB_ACTIONS", Path::new("true")));
                mimic_logging_stderr(&args, &env, log)
            }
            None => mimic(&args, &env),
        }
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("bws.log")).map(|t| t.lines().filter(|l| !l.starts_with("secret list")).map(String::from).collect()).unwrap_or_default()
    }
}

#[test]
fn given_the_second_codex_login_when_it_is_named_then_real_does_not_include_it_and_it_cannot_run_beside_the_first() {
    // Given/When the modes are read
    let names = |v: &str| parse_mode(Some(v)).map(|c| c.iter().map(|c| c.name).collect::<Vec<_>>());
    // Then real stays the three, the second login is named on its own, and the two Codex logins together are refused
    assert_eq!(names("real").unwrap(), ["claude", "codex", "opencode"]);
    assert_eq!(names("real:codex-ci").unwrap(), ["codex-ci"]);
    assert_eq!(names("real:claude,codex-ci").unwrap(), ["claude", "codex-ci"]);
    assert!(names("real:codex,codex-ci").unwrap_err().contains("CODEX_HOME"));
}

#[test]
fn given_the_second_codex_login_in_the_vault_when_a_run_starts_then_it_is_restored_owner_only_where_the_provider_layout_reads_it_and_the_command_gets_no_vault_token() {
    // Given the vault holding the login, and a command that reports where CODEX_HOME is, the mode of the login and what else it holds
    let vault = Vault::holding(Some(&ci_login(CI_REFRESH, "acct-1")));
    let dump = vault.dir.path().join("dump");
    let script = format!("echo \"$CODEX_HOME\" > {d}.home; stat -c %a \"$CODEX_HOME/auth.json\" > {d}.mode; stat -c %a \"$CODEX_HOME\" > {d}.dirmode; cat \"$CODEX_HOME/auth.json\" > {d}.login; env > {d}.env", d = dump.display());
    // When it is launched
    let run = vault.exec(&[], &script);
    // Then it passed naming the second login, and the command's CODEX_HOME is `<home>/providers/codex`, the directory 0700 and the file 0600 holding the vault's login
    assert_eq!((run.code, run.json["ok"].clone(), run.json["real_clis"].clone()), (0, json!(true), json!(["codex-ci"])), "{}\n{}", run.json, run.stderr);
    let d = dump.display().to_string();
    let home = std::fs::read_to_string(format!("{d}.home")).unwrap();
    assert!(home.trim().ends_with("/providers/codex"), "{home}");
    assert_eq!((std::fs::read_to_string(format!("{d}.mode")).unwrap().trim(), std::fs::read_to_string(format!("{d}.dirmode")).unwrap().trim()), ("600", "700"));
    assert!(std::fs::read_to_string(format!("{d}.login")).unwrap().contains(CI_REFRESH));
    // And the command held no vault token and no login as a variable, and the directory is gone once the run ended
    let held = std::fs::read_to_string(format!("{d}.env")).unwrap();
    assert!(!held.contains("BWS_") && !held.contains("ci-token") && !held.contains(CI_KEY) && !held.contains(CI_REFRESH), "{held}");
    assert!(!Path::new(home.trim()).exists());
}

#[test]
fn given_a_run_that_refreshes_the_login_when_it_ends_then_it_is_written_back_to_the_one_key_and_no_value_is_printed() {
    // Given the vault's login and a command that refreshes it as codex does
    let vault = Vault::holding(Some(&ci_login(CI_REFRESH, "acct-1")));
    let rotated = ci_login(CI_ROTATED, "acct-1");
    let plain = String::from_utf8(std::process::Command::new("sh").arg("-c").arg(format!("printf %s '{rotated}' | base64 -d")).output().unwrap().stdout).unwrap();
    // When the run ends, the command having printed nothing of the login
    let run = vault.exec(&[], &format!("printf %s '{plain}' > \"$CODEX_HOME/auth.json\""));
    // Then the new login was written to that one secret by id with the CI credential, the verb says updated, and no output of the verb holds either refresh token
    assert_eq!((run.code, run.json["ok"].clone(), run.json["login_written_back"].clone(), run.json["codex_login_rotated"].clone()), (0, json!(true), json!({CI_KEY: "updated"}), json!(true)), "{}\n{}", run.json, run.stderr);
    assert_eq!(vault.calls(), [format!("secret edit --value {rotated} --output none -- cx | token=ci-token")]);
    for printed in [run.json.to_string(), run.stderr.clone()] {
        assert!(!printed.contains(CI_REFRESH) && !printed.contains(CI_ROTATED) && !printed.contains(&rotated), "{printed}");
    }
}

#[test]
fn given_a_run_that_leaves_the_login_alone_or_fails_when_it_ends_then_nothing_is_written_unless_the_login_changed() {
    // Given the vault's login
    let vault = Vault::holding(Some(&ci_login(CI_REFRESH, "acct-1")));
    // When a run leaves the file as it was
    let calm = vault.exec(&[], "true");
    // Then nothing was written and no write-back is reported
    assert_eq!((calm.code, calm.json["login_written_back"].clone(), calm.json["codex_login_rotated"].clone()), (0, json!({}), json!(false)), "{}", calm.json);
    assert!(vault.calls().is_empty(), "{:?}", vault.calls());
    // And a run that refreshed the login and then failed still writes it back, because the old refresh token is dead either way
    let rotated = ci_login(CI_ROTATED, "acct-1");
    let plain = String::from_utf8(std::process::Command::new("sh").arg("-c").arg(format!("printf %s '{rotated}' | base64 -d")).output().unwrap().stdout).unwrap();
    let failed = vault.exec(&[], &format!("printf %s '{plain}' > \"$CODEX_HOME/auth.json\"; exit 3"));
    assert_eq!((failed.code, failed.json["exit"].clone(), failed.json["login_written_back"].clone()), (1, json!(3), json!({CI_KEY: "updated"})), "{}", failed.json);
    assert_eq!(vault.calls().len(), 1);
}

#[test]
fn given_a_refreshed_login_of_another_account_when_it_ends_then_it_is_refused_and_the_run_fails_naming_the_step_to_mint_it_again() {
    // Given the vault's login and a command that leaves the login of another account
    let vault = Vault::holding(Some(&ci_login(CI_REFRESH, "acct-1")));
    let other = ci_login(CI_ROTATED, "acct-2");
    let plain = String::from_utf8(std::process::Command::new("sh").arg("-c").arg(format!("printf %s '{other}' | base64 -d")).output().unwrap().stdout).unwrap();
    // When the run ends
    let run = vault.exec(&[], &format!("printf %s '{plain}' > \"$CODEX_HOME/auth.json\""));
    // Then nothing was written, the run failed, and the message names the step and no value
    assert_eq!((run.code, run.json["ok"].clone(), run.json["login_written_back"].clone()), (1, json!(false), json!({CI_KEY: "refused"})), "{}", run.json);
    assert!(vault.calls().is_empty() && run.json["warnings"][0].as_str().unwrap().contains("wf secret login codex-ci"), "{}", run.json);
    assert!(!run.json.to_string().contains(CI_ROTATED) && !run.stderr.contains(CI_ROTATED));
}

#[test]
fn given_no_value_for_the_second_login_when_the_run_is_launched_then_it_is_skipped_with_the_reason_when_asked_and_fails_otherwise() {
    // Given a vault that holds nothing under the key, and a marker the command would leave
    let vault = Vault::holding(None);
    let marker = vault.dir.path().join("ran");
    let touch = format!("touch {}", marker.display());
    // When it is launched asking to skip an empty credential
    let skipped = vault.exec(&["--skip-when-empty"], &touch);
    // Then it passed without running, the message names the key and the step to mint it, and it says so on stderr too
    assert_eq!((skipped.code, skipped.json["ok"].clone(), skipped.json["ran"].clone()), (0, json!(true), json!(false)), "{}", skipped.json);
    let why = skipped.json["skipped"].as_str().unwrap();
    assert!(why.contains(CI_KEY) && why.contains("wf secret login codex-ci") && skipped.stderr.contains(why), "{why}");
    assert!(!marker.exists());
    // And a key held with an empty value is skipped the same way
    let blank = Vault::holding(Some(""));
    let blank_run = blank.exec(&["--skip-when-empty"], &touch);
    assert_eq!((blank_run.code, blank_run.json["ran"].clone()), (0, json!(false)), "{}", blank_run.json);
    assert!(!marker.exists());
    // And without the flag the missing value fails with exit 2 and the command never ran
    let failed = vault.exec(&[], &touch);
    assert_eq!(failed.code, 2, "{}", failed.json);
    assert!(failed.json["error"].as_str().unwrap().contains(CI_KEY) && !marker.exists(), "{}", failed.json);
}

/// What the real Codex proof prints: cargo's lines and the test's result, with no secret in them.
const PROOF_OUTPUT: &str = "echo '   Compiling remote-chat-agent-host v0.1.0' >&2; echo 'running 1 test' >&2; echo 'test result: ok. 1 passed; 0 failed' >&2";

#[test]
fn given_a_step_on_a_runner_when_a_proof_with_the_normal_output_ends_then_its_log_beside_the_scanned_files_holds_no_value_and_the_run_passes() {
    // Given the vault's login, a scan directory that holds the log the step redirects the verb's stderr into, and the verb running as on GitHub Actions
    let vault = Vault::holding(Some(&ci_login(CI_REFRESH, "acct-1")));
    let proof = vault.dir.path().join("codex-proof");
    std::fs::create_dir_all(&proof).unwrap();
    let log = proof.join("exec.log");
    let scan = proof.display().to_string();
    // When the proof runs and prints what the real test prints
    let run = vault.exec_on_a_runner(&["--scan", &scan], PROOF_OUTPUT, &log);
    // Then the run passed with no finding: the verb did not write the login into the log itself
    assert_eq!((run.code, run.json["ok"].clone(), run.json["findings"].clone(), run.json["scanned"].clone()), (0, json!(true), json!([]), json!(1)), "{}\n{}", run.json, run.stderr);
    // And the log holds the proof's lines and neither the login, its tokens, nor a mask announcement of them
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("test result: ok") && !text.contains("::add-mask::"), "{text}");
    let login_b64 = ci_login(CI_REFRESH, "acct-1");
    assert!(!text.contains(&login_b64) && !text.contains(CI_REFRESH) && !text.contains("ci-access-token-value-0012"), "{text}");
}

#[test]
fn given_a_step_on_a_runner_when_a_proof_prints_or_writes_the_login_then_the_run_is_held_back_naming_the_finding_and_the_log_holds_no_value() {
    // Given the vault's login, the scan directory with the step's log, and the verb running as on GitHub Actions
    let vault = Vault::holding(Some(&ci_login(CI_REFRESH, "acct-1")));
    let proof = vault.dir.path().join("codex-proof");
    std::fs::create_dir_all(&proof).unwrap();
    let log = proof.join("exec.log");
    let scan = proof.display().to_string();
    // When the proof prints a refresh token, and (second run) writes the login into a file of the scanned directory
    let printed = vault.exec_on_a_runner(&["--scan", &scan], &format!("echo 'refreshed {CI_REFRESH}' >&2"), &log);
    let planted = vault.exec_on_a_runner(&["--scan", &scan], &format!("echo '{CI_REFRESH}' > \"{scan}/result.json\""), &log);
    // Then each is held back with exit 1, the first naming the output, the second the file, and both the key
    assert_eq!((printed.code, printed.json["ok"].clone()), (1, json!(false)), "{}", printed.json);
    assert_eq!(printed.json["findings"], json!([format!("the command's own output holds the value of {CI_KEY}")]));
    assert_eq!((planted.code, planted.json["ok"].clone()), (1, json!(false)), "{}", planted.json);
    assert_eq!(planted.json["findings"], json!([format!("result.json holds the value of {CI_KEY}")]));
    // And what the verb let into the log is redacted: the token is not there
    assert!(!std::fs::read_to_string(&log).unwrap().contains(CI_REFRESH));
    assert!(!printed.json.to_string().contains(CI_REFRESH) && !planted.json.to_string().contains(CI_REFRESH));
}
