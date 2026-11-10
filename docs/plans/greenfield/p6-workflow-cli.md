# P6: slim workflow-cli into the `wf` target surface

> Status 2026-10-21: history. `clis/workflow-cli` was deleted when wf moved to Rust, so the layering described here is not current; wf's target architecture is type 3 in `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.

Command inventory of `clis/workflow-cli`, taken 2026-10-04 after cheap-flow and infra-ops were folded in. It ends with the ordered list of units. Decision: workflow-cli absorbs `clis/wf` (unit 14), not the other way round, because that is less churn.

Overrides to the verdicts below (2026-10-04):
- **`report-health`: KEEP.** It is the only producer of the `health_reports` rows that the service-report feature (a kept product surface) displays. It has no scheduled caller today, so a later unit should run it from the vm-ops-api heartbeat or the verify pipeline.
- **`artifacts publish`: FOLD into `verify`.** The contract's verify report is published as an HTML artifact, so `verify` takes over this command instead of it being dropped.

I couldn't get exact LOC for some subcommands, so numbers marked ~ are section estimates. "Src" means non-test TS. workflow-cli today is 27.5k src plus about 17.9k of tests and e2e, about 45k in total. Of the src, `src/entrypoints/cli.ts` alone is 4,879 lines.

## Command table

Callers were counted outside `clis/workflow-cli`, `dist/`, `bundle/` and `docs/archive`. "docs" means plans, solutions and gaps only. Verdicts: KEEP = in the target surface or used by a kept runbook, skill or hook; FOLD = merge into a kept command; DROP = no live callers.

| Group / subs | Src LOC (use-cases + data it alone uses + cli.ts section) | Tests | Live callers | Verdict |
|---|---|---|---|---|
| `work`, `status [slug]` | ~90 cli (delegates to `clis/wf/bin/wf-worker.js`) | – | skills cf-work, cf-lfg, cf-plan; `.agents/rules/delegation.md` | KEEP |
| `status all` | 78 `status-all.ts` + 66 | small | skill only | DROP |
| `worker`, `caveat-guard`, `agent-config-sync` (passthroughs via `src/data/cliRegistry.ts`) | ~60 | – | `.agents/config/hooks.json` (caveat-guard, agent-config-sync sync) | KEEP, rename to `caveat` and `agents sync` |
| `dispatch`, `opencode dispatch/batch/stats/savings/pricing/clear-cooldown` | 457 `dispatchCommands.ts` + 73 | – | 8 skills, 2 runbooks, `.agents/CLAUDE.md` | KEEP `dispatch`; fold the `opencode` subcommands into `dispatch --stats` etc. |
| `secrets` (mounted from `@cli/secrets`) + `registry sync` | 54 + 25 | – | `lefthook.yml:62` (`secrets doctor`), `cloud-init.sh.tftpl:12`, `deny.json`, 7 runbooks | KEEP |
| `ios` (start/stop/build/launch/sim/tap/swipe/screenshot…) | 2,251 + `data/ios` 256 + `lib/simulator` 90 + `lib/hardware-keyboard` 117 + 475 | 2,084 | 10 runbooks, `deny.json` | KEEP |
| `android` | 793 + `data/android` 140 + 147 | 731 | skill only | KEEP (target surface) |
| `metro` (start/stop/status/restart/link/…) | 1,174 + `data/expo` 730 (shared with ios/android) + 262 | 569 | 15 runbooks, 6 skills, hook-runtime | KEEP |
| `metro dev-bridge` | 133 + ~75 | – | none | DROP |
| `maestro`, `prepare maestro`, `lint maestro` | 592 + `data/qa/maestro` 74 + 174 + 123 | 834 | `.agents/skills/workflow-cli` only | DROP |
| `prepare` (verb-first alias) | 82 | in command-mapping test | skill only | DROP (keep `ios/prepare.ts` and `android/prepare.ts`, which launch uses) |
| `browser` | 76 (forwards to `@cli/multi-browser`) | small | 8 runbooks, 3 skills | KEEP |
| `auth` (`@cli/vnc-auth-capture`) | 90 | `authLogin` test | `cloud-init.sh.tftpl:323`, 3 runbooks | FOLD into the CLI_REGISTRY passthrough |
| `tunnel`, `tailscale` | `infra/tunnel` 167 + `tailscale*` 719 | part of infra 1,685 | `.agents/hooks/session-heal.sh:7` (`tailscale up`), `.agents/skills/browser-cli` (tunnel), 8 runbooks | FOLD: `tailscale up` into `vm status --heal`; `tunnel` under `browser` |
| `infra oracle` (tf apply/exec, sync-creds, agent-deps, oci instance) | rest of `use-cases/infra` (~500) + `data/infra` 684 + `contracts/infra` 389 + `infraCommands.ts` 209 + `di/infraContainer.ts` 64 | ~1,000 | `docs/archive/runbooks/greenfield-p1.md`, `.agents/instructions/infra`, `infra/oracle-cloud/*.md` | FOLD into `deploy status/run`; drop `oci instance` and `tf exec` |
| `vm recover` | 153 | some of the vm tests | 12 refs (`.agents/instructions/infra/oracle-cloud.md`, greenfield-p1) | KEEP |
| `vm precommit` | 189 + `affected.ts` 28 + `sessionScript.ts` 140 | ~400 | `lefthook.yml:54` | FOLD into `verify`. It imports `vm/vmJob.ts`: the other agent's vm-jobs removal must not break it |
| `vm job`, `vm test`, `vm ledger` | – | – | – | Being removed (other agent) |
| `lint files/budget/changed/all` | ~240 + 62 | 339 | `lefthook.yml:46,52`, `.agents/instructions/root.md` | FOLD into `verify --staged` (or keep hidden) |
| `agent-shell exec/exec-status/exec-cancel` | `runScript` + `execScript` + `ssh` ≈ 250 + `data/agentShell` 160 | ~600 | `deny.json:45`, `infra/oracle-cloud/agent-shell/README.md`, 4 runbooks | FOLD into `vm exec <file>` |
| `agent-shell build/release/redeploy/install/sync` | `buildImage` + `installDep` + `sync` ≈ 275 + ~300 cli | ~400 | agent-shell README/Dockerfile, `.agents/setup.sh:56` (comment) | FOLD into `deploy run agent-shell` |
| `agent-shell pty`, `agent-shell e2e` | 283 + 129 + ~120 | ~400 | `infra/oracle-cloud/agent-shell/e2e-wf-smoke.sh` only | DROP (move into the smoke script) |
| `logs` (BetterStack tail/doctor) | 381 + `data/logs` 511 + ~60 | ~300 | greenfield-p1 and vnc-login-invariants-sweep runbooks, agent-shell scripts | FOLD into `vm logs <app>` |
| `log emit` + `entrypoints/logger.ts` | 72 + 41 | ~80 | `infra/oracle-cloud/logger-cli/Dockerfile:13` (bun-compiles `src/entrypoints/logger.ts`) | KEEP the entrypoint only (not a wf subcommand) |
| `runbook list/show/run` | 671 + `data/agent` 177 + 221 | 842 | `.agents/skills/cf-dispatch`, 4 runbooks | FOLD into `dispatch --runbook`; drop `runOnVm.ts` and `recordResult` |
| `gemini sync/regen/which` | 469 + 104 + ~260 | 374 | `.agents/skills/gemini-context-sync` | FOLD into `knowledge index --gemini` |
| `edit-stats` | 142 + `data/editStatsOrds` 143 + 131 | 84 | `docs/archive/runbooks/voice-edit-iterative-solve.md` | DROP unless that runbook is kept |
| `report-health` | 965 + `data/ordsHealthReports` 146 + 126 | 179 | comments only (`features/service-report`, `autonomous-db.tf`) | DROP |
| `artifacts publish` | 367 + 209 | 666 | comment in `verification-artifacts.tf` | DROP (or reuse later as `verify --publish`) |
| `goal`, `metrics`, `flow-test` | 405 / 667 / 315 | 372 / 357 / 89 | docs only | DROP |
| `debug`, `targets`, `doctor`, `help-search`, `completion`, `-i` | 157 / 60 / 252 / ~140 / ~160 / ~330 | 157 / – / 216 / 1,072 for help+completion+interactive | `.agents/skills/workflow-cli` only | DROP |
| `usage`, `vnc`, `vm-ops`, `__usage-send` | ~400 / ~380 / 95 | ~600 | – | Being removed (other agent) |

## Estimated LOC

- Drops: about 6.2k src and 4.3k tests.
- Folds: about 2.6k src and 1.5k tests.
- The other agent's removals: about 1.5k src and 1.5k tests.

That leaves workflow-cli at about 17k src and 10.5k tests, roughly 27k. Adding clis/wf (6.6k src, 2.5k tests) gives about 36k. Even with tests excluded, about 24k of src would use up the whole ≤25k tooling budget on its own. The shrink below is therefore required, not optional.

## Biggest remaining modules and how to shrink them

- **`src/entrypoints/cli.ts` (4,879 lines).**
  - It has 110 imports. Lines 1–680 are tracing, usage spans and `run`/`runJsonAware` wrappers. The usage parts go away with the other agent's removal.
  - It also holds inline wiring for every group, including 343 lines of `ios` sim subcommands at 4097–4440.
  - Split it into `entrypoints/commands/<group>.ts` the way `dispatchCommands.ts` already is. Combined with the drops, about 1.8k of glue should remain.
- **`src/use-cases/ios/launch.ts` (904 lines).**
  - It contains `iosLaunch` (lines 165–455), `iosBuildExpoGo` (456–574), `iosLaunchExpoGo` (575–827) and `resolveIosLaunchSlot` (828–904).
  - `detectScheme` is duplicated in `ios/build-dev.ts:43`, and `podsUpToDate`/`nativeBuildUpToDate` overlap with build-dev's cache checks. Pull these into one `ios/nativeBuild.ts`; that saves about 150 lines.
- **`src/data/vmops/client.ts` (800 lines).**
  - The vm-job, usage, ledger and `fetchWfSmokeTest` functions are dead once the other agent's removals land, which takes it to about 400.
  - The rest overlaps with the vm-ops-api client, so it could import a shared client from `infra/oracle-cloud/vm-ops-api`.
- **`src/contracts/index.ts` (800 lines).**
  - These are 33 hand-written port interfaces. `OllamaClient`, `RdctlClient`, `KnipClient` and `LintSuiteClient` have zero users.
  - Together with `di/container.ts` (181) and `di/types.ts` (162), this DI layer is mainly there for the `tests/fakes/*` simulators (about 30 files). Replacing it with direct calls plus PATH stubs (`tests/fixtures/bin/*` already exists) saves about 1k lines.
- **Other large files.**
  - `lib/spawn.ts` (573): five retry/redaction/handoff variants, each with its own test; clis/wf has its own spawn in `data/processPorts.ts`. Keep one.
  - `data/logs/betterstack.ts` (511): trims to about 200 once only `vm logs <app>` remains.
  - `data/expo/metro.ts` (409).
  - Small duplicates: `sleep` exists in both `lib/sleep.ts` and `metro/forwardLog.ts:90`, and repo-root lookup in both `lib/repoRoot.ts` and `clis/wf/src/entrypoints/cli.ts:67`.

## Which package absorbs which

workflow-cli should absorb clis/wf, because that's much less churn:

- 69 files outside the package reference `clis/workflow-cli`. These include `lefthook.yml` (`bundle/wf.js`), root `package.json` (`"wf"` script), `cloud-init.sh.tftpl`, the agent-shell and logger-cli Dockerfiles, `vm-ops-api/src/scripts/{redeploy,trunkBuild}.ts` and `tooling/cli-bundle` specs.
- clis/wf is referenced only from `cliRegistry.ts`, `cli.ts` and `.agents/skills/cf-work/SKILL.md`.
- workflow-cli already owns the `wf` bin, the committed bundle (2.36 MB, built with `@tooling/cli-bundle`) and the image bakes.
- Moving `clis/wf/src/{domain,data,use-cases}` into `workflow-cli/src` lets `work`, `status`, `worker` and `knowledge` call functions in-process instead of spawning `wf-worker`. clis/wf is ESM built with tsc; esbuild bundles that fine.
- If you still want the `clis/wf` directory name, do it as one final pure `git mv` plus path-rewrite unit.

## Ordered units

Each unit removes or merges one command group.

1. **maestro and the prepare alias.** Files: `use-cases/maestro/*`, `use-cases/lint/maestro.ts`, `data/qa/maestro.ts`, the maestro and `prepare` sections of `cli.ts`, `tests/**/maestro*`, `tests/fakes/maestro.ts`. About 1.9k removed.
2. **The help/introspection group.** Files: `use-cases/{help,completion,interactive,debug}`, `targets.ts`, `doctor.ts`, `data/qa/doctor.ts`, and their tests. About 2.3k.
3. **report-health and artifacts.** Files: `use-cases/report/*`, `use-cases/artifacts/*`, `data/ordsHealthReports`, and their tests. About 2.6k.
4. **goal, metrics and flow-test.** Files: `use-cases/{goal,metrics,flowTest}`, and their tests. About 2.2k.
5. **edit-stats, `status all` and `metro dev-bridge`.** Files: `use-cases/editStats`, `data/editStatsOrds`, `status-all.ts`, `metro/devBridge.ts`. About 0.9k.
6. **agent-shell becomes `vm exec` and `deploy run`.** Files: `use-cases/agentShell/*` and the agent-shell section of `cli.ts`; drop `pty` and `e2eOnVm`. About 1.4k.
7. **logs becomes `vm logs <app>`.** Files: `use-cases/logs/*`, `data/logs/betterstack.ts`. About 0.5k.
8. **infra becomes `deploy`; tailscale and tunnel are re-homed.** Files: `use-cases/infra/*`, `infraCommands.ts`, `data/infra`, `contracts/infra`. Also update `.agents/hooks/session-heal.sh` and the greenfield-p1 runbook. About 1.2k.
9. **runbook and opencode fold into `dispatch`; gemini folds into `knowledge`.** Files: `use-cases/runbook/*`, `data/agent`, `regenerateGeminiDocs.ts`, `syncGeminiDoc.ts`, `dispatchCommands.ts`, plus the cf-dispatch and gemini-context-sync skills. About 1.5k.
10. **lint and vm precommit become `verify`.** Files: `use-cases/lint/*`, `use-cases/vm/{precommit,affected,sessionScript}.ts`, `lefthook.yml`. About 0.4k. This has to be sequenced with the other agent's vm-jobs removal.
11. **Passthrough renames to `agents sync`, `caveat on|off` and `auth`.** Files: `data/cliRegistry.ts`, `.agents/config/hooks.json`. About 0.1k.
12. **DI and contracts collapse.** Files: `contracts/index.ts`, `di/*`, `tests/fakes/*`. About 1.5k.
13. **Split `cli.ts` and dedupe ios.** Files: `entrypoints/cli.ts` into `entrypoints/commands/*`; `ios/launch.ts` and `ios/build-dev.ts`. About 0.6k.
14. **Absorb clis/wf.** Move `clis/wf/src` into `workflow-cli/src`, delete `clis/wf/bin/wf-worker.js` and the worker passthrough, and update the cf-work skill. About 0.3k removed, mostly duplicate glue.
