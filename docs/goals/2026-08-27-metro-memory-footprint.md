---
id: metro-memory-footprint
title: Metro memory footprint — profile it, then cut it
status: superseded
created: 2026-08-27
branch: claude/metro-memory-footprint-loop
criteria:
  # ---- Phase 1: measurement capability + baseline. No optimization yet.
  - id: p1.profiler-exists
    phase: 1
    description: '`wf ship mla-tamagui` samples a running Metro instance and records peak RSS, module count, bundle bytes, and cold/warm bundle wall-time into docs/goals/metro-memory-profile.json. Lives in a clis/* package surfaced via wf, never scripts/.'
    command: |-
      wf ship mla-tamagui --help > /dev/null 2>&1 \
        && test -d clis/workflow-cli/src/use-cases/metro \
        && grep -rq "metro-memory-profile.json" clis/
  - id: p1.profile-artifact-shape
    phase: 1
    description: 'docs/goals/metro-memory-profile.json exists and declares the VM RAM budget the concurrency math is against'
    command: |-
      node -e "
      const d = require('./docs/goals/metro-memory-profile.json');
      if (!d.vm || !(d.vm.totalRamGb > 0)) process.exit(1);
      if (!Array.isArray(d.measurements)) process.exit(1);
      "
  - id: p1.baseline-recorded
    phase: 1
    description: 'The CURRENT dev-bridge configuration is measured and recorded as measurement id "baseline-all-impl" (every feature -impl, --max-old-space-size=4096) with real non-zero peak RSS, module count and cold-bundle time'
    command: |-
      node -e "
      const d = require('./docs/goals/metro-memory-profile.json');
      const m = d.measurements.find(x => x.id === 'baseline-all-impl');
      if (!m) process.exit(1);
      for (const k of ['peakRssMb','moduleCount','bundleBytes','coldBundleMs']) {
        if (!(m[k] > 0)) process.exit(1);
      }
      if (m.config.maxOldSpaceMb !== 4096 || m.config.features !== 'all-impl') process.exit(1);
      "

  # ---- Phase 2: attribute the cost per lever, independently, before adopting any
  - id: a1.feature-mocks-measured
    phase: 2
    description: 'Measurement id "mocks-except-one" recorded: only the feature under test gets its -impl, every other feature stays a mocks stub. This is the lever DEV_BRIDGE_START_SCRIPT currently overrides to maximum (server.js:327-339).'
    command: |-
      node -e "
      const d = require('./docs/goals/metro-memory-profile.json');
      const m = d.measurements.find(x => x.id === 'mocks-except-one');
      if (!m) process.exit(1);
      for (const k of ['peakRssMb','moduleCount','bundleBytes','coldBundleMs']) {
        if (!(m[k] > 0)) process.exit(1);
      }
      "
  - id: a2.heap-ceiling-measured
    phase: 2
    description: 'Measurement id "heap-reduced" recorded: same feature scope as the baseline but a lower --max-old-space-size, to separate V8 heap-ceiling effects (a ceiling V8 grows toward under GC pressure) from genuine live-set size'
    command: |-
      node -e "
      const d = require('./docs/goals/metro-memory-profile.json');
      const m = d.measurements.find(x => x.id === 'heap-reduced');
      if (!m) process.exit(1);
      if (!(m.config.maxOldSpaceMb > 0) || m.config.maxOldSpaceMb >= 4096) process.exit(1);
      if (!(m.peakRssMb > 0) || !(m.coldBundleMs > 0)) process.exit(1);
      "
  - id: a3.shared-cache-measured
    phase: 2
    description: 'Measurement id "shared-transform-cache" recorded: a second worktree bundling against a transform cache already warmed by the first. Metro keys that cache by content hash, so near-identical variant worktrees should mostly hit it -- this is a wall-clock/CPU lever, and the record must show whether it moves peak RSS at all.'
    command: |-
      node -e "
      const d = require('./docs/goals/metro-memory-profile.json');
      const m = d.measurements.find(x => x.id === 'shared-transform-cache');
      if (!m) process.exit(1);
      if (!(m.coldBundleMs > 0) || !(m.peakRssMb > 0)) process.exit(1);
      "
  - id: a4.conclusions-derived
    phase: 2
    description: 'The artifact states, as numbers derived from the measurements above rather than estimates: the recommended per-instance heap ceiling, the recommended default feature scope, and how many concurrent Metro instances actually fit the VM RAM budget'
    command: |-
      node -e "
      const d = require('./docs/goals/metro-memory-profile.json');
      const c = d.conclusions;
      if (!c) process.exit(1);
      if (!(c.recommendedMaxOldSpaceMb > 0)) process.exit(1);
      if (!c.recommendedFeatureScope) process.exit(1);
      if (!(c.maxConcurrentMetros >= 1)) process.exit(1);
      if (!c.derivedFrom || !Array.isArray(c.derivedFrom) || c.derivedFrom.length < 2) process.exit(1);
      "

  # ---- Phase 3: adopt only what the numbers justify
  - id: i1.feature-scope-selectable
    phase: 3
    description: 'dev-bridge start accepts a feature-scope parameter instead of unconditionally writing every feature into .worktree-features, defaulting to the profiled recommendation. The all-impl behavior stays reachable explicitly.'
    command: |-
      grep -q "features" infra/oracle-cloud/vm-ops-api/server.js \
        && node -e "
        const s = require('fs').readFileSync('infra/oracle-cloud/vm-ops-api/server.js','utf8');
        if (!/DEV_BRIDGE_FEATURE_SCOPE|featureScope|features=/.test(s)) process.exit(1);
        "
  - id: i2.heap-ceiling-matches-profile
    phase: 3
    description: 'Every --max-old-space-size in the dev-bridge scripts equals conclusions.recommendedMaxOldSpaceMb -- the deployed config and the profile cannot drift apart'
    command: |-
      node -e "
      const fs = require('fs');
      const want = require('./docs/goals/metro-memory-profile.json').conclusions.recommendedMaxOldSpaceMb;
      const s = fs.readFileSync('infra/oracle-cloud/vm-ops-api/server.js','utf8');
      const found = [...s.matchAll(/--max-old-space-size=(\d+)/g)].map(m => Number(m[1]));
      if (found.length === 0) process.exit(1);
      if (!found.every(v => v === want)) process.exit(1);
      "
  - id: i3.concurrency-capped
    phase: 3
    description: 'dev-bridge start refuses to launch more concurrent Metro instances than conclusions.maxConcurrentMetros, with a clear error naming the cap -- the guardrail against the OOM that took the whole VM down on 2026-08-27'
    command: |-
      node -e "
      const s = require('fs').readFileSync('infra/oracle-cloud/vm-ops-api/server.js','utf8');
      if (!/MAX_CONCURRENT_METRO|maxConcurrentMetros|concurrency cap/i.test(s)) process.exit(1);
      "
  - id: i4.cap-enforced-live
    phase: 3
    description: 'The cap is proven live, not just present in source: starting one past the cap returns a refusal naming it, and the refusal is a response only that check can produce (not a generic 404/500)'
    command: |-
      node -e "
      const d = require('./docs/goals/metro-memory-profile.json');
      if (!d.capVerifiedLive || !d.capVerifiedLive.refusalBody) process.exit(1);
      if (!d.capVerifiedLive.at) process.exit(1);
      "

  # ---- Phase 4: stop needing N processes to compare N variants of one component
  - id: v1.variant-toggle-pattern
    phase: 4
    description: 'A single-process variant-comparison pattern exists and is applied to the tab-bar overflow case: one Metro, one link, variants switchable at runtime (prop or store-driven) instead of one branch+Metro per variant'
    command: |-
      grep -rq "overflowMode\|variantToggle" libraries/design-system/ui/tabs.tsx \
        && grep -rq "overflowMode\|variantToggle" features/dev-tools-impl/src/
  - id: v2.skill-updated
    phase: 4
    description: 'The metro-concurrent-debugging skill leads with the single-process variant toggle for component-level variants, states the measured concurrency cap as a hard number, and keeps N-Metro fan-out only for genuinely divergent branches'
    command: |-
      node -e "
      const s = require('fs').readFileSync('.agents/skills/metro-concurrent-debugging/SKILL.md','utf8');
      if (!/variant toggle|single-process/i.test(s)) process.exit(1);
      if (!/docs\/goals\/metro-memory-profile\.json|measured cap/i.test(s)) process.exit(1);
      "

  # ---- Phase 5: keep it from regressing
  - id: g1.budget-gate
    phase: 5
    description: 'A convention-lint rule fails when the dev-bridge heap ceiling x concurrency cap would exceed the VM RAM budget recorded in the profile -- so nobody can raise either number back to an OOM configuration without the gate catching it'
    command: 'pnpm run metro-memory-budget:check'
  - id: g2.learning-compounded
    phase: 5
    description: 'The OOM incident and the profiling result are written up in docs/solutions/ -- what the real per-instance cost is, which levers moved it and by how much, and which did not'
    command: |-
      ls docs/solutions/performance-issues/*metro*mem* docs/solutions/performance-issues/*metro*footprint* 2>/dev/null | head -1 | grep -q .
---

# Metro memory footprint — profile it, then cut it

> Superseded 2026-10-21 by `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`: this goal measures per-feature `-impl` and mocks swaps of the feature triad, which that doc retires and the tree no longer has.

## Why this goal exists

On 2026-08-27, three concurrent dev-bridge Metro instances (one per branch,
comparing three variants of a single tab-bar component) took the whole shared
VM down: `vm-ops-api`, `vm-gateway` and `metro-runner` all went unreachable
over the tailnet, the OCI instance stayed `RUNNING` the entire time, a
`SOFTRESET` hung in `STOPPING` long enough that OCI refused further actions
with `409 Conflict`, and recovery took roughly 40 minutes. Nothing was lost,
but the whole comparison run was.

The trigger is known and quoted, not guessed. `DEV_BRIDGE_START_SCRIPT`
deliberately opts every dev-bridge worktree into the **largest possible
bundle** (`infra/oracle-cloud/vm-ops-api/server.js:327-339`):

> The dev-bridge is for active feature development, not bundle-size profiling
> -- so every feature gets its real `-impl` by default here, never the mocks
> stub.

Meanwhile the repo's *default* is the opposite: `@feature/<name>` resolves to
a lightweight mocks stub unless a worktree opts in (`wf feature enable`,
`.worktree-features`, `scripts/setup-worktree.mjs`). So the machinery to
shrink a dev-bridge bundle already exists and is fully built — the dev-bridge
just overrides it to maximum, for every branch, whether or not the branch
touches those features. Each instance then runs with
`NODE_OPTIONS="--max-old-space-size=4096"` (`server.js:465` and `:543`), so
three instances carry a 12 GB combined heap ceiling on a 12 GB VM that is also
running every other always-on service.

## The rule that decides every change here

> **No lever gets adopted before it is measured.** Phase 2 records what each
> one actually saves, independently, against the Phase 1 baseline. A lever
> that does not move the number gets written down as not moving it — that is
> a result, not a failure — and is not shipped anyway.

This exists because the four plausible levers have very different real
payoffs and the intuitions are unreliable:

- **Feature mocks** (`wf feature disable` everything but the branch's own
  feature) — shrinks the module graph itself. Expected to be the dominant
  lever; currently overridden to maximum. Untested.
- **Heap ceiling** (`--max-old-space-size`) — a *ceiling*, not a reservation.
  V8 grows toward it under GC pressure rather than collecting harder, so a
  lower value can cut peak RSS with no correctness change and some wall-clock
  cost. Untested at any value other than 4096.
- **Direct-navigation / route targeting** — expected to be *weak* here:
  Expo Router resolves its full route table in dev regardless of initial
  route, and the bundle URL already carries `lazy=true`, so per-route code is
  already deferred. Worth measuring precisely because the intuition says it
  should help more than it will.
- **Shared transform cache across variant worktrees** — Metro's transform
  cache is content-hash-addressed, and three variant worktrees differ in ~2
  files, so this should be a large *wall-clock* win and roughly no *peak-RSS*
  win. Measure both so the distinction is on record.

## The artifact

`docs/goals/metro-memory-profile.json` is the ledger every phase writes to and
every later criterion reads back:

```json
{
  "vm": { "totalRamGb": 12, "measuredAt": "2026-08-27T00:00:00Z" },
  "measurements": [
    {
      "id": "baseline-all-impl",
      "config": { "features": "all-impl", "maxOldSpaceMb": 4096 },
      "peakRssMb": 0,
      "moduleCount": 0,
      "bundleBytes": 0,
      "coldBundleMs": 0,
      "warmBundleMs": 0,
      "measuredAt": "..."
    }
  ],
  "conclusions": {
    "recommendedMaxOldSpaceMb": 0,
    "recommendedFeatureScope": "",
    "maxConcurrentMetros": 0,
    "derivedFrom": ["baseline-all-impl", "mocks-except-one"]
  },
  "capVerifiedLive": { "at": "...", "refusalBody": "..." }
}
```

`conclusions` is what Phase 3 enforces against the deployed config, so the
profile and `server.js` cannot drift apart (`i2`, `g1`).

## Measurement discipline

- **Peak RSS, not steady-state.** The OOM happened *during* concurrent cold
  bundling; a post-bundle reading understates it. Sample throughout.
- **One variable per measurement.** `heap-reduced` changes only the ceiling
  from the baseline; `mocks-except-one` changes only the feature scope.
- **Cold means cold.** Metro caches transforms on disk; a "cold" number
  measured against a warm cache is not a cold number. Clear the cache, or
  record that you did not and label the number warm.
- **Measure on the real VM, not just locally.** The sandbox has 15 GB and no
  competing services; the VM has 12 GB and runs everything else. A local
  number is a useful sanity check and not the budget.
- **Do not run the measurement fleet concurrently.** Profiling the thing that
  caused an outage should not cause the outage. One instance at a time until
  `a4` says how many fit.

## Recovery, if profiling does take the VM down again

Downtime is free here (per `CLAUDE.md`), so this is about not getting stuck:

1. `oci compute instance list --compartment-id <id>` — confirm the instance
   state before assuming the host died. On 2026-08-27 it read `RUNNING` the
   whole time; the failure was container/network-level.
2. `SOFTRESET` waits on a guest that may be wedged, and while it waits OCI
   returns `409 Conflict` for any further action — including `RESET`. Expect
   to wait it out; ~15 minutes in that state was normal.
3. `/health` answering `{"error":"unauthorized"}` is **recovery**, not
   failure — it means the service is up and the poll simply omitted the
   bearer token.

## How this file is used

Run each criterion's `command` by hand; it is met when it exits 0. (the goal runner
command was removed in the P6 workflow-cli slimming.)

## Working agreements for each tick

- **One criterion per tick.** Take the first unmet one; do not widen scope.
- **Phase order is load-bearing.** Phase 2 numbers decide Phase 3's config
  values. Skipping ahead to "obviously we should lower the heap" is exactly
  the guessing this goal exists to replace.
- **Land it green.** `pnpm --filter mobile-learning-app run arch` before
  committing anything under `frontend/` or `libraries/`.
- **Record the misses.** A lever that measured flat belongs in the artifact
  and in `g2`'s write-up, not silently dropped.
