---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/src/wiring.rs"
  - "clis/orca-serve/tests/deployed_verification.rs"
  - "clis/secrets/src/data/registry.json"
opened: 2026-11-05
last_verified: 2026-11-05
links:
  - "docs/plans/rca-parity/plan.md"
---
# `orca-serve` has no device bearer and no verification device on the deployed service yet

## What is broken

`orca-serve serve` opens rca's agent API socket with a device bearer (`ORCA_RCA_BEARER`) and stops at startup naming the variable without one.
The registry names the key, but the vault holds no value, so the first deploy leg of `orca-serve` fails its preflight (`wf secret verify`).
Its deployed verification test (`clis/orca-serve/tests/deployed_verification.rs`, `WF_DEPLOYED=1`) also needs `ORCA_VERIFY_PAIR_URL`, the pairing link of a
device paired with the deployed host, which can only be minted once the host runs.

## Why blocked

A bearer is minted by the service that will check it, and a pairing link by the host that will accept it: neither exists before the deploy.
Both are credentials the owner's vault holds, and no agent invented a value for them.

## What unblocks it

1. `wf offload bearers mint --label orca` (over the tailnet, with the service secret); store the printed token under `ORCA_RCA_BEARER`
   (`wf secret create ORCA_RCA_BEARER`), then push: the `orca-serve` leg deploys.
2. Run `orca-serve pair --endpoint ws://orca-serve:6768` in the `orca-serve` container and store the printed `url` under
   `ORCA_VERIFY_PAIR_URL`. Step 1 is done and the service deploys (CI run 42147245467). Step 2 is open: `wf vm exec` reaches only agent-shell's own
   Docker, and vm-ops-api exposes no exec into a managed container (`wf vm ops logs` is read-only), so pairing needs a vm-ops-api route that runs
   one allowlisted command in a managed container, or the owner running it on the VM. The next push's deployment verification asks `status.get` as that device and a wrong token.
3. `wf secret doctor` no longer lists either key. The orcad oracle (VM and CI emulator) is a separate run and is not part of this gap.
