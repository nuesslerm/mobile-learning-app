---
status: open
area: secrets
paths:
  - "clis/secrets/src/secrets/**"
  - "clis/secrets/tests/flow/secrets.rs"
opened: 2026-10-03
last_verified: 2026-10-20
links: []
---
# A dispatch-test flake parked the secrets `runCliArgs` timeout fix

## What is broken

`dispatch.unit.test.ts` failed 21/45 on an unmodified head (`TypeError: Cannot read properties of
undefined (reading 'stdout')`: the mocked `spawn` returned undefined) whenever the TypeScript secrets
package made it affected, which blocked a `runCliArgs` timeout fix. On 2026-10-04 the suite moved into
the Node remote-chat-agent's unit tests and passed 45/45; the root cause was never found, and the fix was
parked in `.agents/tmp/parked/secrets-timeout.patch`. That path is gitignored and the patch was never
committed, so it exists only on the machine that parked it, if anywhere; it is not in this checkout.

On 2026-10-08 that suite, the TypeScript dispatch core and `runCliArgs` were deleted. Dispatch is now
`wf offload ask` (`clis/offload/src/dispatch_run.rs`, covered by `clis/offload/tests/flow/ask.rs`), and the `bws` calls
`runCliArgs` made go through `run_cli` in `clis/secrets/src/secrets/backend.rs` and `run_bws` in
`clis/secrets/src/secrets/vault.rs`, both of which still wait on the child with no timeout.

## Why blocked

Nothing blocks it now: the flaking vitest suite is gone, but the parked patch was a fix to `runCliArgs`,
so it targets the deleted TypeScript and cannot be re-applied as is.

## What unblocks it

Add a timeout to `run_cli` and `run_bws` (wait on the child with a deadline and kill it when it passes)
and a flow test in `clis/secrets/tests/flow/secrets.rs` where a stubbed `bws` hangs, commit, and run the `wf`
flow tests. Use the parked patch only as a reference if it can still be found.

## Re-checked 2026-10-20

Re-checked against dev at 95e494827. Both bws entry points still block on Command::output() with no deadline: run_cli at clis/secrets/src/secrets/backend.rs:37 and run_bws at clis/secrets/src/secrets/vault.rs:24. The flow tests in clis/secrets/tests/flow/secrets.rs have no case for a bws that hangs. The retry added on 17 Oct (backend.rs:229-240) covers only errors that return, so a hung bws still hangs wf. The fix the gap names is still undone.

Out of date above:

- The title and area framing ("A dispatch-test flake parked the secrets runCliArgs timeout fix") are historical. The flaking suite and runCliArgs were deleted on 2026-10-08 (a0595f138). The live gap is only that bws child processes have no timeout.
- It names run_cli and run_bws as the only paths. run_cli also serves clis/secrets/src/secrets/audit.rs:57 (bws secret list) and keychain_lookup (backend.rs:75, the security CLI).
- It does not mention that since da0702db5 (2026-10-17) backend.rs:229-240 retries transient bws failures three times. A timeout added to run_cli also needs a reason string that this retry recognises, otherwise each hang waits up to four deadlines.
