# Rust CLI migration: consolidated before/after table (interim)

Every command ported so far, TypeScript bundle against the Rust binary. One x86_64 4-core container, release build
(`opt-level z`, LTO, stripped), Node 22, n=5 medians, peak RSS from `wait4`/`RUSAGE_CHILDREN`, a fresh process per run.
Stubs stand in for the vault, tailnet, VM and OCI, so these are the wrapper's own cost, not end-to-end times. Each
row's method and oracle are in `docs/audits/2026-09-02-token-diet-decision-log.md`. This is the **interim** table: the
TypeScript is not deleted and no call site runs the Rust binary yet, so the final table (and the install-footprint
saving, which needs the TypeScript gone) waits for that.

| Command | TypeScript | Rust |
| --- | --- | --- |
| `wf --version` (the Node floor) | 266.8 ms / 64.2 MB | 3.9 ms / 10.9 MB |
| `wf agents mcp-guard` (every MCP call) | 409.3 ms / 64.3 MB | 11.6 ms / 10.9 MB |
| `agent-config-sync mcp-guard` (standalone) | 162.6 ms / 57.4 MB | 10.5 ms / 10.9 MB |
| `agent-config-sync sync --check` | 270.3 ms / 68.0 MB | 30.0 ms / 11.0 MB |
| `wf secrets sync-env` | 261.2 ms / 65.2 MB | 8.0 ms / 11.0 MB |
| `wf secrets list` | 256.1 ms / 64.8 MB | 5.3 ms / 11.0 MB |
| `wf cli-bench` footprint count | 82 s (symlink bug) | 0.49 s |
| `wf vm-ops health` | 313.4 ms / 65.7 MB | 24.3 ms / 10.9 MB |
| `wf deploy plan --force` | 255.4 ms / 64.5 MB | 5.8 ms / 11.0 MB |
| `wf deploy agent-deps list` | 276.2 ms / 64.6 MB | 6.3 ms / 11.0 MB |
| `wf deploy tf exec` (wrapper) | 284.5 ms / 64.7 MB | 20.5 ms / 10.9 MB |
| `wf tailscale auth-key rotate` (dry run) | 251.3 ms / 64.5 MB | 7.3 ms / 11.0 MB |
| `wf auth done` (refused port) | 295.5 ms / 69.1 MB | 16.3 ms / 11.0 MB |
| `wf dispatch --stats` (5,000-row log) | 419.7 ms / 95.9 MB | 71.1 ms / 16.0 MB |
| `wf dispatch --savings` (5,000-row log) | 340.3 ms / 81.9 MB | 53.3 ms / 13.1 MB |
| `wf dispatch --clear-cooldown` | 253.8 ms / 65.4 MB | 6.7 ms / 11.1 MB |
| `wf verify --budget` | 288.2 ms / 65.5 MB | 14.3 ms / 11.0 MB |

## Footprint

| | TypeScript | Rust |
| --- | --- | --- |
| Shipped artifact | `wf.js` bundle 2,040,087 B, plus a Node runtime | one binary, 2,629,296 B, no runtime |
| Resident floor per process | about 64 MB (Node plus the loaded bundle) | about 11 MB |
| Dependencies at install | 256,087 `node_modules` entries (symlinks not followed) | none at run time; 9 direct crates in `wf` at build time (`clis/wf/Cargo.toml`) |

The Rust binary still hands every unported command to the TypeScript bundle through `__legacy`, so the install
footprint saving is **not realised** yet: the TypeScript and its `node_modules` stay in place until the last command
moves.

## What these numbers do not show

- Nothing in CI, lefthook or the agent hooks calls the Rust binary, so none of this saving is live today.
- Commands whose time is the child programs they start (builds, linters, simulators) would gain only the startup
  floor; the per-call hooks (`agents mcp-guard`, the budget and stats reads) are where the saving lands.
- aarch64: type-checks, not linked or run. The VM and the `gh-runner` image are unverified.

## Update after the browser, metro, knowledge and android slices

n=5 medians, wrapper only, python3 `RUSAGE_CHILDREN`.

| command | TypeScript | Rust |
| --- | --- | --- |
| `wf browser list-devices` | 1371 ms / 166 MB | 9 ms / 12 MB |
| `wf metro status` | 257 ms / 65 MB | 13 ms / 12 MB |
| `wf knowledge index` (this repo) | 529 ms / 94 MB | 28 ms / 13 MB |
| `wf android status` | 283 ms / 73 MB | 9 ms / 13 MB |

Binary size is now 3,265,664 B (it was 2,629,296 B; the embedded help table, the caveat-guard prompts and the device
presets account for the growth). Install footprint is unchanged for the same reason as above. This is still an interim
table: the final one, with the install footprint after the TypeScript is gone, can only be recorded once `bin/wf` is the
Rust binary and the replaced TypeScript is deleted.

## Update after verify --dry-run, auth login, bench list/compare and report-health

n=5 medians, wrapper only, a fresh python3 `RUSAGE_CHILDREN` process per run.

| command | TypeScript | Rust |
| --- | --- | --- |
| `wf bench compare` | 696 ms / 84 MB | 26 ms / 11 MB |
| `wf auth list` | 235 ms / 65 MB | 5 ms / 11 MB |
| `wf verify HEAD --dry-run --skip-tier1` | 249 ms / 65 MB | 14 ms / 11 MB |

Binary size is now 3518424 B. Since the previous update the lefthook gate (`verify --staged`, `secrets doctor`) calls the
Rust `wf` from PATH, so the per-commit gate now runs on it; CI workflows and the images still call the bundle and wait on
a Rust build in the `gh-runner` image and an aarch64 artifact (no Docker daemon is available in the authoring sandbox to
verify either). The final table (install footprint after the TypeScript is deleted) is still not recordable.

## Update after the Rust browser daemon (attached sessions)

| flow (open, navigate, eval via the CLI against one Chromium) | Node/Playwright daemon | Rust daemon (`WF_BROWSER_DAEMON=rust`) |
| --- | --- | --- |
| wall time | 1657 ms | 370 ms |
| daemon resident set | 184.5 MB | 4.8 MB |

Binary size is now 3,580,872 B.
