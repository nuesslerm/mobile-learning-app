# greenfield — P0 measurements (2026-10-03)

Raw logs: `.agents/tmp/p0/` (not committed).

## 1. Tier timings: sandbox vs VM (`frontend/mla-tamagui`, same commit)

| Tier | Sandbox (x86, 4 CPU) | Peak child RSS | VM (arm64, 2 OCPU, vm-job) |
|---|---|---|---|
| typecheck (`tsc --noEmit`) | 16 s | 1,027 MB | 8 s |
| lint (eslint, warm cache) | 22 s | 1,527 MB | 186 s |
| `test:flow` (54 tests) | 56 s | 561 MB | 165 s |
| unit (vitest, 491 tests) | 8 s | 376 MB | 24 s |
| `export:web` | 31 s | 1,111 MB | 503 s |
| **Total** | **133 s** | | **886 s** |

Notes:
- VM typecheck is faster because its worktree has a warm `tsbuildinfo`.
- VM lint ran cold. Its eslint cache lives in the session worktree, which is rebuilt per session key.
- Peak RSS is the largest single child process (python `RUSAGE_CHILDREN`), not the whole process tree.

**Placement verdict:** the sandbox runs the whole pre-land suite about 6.7× faster (133 s vs 886 s), and export:web is 16× faster (31 s vs 503 s). Tiers 1 and 2 belong on the sandbox; the VM keeps image builds, deploy and tier 3.

## 2. Worktree cost on the sandbox

| Measure | Value |
|---|---|
| `git worktree add` + `pnpm install --offline --frozen-lockfile` | 182 s (dominated by `prepare` builds of the clis) |
| Real disk per worktree (shared pnpm store, hard links) | 715 MB |
| Apparent size | 3,265 MB |
| Free disk before | 9,303 MB |

**Verdict:**
- The 9.3 GB sandbox fits at most 6 concurrent worktrees with a 2 GB floor, so a disk-aware AIMD is required.
- The 182 s install is mostly `prepare`. P3 should reuse a warm worktree per worker slot (reset to the unit's base, not re-created) and drop the per-package `prepare` builds when P5/P6 delete those packages.

## 3. nx cache

| Run | Wall |
|---|---|
| `nx run-many -t typecheck --skip-nx-cache` | 14.6 s |
| Same, cache enabled (`targetDefaults.typecheck.cache: true`) | 15.2 s, no cache hit logged |

**Verdict:** no measurable cache gain, which is under the 30% threshold. **Drop nx** in P5 and use `pnpm --filter "...[origin/dev]"`.

## 4. Cheap-model ladder (local dispatch ledger, `.agents/tmp/opencode-dispatch-runs.jsonl`)

| Shape | Model | Runs | OK | p50 |
|---|---|---|---|---|
| implementer:unit | deepseek-v4-flash | 139 | 139 | 98 s |
| implementer:flow | deepseek-v4-flash | 38 | 38 | 234 s |
| implementer:contract | deepseek-v4-flash | 9 | 9 | 167 s |
| implementer:integration | deepseek-v4-flash | 5 | 5 | 688 s |
| implementer:integration | MiniMax-M3 | 2 | 2 | 1,399 s |
| implementer:unit | MiniMax-M3 | 1 | 1 | 860 s |
| judge/hook (unknown shape) | MiniMax-M3 | 452 | 452 | 44 s |
| implementer:unit / flow | kimi (failed) | 53 | **0** | 9–13 s |
| any | kimi-for-coding | 3 | 3 | 151 s |

Verdicts:
- **Correction (p1-10, 2026-10-04): kimi is not broken.**
  - The 0/53 count was misattributed. Per attempt, 106 `bad-output` failures came from a stale provider id, `kimi-for-coding/*` (53 on kimi-for-coding, 53 on k3), which has since been renamed to `kimi-code-plan-global/*`.
  - The current provider shows 4 OK, 13 timeouts and 10 wrote-nothing across ~30 attempts: unreliable, but working.
  - The shipped ladder already orders deepseek → MiniMax → kimi, so no change is needed.
- **Implementer ladder:** deepseek-v4-flash → kimi-for-coding (once fixed) → MiniMax-M3 → Haiku → Sonnet. MiniMax implements correctly but slowly (860–1,400 s p50), so it is the hedge, not the second try.
- **Judge and vision:** MiniMax-M3 (44 s p50 across 452 judge calls) first.

## 5. Package usage ledger
Columns:
- *dependents*: workspace packages listing it as a dependency;
- *hooks*: named in `lefthook.yml` or `.agents/config/hooks.json`;
- *compose*: named in a docker-compose file;
- *wf dep*: a dependency of `clis/workflow-cli`.

| package | name | dependents | hooks | compose | wf dep | last commit |
|---|---|---|---|---|---|---|
| backend/leetcode-auth-cookie-capture | @app/leetcode-auth-cookie-capture | 0 | 0 | 0 |  | 2026-09-08 |
| backend/leetcode-auth-cookie-capture-contracts | @app/leetcode-auth-cookie-capture-contracts | 1 | 0 | 0 |  | 2026-08-30 |
| backend/leetcode-auth-cookie-capture-mock | @app/leetcode-auth-cookie-capture-mock | 0 | 0 | 0 |  | 2026-10-02 |
| backend/leetcode-backend | @app/leetcode-backend | 1 | 0 | 0 |  | 2026-09-24 |
| backend/leetcode-backend-contracts | @app/leetcode-backend-contracts | 2 | 0 | 0 |  | 2026-10-02 |
| backend/leetcode-backend-mock | @app/leetcode-backend-mock | 0 | 0 | 0 |  | 2026-10-02 |
| backend/opencode-telegram-bot | @app/opencode-telegram-bot | 1 | 0 | 0 |  | 2026-09-06 |
| backend/plannotator-proxy | @app/plannotator-proxy | 0 | 0 | 0 |  | 2026-07-27 |
| backend/remote-chat-agent | @app/remote-chat-agent | 0 | 0 | 0 |  | 2026-10-03 |
| backend/remote-chat-agent-contracts | @app/remote-chat-agent-contracts | 7 | 0 | 0 |  | 2026-10-02 |
| backend/remote-chat-agent-mock | @app/remote-chat-agent-mock | 0 | 0 | 0 |  | 2026-10-02 |
| backend/reporting-service | @backend/reporting-service | 0 | 0 | 0 |  | 2026-10-02 |
| backend/vm-gateway | @app/vm-gateway | 0 | 0 | 0 |  | 2026-09-24 |
| clis/agent-approve-guard | @lib/agent-approve-guard | 0 | 0 | 0 |  | 2026-09-11 |
| clis/agent-config-sync | @cli/agent-config-sync | 0 | 0 | 0 |  | 2026-10-02 |
| clis/agent-evals | @cli/agent-evals | 0 | 0 | 0 |  | 2026-10-03 |
| clis/arch-diagrams | @cli/arch-diagrams | 1 | 0 | 0 | y | 2026-10-02 |
| clis/caveat-guard | @cli/caveat-guard | 1 | 0 | 0 |  | 2026-10-03 |
| clis/cheap-flow | @cli/cheap-flow | 1 | 0 | 0 | y | 2026-10-03 |
| clis/convention-lint | @lib/convention-lint | 2 | 0 | 0 | y | 2026-10-03 |
| clis/env-bootstrap | @cli/env-bootstrap | 0 | 0 | 0 |  | 2026-10-02 |
| clis/harness | @cli/harness | 1 | 1 | 1 | y | 2026-10-02 |
| clis/infra-ops | @cli/infra-ops | 1 | 0 | 0 | y | 2026-10-03 |
| clis/logger | @cli/logger | 0 | 0 | 2 |  | 2026-10-02 |
| clis/multi-browser | @cli/multi-browser | 5 | 0 | 0 | y | 2026-10-02 |
| clis/opencode-harness | @cli/opencode-harness | 5 | 0 | 0 | y | 2026-10-02 |
| clis/secrets | @cli/secrets | 12 | 1 | 9 | y | 2026-10-03 |
| clis/vnc-auth-capture | @cli/vnc-auth-capture | 4 | 0 | 0 | y | 2026-10-02 |
| clis/workflow-cli | @cli/workflow-cli | 0 | 0 | 0 | y | 2026-10-03 |
| features/coding-agent | @lib/coding-agent-contracts | 3 | 0 | 0 |  | 2026-09-06 |
| features/coding-agent-impl | @feature/coding-agent | 1 | 0 | 0 |  | 2026-10-01 |
| features/coding-agent-mocks | @feature/coding-agent | 1 | 0 | 0 |  | 2026-09-26 |
| features/dev-tools | @lib/dev-tools-contracts | 1 | 0 | 0 |  | 2026-10-02 |
| features/dev-tools-impl | @feature/dev-tools | 1 | 0 | 0 |  | 2026-10-02 |
| features/dev-tools-mocks | @feature/dev-tools | 1 | 0 | 0 |  | 2026-10-02 |
| features/leetcode | @lib/leetcode-contracts | 1 | 0 | 10 |  | 2026-10-02 |
| features/service-report | @lib/service-report-contracts | 2 | 0 | 0 |  | 2026-10-02 |
| features/service-report-impl | @feature/service-report | 1 | 0 | 0 |  | 2026-10-01 |
| features/service-report-mocks | @feature/service-report | 1 | 0 | 0 |  | 2026-10-02 |
| features/settings | @lib/settings-contracts | 0 | 1 | 0 |  | 2026-10-02 |
| features/settings-impl | @feature/settings | 1 | 0 | 0 |  | 2026-10-01 |
| features/settings-mocks | @feature/settings | 1 | 0 | 0 |  | 2026-10-02 |
| frontend/mla-tamagui | mla-tamagui | 0 | 0 | 0 |  | 2026-10-03 |
| frontend/libraries/app-config | @lib/app-config | 3 | 0 | 0 |  | 2026-10-02 |
| frontend/libraries/app-di | @lib/app-di | 5 | 0 | 0 |  | 2026-10-02 |
| libraries/eslint-config-node-ts | @lib/eslint-config-node-ts | 11 | 0 | 0 |  | 2026-08-30 |
| libraries/eslint-plugin-lint-contentions | @lib/eslint-plugin-lint-contentions | 17 | 0 | 0 |  | 2026-10-02 |
| libraries/eslint-plugin-mobile-learn | @lib/eslint-plugin-mobile-learn | 25 | 0 | 0 | y | 2026-10-02 |
| frontend/libraries/flow-test-harness | @lib/flow-test-harness | 4 | 0 | 0 |  | 2026-09-30 |
| frontend/libraries/gesture-geometry | @lib/gesture-geometry | 2 | 0 | 0 |  | 2026-10-02 |
| libraries/hook-runtime | @lib/hook-runtime | 0 | 0 | 0 |  | 2026-10-02 |
| libraries/hook-test-kit | @lib/hook-test-kit | 1 | 0 | 0 |  | 2026-09-03 |
| frontend/libraries/logging | @lib/logging | 26 | 1 | 12 | y | 2026-10-02 |
| libraries/rate-limit | @lib/rate-limit | 4 | 0 | 0 |  | 2026-10-02 |
| frontend/libraries/registry-service | @lib/registry-service | 3 | 0 | 0 | y | 2026-10-02 |
| frontend/libraries/rn-agentation | @lib/rn-agentation | 2 | 0 | 0 |  | 2026-10-02 |
| libraries/socks5-forward | @lib/socks5-forward | 2 | 0 | 0 |  | 2026-10-02 |
| frontend/libraries/tamagui-design-system | @lib/tamagui-design-system | 6 | 0 | 0 |  | 2026-10-02 |
| libraries/vm-request-auth | @lib/vm-request-auth | 3 | 0 | 0 | y | 2026-10-02 |
| frontend/libraries/voice-edit-dsl | @lib/voice-edit-dsl | 4 | 0 | 0 | y | 2026-10-02 |
| tooling/cli-nx-plugin | @tooling/cli-nx-plugin | 8 | 0 | 0 | y | 2026-10-03 |

## 6. Sandbox lifetime
- This cannot be measured inside one session without going idle.
- It is measured in P3: a `wf offload worker` heartbeat log plus the lease expiry it causes.
- Leases keep the design correct either way.

## 7. VM incidents during P0
- **22:46–23:12 UTC:** the VM hung while trunk-building remote-chat-agent inside an uncapped dind, and was hard-reset at 23:12.
- Dind came back on its own (the stale-pidfile fix `29967cfa` worked).
- **Fix landed:** `2e0888d7`, `agent-dind` `mem_limit: 6g`. The `app` cgroup reads `8589934592` (8 GiB).
- **23:14:** vm-ops-api self-redeployed to `05ebfeb024eb` (the guards commit). That confirms P1's premise: an image build alone can take the whole VM down.
- **23:50:** two VM timing jobs ended `exit=1` straight after the worktree sync with no script output.
  - Cause: `wf vm job start <name> -- bash -c '<script>'` joins argv into `eval '<joined>'` without shell-quoting, so `bash -c` received only `cd`.
  - Workaround: pass the script as one argument.
  - Fix belongs in P6 (`wf offload worker` takes a script file, never a joined argv).
- `wf vm job logs <name> --follow` exited 0 before the job's output ended. The job itself finished `exit=0` with every result line in `output.log`, so the follow stream truncates.
  - Fix belongs in P6 (`wf offload worker` reads the job's own exit and output files).
