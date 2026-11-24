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

    // When `rca ask hello` runs
    let out = b.run(&["rca", "ask", "hello"]);

    // Then cargo built the rca package's binary once and that binary ran with the arguments
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout(&out), "ran:rca rca ask hello");
    let builds = b.builds();
    assert_eq!(builds.len(), 1);
    assert!(builds[0].starts_with("build --locked -p rca --bin rca --manifest-path "), "{builds:?}");
}

#[test]
fn two_verbs_of_one_crate_share_its_binary_and_a_verb_of_another_crate_builds_another() {
    // Given a bench
    let b = Bench::new(0);

    // When a ship, a vm and a lib verb run
    let ship = b.run(&["ship", "--help"]);
    let vm = b.run(&["vm", "net"]);
    let lib = b.run(&["lib", "release"]);

    // Then ship and vm ran the deploy binary and lib ran wf-lib
    assert_eq!((stdout(&ship), stdout(&vm), stdout(&lib)), ("ran:deploy ship --help".into(), "ran:deploy vm net".into(), "ran:wf-lib lib release".into()));
    let packages: Vec<String> = b.builds().iter().map(|l| l.split(' ').nth(3).unwrap().to_string()).collect();
    assert_eq!(packages, vec!["deploy", "deploy", "wf-lib"]);
}

#[test]
fn a_verb_no_thin_binary_serves_and_help_run_the_full_router() {
    // Given a bench
    let b = Bench::new(0);

    // When an unlisted verb and `--help` run
    let check = b.run(&["check"]);
    let help = b.run(&["--help"]);

    // Then both built and ran the full `wf` package
    assert_eq!((stdout(&check), stdout(&help)), ("ran:wf check".into(), "ran:wf --help".into()));
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
        let commands: String = ["src", "src-rs"].iter().flat_map(|d| std::fs::read_dir(dir.join(d)).into_iter().flatten().flatten()).filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect();

        // Then it names the binary and serves the verb
        assert!(manifest.contains(&format!("name = \"{bin}\"")), "{package} has no binary {bin}");
        assert!(commands.contains(&format!("(\"{verb}\", ")), "{package} does not serve {verb}");
    }
}

fn ran(b: &Bench, args: &[&str]) -> String {
    stdout(&b.run(args))
}

#[test]
fn the_gates_that_need_only_the_checkout_run_the_pipeline_binary() {
    // Given a bench
    let b = Bench::new(0);

    // When the staged, budget, dead-refs and pushed gates run, alone and with their own options
    let ran: Vec<String> = [
        vec!["check", "--staged"],
        vec!["check", "--budget", "--strict"],
        vec!["check", "--dead-refs"],
        vec!["check", "--dead-refs", "--staged"],
        vec!["check", "--staged", "--dead-refs"],
        vec!["check", "--pushed", "--base", "main"],
    ]
    .iter()
    .map(|a| ran(&b, a))
    .collect();

    // Then each ran the pipeline package's binary with the same words
    assert_eq!(
        ran,
        vec![
            "ran:pipeline check --staged",
            "ran:pipeline check --budget --strict",
            "ran:pipeline check --dead-refs",
            "ran:pipeline check --dead-refs --staged",
            "ran:pipeline check --staged --dead-refs",
            "ran:pipeline check --pushed --base main",
        ]
    );
    assert!(b.builds().iter().all(|l| l.starts_with("build --locked -p pipeline --bin pipeline ")), "{:?}", b.builds());
}

#[test]
fn a_gate_combined_with_a_flag_only_the_router_reads_runs_the_full_router() {
    // Given a bench
    let b = Bench::new(0);

    // When combined or router-only shapes run
    let shapes: Vec<Vec<&str>> = vec![
        vec!["check", "--dead-refs", "--budget"],
        vec!["check", "--dead-refs", "--dead-refs"],
        vec!["check", "--staged", "--health"],
        vec!["check", "--budget", "--visual", "x.png"],
        vec!["check", "--staged", "--drift"],
        vec!["check", "--staged", "--deployed", "abc"],
        vec!["check", "--staged", "--arch"],
        vec!["check", "--staged", "--dry-run"],
        vec!["check", "--health"],
        vec!["check", "--arch"],
        vec!["check", "--drift"],
        vec!["check", "--deployed", "abc"],
        vec!["check", "--help"],
    ];

    // Then every one ran the `wf` package, never the thin pipeline
    for shape in shapes {
        let out = ran(&b, &shape);
        assert_eq!(out, format!("ran:wf {}", shape.join(" ")), "{shape:?}");
    }
}

#[test]
fn secret_agent_config_and_flow_run_their_own_thin_binaries_with_the_same_words() {
    // Given a bench
    let b = Bench::new(0);

    // When a secret, an agent-config and an unpublished flow verb run
    let out: Vec<String> = [vec!["secret", "list"], vec!["agent-config", "sync", "--check"], vec!["flow", "--json", "x.flow.test.tsx"]].iter().map(|a| ran(&b, a)).collect();

    // Then each ran the binary of its own crate
    assert_eq!(out, vec!["ran:secrets secret list", "ran:agent-config agent-config sync --check", "ran:harness-flow flow --json x.flow.test.tsx"]);
    let packages: Vec<String> = b.builds().iter().map(|l| l.split(' ').nth(3).unwrap().to_string()).collect();
    assert_eq!(packages, vec!["secrets", "agent-config-sync", "harness-flow"]);
}

#[test]
fn secret_login_and_a_published_flow_run_the_full_router() {
    // Given a bench
    let b = Bench::new(0);

    // When the shapes only the router serves run
    let shapes: Vec<Vec<&str>> = vec![vec!["secret", "login", "listener"], vec!["flow", "x.flow.test.tsx", "--publish"]];

    // Then both ran the `wf` package
    for shape in shapes {
        assert_eq!(ran(&b, &shape), format!("ran:wf {}", shape.join(" ")), "{shape:?}");
    }
}
