# greenfield — contract

> Status 2026-10-21: history. `clis/workflow-cli` was deleted when wf moved to Rust, so the layering described here is not current; wf's target architecture is type 3 in `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.

Shapes every phase builds against. Requirements: `requirements.md`. Every module named here is in `clis/workflow-cli/src/worker` unless stated otherwise.

## Units (`docs/plans/<slug>/units.jsonl`, one JSON object per line)
```ts
export type Needs = 'any' | 'arm64' | 'deployed';
export type Unit = {
  id: string;              // unique within the plan, e.g. 'p3-04'
  title: string;
  files: string[];         // 1..3 repo-relative paths; also the scheduler's conflict set
  brief: string;           // implementer prompt: Stack / TASK / CONSTRAINTS
  deps: string[];          // ids in the same plan
  gate: string;            // shell command; exit 0 = pass
  needs?: Needs;           // default 'any'
  acceptance?: string;     // text the tier-2 vision judge checks changed routes against
};
```

## Done marker
A unit is done exactly when `origin/dev` holds a commit whose message has the trailer `Unit: <slug>/<id>`. `done(slug)` = the set read with `git log origin/dev --format=%(trailers:key=Unit,valueonly)`.

## Lease
- Ref: `refs/leases/<slug>/<id>`.
- It points at a parentless commit with an empty tree. The commit message is JSON:
```ts
export type Lease = {
  worker: string;          // '<site>:<hostname>:<pid>', site = 'sandbox' | 'vm'
  attempt: number;         // 1-based; the hedge run is a separate attempt with its own worktree
  model: string;           // ladder rung in use
  startedAt: string;       // ISO
  heartbeatAt: string;     // ISO, renewed every 60 s
};
```
- **Claim:** `git push origin <leaseCommit>:refs/leases/<slug>/<id>`. Git rejects the push when the ref already exists, so a rejection means the unit is taken.
- **Renew:** `git push --force-with-lease=refs/leases/<slug>/<id>:<previous> origin <next>:refs/leases/<slug>/<id>`.
- **Expire:** any worker may run `git push origin :refs/leases/<slug>/<id>` once `now - heartbeatAt > 180 s`.
- **Release:** delete the ref after landing or on a permanent failure.
- A permanent failure leaves `refs/failed/<slug>/<id>` pointing at a commit that carries the last gate output (≤ 8 KB) for Claude to adjudicate.

## Scheduler (pure, `domain/scheduler.ts`)
```ts
export type Caps = { site: 'sandbox' | 'vm'; arm64: boolean; deployed: boolean; free: number };
export function runnable(units: Unit[], done: Set<string>, leases: Map<string, Lease>, caps: Caps): Unit[];
```
`runnable` returns units that meet all of these conditions:
- not in `done`;
- not leased, and not under `refs/failed`;
- every dep is in `done`;
- `needs` is satisfied by `caps` (`any`: either site; `deployed`: `vm` only);
- `files` does not intersect the files of any leased unit.

It returns them in units.jsonl order, at most `caps.free`.

## AIMD (pure, ported from `infra/oracle-cloud/vm-ops-api/vm-jobs.js`)
```ts
export type Controller = { limit: number; floor: 1; ceiling: number; cleanStreak: number;
  cooldownUntil: number | null; lastIncreaseAt: number | null };
export type Signal =
  | { type: 'tick'; freeMb: number; psiSomeAvg10: number | null; freeDiskMb: number | null;
      waitingOnLimit: boolean; youngestRunningAgeMs: number | null }
  | { type: 'finish'; exitCode: number; peakMb: number | null }
  | { type: 'lost' };
export function observe(c: Controller, s: Signal, now: number): { controller: Controller; change: { from: number; to: number; reason: string } | null };
```
- **Defaults:** limit 2. Ceiling: sandbox 6, VM 4.
- **Probe up:** when `waitingOnLimit && freeMb >= 3072 && (psi ?? 0) <= 10 && (freeDiskMb ?? Infinity) >= 2048`, at least 60 s since the last increase, and the youngest unit is at least 120 s old.
- **Decrease** (halve, cooldown 300 s) on any of: `exitCode === 137`, `freeMb < 1536`, `psi > 20`, `freeDiskMb < 1024`, `lost`.
- **Launch spacing:** no new launch within 60 s of the previous one.

## Worker loop (`use-cases/worker.ts`)
1. Every 15 s:
   - fetch `origin/dev` and `refs/leases/*`;
   - expire stale leases;
   - `runnable(...)`;
   - claim up to `controller.limit - running`, subject to launch spacing.
2. For each claimed unit:
   1. `git worktree add` from `origin/dev`, then `pnpm install --offline --frozen-lockfile` against the shared store;
   2. dispatch the implementer (ladder plus hedging);
   3. run `gate`;
   4. run tier 1 and tier 2 when the unit's files match `frontend/**|features/**|libraries/**`;
   5. commit with the trailer;
   6. land.
3. **Land:** `fetch` → `rebase origin/dev` → re-run `gate` if the rebase changed any file in `files`, else skip → `push origin HEAD:refs/heads/dev`.
   - Up to 3 retries on a non-fast-forward.
   - On a conflict: release the lease and leave the unit runnable.
4. **SIGTERM/SIGINT:** stop claiming, push every unit already committed, release every lease, exit 143.
5. **Telemetry:** every state change is one BetterStack event, `worker.unit.<claimed|gated|landed|failed|expired>`, carrying `{slug, id, worker, model, ms}`.

## Ladder (`clis/wf/src/data/ladder.json`)
```ts
export type Ladder = {
  pools: Record<string, { secretKey: string | null; resetHint: string; enabled?: boolean }>;
  models: Record<string, { pool: string; vision: boolean; p50Ms: number }>;   // p50 refreshed from stats
  ladders: { implementer: string[]; vision: string[]; text: string[] };
};
```
- **Hedge:** when an attempt runs past `2 * models[m].p50Ms` and `running < limit`, start the next rung as `attempt + 1` in its own worktree. Whichever attempt passes the gate first lands, and the other is killed.
- **Escalate to Claude:** after 2 rungs fail the gate, write `refs/failed/...`.

## Verify report (per dev SHA)
- Stored in OCI at `verify/<sha>.json`. An HTML render is published as an artifact.
```ts
export type TierResult = { tier: 1 | 2 | 3; status: 'pass' | 'fail' | 'skip'; ms: number;
  items: { name: string; status: 'pass' | 'fail'; detail?: string; screenshot?: string }[] };
export type VerifyReport = {
  sha: string; units: string[];             // Unit trailers landed in this sha range
  tiers: [TierResult, TierResult, TierResult];
  leadTime: { preLandMs: number[]; deployVerifiedMs: number | null };
  deployed: { app: string; commit: string; healthOk: boolean }[];
};
```
- **Tier 2 visual:** for each route, a Playwright screenshot on the web export.
  - Deterministic fails: blank page, console error, render throw.
  - Pixel diff against `baselines/<route>/<prevDevSha>.png` (threshold 0.1%).
  - On a changed route, a fail additionally needs both vision judges (two different pools) to agree against `acceptance`.
  - Baselines: OCI `baselines/`, pruned to the last 20 SHAs.
- **Tier 3:** runs once every changed backend reports `/health.commit == sha`, and `GET /services/metro-runner/dev-bridge/status` reports `{ commit: sha }`, after a `POST …/dev-bridge/sync`.
  - App e2e uses Playwright against the shared Metro's web bundle on `metro-runner-main:20000`, the same server Expo Go loads from.
  - During the run, status reports `testing: sha`.
  - Runs the suites mapped to the apps, plus the EAS Update for frontend changes.
  - A failure appends a fix unit to `docs/plans/fix-<sha8>/units.jsonl` on dev.

## Plan PR description (rewritten after every tier-3 run)
```md
**Try it:** [Open in Expo Go](http://vm-ops-api-main:8080/expo-go?url=exp%3A%2F%2Fmetro-runner-main%3A20000) · `exp://metro-runner-main:20000`
**Metro serves:** `dev@<sha8>` · **Verify:** flow <✅|❌> · visual <✅|❌> · e2e <✅|❌|⏳> · [report](<artifact url>)
```

## Knowledge ledger
```ts
export type GapRecord = {          // docs/gaps/<slug>.md frontmatter
  status: 'open' | 'blocked'; area: string; paths: string[];
  opened: string; last_verified: string; verify?: string; links: string[];
};
export type LearningExtras = { paths: string[]; from_gap?: string };  // added to docs/solutions frontmatter
export function knowledgeFor(files: string[], records: (GapRecord | Learning)[]): Array<GapRecord | Learning>; // glob match on paths, ranked open-gaps-first then recency, top 3
```
- `wf knowledge for <files…>`: the result of `knowledgeFor`, as markdown links.
- `wf knowledge index`: regenerates `docs/known-gaps.md`; the budget check fails above 200 lines.
- `wf knowledge stale`: lists gaps with `last_verified` more than 30 days old, so `wf offload plan` can enqueue re-verify units.
- **Closing a gap** happens only through `/cf-compound <slug>`. One commit writes or updates the learning with `from_gap: <slug>` and deletes the gap file.

## Health contract
- **HTTP apps:** `GET /health` → `200 { ok: true, commit: string, startedAt: string }`.
- **Other containers:** a compose `healthcheck`.
  - dind: `docker info`;
  - sessions: `pgrep sshd`;
  - tailscale: `tailscale status --json`.
- **Entrypoints:** every one removes `*.pid` files under `/var/run` and `/run` that it owns before starting its daemon.

## Watchdog (`infra/oracle-cloud/watchdog/`, own container, ≤ 200 LOC)
Every 30 s:
- inspect every compose container;
- an unhealthy container, or one that restarted more than 3 times in the last 5 min, gets `docker compose up -d --force-recreate <service>` and an `error`-level `watchdog.recreated` event;
- a lease with no heartbeat for 30 min → `watchdog.stale_lease`;
- a deployer stage longer than its timeout → `watchdog.deploy_stuck`.

## Deployer timeouts
| Stage | Timeout |
|---|---|
| fetch | 60 s |
| build | pressure-gated (below) |
| compose up | 300 s |
| health confirm | 300 s |

- The build stage has no fixed deadline. It starts only when `/proc/pressure` shows headroom (io `some` avg60 < 20, memory `full` avg60 < 5), otherwise it waits with doubling backoff capped at 60 s and logs `trunk_deploy.waiting_for_headroom`. It is cancelled as `trunk_deploy.build_stalled` when io `full` avg10 stays ≥ 70 for a full 60 s window while the job log grows by no bytes. Cancel signals the job's process group (TERM, then KILL after 10 s), so no `docker build` client survives. A 2 h ceiling covers wait plus run as a last resort (`stage_timeout`). Without PSI the build is admitted with a `trunk_deploy.pressure_unavailable` warning.
- A breach is `trunk_deploy.stage_timeout` at `error` level.
- A compose diff on any service → `up -d --force-recreate` for that service (no `--no-deps` shortcut for changed services).

## `wf` command surface (P6 target)
`work <slug>`, `status <slug>`, `worker [--max N]`, `dispatch "<prompt>"`, `verify <sha>`, `vm status|recover|logs <app>|exec <file>`, `deploy status|run <app>`, `secrets get|set|sync-env`, `metro`, `browser`, `ios`, `android`, `agents sync`, `caveat on|off`, `knowledge for|index|stale`.

## Spawn guard
Every `spawn(..., { detached: true })` passes:
- `env.WF_SPAWN_DEPTH = String(depth + 1)`; the callee refuses to run when the depth is ≥ 2;
- a `timeout`.

ESLint rule `wf/guarded-detached-spawn` enforces both.

## Verification of this contract
- `domain/scheduler.ts` and the AIMD port: unit tests with Given/When/Then, 100% branch coverage on the pure functions.
- Lease protocol: integration test against a local bare repo:
  - two concurrent claims → exactly one wins;
  - expiry → reclaim.
- P3 exit gate (runbook): a 10-unit plan across both workers; the sandbox is killed mid-run; zero lost commits.
