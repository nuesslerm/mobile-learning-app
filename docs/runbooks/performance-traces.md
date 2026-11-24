# Local performance traces

Run from the repository root with Python 3, Cargo and a current build of `wf`.

```sh
wf bench trace --budgets clis/app-bench/scenarios/rca-hosts-trace.json -- \
  cargo test --locked --manifest-path clis/Cargo.toml \
    -p orca-serve -p t3-serve --test performance -- --ignored
```

Each protocol client connects to its serve process over loopback. The serve process calls RCA, and an attached RCA executor owns the temporary repository. On each of 30 iterations, Orca resolves and reads a 4 KiB terminal artifact and T3 lists a project directory and reads its 4 KiB file. The parent span times the interaction and child spans time the individual wire calls. The harness checks the contents and observes the calls through its recording proxy. Compilation, process startup and pairing happen outside these spans. Local and CI runs use Cargo's debug test profile. Timings include the test proxy and exclude device rendering and model inference.

The command starts a loopback receiver on a free port, sets the child process's OTLP endpoint and protocol, and accepts OTLP/HTTP JSON at `/v1/traces`. An instrumented workload can replace the Cargo command. Configure its tracer for HTTP/JSON and flush it before exit. Give the fixture a versioned suite name, minimum sample count and a p95 budget in milliseconds per operation. Change the suite name when changing the fixture. A lost batch that leaves too few samples fails the gate.

Local runs append to the versioned `clis/app-bench/history.sqlite`. CI uses a separate database per run so concurrent jobs can publish independently. The database records the Git revision, timestamp, machine profile, trace and parent IDs, operation names, timestamps and verdict. Keep span names static. The local profile includes the machine name. Use an explicit `--profile` for comparable runs on an identified runner and build profile.

```sql
SELECT started, substr(revision,1,8) AS commit_id, profile, name, samples,
       round(p95_ms,3) AS p95_ms, round(budget_ms,3) AS limit_ms, passed
FROM runs JOIN metrics ON metrics.run_id = runs.id
ORDER BY started DESC, name;
```

In the CLI build pipeline, the first passing run on the Ubuntu x64 runner seeds the comparison cache. Each later run must stay within both the committed budget and the first passing p95 plus the larger of 50% or 0.10 ms. Fixed references catch cumulative drift. If GitHub evicts the baseline cache, the committed budgets still apply and the next passing run reseeds it. The runner publishes a `performance-sqlite` artifact even on a budget failure. Download it within the 90-day retention window and merge it into your local history. Merging the same run twice is safe.

```sh
gh run download "$RUN_ID" -n performance-sqlite -D ".performance/import/$RUN_ID"
wf bench trace --merge ".performance/import/$RUN_ID/history.sqlite"
```

To publish p95 gauges, add `--send` before the command separator and supply the existing `BETTERSTACK_METRICS_HOST` and `BETTERSTACK_METRICS_TOKEN` environment variables. The summary reports export status. SQLite commits before the network request and remains available offline.
