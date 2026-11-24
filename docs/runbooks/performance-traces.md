# CLI performance tracing

Use a current build of `wf` and Python 3. To record an ordinary command in the repository's SQLite history, put `--performance` before the command. The command keeps its stdout and exit status. The capture summary goes to stderr.

```sh
wf --performance rca bearers list
```

To run the performance cases in the Orca and T3 integration suites, use the same option on the test runner. This prints a JSON verdict and exits nonzero if a test fails, a required operation has too few samples, or its p95 exceeds its limit.

```sh
wf test run --performance
```

Each test pairs a protocol client with a real serve process on loopback. The file workloads resolve and read a 4 KiB Orca terminal artifact or list and read a T3 project file through an RCA service and executor. The chat workloads fetch an Orca session history page and a T3 thread projection from a seeded session. Each workload runs 30 times after setup. The client checks the returned data. Server dispatch and outgoing API calls emit their own spans on the same code paths used during normal operation. HTTP upgrade spans end when the handshake completes. Debug builds, the recording proxy and local sockets are part of this measurement. Device rendering and model inference are outside it.

## History

The loopback receiver accepts OTLP/HTTP JSON at `/v1/traces`. Its default database is `clis/app-bench/history.sqlite`. Each run records its Git revision, timestamp, machine profile, suite and verdict. Each span records its operation, IDs and timing. The `span_sources` table adds service and available HTTP or external-client call sites to new captures. Parent IDs connect nested spans within an instrumented process. Use the run ID to find the other processes in the same workload.

```sql
SELECT started, revision, profile, name, samples,
       round(p95_ms, 2) AS p95_ms, round(budget_ms, 2) AS limit_ms, passed
FROM runs JOIN metrics ON metrics.run_id = runs.id
WHERE name = 'orca.session.history'
ORDER BY started DESC;
```

Set `WF_PERFORMANCE_DB` to choose another file and `WF_PERFORMANCE_PROFILE` to name a comparable machine and build profile. For the suite, `--db` and `--profile` also work. Set `WF_PERFORMANCE_SEND=1` or pass `--send` to the performance suite to export p95 gauges using `BETTERSTACK_METRICS_HOST` and `BETTERSTACK_METRICS_TOKEN`. The verdict reports the delivery status. SQLite commits before export.

## CI

The CLI build pipeline runs these integration cases on Ubuntu x64 on pushes to dev. The first passing run pins a baseline for this fixture and runner. Each required operation must meet the ceiling in `clis/app-bench/scenarios/rca-hosts-trace.json` and stay within the baseline p95 plus the greater of 50% or the minimum change for that operation. The default minimum is 0.10 ms. Executor file reads and artifact stats use 3 ms because consecutive local runs showed 1–2 ms of p95 variation in those operations. The fixed reference catches cumulative drift. If GitHub evicts the baseline cache, the absolute limits still apply and the next passing run reseeds it.

Each job publishes a `performance-sqlite` artifact, including on budget failures. Importing an artifact twice preserves one copy of each run. Change the suite name and cache key together when the fixture changes.

```sh
gh run download "$RUN_ID" -n performance-sqlite -D ".performance/import/$RUN_ID"
wf test run --performance --merge ".performance/import/$RUN_ID/history.sqlite"
```
