---
id: coverage-debt-and-browser-consolidation
title: Coverage debt and browser consolidation
status: superseded
created: 2026-08-22
branch: claude/coverage-debt-browser-consolidation-loop2
criteria:
  # ---- Measurement: close the tests/unit-only blind spot before rebalancing
  - id: m1.tests-dir-blindspot-fixed
    phase: 1
    description: 'unit-test-budget and test-tier-matches-layer scan __tests__/ in addition to tests/unit/, and their baselines already account for the newly-visible files'
    command: |-
      grep -q "__tests__" clis/workflow-cli/src/use-cases/metrics/unitTestBudget.ts \
        && grep -q "__tests__" clis/convention-lint/rules/test-tier-matches-layer.ts \
        && pnpm run unit-test-budget:check \
        && pnpm run test-tier-layer:check

  # ---- Per-category coverage ceilings
  - id: c1.category-ceilings-defined
    phase: 2
    description: 'Every workspace package has a declared coverage-ceiling category (ui-feature | backend-service | cli-tool | eslint-plugin | pure-logic), and a gate enforces it instead of a uniform integration+e2e target'
    command: |-
      node -e "
      const d = require('./package-categories.json');
      const n = Object.keys(d.categories || {}).length;
      if (n < 50) process.exit(1);
      " && pnpm run category-ceiling:check

  # ---- Runbook execution
  - id: r1.runbook-freshness-baseline-empty
    phase: 3
    description: 'Every agent-driven runbook has a dated, web-leg result record -- runbook-freshness-baseline.json is empty'
    command: |-
      node -e "
      const d = require('./docs/runbooks/runbook-freshness-baseline.json');
      if ((d.baseline?.paths ?? []).length > 0) process.exit(1);
      "
  - id: r2.web-only-policy-documented
    phase: 3
    description: 'CLAUDE.md documents that a web-only, SDK-executed runbook record is permanently sufficient verification for platform-agnostic behavior, not only encoded as gate logic'
    command: 'grep -q "permanent verification standard" CLAUDE.md'

  # ---- Browser automation consolidation
  - id: b1.desktop-preset-named
    phase: 4
    description: '@cli/multi-browser DEVICE_REGISTRY has a named "desktop" preset id for the bare-viewport, no-bezel mode'
    command: "grep -q \"id: 'desktop'\" clis/multi-browser/harness-shell/contracts/src/index.ts"
  - id: b2.playwright-ban-rule-clean
    phase: 4
    description: 'A convention-lint rule bans raw playwright browser launches for app-testing outside clis/multi-browser and libraries/playwright-mcp, with the reasoned #201 exception, and the repo is clean against it'
    command: 'pnpm run no-raw-playwright:check'
  - id: b3.interactive-specs-migrated
    phase: 4
    description: 'The 7 remaining interactive e2e specs drive their browser sessions through the multi-browser SDK, matching app-shell.spec.ts'
    command: |-
      for f in agentation live-backends material-tiers mobile-logging runbook-agent-sanity theme-switching; do
        grep -q "@cli/multi-browser" "frontend/mobile-learning-app/e2e/web/$f.spec.ts" || exit 1
      done
      grep -q "@cli/multi-browser" frontend/mobile-learning-app/e2e/voice-assist.spec.ts
  - id: b4.golden-specs-still-sdk-driven
    phase: 4
    description: 'The 2 golden specs still drive their sessions through the multi-browser SDK (no regression to a raw launch) as the other specs migrate around them'
    command: |-
      for f in frontend/mobile-learning-app/e2e/golden/screens.golden.spec.ts frontend/mobile-learning-app/e2e/golden/storybook-stories.golden.spec.ts; do
        grep -q "@cli/multi-browser" "$f" || exit 1
        ! grep -qE "chromium\.launch|browserType\.launch" "$f" || exit 1
      done
  - id: b5.vnc-auth-capture-migrated
    phase: 4
    description: "clis/vnc-auth-capture's OAuth-callback e2e tests are migrated off raw playwright onto the multi-browser SDK's desktop mode"
    command: |-
      for f in clis/vnc-auth-capture/tests/e2e/codexOAuthCallback.e2e.test.ts clis/vnc-auth-capture/tests/e2e/claudeCredentialRelay.e2e.test.ts; do
        grep -q "@cli/multi-browser" "$f" || exit 1
        ! grep -qE "from 'playwright'|from \"playwright\"" "$f" || exit 1
      done

  # ---- Unit-test rebalancing: full elimination
  - id: u1.unit-test-budget-baseline-empty
    phase: 5
    description: 'unit-test-budget-baseline.json is empty or deleted -- every package (the original 14 plus whatever m1 surfaced) is back under budget'
    command: |-
      node -e "
      const fs = require('node:fs');
      if (!fs.existsSync('unit-test-budget-baseline.json')) process.exit(0);
      const d = require('./unit-test-budget-baseline.json');
      if (Object.keys(d.packages ?? {}).length > 0) process.exit(1);
      "
  - id: u2.test-tier-layer-baseline-empty
    phase: 5
    description: 'test-tier-layer-baseline.json is empty or deleted -- every mistiered tests/unit/ file (the original 61 plus whatever m1 surfaced) is resolved'
    command: |-
      node -e "
      const fs = require('node:fs');
      if (!fs.existsSync('test-tier-layer-baseline.json')) process.exit(0);
      const d = require('./test-tier-layer-baseline.json');
      if ((d.baseline?.paths ?? []).length > 0) process.exit(1);
      "
  - id: u3.test-smells-baseline-empty
    phase: 5
    description: 'test-smells-baseline.json is empty or deleted -- every test-smell violation (the original 71 plus whatever m1 surfaced) is resolved'
    command: |-
      node -e "
      const fs = require('node:fs');
      if (!fs.existsSync('test-smells-baseline.json')) process.exit(0);
      const d = require('./test-smells-baseline.json');
      if ((d.baseline?.paths ?? []).length > 0) process.exit(1);
      "

  # ---- The whole thing actually holds
  - id: f1.conventions-green
    phase: 6
    description: 'Every convention-lint rule passes repo-wide'
    command: 'pnpm run conventions:check'
  - id: f2.arch-green
    phase: 6
    description: 'The app architecture gate passes end to end'
    command: 'pnpm --filter mobile-learning-app run arch'
---

# Coverage debt and browser consolidation

> Superseded 2026-10-21 by `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`: m1 greps the deleted `clis/convention-lint` and `clis/workflow-cli` and runs `pnpm run test-tier-layer:check`, which no longer exists, and c1's five package categories are replaced by that doc's 16 package types.

## Why this goal exists

`docs/goals/2026-08-22-test-and-architecture-reset.md` (PR #200) built real
gates but landed them with shrink-only baselines rather than a clean ledger,
because closing all of it in one loop would have been unreviewable. Verifying
that reset's own report (published as an artifact the same session) surfaced
two more things the report itself couldn't have counted: the
`unit-test-budget` and `test-tier-matches-layer` gates only scan `tests/unit/`,
so ~29 co-located `__tests__/` files — including the report's own cited
"canonical specimen of the grievance",
`features/dev-tools-impl/src/ui/__tests__/OverlayTab.test.ts` — are invisible
to both gates; and the repo's browser-testing surface is split across three
uncoordinated paths (the multi-browser CLI/SDK, 10 raw-Playwright specs under
`e2e/`, and 3 more files using bare `playwright` outside app-testing).

This goal closes both: the full debt ledger from the first reset (14
over-budget packages, 71 test-smell files, 61 mistiered tests, 16 runbooks —
plus whatever the `__tests__/` blind-spot fix newly surfaces), and
consolidates all app-testing browser automation onto the multi-browser
CLI/SDK.

Full context, the design rationale, and every decision behind the criteria
above: `docs/superpowers/plans/2026-08-22-001-refactor-coverage-debt-and-browser-consolidation-plan.md`.

## The rule that decides every deletion (unchanged from the first reset)

> Keep a unit test only if its subject is a `use-cases/` or `contracts/`-level
> pure function with real branching, or a genuine algorithm, parser, or regex.
> Everything testing `data/` gateways, `di/` wiring, or `ui/` rendering is
> deleted, or promoted to that package's category ceiling (not automatically
> to e2e when the category's ceiling is lower — see `c1`).

## Full elimination, unconditionally

Unlike the first reset, this loop's target is every currently-baselined debt
item resolved and every baseline file emptied or deleted — including
whatever `m1` newly surfaces from the `__tests__/` blind-spot fix. This is a
deliberate, explicit choice, made with full knowledge that the true baseline
size is not yet measured: **do not re-freeze newly-surfaced debt into a new
baseline as a scope-reduction move.** If `m1` reveals substantially more debt
than expected, that is more work for `u1`-`u3`, not a reason to lower the
bar.

## How this file is used

Run each criterion's `command` by hand; it is met when it exits 0. (the goal runner
command was removed in the P6 workflow-cli slimming.)

## Working agreements for each tick

- **One criterion per tick.** Take the first unmet one; do not widen scope.
- **Land it green.** Run the local gates before committing.
- **Never weaken a criterion to pass it.** Editing this file's `command` to
  make it easier is the one prohibited move. Add criteria freely; only the
  human relaxes one.
- **Report the tier that actually ran.** Runbook/e2e evidence via the
  multi-browser CLI/SDK (device-frame + fake-keyboard, or bare `desktop` mode
  for golden/non-app automation) counts as fully sufficient — no native leg is
  required. See the plan doc's Key Decisions for the exact scoping (native
  checks remain available for genuinely platform-specific runbooks).
- **Push after every criterion** to this branch
  (`claude/coverage-debt-browser-consolidation-loop2`) — this PR grows one
  commit per criterion, same as the first reset.
