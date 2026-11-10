//! Flow tests of the reporting service: each request enters through the router `main` serves, built by `App` from ports whose
//! outside world is doubled (see `flow/harness.rs`). Given/When/Then.
#[path = "flow/harness.rs"]
mod harness;

use axum::http::{Method, StatusCode};
use harness::{call, eventually, get, Beats, Container, Doubles, Proc, ScriptedRunner, TestClock, World, DAY, HOUR, MINUTE, NOW};
use reporting_core::application::{Intervals, RunnerObservation, RunnerSource};
use reporting_service::infrastructure::{GithubRunner, SinkPublisher};
use std::sync::Arc;
use telemetry::testing::InMemoryMetricsSink;
use reporting_core::domain::prune::StoragePolicy;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const MEMINFO: &str = "MemTotal:       8000 kB\nMemFree:        2000 kB\n";

fn intervals(sweeps: u64, prune: u64) -> Intervals {
    let ms = Duration::from_millis;
    Intervals { containers: ms(sweeps), host: ms(sweeps), runner: ms(sweeps), storage: ms(sweeps), prune: ms(prune) }
}

fn by(samples: &Value, scope: &str, name: &str) -> BTreeMap<String, f64> {
    samples["samples"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["scope"] == scope && s["name"] == name)
        .map(|s| (s["metric"].as_str().unwrap().to_string(), s["value"].as_f64().unwrap()))
        .collect()
}

fn metrics(entries: &[(&str, f64)]) -> BTreeMap<String, f64> {
    entries.iter().map(|(metric, value)| (metric.to_string(), *value)).collect()
}

fn pick(value: &Value, keys: &[&str]) -> Value {
    Value::Object(keys.iter().map(|key| (key.to_string(), value[*key].clone())).collect())
}

fn money(value: &Value) -> Option<f64> {
    value.as_f64().map(|cost| (cost * 1e9).round() / 1e9)
}

async fn rows(app: &reporting_service::api::App) -> u64 {
    get(app, "/health").await["rows"].as_u64().unwrap()
}

fn stats(cpu: f64, precpu: f64) -> Value {
    json!({
        "cpu_stats": { "cpu_usage": { "total_usage": cpu }, "system_cpu_usage": 2000, "online_cpus": 4 },
        "precpu_stats": { "cpu_usage": { "total_usage": precpu }, "system_cpu_usage": 1000 },
        "memory_stats": { "usage": 1000, "limit": 4000 },
        "blkio_stats": { "io_service_bytes_recursive": [{ "op": "read", "value": 500 }, { "op": "Write", "value": 2500 }] }
    })
}

fn seed_history(world: &World) {
    world.seed(&[
        (NOW - MINUTE, "host", "host", "cpu", 50.0),
        (NOW - 2 * MINUTE, "host", "host", "cpu", 90.0),
        (NOW - MINUTE, "host", "host", "memory_used", 3.0 * GIB),
        (NOW - MINUTE, "host", "host", "disk_used", 10.0 * GIB),
        (NOW - MINUTE, "container", "web", "memory_used", 999.0),
        (NOW - 12 * HOUR, "host", "host", "cpu", 2.0),
        (NOW - 3 * DAY, "host", "host", "cpu", 3.0),
        (NOW - 20 * DAY, "host", "host", "cpu", 4.0),
    ]);
}

fn write_plans(world: &World) {
    world.write("libraries/rca-core/data/model-pricing.json", &json!({ "model-a": { "inputPerMTok": 3, "outputPerMTok": 15, "cacheReadPerMTok": 0.3 }, "model-b": { "inputPerMTok": 1, "outputPerMTok": 2 } }).to_string());
    let runs = [
        json!({ "ts": "2026-10-01T10:00:00Z", "runId": "r1", "model": "model-a", "pool": "p", "cost": 0.5, "durationMs": 1000, "tokens": { "input": 1_000_000, "output": 100_000, "cacheRead": 1_000_000, "total": 2_100_000 } }),
        json!({ "ts": "2026-10-01T11:00:00Z", "runId": "r2", "model": "unpriced", "cost": 0.25, "tokens": { "total": 10 } }),
        json!({ "ts": "2026-10-02T09:00:00Z", "runId": "r3", "model": "model-b", "tokens": { "input": 500_000, "output": 500_000, "total": 1_000_000 } }),
        json!({ "ts": "not-a-date", "runId": "r4" }),
    ];
    let log: String = runs.iter().map(|run| format!("{run}\n")).collect();
    world.write(".agents/tmp/opencode-dispatch-runs.jsonl", &format!("{log}{{\"torn\": \n\n{{\"ts\": 5, \"runId\": \"bad-ts\"}}\n"));
    let zeta = json!({
        "slug": "zeta", "startedAt": "2026-10-01T00:00:00Z", "finishedAt": "2026-10-03T00:00:00Z",
        "units": [
            { "id": "u1", "title": "First", "tier": "small", "status": "done", "seconds": 12, "opencodeUsd": 0.75, "opencodeTokens": 99, "runIds": ["r1", "r2", "missing"] },
            { "id": "u2", "title": "Second", "runIds": ["r3", "r4"] },
            { "id": "u3", "title": "No runs" }
        ]
    });
    for (dir, manifest) in [
        ("zeta", zeta.to_string()),
        ("alpha", json!({ "slug": "alpha", "units": [] }).to_string()),
        ("with space", json!({ "slug": "with space", "units": [] }).to_string()),
        ("torn", r#"{"slug": "torn", "units": ["#.to_string()),
        ("no-slug", json!({ "units": [] }).to_string()),
        ("no-units", json!({ "slug": "no-units" }).to_string()),
    ] {
        world.write(&format!("docs/plans/{dir}/manifest.json"), &manifest);
    }
}

#[tokio::test]
async fn given_a_running_service_when_a_caller_reads_it_empty_then_with_history_then_with_plans_then_every_report_answers_as_promised() {
    let world = World::new().await;
    let app = world.app(Doubles { commit: Some("abc1234"), ..Default::default() });

    let health = call(&app, Method::GET, "/health").await;
    let refused = [
        call(&app, Method::POST, "/health").await,
        call(&app, Method::DELETE, "/api/headroom").await,
        call(&app, Method::GET, "/api/nope").await,
        call(&app, Method::GET, "/api/plans/a/b").await,
    ];
    let bad_windows = [call(&app, Method::GET, "/api/metrics/series?window=90d").await, call(&app, Method::GET, "/api/metrics/series?window=").await];

    assert_eq!((health.status, health.content_type.as_deref()), (StatusCode::OK, Some("application/json")));
    assert_eq!(pick(&health.body, &["status", "rows", "commit"]), json!({ "status": "ok", "rows": 0, "commit": "abc1234" }));
    assert!(health.body["uptime"].is_u64(), "{health:?}");
    for reply in refused {
        assert_eq!((reply.status, reply.body), (StatusCode::NOT_FOUND, json!({ "error": "not_found" })));
    }
    for reply in bad_windows {
        assert_eq!((reply.status, reply.body), (StatusCode::BAD_REQUEST, json!({ "error": "invalid_window", "accepted": ["1h", "24h", "7d", "30d"] })));
    }
    assert_eq!(get(&app, "/api/metrics/current").await, json!({ "samples": [] }));
    assert_eq!(get(&app, "/api/plans").await, json!({ "plans": [] }));
    assert_eq!(
        get(&app, "/api/headroom").await,
        json!({ "resources": [
            { "resource": "cpu", "used": 0.0, "ceiling": 2.0, "remaining": 2.0, "unit": "ocpu" },
            { "resource": "memory", "used": 0.0, "ceiling": 12.0 * GIB, "remaining": 12.0 * GIB, "unit": "bytes" },
            { "resource": "disk", "used": 0.0, "ceiling": 150.0 * GIB, "remaining": 150.0 * GIB, "unit": "bytes" }
        ] })
    );

    seed_history(&world);
    let current = get(&app, "/api/metrics/current").await;
    let headroom = get(&app, "/api/headroom").await;
    let default_window = get(&app, "/api/metrics/series").await;
    let mut windows = Vec::new();
    for window in ["1h", "24h", "7d", "30d"] {
        windows.push(get(&app, &format!("/api/metrics/series?window={window}")).await);
    }

    assert_eq!(rows(&app).await, 8, "every stored sample");
    let mut newest = current["samples"].as_array().unwrap().clone();
    newest.sort_by_key(|s| (s["scope"].to_string(), s["metric"].to_string()));
    assert_eq!(
        Value::Array(newest),
        json!([
            { "ts": NOW - MINUTE, "scope": "container", "name": "web", "metric": "memory_used", "value": 999.0 },
            { "ts": NOW - MINUTE, "scope": "host", "name": "host", "metric": "cpu", "value": 50.0 },
            { "ts": NOW - MINUTE, "scope": "host", "name": "host", "metric": "disk_used", "value": 10.0 * GIB },
            { "ts": NOW - MINUTE, "scope": "host", "name": "host", "metric": "memory_used", "value": 3.0 * GIB }
        ])
    );
    assert_eq!(
        headroom["resources"],
        json!([
            { "resource": "cpu", "used": 1.0, "ceiling": 2.0, "remaining": 1.0, "unit": "ocpu" },
            { "resource": "memory", "used": 3.0 * GIB, "ceiling": 12.0 * GIB, "remaining": 9.0 * GIB, "unit": "bytes" },
            { "resource": "disk", "used": 10.0 * GIB, "ceiling": 150.0 * GIB, "remaining": 140.0 * GIB, "unit": "bytes" }
        ])
    );
    assert_eq!(default_window["window"], "1h");
    let shape = |series: &Value| -> Vec<(String, String, i64, f64)> {
        series["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["granularity"].as_str().unwrap().into(), p["metric"].as_str().unwrap().into(), NOW - p["ts"].as_i64().unwrap(), p["value"].as_f64().unwrap()))
            .collect()
    };
    let point = |granularity: &str, metric: &str, age: i64, value: f64| (granularity.to_string(), metric.to_string(), age, value);
    assert_eq!(
        shape(&default_window),
        [
            point("raw", "cpu", 2 * MINUTE, 90.0),
            point("raw", "memory_used", MINUTE, 999.0),
            point("raw", "cpu", MINUTE, 50.0),
            point("raw", "disk_used", MINUTE, 10.0 * GIB),
            point("raw", "memory_used", MINUTE, 3.0 * GIB),
        ],
        "the hour as stored"
    );
    let granularities: Vec<(&str, &str, usize)> = windows
        .iter()
        .map(|series| {
            let points = series["points"].as_array().unwrap();
            (series["window"].as_str().unwrap(), points[0]["granularity"].as_str().unwrap(), points.len())
        })
        .collect();
    assert_eq!(granularities, [("1h", "raw", 5), ("24h", "minute", 6), ("7d", "hour", 6), ("30d", "hour", 7)], "the longer windows are averaged per minute or hour as asked, nothing is rolled up in advance");
    let host_cpu = |series: &Value| -> Vec<f64> { series["points"].as_array().unwrap().iter().filter(|p| p["scope"] == "host" && p["metric"] == "cpu").map(|p| p["value"].as_f64().unwrap()).collect() };
    assert_eq!(host_cpu(&windows[2]), [3.0, 2.0, 70.0], "the two readings of the last hour are one hour's average");
    assert_eq!(host_cpu(&windows[3]), [4.0, 3.0, 2.0, 70.0]);

    let host_only = get(&app, "/api/metrics/series?window=1h&scope=host").await;
    assert!(host_only["points"].as_array().unwrap().iter().all(|p| p["scope"] == "host") && !host_only["points"].as_array().unwrap().is_empty(), "scope=host leaves out the containers' points");

    let only_cpu = get(&app, "/api/metrics/series?window=24h&scope=host&metric=cpu,disk_used").await;
    let metrics: std::collections::BTreeSet<&str> = only_cpu["points"].as_array().unwrap().iter().map(|p| p["metric"].as_str().unwrap()).collect();
    assert_eq!(metrics.into_iter().collect::<Vec<_>>(), ["cpu", "disk_used"], "only the metrics asked for");
    let recent = get(&app, &format!("/api/metrics/series?window=30d&since={}", NOW - 2 * HOUR)).await;
    assert!(recent["points"].as_array().unwrap().iter().all(|p| p["ts"].as_i64().unwrap() >= NOW - 2 * HOUR - HOUR) && !recent["points"].as_array().unwrap().is_empty(), "nothing from before since (to the hour the points are averaged over)");

    write_plans(&world);
    let plans = get(&app, "/api/plans").await;
    let zeta = get(&app, "/api/plans/zeta").await;
    let alpha = get(&app, "/api/plans/alpha").await;
    let spaced = call(&app, Method::GET, "/api/plans/with%20space").await;
    let missing = [call(&app, Method::GET, "/api/plans/nope").await, call(&app, Method::GET, "/api/plans/torn").await, call(&app, Method::GET, "/api/runs/bad-ts").await, call(&app, Method::GET, "/api/runs/zzz").await];
    let [r1, r2, r3, r4] = [get(&app, "/api/runs/r1").await, get(&app, "/api/runs/r2").await, get(&app, "/api/runs/r3").await, get(&app, "/api/runs/r4").await];
    world.remove("libraries/rca-core/data/model-pricing.json");
    let r1_unpriced = call(&app, Method::GET, "/api/runs/r1").await;

    let slugs: Vec<&str> = plans["plans"].as_array().unwrap().iter().map(|p| p["slug"].as_str().unwrap()).collect();
    assert_eq!(slugs, ["alpha", "with space", "zeta"], "valid manifests only, by slug");
    assert_eq!(plans["plans"][0], json!({ "slug": "alpha", "units": 0, "runIds": 0, "resolved": 0, "unresolved": 0, "unpricedRuns": 0, "days": [] }));
    let zeta_summary = &plans["plans"][2];
    assert_eq!(pick(zeta_summary, &["units", "runIds", "resolved", "unresolved", "unpricedRuns"]), json!({ "units": 3, "runIds": 5, "resolved": 4, "unresolved": 1, "unpricedRuns": 2 }));
    let days: Vec<(String, u64, f64, f64, Option<f64>, u64)> = zeta_summary["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["day"].as_str().unwrap().into(), d["runs"].as_u64().unwrap(), d["tokens"].as_f64().unwrap(), money(&d["cost"]).unwrap(), money(&d["notionalCost"]), d["unpricedRuns"].as_u64().unwrap()))
        .collect();
    assert_eq!(
        days,
        [("2026-10-01".into(), 2, 2_100_010.0, 0.75, Some(4.8), 1), ("2026-10-02".into(), 1, 1_000_000.0, 0.0, Some(1.5), 0), ("unknown".into(), 1, 0.0, 0.0, None, 1)]
    );
    assert_eq!(
        pick(&zeta, &["slug", "startedAt", "finishedAt", "runIds", "resolved", "unresolved", "unpricedRuns"]),
        json!({ "slug": "zeta", "startedAt": "2026-10-01T00:00:00Z", "finishedAt": "2026-10-03T00:00:00Z", "runIds": 5, "resolved": 4, "unresolved": 1, "unpricedRuns": 2 })
    );
    let (u1, u2, u3) = (&zeta["units"][0], &zeta["units"][1], &zeta["units"][2]);
    assert_eq!(
        pick(u1, &["id", "title", "tier", "status", "seconds", "opencodeUsd", "opencodeTokens", "runIds", "resolved", "unresolved"]),
        json!({ "id": "u1", "title": "First", "tier": "small", "status": "done", "seconds": 12.0, "opencodeUsd": 0.75, "opencodeTokens": 99.0, "runIds": ["r1", "r2", "missing"], "resolved": 2, "unresolved": 1 })
    );
    assert_eq!(money(&u1["runs"][0]["notionalCost"]), Some(4.8));
    assert_eq!(
        pick(&u1["runs"][0], &["runId", "ts", "day", "model", "pool", "cost", "tokens", "durationMs", "priced"]),
        json!({ "runId": "r1", "ts": "2026-10-01T10:00:00Z", "day": "2026-10-01", "model": "model-a", "pool": "p", "cost": 0.5, "tokens": 2_100_000.0, "durationMs": 1000.0, "priced": true })
    );
    assert_eq!(pick(&u1["runs"][1], &["runId", "pool", "priced", "notionalCost", "durationMs"]), json!({ "runId": "r2", "pool": null, "priced": false, "notionalCost": null, "durationMs": 0.0 }));
    assert_eq!(pick(u2, &["tier", "status", "seconds", "opencodeUsd", "opencodeTokens"]), json!({ "tier": null, "status": null, "seconds": null, "opencodeUsd": null, "opencodeTokens": null }));
    assert_eq!(pick(u3, &["runIds", "resolved", "unresolved", "runs"]), json!({ "runIds": [], "resolved": 0, "unresolved": 0, "runs": [] }));
    assert_eq!(pick(&alpha, &["slug", "startedAt", "finishedAt", "units"]), json!({ "slug": "alpha", "startedAt": null, "finishedAt": null, "units": [] }));
    assert_eq!((spaced.status, &spaced.body["slug"]), (StatusCode::OK, &json!("with space")));
    for reply in missing {
        assert_eq!((reply.status, reply.body), (StatusCode::NOT_FOUND, json!({ "error": "not_found" })));
    }
    assert_eq!(money(&r1["notionalCost"]), Some(4.8));
    assert_eq!(
        pick(&r1, &["runId", "day", "model", "pool", "cost", "tokens", "durationMs", "priced"]),
        json!({ "runId": "r1", "day": "2026-10-01", "model": "model-a", "pool": "p", "cost": 0.5, "tokens": 2_100_000.0, "durationMs": 1000.0, "priced": true })
    );
    assert_eq!(pick(&r3, &["priced", "notionalCost", "cost", "model", "pool"]), json!({ "priced": true, "notionalCost": 1.5, "cost": 0.0, "model": "model-b", "pool": null }));
    assert_eq!(pick(&r2, &["priced", "notionalCost", "cost"]), json!({ "priced": false, "notionalCost": null, "cost": 0.25 }));
    assert_eq!(pick(&r4, &["priced", "notionalCost", "model", "day", "tokens"]), json!({ "priced": false, "notionalCost": null, "model": null, "day": "unknown", "tokens": 0.0 }));
    assert_eq!((r1_unpriced.status, pick(&r1_unpriced.body, &["priced", "notionalCost"])), (StatusCode::OK, json!({ "priced": false, "notionalCost": null })));
}

#[tokio::test]
async fn given_containers_a_host_and_history_when_the_service_samples_stops_and_restarts_without_docker_then_it_stores_and_keeps_answering() {
    let world = World::new().await;
    {
        let mut engine = world.engine.0.lock().unwrap();
        engine.containers = vec![
            Container { id: "aaa", names: &["/web"], status: "", stats: Ok(stats(200.0, 100.0)) },
            Container { id: "idle", names: &["/idle"], status: "", stats: Ok(stats(100.0, 100.0)) },
            Container {
                id: "perc",
                names: &["/perc"],
                status: "",
                stats: Ok(json!({ "cpu_stats": { "cpu_usage": { "total_usage": 200, "percpu_usage": [1, 2] }, "system_cpu_usage": 2000 }, "precpu_stats": { "cpu_usage": { "total_usage": 100 }, "system_cpu_usage": 1000 }, "memory_stats": { "usage": 1000, "limit": 4000 } })),
            },
            Container { id: "bad", names: &["/bad"], status: "", stats: Err(500) },
            Container { id: "deadbeef", names: &[], status: "", stats: Ok(stats(200.0, 100.0)) },
        ];
        let at = NOW / 1000 + 5;
        engine.events = format!(
            "{}\n{}\n{}\nnot an event\n",
            json!({ "Type": "container", "Action": "oom", "Actor": { "Attributes": { "name": "web" } }, "time": at }),
            json!({ "Type": "container", "Action": "die", "Actor": { "Attributes": { "name": "idle", "exitCode": "137" } }, "time": at }),
            json!({ "Type": "container", "Action": "die", "Actor": { "Attributes": { "name": "perc", "exitCode": "0" } }, "time": at }),
        );
        engine.df = json!({ "Containers": [{ "Id": "a1", "Names": ["/db"], "SizeRw": 10, "SizeRootFs": 100 }, { "Id": "a2", "Names": ["/cache"] }, { "SizeRw": 1 }] });
    }
    world.seed(&[(NOW - 1_000, "container", "old", "cpu", 50.0)]);
    let app = world.app(Doubles { proc: Proc::new(true, vec![MEMINFO], Some((1000.0, 400.0))), clock: TestClock::new(NOW, 1_000), ..Default::default() });

    let scheduler = app.start_sampling(intervals(30, 30));
    let current = eventually(&app, "/api/metrics/current", |c| by(c, "container", "web").contains_key("cpu") && by(c, "container", "cache").contains_key("size_rw") && by(c, "host", "host").len() == 5).await;
    let rows_running = rows(&app).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let rows_later = rows(&app).await;
    scheduler.stop();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let rows_stopped = rows(&app).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let rows_after_stop = rows(&app).await;
    let health = get(&app, "/health").await;

    assert_eq!(by(&current, "container", "web"), metrics(&[("cpu", 40.0), ("memory", 1000.0), ("memory_limit", 4000.0), ("disk_read_bytes", 500.0), ("disk_write_bytes", 2500.0), ("oom", 1.0)]), "the engine's oom event and the container's disk traffic are stored for it");
    assert_eq!(by(&current, "container", "idle")["died"], 137.0, "a failed exit is stored with its code");
    assert!(!by(&current, "container", "perc").contains_key("died"), "a clean exit is not an event worth keeping");
    assert_eq!((by(&current, "container", "idle")["cpu"], by(&current, "container", "perc")["cpu"]), (0.0, 20.0), "a stalled counter is 0; without online_cpus the percpu count is used");
    assert_eq!(by(&current, "container", "bad"), metrics(&[("absent", 1.0)]));
    assert_eq!(by(&current, "container", "deadbeef")["memory"], 1000.0, "a nameless container goes by its id");
    assert_eq!(by(&current, "container", "db"), metrics(&[("size_rootfs", 100.0), ("size_rw", 10.0)]));
    assert_eq!(by(&current, "container", "cache"), metrics(&[("size_rootfs", 0.0), ("size_rw", 0.0)]));
    assert_eq!(current["samples"].as_array().unwrap().iter().filter(|s| s["metric"] == "size_rw").count(), 2, "a df entry without name or id is skipped");
    assert_eq!(
        by(&current, "host", "host"),
        metrics(&[
            ("cpu", 50.0),
            ("cpu_iowait", 10.0),
            ("cpu_kernel", 15.0),
            ("cpu_steal", 20.0),
            ("disk_total", 1000.0 * 1024.0),
            ("disk_used", 600.0 * 1024.0),
            ("load1", 0.5),
            ("memory_total", 8000.0 * 1024.0),
            ("memory_used", 6000.0 * 1024.0),
            ("psi_io", 12.5),
            ("psi_memory", 0.0),
            ("swap_used", 0.0)
        ]),
        "the host's load, pressure (cpu has none on this double) and swap are stored beside cpu, memory and disk"
    );
    assert!(by(&current, "host", "docker").contains_key("list_ms"), "how long the Docker daemon took to list containers is stored");
    let requests = world.engine.requests();
    for expected in ["GET /containers/json", "GET /containers/aaa/stats?stream=false", "GET /system/df?verbose=1"] {
        assert!(requests.iter().any(|r| r == expected), "{expected} in {requests:?}");
    }
    assert_eq!(by(&current, "container", "old")["cpu"], 50.0, "history written before the service started is kept: nothing is cut while the database has room");
    assert!(rows_later > rows_running, "sweeps keep storing while running: {rows_running} then {rows_later}");
    assert_eq!(rows_after_stop, rows_stopped, "no sweep after stop");
    assert_eq!(health["commit"], "unknown");

    drop(app);
    let readings = vec![MEMINFO, "MemTotal:       8000 kB\nMemFree:        1000 kB\n"];
    let restarted = world.app(Doubles { proc: Proc::new(false, readings, None), clock: TestClock::new(NOW + DAY, 0), socket: Some("/nonexistent/docker.sock".into()), ..Default::default() });
    let scheduler = restarted.start_sampling(intervals(30, 3_600_000));
    let current = eventually(&restarted, "/api/metrics/current", |c| by(c, "host", "host").get("memory_used") == Some(&(7000.0 * 1024.0))).await;
    let rows_restarted = rows(&restarted).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let health = call(&restarted, Method::GET, "/health").await;
    scheduler.stop();

    assert_eq!(by(&current, "container", "web")["memory"], 1000.0, "the first run's samples survive the restart");
    assert_eq!(
        by(&current, "host", "host"),
        metrics(&[
            ("cpu", 50.0),
            ("cpu_iowait", 10.0),
            ("cpu_kernel", 15.0),
            ("cpu_steal", 20.0),
            ("disk_total", 1000.0 * 1024.0),
            ("disk_used", 600.0 * 1024.0),
            ("load1", 0.5),
            ("memory_total", 8000.0 * 1024.0),
            ("memory_used", 7000.0 * 1024.0),
            ("psi_io", 12.5),
            ("psi_memory", 0.0),
            ("swap_used", 0.0)
        ]),
        "only meminfo changed, and its newer reading replaced the one at the same instant"
    );
    assert_eq!((health.status, health.body["rows"].as_u64()), (StatusCode::OK, Some(rows_restarted)), "the service answers; failed sweeps store nothing");
}

#[tokio::test]
async fn given_a_database_near_its_budget_when_the_prune_runs_then_only_the_oldest_rows_go_and_only_while_over_the_high_mark() {
    // Given ten rows of 100 bytes against a budget of 1000, with the marks at 90 and 85 percent, and no sweeps running
    let world = World::new().await;
    let history: Vec<(i64, &str, &str, &str, f64)> = (0..10).map(|i| (NOW - (10 - i) * MINUTE, "host", "host", "cpu", i as f64)).collect();
    world.seed(&history);
    let policy = StoragePolicy { budget_bytes: 1000, high_pct: 90, low_pct: 85 };
    let app = world.app(Doubles { policy, bytes_per_row: 100, ..Default::default() });

    // When the prune job runs
    let scheduler = app.start_sampling(Intervals { containers: Duration::from_secs(3600), host: Duration::from_secs(3600), runner: Duration::from_secs(3600), storage: Duration::from_secs(3600), prune: Duration::from_millis(30) });
    let series = eventually(&app, "/api/metrics/series?window=1h", |s| s["points"].as_array().unwrap().len() <= 8).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let later = get(&app, "/api/metrics/series?window=1h").await;
    scheduler.stop();

    // Then the two oldest rows went (to 800 bytes, under the low mark) and later passes found nothing over the high mark
    let values: Vec<f64> = series["points"].as_array().unwrap().iter().map(|p| p["value"].as_f64().unwrap()).collect();
    assert_eq!(values, [2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    assert_eq!(later["points"].as_array().unwrap().len(), 8, "no further cutting under the high mark");
}

#[tokio::test]
async fn given_a_heartbeat_when_the_host_is_swept_then_it_is_pinged_each_sweep_and_no_longer_once_the_service_stops() {
    // Given a service whose heartbeat counts its pings
    let world = World::new().await;
    let beats = std::sync::Arc::new(Beats::default());
    let app = world.app(Doubles { proc: Proc::new(true, vec![MEMINFO], Some((1000.0, 400.0))), beats: beats.clone(), ..Default::default() });

    // When it sweeps the host every 30 ms and is then stopped
    let scheduler = app.start_sampling(intervals(30, 3_600_000));
    for _ in 0..100 {
        if beats.0.load(std::sync::atomic::Ordering::SeqCst) >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler.stop();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let at_stop = beats.0.load(std::sync::atomic::Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Then it was pinged at least three times, and the pings ended with the sweeps
    assert!(at_stop >= 3, "pings while running: {at_stop}");
    assert_eq!(beats.0.load(std::sync::atomic::Ordering::SeqCst), at_stop, "no ping after the service stopped");
}

#[tokio::test(flavor = "multi_thread")]
async fn given_containers_with_and_without_a_healthcheck_when_the_service_samples_then_the_health_of_each_checked_one_is_reported() {
    // Given a healthy container, one whose healthcheck fails, one still starting and one with no healthcheck
    let world = World::new().await;
    {
        let mut engine = world.engine.0.lock().unwrap();
        engine.containers = vec![
            Container { id: "ok", names: &["/runner-ok"], status: "Up 3 minutes (healthy)", stats: Ok(stats(200.0, 100.0)) },
            Container { id: "sick", names: &["/runner-sick"], status: "Up 3 minutes (unhealthy)", stats: Ok(stats(200.0, 100.0)) },
            Container { id: "boot", names: &["/runner-boot"], status: "Up 5 seconds (health: starting)", stats: Ok(stats(200.0, 100.0)) },
            Container { id: "plain", names: &["/plain"], status: "Up 3 minutes", stats: Ok(stats(200.0, 100.0)) },
        ];
        engine.df = json!({ "Containers": [] });
    }
    let app = world.app(Doubles { proc: Proc::new(true, vec![MEMINFO], Some((1000.0, 400.0))), clock: TestClock::new(NOW, 1_000), ..Default::default() });

    // When the service samples
    let scheduler = app.start_sampling(intervals(30, 30));
    let current = eventually(&app, "/api/metrics/current", |c| by(c, "container", "plain").contains_key("cpu")).await;
    scheduler.stop();

    // Then a passing check reads 1, a failing one 0, and a container still starting or with no check has no health reading
    assert_eq!(by(&current, "container", "runner-ok").get("health"), Some(&1.0), "{current}");
    assert_eq!(by(&current, "container", "runner-sick").get("health"), Some(&0.0), "{current}");
    assert!(!by(&current, "container", "runner-boot").contains_key("health"), "{current}");
    assert!(!by(&current, "container", "plain").contains_key("health"), "{current}");
}

/// A service that sweeps containers and the host into `sink`, over three containers: healthy, unhealthy and without a healthcheck.
async fn sweep_into(world: &World, publisher: Arc<SinkPublisher>) -> Value {
    {
        let mut engine = world.engine.0.lock().unwrap();
        engine.containers = vec![
            Container { id: "ok", names: &["/agent-shell-gh-runner"], status: "Up 3 minutes (healthy)", stats: Ok(stats(200.0, 100.0)) },
            Container { id: "sick", names: &["/sick"], status: "Up 3 minutes (unhealthy)", stats: Ok(stats(200.0, 100.0)) },
            Container { id: "plain", names: &["/plain"], status: "Up 3 minutes", stats: Ok(stats(200.0, 100.0)) },
        ];
        engine.df = json!({ "Containers": [] });
    }
    let app = world.app(Doubles { proc: Proc::new(true, vec![MEMINFO], Some((1000.0, 400.0))), clock: TestClock::new(NOW, 1_000), publisher, ..Default::default() });
    let scheduler = app.start_sampling(intervals(30, 3_600_000));
    eventually(&app, "/api/metrics/current", |c| by(c, "host", "host").contains_key("disk_used") && by(c, "container", "plain").contains_key("cpu")).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    scheduler.stop();
    tokio::time::sleep(Duration::from_millis(150)).await;
    get(&app, "/api/metrics/current").await
}

#[tokio::test(flavor = "multi_thread")]
async fn given_a_metrics_sink_when_the_service_samples_then_the_sink_receives_what_was_stored_named_and_labelled_for_alerting() {
    // Given a sink and containers with and without healthchecks
    let world = World::new().await;
    let sink = Arc::new(InMemoryMetricsSink::default());

    // When the service sweeps
    let stored = sweep_into(&world, Arc::new(SinkPublisher::new(sink.clone()))).await;

    // Then the sink got the health of each checked container under its alert name, the host's figures, and none for the unchecked one
    let readings: Vec<_> = sink.batches().into_iter().flatten().collect();
    let latest = |metric: &str, name: &str| readings.iter().filter(|r| r.metric == metric && r.name == name).max_by_key(|r| r.at).map(|r| (r.scope.clone(), r.value));
    assert_eq!(latest("container.health", "agent-shell-gh-runner"), Some(("container".into(), 1.0)));
    assert_eq!(latest("container.health", "sick"), Some(("container".into(), 0.0)));
    assert_eq!(latest("container.health", "plain"), None);
    assert!(latest("host.docker.list_ms", "docker").is_some_and(|(scope, _)| scope == "host"));
    assert!(latest("host.cpu", "host").is_some());

    // And every figure the store holds now is one the sink was given with the same value
    for sample in stored["samples"].as_array().unwrap() {
        let (scope, name, metric, value) = (sample["scope"].as_str().unwrap(), sample["name"].as_str().unwrap(), sample["metric"].as_str().unwrap(), sample["value"].as_f64().unwrap());
        let name_of = match (scope, name) {
            ("host", "host") => format!("host.{metric}"),
            ("host", n) => format!("host.{n}.{metric}"),
            _ => format!("{scope}.{metric}"),
        };
        assert_eq!(latest(&name_of, name), Some((scope.to_string(), value)), "{sample}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn given_a_sink_that_refuses_every_batch_when_the_service_samples_then_the_sweeps_and_the_stored_rows_carry_on_and_each_refusal_is_counted() {
    // Given a sink that refuses everything
    let world = World::new().await;
    let sink = Arc::new(InMemoryMetricsSink::failing());
    let publisher = Arc::new(SinkPublisher::new(sink.clone()));

    // When the service sweeps for a few ticks
    let stored = sweep_into(&world, publisher.clone()).await;

    // Then the sink was reached more than once (the requests arrived), each refusal was counted, and the store has the same readings as ever
    assert!(sink.attempts() >= 4, "attempts: {}", sink.attempts());
    assert_eq!(publisher.failures(), sink.attempts() as u64);
    assert_eq!(by(&stored, "container", "agent-shell-gh-runner").get("health"), Some(&1.0), "{stored}");
    assert_eq!(by(&stored, "container", "sick").get("health"), Some(&0.0), "{stored}");
    assert!(by(&stored, "host", "host").contains_key("cpu"), "{stored}");
}

fn observed(listener_up: bool, busy: bool, queued_jobs: u64) -> Option<RunnerObservation> {
    Some(RunnerObservation { listener_up, busy, queued_jobs })
}

#[tokio::test(flavor = "multi_thread")]
async fn given_a_runner_that_goes_offline_with_jobs_queued_when_it_is_sampled_then_idle_time_grows_until_it_works_again_and_a_silent_github_reads_unreachable() {
    // Given GitHub answering: idle and offline with nothing queued, no answer, offline with two queued, busy, idle again
    let world = World::new().await;
    let sink = Arc::new(InMemoryMetricsSink::default());
    let script = vec![observed(false, false, 0), None, observed(false, false, 2), observed(true, true, 2), observed(true, false, 1)];
    let app = world.app(Doubles {
        clock: TestClock::new(NOW, 1_000),
        publisher: Arc::new(SinkPublisher::new(sink.clone())),
        runner: Some(Arc::new(ScriptedRunner::new(script))),
        ..Default::default()
    });

    // When the runner is sampled five times (the other sweeps are an hour apart)
    let scheduler = app.start_sampling(Intervals { containers: Duration::from_secs(3600), host: Duration::from_secs(3600), runner: Duration::from_millis(30), storage: Duration::from_secs(3600), prune: Duration::from_secs(3600) });
    for _ in 0..200 {
        if sink.batches().len() >= 5 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    scheduler.stop();

    // Then each batch says what GitHub said: unreachable as the only figure when it did not answer, idle seconds counted from the first idle look and reset by work
    let batches = sink.batches();
    assert!(batches.len() >= 5, "batches: {}", batches.len());
    let figure = |batch: &Vec<telemetry::Reading>, metric: &str| batch.iter().find(|r| r.metric == format!("ci.runner.{metric}")).map(|r| r.value);
    let series = |metric: &str| batches.iter().take(5).map(|b| figure(b, metric)).collect::<Vec<_>>();
    assert_eq!(series("github_reachable"), vec![Some(1.0), Some(0.0), Some(1.0), Some(1.0), Some(1.0)]);
    assert_eq!(series("listener_up"), vec![Some(0.0), None, Some(0.0), Some(1.0), Some(1.0)]);
    assert_eq!(series("queued_jobs_waiting"), vec![Some(0.0), None, Some(2.0), Some(2.0), Some(1.0)]);
    assert_eq!(series("idle_seconds"), vec![Some(0.0), None, Some(2.0), Some(0.0), Some(0.0)]);
    assert!(batches[0].iter().all(|r| r.scope == "runner" && r.name == "vm-main"));
}

#[tokio::test]
async fn given_github_when_the_real_runner_adapter_asks_then_it_sends_the_token_and_reads_the_runner_and_the_queue_and_a_refusal_is_an_error() {
    // Given a fake GitHub that records what it is asked and lists two runners and four queued runs
    let seen = Arc::new(std::sync::Mutex::new(Vec::<(String, Option<String>)>::new()));
    let log = seen.clone();
    let router = axum::Router::new().fallback(move |uri: axum::http::Uri, headers: axum::http::HeaderMap| {
        let log = log.clone();
        async move {
            let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).map(String::from);
            log.lock().unwrap().push((uri.path_and_query().unwrap().to_string(), auth.clone()));
            if auth.as_deref() != Some("Bearer good") {
                return (StatusCode::UNAUTHORIZED, axum::Json(json!({ "message": "Bad credentials" })));
            }
            let body = if uri.path().ends_with("/runners") { json!({ "runners": [{ "name": "other", "status": "online", "busy": true }, { "name": "vm-main", "status": "online", "busy": false }] }) } else { json!({ "total_count": 4, "workflow_runs": [] }) };
            (StatusCode::OK, axum::Json(body))
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    // When the adapter observes with the right token and then with a wrong one
    let good = GithubRunner::new(&base, "owner/repo".into(), "vm-main".into(), "good".into()).unwrap().observe().await;
    let bad = GithubRunner::new(&base, "owner/repo".into(), "vm-main".into(), "bad".into()).unwrap().observe().await;

    // Then our runner (not the other one) is read as online and idle with four queued, the paths are the repository's, and the refusal is an error that did reach GitHub
    assert_eq!(good.unwrap(), RunnerObservation { listener_up: true, busy: false, queued_jobs: 4 });
    assert!(bad.unwrap_err().to_string().contains("401"));
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|(path, auth)| path == "/repos/owner/repo/actions/runners?per_page=100" && auth.as_deref() == Some("Bearer good")), "{seen:?}");
    assert!(seen.iter().any(|(path, _)| path == "/repos/owner/repo/actions/runs?status=queued&per_page=1"), "{seen:?}");
    assert!(seen.iter().any(|(_, auth)| auth.as_deref() == Some("Bearer bad")), "{seen:?}");
}
