# RCA mobile parity results

The [case manifest](rca-mobile-parity.json) defines the assertions to automate. Each result below records the strongest evidence available for that assertion. Simulator runs used native T3 Code and Orca Mobile builds on iPhone Simulator, with RCA and both adapters on the Mac. Integration tests used the real adapters with scripted providers.

## Latest coverage by case

The initial matrix and dated sections below preserve earlier observations, including failures that later changes resolved. The original Zagreb workspace also has a checked index of local bundles at `.context/rca-mobile-parity-evidence-index.json`.

| Case | Strongest simulator evidence | Remaining coverage |
| --- | --- | --- |
| `fresh-chat-launch` | T3 automatically sent the pending current-checkout chat after the shell synchronized, then launched a new chat in a worktree based on `origin/dev`. | Native coverage includes Claude in both workspace modes and Codex in the current checkout. |
| `reconnect` | Both apps recovered Claude and OpenCode questions with two queued messages after adapter restarts. Both recovered a pending Codex approval. | Additional interruption timings and physical-network handoff. |
| `queue-lifecycle` | Orca Codex completed native creation, edit, reorder, Stop and Resume. T3 Codex completed creation, edit, Stop and Resume. Orca Claude and OpenCode and T3 OpenCode exercised native queue delivery and steering. | T3 native reorder and the startup interval when the composer disables Send. |
| `approve-deny-question` | Native Codex and OpenCode Allow and Deny on both apps. Native Claude and OpenCode structured questions on both apps. | Cross-turn stale-response rejection through the native clients. |
| `photo-picker` | T3 Claude and Orca Codex sent two images in order and retained the previews after a cold reopen. | Additional provider and app combinations. |
| `model-switch` | Codex, Claude and OpenCode switched models in both apps and retained the queued selection after the composer changed again. OpenCode used two configured MiniMax models. | Deployed host catalog and credentials still need physical-device verification. |
| `skill-load` | OpenCode selected and ran a workspace skill from autocomplete in both apps. Orca Codex passed autocomplete and T3 Codex invoked a typed skill. | Native selection on the other provider combinations. |
| `child-and-background` | Both apps launched child agents and background work for all three providers. Claude, OpenCode and Codex 0.172.0 Stop exited the owned worker PID in the tested active turns. Both apps displayed Claude's detached completion receipt and stdout on the original turn. | Older Codex releases lack the targeted terminal API. T3 OpenCode's first completion fixture relaunched. |
| `history-reopen` | Warm and cold opens in both apps against 96-turn histories, with ordered scroll samples and paired history receipts. | Physical-device latency. |
| `worktree-retirement` | Both apps kept two branches, files and chats isolated and returned to the survivor after removing the clean checkout. T3 required Settle to remove its retired chat from Working. | None for the tested variants. |
| `linux-host-phone` | No end-to-end run. | The physical iPhone appeared briefly, but both deployed adapter health probes failed over the tailnet. |

## Initial provider matrix

The first six rows created Python services and merged into `dev`. Five rows passed all three merge-commit core CI workflows. On #88, backend core passed and CLI and mobile core failed. Later dev revisions passed those core workflows. The detailed interaction traces and screenshots are indexed in `.context/rca-native-check/interaction-evidence/README.md` on the original Mac checkout. [#88](https://github.com/nuesslerm/mobile-learning-app/pull/88), [#89](https://github.com/nuesslerm/mobile-learning-app/pull/89), [#90](https://github.com/nuesslerm/mobile-learning-app/pull/90), [#91](https://github.com/nuesslerm/mobile-learning-app/pull/91), [#92](https://github.com/nuesslerm/mobile-learning-app/pull/92), [#93](https://github.com/nuesslerm/mobile-learning-app/pull/93)

| App | Provider | Adapter and provider proof | Native simulator proof |
| --- | --- | --- | --- |
| T3 | Claude | Steer, queue, child agent and timed command output | Two Photos attachments read correctly, Sonnet to Haiku in-thread, child agent and timed shell completion, and Send now promoted a queued message into the active turn |
| T3 | Codex | Steer, queue, child agent and timed command output | Interaction cases still need native execution |
| T3 | OpenCode | Steer, queue, child agent and timed command output on default model | Alternate model rejected its expired credential |
| Orca | Claude | Steer, queue, child agent and timed command output | Initial workflow showed a command approval and an approval action. Full approval and question case still open |
| Orca | Codex | Steer, queue, child agent, timed command output, alternate model and adapter-staged image | Child agent and timed command started from composer. The app rendered adapter-sent results and showed skill autocomplete |
| Orca | OpenCode | Steer, queue, child agent and timed command output on default model | Alternate model rejected its expired credential |

## 2026-11-24 simulator and integration follow-up

Local evidence is under `.context/rca-parity/` in the original Zagreb workspace. The iPhone 18 Pro Max simulator ran iOS 27.0 with T3 Code 2.3.0 (build 1) and Orca Mobile 0.0.68 (build 1). Local LaunchAgent binaries were already running at the start of the session. Test logs record the integration build, which is a separate run from the Mac service used by the simulator.

| Case | T3 simulator | Orca simulator | Adapter or other evidence |
| --- | --- | --- | --- |
| `reconnect` | Claude turn survived Home and reopen. RCA recorded one start, one completion and the expected Bash output. Pending approval and a populated queue still need a connection-drop test. | Codex turn remained visible after Home and reopen. RCA retained the same turn ID and accepted a second steer after reopen. Pending approval and a populated queue still need a connection-drop test. | Orca integration passed service restart and continue once. |
| `queue-lifecycle` | The native Queue tap returned “already has an active run.” RCA queue was empty and the draft remained. This prevented the edit, move and resume steps in this run. | Two follow-up sends steered the same Codex turn. Holding Send did not expose Queue. Native Stop produced an interrupted turn and `queue.held`. Two-item edit, move and resume still need native evidence. | T3 thread-surface and Orca agent-session tests passed their queue scenarios. Orca's client gates its queue control on `agent-session.queued-messages.v2`, which the tested adapter source does not advertise. |
| `approve-deny-question` | Claude emitted `input.requested` and Codex in Supervised mode emitted `approval.requested`. T3 projected an empty `runtimeRequests` list in both cases. Neither the choices nor Allow and Deny controls appeared. Both test turns were stopped from T3. | Claude's Blue or Green choices remained available after Home and reopen. Selecting Green recorded one `input.answered`, cleared the chooser and completed the same turn with “You chose Green.” The harmless Python command ran without an approval prompt under this host's settings, so this follow-up could not exercise denial. | Both adapters passed scripted approval and question response forwarding. Orca also checked stale answers. |
| `photo-picker` | Prior Claude run identified both images. Cold-reopen persistence and hash checks remain open. | Codex read both images in order after native selection. Both composer thumbnails were visible. Cold reopen restored the reply but omitted the user message thumbnails. | T3 passed the small-file staging and readback case but failed the maximum-size upload with HTTP 400. Orca byte-for-byte image integration failed with a socket reset. |
| `model-switch` | Prior Claude Sonnet to Haiku switch passed. Codex completed on GPT-6.1 Sol, then T3 projected GPT-5 on the next turn and the CLI rejected it. The OpenCode picker offered only `rca default`. Queued model retention remains open. | A fresh Codex thread displayed GPT-6.1 Sol but the CLI rejected its `gpt-5-codex` turn. Explicitly selecting the displayed model let the next prompt complete. Alternate selection and queued model retention still need native assertions. | Orca model catalog and selection integration passed. Live catalog offered GPT-6.1 Sol, GPT-5 Codex and GPT-5 on this run. |
| `skill-load` | Prior run showed no native autocomplete entries after adapter discovery. | Native autocomplete inserted the installed skill, but the send returned to the composer and RCA recorded no new turn. No skill load was observed. | Orca discovery passed. T3 projection decoded workspace skill data in the initial run. |
| `child-and-background` | Prior native Claude child and timed Bash task completed. In this follow-up Claude invoked Agent and Bash, then finished the parent turn saying both were still running. After reopening, the projection had no child rows and the trace had no final stdout marker. Background PID cancellation remains open. | Prior native Codex child and timed task completed. This run exercised stopping a foreground turn. A child or background PID cancellation still needs coverage. | Initial six-pair traces capture CLI tool execution and actual background output. |
| `history-reopen` | Three warm taps showed the known stopped turn in 1.22–1.40 s. Cold tap took 1.43 s, with 7.79 s from launch through observed history. | Three warm taps showed the known reply in 1.02–1.06 s. Cold tap took 1.15 s, with 7.96 s from launch through the reply. | Timings include device automation and accessibility observation. One initial Orca cold probe did not see the picker within 6.31 s. Seeded two-page pagination remains open for both apps. |
| `worktree-retirement` | Two-worktree isolation and picker retirement still need native assertions. | Opened `main` with no tabs and returned to `dev-opencode-orca`, which restored the Claude thread. Cold launch briefly showed retired rows from cache and refreshing removed them. File isolation remains open. | Fresh Orca `worktree.ps` omitted the retired branches. |
| `linux-host-phone` | Blocked for the deployed-host run. | Blocked for the deployed-host run. | Mac probe found neither tailnet hostname and could not resolve the VM ops credential. No physical iPhone was available to the device driver. |

### Integration commands and coverage

```sh
cargo test --manifest-path clis/Cargo.toml -p t3-serve --test integration thread_surface:: -- --test-threads=2
cargo test --manifest-path clis/Cargo.toml -p t3-serve --test integration attachments:: -- --test-threads=2
cargo test --manifest-path clis/Cargo.toml -p orca-serve --test integration -- --test-threads=2
```

T3 thread surface passed 12 of 12. T3 attachment-filtered integration passed 3 of 4. Orca integration passed 18 of 36, including queue, approval, question, catalog and restart recovery. Other Orca failures included the attachment socket reset, missing executor Chromium and replay failures in a checkout whose Orca reference differed from its pin. Keep these integration failures visible alongside the native evidence.

In a local Linux arm64 container at `53775ec2`, T3 passed all 12 `thread_surface::` integration cases and Orca passed all six `agent_sessions::` cases, including the image readback that had failed in the Mac run. The deployed Linux host and the physical network handoff remain unverified.

## T3 runtime-request follow-up

On the same iPhone simulator build, a fresh Claude question displayed Blue and Green. Restarting only the T3 adapter dropped the client connection while the RCA turn stayed active. The request retained its ID after reconnect. Selecting Green in T3 recorded one `input.answered` event, resolved the request and completed the original turn with Green.

Codex in Supervised mode displayed Allow once and Decline for two read-only Python commands. Allow once produced `RCA_PARITY_T3_ALLOW_ONCE` on stdout. Decline resolved the other request without executing its output marker. Both controls cleared after the user acted.

For the combined reconnect case, open a Claude question from T3 and seed two uniquely marked follow-ups through the paired T3 adapter. Wait for both queue IDs at RCA before restarting only the T3 adapter. T3 showed the same question and two queued messages after reconnect. Submitting Green resolved the question and RCA ran both follow-ups once, in order. T3's projection still showed both entries as queued after RCA recorded both completions, leaving the app-side assertion failing. The seeded queue exercises recovery with existing messages. The native Queue composer remains a separate check.

The paired-client integration regression checks pending approval and question projection, the question snapshot after a new socket connects, and resolution after approval, answer and dismissal. The local T3 thread-surface suite passed 12 tests and the RCA adapter flow suite passed 14. Simulator recordings, screenshots and request IDs are under `.context/rca-runtime-requests/` in Zagreb.

The device driver can now see the physical iPhone with both apps installed. The deployed `t3-serve` and `orca-serve` tailnet health probes still fail, so the phone-to-Linux network check remains open.

## T3 queue recovery after adapter restart

Replayed `reconnect` on T3 with Claude and two follow-ups seeded through the paired adapter. The same question and queue IDs survived the socket drop. Submitting Green resolved the question exactly once. RCA started and finished each queued turn once, in A then B order. T3 showed both replies, completed all three runs and removed the queue control without a history refresh. The simulator recording, screenshots and ordered event receipts are under `.context/rca-queue-recovery/` in Zagreb.

The restart regression in `agent_threads::` asserts playback through both queued turns, recovery of a queued turn already in flight, and user-controlled resume of a force-stopped queue. All eight focused flow tests and all twelve thread-surface integration tests passed on Linux. The concurrent Mac checks hit connection resets in the loopback HTTP helper. The serial Mac flow run passed all eight cases.

The seeded native queue controls test used T3's Stop button on a pending question. RCA held both entries across an adapter restart. T3's edit updated the first queued prompt in place. Tapping Send now started that edited entry and released the queue, which ran the second entry in order. Both finished once. The queue menu offered Send now, Edit and Remove. The held entries lacked `queueHeld` in T3's projection, so the dedicated Resume queue control did not appear. Native queue creation and reorder remain open.

Replayed the same reconnect scenario on Orca with Claude. Seeded both messages through paired `agentSession.send` calls with `queue-if-active` and restarted only the Orca adapter. The question and queue remained available. Tapping Green recorded one answer, and the Orca transcript showed both queued replies once in A then B order. RCA recorded matching starts and completions. This run covers reconnect with an existing queue. The native Orca composer queue controls remain a separate check.

## T3 Resume queue after forced stop

On the iPhone 18 Pro Max simulator, open a Claude question to hold the first turn open. Seed two distinguishable follow-ups through the paired adapter and confirm both queue IDs at RCA. Tap Stop on the question, then wait for RCA to report a held queue and T3 to project `queueHeld: true` on both queued runs. Restart only the T3 adapter. Open the native queue panel and tap Resume queue. The panel showed Resume queue after reconnect, RCA recorded one resume and two starts in the original order, and T3 showed both replies once. Both queues cleared. Verdict for `queue-lifecycle` stays partial because the two messages were seeded and reorder was not exercised.

The adapter projects the hold after RCA accepts a forced stop and restores it from RCA's queue state on restart. The Linux focused flow suite passed 9 tests, including recovery for saved rows that predate the fix. All 12 Linux thread-surface integration tests passed. The preceding Mac flow check had 7 passes and one loopback connection reset in the test HTTP helper.

## T3 reconnect with a pending command approval

Use a new Supervised Codex thread on GPT-6.1 Sol. Send a read-only shell command that prints one fixed marker. Wait for Allow once and confirm one pending command request in the paired T3 projection. Seed two distinguishable follow-ups and wait for RCA to list both queue IDs. Restart only the T3 adapter. Confirm the same approval ID and queue IDs, then tap Allow once in T3. RCA recorded one acceptance for that request on the original turn and one stdout event with the expected marker. The original turn and the two queued turns started and completed once in order. T3 showed both follow-up replies, the approval cleared, and the queue emptied.

This passes the forced-disconnect command-approval variant of `reconnect` for T3 with Codex. The equivalent Orca command-approval replay and the full app–provider matrix remain open. The fixture seeds the queue through a paired connection so a harness can run it independently of native queue creation.

## Orca reconnect with a pending command approval

The Orca Codex permission picker showed Host's Agent Permissions as disabled, and a harmless command sent from the composer completed without a prompt under that host default. To exercise approval reconnect without changing host permissions, start one turn with `runtime_mode: approval-required` through RCA in an Orca test chat. Queue two marked follow-ups through the paired adapter. Restart only the Orca adapter, confirm the same approval and queue IDs, then tap Allow in Orca. RCA recorded one matching approval answer and stdout event. Both follow-ups started and finished once in order, and the Orca transcript showed both replies and an empty queue.

This passes the forced-disconnect command-approval variant of `reconnect` on Orca with Codex and an RCA-started fixture. Configuring an approval-required turn from the Orca app remains open.

## Orca photo previews after a cold reopen

Attach the blue/orange swatch first and the green/purple swatch second through Orca's Photos picker. Confirm two composer previews. Send a prompt asking for the colors in order, terminate Orca, relaunch it and reopen the same chat. Codex reported blue, orange, green and purple in order. Both thumbnails rendered after cold reopen. The restored history had two ordered image references. Preview bytes matched the hashes stored by RCA, and the decoded RGB pixels matched the original library images exactly. The uploaded PNGs had different file hashes from the originals.

RCA now includes ordered attachment references in the user message event. Orca uses those references when it rebuilds the transcript. Messages already saved without references have no source for thumbnail replay. The Linux regression failed against the old code and passed after the fix. The Orca agent-session integration suite passed all six cases. This run covers Orca with Codex. Its bundle is under `.context/rca-orca-approval/photo-published/run-12ffe4c675f30c96/`. The T3 follow-up is below.

## T3 queue follow when RCA is unavailable

During the local RCA restart for photo verification, repeated subscription failures left the head of several T3 queues in place. T3 kept taking the same head in the same drain. The Mac counted about 16,000 RCA connections in TIME_WAIT, and loopback requests failed across both adapters and the simulator driver. Suspending the local T3 process stopped the new connections and freed the ports.

T3 now returns from that drain if the same row is still waiting. The regression forces the first subscription to fail before the queued turn starts. It asserts one attempt in that drain, persistence of the waiting row, and successful playback on a later drain without starting another RCA turn. It passed on Mac and the ten Linux agent-thread flow tests passed. After restarting the patched local T3 adapter, RCA, T3 and Orca all returned HTTP 200.

## T3 photo preview after a cold reopen

Select the same two swatches in the same order with T3's Photo Library picker, then send a Claude Sonnet turn and cold-reopen the chat. Both composer images appeared. The staged files matched both original PNG SHA-256 digests, and Claude reported the colors in order. The reopened projection still had both upload IDs, but the thumbnails were blank and both paired `assets.createUrl` requests returned `AssetAttachmentNotFoundError`.

T3 still projects the pending upload IDs on the sent message. The pinned T3 server claims a permanent copy before dispatch so pending-upload cleanup can remove its own copy. The RCA adapter has no equivalent claim step. The end-to-end failing check now has stable assertions for the before and after projection, staged hashes and asset readback.

## T3 photo cleanup and cold reopen follow-up

Repeat the two-photo picker with Claude Sonnet. After send, use a paired connection to call `attachments.delete` for both pending upload IDs, then fully terminate and relaunch T3. Reopen the same chat from the Chats list. Both thumbnails render and both inline photo links open the correct image. The reopened projection preserves the message ID, both attachment IDs and the composer records that bind those links. The agent identified blue, orange, green and purple in order. Both `assets.createUrl` reads return PNG bytes with the original fixtures' SHA-256 digests after the cleanup and after cold reopen.

T3 now protects blobs referenced by a saved message from draft cleanup and persists composer context on the message and user turn item. The regressions confirm that an abandoned upload still deletes, a sent upload can be read after cleanup and a new paired connection, and both launch paths preserve composer context across a T3 restart. The affected Linux suites passed 17 flow and 4 integration tests. This passes `photo-picker` on T3 with Claude Sonnet. The bundle is under `.context/rca-t3-photo/published/run-283ba599e47fa289/`. The other T3 providers still need native coverage.

## T3 creates two queued messages from the composer

Start a timed Codex turn through the paired adapter in an existing T3 chat and wait until the app shows Running sleep. With the turn active, enter each marked follow-up in T3, hold the composer Send control and tap Queue. Confirm both cards in T3 and both IDs in the waiting RCA queue. Tap Stop, open the paused queue and edit the first prompt. The edit retains its ID. Tap Resume queue in T3. RCA recorded two additions, one edit, one force-stop hold, one resume and one completion for each follow-up. T3 displayed the edited first reply and the second reply in order.

This verifies native queue creation, Stop, Edit and Resume with Codex once the provider turn is running. The held queue menu offered Send now, Edit and Remove. Native reorder and queueing before the provider opens remain open.

## T3 Codex queue, model switching and command approvals

The simulator foreground bundle was `com.t3tools.t3code` during these follow-ups. Their first publication identified the app as Orca. The corrected bundles below retain the same recordings and assign all native actions to T3 Code. The separate paired Orca journal reads remain adapter evidence.

- **Queue lifecycle.** Start the timed command from T3's composer and wait for Running sleep. Use the Queue chord twice, Stop, and edit the second card while the queue is paused. Its ID stayed the same. Resume started and completed both follow-ups once in their original order. Dragging the second card up left the order unchanged. Verdict is partial. Evidence is under `.context/rca-t3-codex-queue-corrected/published/run-f6270de46fc90ea1/`.
- **Model switching.** The first send in a new Codex chat matched the displayed GPT-6.1 Sol. In an existing chat, select GPT-5.6 Sol and start a timed turn. Queue the follow-up on that model and switch back to GPT-6.1 Sol while it waits. The provider turn records confirm GPT-5.6 Sol on the timed and queued turns and GPT-6.1 Sol on the next direct send. The queued turn ran once, the expected replies rendered and the queue cleared. Evidence is under `.context/rca-t3-codex-model-switch-corrected/published/run-d95fcc0f71e403a4/`.
- **Command approvals.** Set Supervised and send two harmless commands. Decline recorded one denial with no stdout. Allow once recorded one acceptance and one stdout event on the second turn. T3 cleared both prompts. During the second request only the Orca adapter was restarted, so the unchanged pending approval in the paired Orca history is adapter recovery evidence. The T3 connection stayed up. Evidence is under `.context/rca-t3-codex-approval-corrected/published/run-0ea57ed764b26bad/`.

## Orca queue handoff integration

Orca now advertises queueing and reordering and projects dispatch handoffs in history and subscription replay. The Linux paired-client regression checked reorder, edit, removal, stop, send now, resume and handoff identity after reconnect. All six Orca agent-session integration tests passed. A separate paired Orca session also read both handoffs from the T3 Codex queue fixture. Native Orca queue lifecycle and editing while a turn finishes remain open. RCA's scheduler does not enforce the Orca edit lease.

## Confirmed Orca Codex queue and displayed default

The foreground bundle was `com.stably.orca.mobile` before and after recording. Create a Codex tab in an RCA-registered test worktree and send a timed command without opening the model picker. The Codex turn record matched the displayed GPT-6.1 Sol. Queue two marked replies from the Orca composer. Drag the second card above the first, then tap Stop. RCA recorded one move and held both original cards in the reversed order. Opening Edit withdrew the first-created card and moved its text to the composer. Resume ran the remaining card once, rendered its reply and cleared the queue. A fresh Orca history response linked its submission to the original card ID.

Queue lifecycle remains partial because the edit did not preserve the card. The first-send default passed. Orca's in-thread alternate model and queued model retention remain open. The recording, screenshots, RCA events and provider model IDs are under `.context/rca-orca-confirmed/published/run-b9eb8616709985c7/` in Zagreb.

## Orca Codex loads a skill from slash autocomplete

Register a disposable RCA worktree and add the `rca-parity-compass` fixture under `.agents/skills/`. Open a Codex tab in that worktree with `com.stably.orca.mobile` foreground. Type the slash prefix, select the skill, and ask for the compass fixture marker without writing the marker in the prompt. Codex's turn record contains the expanded skill path and file marker. Orca shows the marker in the reply from the same turn. This passes `skill-load` for Orca with Codex. The fixture, recording, screenshots and filtered provider receipt are under `.context/rca-native-skill/published/run-569e7301fb781322/` in Zagreb.

## Confirmed Orca Codex alternate and queued model

In an existing Orca Codex chat, select GPT-5.6 Sol and start a timed command. Queue a follow-up on that model, then select GPT-6.1 Sol before the command finishes. The queued card retains its ID and GPT-5.6 Sol. Codex's turn records show GPT-5.6 Sol for the timed and queued turns. The next direct turn uses GPT-6.1 Sol. RCA recorded one queued dispatch and the app rendered all expected replies. Together with the verified fresh-chat default, this passes `model-switch` for Orca with Codex. The recording, screenshots and filtered provider receipts are under `.context/rca-orca-confirmed-model/published/run-7e4518c1bb9eb455/` in Zagreb.

## Orca switches worktrees and removes the retired checkout

Register two RCA worktrees of the same repo at different committed revisions. From the Orca picker, open B, start a Codex tab and run a read-only branch, revision and file marker probe. Return to A and run the same probe. Each tab displayed its own branch, revision and marker count. RCA assigned their threads to different workspace IDs. Verify B is clean and idle, then call `worktree.remove` without force. B disappeared from RCA and the Orca picker. Both branch refs remained. A reopened with its prior tabs and transcript. This passes `worktree-retirement` for Orca with Codex.

T3's Add Project refused the same registered external folder because it was outside the adapter's configured project root. Its complete switch-and-retire case still needs a run through T3's own New worktree flow. Recordings, screenshots, tool stdout, branch refs and removal receipts are under `.context/rca-worktree-parity/published/run-b914dcc3fc30fa59/` in Zagreb.

## T3 creates a worktree and invokes a typed skill

Choose New worktree and `origin/dev` in the T3 composer. The new thread ran in the checkout RCA registered. Codex's Git stdout matched the new branch and the checkout's commit. This verifies creation and execution in one T3 worktree. The two-worktree switch and retirement remain open.

RCA discovered a test skill in that worktree. The previous local T3 binary required `providers:manage` for the app's workspace catalog refresh. Rebuilding from `dev`, which includes the read-scoped refresh fix, let a paired request read the skill. The recorded app still showed no suggestion for the slash or dollar prefix. Typing the skill identifier directly worked. Codex's turn record shows it expanded the skill, and T3 displayed the undisclosed marker. The `skill-load` verdict is partial because selection from autocomplete remains open. Evidence is under `.context/rca-t3-native-skill/published/run-072437ac01db2c6e/` in Zagreb.

## Orca edits and reorders two queued messages in place

Start a timed Codex turn from the Orca composer. Queue A and B. Edit A in the inline editor and confirm RCA still lists the same card ID while its edit lease is held. Save the correction and drag B ahead of A. Tap Stop and verify both cards are held in the new order with no queued turn started. Tap Resume. RCA started and finished B and edited A once each, in that order. Orca displayed both replies and cleared the queue. A fresh history read linked both submissions to their original cards. This passes `queue-lifecycle` for Orca with Codex.

RCA now holds the edit lease at the queue scheduler. The Linux paired-client regression lets the active question turn finish during the edit, verifies the old prompt stays queued and releases the card only after saving. It failed on the original adapter and passed with the fix. The Linux checks passed 33 agent API unit, 4 queue flow, 6 contract flow and 7 Orca agent-session integration tests. The native recording, screenshots and event assertions are under `.context/rca-orca-queue-edit/published/run-5124a3b7656ea687/` in Zagreb.

## Cold and warm reopen with seeded 96-turn histories

Both native bundle IDs were verified. Each app used its own fixture with ordinal markers so replay, scroll order and duplicate detection can be automated independently of model output. One real T3 turn supplied the fixture shape. The other 95 turns were synthetic.

| App | Warm reopen (ms) | Cold tap (ms) | Cold launch through first message (ms) | History assertion |
| --- | --- | --- | --- | --- |
| Orca | 1285, 1273, 1332 | 1405 | 7647 | Native replies 052–096 were contiguous across more than two page boundaries. Eight paired history pages reassembled all 96 replies in order. |
| T3 | 1533, 1541, 1576 | 1934 | 7861 | Native scroll reached reply 045, with consecutive ordinals in each sampled window. The paired projection decoded all 192 messages and verified all 96 reply markers in order. |

Timing includes the simulator driver's observation latency. T3 originally displayed Reconnecting and its WebSocket closed after 327,202 bytes of a declared 720,896-byte response. macOS accepted sockets inherited nonblocking mode from the listener. T3 now puts each accepted socket into blocking mode before reading its HTTP request. With the updated adapter, the same response arrived in full. The delayed-reader regression failed on the original Mac build and passed after the change. All 19 targeted T3 socket flow tests passed separately on macOS and Linux. The CI catalog regression traced to an outdated Codex model recording. Updating it to match the advertised models passed all four RCA service catalog flow tests on Linux.

The local RCA launch configuration now persists thread state. A separate real Codex sentinel retained its thread, event and turn IDs through an RCA restart. The recorded initial restart used an in-memory test service and cleared its local test threads.

Bundles in Zagreb are `.context/rca-long-history/published/run-e4c839f2014f4988/` for Orca and `.context/rca-t3-large-history/published/run-5b9ed2affaf4e5f8/` for T3. They contain recordings, screenshots, individual timing measurements and machine-readable ordinal assertions.

## Native child agents, background completion and process cancellation

The Codex completion run passed in both apps. The child counted 17 lines in the fixture and returned to the parent. A separately controlled Python process remained alive through navigation away and reopening the same thread. After the host released it, the process exited and RCA recorded the actual completion stdout once. Orca and T3 displayed the child count and the command result.

The cancellation variant failed in both apps with Codex CLI 0.163.0. Each native Stop action produced one interrupted RCA turn, but the exact fixture PID remained alive. In Orca the Python process was the leader of its own process group. The app server's command termination operation addresses only client-supplied process IDs from a direct command-execution RPC. These model tool commands used no such client ID. The test harness terminated each verified fixture PID after recording the failure. Both cancellation markers were absent. Completion and cancellation are separate results in `.context/rca-native-background/published/run-043fd5873df2d799/`.

Orca with Claude spawned a real Agent, got the same count, and started Bash with the background flag. The parent settled while the Bash PID stayed active and the reopened chat showed its task ID. After release, the provider's original output file contained both markers and exit code zero. The app surfaced completion only after a separate native prompt asked Claude to read that file. Automatic completion delivery and native cancellation are open. Evidence is under `.context/rca-native-background/published/run-9c2c0d48e107314b/`.

## T3 automatic follow-up during provider startup

The pinned mobile client sends automatic delivery with a start-immediately dispatch placeholder. If a run is active and the provider turn has yet to open, T3 now queues automatic follow-ups. An explicit immediate command still receives a refusal. The red-to-green paired test sends the same automatic command twice and verifies one queued run with the same dispatch receipt.

In the native Claude fixture the composer disabled Send in the startup window the test reached. After startup, the follow-up completed. This records an adapter pass and a partial native case. T3 native reorder and delivery through the startup control still need evidence. See `.context/rca-t3-startup-queue/published/run-8273729c8deab53b/`.

The history and catalog push passed [GitHub backend CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45350384909). Its [CLI workflow](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45350384652) exposed the missing Orca edit-lease declaration, test double and stale generated coverage. The follow-up passed 71 Orca mapper flow tests, both coverage checks and 15 T3 orchestration flow tests on Linux.

## OpenCode structured launch and background controls

Orca's OpenCode button originally selected the terminal fallback. The pinned mobile app's older structured path lists only Claude and Codex. Its negotiated route uses `agent.launchReplay` for the first request and includes a reserved session ID. The adapter now offers the negotiated route. It reserves the operation before creating the thread, registers the tab in the worktree layout and returns the same receipt after retries. A failed acknowledgement after the reservation preserves the uncertainty and a retry starts no duplicate turn. Worktree and terminal requests outside this launch path still use the mobile client's fallback. The pre-fix mapper test found the missing receipt. The final Linux checks passed 73 Orca mapper tests and seven paired agent-session integration tests.

From Orca's native picker, OpenCode opened a structured composer on Default in the registered test worktree. A real task agent counted 17 lines. A guarded Bash process remained alive when the chat reopened, exited after release, and produced exactly one recorded stdout completion. In a fresh turn, native Stop produced an interrupted RCA turn and the exact Bash fixture PID exited without creating its completion marker. This passes OpenCode's launch, child, long command and Stop assertions on Orca.

T3 also returned the OpenCode child count, restored the chat and displayed stdout. OpenCode detached the first completion fixture, then relaunched the same script. This makes that completion variant partial. A separate guarded foreground command exited after native Stop with no completion marker, which passes cancellation. Later local T3 builds were installed from the startup repair's committed source and their installed checksums matched the build outputs. The earlier startup-window recording observed the client disabling Send and does not prove a native queued delivery.

Both apps offered only the OpenCode driver default in their native model picker. The alternate model still needs a catalog entry and working provider authorization. Evidence for OpenCode is under `.context/rca-orca-opencode-launch/published/run-2e12af4131a1cb23/`. The preceding changes passed [GitHub CLI CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45352977993) and [GitHub backend CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45352978127).

## Orca sets per-chat approvals and preserves the pending request

In a fresh native Codex chat, open the model picker, choose Permissions, and select Ask for approval. Send a valid shell command that would create the test's `denied-policy.txt` file. Tap Deny and verify RCA recorded one denial for that approval ID and the file did not appear. Send the second command, which prints a unique stdout marker. Restart only the Orca adapter while its approval is pending. The test recovered the same approval in the app, tapped Allow and recorded one acceptance and one matching stdout event. The app cleared the prompt and the paired read still showed the saved Ask mode.

The adapter now reads and writes the chat's permission choice. The regression verifies the chosen mode reaches a direct turn and is captured when a follow-up is queued. Changing the selection applies to later sends. Linux passed 74 mapper flow tests and seven paired session integration tests. Automatic approvals remain unsupported by this adapter. The recordings and request IDs are under `.context/rca-orca-permission/published/run-0beec39fd9f3d222/`.

A separate native Claude Stop test passed on both apps. Each app stopped one guarded foreground Bash process, produced one interrupted RCA turn and left the completion marker absent. This does not resolve automatic notification from Claude's detached task. See `.context/rca-native-background/published/run-116028ff33fef289/`. The Orca OpenCode launch change passed [GitHub CLI CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45355970001) and [GitHub backend CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45355969979).

## Native queue delivery and direct steering across providers

Orca queued A and B and then used the composer's Steer action on the original active turn with Claude and OpenCode. In each case RCA recorded two queue additions, one steer before the active turn finished, and one start and completion for each queued card in A–B order. The app rendered both replies. This verifies native creation and drain on both providers. The earlier Orca Codex case covers full in-place edit, reorder, Stop and Resume. Evidence is under `.context/rca-orca-opencode-queue/published/run-aa926d087c1144cf/`.

T3's OpenCode Queue action likewise added two cards and ran each once. Direct Steer now originally left its draft and the error “already has an active run.” The adapter now forwards the steer text to the active RCA turn and projects one user item with the steer intent on that same run. Its receipt prevents an accepted command from forwarding again on retry. During the retest, the app cleared the draft and the provider's reply contained the unique steer marker while the projection still contained one run. This passes the OpenCode direct-steer variant. Linux passed all 285 T3 flow tests and 12 paired thread-surface integration tests. Evidence is under `.context/rca-t3-direct-steer/published/run-0473d788c8cee63d/`.

The preceding Orca permission change passed [GitHub CLI CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45358867623) and [GitHub backend CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45358867697).


## T3 creates, switches and retires two test worktrees

Create A and B independently using New worktree, Base branch and `origin/dev`. Run a read-only branch and HEAD probe in each native OpenCode thread. Write one untracked test marker in A, confirm that B cannot read it and switch back to A to confirm it remains. After checking B is idle, has an empty queue and a clean checkout, remove it through RCA with `force: false`. The registered workspace and directory disappeared and both refs remained. T3 retained B's chat until its native Settle action removed it from Working. A reopened with its existing transcript. The recordings, branch and stdout checks, and removal receipt are in `.context/rca-t3-worktree-retirement/published/run-25db32e77643de5d/`.

The first workspace skill attempt in A reached OpenCode from the native composer, but its skill tool returned “not found” for the installed fixture. The matching call ID ended with `ok: false`. A different shell tool read the skill file and exposed the marker. The adapter's paired command discovery found the skill. The bundle records the distinction as partial. Accept a provider skill assertion only when the matching tool call succeeds or the provider records actual skill expansion.

The T3 direct steering change passed [GitHub CLI CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45362407456) and [GitHub backend CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45362407650).


## T3 loads an OpenCode skill from slash autocomplete

The native client sent the correct provider instance and checkout path in its scoped refresh. The adapter returned the workspace catalog but advertised `providerWorkspaceSnapshotsOnDemand: false` in the environment config. T3 uses that flag when deciding whether to merge a fetched catalog into its composer state. The adapter now advertises the on-demand catalog it already serves. The config-and-refresh flow test reproduced the false flag and then passed with the fix. All 285 T3 flow tests passed on Linux arm64.

After installing the rebuilt adapter locally and cold-launching T3, the same workspace's skill appeared under Commands. Tapping it inserted a skill token. The OpenCode tool returned the fixture contents and finished successfully under the same call ID, and the app displayed the marker. The final prompt omitted the marker. This retest used the existing workspace, where the provider had copied the fixture into `.claude/skills` during the first diagnostic. This passes `skill-load` for T3 with OpenCode. The recording, filtered tool receipt, binary checksums and paired refresh diagnosis are in `.context/rca-t3-worktree-retirement/published/run-d15c6671721b41db/`.


## Orca retains the selected Claude model when queueing

Use a Claude chat displaying Sonnet and send one marked turn. Select Haiku and send the alternate marker. Hold another turn with the bounded Python fixture, then queue one Haiku follow-up from Orca's Send menu. Change the composer to Sonnet and verify RCA still lists Haiku on the same queue item. Release the fixture and send a fresh direct turn after the queue drains. The Claude transcript resolved the four marked replies as Sonnet, Haiku, Haiku and Sonnet. RCA recorded one queue addition and one start under the same ID. This passes the model-switch variant in an existing Orca Claude chat. The model picker, recordings, queue receipts and provider model IDs are under `.context/rca-orca-claude-model/published/run-bf55a8c30fdf52b3/`.

## Worktree retirement recovers a lost executor reply

Retiring the clean T3 survivor after skill testing returned `EXECUTOR_GONE`. Git already removed its directory, but RCA still listed the workspace after the executor reattached. An unfixed retry returned `NOT_FOUND` and left the registration. With the fixed local RCA service, retrying the same clean removal cleared the stale entry after the attached executor confirmed the checkout was absent. The repository workspace, other test worktree and both T3 branch refs survived. Other errors, including Git's refusal to remove a dirty checkout, still leave the registration. The workspace group passed all 10 tests on macOS and Linux arm64. The live retry and failing regression are in `.context/rca-t3-worktree-retirement/published/run-4deb82c8c56c4d40/`.


## OpenCode native questions and approvals in both apps

Open a fresh OpenCode chat in the native app. Ask the provider to use its structured question tool for Blue or Green. Seed two distinct follow-ups using paired T3 dispatch or RCA queue operations, record both IDs and restart only the adapter. Both native apps recovered the question. T3's paired projection retained the pending request ID and displayed the same two queue entries. Orca retained both queue IDs and its Green action resolved the original question. Each app's journal recorded one answer on the original turn and one start and finish for each queued item, in order. Both native transcripts showed both replies and the queue cleared.

In the same provider's app controls, choose Supervised in T3 or Ask for approval in Orca. Run separate Bash prompts with test-only stdout markers. Use Allow once in T3 or Allow in Orca for the first request. Use Decline in T3 or Deny in Orca for the second. Both recorded one matching decision per request. The allowed tool printed the exact marker. The denied tool returned the provider's permission refusal and produced no command marker. The controls cleared. The four recordings, filtered journals, and before and after queue receipts are in `.context/rca-opencode-native-requests/published/run-211eee23cc7c3a42/`.

## T3 retains Haiku when the Claude composer switches back to Sonnet

Start a bounded command with Claude Sonnet. While the command holds the original turn, select Haiku and queue a follow-up through the T3 Send menu. Switch the composer back to Sonnet. RCA retained Haiku on the same queue item. Release the command, wait for one queue dispatch, then send a direct turn. The Claude provider log shows Sonnet, Haiku and Sonnet on the active, queued and next direct replies. Evidence is under `.context/rca-t3-claude-queued-model/published/run-fed95d27f44d9c65/`.

## Physical iPhone and Linux host probe

The device driver briefly listed a physical iPhone. Neither service's deployment answered its tailnet health probe, and the local VM Ops credential was unavailable. The Wi-Fi and cellular handoff remains blocked. The redacted probes are in `.context/rca-opencode-native-requests/published/run-9598815fb0c33c8f/`.

The T3 skill catalog change passed [CLI CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45366730433) and [backend CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45366730593). The worktree retirement retry passed [CLI CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45368550307) and [backend CI](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45368550376).


## Codex interactive process cancellation controls

An isolated real Codex 0.163.0 app-server used a scripted loopback model and bounded Python fixtures. It acknowledged interruption while the exact worker remained live both before the initial Unified Exec yield and after the model had received its ongoing process ID. The provider process ID differed from the OS PID. The app-server refused this ID through its standalone command termination method. Thread unsubscribe left the fixtures alive. Thread archive reclaimed the interrupted workers along with an earlier background worker in that same thread. A worker in a different thread survived the archive.

A one-shot control with interactive Unified Exec disabled terminated its worker on interrupt. That control exercised no mobile UI and does not cover the interactive background tasks in the native case. The isolated run reclaimed every owned process. Its reusable fixture and receipts are under `.context/rca-codex-stop-investigation/published/run-70099b9c82abda1b/`. [Upstream Codex issue](https://github.com/openai/codex/issues/42717)

## Native OpenCode switches configured models and keeps the queued choice

The OpenCode Go and Zen probe turns returned an expired-key error. OpenCode's provider log from the earlier native tests showed both apps had used MiniMax-M3. A fresh paired thread completed real turns on MiniMax-M3 and MiniMax-M3.1-Flash. RCA exposes explicitly configured OpenCode choices through its paired model catalog when `RCA_OPENCODE_MODELS` lists their provider and model IDs. An empty list preserves the CLI default. T3 reads the configured alternatives from that catalog and keeps its pre-existing local choices when RCA only supplies the default placeholder.

On each native app, select MiniMax-M3 and start the bounded foreground fixture. Select MiniMax-M3.1-Flash and queue a follow-up, then select MiniMax-M3 again. RCA lists the same queue ID and Flash model both before and after switching back. Release the worker and send one more direct turn. Each native app displayed all three replies. OpenCode's provider transcript linked the three user messages to M3, Flash and M3. The queue dispatched once. The native recordings, catalog, and model receipts are under `.context/rca-codex-stop-investigation/published/run-ce56459958834b91/`. Linux arm64 passed 11 catalog unit tests, the new paired catalog flow test and all 285 T3 adapter flow tests.


## Orca loads an OpenCode skill from workspace autocomplete

OpenCode's Orca composer reads scanned workspace skills. The adapter served the scan but omitted the `skills.discover-workspace.v1` capability, so the composer made no workspace skill request. Advertising that capability enabled the picker. The first retest selected the Verde source from the menu and the actual OpenCode tool reported that the skill was unavailable in its runtime. Seeing a skill in the picker is a discovery assertion.

In a fresh disposable checkout, install the Cyan fixture under `.agents/skills` and run the repo's `wf agent-config sync` before opening an OpenCode tab. In the native Orca composer, select the Cyan result, then send the prompt without its unique marker. The matching OpenCode skill call returned the fixture contents and finished successfully. The same turn's reply displayed the exact marker. Both variants and their receipts are under `.context/rca-codex-stop-investigation/published/run-6b05bc90e69e025e/`. Linux arm64 passed 74 mapper flow tests and both paired catalog integration tests.

## Claude detached task completion

Claude Code 2.1.319 emitted `task_notification` after the parent result while its session remained open. RCA now keeps a process with a running detached task alive and records the terminal receipt on the originating turn. The socket regression delivers it while the next turn runs and verifies that both the live stream and history contain it once. The unit suites passed 72 CLI driver tests and 17 thread group tests on macOS and Linux arm64.

On both apps, a native Claude prompt started a gated background Bash command. The parent replied `STARTED` and finished. I left and reopened the thread, confirmed the fixture PID was still running, then released it. RCA recorded one completion on the original turn, the fixture's stdout contained the expected marker, and each queue stayed unchanged. Orca showed the actual provider summary under **Used 1 tool → Background task** without a follow-up prompt. T3 displayed the Bash launch and wrote zero projection rows for the late receipt. Neither app displayed the detached stdout marker in this variant.

Two fresh T3 chat submissions also stayed on **Starting…** without any adapter event or RCA turn. One repeated after restarting the adapter and cold-launching T3. The successful T3 launch used an existing Claude test thread. The recordings, per-app timelines, terminal queue states, fixture, and selected provider frames are under `.context/rca-claude-detached/published/run-a9bc9af7bfab8c14/`.

## T3 displays live and replayed Claude completion receipts

The T3 run follower stops at `turn.finished`. A separate subscription for an open Claude chat now projects detached task receipts even after the originating run has finished. Its cursor is independent from the run cursor, and the projection reuses the agent event's existing deduplication receipt. The command details show Claude's completion summary.

In the first native test, the worker completed while T3 was still on the search screen. Opening the chat showed the recorded receipt. In the second, I confirmed the original thread was open again before releasing the worker and saw the receipt arrive. Each case had one completed receipt attached to the original run, no follow-up turn before the receipt, an unchanged queue, and the expected worker stdout on disk. Both cases exercised an existing Claude thread. The recordings, accessibility receipts, filtered journal and projection rows, binary hashes, and fixture are under `.context/rca-t3-late/published/run-63ac3302249a0f62/`. The socket regression reproduced the missing update, then passed with the fix. Linux arm64 passed all 286 T3 flow tests.

## Both apps show actual detached Claude stdout

Claude's completion frame identifies a spool file on the executor host. The executor now attaches up to 32 KiB from a regular file for the matching shell task in its Claude session. The tests verify the truncation marker and exclude subagent transcripts, files outside the task directory, and linked files. macOS and Linux arm64 each passed eight executor session flows. The backend flow still attributes the receipt to the original turn.

In both native apps, start the gated worker and reopen the chat after the parent says `STARTED` and completes. Release the same worker, then expand the background task details. Both apps showed the two expected stdout lines, including a marker absent from the prompt. The journal matched the provider's spool and recorded one receipt per original turn. The queues stayed unchanged and neither app started a follow-up turn. The fixture, recordings, screenshots, and filtered timeline are under `.context/rca-claude-stdout/published/run-1d8260a4073d8dc5/`.

## T3 sends fresh chats after the workspace list synchronizes

The native outbox waits for a live workspace list before sending a creation. A warm shell subscription reread the entire event store and timed out after 24 seconds on the local fixture. The adapter now retains the shell deltas within the existing replay window. The same caught-up subscription returned its completion marker in 4 ms. The native decoder also rejected an older fork row with a source thread but no source point type. T3 now reads that field as unknown and preserves the parent in the lineage.

The simulator's pending current-checkout send launched once after reconnect. A second native chat launched once from a fresh worktree based on `origin/dev`. Both replies contained their markers. Linux arm64 passed all 287 T3 flows, including regressions for the replay window and saved fork. The recordings, decoder result, and receipt IDs are under `.context/rca-t3-fresh/published/run-11dddfa86c68e400/`.

## Codex 0.172.0 terminates the stopped turn's worker

The release still leaves an interactive process running after a plain turn interrupt. RCA now uses the app-server's thread-scoped terminal list and terminates only the terminal items started by the interrupted turn. A real app-server probe with a scripted model verified that the selected worker exited and that earlier work in the same thread and work in another thread stayed alive.

The installed native T3 and Orca apps each started a bounded worker through Codex 0.172.0. After I tapped Stop, each app recorded an interrupted turn and the owned PID exited before its natural-completion marker. The first T3 attempt with the standalone Codex binary needed the matching Code Mode helper before it could start the worker. The scoped termination regression and all 73 CLI driver tests passed on Linux arm64. The two recordings, exact worker receipts, and the app-server probe are under `.context/rca-codex-stop-0172/published/run-e8e66cf9d97aa11e/`.
