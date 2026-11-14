# P6: vm-ops-api strangler plan

Route inventory of `infra/oracle-cloud/vm-ops-api/server.js` at `e13d67a7`, followed by the order of the strangler units. Each unit moves one route or loop into the slim TypeScript service, with its tests. The legacy handler serves every path that hasn't moved yet, and `server.js` is deleted last.

Verdicts for the routes the inventory marked KEEP? (decided 2026-10-04, per the user's earlier decisions):
- **opencode-log read/write:** KEEP. It is the live dispatch dashboard, and rca and the coding-agent feature use it.
- **vm-jobs routes and launcher:** DROP. `wf-worker` (P3) replaces them; the `wf vm job` callers and the cheap-flow VM runs go with the cheap-flow fold.
- **bootstrap-token, agent-shell/credentials:** KEEP. Both are part of the auth-capture path (vnc-auth-capture stays).
- **Desktop/VNC routes:** KEEP only `desktop-resize`, which vnc-auth-capture's desktop-launcher calls. `wf vnc` and every other desktop route are dropped.

I've built the inventory from server.js (8,945 lines), but not every verdict follows strictly from the contract. Six live-caller routes and the vm-jobs launcher are marked **KEEP?**: they have callers today, but the P6 contract doesn't clearly need them. The `wf vnc` desktop routes are DROP even though `wf vnc` still calls them; the P6 surface has no `wf vnc`. Line ranges run from each route's `if` to the line before the next route, so comments above a route count toward the route before it.

Most "Callers" entries refer to `clis/workflow-cli/src/data/vmops/client.ts`. The `wf`-prefixed names below are the `wf` commands that call those client functions.

### Routes (all in server.js; the request handler is 3379–3430)
| Route | Lines | LOC | What it does | Callers | Verdict |
|---|---|---|---|---|---|
| GET /architecture[/*] | 3392–3437 | 46 | Serves static diagrams, no auth | docs only | DROP (arch-diagrams is on the deletion list) |
| GET /expo-go | 3442–3456 (+page 3337–3378) | 57 | Expo Go tap-through page | `wf android install-expo`, PR links | KEEP |
| GET /opencode-log, /api/runs, /api/runs/:id, /:runId | 3463–3535 | 73 | Live dispatch dashboard (read side) | browser, features/coding-agent | KEEP? (`wf offload ask --live`) |
| POST /opencode-log/:id/{start,line,finish} | 3340–3422 | 85 | Dashboard write side | wf `opencodeLog.ts`, remote-chat-agent `vmopsLog.ts` | KEEP? |
| GET /health | 3562–3576 | 15 | Liveness, sha, image tag | wf report health, infra-ops, cloud-init, watchdog | KEEP (add `commit`, `startedAt`) |
| GET /provision-refs | 3578–3592 | 15 | Lists sticky refs | wf report health | KEEP (small) |
| GET/POST /cooldowns | 3593–3630 | 38 | Versioned pool-cooldown ledger | remote-chat-agent `remoteCooldowns.ts`, `poolPreflight.ts` | KEEP (contract keeps cooldowns) |
| POST /usage/invocations, GET /usage/summary | 3632–3680 | 48 | Usage database | `wf usage` | DROP (usage DB is deleted) |
| GET /debug/ping, /debug/diag, /debug/disk | 3682–3797 | 120 | BetterStack probe, fixed diagnostics, disk report | README/compose comments only | DROP |
| GET /services/:app/disk-usage | 3803–3832 | 30 | Per-app disk usage | none | DROP |
| GET/POST /services/:app/funnel | 3838–4070 | 233 | Tailscale funnel read/set | none (diff.ts only mentions funnel in an error string) | DROP |
| GET /services/:app/ps | 4074–4107 | 34 | Container status | wf report health, infra-ops `instanceContainerStatus` | KEEP (`wf vm status`) |
| GET /services/:app/resolv-conf | 4111–4146 | 36 | DNS debugging | none | DROP |
| POST /services/:app/restart | 4150–4190 | 41 | Restarts a container | none (infra-ops says the route doesn't exist) | DROP |
| GET agent-shell/authorized-keys, vnc-auth-session/relay-probe, manual-relay | 4194–4306 | 113 | One-off diagnostics | none | DROP |
| GET /services/:app/{workspace-check, toolchain-check} | 4310–4425 | 116 | Diagnostics | docs only | DROP |
| GET agent-shell/wf-smoke-test | 4429–4461 | 33 | Smoke test | e2e-wf-smoke.sh only | MERGE into exec-script |
| POST exec-script, GET exec-script/:id, POST exec-script/:id/cancel | 4465–4708 | 244 | Async script jobs in agent-shell | wf `execScript`, `installDep`, `runScript`, e2e templates | KEEP (`wf vm exec`) |
| POST/GET vm-jobs, GET vm-jobs/:id, POST vm-jobs/:id/cancel | 4712–4890 | 179 | Job queue | wf `startVmJob`, `pollVmJob`, `listVmJobs`, `cancelVmJob` | KEEP? (replaced by `wf offload worker` later) |
| GET agent-shell/vm-ledger/:name | 4894–4930 | 37 | Reads a ledger file | `wf vm` `sessionScript.ts` | KEEP |
| GET agent-deps-smoke-test, opencode-state-check, mounts, dns-check, login-env-check, heartbeat-test, remote-login-smoke-test, opencode-auth-check | 4934–5272, 5565–5619, 6202–6249 | ~450 | Diagnostics | none (heartbeat.sh comment only) | DROP |
| GET /services/:app/verify-betterstack | 5125–5156 (+verify-betterstack.js 170) | 205 | Log-pipeline probe | none (the watchdog has its own) | DROP |
| GET\|POST metro-runner/dev-bridge/{start,sync,stop,status,logs,procs,bundle-check,reset} | 5276–5442 | 167 | Dev-bridge Metro control | wf `metro/devBridge.ts`, `link.ts`, multi-browser `devBridge.ts` | KEEP start/sync/stop/status/logs; DROP procs/bundle-check/reset |
| GET dev-bridge/branches, POST selfheal | 5446–5517 | 72 | Lists branches; manual self-heal | none | DROP |
| POST dev-bridge/gc | 5521–5561 | 41 | Manual GC | none | MERGE into the dev-bridge GC loop |
| desktop-debug, vnc-auth-ttl, desktop-browser-env, -force-resize, -emulate, -page-info, -resize, screenshot, desktop-input | 5623–6198, 6253–6473 | ~800 | VNC desktop control | `wf vnc *`, vnc-auth-capture `desktop-launcher` | DROP (no `wf vnc` in P6; remove the CLI first) |
| GET agent-shell/artifact | 6477–6528 | 52 | Fetches an exec output file | wf `fetchExecArtifact`, runbook `runOnVm` | KEEP |
| GET /services/:app/logs | 6532–6639 | 108 | Docker logs, with follow | wf `fetchServiceLogs` | KEEP (`wf vm logs`) |
| POST /redeploy | 6643–6655 | 13 | Self-redeploy | `wf vm ops redeploy-self` | MERGE into the redeploy unit |
| GET /redeploy/status | 6659–6717 | 59 | Self-redeploy result | wf client, cloud-init | KEEP |
| GET /debug/host-dns-check | 6721–6757 | 37 | Diagnostic | none | DROP |
| POST /services/:app/redeploy | 6761–6918 | 158 | Pulls and restarts an app | wf release/redeploy, cloud-init, `redeploy-app.sh`, the deleted `build-and-push-image.sh` (repo root) | KEEP |
| POST /services/:app/bootstrap-token | 6922–7003 | 82 | Writes the BWS token to `.env.local` | `wf vm ops push-bootstrap-token` | KEEP? (secrets) |
| POST /services/:app/stop | 7007–7067 | 61 | Stops a container | none | DROP |
| POST/GET /scripts, GET /scripts/:name | 7071–7108, 7214–7276 | 105 | Staged scripts for manual SSH | none | DROP |
| POST /agent-shell/credentials | 7112–7210 | 99 | Relays CLI login credentials | vnc-auth-capture `agent-shell-relay.ts` | KEEP? (auth capture) |
| GET /session-log | 7280–7336 | 57 | SSH transcript | cloud-init comment only | DROP |

### Background loops
| Loop | Lines | LOC | Verdict |
|---|---|---|---|
| Heartbeat | 7442–7462 | 21 | KEEP |
| Trunk deployer | 7504–8315 | 812 | KEEP (add stage timeouts) |
| Dev-bridge GC | 8318–8372 | 55 | KEEP |
| Session GC | 8374–8420 | 47 | KEEP |
| vm-jobs launcher | 8466–8863 (+vm-jobs.js 290) | ~690 | KEEP? (moves to `wf offload worker`) |
| Usage spool drain | 2108–2260, 2233 (+usage.js 142) | ~290 | DROP |

### Shared helpers the KEEP routes need
- **Config and validators** (64–160, 2677–2720): `isValidImageTag`, `isValidProvisionRef`, `isValidJobId`.
- **Dev-bridge scripts** (165–1220, about 1,050 lines): the start, sync, stop, status, logs and GC scripts, plus `recordDevBridgeManifest` and `parseDevBridgeCommitInfo`. Selfheal, reset, procs and bundle-check can be dropped.
- **Redeploy builders** (1221–1600, 2261–2604): `buildRedeploySiblingArgs`, `buildRedeployStatusSiblingArgs`, `resolveProvisionRef`, `buildProvisionNodeScript`, `buildComposeRedeployCommand`, `buildAppRedeploySiblingArgs`, `renderBootstrapTokenScript`. These also need verify-env.js (360) and append-env-local.js (109).
- **App tables** (1735–1950): `KNOWN_APPS`, `APP_CONTAINER_NAMES`, `appsForChangedPaths`.
- **Sticky refs** (2003–2055) and **cooldown ledger** (2056–2107).
- **Opencode-log store** (1659–1734), only if that route stays.
- **Auth** (2722–2870): `bodyLimitFor`, `readRequestBody`, signing, rotating token, `isAuthorized`.
- **Docker client** (2871–3200): `dockerRequest`, `dockerExec`, `dockerPutArchive`, log demuxer, `listContainers`, `pickAppContainer`, dev-bridge exec tracking.
- **Output** (3243–3336): `sendJson`, `emitBetterstackLog`.

### Tests
| Area | Tests (in vm-ops-api/tests) |
|---|---|
| exec-script | `exec-script-job-id` |
| vm-jobs | `vm-jobs-routes`, `vm-jobs`, `vm-jobs-controller` |
| redeploy | `redeploy-scope`, `provision-ref`, `compose-image-tag` (plus infra-ops integration tests `provision-ref`, `verify-env`, `append-env-local`) |
| Container pick | `container-pick` |
| Cooldowns | `cooldown-ledger` |
| Bootstrap token | `bootstrap-token` |
| Expo Go | `expo-go-link` |
| Dev-bridge | `dev-bridge-start-script` |
| Session GC | `session-gc` |
| Trunk deployer | `trunk-deploy` |
| Packaging | `dockerfile-copies-requires` |
| Usage (dropped) | `usage`, `usage-route` |
| No tests | `/health`, `/ps`, `/logs`, `/artifact`, `/vm-ledger`, `/redeploy/status`, `/agent-shell/credentials`, `/opencode-log` |

### KEEP totals
- **Strict KEEP:** routes about 1,000 LOC, loops 935, helpers about 2,600, side files 469. Total about 5,000 JS LOC.
- **Including KEEP? items:** about 6,100.
- **Dropped:** about 4,500 lines leave server.js.
- **After a TypeScript rewrite** that drops the long comment blocks, expect about 3,500–4,000 LOC.

### Target layout (TypeScript, same `PORT`, env and `node dist/server.js` entrypoint)
```
src/server.ts                 router + pre-auth block + listen
src/http/{auth,body,json}.ts
src/routes/{health,expoGo,redeploy,redeployStatus,ps,logs,execScript,vmJobs,vmLedger,artifact,devBridge,cooldowns,provisionRefs,bootstrapToken,credentials,opencodeLog}.ts
src/domain/{apps,provisionRef,trunkPlan,aimd,validators}.ts      pure functions
src/data/{docker,github,sibling,ledgerFiles,betterstack}.ts
src/scripts/{devBridge,sessionGc,redeploy}.sh.ts               shell templates
src/loops/{trunkDeployer,devBridgeGc,sessionGc,vmJobsLauncher,heartbeat}.ts
```
Until each unit lands, the slim service sends unmatched paths to the legacy handler.

### Ordered units (estimated new LOC)
1. Scaffold, auth/body/json, docker client, proxy-to-legacy fallthrough (~450)
2. /health with `commit` and `startedAt` (~30)
3. /expo-go (~60)
4. /services/:app/ps (~50)
5. /services/:app/logs (~110)
6. /redeploy/status (~70)
7. /services/:app/redeploy plus self /redeploy, with the provision and env builders (~600)
8. Trunk deployer loop with stage timeouts (~650)
9. Dev-bridge start/sync/stop/status/logs plus scripts (~700)
10. Dev-bridge GC loop (~60)
11. Session GC loop (~60)
12. exec-script trio, absorbing wf-smoke-test (~220)
13. agent-shell/artifact (~55)
14. vm-ledger (~40)
15. vm-jobs routes (~180)
16. vm-jobs launcher plus AIMD (~450)
17. /cooldowns (~60)
18. /provision-refs (~20)
19. /agent-shell/credentials (~100)
20. bootstrap-token (~90)
21. opencode-log read/write, if kept (~170)
22. Heartbeat (~25)
23. Delete server.js, usage.js, verify-betterstack.js, the DROP tests and the client calls to dropped routes (about −9,000)

Before unit 23 deletes the desktop routes, remove `wf vnc` and the vnc-auth-capture call to `desktop-resize`.