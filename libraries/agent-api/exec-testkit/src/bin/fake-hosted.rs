//! The program that stands in for `gh` and `glab` (see `exec_testkit::hosted`): started under either name, it answers by the first rule of its
//! rules file whose arguments (and stdin) equal the start's, and logs every start.
use exec_testkit::hosted::{log_file, rules_file, Rules, Start};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

fn main() {
    let exe = std::env::current_exe().expect("a path of its own");
    let (dir, program) = (exe.parent().unwrap_or(Path::new(".")).to_path_buf(), exe.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string());
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdin = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin);
    let rules: Rules = std::fs::read_to_string(dir.join(rules_file(&program))).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let found = rules.rules.iter().find(|r| (r.args == args || (r.prefix && args.starts_with(&r.args))) && r.stdin.as_ref().is_none_or(|s| *s == stdin));
    let start = Start {
        program: program.clone(),
        argv: args.clone(),
        cwd: std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
        stdin,
        env: rules.log_env.iter().map(|n| (n.clone(), std::env::var(n).ok())).collect::<BTreeMap<_, _>>(),
        matched: found.is_some(),
    };
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(log_file(&program))).expect("the start log");
    let _ = writeln!(log, "{}", serde_json::to_string(&start).unwrap_or_default());
    drop(log);
    let Some(rule) = found else {
        eprintln!("fake {program}: no rule answers {args:?}");
        std::process::exit(97);
    };
    if let Some(dir) = &rule.mkdir {
        let _ = std::fs::create_dir_all(dir);
    }
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    let _ = out.write_all(rule.stdout.as_bytes());
    let _ = err.write_all(rule.stderr.as_bytes());
    let _ = out.flush();
    let _ = err.flush();
    if let Some(flood) = &rule.flood {
        for _ in 0..flood.times {
            if out.write_all(flood.text.as_bytes()).is_err() {
                break;
            }
        }
        let _ = out.flush();
    }
    if rule.sleep_ms > 0 {
        std::thread::sleep(Duration::from_millis(rule.sleep_ms));
    }
    if rule.hang {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    std::process::exit(rule.exit_code);
}
