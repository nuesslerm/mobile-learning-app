# Features

Loaded on demand when working under `frontend/mla-tamagui/features/`. The always-on rules live in the root `AGENTS.md`; this file carries the how-to for this area, moved here verbatim so the root stays small (known-gaps #51).

## Features live in the app's features directory

Every feature sits in `frontend/mla-tamagui/features/<name>`, in one of two forms:

- **In-app**: part of the app package, an Nx project of its own (`project.json`), so its tests are keyed by its own files.
  `leetcode` and `tamil-writing`.
- **Package**: a workspace package with its own `package.json` whose `exports` name its entry points: the barrel
  (`src/index.ts[x]`) and, where it has them, `./routes/*` for the `app/` re-exports and `./testing` for its doubles in
  `src/testing/` (e.g. `coding-agent`'s e2e fake servers in `src/testing/node/`). `@feature/coding-agent`, `@feature/service-report` and `@feature/settings`.

Rings, the layer rules each form follows today, the target and the checks:
`docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.

A package checks itself: its own tsconfig, lint and vitest run. Lint is oxlint over the one `frontend/.oxlintrc.json`,
plus, in the four `@feature/*` packages, dependency-cruiser with the shared layer preset
`frontend/feature-kit/depcruise.feature.cjs` (CI runs it; the pre-commit hook does not reach these packages yet); `vitest.feature.ts` and `vitest.setup.ts` live in `frontend/feature-kit` (an
Nx project of its own: kept here, they would make every package depend on the app project). The app's typecheck, lint and unit tests skip the package directories
(`packages.mjs` lists them for the app's configs; `tsconfig.json` names them). pnpm and Nx give each file to the
innermost package or project, so an edit to a package reruns that package's tasks and its dependents', not the app's own.
There are no `-mocks`/`-impl` sibling packages.

## Flow tests

Feature flow tests mount whole routes via `createFlowApp` with `useDependencyOverrides` overrides; how it works and how a feature opts in: `docs/solutions/testing/flow-tests-mount-whole-routes.md`.

Never `vi.mock` in a flow test (oxlint rejects it). Interact through the `FlowSession` (`tap`, `type`, `layout`, `stroke`, `editor()`). A new fake for a port ships from the port owner's `testing` entry, never from the harness; only a native package is a harness stub (`frontend/libraries/flow-test-harness/README.md`, "Adding a double or a stub"). Where doubles live: `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.
