//! `wf secret login <tool>` by the tool's own link: the codex and claude commands are stubs on PATH (claude's under the real `script`
//! pty, as the descriptor runs it), the vault is a `bws` stub, HOME and the private runtime directory are throwaway.
use crate::support::{create_sh_stub, run_wf, spawn_wf, wait_until, RunOpts, RunResult};
use base64::Engine;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TOKEN: &str = "sk-ant-oat01-FAKEfakeFAKE_0123456789-abcdefghijklmnop";
const CODE: &str = "Zq81-fake-code#state-9x";
const AUTH_JSON: &str = "{\"auth_mode\":\"chatgpt\",\"tokens\":{\"access_token\":\"fake-access\",\"refresh_token\":\"fake-refresh\"}}";
const CLAUDE_URL: &str = "https://claude.ai/oauth/authorize?code=true&client_id=fake&state=abc";

const VAULT: &str = r#"echo "$*" >> "$HOME/bws.log"
if [ "$1 $2" = "secret list" ]; then echo '[{"id":"c1","key":"CLAUDE_CODE_OAUTH_TOKEN","value":"old"},{"id":"x1","key":"CODEX_OAUTH_JSON_B64","value":"old"},{"id":"x2","key":"CODEX_CI_OAUTH_JSON_B64","value":"old"}]'; exit 0; fi
exit 0"#;

const VAULT_REJECTING: &str = r#"echo "$*" >> "$HOME/bws.log"
if [ "$1 $2" = "secret list" ]; then echo '[{"id":"x1","key":"CODEX_OAUTH_JSON_B64","value":"old"}]'; exit 0; fi
if [ "$1 $2" = "secret edit" ]; then echo "rejected: $*" >&2; exit 1; fi
exit 0"#;

/// A login that prints the device link split across two writes and the one-time code amid colour codes, then writes auth.json.
const CODEX_OK: &str = r#"echo "$CODEX_HOME" >> "$HOME/codex-home.log"
echo "$*" >> "$HOME/codex-args.log"
printf 'Welcome to Codex\n\nFollow these steps to sign in with ChatGPT using device code authorization:\n\n1. Open this link in your browser and sign in to your account\n   \033[94mhttps://auth.openai.com/codex/de'
sleep 0.3
printf 'vice\033[0m\n\n2. Enter this one-time code (expires in 15 minutes)\n   \033[94mVUFO-7YUD2\033[0m\n\nDevice codes are a common phishing target. Never share this code.\n'
sleep 0.3
printf '%s' "$AUTH_JSON" > "$CODEX_HOME/auth.json"
echo "Successfully logged in""#;

struct Bench {
    dir: tempfile::TempDir,
    home: PathBuf,
    bin: PathBuf,
    runtime: PathBuf,
}

impl Bench {
    fn new() -> Bench {
        let dir = tempfile::Builder::new().prefix("wf-flow-link-").tempdir().unwrap();
        let (home, bin, runtime) = (dir.path().join("home"), dir.path().join("bin"), dir.path().join("runtime"));
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&bin).unwrap();
        create_sh_stub(&bin.join("bws"), VAULT);
        Bench { dir, home, bin, runtime }
    }

    fn stub(&self, name: &str, script: &str) {
        create_sh_stub(&self.bin.join(name), script);
    }

    /// The environment of a login: no listener credentials at all, a vault that takes writes, and the private directory under the bench.
    fn opts(&self, env: &[(&str, &str)]) -> RunOpts {
        let mut opts = RunOpts::default()
            .bin_dir(&self.bin)
            .cwd(self.dir.path())
            .clean_env()
            .env("HOME", &self.home.display().to_string())
            .env("SECRETS_BACKEND", "bitwarden")
            .env("SECRETS_LOG_BETTERSTACK", "false")
            .env("BWS_ADMIN_ACCESS_TOKEN", "edit")
            .env("WF_LOGIN_RUNTIME_DIR", &self.runtime.display().to_string())
            .env("AUTH_JSON", AUTH_JSON)
            .env("TOKEN", TOKEN)
            .env("WF_LOGIN_TIMEOUT_MS", "20000");
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        opts
    }

    fn login(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut full = vec!["secret", "login"];
        full.extend_from_slice(args);
        run_wf(&full, &self.opts(env))
    }

    fn bws_log(&self) -> String {
        std::fs::read_to_string(self.home.join("bws.log")).unwrap_or_default()
    }

    fn edits(&self) -> Vec<String> {
        self.bws_log().lines().filter(|l| l.contains("secret edit")).map(String::from).collect()
    }

    /// What is left under the private directory once a login ended.
    fn leftovers(&self) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(self.runtime.join("wf-login")).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        found.sort();
        found
    }

    fn logged(&self, file: &str) -> Vec<String> {
        std::fs::read_to_string(self.home.join(file)).map(|t| t.lines().map(String::from).collect()).unwrap_or_default()
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Alive and not a zombie waiting to be reaped.
fn running(pid: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.rsplit_once(')').is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z')))
}

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

/// A claude whose `setup-token` prints the authorization link as the real one does (a hyperlink escape, then the text), waits for the
/// code typed into its terminal, and prints the token.
fn claude_stub(after_code: &str) -> String {
    format!(
        r#"stty -echo
printf 'Browser didn'"'"'t open? Use the URL below to sign in\r\n\r\n\033]8;id=w1;{CLAUDE_URL}\007\033[37m\033[2m{CLAUDE_URL}\033[39m\033[22m\033]8;;\007\r\n\r\nPaste code here if prompted\r\n'
read -r code
printf '%s' "$code" > "$HOME/claude-code.log"
{after_code}"#
    )
}

#[test]
fn a_person_signs_codex_in_by_its_device_link_and_the_login_lands_in_the_vault_without_touching_the_machine() {
    // Given a codex whose device login writes auth.json into the CODEX_HOME it is given, and a machine with no listener credentials
    let b = Bench::new();
    b.stub("codex", CODEX_OK);

    // When the person runs the login for codex
    let r = b.login(&["codex"], &[]);

    // Then they see the link whole and the code on its own line, and the base64 of auth.json is stored under codex's key
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(
        r.stdout,
        "codex: open this link on any device and sign in:\nhttps://auth.openai.com/codex/device\none-time code:\nVUFO-7YUD2\nWaiting for the login to finish...\ncodex: authenticated ✓ (vault CODEX_OAUTH_JSON_B64)\n"
    );
    let edits = b.edits();
    assert!(edits.len() == 1 && edits[0].contains(&format!("--value {}", b64(AUTH_JSON))) && edits[0].ends_with(" x1"), "{edits:?}");
    // and the login ran in a private CODEX_HOME that is gone now, with the credential file store forced, while the real ~/.codex was never made
    let homes = b.logged("codex-home.log");
    assert_eq!(homes.len(), 1);
    assert!(homes[0].starts_with(&b.runtime.display().to_string()) && !Path::new(&homes[0]).exists(), "{homes:?}");
    assert_eq!(b.logged("codex-args.log"), ["login --device-auth -c cli_auth_credentials_store=file"]);
    assert!(!b.home.join(".codex").exists() && b.leftovers().is_empty(), "{:?}", b.leftovers());
    // and no token or file content reached the output
    for secret in ["fake-refresh", "fake-access", &b64(AUTH_JSON)] {
        assert!(!r.stdout.contains(secret) && !r.stderr.contains(secret), "{secret} was printed");
    }

    // When the other codex login runs, and the first again with --copy
    let ci = b.login(&["codex-ci"], &[]);
    let copied = b.login(&["codex", "--copy"], &[]);
    // Then codex-ci lands in its own secret, and --copy also writes the file to the machine's own ~/.codex, owner-only
    assert!(ci.exit_code == 0 && ci.stdout.ends_with("codex-ci: authenticated ✓ (vault CODEX_CI_OAUTH_JSON_B64)\n"), "{}{}", ci.stdout, ci.stderr);
    assert!(b.edits()[1].ends_with(" x2"), "{:?}", b.edits());
    assert!(copied.stdout.ends_with("codex: authenticated ✓ (vault CODEX_OAUTH_JSON_B64; credential copied to this machine)\n"), "{}{}", copied.stdout, copied.stderr);
    let file = b.home.join(".codex/auth.json");
    assert_eq!((std::fs::read_to_string(&file).unwrap(), mode(&file)), (AUTH_JSON.to_string(), 0o600));

    // When --vnc asks for the desktop's listener on a machine that has no credentials for it
    let vnc = b.login(&["codex", "--vnc"], &[]);
    // Then it goes to the listener path and says so, which the default never needed
    assert!(vnc.exit_code == 1 && vnc.stderr.contains("VNC_AUTH_TRIGGER_SECRET and VNC_AUTH_OWNER_CHAT_ID must be set"), "{}", vnc.stderr);
}

#[test]
fn a_codex_login_that_fails_stops_or_never_finishes_stores_nothing_and_says_why() {
    let b = Bench::new();

    // When the login command exits non-zero, exits 0 without a credential file, and cannot be found
    b.stub("codex", "echo 'Error: device code request failed with status 503' >&2\nexit 1");
    let failed = b.login(&["codex"], &[]);
    b.stub("codex", "echo 'https://auth.openai.com/codex/device'\necho");
    let empty = b.login(&["codex"], &[]);
    std::fs::remove_file(b.bin.join("codex")).unwrap();
    let missing = b.login(&["codex"], &[]);
    // Then each fails with the command's own words, nothing is stored and no directory is left
    assert_eq!(failed.exit_code, 1);
    assert!(failed.stderr.contains("codex: login failed (exit 1): Error: device code request failed with status 503"), "{}", failed.stderr);
    assert!(empty.exit_code == 1 && empty.stderr.contains("codex: the login finished but left no auth.json to store"), "{}", empty.stderr);
    assert!(missing.exit_code == 1 && missing.stderr.contains("codex: login failed (exit 127):") && missing.stderr.contains("not found"), "{}", missing.stderr);
    assert!(b.edits().is_empty() && b.leftovers().is_empty());

    // When the login never finishes before the timeout
    b.stub("codex", "echo $$ > \"$HOME/codex.pid\"\necho 'https://auth.openai.com/codex/device'\necho\nsleep 30");
    let hung = b.login(&["codex"], &[("WF_LOGIN_TIMEOUT_MS", "800")]);
    // Then it says it timed out, the command and what it started are stopped, and nothing is stored
    assert_eq!(hung.exit_code, 1);
    assert!(hung.stderr.contains("codex: login did not complete within the timeout (0.") && hung.stderr.contains(" seconds) -- run the same command again for a fresh link"), "{}", hung.stderr);
    let pid = std::fs::read_to_string(b.home.join("codex.pid")).unwrap();
    assert!(!running(pid.trim()), "the login command is still running");
    assert!(b.edits().is_empty() && b.leftovers().is_empty());

    // When the vault has no secret for the login's key, and when it rejects the write with the value in its error
    b.stub("codex", CODEX_OK);
    b.stub("bws", "echo \"$*\" >> \"$HOME/bws.log\"\nif [ \"$1 $2\" = \"secret list\" ]; then echo '[]'; exit 0; fi\nexit 0");
    let no_key = b.login(&["codex"], &[]);
    b.stub("bws", VAULT_REJECTING);
    let rejected = b.login(&["codex"], &[]);
    // Then the first names `wf secret create`, the second names the key, and neither prints the credential
    assert!(no_key.exit_code == 1 && no_key.stderr.contains("the vault has no secret CODEX_OAUTH_JSON_B64 to update; create it first with `wf secret create CODEX_OAUTH_JSON_B64`"), "{}", no_key.stderr);
    assert!(rejected.exit_code == 1 && rejected.stderr.contains("could not update the vault secret CODEX_OAUTH_JSON_B64"), "{}", rejected.stderr);
    for run in [&no_key, &rejected] {
        assert!(!run.stdout.contains("fake-refresh") && !run.stderr.contains("fake-refresh") && !run.stderr.contains(&b64(AUTH_JSON)), "{}{}", run.stdout, run.stderr);
    }
    assert!(b.leftovers().is_empty());
}

#[test]
fn clear_empties_the_vault_secret_and_the_local_file_before_a_link_login_without_asking_the_listener() {
    // Given a machine that holds codex's file, and no listener credentials
    let b = Bench::new();
    b.stub("codex", CODEX_OK);
    let file = b.home.join(".codex/auth.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "{}").unwrap();

    // When codex logs in with --clear
    let r = b.login(&["codex", "--clear"], &[]);

    // Then the file is gone, the secret is first emptied to a value no restore can use and then set by the new login
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.starts_with("codex: vault secret CODEX_OAUTH_JSON_B64 cleared\ncodex: cleared 1 file(s) here\ncodex: open this link"), "{}", r.stdout);
    assert!(!file.exists());
    let edits = b.edits();
    assert!(edits.len() == 2 && edits[0].contains("--value bG9nZ2VkLW91dA==") && edits[1].contains(&format!("--value {}", b64(AUTH_JSON))), "{edits:?}");

    // When claude is asked to copy a credential it does not leave as a file
    b.stub("claude", "echo ran >> \"$HOME/claude-ran.log\"");
    let copy = b.login(&["claude", "--copy"], &[]);
    // Then it says so before running anything
    assert!(copy.exit_code == 1 && copy.stderr.contains("claude: this login declares no credential to copy; run it without --copy"), "{}", copy.stderr);
    assert!(b.logged("claude-ran.log").is_empty());
}

#[test]
fn claude_prints_its_link_then_takes_the_code_from_a_second_invocation_and_stores_the_token() {
    // Given a claude that prints the authorization link and waits for the code in its terminal
    let b = Bench::new();
    b.stub("claude", &claude_stub("printf 'Your OAuth token (valid for 1 year):\\r\\n\\033[97m%s\\033[39m\\r\\n\\033[2mStore this token securely.\\033[22m\\r\\n' \"$TOKEN\""));

    // When the person starts the login, nothing is waiting for a code yet, and a code is sent before the link was printed
    let early = b.login(&["claude", "--code", CODE], &[]);
    let mut first = spawn_wf(&["secret", "login", "claude"], &b.opts(&[]));
    wait_until(Duration::from_secs(15), "the link", || first.out().contains("--code <code>"));
    // Then the early code is refused, and the link is printed as a plain https link ahead of the instruction to send the code
    assert!(early.exit_code == 1 && early.stderr.contains("claude: no `wf secret login claude` is waiting for a code -- start it first"), "{}", early.stderr);
    assert_eq!(first.out(), format!("claude: open this link on any device and sign in:\n{CLAUDE_URL}\nthen send the code the page shows:\n  wf secret login claude --code <code>\n"));
    // and the control files are private
    assert_eq!((mode(&b.runtime.join("wf-login")), mode(&b.runtime.join("wf-login/claude.waiting"))), (0o700, 0o600));

    // When a code with a space in it is sent, and then the real one
    let spaced = b.login(&["claude", "--code", "two words"], &[]);
    let second = b.login(&["claude", "--code", CODE], &[]);
    // Then the first is refused, the second is handed to the waiting login, which finishes and stores the token as the plain string
    assert!(spaced.exit_code == 1 && spaced.stderr.contains("must be one word"), "{}", spaced.stderr);
    assert_eq!((second.exit_code, second.stdout.as_str()), (0, "claude: code handed to the waiting login\n"));
    assert_eq!(first.wait(Duration::from_secs(15)), Some(0), "{}", first.out());
    assert!(first.out().ends_with("code received, finishing the login...\nclaude: authenticated ✓ (vault CLAUDE_CODE_OAUTH_TOKEN)\n"), "{}", first.out());
    let edits = b.edits();
    assert!(edits.len() == 1 && edits[0].contains(&format!("--value {TOKEN}")) && edits[0].ends_with(" c1"), "{edits:?}");
    // and the claude it ran received exactly the code, while neither the code nor the token was printed by either invocation
    assert_eq!(b.logged("claude-code.log"), [CODE]);
    for out in [first.out(), second.stdout.clone(), second.stderr.clone()] {
        assert!(!out.contains(TOKEN) && !out.contains(CODE) && !out.contains("FAKEfakeFAKE"), "{out}");
    }
    assert!(b.leftovers().is_empty(), "{:?}", b.leftovers());
}

#[test]
fn claude_reports_a_refused_code_and_a_missing_one_without_storing_anything() {
    let b = Bench::new();

    // When claude refuses the code it was given (it prints the error and waits for Enter)
    b.stub("claude", &claude_stub("printf 'OAuth error: That code wasn'\"'\"'t accepted. It may be incomplete, expired, already used.\\r\\n\\r\\nPress Enter to retry\\r\\n'\nsleep 30"));
    let mut first = spawn_wf(&["secret", "login", "claude"], &b.opts(&[]));
    wait_until(Duration::from_secs(15), "the link", || first.out().contains("--code <code>"));
    b.login(&["claude", "--code", CODE], &[]);
    // Then the login fails at once with claude's own words and the code is never printed
    assert_eq!(first.wait(Duration::from_secs(15)), Some(1), "{}", first.out());
    assert!(first.out().contains("claude: login failed: OAuth error: That code wasn't accepted. It may be incomplete, expired, already used."), "{}", first.out());
    assert!(!first.out().contains(CODE));

    // When nobody sends the code before the timeout
    let late = b.login(&["claude"], &[("WF_LOGIN_TIMEOUT_MS", "1500")]);
    // Then it says no code arrived and how to send one, and the next login can start
    assert_eq!(late.exit_code, 1);
    assert!(late.stderr.contains("claude: no code arrived within 1.") && late.stderr.contains(" seconds -- run the same command again for a fresh link, then send the code with `wf secret login claude --code <code>`"), "{}", late.stderr);
    assert!(b.edits().is_empty() && b.leftovers().is_empty(), "{:?}", b.leftovers());

    // When a code is sent for a tool whose login takes none
    let none = b.login(&["codex", "--code", CODE], &[]);
    // Then it is refused, and --code does not combine with --copy
    let combined = b.login(&["claude", "--code", CODE, "--copy"], &[]);
    assert!(none.exit_code == 1 && none.stderr.contains("codex: this login takes no code"), "{}", none.stderr);
    assert_eq!(combined.exit_code, 2, "{}", combined.stderr);
}

#[test]
fn an_interrupted_login_stops_the_command_removes_its_files_and_stores_nothing() {
    // Given a login that has printed its link and waits for the code
    let b = Bench::new();
    b.stub("claude", &format!("echo $$ > \"$HOME/claude.pid\"\n{}", claude_stub("echo unreachable")));
    let mut first = spawn_wf(&["secret", "login", "claude"], &b.opts(&[]));
    wait_until(Duration::from_secs(15), "the link", || first.out().contains("--code <code>"));
    // When it is terminated
    first.signal("TERM");
    // Then it leaves with 143 saying nothing was stored, the command is stopped and no file of it is left
    assert_eq!(first.wait(Duration::from_secs(15)), Some(143), "{}", first.out());
    assert!(first.out().ends_with("claude: login stopped; nothing was stored\n"), "{}", first.out());
    let pid = std::fs::read_to_string(b.home.join("claude.pid")).unwrap();
    assert!(!running(pid.trim()), "the login command is still running");
    assert!(b.edits().is_empty() && b.leftovers().is_empty(), "{:?}", b.leftovers());
}

#[test]
fn run_interactively_claude_takes_the_pasted_code_on_stdin() {
    // Given claude's login started in a terminal (a pty from `script`), which the person types the code into
    let b = Bench::new();
    b.stub("claude", &claude_stub("printf 'Your OAuth token:\\r\\n%s\\r\\n' \"$TOKEN\""));
    let opts = b.opts(&[]);
    let inherited = std::env::var("PATH").unwrap_or_default();
    let mut command = std::process::Command::new("script");
    command.args(["-qec", &format!("{} secret login claude", crate::support::wf_binary().display()), "/dev/null"]);
    command.env_clear().env("PATH", format!("{}:{inherited}", b.bin.display())).env("WF_SECRETS_CACHE", "off").env("WF_USAGE", "off").env("TEST_RUNS_ORDS_BASE_URL", "").envs(&opts.env);
    command.current_dir(b.dir.path()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = command.spawn().unwrap();
    // When the code is pasted on its stdin
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(format!("{CODE}\n").as_bytes()).unwrap();
        // the terminal stays open until the login ends, as a person's does
        let out = child.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        drop(stdin);
        // Then the login finishes with the token stored and claude received exactly the pasted code
        assert!(text.contains("or paste it here.") && text.contains("claude: authenticated ✓ (vault CLAUDE_CODE_OAUTH_TOKEN)"), "{text}");
        assert!(!text.contains("FAKEfakeFAKE"), "{text}");
    }
    assert_eq!(b.logged("claude-code.log"), [CODE]);
    let edits = b.edits();
    assert!(edits.len() == 1 && edits[0].contains(&format!("--value {TOKEN}")), "{edits:?}");
    assert!(b.leftovers().is_empty());
}

#[test]
fn a_code_left_by_a_login_that_was_killed_is_never_handed_to_the_next_login() {
    // Given the control files a killed claude login left: a marker of a process that is gone, and the code it never took
    let b = Bench::new();
    let control = b.runtime.join("wf-login");
    std::fs::create_dir_all(&control).unwrap();
    std::fs::write(control.join("claude.waiting"), "2147483646").unwrap();
    std::fs::write(control.join("claude.code"), "stale-code").unwrap();
    b.stub("claude", &claude_stub("printf 'Your OAuth token:\\r\\n%s\\r\\n' \"$TOKEN\""));

    // When a new login starts and the person sends the code for its own link
    let mut first = spawn_wf(&["secret", "login", "claude"], &b.opts(&[]));
    wait_until(Duration::from_secs(15), "the link", || first.out().contains("--code <code>"));
    std::thread::sleep(Duration::from_millis(500));
    let stale_used = b.logged("claude-code.log");
    let sent = b.login(&["claude", "--code", CODE], &[]);

    // Then claude only ever received the new code
    assert!(stale_used.is_empty(), "the stale code reached the login: {stale_used:?}");
    assert_eq!(sent.exit_code, 0, "{}{}", sent.stdout, sent.stderr);
    assert_eq!(first.wait(Duration::from_secs(15)), Some(0), "{}", first.out());
    assert_eq!(b.logged("claude-code.log"), [CODE]);
}

#[test]
fn two_claude_logins_started_together_never_wait_on_the_same_code() {
    // Given a claude that waits for its code
    let b = Bench::new();
    b.stub("claude", &claude_stub("sleep 30"));

    // When two logins start at the same time
    let mut one = spawn_wf(&["secret", "login", "claude"], &b.opts(&[]));
    let mut two = spawn_wf(&["secret", "login", "claude"], &b.opts(&[]));
    wait_until(Duration::from_secs(15), "one of them to be refused", || one.out().contains("already waiting") || two.out().contains("already waiting"));

    // Then exactly one is refused, the other prints its link and goes on waiting
    let (refused, running_one) = if one.out().contains("already waiting") { (&mut one, &mut two) } else { (&mut two, &mut one) };
    assert_eq!(refused.wait(Duration::from_secs(15)), Some(1), "{}", refused.out());
    wait_until(Duration::from_secs(15), "the link", || running_one.out().contains("--code <code>"));
    assert!(!running_one.out().contains("already waiting"), "{}", running_one.out());
    running_one.signal("TERM");
    assert_eq!(running_one.wait(Duration::from_secs(15)), Some(143));
    assert!(b.leftovers().is_empty(), "{:?}", b.leftovers());
}

#[test]
fn a_login_that_was_killed_outright_has_its_private_directory_removed_by_the_next_one() {
    // Given the directory of a login whose process is gone (it held a credential file), and one of a process that still runs
    let b = Bench::new();
    let (dead, alive) = (b.runtime.join("wf-login/run-2147483646-aaaa"), b.runtime.join(format!("wf-login/run-{}-bbbb", std::process::id())));
    for dir in [&dead, &alive] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("auth.json"), AUTH_JSON).unwrap();
    }
    b.stub("codex", CODEX_OK);

    // When another login runs
    let r = b.login(&["codex"], &[]);

    // Then the dead one's directory is gone and the live one's is untouched
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(!dead.exists() && alive.join("auth.json").exists(), "{:?}", b.leftovers());
}

#[test]
fn the_credential_file_is_gone_from_disk_before_the_vault_is_written() {
    // Given a vault whose write records what is still under the private directory at that moment
    let b = Bench::new();
    b.stub("codex", CODEX_OK);
    let runtime = b.runtime.join("wf-login").display().to_string();
    b.stub("bws", &format!("echo \"$*\" >> \"$HOME/bws.log\"\nif [ \"$1 $2\" = \"secret list\" ]; then echo '[{{\"id\":\"x1\",\"key\":\"CODEX_OAUTH_JSON_B64\",\"value\":\"old\"}}]'; exit 0; fi\nif [ \"$1 $2\" = \"secret edit\" ]; then find {runtime} -name auth.json >> \"$HOME/at-write.log\"; fi\nexit 0"));

    // When codex logs in
    let r = b.login(&["codex"], &[]);

    // Then no auth.json was on disk when the vault was written
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(b.logged("at-write.log"), Vec::<String>::new());
    assert_eq!(b.edits().len(), 1);
}
