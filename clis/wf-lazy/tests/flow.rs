//! The launcher against a doubled cargo: the double records its argv and writes a fake thin binary that prints its own.
use std::path::Path;
use std::process::{Command, Output};

struct Bench {
    dir: tempfile::TempDir,
}

impl Bench {
    fn new(cargo_exit: i32) -> Bench {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("target/debug")).unwrap();
        let script = format!(
            "#!/bin/bash\necho \"$*\" >> \"{log}\"\nif [ {exit} -ne 0 ]; then exit {exit}; fi\nbin=\"\"\nwhile [ $# -gt 0 ]; do if [ \"$1\" = --bin ]; then bin=$2; fi; shift; done\nprintf '#!/bin/sh\\necho ran:%s \"$*\"\\n' \"$bin\" > \"{target}/debug/$bin\"\nchmod +x \"{target}/debug/$bin\"\n",
            log = p.join("cargo.log").display(),
            exit = cargo_exit,
            target = p.join("target").display(),
        );
        std::fs::write(p.join("cargo"), script).unwrap();
        Command::new("chmod").arg("+x").arg(p.join("cargo")).status().unwrap();
        Bench { dir }
    }

    fn run(&self, args: &[&str]) -> Output {
        let p: &Path = self.dir.path();
        Command::new(env!("CARGO_BIN_EXE_wf-lazy"))
            .args(args)
            .env("WF_LAZY_CARGO", p.join("cargo"))
            .env("CARGO_TARGET_DIR", p.join("target"))
            .output()
            .unwrap()
    }

    fn builds(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("cargo.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn a_verb_with_a_thin_binary_builds_only_its_package_and_runs_it_with_the_same_arguments() {
    // Given no thin binary built
    let b = Bench::new(0);

    // When `offload ask hello` runs
    let out = b.run(&["offload", "ask", "hello"]);

    // Then cargo built the offload package's binary once and that binary ran with the arguments
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout(&out), "ran:offload offload ask hello");
    let builds = b.builds();
    assert_eq!(builds.len(), 1);
    assert!(builds[0].starts_with("build --locked -p offload --bin offload --manifest-path "), "{builds:?}");
}

#[test]
fn two_verbs_of_one_crate_share_its_binary_and_a_verb_of_another_crate_builds_another() {
    // Given a bench
    let b = Bench::new(0);

    // When a ship, a vm and a parity verb run
    let ship = b.run(&["ship", "--help"]);
    let vm = b.run(&["vm", "net"]);
    let parity = b.run(&["parity", "coverage"]);

    // Then ship and vm ran the deploy binary and parity ran wf-parity
    assert_eq!((stdout(&ship), stdout(&vm), stdout(&parity)), ("ran:deploy ship --help".into(), "ran:deploy vm net".into(), "ran:wf-parity parity coverage".into()));
    let packages: Vec<String> = b.builds().iter().map(|l| l.split(' ').nth(3).unwrap().to_string()).collect();
    assert_eq!(packages, vec!["deploy", "deploy", "parity"]);
}

#[test]
fn a_verb_no_thin_binary_serves_and_help_run_the_full_router() {
    // Given a bench
    let b = Bench::new(0);

    // When an unlisted verb and `--help` run
    let check = b.run(&["check", "--staged"]);
    let help = b.run(&["--help"]);

    // Then both built and ran the full `wf` package
    assert_eq!((stdout(&check), stdout(&help)), ("ran:wf check --staged".into(), "ran:wf --help".into()));
    assert!(b.builds().iter().all(|l| l.starts_with("build --locked -p wf --bin wf ")), "{:?}", b.builds());
}

#[test]
fn a_failed_build_exits_with_cargos_code_names_the_command_and_runs_no_verb() {
    // Given a cargo that fails with 101
    let b = Bench::new(101);

    // When a verb runs
    let out = b.run(&["knowledge", "search", "x"]);

    // Then the launcher exits 101, prints the command to rerun and ran nothing
    assert_eq!(out.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&out.stderr).contains("rerun it with:"), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout(&out), "");
}

#[test]
fn every_verb_of_the_table_is_served_by_its_crate_and_its_binary_exists_there() {
    // Given the table of the launcher and the crates of this workspace
    let clis = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let table = include_str!("../src/main.rs");
    let rows: Vec<(String, String, String)> = table
        .lines()
        .filter_map(|l| l.trim().strip_prefix("(\"").and_then(|r| r.strip_suffix("\"),")))
        .map(|r| r.split("\", \"").map(str::to_string).collect::<Vec<_>>())
        .filter(|p| p.len() == 3)
        .map(|p| (p[0].clone(), p[1].clone(), p[2].clone()))
        .collect();
    assert!(rows.len() >= 9, "{rows:?}");

    for (verb, package, bin) in rows {
        // When the crate's manifest and command table are read
        let dir = std::fs::read_dir(clis)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|d| std::fs::read_to_string(d.join("Cargo.toml")).is_ok_and(|m| m.contains(&format!("name = \"{package}\"\n"))))
            .unwrap_or_else(|| panic!("no crate named {package}"));
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let commands = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();

        // Then it names the binary and serves the verb
        assert!(manifest.contains(&format!("name = \"{bin}\"")), "{package} has no binary {bin}");
        assert!(commands.contains(&format!("(\"{verb}\", ")), "{package} does not serve {verb}");
    }
}
