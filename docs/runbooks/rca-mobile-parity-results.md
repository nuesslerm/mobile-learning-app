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
