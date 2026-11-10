# Nx task cache for the whole pipeline (TypeScript and Rust)

Owner request, 18 Oct: minimise a whole CI run by skipping every part whose inputs did not change, with an Nx-style
content cache for lint, typecheck, tests and deploys, covering the TypeScript and the Rust code, without paying for the
Nx SaaS.

## Status (18 Oct)

Built, with no fallback (owner): N0, N4, the local half of N1 (every package, crate and e2e stage runs through the Nx
cache), and F as Nx projects: leetcode and tamil-writing are nested projects inside the app (`project.json` in each), so
the e2e tests are keyed by them without moving the code; moving them into `features/<name>` packages stays open. The
contract suite is `clis/wf/tests/flow/nx_cache.rs`. N2 (the remote cache) is blob-backed and built (`docs/plans/blob-cache/plan.md`). N3 (deploys by hash) is built, unverified against the registry and the VM. Open: the first CI runs.

## Where the time goes today

The CI run at f0e3d213c took 34.5 min, one job after another on the VM's single runner:

| Job | Minutes | What decides what runs |
| --- | --- | --- |
| plan | 0.3 | `wf test plan --changed --base last-green` |
| typecheck-lint-unit-tests | 3.5 | pnpm's dependency graph from the changed files |
| flow-tests | 5.0 | pnpm graph; cargo path dependencies for crates no pnpm package wraps |
| clis (the wf build) | 1.3 | always |
| deploy legs | 2.2 | `wf ship --changed`: content hash of the images since the running image's commit (N3; before it the package graph, with `ctx-<tree>` reuse) |
| deployment-verification-tests | 0.5 | pnpm graph |
| browser and Android e2e | 3.8 + 16 | every focused test (owner, 18 Oct) |

Selection by changed files is coarse, and every focused e2e test runs on every push whatever changed. A one-line change to `frontend/libraries/tamagui-design-system` re-runs typecheck, lint
and flow for every package that depends on it, even when their own inputs hash the same as last time. Nothing is
remembered between runs except cargo's `target/` and the pnpm store.

## Decision

Nx (MIT core) with:

- **The local cache** (`.nx/cache`) on the runner. The runner's checkout persists between runs (`clean: false`), so a
  task whose inputs hash the same as in any earlier run replays its result in seconds.
- **A self-hosted remote cache**, so the cache survives a VM recreate and dev sandboxes can read it. Nx 20.8+ talks to
  any server that implements its cache OpenAPI spec (`GET`/`PUT /v1/cache/{hash}`, tar bodies, bearer token). `wf` serves
  those two endpoints on loopback for the length of a run and stores to a private OCI bucket through two pre-authenticated
  links from the vault (`docs/plans/blob-cache/plan.md`); nothing is served from the VM. No Nx Cloud and no `@nx/s3-cache`,
  whose licence the Nx docs describe inconsistently.
- **Rust crates as Nx projects.** Phase 0 picks between the `@monodon/rust` plugin (infers a project per Cargo
  member, with build, test and clippy targets) and a hand-written `project.json` per crate group. Either way a crate's
  inputs are its `src/**`, its `Cargo.toml`, the workspace `Cargo.lock` and the toolchain (`rustc --version` as a runtime
  input; the repo pins no `rust-toolchain.toml`), and
  cargo's own incremental build in the runner's persistent `target/` still applies when a crate did change.

Considered: Turborepo (TypeScript first; Rust only through package.json wrappers), moonrepo (first-class Rust, remote
cache over the Bazel remote API; a second new tool where Nx covers both), Bazel or Buck2 (rewrites every build). Nx is
the smallest step from pnpm scripts that covers both languages in one graph.

## What stays

- `wf` stays the entry point (`wf test run --stage`, `wf check --staged`, `wf ship`). It calls Nx instead of building
  pnpm filters itself.
- The `[ci: <stages>]` tag, the time limits and every gate. A cache hit replays a result that the same inputs produced;
  it never turns a red task green.
- Focus still decides which e2e tests exist in CI. Phase N4 changes when a focused test runs: only when its inputs
  changed (owner, 18 Oct).

## Phases

Each phase lands on dev through CI, has its own done check, and is benchmarked against the table above (the CLAUDE.md
benchmark rule). Numbers and the decision go to `docs/audits/2026-09-02-token-diet-decision-log.md`.

Order: N0, then F and N4 (the e2e jobs are where the minutes are), then N1, N2 and N3.

### N0. Spike: one TypeScript package and one Rust crate

1. Add `nx` (pinned) as a root dev dependency, vetted with `pnpm run audit`. In `nx.json`: `neverConnectToCloud: true`,
   no daemon in CI (`NX_DAEMON=false`), `maxCacheSize` set (the VM disk is shared).
2. Infer projects from the pnpm workspace (package-based). `targetDefaults` for `lint`, `typecheck`, `test:unit` and
   `test:flow`: `cache: true`, inputs from `namedInputs` (`default` = the project's files, `production` without tests,
   plus `sharedGlobals`: root `pnpm-lock.yaml`, the root `tsconfig*.json` and ESLint config, the Node version). Outputs: none for
   checks, coverage and report dirs for tests.
3. One Rust crate (`rca-service`) through `@monodon/rust` and through a `project.json`, to compare inference cost,
   hashing time and correctness.
4. Verify, against the docs at the pinned version: whether failed tasks are cached (the plan relies on successes only),
   the remote cache env var names, and how `nx run-many` reports a cache hit.

5. One e2e test as a project (`tamil-writing`, `e2e-browser` only), to prove the inferred inputs and that a cached pass
   prints its bundle URL.

Done when: the second run of `nx run-many -t lint typecheck test:unit -p <both>` replays from cache, and an edit to one
input file re-runs exactly that project and its dependents.

### N1. Every project, local cache, in CI

1. All 39 pnpm packages and the Rust workspaces (`backend/` with 39 members, `clis/` with 4, the vnc listener) as Nx
   projects with explicit inputs. Each Rust-backed pnpm package (e.g. `@app/remote-chat-agent`, whose `test:flow` runs
   cargo) gets the crate sources in its inputs.
2. `wf test run --stage flow-tests` and `--stage typecheck-lint-unit-tests` run `nx run-many -t <targets>` over every
   project. The cache makes unchanged projects instant, so the plan stops computing which packages a change reaches.
   The plan keeps listing what ran and why: a cache hit or the input that changed.
3. `wf check --staged` (the lefthook gate) uses the same targets with the local cache.

Done when: a docs-only push runs typecheck, lint, unit and flow in under 1 min, all from cache. A one-package change
re-runs only that package and its dependents. Benchmark: three runs each of a no-op push, a one-package TypeScript
change and a one-crate Rust change, before and after.

### N2. Remote cache that survives a VM recreate

1. vm-ops-api: `GET` and `PUT /v1/cache/{hash}` per the Nx OpenAPI spec, with a bearer token from the vault (new
   registry entry), storing objects under `nx-cache/` in the OCI bucket through a PAR URL. Flow tested with a fake
   bucket, then against the real one.
   Superseded twice. 23 Oct 2026, first: the cache lived in the Rust vm-ops-api on port 8082, its blobs on a VM volume
   (`docs/plans/vm-ops-api-rs/plan.md`, unit N1). Owner, 23 Oct, later, binding: no on-VM cache, because it cannot be shared
   and a recreate loses it. That listener, its volume, its tokens and `wf vm ops cache-token` are deleted. The cache is
   blob-backed: a generic `BlobStore` port over an OCI Object Storage bucket, with Nx as one kind, shared by the sandbox
   and CI (`docs/plans/blob-cache/plan.md`). The weekly audit (`.github/workflows/nx-cache-audit.yml`) reruns the package
   stages fresh and, when a cached pass fails fresh, moves everyone to a new empty generation with `wf cache purge nx`.
2. CI and the sandboxes point Nx at it. Reads everywhere; writes only from a clean tree on `dev`, so a dirty local tree never
   seeds the shared cache (the blob-cache plan's one write guard).
3. Pruning: a bucket lifecycle rule expires Nx entries after 14 days (blob-cache plan, "Cleanup and budget guard").

Done when: after a VM recreate, the first CI run replays unchanged projects from the bucket (blob-cache plan, unit B6).

### N3. Deploys keyed by the same hash

1. One `image` target per service (inputs: the Docker build context the Dockerfile copies, the Dockerfile, the base
   image tag). Its hash replaces the `ctx-<tree>` tags. An image tagged `nx-<hash>` that already exists in the registry
   is never rebuilt.
2. `wf ship --changed` compares the running image's hash label with the current one, not the package graph since a
   commit. Metro keeps its own check: alive, and at a commit whose app inputs hash the same.

Done when: a push that changes no service's inputs builds and deploys nothing, and one service change rebuilds exactly
that image.

Status (24 Oct 2026): built in `clis/pipeline/src/image_hash.rs` and `clis/deploy`; owner rules, decisions recorded here.

- The hash (24 hex of a git blob over: the Dockerfile, every file its `COPY` lines name in the context, and for a Rust image
  the `Cargo.lock` entries its crates reach) is read from git at `--ref`. The base image tag is a `FROM`/`ARG` line of the
  Dockerfile, so it is in. A workspace crate the image copies but does not build (the closure of `copy_closure::path_closure_by`
  leaves it out) is not an input: a change to it rebuilds nothing else. No separate Nx `image` target: the hash is computed by
  `wf` directly, because Nx would only add a cache around a git read.
- Every build tags `nx-<hash>` next to `sha-<short>` and labels the image `wf.input-hash`; the build script first runs
  `imagetools create -t :sha-<short> :nx-<hash>`, and builds only when that fails. The `ctx-<tree>` tags are gone.
- `--changed` hashes each service's images (and its compose project: the `docker-compose*` files beside its Dockerfile, for
  vm-ops-api the two compose directories) at the running commit and at HEAD and ships the service when they differ. The
  running commit is read from the `sha-<short>` tag as before, not from the label: vm-ops-api's `ps` lists the image name only.
  Equal by construction, because a re-tagged image keeps its hash.
- Metro: no longer a service (retired), so nothing here.
- Not done: vm-ops-api's self-build (`POST /redeploy?ref=`) still builds its images without the reuse check; a reused image
  keeps the `GIT_COMMIT` it was first built with, so `--wait-serving` on a service whose inputs return to an earlier hash
  (a revert) would wait for a commit it never serves.


### F. The app's features become packages (owner, 18 Oct)

The e2e tests can only be keyed by feature if the features are projects of their own. Today two of them live inside
the app: `frontend/mla-tamagui/features/leetcode` (83 files) and `frontend/mla-tamagui/features/tamil-writing` (47). The
others are already packages under `features/` (`@feature/coding-agent`, `@feature/settings`, `@feature/service-report`,
`@feature/dev-tools`).

1. `frontend/mla-tamagui/features/leetcode` becomes `@feature/leetcode` and `tamil-writing` becomes
   `@feature/tamil-writing`, each under `features/<name>` with one barrel and its test doubles in `src/testing/` (the
   CLAUDE.md layout). The routes in `frontend/mla-tamagui/app/` stay thin and import the barrels. The existing
   `features/leetcode` (`@lib/leetcode-contracts`) moves to `libraries/leetcode-contracts`, so the name is free.
2. The empty `features/*-impl` directories go.
3. `pnpm --filter mla-tamagui run arch`, typecheck, the flow tests and a local web export pass unchanged; the browser and
   Android e2e tests pass on the dev Metro after the move.

Done when: no feature code is left under `frontend/mla-tamagui/features/`, and each e2e test's feature is one package.

### N4. An e2e test runs only when its feature changed (owner, 18 Oct)

The e2e jobs are the bulk of a CI run (about 20 min). A focused test runs only when one of its inputs changed since its
last pass; otherwise CI replays that pass and links its published bundle.

1. Each e2e script names what it drives in its header: `features: leetcode, coding-agent`. A small Nx plugin
   (`createNodes` over `e2e/*.lua`) makes every test a project with `e2e-browser` and `e2e-android` targets (the
   platforms in its header), `cache: true`, and these inputs:
   - the named feature packages and what they import in the Nx graph (`^production`): libraries, and npm packages at
     their locked version, patch hash included (`features: *` is the whole app: the mla-tamagui project);
   - the app shell: `frontend/mla-tamagui`'s own files without `features/` (routes, `app-shell/`, `components/`, `lib/`,
     config), not its dependency list;
   - the script itself and its data. Not the lockfile as a whole, not `patches/`, not the workspace globals (owner, 18 Oct:
     leaner); not the backends it calls: their deployment verification tests cover them (owner, 18 Oct);
   - for `e2e-android`, the device image tag and the Expo Go version, runtime inputs too.
   Not inputs: `wf` (the harness) and the e2e workflow file, per the 18 Oct rule that a harness change does not run the
   tests by itself. `[ci: android-e2e-tests]` with `--skip-nx-cache` runs them anyway.
2. `wf test run --stage <platform>-e2e-tests` runs `nx run-many -t e2e-<platform> -p <focused tests>`. A miss runs the
   test as today (one device boot per job, units in order, every run recorded and published); a hit prints the cached
   pass with its bundle URL. When every test hits, the Android job does not boot the device.
3. Only passes are replayed. A failed or errored test is never cached, so it runs again on the next push.
4. `app-routes` names every feature, so any feature change runs it; it stays the smoke test of the whole app.

Done when: a push that changes only `features/tamil-writing` runs tamil-writing's e2e tests and replays the rest; a
docs-only push runs no e2e test and boots no device. Benchmark: e2e job minutes for three such pushes, before and after.

## Risks and how each is caught

- **An undeclared input gives a stale green.** Inputs start broad (whole project dirs plus root config). A weekly
  dispatch runs everything with `--skip-nx-cache` and compares verdicts with the cached ones; any difference is a missing
  input, fixed in `nx.json`.
- **A replayed e2e pass hides a change outside its inputs** (a backend config, the device). The weekly dispatch above
  runs every focused e2e test with `--skip-nx-cache`; a verdict that differs from the cached one names the missing input.
- **Cache poisoned by a sandbox.** The sandbox and CI both write, through one client-side guard (a clean tree on `origin/dev`, node and
  pnpm as `mise.toml` pins them). The backstops are first write wins, the weekly audit and `wf cache purge nx`.
- **Disk.** `maxCacheSize` locally; the bucket is pruned at 30 days.
- **Nx itself fails.** `wf test run --stage` falls back to plain pnpm and cargo with a warning, so CI never stops on the
  cache tool.

## Expected effect

A typical push touches one or two projects. The e2e jobs, about 20 min today, run only the tests whose feature changed:
a backend-only or docs-only push runs none and boots no device, and a one-feature push runs that feature's tests and
`app-routes`. Typecheck, lint, unit and flow drop from about 8.5 min to the changed projects plus about 30 s of hashing
and replay. Deploys skip unchanged images as today, decided by hash instead of commit range.
