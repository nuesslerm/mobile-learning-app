<!-- Drafted 19 Nov from a six-agent review of every not-offered T3 and Orca row and of the two desktop apps; the decisions files stay the source of truth for each row. -->
# Plan: offer the most rows and serve the desktop apps

This plan is built from the four reviewer assessments and the desktop analysis. I did not re-check every claim. Two spot checks held: `clis/rca-exec/src/hosted/plan.rs:325` already calls `repos/{r}/assignees`, and Orca has **8** `rpc:speech.*` phone rows in `docs/plans/rca-parity/coverage/orca-decisions.json`, not 9. The T3 reviewer's tally of 31 new-capability rows is corrected to **33** below.

## 1. Summary (phone rows not served today)

| App | Rows in scope | Offer now (no new executor action) | Offer with a new capability | Needs owner account, credential or device | Keep not offered |
|---|---|---|---|---|---|
| Orca | 151 (not-offered plus excluded) | 2 | 138 | 27 of the 138 (7 GitHub project-scope rows, 17 Linear/Jira, 2 push to a real phone, 1 Codex credit proof) | 11 |
| T3 | 47 | 4 (mapper only) | 33 (8 of them device-host rows) | 6 | 4 |
| Orca desktop (scan, upper bound) | +577 | a few (tabs, scrollback, host probes) | about 446 in principle | about 100 (accounts 34, trackers 54 incl. ui.*Jira, artifacts and connectors 14) | about 131 |
| T3 desktop (scan, upper bound) | +109 | about 9 already answered but not tracked | 42 | 0 | 67 |

How the rows are placed (Orca / T3): phase 0: 2 / 0; phase 1: 29 / 12; phase 2: 45 / 5; phase 3: 23 / 8; phase 4 (credentials): 36 / 6; phase 5 (desktop): 2 / 0; deferred XL: 3 / 10; owner decision: 0 / 2; keep: 11 / 4.

## 2. Phases

The same proof applies to every unit:
- **Flow test**: Given/When/Then over the mapper's rendered answer.
- **Chain test**: mapper → rca-service → rca-exec, with the scripted CLI, the fake gh/glab, or exec-testkit's fake-devserver.
- **Replay**: a mimic cassette recorded against the real host (the orcad-headless oracle or a pinned `t3 serve`), replayed in CI.

When a row moves to served, its decision entry is removed in the same commit.

### Ranking by rows per effort (S=1, M=2, L=4, XL=8)

| Capability | Rows (Orca + T3) | Effort | Rows per point |
|---|---|---|---|
| Correct stale decisions (listAssignableUsers, skills.discover) | 2 + 0 | XS | highest |
| One shared `speech` service group (over the existing `speech` crate) with map-speech and the T3 voice mapper | 8 + 7 (+3 keyed) | M | 7.5 |
| exec-browser (phone rows) | 17 + 0 (+~70 desktop) | L | 4.3 |
| Linear/Jira tracker clients | 17 | L | 4.3 (credential-gated) |
| hosted.pr mutation extensions | 8 | M | 4 |
| hosted.issue family | 15 (+3 desktop) | L | 3.8 |
| put_temp and scratch files | 6 | M | 3 |
| Remote URL, git index blob, hostedReview composition | 5 | M | 2.5 |
| Server-kept layout | 5 (+15 desktop) | M | 2.5 (10 with desktop) |
| git.ai at the host | 4 | M | 2 |
| GitHub projects with account-scoped routing | 16 | XL | 2 (scope re-consent) |
| Executors as hosts, PR refresh epoch, CI checks | 3 each | M | 1.5 |
| Notifications journal, activity, sleeping terminals | 8 | 3M | 1.3 |
| exec-mcp, host provider-account actions, telemetry, guest share | 1–4 each | L | ≤1 |
| exec-device, relays, orchestration | 10 + 5 + 4 | XL | <1, deferred |

### Phase 0: decisions and coverage plumbing (XS–S)

- **Rows**: Orca `github.listAssignableUsers` (maps to pr.reviewerCandidates; add `--paginate` and avatarUrl) and `skills.discover` (skill_scan with `home:true`).
- **Correct the stale reasons**:
  - hostedReview.create: pr.create already takes body, base, draft, labels and reviewers.
  - addPRReviewCommentReply: the host uses the REST replies route.
  - resolveReviewThread: the number is unused on GitHub.
  - GitLab "#84": glab sign-in is already in hosted.auth.
  - clipboard: the host writes a temp file; it has no system clipboard.
  - aiVault: CLI session files are readable at the host.
  - preflight.check: gh/glab auth is already in sourceControl.discover.
  - T3 updateProvider: the CLIs run at the host now.
  - T3 uploadFeedback: it goes to OpenAI under the person's own account.
  - T3 prepared-run: worktree setup exists.
  - T3 refreshUsageRates: a public LiteLLM table, not reset credits.
  - T3 voice: whisper/Groq exist.
- **Desktop plumbing**:
  - Widen the sparse checkouts: T3 `apps/web apps/desktop`; Orca `src/renderer src/preload`.
  - Add a second client root to `wf mimic coverage`, giving each row `desktop`, `cli` and `permission` columns.
  - Add a client dimension to `{t3,orca}.json` and the decisions files.
- **Proof**: the existing table tests, plus the flow, chain and replay set for the 2 rows.
- **Progress (19 Nov)**:
  - listAssignableUsers served (bf4592d0): one page of 100 assignees with avatarUrl; `--paginate` is not added, since gh prints one JSON array per page.
  - skills.discover served (6dd0ac17). It was M, not XS: the host answers a catalog of 23 home and 12 per-repo roots, so `skill_scan` takes directory lists and reports the ones present; root path, id and label are allowlisted as the host's own state.
  - The stale reasons above are corrected in place in both decisions files (a dated "Revisited 19 Nov" sentence names the phase that serves each row).
  - Desktop plumbing done: the sparse sets add Orca `src/renderer src/preload` and T3 `apps/web/src apps/desktop/src`; `wf mimic coverage` scans them as a second client, marks `desktop` per row (a Client column in the tables) and counts desktop-only rows and IPC channels (Orca 290 and 1289, T3 71 and 140). The `cli` and `permission` columns are left for phase 5, where they decide the order.

### Phase 1: cheap reads and the shared speech group (about 5 M units)

- **Capabilities**:
  - hosted.pr extensions: `hosted_pr_state`, `hosted_pr_review_comment`, `hosted_pr_file_viewed`, `comment_id` on `hosted_pr_reply`, `number` optional for GitHub in resolve, GitLab notes moved to `glab api`, `bodyHTML` in COMMENT_FIELDS.
  - git reads: `git_upstream` returns fetch URLs (userinfo stripped), `git_blob` accepts an index revision, and map-hosted gets a remote parser.
  - Singles: `cli_detect` reports git; group-schedules keeps a run log; exec-fs `put_temp`, `scratch_read` and a per-thread scratch dir; `cli_state_read`; the LiteLLM price table.
  - One **`speech` group** in agent-api: models, store, dictation sessions and transcribe. orca-serve gets map-speech; t3-serve maps the `/voice` routes onto it.
  - t3-serve gets a PR refresh epoch.
- **Rows**:
  - Orca (29): updatePRState, updateMRState, addPRReviewComment, addPRReviewCommentReply, resolveReviewThread, setPRFileViewed (pays off fully after phase 2 details), addMRComment, prComments; repoSlug, hostedReview.getCreationEligibility, hostedReview.create; git.diff, git.remoteCommitUrl; preflight.check, automation.runs; clipboard ×5, agentSession.readVisual; speech ×8.
  - T3 (12): voice status, models, transcribe, downloadModel, cancelModelDownload, selectModel, cloudModels; pullRequests invalidate, reportState, subscribeRefreshes; orchestration.getWorkflowScript; server.refreshUsageRates.
- **Effort**: M overall; every piece is S/M.
- **Proof**: chain tests with fake gh/glab and the tiny whisper model already in the image. Replay against the orcad oracle and pinned t3 serve.

### Phase 2: the big families (L)

- **Capabilities**:
  - **hosted.issue**: search, count, view, details, create, edit, comment and todos, for gh and glab. Consider a persisted `hosted_query` table to cut each further row to XS.
  - **exec-browser**:
    - Chromium is already in the agent-shell image; it is driven through the existing `clis/cdp` crate.
    - A `browser` service group keeps the page registry; orca-serve gets map-browser with the 0x62 frame codec.
    - Cap the pages because of `docs/gaps/vm-memory-saturation.md`.
  - **Notification journal**, the **worktree activity projection** with read markers, and **sleeping terminals**.
  - **git.ai at the host**: one-shot CLI runs that can be cancelled.
  - **Host provider-account actions**: `cli_update`, a one-shot `codex app-server` request, and a Claude account request. These also fill usage.summary.
  - In t3-serve: the **new-worktree launch with preparation**, and a **triage bundle** (needs a `service.logs` read).
- **Rows**:
  - Orca (45): github and gitlab work items ×15; browser ×17; notifications getMissedSince, subscribe, unsubscribe; worktree.ps; agentActivity clear and setRead; worktree sleep and activate; git.ai ×4; accounts.consumeCodexResetCredit (proof needs the owner's credit).
  - T3 (5): prepared-run.retry, prepareTriage, provider.uploadFeedback, provider.consumeResetCredit, server.updateProvider.
- **Effort**: L. The issue family and the browser are each about a week.
- **Progress (20 Nov, notification journal, worktree activity, sleeping terminals)**:
  - Service state: `terminal.sleep`, `terminal.wake` and `terminal.sleeping` close a workspace's shells and keep name, title, cwd and size in a file on the state volume, so a sleep survives a restart. `service.notifications` (`notification.since`, `notification.subscribe`) is a journal of finished, failed and waiting turns, numbered within an epoch that is new for each run, as the recorded host's buffer is. `service.panes` (`pane.list`, `pane.mark`) keeps each thread's state with read and clear marks, also on the volume. Both are fed by an `on_recorded` hook of the thread engine.
  - Orca rows served in `map-workspace/src/activity.rs`: `notifications.getMissedSince`, `.subscribe`, `.unsubscribe`, `agentActivity.setRead`, `.clear`, `worktree.ps`, `worktree.sleep`, `worktree.activate`. The push rows (`registerPush`, `testPush`, `unregisterPush`) stay not offered; no T3 row sits over these groups (`server.reportClientActivity` and `pullRequests.activity` were already served).
  - Known differences from the recorded host: the recorded host has no desktop window, so it answers `agentActivity.*` with `agent_activity_needs_desktop_window`; this host keeps the verdicts itself. `worktree.ps` leaves `preview` empty and answers agent rows from threads (`agentType` null, `prompt` empty).
- **Progress (20 Nov, exec-browser)**:
  - Executor: `exec-browser` (actions `browser_open`, `_goto`, `_history`, `_eval`, `_screenshot`, `_input`, `_close`, `_list`) in `rca-exec`, a headless Chromium driven through `clis/cdp`: one page per key (thread or workspace), at most 3 pages, a page idle for 5 minutes is closed by a reaper thread and the browser stops with its last page. Addresses are http or https only and, unless `RCA_EXEC_BROWSER_ALLOW` names `host:port`, never a loopback, link-local, metadata, private or shared-address host; every request the page makes is checked the same way, names against what they resolve to.
  - Service: the `browser` group (`browser.open`, `goto`, `history`, `eval`, `screenshot`, `input`, `close`, `pages`), registered always and **off by default** (`RCA_BROWSER=1` turns it on): every method answers `BROWSER_DISABLED` until the VM memory cap for Chromium exists (Blocked in #83).
  - Orca: `map-browser` serves 12 rows (goto, reload, back, forward, tabCreate, keypress, keyboardInsertText, mouseMove, mouseDown, mouseUp, mouseClick, mouseWheel) and `screencast.ack`; with the browser off they answer the recorded host's `browser_unavailable`. Not offered: the two dialog rows and `screencast` with its `unsubscribe` (no dialog state, no frame stream). Proof: flow tests of `exec-browser` on a real Chromium and `fake-devserver`, the service's chain through the executor, and the orca chain test `tests/integration/browser.rs`.
- **Progress (20 Nov, provider accounts, the T3 worktree launch, the triage bundle)**:
  - Executor: `cli_update` (`claude update`, `opencode upgrade`; codex has none), `codex_app_server_request` (starts `codex app-server`, handshakes, sends one of `account/read`, `account/rateLimits/read` or the reset-credit spend, stops it) and `claude_account_request` (reads the usage under the login in the executor's home through `curl`, the token on its input, never returned), in `exec-proc` and `rca-exec` (`adapters/provider_account.rs`). Service: `accounts` group `usage.summary` now carries each signed-in provider's windows (`limits`) or why it could not answer (`limits_error`), `provider.resetCredit` spends a credit with an idempotency key and answers the account's outcome, and `provider.update` runs the CLI's update. Orca: `accounts.list` limits from `usage.summary` (kept 60 s), `accounts.consumeCodexResetCredit`, and `accounts.hostSignIn*` (desktop rows) over `provider.auth.*` (`map-accounts/src/signin.rs`); the three `orca` CLI Claude sign-in rows are not registered. The doubles are authored, listed in docs/gaps/rca-provider-account-rows.md. `accounts.consumeCodexResetCredit` counts as served in the Orca coverage on a recording of its validation errors only: the reset-credit method name `account/rateLimitResetCredit/consume` is an unverified guess answered by a scripted double, `expectedScope` is not compared, and the action spends a billing credit on a real Codex, so it stays unproven until a real redemption is recorded (item 3 below).
  - T3: a launch on `workspaceStrategy: worktree` claims the thread, dispatches the first message deferred (the run is `preparing`, carrying `workspacePreparation`), and `Effect::Prepare` makes the worktree (`worktree.create`, which starts `worktree_setup`), records it on the thread, follows the setup and reports with `prepared-run.progress`, `release` and `fail`; `prepared-run.retry` is served (no longer not-offered) and prepares again. `server.prepareTriage` is served from the service's own log (`service.logs`, a new method of the `service` group over an in-memory tail of `application::log`), with no third-party clone (decision 7).
- **Progress (21 Nov, O3b: exec-mcp, secret file, process sampling)**:
  - Executor: `mcp_request` (starts a stdio server named in the workspace's `.mcp.json` with only its declared environment, sends `tools/list`, `tools/call` or `resources/read`, stops it), `proc_sample` (the executor's own process and everything below it, from `/proc`, at most 512) and `secret_file_put` (a new file, mode 0600, never replacing, the value in no result or error). Agent API: `mcp.request`, `process.sample` and `file.putSecret` in `files.edit`.
  - Not built: the tool server's `request_secret`, and the T3 rows over these (`mcpApps.*`, `secrets.answerRequest`, `subscribeResourceTelemetry`), which stay not-offered; reasons in docs/gaps/t3-mcp-secret-guest-telemetry-no-success-shape.md.
- **Proof**: a real headless-Chromium chain test against fake-devserver in CI; browser cassettes from the orcad oracle (it drives Chromium over CDP itself); the scripted CLI implements `update` and the app-server requests.

### Phase 3: session depth and the hosted tail (M each)

- **Capabilities**: CI check rerun and details; auto-merge as its own action; the **server-kept layout** (tabs of kind terminal, editor, diff, markdown and chat); launch settings; executors listed as SSH targets; turn interruption causes with usage-limit auto-continue; the host CLI session vault with `thread.import`; terminal-artifact grants (opt-in flag, security review); `provider.subscribe`; **exec-mcp** with per-thread model context; tool-server `request_secret` with `secret_file_put`; `proc_sample` and `proc_watch`; guest share in t3-serve.
- **Rows**:
  - Orca (23): rerunPRChecks, prCheckDetails, setPRAutoMerge; files.open, files.openDiff, markdown readTab and saveTab, agentSession.reveal; settings.getAgentLaunch, mutateNativeChatSessionOptions; ssh connect, getState, listTargetSummaries; cancelUsageLimitContinue, continueInterrupted; aiVault listSessions, prepareSessionResume; files terminal-artifact rows ×4; accounts subscribe and unsubscribe.
  - T3 (8): mcpApps ×4, secrets.answerRequest, subscribeResourceTelemetry, guest-message.dismiss, guest-share.close.
- **Proof**: flow and chain tests per unit. Layout rows also get a session.tabs flow test. MCP uses a scripted stdio MCP server.

### Phase 4: rows gated on credentials and accounts

- **Capabilities**:
  - Account-scoped hosted routing (a request with no workspace goes to an executor whose gh is signed in) and the hosted.project family. Sign-in adds `--scopes=project,read:org`.
  - Tracker clients for Linear GraphQL and Jira REST v3. Tokens stay at the host or in a service secret store, with registry.json entries first.
  - A push gateway client (host-key proof to push.onorca.dev, durable unregister outbox).
  - T3 cloud speech keys.
  - The T3 peer-link client, with the `request_environment_link` tool.
- **Rows**:
  - Orca (36): github.project.* ×16; linear ×12; jira ×5; notifications registerPush, testPush, unregisterPush.
  - T3 (6): voice cloudKeys, setCloudKey, transcribeCloud; peerLinks list, link, answerRequest.
- **Effort**: projects XL; trackers L; push L; peer links XL.
- **Proof**: chain tests against recorded API fixtures. Live recordings need the owner's credentials once; push needs a real phone.

### Phase 5: desktop transport (see section 5)

- **Phone rows**: Orca terminal.getAutoRestoreFit and setAutoRestoreFit (meaningful only with a desktop client).
- **Desktop rows**: serve them by family, in this order: tabs and layout state, workspace core, browser automation, trackers, accounts and skills.

### Deferred (stay not offered with the reason updated; revisit later)

- exec-device with an executor HTTP tunnel: T3 device ×8, Orca desktop emulator ×20. XL, proof only from the CI Android job.
- Self-hosted relay director: Orca pairing.getEndpoints, pairing.provisionRelay and the relay-resume HTTP row. Low value, because orca-serve is directly reachable.
- T3 connect.linkProof and connect.relayConfig: a third-party relay.

## 3. Needs from the owner

Status 19 Nov: the owner consented to item 1 (the GitHub project scope). Items 2 to 10 are blocked on the owner's input; the rows behind them stay not offered until it arrives (issue #83, Blocked).

1. **GitHub project scope** (consented 19 Nov): re-consent gh once with `project,read:org` through the sourceControl.signInStart link. Unlocks 16 project rows.
2. **Linear API key** and **Atlassian site, email and API token**. Also decide where they live (executor host or service secret store). jira.connect is not on the phone allowlist, so the token is set by a `wf` verb or by the desktop.
3. **Codex/ChatGPT and Claude accounts with banked reset credits**, only to record a real redemption. Builds and tests run against scripted app-servers without them.
4. **A real phone** for registerPush and testPush, and approval to send notification payloads through Orca's push.onorca.dev gateway.
5. **OpenAI, xAI or OpenRouter key** for T3 cloud speech, and approval of a phone-set runtime key store outside `registry.json`.
6. **A peer environment and its pairing code** (for example the owner's T3 desktop server) for peerLinks.
7. **Decisions**:
   - Phone-triggered redeploy for T3 updateServer and updateServerWithProgress: it conflicts with "production ships only by a push to dev", so the default is not offered.
   - Re-confirm uploadFeedback now that its destination is known: OpenAI, the person's own account.
   - Whether "true empty" answers count as functional (Orca layout.getRemoteSnapshots `{snapshots:{}}`, `host.wsl` and `pwsh` false on a Linux executor).
8. **Security sign-off**:
   - Terminal-artifact grants that read outside the workspace root.
   - Default grants for a runtime-paired Orca desktop: host-admin and workspace amount to running commands as the host user.
9. **VM memory budget** for headless Chromium (a page cap).
10. **Optional**: a T3 Connect account and an Orca Cloud account, only if third-party relays are ever wanted. The recommendation is not to.

## 4. Keep not offered

| Row(s) | Reason |
|---|---|
| Orca HTTP rows (6): app-store update, GitHub release ×2, favicon, diagnostics submission, DNS probe | The phone calls the third party itself; no host is involved, and it already works. |
| Orca orchestration.partyLocation, partyOpen, workerAutoClose, workerKeepOpen | rca has no coordinator/worker database; serving them needs an XL orchestration model. |
| Orca agentSession.threadGoal | Codex app-server thread goals; rca runs Codex per turn and has no app-server session. |
| T3 server.commitDesktopUpdate | Only for the server embedded in T3's Electron app; t3-serve is never that server. |
| T3 provider.chatgpt.import-profile | The tokens belong to T3's own OAuth client, which the host Codex CLI cannot refresh, and importing them moves a credential through the mapper. |
| T3 provider.chatgpt.reconnect-profile, provider.auth.removeMethod | rca keeps no ChatGPT client registrations; login and logout are already served. |
| T3 server.updateServer ×2 (default, unless the owner decides otherwise) | rca deploys only through CI on a push to dev. |
| Desktop: Orca computer.* ×15 | They control the host's GUI desktop; the executor is headless. |
| Desktop: Orca plugins.* | rca has no Orca plugin runtime. |
| Desktop: Orca managedServer, managedRuntime, orcad, terminalDaemon, wakeRecipeVm, updater.download and install | Orca install administration; rca is deployed by CI. |
| Desktop: Orca sidebarInputs.*, sidebarSession.begin | Behind a feature flag. |
| Desktop: Orca WSL and Windows helpers, startWslHeldPanes | Windows only. |
| Desktop: Orca browser.profileImportFromBrowser, profileDetectBrowsers | The executor has no installed browser to import from. |
| Desktop: Orca `pairing-admin` methods | Never allowed for a paired client. |
| Desktop: Orca agentTeams, viewClaudeAgent, readKittyFile | No equivalent in rca. |
| Desktop: T3, 67 rows | 11 local to the machine (host-resource rows may move later through proc_sample), 35 install/cloud/relay/ACP/voice admin, 8 mcpOAuth, 13 internal commands. |

### Conflicts between inputs, resolved

- **files.open, files.openDiff, markdown tabs, agentSession.reveal**: the desktop analysis keeps them not offered; reviewer 2 serves them through the server-kept layout.
  - **Resolution: serve them.** When a phone is paired directly, orca-serve is the host, and the layout is real state shown by session.tabs and by a paired desktop.
- **mcpApps.***: the desktop analysis keeps them; reviewer 4 serves them through exec-mcp. **Resolution: serve them** (phase 3).
- **ssh.***:
  - The 3 phone read rows are served as executors-as-hosts.
  - The desktop's ssh mutation rows stay not offered.
- **settings.mutateAgentLaunch**: it becomes servable once launch settings exist (phase 3).
- **speech/audio naming**: both reviewers proposed a group over the same `speech` crate. Build one group with two mappers.

## 5. Desktop apps

**The approach**:
- Keep the real Electron apps. Make the mappers faithful remote servers for them: desktop → mapper → rca-service `/agent/v1` → rca-exec.
- Both desktops already keep their local IPC (windows, dialogs, updater, SSH/WSL, webviews) inside Electron. That part needs nothing from rca.

**T3 desktop** (M to start):
1. The desktop adds t3-serve as a `BearerConnectionProfile`. `setLocalEnvironmentEnabled(false)` lets it run on the remote environment alone.
2. t3-serve already serves the descriptor, auth session, websocket-ticket, pairing-token, browser-session, `/ws`, and CORS `*`.
3. Track the about 9 HTTP rows it already answers.
4. Serve the 42 serviceable rows:
   - auth revokes and subscribeAuthAccess;
   - projects.snapshot and mutate;
   - thread search and snapshot;
   - vcs.init and removeWorktree, clone, star, resolvePullRequest, branch commits;
   - searchContents and listIgnoredFiles;
   - preview reports and stopLocalServer;
   - config and keybindings;
   - webhooks and deliveries;
   - agentSessions.scan and import (shares the phase 3 vault);
   - provider.switch, queued-run.reorder, thread.tag.set.
5. Mark the other 67 not offered with reasons.
6. A hosted https web client needs wss/TLS at the VM.

**Orca desktop** (L):
1. Add `orca-serve pair --scope runtime`. Today `crypto/src/pairing.rs:50` always mints a Mobile offer.
2. Keep a scope and grant record per device, enforced like `rpc-caller-scope.ts`: `desktop-control` only when granted at pairing, `pairing-admin` never.
3. Report runtime protocol version 3 and the capabilities the Electron client checks.
4. Accept `clientKind 'runtime'`.
5. Split the table into a phone view (the allowlist) and a desktop view (registry methods by permission). Today `every_served_method_is_a_phone_row` forbids any non-phone row, and other methods get `METHOD_NOT_FOUND`.

**Orca desktop rows, in order once paired**:
1. Tab and layout state, which Electron may need before its UI unblocks: session.tabs.listAll, subscribeAll, move, setTabProps and the rest; layout.*; readTerminalScrollback. That is 15 rows on the phase 3 layout store.
2. Workspace core: exec-fs copy, rename, remove, fs_watch and chunked save; exec-git cherry-pick, revert, sequencer, switch; exec-proc ports_list and ports_kill; terminal panes over the existing ptys.
3. The desktop-only GitHub/GitLab tail on the issue and CI families.
4. Browser automation on exec-browser. The desktop-hosted page path (`browser.clientHost`, `desktopBrowserBridge`, plus an exec-net tunnel) is L and comes later.
5. Accounts and skills, after host-side logins (#83).

**Proof without Electron (CI cannot drive it)**:
- Each mapper hosts the pinned web build: a new `T3_WEB_DIR` for apps/web, and Orca's `vite.web.config.ts` build served from `webClientRoot`.
- `wf browser` pairs it in a chain test: bearer for T3, runtime-scope E2EE for Orca.
- mimic records the methods the desktop calls on connect. That recording becomes the measured desktop row set, replacing the 109 and 577 upper bounds.

**Risks**:
- The counts stay upper bounds until the renderers are scanned.
- Every pin bump moves the target (T3 0.1.9, Orca 1.4.259).
- The web build lacks `desktopBridge`, so the preview webview and similar features behave differently there than in Electron.
- A runtime-scope grant is equivalent to running commands as the host user. Enforce grants per device and get the owner's sign-off (section 3, item 8).