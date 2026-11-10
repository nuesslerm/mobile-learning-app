# One build cache in blob storage, shared by the sandbox and the VM

Status: proposed, 23 Oct 2026. Nothing in this plan is built. Owner request (23 Oct): a common cache mechanism that stores and
cleans up cache in the blob-storage free tier, with no in-VM caching, so the sandbox can prepopulate it and the VM and the sandbox
share it. Earlier owner question: the cache must survive a VM recreate.

## Goal

One cache component, surfaced as `wf cache`, whose only durable store is an OCI Object Storage bucket. The sandbox and CI use the same
code path against the same bucket. A recreated VM finds its Nx task results and its pnpm store already there. Any failure of the store
(unreachable, over budget, credential expired) makes a run cold, never red.

## Constraints

| Item | Value | Source |
| --- | --- | --- |
| Free requests | 50,000 per month | Oracle Always Free page (verified) |
| Free storage | 20 GB combined if the tenancy is Always-Free-only, 10 GB Standard if it is paid or on trial credits | same page (verified); which one applies here is unverified |
| Enforced quota | 15,000,000,000 B tenancy-wide; used 5,376,323,726; free 9,623,676,274 | `oci limits resource-availability get`, quota set at `infra/oracle-cloud/always-free-quota.tf:121-122` |
| Requests used, 1-23 Oct | 82,036 billed (usage meter, 164 % of free), 82,641 (monitoring metric); 80,914 (98 %) from the verification bucket | `oci monitoring`, usage API |
| What overage costs | billed, never blocked: 0.0101 for October so far, about 0.003 per 10,000 requests (inferred from the meter) | usage API; Oracle price pages returned 403 |
| Why treat 10 GB as the line | the tenancy is charged for requests, so it is not Always-Free-only | inferred; headroom to 10 GB is 4.62 GB |
| Request cost of one Nx task | executed: 1 GET (404) + 1 PUT = 2; replayed from the store: 1 GET | Nx protocol (`node_modules/nx/dist/src/tasks-runner/cache.js:71-98,154-163`) |
| Cache request estimate | 20 to 55 thousand per month: CI about 20 pushes a day x about 35 requests, plus sandbox gates up to 57 commits a day x about 20 | estimate, unmeasured |
| Cache byte estimate | pnpm store 0.6 GB (measured, the existing object is 594,782,669 B) + Nx entries well under 0.2 GB (estimate: no task declares outputs, so an entry is terminal output) | |

Plainly: bytes fit the free tier; requests do not, and will not whatever the cache does, because the run-bundle uploader
(`clis/wf/src/test_publish.rs:239-247`, one PUT per file, about 600 requests per CI run) already uses 164 %. The cache adds an
estimated 0.02 per month at the inferred price. The first budget alert is at 0.50 (`infra/oracle-cloud/budget.tf:45`), about 1.6 million
requests above free.

Budgets this plan enforces (committed in `clis/blob-cache/data/budget.json`): 2 GB stored, 60,000 requests per month, 400 requests and
256 MiB per `wf` run, 64 MiB per Nx entry, 1 GiB per pnpm store.

## Current state

- The Nx remote cache is a listener in vm-ops-api (port 8082, volume `nx-cache`, signed tokens, purge, LRU), 1,621 lines plus tests, landed
  in 9fc7c6986. It is tailnet-only and dies with the VM (`docs/runbooks/vm-ops-api-rust-cutover.md:150`).
- Nothing uses it. No workflow, hook or wf path sets `NX_SELF_HOSTED_REMOTE_CACHE_SERVER` or `_ACCESS_TOKEN` (they appear only in
  `clis/wf/src/nx.rs:35`). CI and the sandbox run on their local `.nx/cache` alone. Removing the in-VM store cannot break a run.
- Writes are refused in both real environments. `may_write` (`clis/wf/src/vm_cache.rs:34-56`) wants node and pnpm equal to `mise.toml`
  (node 24.13.1). The sandbox has node 22.22.0 and CI uses `setup-node` 22 (`.github/workflows/deploy.yml:66,126,212`).
- The pre-commit guard (`clis/wf/src/verify.rs:313-327`) and `may_write` disagree (prefix vs exact match, on-dev check only in one).
- Buckets (eu-frankfurt-1, namespace frbwmn2jazha): pnpm-cache 1 object 0.59 GB (no consumer since 22 Oct; a 7-day rule on `nx-cache/` and
  the IAM policy that every lifecycle rule needs, `infra/oracle-cloud/pnpm-cache.tf:87-99`), verification-artifacts 24,925 objects 4.77 GB
  (public read, 30-day rule), golden-screenshots, tfstate (shares the quota; the VM recovery path).
- Nx 23.2.1 talks to a remote only through `GET|PUT {server}/v1/cache/{hash}` with a bearer token (native client, honours
  `HTTP_PROXY`/`NO_PROXY`). The S3, GCS, Azure and shared-fs cache packages are deprecated by Nx (CVE-2025-36852) and not installed.
- Tasks hash their toolchain: `node --version` and `pnpm --version` for all (`nx.json:6-13`), plus `uname -sm` and `rustc --version` for
  Rust tasks and the Chromium build for browser e2e (`nx.json:14-31`). The flow test `the_sandbox_and_the_runner_hash_alike`
  (`clis/wf/tests/flow/nx_cache.rs:538-593`) guards that checkout path, HOME and PATH do not move a hash.
- `wf test publish` already PUTs from the sandbox and the runner through a prefix PAR resolved by `secrets::load_values` (env, then the
  vault): `clis/wf/src/test_publish.rs:255-261`. The runner service carries the vault read token
  (`infra/oracle-cloud/agent-shell/docker-compose.prod.yml:263`).
- `cli-http` (`clis/http/src/lib.rs`) is the one HTTP client: environment proxy, `download(url, dest)` streams to a file, but a request body
  is a `Vec<u8>` and a response read is capped at 512 MiB (`:11`), below the 595 MB pnpm tarball.

## Design

### Shape

A shim runs where Nx runs, not on the VM. `wf test run --stage` and `wf check --staged` start a loopback HTTP server inside the wf
process (tiny_http, already a wf dependency), point Nx at it, and stop it when Nx exits. The shim speaks the one protocol Nx supports and
translates it to a blob store.

```
nx (native client) --http://127.0.0.1:<port>/v1/cache/<hash>--> nx shim (in wf)
                                                   |  BlobStore port
                                    +--------------+---------------+
                                    | OciParStore (cli-http, PAR)  |  DirStore (tests)
                                    +------------------------------+
```

One new crate, `clis/blob-cache`, in the clis workspace (`clis/Cargo.toml`): `domain/` (no I/O: kinds, keys, generation, budgets, breaker,
write-guard decision), `ports.rs`, `adapters/` (`par.rs` over cli-http, `dir.rs`, `oci_cli.rs`), `nx_server.rs` (driving adapter). wf adds
the `cache` verbs and the wiring in `nx.rs`. Ports are blocking traits, as `docs/solutions/architecture/hexagonal-architecture-by-package-type.md` asks.

Two ports, so the credential that runs on every push cannot delete:

- `BlobStore` (data plane): `get(key)`, `put(key, body, if_absent)`, `exists(key)`. Adapter: pre-authenticated requests.
- `BlobAdmin` (operator plane): `usage()`, `list(prefix)`, `delete(keys)`. Adapter: the `oci` CLI with the admin key in `~/.oci`
  (oci 3.95.0 works from the sandbox, read-only calls verified). Used by `wf cache status|gc` only, never in a CI hot path.

### What the shim does for Nx

The shim answers Nx with 200 or 404 only. Measured 23 Oct against Nx 23.2.1 (a probe server, then the shim in
`clis/wf/tests/flow/nx_cache.rs`): a GET answered 401, 403, 409 or 500, or a request that cannot be sent (connection refused, a dead
proxy), makes Nx skip the task, not run it, and exit 1 (`Misconfigured remote cache endpoint: Unexpected response status` or `Failed to
send request`). So none is ever sent, and the shim must be up for as long as Nx runs. Nx accepts an `http://` loopback server, sends
`GET` then `PUT` with `Authorization: Bearer` and a `Content-Length`, and replays on a 200.

- `GET /v1/cache/<hash>`: read `nx/g<gen>/<hash>`; found, 200 with the bytes; absent or any failure, 404.
- `PUT /v1/cache/<hash>`: if this run's GET just returned 404 for the hash, skip the existence check (a PUT is then 2 requests per task,
  not 3); else HEAD first. Present, keep the first writer and answer 200 (first write wins, the property Nx requires). Absent, write
  with `if-none-match: *` when the probe in B5 shows a PAR accepts it, else plain PUT. Read-only mode, over budget, entry above 64 MiB,
  store error: drop the body and answer 200.
- Nx's own `.nx/cache` stays in front as the local tier (Nx reads the remote only on a local miss). It is disposable, not a durable
  store, and no other cache is kept in a VM volume.

Env given to the Nx child: `NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://127.0.0.1:<port>`, `NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=<random
per run>`, and `NO_PROXY`/`no_proxy` extended with `127.0.0.1,localhost` (the sandbox sets a proxy, which would otherwise swallow the
loopback call). `wf test run --fresh` (`NX_SKIP_NX_CACHE=true`, `clis/wf/src/test_plan.rs:282,350`) never starts the shim.

### Namespaces, one per cache kind

One private bucket, `${name_prefix}-build-cache`. The key is `<kind>/g<generation>/<name>`.

| Kind | Key | Writes | Expires (lifecycle) | Shared sandbox to VM |
| --- | --- | --- | --- | --- |
| `nx` | `nx/g<gen>/<nx hash>` | shim, guarded | 14 days | TypeScript tasks (lint, typecheck, unit, flow of pure packages) yes. Rust-building tasks carry `uname -sm` in the hash, so each arch fills its own entries; browser e2e no longer does (30 Oct: the arch does not matter to them). |
| `pnpm` | `pnpm/g<gen>/store-v11.tar.gz` and `store-v11.json` (lockfile sha256, bytes) | `wf cache save pnpm`, guarded, one object overwritten in place | 60 days | yes (content-addressed; arm64 native packages are fetched on the VM) |
| `meta` | `meta/generation` | `wf cache purge` | none | yes |
| `probe` | `probe/<uuid>` | live tests | 1 day | |

The generation is a number read from `meta/generation` (absent means 0) and cached in `~/.cache/wf/blob-cache/generation.json` for 10
minutes, so it costs about no requests. `wf cache purge <kind>` writes a higher number: every reader and writer moves to a new empty
namespace at once, with no list and no delete. The old generation expires by lifecycle.

### Auth

- Data plane: two pre-authenticated requests on the bucket, minted by Terraform, stored in the vault as `BUILD_CACHE_READ_PAR_URL`
  (`AnyObjectRead`, listing denied) and `BUILD_CACHE_WRITE_PAR_URL` (`AnyObjectWrite`), registered in
  `clis/secrets/src/data/registry.json` first. They follow `VERIFICATION_ARTIFACTS_PAR_URL` (`infra/oracle-cloud/verification-artifacts.tf:44-51`):
  no new IAM user, no key on the VM, resolved by wf from the environment, then Bitwarden. A PAR cannot delete or list.
- Rotation: PARs need an expiry and cannot be edited; 2028-10-01, re-minted with `terraform taint` and apply (the existing practice).
- Why not S3-compatible access with sccache or an Nx S3 package: the only IAM user is the tenancy admin, so a secret key is a
  tenancy-admin credential; the compat layer has no conditional write (`infra/oracle-cloud/versions.tf:40-44`); the Nx packages are deprecated.
- Why not point Nx straight at a PAR: a PAR overwrites, so first-write-wins is lost; Nx's reaction to a PAR error is unknown; there is no
  per-run budget or cold fallback.

### Same store for the sandbox and CI

Both run the same wf and resolve the same two secrets; the sandbox through `BWS_DEPLOY_SECRETS_READ_ACCESS_TOKEN` (`wf secret get` of both
PARs works there today for the sibling PARs), the runner through the same variable in its compose environment. No workflow gains an env
variable. Reads need nothing else. A sandbox that has pushed commit C to `dev` seeds the entries CI replays at C, provided both compute
the same hashes, which needs one Node (open decision 2).

### Who may write, and what is in the key

- Read always. Write only when the guard passes: tree clean (untracked files count), `HEAD` an ancestor of `origin/dev`, `node` and `pnpm`
  equal to the pins. One function, `write_allowed`, replaces both `may_write` and the pre-commit guard. A refusal is one line on stderr
  with its reason, and the run reads only.
- Enforcement is client-side. A PAR carries no identity, so any holder of the write PAR can write; the backstops are first-write-wins, the
  weekly audit (`.github/workflows/nx-cache-audit.yml`, which reruns two stages cached and fresh) and `wf cache purge nx`. The audit covers
  only flow-tests and typecheck-lint-unit-tests, not e2e entries.
- Arch and toolchain are in the Nx hash, not in the blob key, so pure TypeScript entries stay shareable across x86_64 and aarch64 while a
  native entry from one arch is never replayed on the other. For `pnpm` the key carries the store layout (`v11`) and the sidecar the lockfile hash.

### Cleanup and budget guard

1. Lifecycle (unattended, authoritative), in `infra/oracle-cloud/build-cache.tf`: DELETE `nx/` 14 days, `pnpm/` 60 days, `probe/` 1 day;
   ABORT multipart uploads after 1 day. Whole days from last modified, one pass a day, first pass up to 24 hours after apply. OCI has
   no last-access rule, so there is no LRU: an entry hit every day still expires on day 14 and is recomputed once.
2. `wf cache gc`: `--status` (bytes and requests against the budget, JSON), `--evict-to <bytes>` (oldest first by creation time),
   `--prefix <p>` (delete). Uses `BlobAdmin`, so it runs where `oci` and `~/.oci` exist (the sandbox); whether the runner has `oci` is
   unverified, so nothing unattended depends on it.
3. Budget guard: per run the shim stops at 400 requests or 256 MiB and goes cold for the rest of the run. Monthly: `wf cache status`
   reads OCI Monitoring for the dedicated bucket (cache-only request count and `StoredBytes`) and exits 1 above the caps; the weekly
   audit calls it. The kill switch is `"enabled": false` in `budget.json` or `WF_BLOB_CACHE=off`.
4. Every run reports `{hits, misses, puts, skipped, requests, bytes, degraded}` on one stderr line and in `bundle.json` under `cache`.

### When the store is unreachable or over budget

| Case | Behaviour |
| --- | --- |
| a PAR secret does not resolve | shim not started; Nx runs on its local cache, as today |
| connect or response timeout (3 s, 10 s) or a 5xx | breaker opens for the rest of the run: GET answers 404, PUT 200 and drops; `degraded: unreachable` |
| 403 on PUT or GET (expired or revoked PAR) | same, `degraded: credential`, one warning naming the secret |
| run budget reached | same, `degraded: budget` |
| entry above 64 MiB | that PUT dropped, others continue |
| guard refuses the write | read-only, reason on stderr |
| a corrupt entry | Nx falls back to running the task is unverified; the weekly audit and `purge` are the remedy |

### Rust build output cannot be shared between the sandbox and the VM

The sandbox is x86_64 with no aarch64 target and no cross linker; the VM is aarch64. Nx hashes `rustc --version` and `uname -sm` into every
Rust task, and sccache keys carry the host triple and the sysroot digests (sccache docs). A sandbox cannot warm any Rust cache the VM would use.
What this plan does instead:

- The VM's own Rust task results (`cargo-flow` per crate, Rust-building flow tests) are Nx entries written by CI on aarch64 and replayed by
  CI on aarch64. After a recreate an unchanged crate replays without compiling.
- Docker image layers stay on Docker Hub (`:buildcache`, about 8.6 GB, outside OCI; 4.62 GB of headroom could not hold it).
- Not in this plan: sccache (S3 key problem above, and it skips binaries, proc-macros and incremental crates) and a prebuilt `wf`
  binary per triple. After a recreate the first jobs still build `wf` from source (over 30 minutes) and the services' Rust layers (about 15
  minutes). The binary is the first follow-up kind (`wf/<triple>/<clis tree>`, written by CI): it needs a way for the runner to learn the
  read URL before wf exists, which is unverified.

## What gets deleted from the in-VM design

All of `backend/vm-ops-api` that serves the cache, 1,621 lines:

- `rs/src/api/nx_cache.rs` (244), `rs/src/infrastructure/nx_cache_store.rs` (178), `rs/src/infrastructure/disk_usage.rs` (35),
  `core/src/domain/nx_cache.rs` (178), `core/src/application/nx_cache.rs` (243), `core/src/testing/cache.rs` (159),
  `rs/tests/adapters/nx_cache_store.rs` (197), `rs/tests/adapters/disk_usage.rs` (22), `integration/tests/integration/nx_cache.rs` (365),
  and their `mod` lines (`api/mod.rs:27,32`, `infrastructure/mod.rs:11,22,27`, `domain/mod.rs:12`, `application/mod.rs:11`, `testing.rs:20`,
  `adapters.rs:21-22`, `integration main.rs:7`).
- Edits: `core/src/application/ports.rs:303-360` (keep `Chunks`, `:296-301`), `rs/src/main.rs:131,141-161` (`cache_listener`),
  `rs/src/infrastructure/config.rs:78-91,187-205` and its test `:249-264`, `integration/src/lib.rs` (cache port, `raw_cache`, `wire_cache`,
  `connect_cache`, `cache_token`).
- Infra: `infra/oracle-cloud/vm-ops-api/docker-compose.yml:48,80-81,117` (volume `nx-cache`), `infra/oracle-cloud/tests/compose-healthchecks.test.mjs:110-117`,
  `infra/oracle-cloud/README.md:765`.
- wf: `clis/wf/src/vm_cache.rs` (122), the `cache-token` and `cache` arms in `vm_ops.rs:118-119`, `CACHE_PORT` (`vmops.rs:19`), help rows
  `help_table.rs:104-106` and their `help/wf__vm__ops__cache*.txt` files, `docs/reference/wf-cli.md:3633-3714` (generated), flow tests
  `clis/wf/tests/flow/vm.rs:937-1021` and the stub in `check.rs:221-233`.
- CI and docs: `.github/workflows/nx-cache-audit.yml:100` (becomes `wf cache purge nx`), `docs/runbooks/vm-ops-api-rust-cutover.md:3,112-127,142,150`,
  `docs/plans/vm-ops-api-rs/plan.md` (cache sections), `docs/plans/nx-cache/plan.md:101-116`, `docs/plans/sandbox-first-ci/spec.md:51-63`.
- Terraform: `infra/oracle-cloud/pnpm-cache.tf` (bucket, PAR, the 7-day `nx-cache/` rule), after its IAM policy moves to `build-cache.tf`.
- Registry: `NX_REMOTE_CACHE_PAR_URL` and `PNPM_CACHE_PAR_URL` stay, marked orphaned (vault secrets are never destroyed).

Kept: `libc`, `hmac`, `sha2`, `hex` (used by siblings and `auth.rs`).

## Units

| Id | Files | Done when |
| --- | --- | --- |
| B1 blob core | `clis/blob-cache/**` (domain, ports, `adapters/par.rs`, `adapters/dir.rs`), `clis/Cargo.toml` | `cargo test --manifest-path clis/Cargo.toml -p blob-cache` passes: one contract suite runs against `DirStore` and against a loopback fake of the PAR URL form (404 gives `None`, a second `put(if_absent)` gives `Exists`, unreachable gives `Err`, generation bump moves the namespace, breaker opens after a failure, budget trips). A grep proves `domain/` has no `std::fs`, `std::net`, `Command` or `cli_http`. |
| B2 nx shim | `clis/blob-cache/src/nx_server.rs`, `clis/wf/tests/flow/nx_cache.rs`, `docs/goals/2026-10-20-after-device-lab.md:146` | `cargo test ... -p wf --test flow -- nx_cache::a_sandbox_pass_replays_on_ci` prints `test result: ok. 1 passed`. The test runs real Nx 23.2.1 against the shim over `DirStore`: site A executes N tasks and the store holds N entries; site B (own checkout, HOME, PATH, empty `.nx/cache`) at the same commit replays all N with an empty ran-log; the same run with `HTTP_PROXY` set still works; a store that errors still ends green and cold; read-only mode makes zero PUTs. The goal check is changed to require `ok. [1-9][0-9]* passed`, since a filter that matches nothing prints `ok. 0 passed`. |
| B3 wf wiring | `clis/wf/src/nx.rs`, `verify.rs`, `test_plan.rs`, new `clis/wf/src/cache.rs`, `clis/wf/src/help/wf__cache*.txt`, `clis/wf/src/help_table.rs`, `docs/reference/wf-cli.md` | Flow tests: `wf test run --stage` and `wf check --staged` start the shim; in a dirty tree or off `origin/dev` they read only, with the reason on stderr; `--fresh` starts none; `shared_cache_configured` and `SHARED_CACHE_ENV` are gone; one `write_allowed`. `wf cache --help` documents the JSON schema; `pnpm run sync:check` and `wf check --budget` pass. |
| B4 admin verbs | `clis/blob-cache/src/adapters/oci_cli.rs`, `clis/blob-cache/data/budget.json`, `cache.rs` | Flow tests over a stub `oci` on `PATH`: `wf cache status --json` reports bytes and requests against the caps and exits 1 over either; `gc --evict-to` deletes oldest first until under; `purge nx` writes `meta/generation` and the next shim reads the new namespace; no verb prints a PAR. |
| B5 infra and secrets | `infra/oracle-cloud/build-cache.tf`, `pnpm-cache.tf` (policy moved), `clis/secrets/src/data/registry.json`, `clis/blob-cache/tests/live.rs` | `wf vm terraform plan` shows additions only; after apply `oci os bucket get` says NoPublicAccess, the lifecycle policy lists the four rules, and `wf secret get BUILD_CACHE_READ_PAR_URL` and `..._WRITE_...` exit 0 in the sandbox and on the runner; `wf secret doctor` shows no drift. A test reads `build-cache.tf` and fails when its lifecycle days differ from `Kind::ttl_days`. The ignored live test (`BLOB_CACHE_LIVE=1`, objects under `probe/`) records in this file whether a PAR accepts HEAD, `if-none-match: *`, a bucket-wide URL ending `/o/`, and one 0.6 GB PUT through the sandbox proxy. |
| B6 CI and Node pin | `.github/workflows/deploy.yml`, `e2e-tests.yml`, `nx-cache-audit.yml`, `mise.toml`, `clis/wf/src/test_publish.rs` | `bundle.json` carries `cache` (flow test on `wf test publish`); `wf test drift --pins` is `ok:true` in the sandbox and in a CI step; `actionlint` passes; two pushes with identical task inputs: the second run bundle shows `puts: 0` and `hits` equal to the tasks not replayed locally. |
| B7 remove in-VM | the "What gets deleted" list | `pnpm run arch:services` and the vm-ops-api crate tests pass; `node --test infra/oracle-cloud/tests/compose-healthchecks.test.mjs` passes without the volume assertions; `grep -rn "NX_CACHE_\|nx-cache\|CACHE_PORT" backend infra clis .github` finds only the audit step's generation text and this plan; `pnpm run sync:check` passes. May land before B1 to B6, because nothing uses the store. |
| B8 pnpm kind | `clis/blob-cache` (kind `pnpm`), `clis/http/src/lib.rs` (`put_file`, streaming), `clis/wf/src/cache.rs`, the install steps of `deploy.yml` and `e2e-tests.yml` | Flow tests over `DirStore`: `wf cache restore pnpm` restores only when the local store is empty; `save pnpm` runs only after a green `--frozen-lockfile` install, only when the lockfile sha differs from the sidecar (or the object is missing), and never when the guard refuses; the pnpm object stays one object. `put_file` streams (a test uploads a file larger than the 512 MiB read cap without holding it in memory). |

Order: B1, B2, B3, B5 (needs the owner's go on cost), B4, B6, B8; B7 any time.

## Open decisions for the owner

1. Accept the request overage? The cache adds an estimated 20 to 55 thousand requests a month to 82,000 already billed against 50,000 free
   (about 0.02 a month at the inferred price, no blocking). Alternative: first shrink the bundle uploader. Recommendation: accept, set the
   60,000 cap, and file the uploader as a separate item (one archive per run would shrink it, but public per-file recording URLs depend on it).
2. Which one Node? `node --version` is part of every task hash and of the write guard, and the two real environments disagree with
   `mise.toml`. Recommendation: pin `mise.toml` and every `setup-node` to 22.22.0, what the sandbox and CI run today, and keep pnpm 11.5.2.
   Moving both to 24.13.1 means installing Node in the sandbox, where the GitHub API behind mise answers 403.
3. Who may write? Recommendation: the sandbox and CI both write through the one write PAR, behind the same client-side guard (this is
   what "the sandbox prepopulates" means). The cost is that the guard is voluntary and a flaky green stays until the weekly audit or a
   purge. The alternative, CI as the only writer, makes the sandbox a read-only consumer and drops the prepopulate half of the request.

## Risks

- Nx's native client against a loopback server was run in the sandbox environment (B2, 23 Oct): `http://` is accepted without a warning and
  `NO_PROXY` is honoured. An unreachable shim is not a cold run but a red one with the tasks skipped, so wf must start the shim before Nx
  and keep it up until Nx exits (B3).
- A first write is permanent for 14 days and the guard is client-side. A wrong green from a clean `dev` commit is replayed until the audit
  (weekly, two stages only) or `purge nx`. Failures are not cached.
- Cross-environment hits are zero until decision 2 is applied: the hash includes `node --version`.
- The tenancy-wide 15 GB quota is shared with tfstate. A runaway cache could make a state write fail, which is the VM recovery path.
  Mitigation: the 2 GB cap, the per-run byte cap, 14 and 60 day expiry; the guard cannot stop a writer that bypasses wf.
- Lifecycle is daily and age-based; a short TTL overshoots between sweeps and the first pass lags up to 24 hours.
- Metrics lag by about a day (82,641 vs 82,036), so the monthly guard is approximate.
- A 0.6 GB PAR PUT worked from the VM (the object exists); from the sandbox through the egress proxy it is unverified. If it fails, only CI
  saves the pnpm store.
- Sandbox latency: about 0.6 to 1.0 s of TLS setup per connection through the proxy, about 1 s to first byte. A cold sandbox replaying 91
  tasks needs a keep-alive connection pool and parallel GETs in the shim, or it adds minutes.
- After a VM recreate the first jobs are still slow (`wf` build, Rust layers). This plan removes the Nx and pnpm part of that cost only.
- The PARs expire 2028-10-01; nothing alerts before then.

## Not verified

- Nx 23.2.1's reaction to a non-200/404 status on a PUT, and to a 200 with a corrupt body (a GET's other statuses are measured above).
- Whether a PAR accepts HEAD, `if-none-match: *`, and a bucket-wide URL ending `/o/`; whether a retention-free PUT over an existing key returns 200.
- Whether this tenancy gets 10 or 20 GB of free storage; whether HEAD, 4xx and lifecycle deletes count as billed requests; the overage price
  (inferred from the usage meter, Oracle's price pages returned 403).
- The request and byte estimates for the cache (no Nx entry has ever been written; sizes are inferred from declared outputs).
- That the runner has `oci`, and its actual node, pnpm and rustc versions (nothing was sent to the VM).
- OCI Monitoring dimensions for a per-bucket request count (the metric names are verified, the exact dimension filter is not).
- Whether `pnpm install` order in the CI jobs lets `wf cache restore pnpm` run before the install on a fresh VM.

## Docker layers via Docker Hub

Status: audited and partly applied, 23 Oct 2026 (unit B9); the seven Rust Dockerfiles are rewritten for a dependency layer (unit B9b), unbuilt:
see the last two subsections for what was proved without a docker daemon. Owner request: cache Docker layers through the Docker Hub free tier, with no
on-VM cache, because an on-VM cache cannot be shared and is lost on a recreate. Layers stay out of the OCI bucket (the `:buildcache`
repositories hold 9.7 GB, the bucket's headroom is 4.62 GB). The ref is `docker.io/nuesslerm/<image>:buildcache`, written with `mode=max`,
the ref and attributes `clis/wf/src/build_image.rs` already uses.

### Docker Hub free tier, as checked

| Fact | Value | Source |
| --- | --- | --- |
| Repositories | unlimited public, up to 1 private | docs.docker.com/docker-hub/usage/, fetched 23 Oct 2026 |
| Pull limit | 200 per 6 hours when logged in as a Personal user, 100 per 6 hours unauthenticated per IPv4 or IPv6 /64 | docs.docker.com/docker-hub/usage/pulls/, same day |
| What counts as a pull | one manifest fetch (one per architecture of a multi-arch image); version checks (HEAD) do not count; blob downloads are not counted | same page |
| Storage cap | none stated; "excessive data transfer, pull rates, or data storage can lead to throttling, or additional charges" (fair use) | usage page |
| Are the cache repos public | yes: all 13 `nuesslerm/<image>` repositories answer 200 to an unauthenticated `GET hub.docker.com/v2/repositories/...` | measured |
| Size of each `:buildcache` tag, as Hub reports it | vnc-auth-capture 2.63 GB, auth-cookie-capture 1.40, agent-shell 1.33, remote-chat-agent 1.31, leetcode-backend 0.62, vm-ops-api-rs 0.60, reporting-service 0.57, watchdog 0.41, stt 0.37, opencode-server 0.30; sum 9.54 GB, 9.71 with the retired Node `vm-ops-api` (0.17); `logger-cli` and `agent-base` have no `:buildcache` tag | measured, Hub API |
| Legacy refs `<image>-cache` (older scripts) | agent-shell-cache 1.71 GB, remote-chat-agent-cache 2.70, vm-ops-api-cache 0.16 (4.56 GB), last pushed 8 to 19 Sep, read by nothing now | measured |

Two consequences. A public cache is public: `mode=max` publishes every builder stage, so the `COPY`ed Rust sources and
`clis/secrets/src/data/registry.json` (key names, no values) of a private GitHub repository are readable (open question 9 of
`docs/plans/vm-ops-api-rs/plan.md`, accepted there). The free private repository would hold one cache repo with a tag per image
(`nuesslerm/buildcache:<image>`), at the price of a login on every reader and a ref that is no longer `<repo>:buildcache`; not done.
Pull exposure (an estimate): a build fetches one manifest per base image and one for its cache, about 6 to 10 per image; `wf ship --changed` ships only the
changed services, so a push stays far under 200 per 6 hours on the VM (logged in through `DOCKERHUB_TOKEN`). A sandbox build
that does not log in shares 100 per 6 hours with whatever else leaves through the same egress address.

### Audit of every image build path

| Path | Runs where | `--cache-from` | `--cache-to` | Verdict |
| --- | --- | --- | --- | --- |
| `clis/wf/src/build_image.rs`, `Mode::Push` (`wf ship <app>`, `--build-only`) | VM, agent-shell docker-in-docker, arm64 | registry `<repo>:buildcache` | none, on purpose: the upload is minutes nothing waits for | reads the registry |
| same file, `Mode::ExportCache`, started by `deploy_run.rs:start_cache_export` after the redeploy | same | same | registry, `mode=max,oci-mediatypes=true,compression=zstd,ignore-error=true`, `--output type=cacheonly` | writes the registry, but only after `wf ship <app>`; `--build-only` never starts it (and `--no-wait` cannot) |
| `backend/vm-ops-api/core/src/application/scripts/self_build.rs` (snapshot `self-build.sh`, golden `self-build-dev-ns.sh`) | same, the vm-ops-api and watchdog images | `type=registry,ref=docker.io/<ns>/<name>:buildcache` | same ref, `mode=max,...,ignore-error=true`, inline; a failed build retries once without the flags | reads and writes the registry; no change |
| swap scripts (`self-redeploy.sh`, `app-redeploy.sh` snapshots) | VM | none, they build nothing: `docker compose pull`, `up -d` | | not a build path |
| `scripts/build-and-push-image.sh` (sandbox, by hand) | sandbox, arm64 under emulation | `<image>-cache` | `<image>-cache`, `mode=max` | used a second, stale repo; fixed below |
| `infra/oracle-cloud/agent-shell/build-and-push.sh` (sandbox, by hand) | sandbox | `<image>-cache` | `<image>-cache`, `mode=min` (final-image layers only, so every builder stage re-ran) | fixed below |
| `infra/oracle-cloud/scripts/deploy-budget-guard-function.sh:65` | an operator's machine | none | none | plain `docker build`, pushes to OCI's registry, not a VM build; not changed |
| `.github/workflows/*` | the runner | none | none | no workflow builds an image; `wf ship` builds on the VM |

Applied (flags in two scripts, no renderer change, so no golden or flow fixture moves): both sandbox scripts now use
`CACHE_REF="${IMAGE}:buildcache"` and `--cache-to "type=registry,ref=${CACHE_REF},mode=max,oci-mediatypes=true,compression=zstd,ignore-error=true"`,
the ref and attributes of the VM builds. A sandbox build and a VM build now warm each other, and a rejected cache write no longer fails a push.
`bash -n` passes on both; no test reads these two scripts' flags.

Not applied, with the reason:

- `--build-only` writes nothing. It is used by hand to ship a branch or to warm a builder, and the next `wf ship <app>` exports. A fix is
  `start_cache_export` after a waited `--build-only` (mirrors `build_and_redeploy`), but it is a change in `deploy_run.rs` and needs a new
  script fixture and `ship.rs` expectations, so it is a proposal: unit B9a, three lines plus a flow test that a waited `--build-only`
  starts a second job whose script is `Mode::ExportCache`.
- `logger-cli` and `agent-base` have no registry cache. `logger-cli` is a one-crate musl build (`telemetry` only) and cheap; `agent-base` is
  built by hand through the sandbox script, which now writes `nuesslerm/agent-base:buildcache`. Nothing else to do.
- The three legacy `<image>-cache` repositories (4.56 GB) are read by nothing. Deleting them needs the Docker Hub owner's session; not done.
- `docs/solutions/tooling-decisions/backend-docker-build-pnpm-fetch-cache-optimization.md` (lines 226-236) still quotes the old
  `${IMAGE}-cache` lines as "current"; update it when the two scripts land.

### What still lives in the VM's BuildKit cache

A registry cache restores a layer only when the instruction and everything before it are unchanged. A `RUN --mount=type=cache` mount is
not a layer: its contents stay in the builder, are never exported, and are gone on a recreate (and on any `buildx prune` that goes past
`--max-used-space 60GB`, `build_image.rs:209` and `self_build.rs:54`). So after a recreate every layer whose inputs did not change comes
back from Docker Hub, and every `RUN` whose inputs changed runs against an empty mount.

Measured and documented costs of that, none measured again here (nothing was sent to the VM): a Rust image rebuilt with its target mount
emptied took 15 to 40 minutes under load (remote-chat-agent, 14 Oct, `docs/audits/2026-09-02-token-diet-decision-log.md:2367`), a cold compile
of both images with a device up took 52 minutes (16 Oct, `build_image.rs:155`), the same build warm is one leg of a median 274 s (15 green
vm-ops-api legs, `docs/plans/vm-ops-api-rs/plan.md:456`), and this plan estimates 15 minutes for the services' Rust layers after a recreate.

| Dockerfile | Cache mounts | Holds | Cost when empty | Proposal |
| --- | --- | --- | --- | --- |
| `backend/leetcode-backend`, `leetcode-auth-cookie-capture`, `reporting-service`, `vm-ops-api`, `infra/oracle-cloud/watchdog` (alpine, `target-musl`), the `rcabuilder` stage of `backend/remote-chat-agent` | cargo registry, `<name>-target` | the whole `deploy`-profile target dir and the `.sources` manifest of `cargo-touch-changed.sh` (incremental state) | the full compile of 380 external crates (`backend/Cargo.lock` lists 395 packages, 15 of them the workspace's) plus the workspace crates: the 15 to 40 minutes above, on every change to any workspace member, because each image copies every member | a dependencies-first layer, applied (B9b), see below |
| `clis/vnc-auth-capture` (`listener` stage) | cargo registry, `vnc-auth-capture-target` | its own 252-package target dir | a full compile of a smaller graph, estimated 5 to 10 minutes; the image is 2.63 GB of cache, mostly the Chromium desktop base, which the registry restores | the same layering, applied (B9b) |
| `infra/oracle-cloud/logger-cli` | cargo registry | one small crate's registry downloads | seconds to a minute (estimate) | none |
| `backend/remote-chat-agent` runtime, `opencode-server`, `infra/docker/agent-base` | apt lists and `/var/cache/apt` | downloaded `.deb` files | the layer itself is a registry hit while its inputs hold; when it re-runs, apt re-downloads tens to a couple of hundred MB, about a minute (estimate) | none: leave the mounts, they are a speed-up only |
| `opencode-server`, `infra/oracle-cloud/agent-shell` (two steps) | `/root/.npm` | npm tarballs | the same: a registry hit while inputs hold, a re-download of the global packages when they change, 1 to 3 minutes (estimate) | none |
| `backend/remote-chat-agent/stt`, `functions/budget-guard` | none | | | none |

Applied for the Rust images (unit B9b, 23 Oct 2026, unbuilt): a dependencies-first layer, so the expensive part is an image layer that
`mode=max` exports and a recreated VM imports. The same three stages sit in `backend/leetcode-backend`, `leetcode-auth-cookie-capture`,
`reporting-service`, `vm-ops-api`, `infra/oracle-cloud/watchdog` (alpine; `CARGO_TARGET_DIR=/src/backend/target-musl` is set in the builder
stage only, because `cargo install` would otherwise build cargo-chef inside it), the `rcabuilder` stage of `backend/remote-chat-agent` and, with
its own workspace root, the `listener` stage of `clis/vnc-auth-capture`:

1. `chef`: the base image and `cargo install cargo-chef --locked --version 0.1.78` (registry mount, so the layer holds the binary only).
2. `planner`: the `COPY` lines the builder had (every path the leg planner watches is still copied, so `deploy.rs` and its tests only lose the
   helper script), then `cargo chef prepare --bin <crate>`. The recipe is the manifests of the crates in that crate's dependency closure and a
   lockfile cut to them, local versions masked. cargo-chef lists workspace members only, so the path crates outside the workspace
   (`clis/telemetry`; `clis/rca-core` for the agent; `clis/cdp`, `clis/http`, `clis/telemetry` for the listener) get a `/skeleton`: their
   manifests and an empty file per `.rs`. The layer then never reads their code (`clis/cdp` changed 61 times since 15 Sep, `rca-core` 20, telemetry 2).
3. `builder`: `COPY` the recipe and the skeleton, `cargo chef cook --profile deploy -p <crate>` (registry mount, no target mount, no incremental
   flag, the target dir the final build uses), `COPY --from=planner /src /src`, then one `RUN` that touches every `.rs` outside the target dir,
   runs `cargo build --profile deploy --locked -p <crate>` and copies the binary. The runtime stages did not change.

What is not obvious, and why:

- `--bin` on `prepare`: an unfiltered recipe carries all 13 members and the whole lockfile, so a dependency added to one service would
  re-cook all six images. Filtered, a sibling's manifest edit left the other recipes byte-identical (checked below).
- No `--locked` on `cook` (the final build keeps it, against the real lockfile): the cut lockfile still lists dependency edges the full
  workspace switched on (`digest` to `subtle`, `telemetry` to `futures-executor`) that the cut closure does not, so `cargo metadata --locked`
  refuses 5 of the 6 skeletons. Cargo prunes the edges; package versions stay those of the lock. The listener's recipe is unfiltered and resolves
  with `--locked`; it builds without it, as it did before.
- The `touch`: cargo-chef deletes the compiled dummy libraries of workspace members only, and a `COPY`ed source keeps the checkout's old mtime,
  so a stubbed path crate stays a stub. Reproduced in a toy workspace: `cannot find function` without the touch, the right binary with it.
- The registry mount stays, a speed-up like apt and npm. It is not in the layer, so on a recreated VM the final build downloads the crates of its
  unit graph again (estimated under a minute).
- `infra/oracle-cloud/build/cargo-touch-changed.sh` is deleted: it normalised mtimes against a manifest kept in the target mount and nothing else
  used it. Its entries in the `vm-ops-api` and `vnc-auth-session` legs (`deploy.rs`), the two flow-test expectations (`ship.rs`,
  `deploy_legs.rs`) and the `.sources` output dir of `dead_refs.rs` went with it.

Cost and trade-off, unmeasured:

- A change to a dependency now recompiles every dependency of that image from nothing, because no target mount is left to start from: the 15 to 40
  minutes above, where a warm mount rebuilt only the changed crates. `backend/Cargo.lock` changed in 28 commits since 15 Sep; the filtered recipe
  confines each to the images whose closure it touches. A source change costs the workspace crates alone, as before. After a recreate the first
  build of each image hits the layer instead of compiling, which is the point. Not done: a target mount inside the cook `RUN`, copied into the
  layer, would keep the warm-VM speed, but it is the on-VM cache the owner dropped.
- cargo-chef is compiled once per base-image change (the floating `rust:1-*` tag; a new Rust release invalidates every cook layer anyway).
- One cook layer per image in its `:buildcache`, estimated 0.3 to 0.5 GB compressed each (`backend/target/release` is 441 MB in this checkout for a
  partial release build), about 2 to 3 GB more on Docker Hub across seven images; public, like the rest of the cache; no storage cap is stated,
  only fair use.

Proved here, without a docker daemon (`docker info` cannot connect, and the sandbox's guard sends image builds to the VM):

- cargo-chef 0.1.78, read from its source (`recipe.rs`, `skeleton/mod.rs`, `read.rs`, `version_masking.rs`) and installed with
  `cargo install --locked --version 0.1.78`.
- Recipes on the real workspace: for all six crates `cargo chef prepare --bin <crate>` is byte-identical after a source-only edit (a line
  appended to every `.rs` of the workspace, `telemetry` and `rca-core`, and a new module file in `service-kit`). After `watchdog/rs` gained a
  dependency the recipes of `reporting-service-rs` and `vm-ops-api-rs` were identical and `watchdog-rs`'s changed. A local version bump is masked.
- Skeletons: for the six crates and the listener, `cargo tree -e normal,build --target all -f '{p} [{f}]'` of the full workspace and of the tree
  restored from the recipe plus the path-crate skeleton differ in 0 of 329, 320, 315, 340, 307, 427 and 309 lines. Same versions and features
  mean the same dependency units, so the final build reuses what the cook layer compiled.
- The cook-then-build sequence on a toy workspace of the same layout (a `deploy` profile, a path crate outside it, a stubbed one, real sources laid
  over with 2020 mtimes): no registry crate recompiled in the final step, the real binary only with the touch.
- hadolint 2.12.0 parses all seven Dockerfiles and reports nothing new. The wf flow tests `ship::the_ship_plan_deploys_what_changed_since_each_service_s_running_commit`
  (the desktop leg's paths equal its Dockerfile's `COPY` sources), `deploy_legs::a_push_ships_exactly_the_legs_built_from_what_it_changed` and
  `check::dead_refs_names_each_path_link_package_command_verb_method_and_script_that_is_gone_and_passes_a_clean_tree` pass.

Not proved: any image build. The VM build in CI is the proof, and the repo rule's benchmark is still owed (one image, `reporting-service`, an
empty builder and a warm `:buildcache`, before and after, n=1 then n=3; the numbers go to the decision log).

### Not verified here

- Any build time above: the numbers are the repository's own earlier measurements and this plan's estimates; no build ran, and nothing was
  sent to the VM.
- Whether Docker Hub counts a `--cache-from` manifest fetch against the pull limit (the docs define a pull as a manifest fetch and exempt
  version checks); whether the sandbox's egress address is shared.
- Whether Docker Hub deduplicates a layer pushed to two repositories of one namespace for storage accounting.
- Whether `ignore-error=true` and zstd are accepted by the BuildKit in the sandbox's builder (the VM's builder runs them today).
- B9b: that BuildKit keys `COPY --from=planner /recipe.json` and the skeleton by content, so a source-only change leaves the cook layer a hit
  (BuildKit's documented behaviour and the basis of cargo-chef's own Dockerfile recipe; not observed here); the size of a cook layer; the planner's
  cost on a cold registry mount (`cargo metadata` downloads every crate of the lock, 395, not only the closure; estimated about a minute); the
  first real cook, a full compile of one image, on the VM.
