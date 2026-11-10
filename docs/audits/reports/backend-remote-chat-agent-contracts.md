# backend/remote-chat-agent-contracts: the OpenAPI builder is removed, the contracts stay

Date: 2026-10-09. Plan: `docs/plans/rust-services/standard-crates.md`. Existing package, TypeScript. The app (`features/coding-agent`) imports it in 39 places; nothing else does.

## Scope

- Removed: `src/openapi.ts` (the OpenAPI document builder that fed the Prism mock package, itself removed) and the `zod-openapi` dependency. `src/index.ts` exports the contracts only.
- Comments that pointed at the TypeScript route files now point at `backend/remote-chat-agent/rs/src/api/routes/*.rs`, the Rust service the contracts describe.
- `chat.test.ts` and `worktree.test.ts` moved in from the removed service tree (import `../src/index.js`).

## Before and after

No runtime change: the package is types and zod schemas that the app reads, and its exports other than the removed builder are unchanged. 8 test files, 83 tests, all passing (unit, `pnpm --filter @app/remote-chat-agent-contracts run test`). No benchmark: nothing in the package runs on a hot path.

## Deliberate behaviour changes

- The package no longer builds an OpenAPI document. Nothing consumed it once the mock packages went.

## Deviations from the instructions

- The package stays TypeScript, because the app imports its types; the standard-crates plan covers the Rust backends and CLIs. The contracts are checked against the Rust service by the black-box suite of `backend/remote-chat-agent` (314 tests), not by a generated schema.

## Known gaps

- The zod contracts and the Rust request types are two descriptions of the same messages, kept in step by hand and by the black-box suite.
