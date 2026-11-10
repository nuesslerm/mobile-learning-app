# libraries/eslint-plugin-mobile-learn: the retired entrypoint

Date: 2026-10-08.

## Scope

The `require-span-coverage` rule lists the TypeScript service entrypoints that must create a span. `backend/remote-chat-agent/src/index.ts` was one; the file is gone (the service is Rust and exports its spans itself), so the entry is removed from the rule's default list. The rule's test now uses the other listed entrypoint (`backend/leetcode-auth-cookie-capture/src/server.ts`) for its cases and gains a case that the retired path is no longer checked; that case was run red first.

## Before and after

| Measure | Before | After |
| --- | --- | --- |
| Entrypoints in the default list | 2 | 1 |
| Rule tests | 11 | 12 (the retired-entrypoint case added) |
| Runtime numbers, install footprint | not applicable: a lint rule | not applicable |

## Tests, by tier

- Unit: `require-span-coverage.test.ts`, 12 of 12 pass.

## Deliberate behaviour changes

- The rule no longer requires a span in `backend/remote-chat-agent/src/index.ts` (the file is gone); it still requires one in the other listed entrypoint.

## Architecture deviations

- None.

## Known gaps

- The Rust service's span coverage has no lint rule; it is covered by `rs/tests/telemetry.rs` (a request reaches the sink as a span, and a registered secret never appears in a shipped record).
