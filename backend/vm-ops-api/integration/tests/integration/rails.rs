//! The harness's own safety rails, which keep every case off the real VM and away from the runner's credentials. These
//! start no service: a rail that holds panics before any child starts, and the child they would start is the fake-tool
//! printing the names of its variables (never their values, which the report of a failed case would carry), or a helper
//! line that only echoes its words. The last case runs the `docker` double itself, to show it records a sibling and
//! never runs the script it is given.
use crate::world;
use std::path::Path;
use std::time::Duration;
use serde_json::json;
use vm_ops_api_integration::{assert_local, case, expect_panic, ToolRule, World, CHILD_ENV};

/// Starts `fake-tool env-names` in the world exactly as the service would be started, so a rail that does not hold runs
/// nothing harmful; returns the variable names it was handed.
fn start_in(w: &World) -> String {
    let exit = w.run_program_to_exit(env!("CARGO_BIN_EXE_fake-tool"), &["env-names".to_string()], Duration::from_secs(10));
    assert_eq!(exit.code, Some(0), "fake-tool env-names in the world: {}", exit.stderr);
    exit.stdout
}

fn executable_script(at: &Path) {
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(at, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

case!(a_base_url_other_than_loopback_panics, {
    // Given a case's dir
    let dir = Path::new("/tmp/vm-ops-api-it-case");
    // When the base URL is the VM's tailnet name, Then the rail panics
    expect_panic("safety rail: http://vm-ops-api-main:8080", || assert_local("http://vm-ops-api-main:8080", dir));
});

case!(loopback_and_a_socket_in_the_case_dir_pass_and_a_socket_outside_it_panics, {
    // Given a case's dir
    let dir = Path::new("/tmp/vm-ops-api-it-case");
    // When the target is loopback TCP or a socket inside the dir, Then the rail lets it through
    assert_local("http://127.0.0.1:18080/health", dir);
    assert_local("unix:/tmp/vm-ops-api-it-case/docker.sock", dir);
    // When the socket is outside the dir, or climbs out of it, Then the rail panics
    for outside in ["unix:/var/run/docker.sock", "unix:/tmp/vm-ops-api-it-case/../docker.sock", "http://127.0.0.2:18080"] {
        expect_panic(&format!("safety rail: {outside}"), || assert_local(outside, dir));
    }
});

case!(a_docker_ahead_of_the_double_on_path_panics_before_the_child_starts, {
    // Given a world whose PATH puts another executable `docker` ahead of the doubles, as a real CLI in /usr/bin would be
    let mut w = world();
    let other = w.dir().join("other");
    executable_script(&other.join("docker"));
    let path = format!("{}:{}", other.display(), w.dir().join("bin").display());
    w.set("PATH", &path);
    // When the child would start, Then the rail panics first
    expect_panic("not to the double in", || {
        start_in(&w);
    });
});

case!(a_double_that_is_not_the_fake_tool_panics_before_the_child_starts, {
    // Given a world whose bin/docker is relinked to another program, and one whose bin/wf is overwritten with a script
    let relinked = world();
    let other = relinked.dir().join("other/docker");
    executable_script(&other);
    std::fs::remove_file(relinked.dir().join("bin/docker")).unwrap();
    std::os::unix::fs::symlink(&other, relinked.dir().join("bin/docker")).unwrap();
    let overwritten = world();
    std::fs::remove_file(overwritten.dir().join("bin/wf")).unwrap();
    executable_script(&overwritten.dir().join("bin/wf"));
    // When either child would start, Then the rail panics first
    for w in [&relinked, &overwritten] {
        expect_panic("not the fake-tool", || {
            start_in(w);
        });
    }
    // When a world is built around a binary other than the fake-tool, Then the rail panics
    expect_panic("is not this crate's fake-tool", || {
        World::new("/usr/bin/docker");
    });
});

case!(a_docker_socket_or_host_path_outside_the_case_dir_panics_before_the_child_starts, {
    // Given worlds whose Docker socket or a host path is unset, empty, the host's own or climbs out of the case's dir
    type Change = fn(&mut World);
    let changes: [(&str, Change); 7] = [
        ("DOCKER_SOCKET is unset or empty", |w| {
            w.unset("DOCKER_SOCKET");
        }),
        ("DOCKER_SOCKET is unset or empty", |w| {
            w.set("DOCKER_SOCKET", "");
        }),
        ("safety rail: unix:/var/run/docker.sock", |w| {
            w.set("DOCKER_SOCKET", "/var/run/docker.sock");
        }),
        ("safety rail: PROC_ROOT", |w| {
            w.unset("PROC_ROOT");
        }),
        ("safety rail: POOL_COOLDOWN_STATE_FILE", |w| {
            w.set("POOL_COOLDOWN_STATE_FILE", "/dev/full");
        }),
        ("safety rail: VM_DIR", |w| {
            let up = format!("{}/../vm", w.dir().display());
            w.set("VM_DIR", &up);
        }),
        ("safety rail: FAKE_TOOL_LOG", |w| {
            w.set("FAKE_TOOL_LOG", "/tmp/calls.jsonl");
        }),
    ];
    for (expected, change) in changes {
        let mut w = world();
        change(&mut w);
        // When the child would start, Then the rail panics first
        expect_panic(expected, || {
            start_in(&w);
        });
    }
});

case!(a_variable_outside_the_allowlist_panics, {
    // Given a world, When a case hands the service a credential the allowlist does not name, Then the rail panics
    expect_panic("safety rail: VM_GATEWAY_AUTH_SEED is not a variable the harness hands the service", || {
        world().set("VM_GATEWAY_AUTH_SEED", "x");
    });
});

case!(the_child_gets_only_the_variables_the_harness_sets, {
    // Given the suite's own environment, which holds a variable the harness never hands on (test:integration sets it)
    assert!(std::env::var_os("VM_OPS_API_CMD").is_some(), "run the suite through test:integration, which sets VM_OPS_API_CMD");
    // When a child starts in a world exactly as the service would
    let printed = start_in(&world());
    // Then every variable it sees is one the harness set, so nothing of the runner's environment reaches the child
    let names: Vec<&str> = printed.lines().collect();
    assert!(names.contains(&"VM_OPS_API_AUTH_TOKEN"), "{printed}");
    let stray: Vec<&&str> = names.iter().filter(|n| !CHILD_ENV.contains(n)).collect();
    assert!(stray.is_empty(), "the child saw variables the harness never set: {stray:?}");
});

case!(a_helper_line_that_could_reach_github_or_the_host_panics_before_it_runs, {
    // Given a world, and helper lines that only echo, so a rail that does not hold runs nothing harmful
    let w = world();
    let inside = w.dir().join("vm/.env").display().to_string();
    let refused = [
        ("echo https://raw.githubusercontent.com/x", "safety rail: https://raw.githubusercontent.com/x"),
        ("echo 'http://127.0.0.2:80/x'", "safety rail: http://127.0.0.2:80/x"),
        ("echo /etc/x", "safety rail: the helper line names /etc/x, outside"),
        ("echo '/home/ubuntu/vm-ops-api/.env'", "safety rail: the helper line names /home/ubuntu/vm-ops-api/.env, outside"),
        ("echo ../x", "safety rail: the helper line names ../x, outside"),
        ("echo -o/etc/x", "safety rail: the helper line names -o/etc/x, outside"),
        ("echo '--dest=/etc/x'", "safety rail: the helper line names /etc/x, outside"),
        ("echo 'cp a /etc/x'", "safety rail: the helper line names /etc/x, outside"),
        ("echo 'x,https://raw.githubusercontent.com/x'", "safety rail: https://raw.githubusercontent.com/x"),
        (&format!("echo {inside}/../../x"), "/../../x, outside"),
        ("echo \"/home/ubuntu/vm-ops-api/.env\"", "holds a word sh would expand"),
        ("echo $VM_DIR/docker-compose.yml", "holds a word sh would expand"),
        ("echo `id`", "holds a word sh would expand"),
        ("echo ~/x", "holds a word sh would expand"),
        ("echo x > x", "holds a word sh would expand"),
        ("echo x; echo y", "holds a word sh would expand"),
        ("echo 'x'/etc", "holds a word sh would expand"),
        ("X=/etc echo x", "holds a word sh would expand"),
    ];
    for (line, expected) in refused {
        // When it would run, Then the rail panics first
        expect_panic(expected, || {
            w.run_helper(line, &[], Duration::from_secs(10));
        });
    }
    // When a helper would be handed a variable other than the provisioning token, Then the rail panics first
    expect_panic("safety rail: VM_GATEWAY_AUTH_SEED is not a variable a helper command reads", || {
        w.run_helper("echo x", &[("VM_GATEWAY_AUTH_SEED", "x")], Duration::from_secs(10));
    });
    // When a line of bare and single-quoted words inside the world runs, Then it runs, as sh reads it
    let exit = w.run_helper(&format!("echo '{inside}' x"), &[("GITHUB_RAW_TOKEN", "")], Duration::from_secs(10));
    assert_eq!((exit.code, exit.stdout), (Some(0), format!("{inside} x\n")), "{}", exit.stderr);
});

case!(a_docker_on_the_helper_path_that_is_not_the_fake_tool_panics_before_the_helper_runs, {
    // Given a world whose bin/docker is overwritten with a script
    let w = world();
    std::fs::remove_file(w.dir().join("bin/docker")).unwrap();
    executable_script(&w.dir().join("bin/docker"));
    // When a helper line would run, Then the rail panics first
    expect_panic("not the fake-tool", || {
        w.run_helper("true", &[], Duration::from_secs(10));
    });
});

case!(the_docker_double_records_each_sibling_and_never_runs_its_script, {
    // Given a world whose docker double answers one read-only sibling with output and exit code 3
    let mut w = world();
    w.rule(ToolRule::docker(&["run", "--rm"]).rest(r"cat x$").answer("out\n", "err\n", 3));
    let docker = w.dir().join("bin/docker").display().to_string();
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        w.run_program_to_exit(&docker, &args, Duration::from_secs(10))
    };
    // When a detached sibling is run whose script would leave a file in the world
    let marker = w.dir().join("ran");
    let script = format!("touch {}", marker.display());
    let detached = ["run", "--pull", "always", "--rm", "-d", "-v", "a:a", "ns/vm-ops-api:t1", "sh", "-c", script.as_str()];
    let exit = run(&detached);
    // Then it prints a container id and exits 0, the script never ran, and the call is recorded with its argv
    assert_eq!(exit.code, Some(0), "{}", exit.stderr);
    let id = exit.stdout.strip_suffix('\n').unwrap_or("");
    assert!(id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()), "no container id: {:?}", exit.stdout);
    assert!(!marker.exists(), "the docker double ran the sibling's script");
    assert_eq!(w.calls(), [json!({ "tool": "docker", "argv": detached })]);
    // When the scripted sibling and an unscripted call run, Then the first answers as scripted and the second exits 97
    let scripted = run(&["run", "--rm", "ns/vm-ops-api:t1", "sh", "-c", "cat x"]);
    assert_eq!((scripted.code, scripted.stdout.as_str(), scripted.stderr.as_str()), (Some(3), "out\n", "err\n"));
    assert_eq!(run(&["ps", "-a"]).code, Some(97));
});
