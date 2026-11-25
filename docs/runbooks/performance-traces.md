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

## Per-call API report

Run the host and RCA catalog on the same machine and build profile for comparable results.

```sh
wf test run --performance --catalog
wf test run --performance --report .performance/report.html
```

The catalog runner reads the pinned Orca and T3 endpoint inventories, the RCA HTTP router and service method registry, and the executor action table. Each of five rounds starts fresh test hosts. Orca and T3 dispatch recorded schema-valid payloads where available, falling back to minimal inputs. T3 HTTP and the Orca handshake use loopback sockets. RCA service methods receive an empty parameter object on an isolated service. RCA HTTP probes pass through the actual router with empty JSON and no credentials. Executor actions run with their required arguments against the recording executor. The report labels accepted calls, refusals and HTTP statuses. Seven upstream HTTP calls originate on the Orca phone and are listed as external.

CI requires five calls with outcomes for each local endpoint. A first passing catalog run seeds each median. Later runs with the same suite, profile and outcome counts must stay within that median plus the greater of 50% or 5 ms. A decrease in the number of successful calls also fails the gate. The separate full-chain tests continue to enforce 30-sample p95 limits. Both workloads publish their SQLite data and the offline HTML report in the CI artifact.

Open an endpoint in **Endpoint coverage** to inspect individual invocations. **Per-API regression checks** records each comparable median and verdict. **Operation timings** includes every recorded operation, and **Individual calls and spans** lists the time, duration, result, HTTP status, service, call site and trace identifiers where available. The run selector includes older captures. Their missing outcome and source fields are displayed as empty. Traces stay local unless export is enabled. The pinned host inventory is always available, and imported catalog runs add their captured RCA and executor inventories.

## Harness journeys

The device harnesses emit nested OpenTelemetry spans for setup, Lua calls, named steps, waits and teardown. Browser calls include the daemon RPC and iOS calls include the simulator bridge operation. Android includes adb and UI server calls. The flow harness times Vitest and the separate Chromium golden pass. Times include child spans. Summing parents and children double counts their work.

```sh
WF_PERFORMANCE_DB=/tmp/tamil.sqlite wf --performance flow run tests/flow/tamilLessons.flow.test.tsx --json
wf test run --performance --db /tmp/tamil.sqlite --report /tmp/tamil.html
```

For a driver comparison run `wf --performance bench drivers --platform browser --url "$TAMIL_LIBRARY_URL" --out "$NEW_DIRECTORY"` against a prepared web export. On native devices also pass `--device` and `--app-id`. Each pair reverses driver order. Screenshot boundaries mark the ready screen and the completed return. RSS sums sampled host process trees, including the isolated wf browser daemon. Device app memory and shared pages require separate interpretation. Maestro writes command timings to its own JSON. Buoy Bench records app-side frame and rendering measurements separately.

Hosted device and integration jobs retain their SQLite captures in their existing test-run artifacts. A native three-sample benchmark push also runs a fresh Android driver pair per booted emulator. Check the run verdict and the number of observed spans before comparing timings. An Nx cache replay can produce a runner span without fresh device work.
