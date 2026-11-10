//! The replay double of `gh` and `glab` as the executor starts it.
use exec_testkit::hosted::Flood;
use exec_testkit::{HostedClis, Rule};
use std::io::Write;
use std::process::{Command, Stdio};

fn run(clis: &HostedClis, program: &str, args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(clis.dir().join(program)).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).env("GH_TOKEN", "t").spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

fn rule(program: &str, args: &[&str]) -> Rule {
    Rule { program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), ..Rule::default() }
}

#[test]
fn given_rules_for_gh_and_glab_when_each_is_started_with_its_argv_then_it_prints_its_rule_and_logs_the_start_and_an_unknown_argv_fails() {
    // Given a rule for each program, one that needs its stdin, and one that exits non-zero
    let clis = HostedClis::install();
    clis.rules(
        &[
            Rule { stdout: "[1]".into(), ..rule("gh", &["pr", "list"]) },
            Rule { stdout: "ok".into(), stdin: Some("body".into()), ..rule("gh", &["pr", "comment"]) },
            Rule { stderr: "boom".into(), exit_code: 4, ..rule("glab", &["mr", "view"]) },
        ],
        &["GH_TOKEN"],
    );
    // When each is started
    let list = run(&clis, "gh", &["pr", "list"], "");
    let comment = run(&clis, "gh", &["pr", "comment"], "body");
    let wrong_stdin = run(&clis, "gh", &["pr", "comment"], "other");
    let failing = run(&clis, "glab", &["mr", "view"], "");
    let unknown = run(&clis, "gh", &["api", "/user"], "");
    // Then each prints its rule, a stdin that differs or an argv that is unknown answers 97, and the log keeps argv, stdin, the named variable and whether a rule answered
    assert_eq!((list.0, list.1.as_str()), (0, "[1]"));
    assert_eq!((comment.0, comment.1.as_str()), (0, "ok"));
    assert_eq!((wrong_stdin.0, unknown.0, failing.0, failing.2.as_str()), (97, 97, 4, "boom"));
    assert!(unknown.2.contains("no rule answers"));
    let starts = clis.starts("gh");
    assert_eq!(starts.iter().map(|s| s.matched).collect::<Vec<_>>(), [true, true, false, false]);
    assert_eq!((starts[1].stdin.as_str(), starts[1].env["GH_TOKEN"].as_deref()), ("body", Some("t")));
    assert_eq!(clis.argvs().len(), 5);
    // And setting rules again forgets the earlier starts
    clis.rules(&[], &[]);
    assert!(clis.starts("gh").is_empty());
}

#[test]
fn given_a_rule_that_floods_or_hangs_when_started_then_it_prints_what_it_was_told_to_and_stays_until_killed() {
    let clis = HostedClis::install();
    clis.rules(&[Rule { flood: Some(Flood { text: "x".into(), times: 100_000 }), ..rule("gh", &["pr", "diff"]) }, Rule { hang: true, stdout: "waiting\n".into(), ..rule("gh", &["run", "watch"]) }], &[]);
    assert_eq!(run(&clis, "gh", &["pr", "diff"], "").1.len(), 100_000);
    let mut child = Command::new(clis.dir().join("gh")).args(["run", "watch"]).stdout(Stdio::piped()).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none(), "a hanging rule ended");
    child.kill().unwrap();
}

#[test]
fn given_every_interaction_of_the_hosted_cassette_when_played_then_each_call_prints_what_its_steps_hold_and_exits_with_its_code() {
    // Given the interactions of the exec-hosted cassette set, each with the calls of one executor request
    let set = exec_testkit::Cassette::open(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../exec-hosted/tests/cassettes/exec-hosted"));
    assert!(!set.interactions.is_empty() && set.provenance.kind != "recorded", "{}", set.provenance.kind);
    for interaction in &set.interactions {
        let clis = HostedClis::install();
        // When the double plays it and each call is started with the argv and stdin its steps hold
        clis.play(interaction, &[]);
        for rule in interaction.rules() {
            let args: Vec<&str> = rule.args.iter().map(String::as_str).collect();
            let (code, stdout, stderr) = run(&clis, &rule.program, &args, rule.stdin.as_deref().unwrap_or(""));
            // Then it prints the recorded stdout and stderr and exits with the recorded code
            assert_eq!((code, stdout, stderr), (rule.exit_code, rule.stdout.clone(), rule.stderr.clone()), "{}: {} {:?}", interaction.id, rule.program, rule.args);
        }
    }
}
