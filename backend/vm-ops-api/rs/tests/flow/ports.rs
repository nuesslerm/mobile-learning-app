//! The two listeners: the CI port serves only what a CI job needs and answers its own seed alone, the admin port serves every
//! route and answers the admin seed alone. Given/When/Then, through the routers `main` serves.
use super::harness::{legacy_headers, world, World, CI_SEED};
use vm_ops_api_core::application::ports::Event;
use vm_ops_api_core::domain::exposure::{Exposure, ROUTES};

const NOT_FOUND: &str = r#"{"error":"not found"}"#;
const UNAUTHORIZED: &str = r#"{"error":"unauthorized"}"#;

fn off_port(path: &str, method: &str) -> Event {
    Event::warn("http.route_not_on_port").with("port", "ci").with("method", method).with("path", path)
}

fn concrete(path: &str) -> String {
    path.replace("{app}", "leetcode-backend").replace("{run}", "run-1").replace("{job}", "0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab").replace("{name}", "pool-cooldowns.json")
}

fn refused_off_port(world: &World) -> Vec<Event> {
    world.log.events().into_iter().filter(|event| event.message == "http.route_not_on_port").collect()
}

#[tokio::test]
async fn given_the_ci_port_when_a_ci_signed_request_reads_health_and_a_container_list_then_both_answer() {
    // Given the service with both ports
    let world = world(false);

    // When health and a container list are read on the CI port, signed with the CI seed
    let health = world.ci_signed("GET", "/health", b"").await;
    let status = world.ci_signed("GET", "/services/leetcode-backend/ps", b"").await;

    // Then both are 200 and no route was refused as off this port
    assert_eq!((health.status, status.status), (200, 200), "{} / {}", health.body, status.body);
    assert_eq!(refused_off_port(&world), []);
}

#[tokio::test]
async fn given_the_exec_script_route_when_it_is_posted_on_each_port_then_the_admin_port_runs_it_and_the_ci_port_answers_404_naming_port_and_path() {
    // Given the service, and no agent-shell container
    let world = world(false);

    // When the exec-script is posted on the CI port with the CI seed, and on the admin port with the admin seed
    let on_ci = world.ci_signed("POST", "/services/agent-shell/exec-script", b"{}").await;
    let on_admin = world.signed("POST", "/services/agent-shell/exec-script", b"{}").await;

    // Then the CI port says 404 and logs the port and the path, and the admin port reached the route (agent-shell is not running)
    assert_eq!((on_ci.status, on_ci.body.as_str()), (404, NOT_FOUND));
    assert_eq!(refused_off_port(&world), [off_port("/services/agent-shell/exec-script", "POST")]);
    assert_eq!(on_admin.status, 503, "{}", on_admin.body);
    assert!(on_admin.body.contains("agent-shell container is not running"), "{}", on_admin.body);
}

#[tokio::test]
async fn given_every_route_of_the_table_when_a_ci_signed_request_reaches_the_ci_port_then_exactly_the_admin_only_routes_are_404_off_port() {
    // Given the service, and every route as the table lists it
    for spec in ROUTES {
        let world = world(false);
        let path = concrete(spec.path);

        // When the route is requested on the CI port, signed with the CI seed
        let reply = world.ci_signed(spec.method, &path, b"").await;

        // Then an admin-only route is a 404 the log names, and a route the CI port serves is never refused as off this port
        let refused = refused_off_port(&world);
        match spec.exposure {
            Exposure::AdminOnly => {
                assert_eq!((reply.status, reply.body.as_str()), (404, NOT_FOUND), "{} {path}", spec.method);
                assert_eq!(refused, [off_port(&path, spec.method)], "{} {path}", spec.method);
            }
            Exposure::Shared | Exposure::CiOnly => assert_eq!(refused, [], "{} {path}", spec.method),
        }
    }
}

#[tokio::test]
async fn given_a_public_admin_route_when_it_is_read_on_the_ci_port_then_it_is_401_without_credentials_and_never_served() {
    // Given the service
    let world = world(false);

    // When the opencode run list, a public route of the admin port, is read with no credentials on each port
    let on_admin = world.send("GET", "/opencode-log/api/runs", &[], b"").await;
    let on_ci = world.send_on(&world.ci_router, "GET", "/opencode-log/api/runs", &[], b"").await;

    // Then the admin port serves it and the CI port treats it as no route at all, so it asks for credentials first
    assert_eq!(on_admin.status, 200, "{}", on_admin.body);
    assert_eq!((on_ci.status, on_ci.body.as_str()), (401, UNAUTHORIZED));
}

#[tokio::test]
async fn given_a_seed_when_it_is_presented_on_the_other_port_then_it_is_401_and_nothing_runs() {
    // Given the service
    let world = world(false);

    // When a CI-signed request reaches the admin port and an admin-signed one the CI port, for a route both serve
    let ci_on_admin = world.ci_signed_on_admin("GET", "/health", b"").await;
    let admin_on_ci = world.admin_signed_on_ci("GET", "/health", b"").await;
    // And for a route only the admin port serves, where the CI seed also fails
    let ci_on_admin_exec = world.ci_signed_on_admin("POST", "/services/agent-shell/exec-script", b"{}").await;

    // Then each is 401 with no detail, and the seed's own port still lets it in
    for reply in [&ci_on_admin, &admin_on_ci, &ci_on_admin_exec] {
        assert_eq!((reply.status, reply.body.as_str()), (401, UNAUTHORIZED));
    }
    assert_eq!(world.ci_signed("GET", "/health", b"").await.status, 200);
    assert_eq!(world.signed("GET", "/health", b"").await.status, 200);
}

#[tokio::test]
async fn given_the_raw_seeds_when_they_are_presented_then_the_ci_port_takes_neither_and_the_admin_port_only_its_own() {
    // Given the admin port in its migration posture, which still takes its raw seed
    let world = world(false);
    let raw_ci = vec![("authorization".to_string(), format!("Bearer {CI_SEED}"))];

    // When the raw seeds are presented to each port
    let admin_raw_on_ci = world.send_on(&world.ci_router, "GET", "/health", &legacy_headers(), b"").await;
    let ci_raw_on_ci = world.send_on(&world.ci_router, "GET", "/health", &raw_ci, b"").await;
    let ci_raw_on_admin = world.send("GET", "/health", &raw_ci, b"").await;
    let admin_raw_on_admin = world.send("GET", "/health", &legacy_headers(), b"").await;

    // Then only the admin seed on the admin port passes
    assert_eq!([admin_raw_on_ci.status, ci_raw_on_ci.status, ci_raw_on_admin.status, admin_raw_on_admin.status], [401, 401, 401, 200]);
}
