---
status: open
area: rca-parity
paths:
  - "tooling/mimic-drivers/src/t3/release.ts"
  - "clis/t3-serve/cassettes/t3-v0.1.6/mimic.toml"
  - "tooling/t3-conformance/t3-pin.json"
opened: 2026-11-04
last_verified: 2026-11-06
links:
  - "docs/plans/rca-parity/plan.md"
  - "docs/plans/mimic/plan.md"
---
# The pinned T3 server has not been run by `wf mimic verify --live`

## What is broken

The mock (`mock-server.ts`) and the Nx target `t3-conformance-live` are gone (mimic X5). `wf mimic verify t3-v0.1.6 --live` re-records the pinned
server through the T3 driver (`tooling/mimic-drivers/src/t3`) and diffs it with the committed set, but it has only run against a stand-in server and
faked downloads: nothing has yet replayed the real `t3@0.1.6` as a server, so a golden that no longer matches what the pinned T3 does would not show.

## Why blocked

The pinned runtime is reachable and verifiable without any credential (`t3-0.1.6-linux-x64.tar.gz`, 123152548 bytes, sha256 in `t3-pin.json`), and the
unpacked `t3` starts (`t3 --help`). The permission policy of the sandbox that built this refused to start the downloaded binary as a server, and the shape
of what `t3 auth pairing create --json` prints is read tolerantly (`credential`, `token`, `code` or `secret`) until a real run confirms it.

## What unblocks it

On the VM (or any session that may start the pinned binary): `wf mimic verify t3-v0.1.6 --live`, then replay the result against `t3-serve serve`
(`clis/t3-serve/tests/integration/chain.rs` does it over the chain). Fix the pairing parse if the output shape differs.

## Update (R1, 7 Nov 2026)

The pinned server was run on the VM (linux-arm64 release, sha256 equal to the pin) and 202 of 205 phone rows were recorded from it: `clis/t3-serve/cassettes/t3-recorded-{1,2,3}`. `verify <set> --live` on the VM is the re-run. See `docs/plans/mimic/plan.md` R1 and `docs/runbooks/record-reference-servers-on-the-vm.md`.
