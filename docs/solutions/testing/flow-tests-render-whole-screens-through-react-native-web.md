---
title: Flow tests render whole Tamagui screens through react-native-web in jsdom — the harness owns the native stubs
date: 2026-09-06
category: testing
tags: [testing, flow-test, react-native-web, jsdom, tamagui, vitest, testing-library, harness, stubs]
problem_type: knowledge
module: frontend/libraries/flow-test-harness
---

## Context

A store unit test had been reported as a "flow test". A flow test renders the real screen and drives it the way a user does (tap, type, wait, read), with only the process boundary replaced by a fixture. Nothing in the repo could do that for the Tamagui screens: the `react-native` stub the unit configs alias in has no `Touchable`/`Animated` surface (Tamagui's native build fails with `Cannot destructure property 'Mixin' of '_reactNative.Touchable'`), and `@testing-library/react-native` pulled React Native's Flow-syntax entry (`Unexpected token 'typeof'`) because the hoisted copy resolved its peer to the real package.

`frontend/libraries/flow-test-harness` (`@lib/flow-test-harness`) exists so every ui-feature package gets the same answer: `flowTestConfig()` for vitest and `renderScreen()` for the test. The first consumer, `features/coding-agent-impl`, runs 23 scenarios over three screens through `pnpm --filter mla-tamagui run test:flow`, and the pre-commit `test-affected` task runs `test:flow` next to `test:unit`.

## What did not work

- **Richer react-native stub under `@testing-library/react-native`.** Tamagui's native path reaches deep into React Native (`Touchable.Mixin`, `Animated`, `PixelRatio`); a stub would chase internals forever.
- **Aliasing `react-native` → `react-native-web` for inlined deps only.** An externalised dependency loads through Node, where the alias does not apply, and the Flow entry comes back. `server.deps.inline: true` is required.
- **Babel over everything.** The logging babel preset transformed `node_modules` `.jsx` files it cannot parse (`@gluestack-ui/core`); the plugin is filtered to project sources.
- **One native package at a time.** `react-native-reanimated`, `@gorhom/bottom-sheet`, `react-native-safe-area-context`, `react-native-markdown-display` and the per-icon `lucide-react-native/dist/esm/icons/*.mjs` imports each broke the import chain in turn; each now has a stub in `src/stubs`.
- **CommonJS stubs shaped as `{ __esModule, default }` or as a Proxy.** Under vite-node's interop a default import can receive the whole module object (`Element type is invalid … got: object`), and named imports are built by enumerating own properties, which a Proxy without `ownKeys` does not expose (`… got: undefined`). Answering React's static-property probes (`childContextTypes`, `getDerivedStateFromProps`) with a component produced legacy-context warnings on every render.

## Solution

- `flowTestConfig({ packageDir })`: jsdom, `react-native` → `react-native-web`, `TAMAGUI_TARGET=web`, `server.deps.inline: true`, the stub aliases from `flowTestAliases()`, a setup file with the `matchMedia`/`ResizeObserver` polyfills, `IS_REACT_ACT_ENVIRONMENT`, RTL cleanup and harness resets.
- `renderScreen(ui, { theme })` wraps the screen in the app's own `TamaguiProvider` + `PortalProvider`; queries come from `@testing-library/react` (`testID` → `data-testid`), presses from `userEvent`.
- `routerHarness()` seeds `useLocalSearchParams` and records `router.push`; `storageHarness()` seeds the async-storage map. Both read the globals the `expo-router` and async-storage stubs write to.
- Stub shape rule: the module object **is** the default export (a component or the API object) with named exports as **own properties** (`Object.assign(Default, named, { default: Default })`); icon proxies synthesise only PascalCase names.
- Fixture ids: the fixture gateway keys threads as `fx-thread-<FixtureId>`; dispatch-run fixtures are bare (`fx-run-finished`).

## Why this works

The web export is what the e2e tier already verifies, so rendering through `react-native-web` tests the same component tree the product ships on web, with the native-only packages reduced to passthrough Views. Because every stub lives in the harness, a new native dependency is added once (`flowTestAliases`) and every consumer picks it up; a local `vi.mock` would drift per package.

## What the first run found

The flow tier paid for itself on day one: the work-log toggle flipped its label without revealing rows (`useThreadFeed` built the feed with an empty expanded set), approval and user-input cards never cleared after answering through the fixture gateway (its resolved activities carried no `requestId`), and the settings sheet could not mount (stub interop). An implementer who was told never to bypass feed components wrote the failing assertion down as a comment instead of weakening the screen; the fix units turned those comments back into assertions.

## Prevention

- A unit that adds or changes a screen adds or extends that screen's `*.flow.test.tsx`; its gate runs `pnpm --filter mla-tamagui run test:flow`. A store or hook test is a unit test, never a flow test.
- New native package in the design system → add a stub to `frontend/libraries/flow-test-harness/src/stubs` and `flowTestAliases()`; never a per-package `vi.mock`.
- `root.require-flow-tests` (convention-lint) fails a ui-feature package with screens but no flow tests, with a waiver list that is the debt list.
