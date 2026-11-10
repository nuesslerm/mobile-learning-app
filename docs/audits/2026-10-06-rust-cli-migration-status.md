# Rust CLI migration: status, evidence and what is left

Written 2026-10-06 at the end of the first working session. Everything here is on
stacked draft PRs #309 to #319; none is merged, no call site is repointed.
Numbers are from one x86_64 container (4 cores), release builds, n=5, peak RSS
from rusage. Raw detail is in `2026-09-02-token-diet-decision-log.md`.

## What the baseline said

Every TypeScript `wf` command costs about 266 ms and 64 MB, `--version`
included: Node's own floor is 41.7 MB and the whole bundle load is the rest.
Dependencies add 8 to 11 MB each (commander, yaml, pngjs, OpenTelemetry,
loglayer); Playwright adds 95 MB and 755 ms but only on browser commands. No
TypeScript-side dependency swap can remove the floor, so the work went straight
to Rust.

## Ported, with evidence

| PR | Command(s) | TypeScript | Rust | Oracle |
| --- | --- | --- | --- | --- |
| #312 | dispatcher, `--version` | 266.8 ms / 64.2 MB | 3.9 ms / 10.9 MB | 4 Rust tests, existing suite on both binaries |
| #315 | agent-config-sync `sync`, `--check`, `mcp-guard` | 270 ms / 68 MB (check), 163 ms / 57 MB (guard) | 30 ms / 11 MB, 10.5 ms / 10.9 MB | zero drift on the real repo, golden tree written by the TypeScript, byte-identical deny |
| #316 | `wf secrets list|get|env|sync-env|doctor` | 261 ms / 65 MB (sync-env) | 8.0 ms / 11 MB | 32 black-box tests, same suite on both binaries, stub `bws` |
| #317 | `wf bench` | 82 s for the footprint (bug) | 0.49 s | 5 black-box tests, both binaries |
| #318 | `wf agents ...`, `wf caveat on|off|status` | 409 ms / 64 MB (`agents mcp-guard`, the per-MCP-call hook) | 11.6 ms / 10.9 MB | 9 black-box tests, both binaries |

The whole integration directory (80 tests) passes against the TypeScript bundle
and against the Rust binary through `WF_BIN`. Binary: 2.1 MB for everything above
(1.9 MB bundle plus a 41.7 MB runtime before).

One correction to the baseline: the TypeScript `node_modules` entry count
followed pnpm symlinks (4,406,964 entries, 82 s); the real count is 256,087.

## What is left, and what each piece needs

| Package | TypeScript lines | State | What it needs |
| --- | --- | --- | --- |
| `clis/workflow-cli` (`wf`) | 25,691 | `secrets` read path, `bench`, `agents`, `caveat` toggle ported; the rest forwards to the TypeScript | See below |
| `clis/multi-browser` | 11,761 | untouched | Playwright; drive Chromium over CDP from Rust or keep a Node sidecar spawned only for browser commands |
| `clis/vnc-auth-capture` | 3,834 | untouched | Depends on multi-browser and secrets |
| `clis/secrets` | 3,578 | read path in the Rust `wf`; library stays | Writers (`set`, `delete`, `push-notes`, `create`, `rename`, `update`) and the Node library API that backend services import |
| `clis/caveat-guard` | 3,259 | toggle only | Imports the 25k-line `@app/remote-chat-agent` runner (opencode); needs that seam decided |
| `clis/agent-config-sync` | 2,179 | complete in Rust beside the TypeScript | Repoint call sites, then delete the TypeScript |

Inside `wf` the remaining groups, by what they are:
- **Orchestration where child processes dominate** (`verify` 1,031 lines, `deploy`
  314, `infra` 1,530, `metro` 1,126, `ios` 2,288, `android` 645, `agentShell`
  585, `runbook` 338, `artifacts` 370, `report` 1,156): a Rust port removes the
  Node floor, but the wall time is the linters, builds and simulators they start,
  so the runtime saving is small. They are needed for the "no TypeScript" goal,
  not for memory.
- **A signed network client** (`vm-ops`, `vm exec|logs|ledger|recover`, 700 lines
  plus the 440-line client): Ed25519 request signing (`@lib/vm-request-auth`) and
  curl through tailscaled's SOCKS5 proxy. Portable with a small crypto crate and
  a stub `curl` for the black-box tests, but unverifiable here without a tailnet.
- **`dispatch`, `work`, `knowledge`** (the opencode ladder and the git-state
  worker) import `@app/remote-chat-agent`: same seam as caveat-guard.

## Decisions that are open and are not mine to settle silently

1. **Distribution.** Hooks, lefthook and CI call `node clis/*/bundle/*.js`. The
   Rust `wf` is built by `.agents/setup.sh` as `wf-rs` (#314) and nothing calls
   it. Until `gh-runner`, the VM image and an aarch64 build carry cargo, no
   hook-called command can be repointed. A missing guard binary must block
   (exit 2), never allow.
2. **The merge order.** Eleven stacked drafts. They are meant to merge bottom to
   top; each PR's base is the one below it, so retarget to `dev` as each lands.
3. **The runner seam** (`@app/remote-chat-agent`): caveat-guard, `dispatch`,
   `work` and `knowledge` all sit behind it. It is the largest single piece, and
   the stated plan was to defer it.

## Where the time actually goes

The saving that matters per day is on frequently run short commands, and those
are done: the per-MCP-call guard (409 to 11.6 ms), the pre-commit and deploy
secrets paths (261 to 8 ms) and every `--version` or `bench`. What remains is
mostly code that waits on other programs.

## Known gaps in the Rust `wf` (found by reading `cli.ts`, not yet closed)

- **Command telemetry.** The TypeScript `wf` wraps every command in an
  OpenTelemetry span and records metrics (`getNodeTracer`, `getNodeMeter`,
  `flushNodeOtel`), exported over OTLP when `BETTERSTACK_OTLP_*` is set. The Rust
  `wf` emits none, so `wf` usage that goes through it is invisible at the sink.
  Needs a decision on transport (the secrets port used `curl`; OTLP over HTTP/JSON
  would fit) and a check that the line a command causes can be queried at the sink.
- **`--json` and error envelope.** Native commands print plain errors
  (`Error: ...`). The TypeScript prints `wf: <message>` or, under `--json`, an
  `{ok:false,error}` line. Native commands currently hand `--json` to the
  TypeScript (an unrecognised option returns "not mine"), which is correct but
  means those invocations still pay the Node cost.
- **Env files** were a gap too and are fixed in #320; any other wiring that
  `cli.ts` runs before a command should be read before the next port.
