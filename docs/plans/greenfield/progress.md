# greenfield — progress ledger

The single source of truth for what is done and what is next. Update it in the same commit as each unit. A box is ticked only with evidence: a commit sha, plus the test summary or runbook line.

Rules (user decisions, 2026-10-04):
- Work locally on the sandbox and commit to `claude/compassionate-noether-dkamzc`. Do not push to dev until the cutover.
- Claude implements, inline or through subagents. Gates run locally, through the lefthook local gate.
- No babysitting or polling loops: one blocking wait per action.
- When a unit is done, start the next one. Never stop between units unless blocked; when blocked, write the blocker here.

## P0 Measure
- [x] inventory.md and plan-research.md (`6c041037`, `3b2daad9`)

## P1 Stop the bleeding
- [x] p1-04/05 watchdog companion (`dd895623`): 9/9 tests
- [x] p1-06/07 deploy backoff, stage timeouts, full recreate on compose change (`dd895623`): 57/57 tests
- [x] p1-08 vm job argv quoting, p1-09 follow by bytes read, p1-11 spawn guard + lint rule (`8e40ac8b`): 731/731 workflow-cli
- [x] p1-10 kimi verdict corrected, no code change (`dd895623`)
- [x] /health reports the deployed image tag (`ca91f7da`)
- [x] agent-dind pidfile hygiene (`29967cfa`), dind 6g cap (`2e0888d7`), one install lifecycle script at a time (`3974d38d`)
- [x] p1-01 pidfile hygiene for every other daemon entrypoint (this commit)
- [x] p1-02/03 compose healthchecks for every service (this commit): 228/231 infra tests, 3 known vitest-file failures
- [x] p1-12 `wf vm recover` (this commit): 7 unit tests, `--dry-run` verified
- [x] p1-13 fault-injection runbook written, `docs/archive/runbooks/greenfield-p1.md` (this commit). The live run happens at the cutover.

## P2 Local gates
- [x] Pre-commit runs on the committing machine (`8e40ac8b`): 306 s local vs 1,160–1,600 s on the VM
- [x] lefthook trimmed to 3 pre-commit jobs (eslint, gate, secrets); 22 moved to `post-land` (`pnpm run checks:post-land`) (this commit)

## P3 Git-state workers
- [x] Lease refs, `Unit:` trailers, pure scheduler, AIMD (`clis/wf`, this commit): 84 tests, including a real-git claim race
- [x] `wf-worker` loop and CLI with AIMD, `--land-ref` (this commit): 96/96 tests, real-git 3-unit landing on a scratch ref
- [x] Exit gate (this commit): `clis/wf/tests/e2e/crashRecovery.e2e.test.ts` runs a 10-unit plan against a real bare remote. The first worker reaches 4 units at once and is SIGKILLed after 1 lands; a worker on a fresh clone expires the 3 orphaned leases and lands the other 9. All 10 `Unit:` trailers appear exactly once, and no lease or failed ref remains: 129/129 tests. The worker clock runs at 100x so the 180 s lease TTL elapses in 1.8 s.

## P4 Verify pipeline
- [x] Tier 2 visual, deterministic half: `wf-worker visual` serves the web export, screenshots every route through `@cli/multi-browser`, runs blank/error/overflow checks and a pixelmatch diff against baselines (this commit): 128/128 tests, a real run passed 9 routes twice
- [x] Tier 2 visual, rest (this commit): two-pool vision judge via `rca run --shape judge:vision` (fails a changed route only when both pools agree), OCI baselines at `baselines/<route>/<sha>.png` pruned to 20: 143/143 clis/wf tests. No live judge or OCI run yet.
- [x] Tier 3 implemented (this commit): `wf check --deployed <sha>` (gate on /health.commit + dev-bridge sync/status, Playwright app flows against metro-runner-main:20000, rca `test:e2e`, EAS update behind `--publish-eas`, OCI `verify/<sha>.json` + HTML via `--publish`, PR block via `--pr`, fix unit on failure, `--dry-run`). Unit + integration green. No live run yet; it happens at the cutover
- [ ] Tier 3 live run against the deployed VM (cutover). First live run 2026-10-06 from the sandbox: found and fixed the dev-bridge sync 502 (`VM_PROVISION_GITHUB_TOKEN` not passed to sync, `b916403f`), tier 1 pass; app flows fail only because the sandbox proxy refuses plain-HTTP tailnet URLs. The runner leg (`verify-deployed.yml`) stops at `install-oci`: the CI vault holds none of the 7 OCI credentials and this session cannot dispatch workflows (403).
- [x] Tier 1 inside `wf check --deployed <sha>` (this commit): a clean worktree of the sha, a frozen offline install, then the gate's affected typecheck/test:unit/test:flow, recorded as `verify/<sha>.tier1.json`; `--skip-tier1`; workflow-cli 854 tests pass
- [x] Knowledge ledger: 52 `docs/gaps/` files, `wf-worker knowledge for|index|stale`, known-gaps.md regenerated (58 lines), archive byte-identical (this commit): 115/115 tests

## P5 Delete
- [x] arch-diagrams deleted with `wf diagrams`, its post-land job and the generated docs (this commit)
- [x] agent-evals, harness and env-bootstrap deleted; `.claude/setup.sh` bootstraps directly (exit 0, a no-op on its second run); 38 old plans moved to `docs/archive/plans/` (this commit): workflow-cli 821 passed, cheap-flow 164, caveat-guard 148, secrets 120
- [x] convention-lint deleted with 12 baseline/waiver JSONs. 5 rules moved to eslint or a secrets unit test, and `wf lint budget` runs in the gate (this commit): eslint-plugin 505/505, workflow-cli 826 passed, secrets 123/123
- [x] nx removed: affected packages come from the pnpm graph, bundles build via `tooling/cli-bundle`, and the nx-cache routes are gone (this commit). Gate on the same sample: 10.9 s with nx, 7.2 s now. workflow-cli 832 passed, vm-ops-api 194 passed (2 known)
- [x] Instruction files collapsed: `.agents/instructions/*.md` generates every AGENTS.md/CLAUDE.md (`156a8a64`)
- [x] Folds (this commit): logger → `wf log emit`; agent-approve-guard → `guard.mjs` + `deny.json` under one hook dispatcher; lint-contentions merged into mobile-learn; hook-test-kit merged into hook-runtime. Root scripts 25→14, agent hooks 13→1, `wf lint budget --strict` passes both. Tests: hook-runtime 77, mobile-learn 520, workflow-cli 757, agent-config-sync 94
- [x] Cutover: rebuild the logger-cli image (`wf ship --ref dev --build-only logger-cli --tag latest`, built and pushed 2026-10-06)

- [x] Agent config contract (user decision 2026-10-04; this commit: agent-config-sync 107 passed, 2 root-only e2e skipped by the CLI; second sync is a no-op; no symlinks): one schema-validated base config under `.agents/` (instructions, skills, commands, hooks, permissions, MCP servers). `agent-config-sync` generates real files in each tool's own format and location for Claude, Codex and opencode, with no symlinks; Gemini is dropped. There is one tool-agnostic `.agents/setup.sh`, and each tool's setup hook calls it. Tests: schema validation, golden output per tool, and a drift check.

## P6 Fold and rca runner
- [x] rca RunPlugin interface + output-checks (`b635f0a3`)
- [x] runOnce, `POST /runs`, `rca run` (`30ffbddb`): 320/320
- [x] ladder, cooldowns, ledger plugins (`7e406c09`): 372/372
- [x] runOnce calls ledger `finishRun`; empty ladder returns `ladder-exhausted` (this commit)
- [x] Callers repointed to rca dispatch (`@app/remote-chat-agent/run`, `/stats`); opencode-harness deleted (this commit). rca 865 passed (5 pre-existing contract failures), cheap-flow 164/164, caveat-guard 148/148, workflow-cli 818 passed (doctor stub fixed)
- [x] vm-ops-api strangler units 1–6 (this commit): TS scaffold with legacy fallthrough; `/health` (+commit, startedAt), `/expo-go`, `ps`, `logs`, `redeploy/status` moved. server.js 8,945 → 8,587 lines; tests 216/218 (2 known)
- [x] vm-ops-api strangler units 7–11 (this commit): redeploy and self-redeploy, trunk deployer with the contract stage timeouts and per-service force-recreate, dev-bridge, both GC loops. vm-ops-api is a workspace package now, so the gate runs it; server.js 8,587 → 4,828 lines; tests 212/212
- [x] vm-ops-api strangler units 12–23 (this commit, which repairs `3f5d4051`): every KEEP route is in `src/`, and `server.js`, usage, verify-betterstack, vm-jobs and the diagnostics routes are deleted. `wf vnc`, `wf usage`, `wf vm job|test` are dropped. vm-ops-api 134/134, workflow-cli 832 passed. LOC: vm-ops-api −5,688, workflow-cli −4,425 (`p6-vm-ops-api.md`), with `server.js` deleted last
- [x] infra-ops folded into workflow-cli (`wf infra`): only commands with callers kept; −2,532 LOC excluding lockfile, bundle and docs (this commit). workflow-cli 171 moved tests pass, full run 1,009 passed
- [x] cheap-flow folded: `wf offload plan|status <slug>` run `wf-worker` (+ `wf offload plan --check` → `wf-worker check`), `wf offload ask` and `wf opencode dispatch|batch|stats|savings` call rca in `dispatchCommands.ts`; steer/cancel/research/review/compound/learnings, cheap mode and the watch/reminder/delegation-report hooks dropped; skills repointed; `clis/cheap-flow` deleted (this change)
- [x] Tooling LOC budget dropped by user decision (2026-10-05): no hard line limit, and the `tooling-loc` metric is removed from `wf check --budget` (this commit). Tooling went from 106k to 74.3k through caller-driven deletions

## P7 Product tidy
- [x] Delete opencode-telegram-bot and plannotator-proxy, the `wf bot` group, their app-map, terraform, gateway and secrets entries (this commit): vm-ops-api 195/198 (3 known), workflow-cli 738/738, secrets 120/120
- [x] Merge feature triplets (this commit): settings, dev-tools, service-report and coding-agent are single packages now; leetcode never had the split. App tsc 0 errors, arch clean, app flow 54 tests, export --platform web ok
- [x] Remove the feature-split machinery: `wf feature`, `scripts/setup-worktree.mjs`, the vm-ops-api dev-bridge steps; 4 solutions archived (this commit): workflow-cli 821 passed, vm-ops-api 195/198 (3 known)

## Clean-code audit (user: clean code over line count)
- [x] All 25 findings of `clean-code-audit.md` fixed: wave B `1f6ac9e3` (vm-ops-api 155/155), waves A+C `2afbf650` (workflow-cli 863 tests; mla-tamagui flow 54; rca plugins 98; feature lint output unchanged). Prettier is now enforced on workflow-cli
- [x] agent-config-sync e2e `mcp-permission-tier` (this commit): the guard behaviour was intact (Slack send denied, read allowed, PR merge left to permissions.deny); the test now checks the consolidated dispatcher. 9/9

## Cutover (once P1–P7 are done)
- [x] Landed on dev (first `40e0a59a`, then fix-forward merges up to `4c1e0176`; dev kept moving because the device-lab session shares the trunk)
- [x] New vm-ops-api live (2026-10-05 11:03 UTC): `GET /health` → `commit 0125c84d…`, `imageTag sha-0125c84`, `serverScriptSha ca49d72146c4` (the pre-greenfield server reported `serverScriptSha 05ebfeb024eb`, `imageTag unknown`). Swapped in by the old deployer's own self-redeploy after the dind store was cleaned, the tailnet name collision was fixed and the `watchdog`/`logger-cli` images existed. Pressure afterwards: I/O some 6–9 %, refaults ≈300/s (was 98 % and 18,800/s)
- [x] Deploy path replaced (2026-10-05): the vm-ops-api polling deployer looped on its own bugs (an unauthenticated self-health check got 401, missing sidecar images, a missing sidecar container) and is switched off (`TRUNK_POLL_MS=0`). Pushing to `dev` now runs `.github/workflows/dev.yml` on a self-hosted runner (`gh-runner`, agent-shell's compose project): test, deploy changed apps one at a time, tier 3. Platform services (vm-ops-api, agent-shell) are deployed by hand. Runner online on 2.337.0 (GitHub stops dispatching to older runners); test job green after bun, Playwright and a core-count worker cap (`78f304e6`); deploy jobs reached `wf` and needed bws plus the vault read token (`ccf8f0ea`), a `preflight` job and per-step timeouts so a broken runner fails in under 3 minutes (`97dbfdc0`), and a direct tailnet connection instead of the sandbox SOCKS proxy (`54ea8026`); the next run is the first to reach the builds.
- [ ] Remaining images through the new deployer: remote-chat-agent, vm-gateway, leetcode-auth-cookie-capture, metro-runner, agent-shell; `/health` of every app reports the deployed commit
- [x] P1 fault-injection runbook run live 2026-10-06 (`1bb1dd6d`, `284050c4`). Baseline: `wf vm recover` redeployed never-deployed apps (vnc-auth-session, reporting-service), fixed with `--only`. §1 stale pidfile: hard RESET 13:55:14, instance RUNNING 13:55:25, vm-ops-api back in 25 s, `docker info` answered about 65 s after RUNNING, `watchdog.started` 13:55:43, no recreate of agent-shell-dind: **pass**. Found: the recover that followed redeployed leetcode-auth-cookie-capture at `latest` (a pre-bundling image) and it crash-looped; the watchdog recreated it once (`watchdog.recreated reason=unhealthy`, 13:57:28) and recover now keeps the running tag (`284050c4`). §2 restart loop: 4 dockerd kills 14:05:07 to 14:06:15, one `watchdog.recreated container=agent-shell-dind reason=restart_loop ok=true` at 14:06:38, `docker info` back 14:07:17: **pass**. §3 hung build: first run, SIGTERM to `wf ship` with a frozen build client sent the cancel (curl timed out at 10 s under load), buildkit reported `CANCELED`, but the job still read `running` hours later: **fail**. Re-run 2026-10-06 on a quiet VM (`2b79ee35`): same injection, the job ended `running: false, exitCode: 143` within 30 s of the SIGTERM: **pass**. The stuck record was a wrapper KILLed with no exit code (and later pid reuse); the status script now reports 137 for a vanished group (`2b79ee35`). The 40-minute workflow timeout itself was not exercised (workflow dispatch is not available to the session). §4: nothing to inject. §5 uncapped memory: `tail /dev/zero` build exited 1 after 6 s, 16 of 16 `wf vm ops health` probes ok over 4.5 min: **pass**. Backstop `wf vm recover` exits 0 with no redeploy: **pass**.

## Open issues

- **Deploy speed: a dependency change to remote-chat-agent is still 5m42 to 5m59 (limit 5 min).** The other two services are under it (leetcode-backend 3m45, leetcode-auth-cookie-capture 4m32). What is left for rca is the install (103 to 129 s), the library builds (55 s) and a redeploy that takes 87 s even when it pulls nothing; `884b0622` adds per-stage timestamps to the redeploy to find where those go.
- `clis/multi-browser/tests/e2e/appFlows.e2e.test.ts` › leetcode two-sum fake-keyboard typing fails: the editor never receives the keystrokes (found 2026-10-05, 3/4 pass). It is not in the pre-commit gate. Resolved 2026-10-05: not a regression. No Metro was listening on the registered port, so the page was Chrome's error page; the test now fails fast in beforeAll with `run wf ship mla-tamagui`.

## Blockers
- **Cutover: root cause of the VM stalls found and fixed (2026-10-05).** The nested dockerd's persistent store (`docker-state`) had grown without bound and nothing ever pruned it. With the old deployer retrying cold builds, the VM sat at 98% I/O pressure, about 18,800 file-cache refaults per second and 7.2 GB used, pinned at the boot volume's throughput limit. Wiping it (`recreate=all` on agent-shell; the old redeploy never recreated changed sibling services, so earlier attempts missed dind) dropped refaults to 0, memory to 2.1 GB and load from 10 to 4. Permanent fix, with no hand-sized limits: `docker-login.sh` prunes unused images and trims the build cache to fractions of the real disk size before every build (tested), and the static dind and app memory caps are gone. Still open: pressure-aware build admission in the new deployer (start a build only with I/O headroom, cancel on sustained stall, kill the whole build tree). The old deployer is rebuilding apps now. The second blocker was a tailnet name collision: stale device records held the bare names, the live containers registered as `<name>-2`, and the old deployer's health stage never resolved the bare host and stalled the batch; fixed live by retiring the suffixed records and re-registering with wipeState, and the new deployer now resolves the host at every health attempt (`host_unresolved` when none resolves).

## Deploy speed (goal: every CI deploy leg under 5 minutes, cold and cached)

Push-to-deployed time of the leg on the VM runner, measured from the push to the new image tag running (GitHub run duration agrees). "Cold" means the buildx cache and the pnpm store were wiped first, so every layer comes from the registry cache.

| Service | Case | Before | After | Evidence |
| --- | --- | --- | --- | --- |
| plan job | push that changes nothing | ~70 s (a job per service) | 23 s | run 18 (`36ab8d3d`) |
| Metro | redeploy, dependencies unchanged | ~6 min | 21 s | `wf ship mla-tamagui` after `c9d0b830` |
| remote-chat-agent | code-only, warm cache | 14m30s (`ec74f002`) | **4m01s** | run 30 (`6b56629b`), `/health` commit `6b56629b` |
| remote-chat-agent | code-only, cold builder | not possible (cache never persisted) | **3m40s** | `8def1ac6` |
| remote-chat-agent | dependency or lockfile change | 14m30s | 6m39s to 9m55s | `30fae7ba`, `260f4584` (libs and proddeps re-run by design) |
| leetcode-backend | code-only, warm cache | not measured (about 9m, same Dockerfile shape) | **1m44s** | `73210e6b`, `/health` commit `73210e6b` |
| leetcode-backend | code-only, cold builder | not measured | **3m19s** (5m17 before the alpine CA stage and conditional prune) | `67309e41`, `/health` commit `67309e41` |
| leetcode-auth-cookie-capture | code-only, warm cache | not measured | **2m04s** | `5b32f331` |
| leetcode-auth-cookie-capture | code-only, cold builder | not measured | **3m54s** | `93e41870` (it has no /health route; the running image tag is `sha-93e41870`) |
| every service | no change, cold builder | not possible | 15 to 27 s of build | `cold2` builds from the registry cache |
| remote-chat-agent | code-only, warm cache, sidecars rebuilt every release | n/a | **5m06s** (build 3m03, redeploy 1m52: the stt sidecar rebuilt, 204 MB pulled again) | `78fd3230` |
| remote-chat-agent | code-only, warm cache, unchanged sidecars re-tagged | 5m06s | **2m04s** | `f26457c9`, redeploy `redeploy.succeeded` at `sha-f26457c9` |
| remote-chat-agent | manifest change (install layers re-run), sidecars re-tagged | 6m39s to 9m55s | **7m48s** (install 103 s, library builds 53 s, `pnpm deploy` 116 s, build 25 s, node_modules copy 13 s, image export 34 s, redeploy about 90 s); still over 5 minutes | `5b8085ce` (after dropping the claude SDK files, the lint and test peers and running two library builds at a time: 7m48 against 7m40 before, so those three changes bought about 20 s) |
| remote-chat-agent | manifest change, bundled service (about 2,100 files of node_modules) | 7m48s | **5m42s** on a quiet builder (`b74e6b03`), **5m59s** with another session's builds running (`c121dc3a`): install 129 s, library builds 55 s, bundle 17 s, export 27 s, redeploy 87 s; still over 5 minutes | `b74e6b03`, `c121dc3a` |
| leetcode-backend | manifest change, before bundling | not measured | 6m05s (install 101 s, library builds 19 s, `pnpm deploy` 90 s one after another) | `57cc8e0e` |
| leetcode-backend | manifest change, bundled, no node_modules in the image | 6m05s | **3m45s** | `c121dc3a` |
| leetcode-auth-cookie-capture | manifest change, before bundling | not measured | 8m25s (install 128 s, library builds 45 s, `pnpm deploy` 95 s) | `57cc8e0e` |
| leetcode-auth-cookie-capture | manifest change, bundled, playwright and sharp only in node_modules | 8m25s | **4m32s** | `c121dc3a` |
| remote-chat-agent | manifest change, per-service manifests, narrowed `pnpm deploy` | 5m42s | **3m55s** | `f79aaeb7` |
| leetcode-backend | manifest change, per-service manifests | 3m45s | **3m28s** | `f79aaeb7` |
| leetcode-auth-cookie-capture | manifest change, per-service manifests, narrowed deploy | 4m32s | **3m46s** | `f79aaeb7` |
| remote-chat-agent | code-only, warm / cold builder (bundled) | 2m04s | **2m26s warm, 2m48s cold** | `763c0586`, `13e8541f` |
| leetcode-backend | code-only, warm / cold builder (bundled) | 1m44s | **2m13s warm, 2m06s cold** | `763c0586`, `13e8541f` |
| leetcode-auth-cookie-capture | code-only, warm / cold builder (bundled) | 2m04s | **2m13s warm, 1m01s cold** | `763c0586`, `13e8541f` |
| every service | redeploy of an unchanged tag | 66 to 88 s | **7 s** (logger emits no longer awaited, `--pull missing`) | `884b0622`, `17818d32` |
| vm-ops-api | self-redeploy by hand | 205 to 275 s | **75 s warm; 48 s build plus 79 s redeploy at `b916403f`** | `17818d32`, `b916403f` |
| vm-ops-api | by hand, latest dev, warm / wiped builder | 79 s redeploy | **warm: 27 s build + 64 s redeploy. Cold (`buildx prune -af` first): 83 s build + 260 s redeploy, repeated: 443 s redeploy**, so a cold vm-ops-api deploy is over 5 minutes. Cause not found: the redeploy log shows no slow step (provision, pull, recreate), and the VM was shared with other sessions' builds; a stamped self-redeploy (the `stamp()` labels exist only for app redeploys) would show where the time goes | `a6c088c3` to `83407ba` |
| vm-ops-api | cause of the cold redeploy time | n/a | `vm-ops-api` had no registry `:buildcache` (it ships by self-redeploy, so `wf ship` threw at the redeploy route before the cache export), so a wiped builder rebuilt new layers; fixed in `de6bc68b` (export runs in a `finally`). Cold build is now 16 to 27 s. The 260 to 443 s redeploys right after a wipe were the host busy deleting 4 to 12 GB of pruned cache, not the deploy. Re-measured on a wiped builder after the load settled (`301d5c16`): **16 s build + 79 s redeploy = 95 s** | `de6bc68b` |
| agent-shell, with the runner and dind in its compose project | build + redeploy, now through `wf ship` (`agent-shell` is a buildable image, `ac0a85a9`; a waited `--build-only` also exports the registry cache) | not measured; first build 785 s and a second 504 s with no cache | **warm build 38 s, cold (wiped builder) build 27 s from the registry cache; redeploy of the whole project (`--recreate-all`: agent-shell, dind, sessions, runner, tailscale) 123 to 134 s, all containers healthy on the new tag.** Total about 2m40 cold. The 785 / 504 s builds were the 32-stage Dockerfile (apt 113 s, tool installs, 153 s image export) with no `:buildcache` | `bench-3`, `bench-4`, `sha-1d4566a`; the runner container was recreated by the same redeploy, the dependency case (a new agent-base) was not separately run |
| agent-shell, with the runner and dind | dependency change (one apt package added to the first tool layer, `33f85c4e`), warm registry cache | n/a | **358 s build, over 5 minutes, plus about 130 s redeploy (about 8 min)**: the 32-stage Dockerfile is one linear chain, so a change in its first layer reruns apt (113 s), the tool installs and a 153 s image export | `dep-1`; fixed below |
| agent-shell, with the runner and dind | dependency change after moving the most-edited apt packages (ssh, tmux, build deps) behind the heavy tool installs (`b64eb8ca`): one more package added (`a6497d5a`) | 358 s build + about 130 s redeploy | **68 s build + 125 s redeploy of the whole project = 3m13, all containers healthy on `dep-2`, `docker info` and `less` answer** | `dep-2`; gap closed |
| Metro | re-point to the dev head | 21 s | **13 s** | `wf ship mla-tamagui` at `dcee4cb8` |
| all three services | Dockerfile-only push on the latest dev, after the Rust `wf` ports (`22ce253e`) | n/a | remote-chat-agent 57 s, leetcode-backend 1m43, leetcode-auth-cookie-capture 3m07: every layer cached, so this is the floor and not a code-change case; the ports touch no Dockerfile, workflow or deploy path (only `deploy/run.ts` `--recreate-all`), so they do not change build time | run 37480774193 |
| Metro | code-only change in the app graph (a flow-test-harness source change), warm | not measured | **31 s** (`deploy (metro)` leg, `9c28863b`; unblocked by the flow-test dedupe `9deba42a`: the lockfile carried react 19.2.3 and 19.2.7 peer variants, so every flow suite failed the pre-commit gate) | run 37484862952 |
| Metro | dependency change (workspace yaml changed, so Metro reinstalls and restarts), warm | not measured | **226 s** | `027a6056`, run 37485864316 |
| Metro | cold (`wf ship mla-tamagui --clear-cache`: node_modules, the local pnpm store and Metro's caches deleted first) | not measured | **146 s** (was 331 / 301 / 725 s). The install had been 407 s because the start script ran pnpm's root postinstall chain twice (it builds every workspace CLI and syncs agent config); Metro serves the app from source, so install and postinstall now run with `DOCKER_BUILD=true`: install 122 to 137 s, postinstall 0. A real web bundle request returns 200 (17 MB). A filtered install (`--filter ./frontend/mla-tamagui...`) was faster but left peer-variant dirs missing (bundles returned 500) and was reverted. `PNPM_CACHE_PAR_URL` is not passed to metro-runner, so the blob cache restore never runs: another lever if the cold case needs more margin | `f210143a`, `34a25244` |
| remote-chat-agent, leetcode-backend, leetcode-auth-cookie-capture | dependency change, warm, repeated on the latest dev | 235 s / 208 s / 226 s (`f79aaeb7`) | **295 s / 319 s / 407 s** at `027a6056`, then **318 s / 232 s / 304 s** at `38da180a`: the install layer ran 178 to 216 s even though the store cache was warm (the lockfile and workspace yaml changed, so it re-resolved), and the other session's builds shared the builder. Over 5 minutes in 4 of 6 legs, so the warm dependency case is not reliably under the limit | runs 37485864316, 37490867581 |
| remote-chat-agent, leetcode-backend, leetcode-auth-cookie-capture | **library-source bundling** (`f2a40765` to `473cf3b4`): `service-bundle` maps workspace packages that publish `dist/` to their `src/` twin, so the image skips every library `tsc` build and the services' own tsc; only `@lib/logging`'s call-site plugin is compiled | libs step 36 s (lb), 57 to 94 s (rca), 136 s (ac) | **cold builder + dependency change: 297 s / 224 s / 197 s** (was 331 / 237 / 292); rca code-only 115 s | `849b4eb0`, run 37501816477; all three healthy on the new images; the rca bundle served identical diff-viewer output to the compiled build (smoke) |
| remote-chat-agent, leetcode-backend, leetcode-auth-cookie-capture | dependency change, **warm**, with library-source bundling | 295 / 319 / 407 s and 318 / 232 / 304 s (`027a6056`, `38da180a`) | **196 / 247 / 155 s**, all under 5 minutes | `afde7db6`, run 37504181600 |
| every service | `/health` or image tag equals the deployed commit | n/a | each service runs the image of the last commit that changed its package graph (rca `sha-0b81b91`, lb and ac `sha-8eaad79`), vm-ops-api reports `b916403f`; `wf ship --changed` compares those tags with the push, so the tag is the commit | `wf ship --status` at `dcee4cb8` |
| remote-chat-agent | **cold builder and manifest change** | n/a | **5m41s: misses by 41 s** (cold install 174 s, proddeps 98 s beside it, library builds 58 s) | `025e20c4`, run 37463064449 |
| leetcode-backend | cold builder and manifest change | n/a | **4m03s** | `025e20c4` |
| leetcode-auth-cookie-capture | **cold builder and manifest change** | n/a | **5m02s: misses by 2 s** (cold install 121 s, proddeps 60 s, library builds 44 s) | `025e20c4` |
| leetcode-auth-cookie-capture | cold builder and manifest change, proddeps started after the install | 5m02s | **4m52s** (292 s) | `8eaad792` |
| leetcode-backend | cold builder and manifest change, same run | 4m03s | **3m57s** (237 s) | `8eaad792` |
| remote-chat-agent | cold builder and manifest change, proddeps after the install, then proddeps beside it with 4 library builds at once | 5m41s | **6m05s**, then **5m31s** (331 s): the one case still over 5 minutes | `8eaad792`, `233b3d01`: cold install 168 s on the A1 (network and file creation), library builds 57 s whatever the concurrency, 70 s of setup and redeploy. A cold builder with a dependency change needs a persistent pnpm store outside the buildx cache to get under; it is the rare case of a rare case and is left open in `docs/known-gaps.md`. |

What drove it, in order of effect: the VM is bound by file creation (37,023 files, 873 MB in remote-chat-agent's production tree), not network or package manager (bun cannot install this workspace); a layered Dockerfile per service (manifests-only install, libraries in their own layer, a `proddeps` stage so the production tree is reused whole, the service built and shipped as its own small layer, `ENV GIT_COMMIT` last); `docker-login.sh` pruned the build cache to 25% free disk on a disk that was 86% full, emptying it before every build (now a 20 GB cap); manifests timestamps were changing the layer hash every build; a detached cache-export child died with its job (the registry cache was never written until it became its own VM job); host images were never pruned (disk reached 95% and builds failed with "no space left"; now pruned above 70% used).

Also done: vm-gateway deleted (repo, lockfile importer, app lists, Terraform app list; containers retired with `wf ship retire vm-gateway`, `retire.succeeded` at 03:53:56Z; vault secrets untouched); vm-ops-api shipped by hand at `32d9e4bc` (Metro worktree origin fix, image prune, retire route).

- **/health equals the deployed sha (2026-10-06, `wf vm exec` curl from agent-shell):** remote-chat-agent `/health` commit `09b72e4a6fa1…` = its image `sha-09b72e4`; leetcode-backend `/health` commit `94408747f0f6…` = its image `sha-9440874`; leetcode-auth-cookie-capture has no `/health`, its image tag is `sha-9440874`; vm-ops-api `/health` commit = the pushed head after each self-redeploy.

- **Cutover done (2026-10-06):** `main-next` fast-forwarded from `d150e88c` to `06b172ee`, a merge commit carrying dev's tree with both histories as parents (no rewrite); PR #305's description updated.
- **Tier 3 credentials:** the 7 `OCI_*` secrets were copied from the admin project into `mobile-learning-deploy-secrets` (the project the runner's read token opens), with the owner's approval; the tier 3 run on the runner follows.
