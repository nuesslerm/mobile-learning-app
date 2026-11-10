# greenfield — requirements (rev 2)

## 0. Context and intent
The user dislikes the current setup. They want it rebuilt as if greenfield, but by migrating this repo, not starting a new one.

Optimisation targets, in priority order:
1. **Robust.** Nothing dies silently; every failure is visible and self-heals.
2. **Simple.** Fewer moving parts; deleting code is the default.
3. **Iterable.** A change goes from idea to verified on `dev` in minutes.
4. **Maximal resource use.** The sandbox and the VM both do real work in parallel.

Constraints:
- Cheap models are free (subscriptions). Model choice optimises success rate and latency only; extra parallel calls cost nothing.
- Verification on `dev` runs in a fixed order: flow tests first, then visual checks, then e2e.

This spec replaces the "consolidate, remove nothing" simplify-setup direction. simplify-setup is frozen: s18 is not resumed, and its wf registry and usage DB get the same keep/delete review as everything else (§6).

**User decisions (2026-10-03):**
- Every product surface stays: Tamil, LeetCode, coding agent, dashboards/reports.
- Of the bots, only remote-chat-agent stays. opencode-telegram-bot and plannotator-proxy are deleted.
- The codex, gemini and opencode config mirrors stay, through a slimmed config sync.
- caveat-guard stays, but becomes opt-in and off by default.

## 1. Measured starting point (2026-10-03)

### Size
- 4,071 tracked files, 38 workspace projects, 3,395 lockfile entries, `node_modules` 3.1 GB.

| Area | LOC (ts/js/sh, without bundles) | Biggest |
|---|---|---|
| clis/ (16 packages) | ~160k | workflow-cli 44.3k, convention-lint 14.5k, multi-browser 13.0k, agent-evals 9.8k, cheap-flow 8.7k, vnc-auth-capture 8.6k, opencode-harness 8.3k, infra-ops 8.2k, agent-approve-guard 7.0k, caveat-guard 6.0k, secrets 5.8k, agent-config-sync 5.6k, harness 5.0k, env-bootstrap 4.9k |
| vm-ops-api | 13.1k | server.js alone is 9,137 lines |
| libraries/ (17) | ~47k | eslint-plugin-mobile-learn 12.4k, tamagui-design-system 7.8k, logging 6.4k |
| backend/ (13) | ~71k | remote-chat-agent 34.2k, opencode-telegram-bot 23.3k |
| product frontend | ~35k app + ~38k features/* | |

### Process
- `lefthook.yml`: 362 lines, ~30 pre-commit jobs; the commit gate runs on the VM.
- Agent hook commands: 19.
- Root scripts: 62.
- Convention rules: 47.
- Baseline/waiver JSONs: 8.
- AGENTS/CLAUDE.md files: 10.
- Docs: 513 files, including 170 solutions.
- `docs/known-gaps.md`: 10,161 lines.

### Executors
- **Sandbox:** x86_64, 4 CPU, 15 GB RAM, about 9 GB free disk. Ephemeral, has Chromium. Today it only orchestrates.
- **VM:** arm64, 2 OCPU, 12 GB. Runs 9 apps plus agent-shell (dind), and does every gate, build and test. vm-jobs uses AIMD with a ceiling of 8.

### Models (124 runs)

| Model | Result | p50 |
|---|---|---|
| deepseek-v4-flash | 47/47 | 92 s |
| MiniMax-M3 | 42/42 | 640 s |
| kimi-for-coding | 14/14 | – |

- 20 runs failed on infrastructure or rotation before any model ran.

### Failures this week
- A stale dind pidfile left builds hanging silently for 7 h.
- A usage-recorder fork loop drove load to 18–22.
- 4 parallel installs hung the VM; it needed a hard reset.
- A missing Dockerfile COPY caused a crash loop.
- A probe parse bug.
- A stale bare-repo origin.
- Lost commits from concurrent pushes.
- Nested nx recursion.

Common root cause: bespoke layers, each with its own failure mode, and no shared health model.

## 2. Target architecture

```
 Claude (plans, reviews, adjudicates) ──writes──▶ docs/plans/<slug>/units.jsonl (DAG, files[] per unit)
                                                     │
                     STATE = GIT (GitHub): units.jsonl + lease refs + `Unit: <id>` commit trailers on dev
                     ┌──────────────────────────────┴─────────────────────────────┐
           SANDBOX WORKER  `wf offload worker`                                  VM WORKER  `wf offload worker`
           x86 · 4 CPU · 15 GB · ephemeral                              arm64 · 2 OCPU · 12 GB · always on
           cheap-model implementer · lint/type/unit                     image builds · deploy · e2e/runbooks
           flow tests · web export · Playwright visual                  shared dev Metro · EAS update
           AIMD(RAM, PSI, free disk)                                    AIMD(RAM, PSI)
                     └──────── pre-land: tier 1 flow + tier 2 visual ──┬──┘
                                                                       ▼
                                      rebase → push to dev (atomic; lease held)
                                                                       ▼
                     VM deployer: changed apps → build → up → /health sha ──▶ tier 3 e2e ──▶ verify report
```

### 2.1 State in git, not in a service
**Review finding:** a queue service on the VM is a single point of failure. The VM is the component that failed most this week.

Decision: **all coordination state lives in GitHub.** No new service.

- **Units:** `docs/plans/<slug>/units.jsonl`, unchanged schema `{id, title, files[], brief, deps[], gate}`, plus a `needs: any|deployed` field.
- **Done:** a unit is done exactly when `dev` holds a commit with the trailer `Unit: <slug>/<id>`. The status is derivable and needs no manifest file to drift.
- **Lease:**
  - A worker claims a unit by atomically creating `refs/leases/<slug>/<id>` with `git push origin <sha>:refs/leases/...`; the push fails if the ref exists.
  - The ref points to a commit whose message is `{worker, startedAt, heartbeatAt}`.
  - Heartbeat: the worker force-pushes an updated lease commit every 60 s.
  - Any worker deletes a lease whose heartbeat is more than 180 s old.
  - So a dead sandbox or VM frees its units within 3 min, and either side can run alone.
- **Scheduler:** a pure function both workers run: `runnable(units, doneSet, leases, myCaps)`.
  - It picks units whose deps are all done and whose `needs` match the worker.
  - It skips any unit whose `files[]` overlap a unit already leased. This prevents the conflict churn that parallel landing otherwise causes.

### 2.2 Workers
- **Same code on both machines:** `wf offload worker [--max N]`.
- **AIMD per worker,** reusing the pure `observe` and `nextRunnable` from `vm-jobs.js`:
  - start at 2, floor 1;
  - ceiling: sandbox 6, VM 4;
  - +1 per minute while there is at least 3 GB memory headroom, PSI ≤ 10, and (sandbox only) at least 2 GB free disk;
  - halve on OOM, PSI > 20, or low memory or disk;
  - warm-up: 60 s between launches.
- **Isolation:** one worktree per unit and one shared pnpm store per machine, warmed once. A unit's install is then mostly hard links, which removes the install spike that hung the VM. Worktrees are deleted after landing.
- **Landing,** while still holding the lease:
  - `git fetch origin dev`, rebase, re-run the unit gate if the rebase touched the unit's files, then `git push origin HEAD:dev`;
  - a rejected push retries up to 3 times;
  - a real conflict re-queues the unit on the new base;
  - SIGTERM triggers push-then-exit (kept from today's session script).

### 2.3 Placement (initial; P0 confirms it with timings)

| Work | Where | Reason |
|---|---|---|
| Cheap-model implementer calls | sandbox, VM overflow | network-bound; the sandbox has more RAM headroom |
| lint, typecheck, unit, flow tests | sandbox | 4 x86 cores beat 2 OCPU, and the VM stays free |
| Web export, Playwright screenshots, vision judge | sandbox | Chromium is preinstalled; no deploy needed |
| Docker image builds | VM | native arm64; the sandbox would emulate (the approve-guard rule stays) |
| Deploy, e2e, runbooks, shared dev Metro, EAS update | VM | needs the tailnet and the deployed services |
| Pre-commit gate | whichever machine commits (normally the sandbox) | removes the VM round-trip and the patch handoff in `precommit.ts` |

### 2.4 Who implements the migration (user decision, 2026-10-04)
- **Claude drives every implementation unit of this greenfield migration.**
  - It works inline for small, known-location edits.
  - It uses Claude subagents (the Agent tool) where a unit is independent or needs wide reading. Independent units run as parallel subagents.
  - For this plan, this overrides the repo rule "never delegate through the Agent tool" and `/cf-work`'s rule that Claude never implements units.
- Tests and gates still run as specified: tiers 1–2 on the sandbox, image builds and tier 3 on the VM.
- The cheap-model ladder below remains the runtime for `wf offload ask`, the vision and text judges, re-verify units and the knowledge-ledger migration. It is no longer the implementer for greenfield units.

### 2.4b Cheap models
- **One ladder,** ordered by okRate, then p50:
  - implementer: deepseek-v4-flash → kimi-for-coding → k3 → MiniMax-M3 → Haiku → Sonnet;
  - vision: kimi-for-coding → MiniMax-M3 → Sonnet.
- Cost is removed from all logic. Pool caps and cooldowns stay, because subscriptions have rate windows.
- **Hedging:**
  - when a run passes 2× its model's p50 and the worker has spare capacity, start the next rung in a separate worktree;
  - the first run to pass the gate wins and the other is killed;
  - this replaces sequential escalation for slow tails.
- **Claude** writes requirements, contracts and units, reviews, and adjudicates units that failed on 2 rungs. It never implements units.

### 2.5 Verification (fixed order, fail fast)

**Review finding:** e2e only after landing means `dev` can break.

Decision: tiers 1 and 2 run **before landing**. Tier 3 runs **after deploy**.

**Tier 1 — flow (pre-land, sandbox).**
- `test:flow` with harness doubles and Given/When/Then, for every unit touching `frontend/**` or `features/**`.
- Affected unit tests for everything else.

**Tier 2 — visual (pre-land, sandbox).**
- The visual check:
  1. web export;
  2. Playwright on the routes the unit's files map to (route map derived from imports);
  3. deterministic checks: render, no console errors, not blank;
  4. pixel diff against the dev baseline.
- **Review finding:** LLM judges are noisy. A vision judge runs only on routes the unit changed, against the unit's acceptance text. A failure needs two models from different pools to agree.
- Unchanged routes that differ are regressions with no judge involved.
- Baselines live in OCI Object Storage, keyed by `route/devSha` and pruned to the last 20 SHAs, not in git.

**Tier 3 — e2e (post-deploy, VM), on the shared dev Metro.**
- App e2e runs against the **same shared dev Metro the user opens in Expo Go**: `exp://metro-runner-main:20000`, served by `metro-runner` on the `dev` trunk. While a change is being tested, the phone already runs the build under test. No separate e2e bundle is built.
- Sequence after a land:
  1. The deployer confirms that every changed backend reports `/health.commit == dev head`.
  2. `POST /services/metro-runner/dev-bridge/sync` moves the shared Metro's worktree to the dev head.
  3. Tier 3 waits until `dev-bridge/status` reports that sha.
  4. Playwright runs the app e2e against the same Metro's web bundle, on the same server, port and JS that Expo Go loads.
  5. The backend e2e and runbook suites mapped to the changed apps run.
- **Busy flag:** while tier 3 runs, `dev-bridge/status` reports `testing: <sha>`. The Expo Go tap-through page shows "under test" instead of hiding the build, so the user can keep using it.
- Frontend changes also publish an EAS Update with `--initial-route`, which keeps today's mobile definition of done.

**PR description.**
- Every plan run keeps one draft PR from its session branch into `dev`.
- The PR description is rewritten after every tier-3 run and always carries:
  - the Expo Go link for the shared dev Metro, in the tap-through form GitHub keeps: `[Open in Expo Go](http://vm-ops-api-main:8080/expo-go?url=exp%3A%2F%2Fmetro-runner-main%3A20000)`, plus the raw `exp://metro-runner-main:20000` in a code span;
  - the dev sha that Metro serves;
  - the three tier results;
  - a link to the verify report.

**Results.**
- One verify report per dev SHA: JSON in OCI plus a static HTML artifact.
- A tier-3 failure enqueues a fix unit, linked to the SHA, at the head of the DAG.
- No automatic reverts, to avoid revert churn. The user sees the red report.

### 2.6 Deploy
- The VM deployer polls `dev`, maps changed paths to apps, builds only the changed images with a registry build cache, runs `compose up`, and confirms the `/health` sha.
- Every stage has a timeout. A stuck or failed stage becomes a BetterStack error within 5 min (fixes the 7 h silent hang).
- A compose change to any service fully recreates that service, not just `app` (fixes the `--no-deps` blind spot that left dind broken).
- Self-update keeps today's proven sibling-container mechanism.

### 2.7 Robustness
1. **Health contract.**
   - HTTP apps expose `/health` returning `{ok, commit, startedAt}`.
   - Every other container gets a compose `healthcheck` (dind: `docker info`).
   - Every entrypoint clears stale pidfiles before starting.
2. **Watchdog.** A separate ~200-line container that survives any vm-ops-api rewrite.
   - A container that is unhealthy or restart-looping (more than 3 restarts in 5 min) is recreated and logged as an error.
   - A lease older than 30 min with no new commits is logged as an error.
3. **Bounded spawns.**
   - Every detached spawn carries a recursion-guard env var and a timeout.
   - An eslint rule enforces both.
4. **`wf vm recover`:** health sweep, recreate unhealthy containers, OCI soft reboot if the API is unreachable, hard reset after 180 s, re-verify. No human step.
5. **Babysitting is structural** (leases + watchdog + report), so no hand-written Monitor scripts or scheduled check-ins.

### 2.8 Simplicity budget (enforced by one convention check)
- Tooling LOC: no line budget (user decision 2026-10-05): tooling is as small as it can be and no smaller, so deletions are driven by callers and design, not by a count.
- Pre-commit: ≤ 5 jobs (lint-staged, typecheck affected, unit affected, flow affected, secrets scan).
- Agent hooks: ≤ 4 (approve-guard, session setup, output masking, caveat-guard when opted in).
- Instructions: one root CLAUDE.md of ≤ 150 lines, with area files only where needed.
- `known-gaps.md`: becomes a generated index of open gaps (§2.10), ≤ 200 lines. Nothing is archived unread.
- Root scripts: ≤ 15.
- **nx:** dropped in favour of `pnpm --filter "...[origin/dev]"` affected filtering. That also deletes cli-nx-plugin and the nx-cache proxy. Exception: P0 shows the cache saves ≥ 30% wall time.

### 2.10 Knowledge compounding (replaces the 10k-line `known-gaps.md`)
**What exists today:**
- `docs/known-gaps.md` holds 110 entries in 10,161 lines. The median entry is 31 lines, but one runs to 2,180 lines.
- At least 44 entries are already marked resolved but never left the file.
- `docs/solutions/` holds 170 schema-checked learnings (frontmatter: `title, date, category, tags, problem_type, module`), written by `/cf-compound`.
- The knowledge is real, but it is write-only: no agent reads a 10k-line file before touching code. That is why the same gaps keep being rediscovered.

**Design:** one ledger, two record types, one lifecycle. This is the same shape as a personal wiki: small linked pages, not one long log.

1. **Gap record:** `docs/gaps/<slug>.md`, one file per open problem.
   - Frontmatter: `status: open|blocked`, `area`, `paths: [globs]`, `opened`, `last_verified`, `verify: <command>` (optional, exits 0 once fixed), `links: [solutions/…, gaps/…]`.
   - Body: at most 40 lines under three headings: What is broken, Why blocked, What unblocks it. Evidence goes in links, never in pasted logs.
2. **Learning record:** the existing `docs/solutions/<category>/<slug>.md`, gaining two frontmatter fields: `paths: [globs]` and `from_gap: <slug>`.
3. **Lifecycle (the compounding step):**
   - A gap closes only through `/cf-compound`. It writes or updates the learning that would have prevented the gap, links it, and deletes the gap file in the same commit.
   - Git history keeps the narrative.
   - A gap that taught nothing reusable is deleted with a one-line reason in the commit.
4. **Retrieval** (the reason it compounds): `wf knowledge for <files…>` returns the open gaps and learnings whose `paths` globs match.
   - `wf offload plan` prepends the top 3 matches to every unit brief.
   - The reviewer agent gets the same list.
   - Knowledge reaches the model at the moment it is relevant, without anyone reading an index.
5. **Freshness:**
   - A gap whose `last_verified` is older than 30 days gets a cheap-model re-verify unit. That unit runs `verify:` when present; otherwise it re-checks the "why blocked" claim.
   - The outcome closes the gap (through cf-compound) or bumps `last_verified`.
6. **Index:** `docs/known-gaps.md` is generated by `wf knowledge index`: one line per open gap (`area · title · age · link`). The budget check fails when it passes 200 lines, which forces closing or merging instead of appending.
7. **Concepts:** `CONCEPTS.md` stays the hub. A learning that introduces a term links it there, and the budget check flags dangling links.

**Migration (P5):** cheap-model units of about 10 entries each classify every section:
- **open:** becomes a gap file;
- **resolved with a reusable lesson:** becomes cf-compound into `docs/solutions`;
- **resolved, no lesson:** dropped.

A single snapshot `docs/archive/known-gaps-2026-10.md` is kept read-only for traceability. A review unit samples 10% of the dropped entries for wrongly discarded knowledge.

### 2.9 Iterability metrics (in every verify report)
**Review finding:** image builds dominate lead time, so it is measured in two parts.

- **Pre-land lead time** (unit leased → landed with tiers 1 and 2 green): target p50 ≤ 10 min, p90 ≤ 20 min.
- **Deploy-verified lead time** (landed → tier 3 green): target p50 ≤ 20 min, p90 ≤ 40 min.
- **Throughput:** units landed per hour.
- **Utilisation:** average running units per worker against its AIMD limit.

## 3. The slim `wf` (`clis/wf`, ≤ 15k LOC, hexagonal, one esbuild bundle)

Correction, 2026-10-21: wf is a Rust crate, not an esbuild bundle, and section 2.8 dropped the LOC budget. Its target architecture is type 3 in `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.

| Command | Replaces |
|---|---|
| `wf offload plan <slug>` / `wf offload plan --status <slug>` | cheap-flow run-work, cheap-flow-watch, manifests |
| `wf offload worker [--max N]` | vm-jobs launcher, session scripts, precommit handoff |
| `wf offload ask "<prompt>"` | thin client of remote-chat-agent's one-shot run mode (§3a); opencode-harness is deleted |
| `wf check --deployed <sha>` | report-health tiers, runbook runner, flow-test |
| `wf vm status\|recover\|logs <app>\|exec <file>` | vm-ops, agent-shell, infra-ops (reboot) |
| `wf ship --status\|run <app>` | agent-shell release/build/redeploy |
| `wf secret get\|set\|sync-env` | secrets (bitwarden + macos backends only) |
| `wf ship mla-tamagui\|browser\|ios\|android` | kept as-is (audited in P0) |
| `wf knowledge for\|index\|stale` | known-gaps.md by hand, cf-compound lookups |
| `wf agent-config sync` | agent-config-sync, slimmed to claude/codex/gemini/opencode mirrors |

The cf-* skills stay as thin prompt files that call `wf offload plan` and `wf offload plan --status`.

## 3a. One agent runner: remote-chat-agent (user decision, 2026-10-04)
- Today there are two runners:
  - **opencode-harness** (8.3k LOC): one-shot headless jobs, with the model ladder, rotation, pool cooldowns, stats ledger and output checks;
  - **remote-chat-agent** (rca): long-running interactive threads, with claude, codex and opencode drivers and streaming.
  - They share only CLI spawning and watching; rca imports only `createIdleWatchdog` from the harness.
- **Decision:** rca is the only runner. A one-shot run is a thread with one message: start, wait for completion, return the output and the diff stats.
- **Run plugins.** rca gains a small plugin interface around every run, interactive or one-shot. The four harness features become plugins, not core code:

  ```ts
  type RunPlugin = {
    name: string;
    selectModel?(ctx: RunContext, tried: Attempt[]): ModelChoice | null;   // ladder
    beforeAttempt?(ctx: RunContext, attempt: Attempt): Skip | void;      // pool cooldowns
    validateOutput?(ctx: RunContext, out: AttemptOutput): Verdict;       // bad-output / wrote-nothing checks
    afterAttempt?(ctx: RunContext, attempt: Attempt, verdict: Verdict): void; // stats ledger, cooldown learning
  };
  ```

  - **Plugins:** `ladder` (model-ladders.json, rotation, escalation, hedging), `cooldowns` (pool ledger), `ledger` (run log and stats), `output-checks`.
  - **Enabling:** each run carries a plugin list. One-shot runs enable all four by default; interactive threads opt in, typically to ladder and ledger.
  - **Isolation:** plugins are plain modules in `backend/remote-chat-agent/src/plugins/<name>/`, each with its own unit tests (Given/When/Then). Drivers (claude, codex, opencode) never import a plugin; the run use-case calls the hooks.
- **Two entrypoints over the same use-cases:**
  - the server (VM, today's routes plus `POST /runs`);
  - a one-shot CLI (`rca run --prompt … --cwd … --shape …`), so a sandbox worker runs units against its local worktree without a network hop.
- `wf offload ask`, cheap-flow and caveat-guard call the rca CLI. opencode-harness is deleted, and its tests move with the code they cover.
- **Migration (P6):** strangler.
  1. Move the ladder, cooldowns, ledger and checks into rca, with their tests.
  2. Add the CLI entrypoint.
  3. Repoint `wf offload ask`, cheap-flow and caveat-guard.
  4. Delete opencode-harness.
- Exit gate: the harness test scenarios pass against rca, and a cheap-flow unit lands through `rca run`.

## 4. P0 — measure before cutting (sandbox, read-only, ~1 h)
Outputs: `inventory.md` and `plan-research.md`.

1. **Usage ledger per package:**
   - inbound imports;
   - whether it is deployed;
   - whether hooks or lefthook use it;
   - `wf` calls in the last 30 days, from BetterStack when WF_INVOCATIONS is empty;
   - last commit.
   - Verdict: keep, fold into `wf`, or delete.
2. **Timings, sandbox vs VM (wall clock and peak RSS):** lint, typecheck, affected unit tests, `test:flow`, web export, one Playwright pass, one image build (VM), one e2e, nx cached vs uncached.
3. **Model re-rank** per shape: okRate, p50, p90.
4. **Sandbox lifetime check:** does a running `wf offload worker` keep the cloud container alive while the session is idle? This decides whether the sandbox is primary capacity or only opportunistic.
5. **Disk per worktree:** measured with the shared pnpm store, to set the sandbox disk ceiling.

## 5. Migration phases
Every phase is its own `units.jsonl`. Units touch 1–3 files, land on dev alone, and are verified tier 1 → 2 → 3.

| Phase | Goal | Exit gate (Tier 1 runbook unless noted) |
|---|---|---|
| **P0 Measure** | §4 | user approves inventory.md |
| **P1 Stop the bleeding.** Only code that survives the rewrite | entrypoint pidfile hygiene, compose healthchecks for all services, watchdog container, deployer stage timeouts and alerts, deployer full-recreate on compose change, spawn guard lint, `wf vm recover` | inject each §1 failure (stale pid, restart loop, hung build, fork recursion): each is auto-recovered or reported in BetterStack within 5 min |
| **P2 Sandbox gates locally** | lefthook to ≤ 5 jobs run locally; VM pre-commit gate retired | commit-to-push p50 before vs after; VM idle during commits |
| **P3 Git-state workers** | lease refs, `Unit:` trailers, scheduler, `wf offload worker` on both machines, AIMD per worker, shared pnpm store | 10-unit test plan runs ≥ 4 units at once across both machines; killing the sandbox mid-run leaves the VM to finish within 3 min; zero lost commits; zero overlapping-files concurrency |
| **P4 Verify pipeline** | tier 2 visual (pixel diff, two-model judge, OCI baselines), tier 3 post-deploy with EAS update, verify report, auto fix units | a seeded visual regression is caught at tier 2 and a seeded API break at tier 3 |
| **P5 Delete** | everything §6 marks delete; instruction files collapsed; known-gaps migrated into the knowledge ledger (§2.10); nx removed if P0 agrees | §2.8 budget check green; full verify green on the dev head |
| **P6 Fold into `clis/wf` + vm-ops-api strangler** | new `wf` commands one at a time, each with a test, old command deleted in the same unit; vm-ops-api routes move to a slim service one route per unit; server.js deleted last | all §3 commands tested; server.js gone; deploys still confirm the sha |
| **P7 Product tidy** | merge features/* contract/impl/mocks triplets where the split adds nothing; delete the opencode-telegram-bot and plannotator-proxy packages and their deploy wiring | app tiers 1–3 green |

Order rationale:
- P1 comes first: nothing else is safe while the VM can die silently, and P1 only builds things that outlive the rewrite.
- P2 is the cheapest speed-up.
- P3 and P4 build the new spine.
- Deletion follows proven replacement.
- P1–P2 land through today's cheap-flow; P3 onward runs on the new workers (self-hosting).

**Execution mode (user decision, 2026-10-04): local first, then one VM cutover.**
- Every phase is built and verified on the sandbox, and commits land on the session branch only. `dev` is not fast-forwarded, because each dev push makes the old trunk deployer rebuild on the VM.
- The VM is left alone apart from break-glass recovery.
- When the VM architecture is simplified (P1 robustness, P6 slim vm-ops-api), the branch lands on `dev` once, followed by a single full redeploy:
  - every image is built from that sha;
  - every compose service is recreated (`recreate=all`);
  - each one is confirmed through `/health`.
- That is followed by one tier-3 run on the shared Metro.

## 6. Keep / fold / delete
- **Keep:**
  - product: frontend/mla-tamagui with all routes, features/*;
  - backends: leetcode-backend (+ contracts and mock), leetcode-auth-cookie-capture (+ contracts and mock), remote-chat-agent (+ contracts and mock), reporting-service, vm-gateway;
  - libraries: tamagui-design-system, flow-test-harness, logging (slimmed), app-config, app-di, vm-request-auth, voice-edit-dsl, gesture-geometry, registry-service and rate-limit if P0 shows they are imported;
  - the vnc-auth-capture core (needed for leetcode/claude logins);
  - eslint-plugin-mobile-learn, cut to the rules actually firing.
- **Merge into remote-chat-agent:** opencode-harness (§3a).
- **Fold into `clis/wf`:** workflow-cli, cheap-flow, secrets, infra-ops (OCI recover), agent-config-sync (slim mirror sync), agent-approve-guard (deny list), caveat-guard (opt-in hook, off by default).
- **Delete:**
  - backends opencode-telegram-bot and plannotator-proxy;
  - clis agent-evals, arch-diagrams, harness, env-bootstrap (mise + session setup script), logger CLI, multi-browser (plain Playwright), convention-lint (≤ 5 rules move into eslint, plus the §2.8 budget check);
  - simplify-setup's usage DB and registry;
  - cli-nx-plugin and the nx-cache proxy (if P0 agrees);
  - 8 baseline/waiver JSONs, about 25 lefthook jobs, about 15 hook commands;
  - 38 old plans (archived).
- **P0 re-checks** every delete item against the usage ledger. Anything still in use moves to fold.

## 7. Risks
- **The sandbox's ephemeral lifetime is unknown.** P0 item 4 measures it; leases make the design correct either way.
- **9 GB free on the sandbox:** shared store, delete worktrees after landing, AIMD on disk.
- **x86 and arm64 drift:** image build per app on the VM before deploy; tier 3 catches runtime drift.
- **GitHub rate limits on lease heartbeats:** about 1 push/min per active unit; at ≤ 10 concurrent units that is far below limits. The heartbeat drops to 120 s if P3 sees throttling.
- **Deleting something in use:** P0 ledger, plus deletion only after a proven replacement.
- **Running the migration with the old system:** P1–P2 are small; from P3 the new workers carry the rest.
- **Subscription rate windows:** pool cooldowns plus hedging spread across pools.

## 8. Adversarial review log (rev 1 → rev 2)

| # | Finding | Decision |
|---|---|---|
| 1 | Queue service on the VM is a SPOF, and the VM was the top failure source | state in git (lease refs + trailers); no service |
| 2 | Parallel units on the same files cause rebase churn | scheduler skips units with overlapping `files[]` |
| 3 | e2e after landing lets `dev` break | tiers 1–2 before landing, tier 3 after deploy with auto fix units, no auto-revert |
| 4 | Vision judges are noisy | deterministic checks + pixel diff gate; judge only on changed routes, 2-pool agreement |
| 5 | Visual baselines in git bloat the repo | OCI Object Storage, pruned to 20 SHAs |
| 6 | One lead-time target hides slow image builds | split into pre-land and deploy-verified metrics |
| 7 | P1 hardening of server.js is wasted when it is rewritten | P1 builds only surviving pieces (entrypoints, healthchecks, separate watchdog) |
| 8 | Big-bang vm-ops-api rewrite is risky | strangler, one route per unit, server.js deleted last |
| 9 | nx adds a plugin, a cache proxy and recursion bugs | pnpm affected filters, unless P0 shows ≥ 30% cache gain |
| 10 | Sandbox lifetime unknown | P0 measures it; design correct either way via leases |
| 11 | Hedging doubles worktrees | only with spare capacity; the loser is killed immediately |
| 12 | Spec dropped mobile definition of done | EAS Update with `--initial-route` stays in tier 3 |
| 13 | Deleting tools still in use | P0 ledger re-checks every delete; deletion only after a proven replacement |
