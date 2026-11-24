# RCA mobile parity results

The [case manifest](rca-mobile-parity.json) defines the assertions to automate. Each result below records the strongest evidence available for that assertion. Simulator runs used native T3 Code and Orca Mobile builds on iPhone Simulator, with RCA and both adapters on the Mac. Integration tests used the real adapters with scripted providers.

## Initial provider matrix

The first six rows created Python services and merged into `dev`. Five rows had green merge-commit CI in the recorded run. The detailed interaction traces and screenshots are indexed in `.context/rca-native-check/interaction-evidence/README.md` on the original Mac checkout. [#88](https://github.com/nuesslerm/mobile-learning-app/pull/88), [#89](https://github.com/nuesslerm/mobile-learning-app/pull/89), [#90](https://github.com/nuesslerm/mobile-learning-app/pull/90), [#91](https://github.com/nuesslerm/mobile-learning-app/pull/91), [#92](https://github.com/nuesslerm/mobile-learning-app/pull/92), [#93](https://github.com/nuesslerm/mobile-learning-app/pull/93)

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

RCA now includes ordered attachment references in the user message event. Orca uses those references when it rebuilds the transcript. Messages already saved without references have no source for thumbnail replay. The Linux regression failed against the old code and passed after the fix. The Orca agent-session integration suite passed all six cases. This run covers Orca with Codex. T3 photo persistence and the other providers still need native evidence.

## T3 queue follow when RCA is unavailable

During the local RCA restart for photo verification, repeated subscription failures left the head of several T3 queues in place. T3 kept taking the same head in the same drain. The Mac counted about 16,000 RCA connections in TIME_WAIT, and loopback requests failed across both adapters and the simulator driver. Suspending the local T3 process stopped the new connections and freed the ports.

T3 now returns from that drain if the same row is still waiting. The regression forces the first subscription to fail before the queued turn starts. It asserts one attempt in that drain, persistence of the waiting row, and successful playback on a later drain without starting another RCA turn. It passed on Mac and the ten Linux agent-thread flow tests passed. After restarting the patched local T3 adapter, RCA, T3 and Orca all returned HTTP 200.

## T3 photo preview after a cold reopen

Select the same two swatches in the same order with T3's Photo Library picker, then send a Claude Sonnet turn and cold-reopen the chat. Both composer images appeared. The staged files matched both original PNG SHA-256 digests, and Claude reported the colors in order. The reopened projection still had both upload IDs, but the thumbnails were blank and both paired `assets.createUrl` requests returned `AssetAttachmentNotFoundError`.

T3 still projects the pending upload IDs on the sent message. The pinned T3 server claims a permanent copy before dispatch so pending-upload cleanup can remove its own copy. The RCA adapter has no equivalent claim step. The end-to-end failing check now has stable assertions for the before and after projection, staged hashes and asset readback.

## T3 photo cleanup and cold reopen follow-up

Repeat the two-photo picker with Claude Sonnet. After send, use a paired connection to call `attachments.delete` for both pending upload IDs, then fully terminate and relaunch T3. Reopen the same chat from the Chats list. Both thumbnails render and both inline photo links open the correct image. The reopened projection preserves the message ID, both attachment IDs and the composer records that bind those links. The agent identified blue, orange, green and purple in order. Both `assets.createUrl` reads return PNG bytes with the original fixtures' SHA-256 digests after the cleanup and after cold reopen.

T3 now protects blobs referenced by a saved message from draft cleanup and persists composer context on the message and user turn item. The regressions confirm that an abandoned upload still deletes, a sent upload can be read after cleanup and a new paired connection, and both launch paths preserve composer context across a T3 restart. The affected Linux suites passed 17 flow and 4 integration tests. This passes `photo-picker` on T3 with Claude Sonnet. The other T3 providers still need native coverage.

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
