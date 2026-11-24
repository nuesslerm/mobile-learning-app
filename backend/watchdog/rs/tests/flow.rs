//! Flow tests of the watchdog: the run loop `main` starts, built by `wiring::watchdog` with the `docker` binary and the clock
//! doubled, its events read back from the OTLP ingest they are exported to. Given/When/Then.
#[path = "flow/harness.rs"]
mod harness;

use harness::{app, run, Container, Docker, Failure};
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(30);

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

fn named(name: &'static str, health: Option<&'static str>, restarts: i64) -> Container {
    Container { service: Some(name), ..app(name, health, restarts) }
}

/// The VM as `docker` shows it at sweep `k` (from 1, 30 s apart): empty at first, then quiet containers, two unhealthy
/// ones, and restart counters that move differently.
fn vm(k: usize) -> Vec<Container> {
    if k == 1 {
        return vec![];
    }
    let at = |counts: &[(usize, i64)]| counts.iter().rev().find(|(from, _)| k >= *from).map_or(0, |(_, count)| *count);
    vec![
        app("app-1", Some("healthy"), 0),
        app("starting-1", Some("starting"), 0),
        app("plain-1", None, 0),
        named("watchdog", Some("unhealthy"), at(&[(2, 0), (3, 99)])),
        Container { project: None, service: None, ..app("unlabelled-1", Some("unhealthy"), 0) },
        Container { service: Some("a"), working_dir: Some("/home/ubuntu/x"), config_files: &["/home/ubuntu/x/a.yml", "", "/home/ubuntu/x/b.yml"], ..app("a-1", Some("unhealthy"), 0) },
        Container { service: Some("b"), ..app("b-1", Some("unhealthy"), 0) },
        named("loop-1", Some("healthy"), at(&[(2, 0), (3, 2), (4, 4)])),
        named("three-1", Some("healthy"), at(&[(2, 0), (3, 3)])),
        named("recreated-1", Some("healthy"), at(&[(2, 9), (3, 0), (4, 1), (5, 2)])),
        named("slow-1", Some("healthy"), at(&[(2, 0), (3, 2), (15, 4)])),
    ]
}

#[tokio::test]
async fn given_a_vm_with_quiet_unhealthy_and_restarting_containers_when_the_watchdog_runs_for_ten_minutes_then_it_recreates_exactly_the_sick_ones() {
    let docker = Docker::new((1..=24).map(vm).collect());

    let seen = run(docker, "main", EVERY, 24, None).await;

    let started = &seen.records[0];
    assert_eq!((started.message.as_str(), started.severity, started.get("hostLabel")), ("watchdog.started", 9, "main"));
    assert!(!seen.exports.is_empty() && seen.exports.iter().all(|e| *e == ("/v1/logs".into(), "Bearer tok".into(), "watchdog".into())), "{:?}", seen.exports);
    let ids: Vec<String> = (0..11).map(|i| format!("id{i}")).collect();
    let ps = argv(&["ps", "-a", "--format", "{{json .}}"]);
    let inspect: Vec<String> = ["inspect".to_string()].into_iter().chain(ids).collect();
    assert_eq!(seen.calls[..3], [(0, ps.clone()), (30, ps), (30, inspect)], "an empty listing is never inspected");

    let recreate_a = argv(&["compose", "-p", "proj", "--project-directory", "/home/ubuntu/x", "-f", "/home/ubuntu/x/a.yml", "-f", "/home/ubuntu/x/b.yml", "up", "-d", "--force-recreate", "--no-deps", "a"]);
    let recreate_b = argv(&["compose", "-p", "proj", "up", "-d", "--force-recreate", "--no-deps", "b"]);
    let recreate_loop = argv(&["compose", "-p", "proj", "up", "-d", "--force-recreate", "--no-deps", "loop-1"]);
    assert_eq!(
        seen.compose_calls(),
        [(60, recreate_a.clone()), (60, recreate_b.clone()), (90, recreate_loop), (660, recreate_a), (660, recreate_b)],
        "unhealthy ones in listing order once they have been unhealthy on two sweeps, and again after the ten-minute cooldown; a loop is more than three restarts in five minutes"
    );
    let recreated: Vec<(i64, &str, &str, &str, &str)> =
        seen.named("watchdog.recreated").iter().map(|r| (r.severity, r.get("container"), r.get("reason"), r.get("ok"), r.get("hostLabel"))).collect();
    let event = |container, reason| (17, container, reason, "true", "main");
    assert_eq!(recreated, [event("a-1", "unhealthy"), event("b-1", "unhealthy"), event("loop-1", "restart_loop"), event("a-1", "unhealthy"), event("b-1", "unhealthy")]);
    assert!(seen.records.iter().all(|r| !r.attributes.contains_key("error") && r.message != "watchdog.sweep_failed"), "{:?}", seen.records);
}

#[tokio::test]
async fn given_a_failing_docker_or_ingest_when_swept_then_failures_become_error_events_and_the_loop_goes_on() {
    let mut ps_down = Docker::new(vec![vec![app("app-1", Some("healthy"), 0)]]);
    ps_down.ps = Some(Failure { code: 1, stderr: "cannot connect\n" });
    let mut inspect_down = Docker::new(vec![vec![app("app-1", Some("healthy"), 0)]]);
    inspect_down.inspect = Some(Failure { code: 1, stderr: "no such object\n" });
    let mut compose_down = Docker::new(vec![vec![app("app-1", Some("unhealthy"), 0)]]);
    let stderr: &'static str = Box::leak(format!("{}boom\n", "x".repeat(600)).into_boxed_str());
    compose_down.compose = Some(Failure { code: 1, stderr });
    let unhealthy = Docker::new(vec![vec![app("app-1", Some("unhealthy"), 0)]]);

    let ps_failed = run(ps_down, "main", EVERY, 3, None).await;
    let inspect_failed = run(inspect_down, "main", EVERY, 1, None).await;
    let compose_failed = run(compose_down, "main", EVERY, 2, None).await;
    let no_ingest = run(unhealthy, "main", EVERY, 2, Some("http://127.0.0.1:9")).await;

    let failures = ps_failed.named("watchdog.sweep_failed");
    assert_eq!(failures.len(), 3, "one per sweep: {failures:?}");
    assert!(failures.iter().all(|f| f.severity == 17 && f.get("error") == "docker ps failed: cannot connect"), "{failures:?}");
    assert!(ps_failed.calls.iter().all(|(_, call)| call[0] == "ps"));
    let failures = inspect_failed.named("watchdog.sweep_failed");
    assert_eq!((failures.len(), failures[0].get("error")), (1, "docker inspect failed: no such object"));
    let failed = compose_failed.named("watchdog.recreated");
    assert_eq!((failed[0].severity, failed[0].get("ok")), (17, "false"));
    assert_eq!(failed[0].get("error").chars().count(), 500, "the tail of a trimmed stderr");
    assert!(failed[0].get("error").ends_with("boom"));
    assert_eq!(no_ingest.compose_calls().len(), 1, "an unreachable ingest does not stop the work");
}

/// agent-shell as compose runs it: every service in the tailscale sidecar's network. Sweeps 1 and 2: the dind is unhealthy. Sweep 2:
/// the sidecar was recreated (a new id), the dind followed it, the droid daemon and a stopped session still point at the old one.
fn agent_shell(k: usize) -> Vec<Container> {
    let tailscale = if k == 1 { "ts-old" } else { "ts-new" };
    let in_net = |name, health, network_of| Container { service: Some(name), network_of: Some(network_of), ..app(name, health, 0) };
    vec![
        Container { id: Some(tailscale), ..named("tailscale", Some("healthy"), 0) },
        in_net("agent-dind", Some(if k <= 2 { "unhealthy" } else { "healthy" }), tailscale),
        in_net("agent-droid", None, "ts-old"),
        Container { running: false, ..in_net("agent-sessions", Some("healthy"), "ts-old") },
    ]
}

#[tokio::test]
async fn given_a_daemon_left_in_the_network_of_a_recreated_sidecar_when_swept_then_only_it_is_recreated_and_never_its_sidecar() {
    let docker = Docker::new((1..=3).map(agent_shell).collect());

    let seen = run(docker, "main", EVERY, 3, None).await;

    let recreate = |service| argv(&["compose", "-p", "proj", "up", "-d", "--force-recreate", "--no-deps", service]);
    assert_eq!(
        seen.compose_calls(),
        [(30, recreate("agent-dind")), (60, recreate("agent-droid"))],
        "the dind alone once it has been unhealthy on two sweeps, its sidecar untouched; then the droid daemon whose network has been gone on two sweeps, once (cooldown); a stopped container is left alone"
    );
    let reasons: Vec<(&str, &str)> = seen.named("watchdog.recreated").iter().map(|r| (r.get("container"), r.get("reason"))).collect();
    assert_eq!(reasons, [("agent-dind", "unhealthy"), ("agent-droid", "orphaned_network")]);
}

/// The reporting-service sidecar on 24 Oct: a redeploy's compose stop read it unhealthy for one sweep, and the recreate the watchdog
/// started raced the redeploy's own ("container name already in use"). Unhealthy on one sweep, or on two sweeps for two different
/// containers (compose replaced it between them), is not sick.
#[tokio::test]
async fn given_a_container_unhealthy_on_one_sweep_or_replaced_between_two_when_swept_then_it_is_left_alone() {
    let sidecar = |health, id| Container { id: Some(id), ..named("tailscale", Some(health), 0) };
    let sweeps = vec![
        vec![sidecar("healthy", "ts-1")],
        vec![sidecar("unhealthy", "ts-1")],
        vec![sidecar("healthy", "ts-1")],
        vec![sidecar("unhealthy", "ts-1")],
        vec![sidecar("unhealthy", "ts-2")],
        vec![sidecar("healthy", "ts-2")],
    ];

    let seen = run(Docker::new(sweeps), "main", EVERY, 6, None).await;

    assert!(seen.compose_calls().is_empty(), "{:?}", seen.compose_calls());
}

/// The remote-chat-agent stack on 23 Oct: the sidecar is stopped at sweeps 2, 4, 5 and 6 (a redeploy recreating it, a flap, then for
/// good) and the stt in its network, running throughout, has no network owner on those sweeps.
fn flapping_sidecar(k: usize) -> Vec<Container> {
    let tailscale = Container { id: Some("ts"), running: !matches!(k, 2 | 4..=6), ..named("tailscale", Some("healthy"), 0) };
    vec![tailscale, Container { network_of: Some("ts"), ..named("stt", None, 0) }]
}

#[tokio::test]
async fn given_a_container_whose_network_owner_is_gone_when_swept_then_it_is_recreated_only_when_the_same_owner_is_gone_on_two_sweeps_in_a_row() {
    let docker = Docker::new((1..=7).map(flapping_sidecar).collect());

    let seen = run(docker, "main", EVERY, 7, None).await;

    let recreate = argv(&["compose", "-p", "proj", "up", "-d", "--force-recreate", "--no-deps", "stt"]);
    assert_eq!(
        seen.compose_calls(),
        [(120, recreate)],
        "gone at sweep 2 (a compose recreate in flight) and back at 3: nothing; gone at 4: the count starts again; gone at 5: recreated; at 6 and 7 the cooldown holds"
    );
    let reasons: Vec<(&str, &str)> = seen.named("watchdog.recreated").iter().map(|r| (r.get("container"), r.get("reason"))).collect();
    assert_eq!(reasons, [("stt", "orphaned_network")]);
}

/// The virtual clock's first sweep, which `Container::finished_at` is read against: 2023-11-14T22:13:20Z.
const FIRST_SWEEP: &str = "2023-11-14T22:13:20Z";
const CONTROL_PLANE_FILES: &[&str] = &["/home/ubuntu/vm-ops-api/docker-compose.yml"];

fn plane(name: &'static str, service: &'static str, running: bool, finished_at: Option<&'static str>) -> Container {
    Container {
        project: Some("vm-ops-api"),
        service: Some(service),
        working_dir: Some("/home/ubuntu/vm-ops-api"),
        config_files: CONTROL_PLANE_FILES,
        running,
        finished_at,
        ..app(name, None, 0)
    }
}

/// The control plane's project at sweep `k` (30 s apart): a sidecar stopped 60 s after the first sweep, the Node service stopped at
/// the first sweep and started again by the watchdog's recreate (a new container, running from sweep 7), the Rust service stopped
/// at the first sweep whose compose file no longer declares it, the watchdog itself stopped, and another project's app stopped.
fn control_plane(k: usize) -> Vec<Container> {
    vec![
        plane("vm-ops-api-tailscale", "tailscale", false, Some("2023-11-14T22:14:20Z")),
        plane("vm-ops-api-ops-api-1", "ops-api", k >= 7, (k < 7).then_some(FIRST_SWEEP)),
        plane("vm-ops-api-ops-api-rs-1", "ops-api-rs", false, Some(FIRST_SWEEP)),
        plane("watchdog", "watchdog", false, Some(FIRST_SWEEP)),
        Container { running: false, finished_at: Some(FIRST_SWEEP), ..app("other-app-1", Some("healthy"), 0) },
    ]
}

#[tokio::test]
async fn given_stopped_control_plane_containers_when_swept_then_each_still_declared_is_recreated_once_it_has_been_stopped_over_two_minutes() {
    let mut docker = Docker::new((1..=10).map(control_plane).collect());
    docker.declares = vec![("vm-ops-api", &["tailscale", "ops-api", "watchdog"])];

    let seen = run(docker, "main", EVERY, 10, None).await;

    let recreate = |service| argv(&["compose", "-p", "vm-ops-api", "--project-directory", "/home/ubuntu/vm-ops-api", "-f", "/home/ubuntu/vm-ops-api/docker-compose.yml", "up", "-d", "--force-recreate", "--no-deps", service]);
    assert_eq!(
        seen.up_calls(),
        [(150, recreate("ops-api")), (210, recreate("tailscale"))],
        "the Node service 150 s in (a stop of exactly two minutes, at the fifth sweep, is not more); the sidecar 150 s after it stopped; never the Rust service, the watchdog or another project's app"
    );
    let reasons: Vec<(&str, &str, &str)> = seen.named("watchdog.recreated").iter().map(|r| (r.get("container"), r.get("reason"), r.get("ok"))).collect();
    assert_eq!(reasons, [("vm-ops-api-ops-api-1", "exited", "true"), ("vm-ops-api-tailscale", "exited", "true")]);
    // And its compose file is read once per sweep from the first one with a container stopped long enough, never before
    let config = argv(&["compose", "-p", "vm-ops-api", "--project-directory", "/home/ubuntu/vm-ops-api", "-f", "/home/ubuntu/vm-ops-api/docker-compose.yml", "config", "--services"]);
    let reads: Vec<u64> = seen.compose_calls().into_iter().filter(|(_, call)| *call == config).map(|(at, _)| at).collect();
    assert_eq!(reads, [150, 180, 210, 240, 270]);
    assert!(seen.named("watchdog.sweep_failed").is_empty(), "{:?}", seen.records);
}

#[tokio::test]
async fn given_a_compose_file_that_cannot_be_read_when_swept_then_nothing_stopped_is_started_and_each_sweep_says_why() {
    let mut docker = Docker::new(vec![control_plane(1)]);
    docker.config = Some(Failure { code: 1, stderr: "no configuration file provided: not found\n" });

    let seen = run(docker, "main", EVERY, 8, None).await;

    assert!(seen.up_calls().is_empty(), "{:?}", seen.up_calls());
    let failures: Vec<(&str, &str)> = seen.named("watchdog.sweep_failed").iter().map(|r| (r.get("error"), r.get("hostLabel"))).collect();
    let said = ("docker compose config failed: no configuration file provided: not found", "main");
    assert_eq!(failures, [said; 3], "one per sweep from the sixth, when the Node service has been stopped over two minutes: {failures:?}");
}

#[tokio::test]
async fn given_a_container_that_stops_again_after_its_recreate_when_swept_then_the_ten_minute_cooldown_holds() {
    let again = |k: usize| {
        let mut plane = control_plane(k);
        plane.retain(|c| c.service == Some("ops-api"));
        if k >= 9 {
            plane[0] = plane_stopped_again();
        }
        plane
    };
    let mut docker = Docker::new((1..=27).map(again).collect());
    docker.declares = vec![("vm-ops-api", &["ops-api"])];

    let seen = run(docker, "main", EVERY, 27, None).await;

    let times: Vec<u64> = seen.up_calls().into_iter().map(|(at, _)| at).collect();
    assert_eq!(times, [150, 750], "recreated at 150 s; stopped again at 240 s, over two minutes at 360 s, but not recreated before the cooldown ends at 750 s");
}

fn plane_stopped_again() -> Container {
    plane("vm-ops-api-ops-api-1", "ops-api", false, Some("2023-11-14T22:17:20Z"))
}

const STACK_FILES: &[&str] = &["/home/ubuntu/remote-chat-agent/docker-compose.yml"];

/// The stack a redeploy left half way: every container created at the first sweep and none started, next to a project with one
/// container running and one created.
fn created_stack(_k: usize) -> Vec<Container> {
    let created = |name: &'static str, project: &'static str, running: bool| Container {
        project: Some(project),
        service: Some(name),
        working_dir: Some("/home/ubuntu/remote-chat-agent"),
        config_files: STACK_FILES,
        running,
        created_at: (!running).then_some(FIRST_SWEEP),
        ..app(name, None, 0)
    };
    vec![
        created("tailscale", "remote-chat-agent", false),
        created("stt", "remote-chat-agent", false),
        created("remote-chat-agent", "remote-chat-agent", false),
        created("web", "half-up", true),
        created("worker", "half-up", false),
    ]
}

#[tokio::test]
async fn given_a_project_whose_containers_were_all_created_and_never_started_when_swept_then_it_is_brought_up_once_per_cooldown() {
    let docker = Docker::new((1..=27).map(created_stack).collect());

    let seen = run(docker, "main", EVERY, 27, None).await;

    let up = argv(&["compose", "-p", "remote-chat-agent", "--project-directory", "/home/ubuntu/remote-chat-agent", "-f", "/home/ubuntu/remote-chat-agent/docker-compose.yml", "up", "-d"]);
    assert_eq!(
        seen.compose_calls(),
        [(150, up.clone()), (750, up)],
        "one plain `up -d` for the stack, not one per container, at the first sweep over two minutes after it was created (120 s is not over) and again after the cooldown; the project with a container running is left alone"
    );
    let reasons: Vec<(&str, &str)> = seen.named("watchdog.recreated").iter().map(|r| (r.get("container"), r.get("reason"))).collect();
    assert_eq!(reasons, [("remote-chat-agent", "created"), ("remote-chat-agent", "created")]);
}
