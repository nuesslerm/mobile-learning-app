---
id: tamagui-second-app
title: Second Expo app on Tamagui with leetcode parity
status: superseded
created: 2026-08-27
branch: claude/design-system-modernization-be74w3
criteria:
  # ---- Phase 1 (#209): fix wf browser. Unblocks all visual verification.
  - id: i1.slot-autovivifies
    phase: 1
    description: '#209 — setBrowserMcpService creates the slot when absent instead of discarding the write (slots.ts:292-298), mirroring setService at :268-275. tests/unit/slots.test.ts:86-90 currently asserts the broken no-op and must assert creation instead.'
    command: |-
      pnpm --filter @lib/registry-service run build >/dev/null 2>&1 \
        && node --input-type=module -e "
        import { setBrowserMcpService, getBrowserMcpService, slotRemove } from './libraries/registry-service/dist/index.js';
        const slot = 'goalcheck/' + Date.now() + '/default';
        setBrowserMcpService(slot, { url: 'http://127.0.0.1:1', startedAt: new Date().toISOString() });
        const created = getBrowserMcpService(slot) !== null;
        slotRemove(slot);
        if (!created) process.exit(1);
        " \
        && pnpm --filter @lib/registry-service run test >/dev/null 2>&1
  - id: i1.failure-is-legible
    phase: 1
    description: '#209 — the daemon is no longer spawned with stdio ignored, and the timeout names the resolved slot and registry path. This bug cost hours purely because it failed silently.'
    command: |-
      ! grep -q "stdio: 'ignore'" clis/multi-browser/src/entrypoints/cli.ts \
        && grep -q "slot" clis/multi-browser/src/entrypoints/cli.ts
  - id: i1.browser-works-clean
    phase: 1
    description: '#209 — wf browser starts and navigates from a clean registry, with no wf ship mla-tamagui needed first'
    command: |-
      wf browser start >/dev/null 2>&1 \
        && wf browser start-device --device goalcheck >/dev/null 2>&1 \
        && wf browser open-link https://example.com --device goalcheck >/dev/null 2>&1

  # ---- Phase 2 (#210): frontend app registry
  - id: i2.registry-exists
    phase: 2
    description: '#210 — a typed registry modeled on ci/apps.ts carries root/nxProject/slug/logTag/ghPagesBasePath per app, killing the hardcodes in paths.ts, ios/launch.ts:463, android/launch.ts:167, logs/tail.ts:229'
    command: |-
      test -f clis/workflow-cli/src/lib/frontendApps.ts \
        && ! grep -q "mobile-learning-app" clis/workflow-cli/src/lib/paths.ts \
        && ! grep -q "'expo-hello-world-app-2'" clis/workflow-cli/src/use-cases/ios/launch.ts \
        && ! grep -q "'expo-hello-world-app-2'" clis/workflow-cli/src/use-cases/android/launch.ts
  - id: i2.registry-cannot-drift
    phase: 2
    description: '#210 — a convention-lint rule fails when the registry disagrees with frontend/*/app.config.js on disk, mirroring the existing vm-app-placement-sync guard. The /expo-hello-world-app-2 base path is currently duplicated in four places.'
    command: 'node clis/convention-lint/dist/entrypoints/cli.js check --rule root.frontend-app-registry-sync'
  - id: i2.app1-still-green
    phase: 2
    description: '#210 — regression spine: app #1 unaffected by the generalization'
    command: 'pnpm --filter mobile-learning-app run arch >/dev/null 2>&1'

  # ---- Phase 3 (#211): app-scoped slots
  - id: i3.slots-are-app-scoped
    phase: 3
    description: '#211 — two apps on one branch no longer share a Metro port, browser daemon, simulator claim or container name. Without this the second wf ship mla-tamagui kills the first app Metro via killByPort.'
    command: |-
      pnpm --filter @lib/registry-service run test >/dev/null 2>&1 \
        && node --input-type=module -e "
        import { resolveSlot } from './libraries/registry-service/dist/index.js';
        const git = { currentBranch: () => 'goal-check' };
        const a = resolveSlot(undefined, git, 'mobile-learning-app');
        const b = resolveSlot(undefined, git, 'mla-tamagui');
        if (a === b) process.exit(1);
        if (!a.includes('mobile-learning-app') || !b.includes('mla-tamagui')) process.exit(1);
        if (resolveSlot(a, git, 'mla-tamagui') !== a) process.exit(1);
        "
  - id: i3.wf-targets-either-app
    phase: 3
    description: '#211 — wf resolves either app without collision'
    command: |-
      wf ship mla-tamagui --app mobile-learning-app >/dev/null 2>&1 \
        && wf ship mla-tamagui --app mla-tamagui >/dev/null 2>&1

  # ---- Phase 4 (#212): shared build presets
  - id: i4.presets-extracted
    phase: 4
    description: '#212 — Tailwind and Babel presets live in a shared package, and the 14 packages that reverse-point at ../../frontend/mobile-learning-app/tailwind.config.ts now consume it'
    command: |-
      test -d tooling/tailwind-config \
        && ! grep -rq "frontend/mobile-learning-app/tailwind.config" features libraries --include=eslint.config.mjs
  - id: i4.app1-still-green
    phase: 4
    description: '#212 — regression spine'
    command: 'pnpm --filter mobile-learning-app run arch >/dev/null 2>&1'

  # ---- Phase 5 (#213): scaffold + prove the stack. The real unknown.
  - id: i5.scaffold-exists
    phase: 5
    description: '#213 — frontend/mla-tamagui is an Expo Router app with tamagui + its babel and metro plugins, and no nativewind/tailwind'
    command: |-
      node -e "
      const p = require('./frontend/mla-tamagui/package.json');
      const d = { ...p.dependencies, ...p.devDependencies };
      if (!d.tamagui || !d['@tamagui/babel-plugin'] || !d['@tamagui/metro-plugin']) process.exit(1);
      if (d.nativewind || d.tailwindcss || d['react-native-css-interop']) process.exit(1);
      if (!d['expo-router']) process.exit(1);
      "
  - id: i5.web-and-native-bundles-build
    phase: 5
    description: '#213 — exports for BOTH web and ios. Web alone would hide the .mjs-vs-.native.js Metro condition-resolution bug this stack is known for, which is exactly the difference between browser and Expo Go.'
    command: |-
      pnpm --filter mla-tamagui run export:web >/dev/null 2>&1 \
        && pnpm --filter mla-tamagui run export:ios >/dev/null 2>&1
  - id: i5.tamagui-renders
    phase: 5
    description: '#213 — a Tamagui component actually renders, not just bundles. Driven through the multi-browser SDK, asserting on a real element.'
    command: 'pnpm --filter mla-tamagui run verify:renders >/dev/null 2>&1'

  # ---- Phase 6 (#214): the design system
  - id: i6.exports-required-surface
    phase: 6
    description: '#214 — exports the primitives the leetcode feature consumes (26 entry points, ~30 symbols in app #1)'
    command: |-
      node -e "
      const req = ['View','Text','Button','IconButton','Icon','Badge','ListItem','Separator','TextField','Sheet','AlertDialog','PromptDialog'];
      const src = require('fs').readFileSync('./libraries/tamagui-design-system/index.ts','utf8');
      for (const s of req) if (!new RegExp('\\\\b'+s+'\\\\b').test(src)) { console.error('missing: '+s); process.exit(1); }
      "
  - id: i6.is-genuinely-tamagui
    phase: 6
    description: '#214 — no className plumbing, nativewind or tailwind-merge leaked in'
    command: '! grep -rqE "nativewind|tailwind-merge|className=" frontend/libraries/tamagui-design-system --include=*.ts --include=*.tsx'

  # ---- Phase 7 (#215): shell + logging + debug
  - id: i7.logging-wired
    phase: 7
    description: '#215 — the logger import is literally the first import in the root layout (without it every Logger.* call, in the app AND in the design system, silently no-ops), the host adapter actually calls configureLogger, and the babel preset is configured for call-site injection'
    command: |-
      [ "$(grep -m1 '^import' frontend/mla-tamagui/app/_layout.tsx)" = "import '@/lib/logger';" ] \
        && grep -q "configureLogger" frontend/mla-tamagui/lib/logger.ts \
        && grep -q "@lib/logging/babel-preset" frontend/mla-tamagui/babel.config.js
  - id: i7.debug-fab-sheet-loglist
    phase: 7
    description: '#215 — a debug FAB opens a Tamagui Sheet with a live log list. Deliberately simple: not a port of the 252-line draggable FAB with its build-info chip, nor of the ListItem-card-per-log-line rendering.'
    command: 'pnpm --filter mla-tamagui run export:web >/dev/null 2>&1 && pnpm --filter mla-tamagui run verify:debug-sheet'

  # ---- Phase 8 (#216): the port
  - id: i8.routes-present
    phase: 8
    description: '#216 — every leetcode route exists, including overlays-as-routes nested under the dynamic segment'
    command: |-
      for r in "app/(app)/(content)/index.tsx" "app/leetcode/[id].tsx" "app/leetcode/login.tsx" \
               "app/leetcode/toolbar.tsx" "app/leetcode/[id]/description.tsx" \
               "app/leetcode/[id]/rename.tsx" "app/leetcode/[id]/reset-code.tsx"; do
        test -f "frontend/mla-tamagui/$r" || exit 1
      done
      test -f frontend/mla-tamagui/features/leetcode/index.ts
  - id: i8.feature-flow-works
    phase: 8
    description: '#216 — the real flow runs in the browser: the catalog renders 72 rows, a row navigates to the editor, the CodeMirror WebView loads and accepts typed input, and the Info button opens the description overlay. The editor needs the app-local react-native-webview web shim plus the matching metro resolveRequest hook; neither comes for free, and a bundle that builds says nothing about whether the iframe came up.'
    command: 'pnpm --filter mla-tamagui run export:web >/dev/null 2>&1 && pnpm --filter mla-tamagui run verify:leetcode'
  - id: i8.dead-code-not-ported
    phase: 8
    description: '#216 — the dead weight stayed behind: app #1s V1 ProblemEditorScreen (755 lines, superseded by V2, no route imports it), WordBank/ (656, absent from the barrel), and the draggable VoiceCommandButton with its useVoiceCommand hook and voiceFabStore (nothing references them). The ported screen is V2, renamed.'
    command: |-
      ! test -d frontend/mla-tamagui/features/leetcode/ui/WordBank \
        && ! test -e frontend/mla-tamagui/features/leetcode/ui/ProblemEditorScreenV2.tsx \
        && ! test -e frontend/mla-tamagui/features/leetcode/ui/VoiceCommandButton.tsx \
        && ! test -e frontend/mla-tamagui/features/leetcode/state/voiceFabStore.ts
  - id: i8.no-classname-survived
    phase: 8
    description: '#216 — no className survived the port. A dropped className renders unstyled rather than erroring, so this is checked rather than eyeballed.'
    command: '! grep -rqE "className=" frontend/mla-tamagui --include=*.tsx'

  # ---- Phase 9 (#217): acceptance
  - id: i9.e2e-browser-passes
    phase: 9
    description: '#217 — an e2e spec drives the real flow in the browser: list, open a problem, type in the editor'
    command: 'pnpm --filter mla-tamagui run test:e2e >/dev/null 2>&1'
  - id: i9.runbook-records-expo-go
    phase: 9
    description: '#217 — a runbook records the manual Expo Go pass with device screenshots, and an automated spec follows the same steps. Expo Go cannot be asserted from CI: the runbook is the record, the spec is the regression net.'
    command: |-
      test -f docs/runbooks/mla-tamagui-sanity.md \
        && grep -q "Expo Go" docs/runbooks/mla-tamagui-sanity.md \
        && pnpm --filter mla-tamagui run test:runbook >/dev/null 2>&1
  - id: i9.app1-untouched
    phase: 9
    description: '#217 — the point of a side-by-side: app #1 and @lib/design-system unchanged apart from the tooling generalization in phases 2-4'
    command: 'git diff --quiet origin/main -- libraries/design-system frontend/mobile-learning-app/app'
---

# Second Expo app on Tamagui with leetcode parity

> Superseded 2026-10-21 by `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`: its plan relies on the feature triad's name-shadowing machinery ("for the feature triad"), which that doc retires and the tree no longer has.

Rather than revamping the existing design system in place, build a **side-by-side comparison**: a
second Expo app on a second design system, reaching parity on the whole leetcode feature. App #1
and `@lib/design-system` stay untouched throughout. Tooling gets generalized so `wf` targets
either app.

**One GitHub issue = one phase = one stacked PR** — #209 through #217, each PR targeting the
previous one's branch.

## Scope

Whole leetcode feature, code editor included. Thin shell (Expo Router, drawer, Tamagui provider,
fonts, safe area) **plus** `@lib/logging` and a basic debug FAB → sheet → log list.
`frontend/libraries/tamagui-design-system`. The more native it looks, the better. **Acceptance is exactly
two things: it works in the browser, and it works in Expo Go.**

**No testID enforcement** — the `require-testid-on-*` / `data-testid-format` /
`testids:extract|check` machinery does not apply to app #2. Runbook and e2e tests still do.
Elements the e2e actually drives still get hand-written *semantic* testIDs
(`LeetcodeListSignIn`), just not lint-enforced, and not the generated positional kind
(`FeaturesLeetcodeUi-ProblemListScreen-Text-181`) which a rewrite invalidates anyway.

## What the research settled

- **Tamagui 2.7.7 is stable.** The much-cited Feb 2026 "SDK 54 + Tamagui V2" thread ran on
  `2.0.0-rc.0`; that risk is stale.
- Tamagui core needs **no native module** — Babel + Metro plugin only, so **Expo Go works**.
- The `@tamagui/toast` → `burnt` Expo Go blocker is **fixed upstream**; 2.7.7 uses
  `@tamagui/native`.
- Tamagui ships `sheet`, `list-item`, `tabs`, `dialog`, `select`, `card`, `button`, `input`,
  `progress`, `separator`, `switch`, `popover`, `spinner` — the leetcode surface is covered.
- `react`/`react-dom` are already pinned to 19.1.0 and `shamefully-hoist=true` is already set,
  pre-solving the two pnpm pain points that thread flags.
- `frontend/*` is already a workspace glob and the eslint scope boundaries match `frontend/*` — a
  second app needs zero changes to either.
- **The code editor is the lowest-risk part, not the highest.** CodeMirror 6 is sealed inside a
  WebView with one DS import and two classNames, and its colors arrive as hex strings via
  postMessage rather than DS tokens.

## The genuine unknown — why phase 5 gates the app work

The one published config verifying this stack ran **without the expo-router plugin** and used
`AppRegistry` rather than `registerRootComponent`. This repo is entirely Expo Router, so
*Tamagui V2 + Expo Router on SDK 54 in a pnpm monorepo* is demonstrated nowhere found in
research. That thread also hits a Metro condition-resolution bug (`.mjs` instead of
`.native.js`), fixed with a `resolveRequest` hook.

If phase 5 fails, stop and reconsider rather than building four more phases on top.

## Why not "shadow the package name"

A tempting shortcut is a `libraries/design-system-tamagui` publishing the name
`@lib/design-system` — the repo already has that name-shadowing machinery for the feature triad —
so feature source stays byte-identical. **It does not hold.** Those files carry 149 `className=`
props, which Tamagui components do not accept; swapping the package name would silently drop
every one and render unstyled. The `src/ui/` rewrite is unavoidable either way.

## Effort split

Hand-written lines, excluding a 7,567-line JSON catalog: **~3,468 (39%) port as-is** ·
**~1,622 (18%) mechanical** (including 149 className conversions) · **~2,354 (27%) real UI
rewrite** · **~1,411 (16%) do not port at all** — `ProblemEditorScreen` (755, dead: no route
imports it) and `WordBank/` (656, absent from the barrel).

## Not a criterion: "looks native"

It is not machine-checkable, and a criterion that cannot fail honestly is worse than none. That
is the runbook's job — screenshots shown, not claimed.
