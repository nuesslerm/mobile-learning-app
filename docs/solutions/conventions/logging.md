---
title: Logging - BetterStack sources and authoring conventions
date: 2026-08-30
category: conventions
tags: [logging, betterstack, observability]
problem_type: knowledge
module: repo-wide
---

# Logging

> **TWO SOURCES.** BetterStack holds a **Backend & Infra (OTLP)** source (every
> `backend/*` service + the `wf` CLI) and a **Mobile Learning App** source (the
> app's REST forwarder). Each has its OWN credential quadruple — `CH_BACKEND_*`
> and `CH_MOBILE_*` — and querying the wrong one returns zero rows, which looks
> exactly like "nothing was logged". Backend normally has data and is what
> `wf vm logs` defaults to; the mobile source only fills after an EAS publish.
> `cluster not found` means either "no such collection" OR "your credential
> cannot see it" — indistinguishable, so check the source before guessing names.
>
> Renamed from `BETTERSTACK_CH_*`, which matched nothing: the vault delivered
> `CH_*` while the code read `BETTERSTACK_CH_*`.


## Authoring rules (TypeScript)

- **No `console.*`** (oxlint `no-console`). Import `Logger` from `@lib/logging`
  and call it directly; there is no per-module logger to create.
- **Nothing about the call site is passed by hand.** The `@lib/logging/babel`
  plugin (in the app's Babel preset) rewrites every `Logger.<method>(...)` to
  `Logger.withContext({ file, line, fn, fnPath, testID }).<method>(...)` at
  compile time: the file, the line, the enclosing function or component, the
  lexical chain of enclosing functions, and the `testID` in scope.
- **Structured lines**: a static `domain.action` message, variables as fields:
  ```ts
  Logger.info('lecture.submit', { lectureId, ms });
  Logger.withError(err).error('api.fail', { endpoint });
  ```
  An `Error` among the fields is serialised with its name, message and stack.
- **Hot paths** (scroll, keystrokes, gestures) do not log per event; log the
  outcome once.
- **Reanimated worklets** can't call the logger directly: bridge via
  `useAnimatedReaction` + `runOnJS`.
- **Catch blocks** log (`Logger.withError(err).error(...)`) or rethrow, never
  swallow.

Lint rules (in `frontend/libraries/eslint-plugin-mobile-learn`, run by oxlint for the frontend):
`no-unhandled-catch`, an error. Static event names, a log in every callback and metadata on every log line are no
longer lint rules (owner, 20 Oct; `require-structured-log` never fired and went in D4b): log where a failure would otherwise be undiagnosable.

## The TypeScript library (`frontend/libraries/logging`)

```
Logger.*  ->  host LogLayer (configureLogger)  ->  scrubber plugin
          ->  PipelineTransport: buffered batches -> stages (appendToLiveTail, appendToPersistence)
          ->  ConsoleEchoTransport (the real console)
console.* ->  installConsoleCapture -> live tail
```

- `logger/`: `Logger`, `configureLogger`. Without `configureLogger` every call
  is a silent no-op, so the host's wiring module must be the first import.
- `babel/`: the call-site injection plugin and preset.
- `pipeline/`, `liveTail/`, `persistence/`, `install/`: the transport, the
  in-memory tail the debug sheet reads, the storage contract with an in-memory
  implementation, and console capture.
- `primitives/`: buffered sink, clock, secret redaction, and `withTraceparent`
  (the W3C `traceparent` header the voice-edit and web-bridge clients send, so
  backend spans join the app's trace).

The app's wiring is `frontend/mla-tamagui/lib/logger.ts`: the live tail and
the console echo, nothing forwarded off the device. A driven run reads the echoed
lines with `wf browser get-logs --level info` (or `device:get_logs{level='info'}` in a
test), the same on Android; in development a page script can also read the tail as
`__liveTail.getEntries()`.

## Secret + PII scrubbing (automatic, on by default)

`configureLogger()` attaches a **`scrubberPlugin`** to the root layer by
default, so **every** log — every module, every transport (console echo,
persistence, BetterStack) — is scrubbed *before* it leaves, via LogLayer's
`onBeforeDataOut` / `onBeforeMessageOut` hooks (i.e. nothing sensitive is ever
persisted or sent, not even for one frame). This is the auto-enabled answer to
"do we scrub secrets/PII" — you don't wire it per-host.

What it redacts (`createScrubber` in `@lib/logging/primitives`):

1. **Sensitive-keyed fields** — any metadata key containing `token`, `secret`,
   `password`, `cookie`, `authorization`, `apikey`, `privatekey`, `credential`
   (case-insensitive substring) → value replaced with `[REDACTED]`.
2. **Env-var secret values** — the *values* of `process.env` vars whose **name**
   looks sensitive (same patterns) are redacted **wherever they appear** in any
   log string, so an interpolated secret (`` `calling ${process.env.MY_API_TOKEN}` ``)
   is caught even in a free-text message. Only values ≥ 8 chars are used, so a
   secret that's `"true"` can't nuke every `"true"`. Snapshotted at
   `configureLogger` time — call it after secrets are loaded.
3. **PII patterns** — email addresses, JWTs, and credit-card-like digit runs.

Tune or opt out per host:

```ts
configureLogger({ root: () => layer });                      // default: on
configureLogger({ root: () => layer, scrub: false });        // off
configureLogger({ root: () => layer, scrub: {               // customized
  scrubPii: false,
  extraSecretValues: [someRuntimeToken],
  keyPatterns: [...DEFAULT, 'ssn'],
}});
```

> The older `redactSensitiveFields` pipeline **Stage** is a no-op under the
> read-only batch contract (it can't mutate entries) — the plugin is the thing
> that actually redacts. Prefer `scrubberPlugin` / `createScrubber`.

## Rust services, `wf` and shell scripts → BetterStack (`clis/telemetry`)

Every Rust process logs through one crate, `clis/telemetry`, built by both cargo
workspaces. A caller states its service name once; everything else a record
carries is inferred:

- **Service subscriber** (feature `service`, used through `service-kit`): one
  line in `main`, `let telemetry = service_kit::telemetry::init!("leetcode-backend");`,
  keeps the guard alive until exit. Then plain `tracing::info!(...)` anywhere:
  JSON lines on the console (errors on stderr) with module, file and line, and
  OTLP logs and spans to BetterStack when a destination is configured.
- **One-shot records** (feature `oneshot`, used by `wf` and the desktop listener):
  `Record::new(level, "event.name").attr("key", value)` is `#[track_caller]`, so
  each record names the file and line that made it; `oneshot::send` posts one
  request and returns `HTTP <status>` on a rejection; `Record::line` is the same
  scrubbed JSON console line (`level`, `ts`, `msg`, fields, `filename`,
  `line_number`).
- **Inferred, never passed**: `service.version` (the calling crate's
  `CARGO_PKG_VERSION`, compiled in by `source!`), `vcs.ref.head.revision`
  (`GIT_COMMIT`), `host.name`, `os.type` as resource attributes, and
  `code.namespace`, `code.filepath`, `code.lineno` on every record.
- **Destination**: `BETTERSTACK_OTLP_TOKEN` / `BETTERSTACK_OTLP_HOST` — a
  **separate** BetterStack source from the one the app bundle uses (a leak from a
  publicly shipped bundle must never be able to forge backend- or CI-attributed
  telemetry). The host must be a bare host name with an optional port; anything
  else, either variable unset, or `SECRETS_LOG_BETTERSTACK=false` means
  console only. Every backend's `docker-compose.prod.yml` must pass both through.
- **Scrubbing** applies to every sink: a field whose name looks like a credential
  is replaced whole, the value of every secret-named environment variable is
  replaced wherever it appears, then emails, JWTs and card numbers.
- **Delivery**: services export in the background and never die because the
  ingest is down.
- **One line per request**: `service_kit::trace::log_request`, layered inside
  the trace layer, writes `http.request` (method, path, status, latencyMs) in the
  request's span, so every line carries its caller's trace id.
- **Proof at the sink**: each service's `rs/tests/deployed_verification.rs` (its
  `test:verify:deployed`, run by CI's deployment-verification stage after the deploy) requests
  the deployed service in a fresh trace and polls the backend logs table until
  that request's `http.request` line arrives, carrying the inferred attributes
  above (`service_kit::e2e`).
- **Shell scripts** (cloud-init, heartbeat, vm-ops-api) call the static `logger`
  binary (`clis/telemetry`'s `logger` bin), or `wf log emit` where wf is
  installed: `logger emit --service <name> --message <event.name>
  [--level info|debug|warn|error] [--host-label light|heavy] [--attr key=value ...]`.
  Both transports run at once: the scrubbed record is printed as a JSON line
  (stderr for warn and error) and sent to BetterStack. The last stdout line is
  `{"sent": bool, "reason"?: string}`; a line that could not be sent (no
  destination, or the ingest refused it) exits `1` with the reason. A caller
  that must go on regardless says so itself (`|| true`, or by reading the
  result line).

## Querying BetterStack (the gotcha)

BetterStack's ClickHouse does **not** expose your data as a plain local table.
`SELECT … FROM t550358_mobile_learning_app_logs` returns
`UNKNOWN_TABLE` — that's expected and does **not** mean logs are missing. You must
go through the **`remote(...)`** (hot) and **`s3Cluster(...)`** (archive) table
functions. The collection is the table name minus the `_logs` suffix
(e.g. `t550358_mobile_learning_app`).

```bash
# Host/user/pass/table come from the source's "Connect remotely → ClickHouse" page
# (CH_BACKEND_HOST / _USER / _PASSWORD / _TABLE).

# Recent / hot data:
curl -sS -u "$CH_BACKEND_USER:$CH_BACKEND_PASSWORD" \
  "https://$CH_BACKEND_HOST/" \
  -G --data-urlencode \
  "query=SELECT dt, raw FROM remote(t550358_mobile_learning_app_logs) ORDER BY dt DESC LIMIT 50 FORMAT JSONEachRow"

# Archived data (older than the hot window):
#   SELECT dt, raw FROM s3Cluster(primary, t550358_mobile_learning_app_s3) WHERE _row_type = 1

# Just crashes / errors:
#   SELECT dt, raw FROM remote(t550358_mobile_learning_app_logs)
#   WHERE level = 'error' ORDER BY dt DESC LIMIT 50
```

Query both halves: BetterStack archives a source's rows every so often, so a quiet
source (the mobile app) can have an empty hot table and everything in the archive.
`wf vm logs` reads the union of the two, so `wf vm logs --source mobile --since 2d
--grep app.boot` finds a CI Android unit's boot however old it is.

Each row's `raw` is the full JSON we sent (`dt`, `level`, `message`, `module`,
`app`, `platform`, plus any structured `context` fields).

> Note: the ingest host id (`s…`) and the ClickHouse table id (`t…`) are
> different numbers **for the same source** — that's normal, not a mismatch.

## Adding a host

1. Add `@lib/logging` as a workspace dependency.
2. Build a `LogLayer` with the transports you want, e.g.
   `new PipelineTransport({ serialize, stages: [appendToLiveTail()] })` and
   `new ConsoleEchoTransport({ console: hostConsole })`.
3. Call `configureLogger({ root: () => layer })` once at start-up, before any
   other import logs.
4. Add `@lib/logging/babel-preset` to the host's Babel config so call sites are
   injected.

## wf usage rows (`HEALTH_RPT.WF_USAGE`)

Every wf command leaves one anonymised row in the Autonomous DB table
`HEALTH_RPT.WF_USAGE` (`infra/oracle-cloud/sql/wf_usage.sql`, ORDS alias
`wf_usage`):

- **Fields**: the command path (`secret get`), exit code, duration, the last
  `wf:` error line it printed, wf's version and the OS.
- **Never stored**: the host, the arguments, or a key name.
- **Scrubbing**: the error is scrubbed like every log sink, with the home
  directory written as `~`.
- **Writer**: a detached copy of wf posts the row (`clis/wf/src/usage_row.rs`), so a
  command never waits on it.
- **Configuration**: `WF_USAGE_ORDS_BASE_URL` and `HEALTH_REPORTS_ORDS_PASSWORD`,
  read from the environment (shell, repo `.env`, `~/.config/wf/.env`), then
  from the vault's sealed local copy when they are not there: the CI runner
  holds only the vault read token, and its commands record rows that way.
- **Failures are loud**:
  - unset configuration prints one stderr line per command;
  - a post that failed is printed by the next command;
  - `WF_USAGE=off` turns rows off (the flow-test harness sets it).

Most used commands, the last week:

```sql
SELECT command, COUNT(*) AS runs, SUM(CASE WHEN exit_code <> 0 THEN 1 ELSE 0 END) AS failed
FROM HEALTH_RPT.WF_USAGE WHERE ts > SYSTIMESTAMP - INTERVAL '7' DAY
GROUP BY command ORDER BY runs DESC;
```
