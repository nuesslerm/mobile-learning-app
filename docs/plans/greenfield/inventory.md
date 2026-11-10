# greenfield — inventory (P0 exit gate)

Evidence:
- `plan-research.md` §5 ledger: dependents, hooks, compose, wf dependency, last commit;
- the user decisions of 2026-10-03.

Verdicts: **keep**, **fold** (into `clis/wf`), **delete**, **merge** (into a sibling). A row marked *check* is re-verified by its P5/P6 unit before anything is removed.

## Backend
| Package | Verdict | Evidence |
|---|---|---|
| leetcode-backend (+contracts, +mock) | keep | LeetCode surface stays; contracts have 2 dependents |
| leetcode-auth-cookie-capture (+contracts, +mock) | keep | LeetCode login |
| remote-chat-agent (+contracts, +mock) | keep | user decision; contracts have 7 dependents |
| reporting-service | keep | dashboards/reports surface stays |
| vm-gateway | keep | tailnet ingress for the VM |
| opencode-telegram-bot | **delete** | user decision; last commit 2026-09-06; 1 dependent to unhook |
| plannotator-proxy | **delete** | user decision; 0 dependents; last commit 2026-07-27 |

## clis
| Package | LOC | Verdict | Evidence |
|---|---|---|---|
| workflow-cli | 44.3k | **fold** → `clis/wf` core | the wf entrypoint; 98 commits in 90 days |
| ~~cheap-flow~~ | 8.7k | **folded** (`wf offload plan/status/dispatch`) | deleted; units run on `wf-worker`, dispatch on rca |
| opencode-harness | 8.3k | **merge into remote-chat-agent**, then delete | 5 dependents; rca becomes the only runner, one-shot via `rca run` (requirements §3a) |
| secrets | 5.8k | **fold** (keep core) | 12 dependents, 9 compose refs |
| infra-ops | 8.2k | **folded** into workflow-cli `wf infra oracle` / `wf vm net` / `wf tunnel` (2026-10-04) | kept: sync-creds, oci reboot/status, tf apply/exec, agent-deps, tailscale up/devices/auth-key, tunnel up |
| agent-config-sync | 5.6k | **fold**, slimmed | user keeps codex/gemini/opencode mirrors |
| agent-approve-guard | 7.0k | **fold** → deny list + small hook | 0 dependents; hook-only |
| caveat-guard | 6.0k | **fold**, opt-in, off by default | user decision |
| vnc-auth-capture | 8.6k | keep (core) | 4 dependents; leetcode/claude login capture |
| multi-browser | 13.0k | **delete** *check* | 5 dependents; replaced by plain Playwright in tier 2 |
| convention-lint | 14.5k | **delete** | ≤ 5 rules move to eslint plus one budget check |
| agent-evals | 9.8k | **delete** | 0 dependents; paid-measurement harness |
| harness | 5.0k | **delete** *check* | 1 dependent, 1 hook, 1 compose ref to unhook |
| env-bootstrap | 4.9k | **delete** | 0 dependents; mise + session setup script |
| arch-diagrams | 1.0k | **delete** | diagrams:check is gone with lefthook trim |
| logger | 0.5k | **fold** into `wf log` *check* | 2 compose refs (cloud-init, vm-ops-api) |

## libraries and tooling
| Package | Verdict | Evidence |
|---|---|---|
| tamagui-design-system, flow-test-harness, app-config, app-di, vm-request-auth, voice-edit-dsl, gesture-geometry, rate-limit, registry-service, socks5-forward, rn-agentation | keep | 2–6 dependents each; product or VM auth |
| logging | keep, slim | 26 dependents |
| eslint-config-node-ts | keep | 11 dependents |
| eslint-plugin-mobile-learn | keep, cut to rules that fire | 25 dependents |
| eslint-plugin-lint-contentions | **merge** into eslint-plugin-mobile-learn *check* | 17 dependents; one plugin is enough |
| hook-runtime, hook-test-kit | **merge** into `clis/wf` hooks | 0–1 dependents |
| tooling/cli-nx-plugin | **delete** with nx | no cache gain measured (`plan-research.md` §3) |

## features (P7)
| Package set | Verdict | Evidence |
|---|---|---|
| coding-agent, service-report, settings, dev-tools, leetcode | keep | all product surfaces stay |
| `*-mocks` / `*-impl` / contract triplets | **merge** where the contract has a single implementation *check* | each mocks/impl has exactly 1 dependent (the app) |

## Process
| Item | Today | Verdict |
|---|---|---|
| lefthook jobs | ~30 | keep 5: lint-staged, typecheck affected, unit affected, flow affected, secrets scan |
| agent hook commands | 19 | keep ≤ 4 |
| root scripts | 62 | keep ≤ 15 |
| baseline/waiver JSONs | 8 | delete |
| AGENTS/CLAUDE.md | 10 | one root file ≤ 150 lines; area notes only for infra and the app |
| docs/solutions | 170 | keep all; add `paths` globs so `wf knowledge for` can retrieve them; merge duplicates |
| docs/plans | 38 | archive all but greenfield |
| known-gaps.md | 10,161 lines, 110 entries (≥ 44 resolved) | migrate into the knowledge ledger (requirements §2.10): open entries become `docs/gaps/*.md`, resolved ones with a lesson go through cf-compound into `docs/solutions`; the index is generated |

## Measured placement (from `plan-research.md` §1)
- The pre-land suite takes 133 s on the sandbox and 886 s on the VM, so tiers 1 and 2 run on the sandbox.
- The VM keeps image builds (native arm64), deploy, tier 3 and the shared Metro.
- The sandbox can hold at most 6 worktrees at 715 MB each, with a 2 GB floor.

## Fixes queued for P1 (found during P0)
1. **kimi pool:** 0 of 53 OK on implementer shapes. Fix its auth or drop it from automatic rungs.
2. **`wf vm job start`** joins argv without quoting (`clis/workflow-cli/src/entrypoints/cli.ts:3192`).
3. **`wf vm job logs --follow`** exits before the output ends.
4. **Dind memory:** the cap landed (`2e0888d7`). It needs a P1 runbook check that a build OOMs inside dind without hanging the VM.
