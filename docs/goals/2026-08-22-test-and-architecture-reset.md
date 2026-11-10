---
id: test-and-architecture-reset
title: Test & architecture reset
status: completed
created: 2026-08-22
branch: claude/clean-arch-refactor-tests-z1strm
criteria:
  # ---- Phase 0: the loop itself (bootstrap; done in the originating session)
  - id: p0.goal-cli
    phase: 0
    description: '`wf goal` is wired into the CLI and can parse this spec'
    command: 'node clis/workflow-cli/bundle/wf.js goal list --json'
  - id: p0.no-vacuous-rule-pass
    phase: 0
    description: 'convention-lint fails loudly on a --rule id that matches no registered rule, instead of running zero rules and exiting 0'
    command: 'pnpm --filter @lib/convention-lint exec vitest run tests/flows/cli-check-flow.test.ts'

  # ---- Phase 1: complexity, maintainability, coupling
  - id: p1.complexity-rules
    phase: 1
    description: 'Root ESLint enforces cognitive-complexity, complexity, max-depth, max-lines-per-function and max-statements'
    command: 'pnpm run complexity-rules:check'
  - id: p1.tests-are-linted
    phase: 1
    description: 'The blanket test-file lint exemption in eslint.config.mjs is gone (tests are governed code)'
    command: 'pnpm run tests-are-linted:check'
  - id: p1.metrics-cli
    phase: 1
    description: '`wf metrics mi|coupling|hotspots` reports Maintainability Index, Ca/Ce instability and churn x complexity'
    command: 'node clis/workflow-cli/bundle/wf.js metrics mi --json'
  - id: p1.metrics-ratchet
    phase: 1
    description: 'root.metrics-ratchet gate passes: no package regressed below its recorded MI / violation baseline'
    command: 'pnpm run metrics-ratchet:check'

  # ---- Phase 2: tier gates covering every package, no exceptions
  - id: p2.integration-tier-repo-wide
    phase: 2
    description: 'require-integration-tier scans every workspace package (waivers must carry real prose), not a 2-entry opt-in list'
    command: |-
      node --input-type=module -e "
      import cfg from './conventions.config.mjs';
      const rule = cfg.rules.find((r) => r.id === 'root.require-integration-tier');
      const pkgs = rule?.config?.packages;
      if (Array.isArray(pkgs) && pkgs.length <= 2) process.exit(1);
      process.exit(0);
      " && pnpm run integration-tier:check
  - id: p2.runbook-per-package
    phase: 2
    description: 'Every workspace package has a runbook with valid frontmatter, or an explicit waiver'
    command: 'pnpm run runbook-per-package:check'
  - id: p2.runbook-frontmatter-baseline-empty
    phase: 2
    description: 'No runbook is grandfathered any more — runbook-frontmatter-baseline.json is empty'
    command: 'pnpm run runbook-baseline-empty:check'
  - id: p2.runbook-freshness
    phase: 2
    description: 'Every runbook has been executed within its declared maxAgeDays, against an ancestor of HEAD'
    command: 'pnpm run runbook-freshness:check'
  - id: p2.health-tiers-derived
    phase: 2
    description: 'report/health.ts derives test tiers from each package rather than a hardcoded list'
    command: 'pnpm run health-tiers-derived:check'

  # ---- Phase 3: the agent-executable runbook runner
  - id: p3.runbook-cli
    phase: 3
    description: '`wf runbook list|show|run` exists; --agent drives the headless SDK, --record writes a result record'
    command: 'node clis/workflow-cli/bundle/wf.js runbook list --json'
  - id: p3.runbook-evidence-required
    phase: 3
    description: 'The runner rejects a UI-affecting step that returns no screenshot evidence'
    command: 'pnpm --filter @cli/workflow-cli exec vitest run tests/unit/runbook'

  # ---- Phase 4: the cull
  - id: p4.test-smell-rules
    phase: 4
    description: 'The five test-smell rules exist in @lib/eslint-plugin-lint-contentions and are unit-tested'
    command: |-
      for r in no-own-module-mock no-mock-assertion-only no-implementation-snapshot no-prop-walking require-behavioral-assertion; do
        grep -q "'$r'" libraries/eslint-plugin-lint-contentions/src/index.ts || exit 1
      done
      pnpm --filter @lib/eslint-plugin-lint-contentions run test
  - id: p4.test-smell-rules-clean
    phase: 4
    description: 'The repo is clean against those rules at error level (no own-module mocks, no mock-only assertions, no prop-walking)'
    command: 'pnpm run test-smells:check'
  - id: p4.test-tier-matches-layer
    phase: 4
    description: 'No tests/unit/ file has a subject that imports from data/ or di/ — those belong to integration or runbook tier'
    command: 'pnpm run test-tier-layer:check'
  - id: p4.unit-test-budget
    phase: 4
    description: 'Every package is within its unit-test LOC budget, and no package regressed against the ratchet'
    command: 'pnpm run unit-test-budget:check'

  # ---- Phase 5: the whole thing actually holds
  - id: p5.conventions-green
    phase: 5
    description: 'Every convention-lint rule passes repo-wide'
    command: 'pnpm run conventions:check'
  - id: p5.arch-green
    phase: 5
    description: 'The app architecture gate passes end to end'
    command: 'pnpm --filter mobile-learning-app run arch'
---

# Test & architecture reset

> Note 2026-10-21: the rule below that a subject's layer decides its test tier ("The rule that decides every deletion") is retired; which test type covers which ring is in `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.

## Why this goal exists

The test suite has inverted the priority order `CLAUDE.md` declares. Measured
at the point this goal was written:

| Tier | Files | Share |
|---|---|---|
| unit-ish | 388 | 77% |
| integration | 59 | 12% |
| e2e | 45 | 9% |
| contract | 9 | 2% |

501 test files, 63,476 test LOC against 147,043 source LOC — and **nothing
above the unit tier gates a merge**. At the same time there is no complexity or
maintainability metric anywhere in the repo, so "well-maintained code" is
unenforced opinion.

The nine gaps this goal closes:

1. Zero complexity metrics — no cognitive complexity, `max-depth`,
   `max-lines-per-function`, `max-statements`, or `complexity` anywhere.
2. Tests are the least-governed code in the repo: `eslint.config.mjs` carries
   `files: ['**/*.test.{ts,tsx}'], rules: {}`, plus a block disabling every
   type-aware rule under `**/tests/**`. Lint does not look where the bloat is.
3. `root.require-integration-tier` gates 2 packages of ~54.
4. `root.require-runbook-frontmatter` has 18 of 20 runbooks grandfathered.
5. No runbook or e2e gate in CI at all — `architecture.yml` stops at unit tests.
6. Runbooks are inert prose, with a hardcoded ~15-entry per-package tier list in
   `report/health.ts` that silently omits every other package.
7. Coupling/instability unmeasured; dependency-cruiser covers 5 packages.
8. The one anti-useless-test gate is a regex over two directories in one package.
9. Canonical specimen of the grievance:
   `features/dev-tools-impl/src/ui/__tests__/OverlayTab.test.ts` spends ~40
   lines on `vi.mock('react-native', …)` before asserting anything. It
   re-implements the framework: it breaks when the implementation changes and
   stays green when the product is broken.

## The rule that decides every deletion

Apply this identically on every tick — clean architecture is what makes it
mechanical, because the layer a subject lives in tells you which tier its test
belongs to:

> **Keep a unit test only if its subject is a `use-cases/` or `contracts/`-level
> pure function with real branching, or a genuine algorithm, parser, or regex.
> Everything testing `data/` gateways, `di/` wiring, or `ui/` rendering is
> deleted, or promoted to the integration or runbook tier.**

Volume is capped by `root.unit-test-budget`: per package,
`unitTestLoc / srcLoc` against a shrink-only baseline, target ≤ 0.25.
Integration, e2e and runbook LOC are **excluded from the numerator** on
purpose — the budget must push work *up* the tiers, never merely delete
coverage.

## How this file is used

Everything above the `---` is the machine contract. Each criterion's `command`
runs from the repo root and **must exit 0**; prose progress is not progress.
That is the direct application of
`docs/solutions/workflow/verify-at-the-sink-not-at-the-source.md` — the agent
that just fixed something is the worst judge of whether the job is done.

```bash
wf goal status          # every criterion, met/unmet, with failure output
wf goal next            # the first unmet criterion — what to work on now
wf goal verify          # exit 0 only when the whole goal is met
wf goal list --json     # the criteria themselves, for tooling
```

### Driving it with native `/goal`

Claude Code ships `/goal` (https://code.claude.com/docs/en/goal): it re-runs a
turn until a small evaluator model judges a condition met. Use it directly —
there is deliberately no custom loop in this repo:

```
/goal `wf goal verify` exits 0 with every criterion reported met. Work the
first unmet criterion from `wf goal next`, one per turn, and paste the real
command output each turn. Do not edit this goal file's criteria.
```

**Why the condition names a command.** The `/goal` evaluator "does not run
commands or read files independently, so write the condition as something
Claude's own output can demonstrate" — it only reads the transcript. A prose
condition therefore gets judged on an agent's self-report, which is exactly the
premature-satisfaction failure `verify-at-the-sink-not-at-the-source.md`
documents. Naming `wf goal verify` forces a deterministic exit code into the
transcript for the evaluator to read instead.

If `/goal` is unavailable (some managed/remote builds omit it), the loop still
works by hand or under `/loop`: `wf goal next` → do that one thing →
`wf goal verify`.

Run it as a loop with `/loop /goal`. `wf goal verify` is the terminating
condition, not an agent's opinion that it is finished.

## Working agreements for each tick

- **One criterion per tick.** Take the first unmet one; do not widen scope.
- **Land it green.** Run the local gates before committing — lefthook pre-commit
  is the only automatic gate since 2026-09-03; nothing runs in GitHub Actions on
  push or PR.
- **Never weaken a criterion to pass it.** Editing this file's `command` to
  make it easier is the one prohibited move. Add criteria freely; only the
  human relaxes one.
- **Waivers need real prose.** Every waiver carries a `why…` field explaining
  which alternative was considered and why it does not fit, matching
  `scripts-waivers.json`'s enforced `minReasoningChars`.
- **Report the tier that actually ran.** "The unit tests pass" is not
  verification of anything above the unit tier.
