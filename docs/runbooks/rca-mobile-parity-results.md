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

Codex in Supervised mode displayed Allow once and Decline for two read-only Python commands. Allow once produced `RCA_PARITY_T3_ALLOW_ONCE` on stdout. Decline resolved the other request without executing its output marker. Both controls cleared after the user acted. The combined reconnect case still needs a populated queue.

The paired-client integration regression checks pending approval and question projection, the question snapshot after a new socket connects, and resolution after approval, answer and dismissal. The local T3 thread-surface suite passed 12 tests and the RCA adapter flow suite passed 14. Simulator recordings, screenshots and request IDs are under `.context/rca-runtime-requests/` in Zagreb.

The device driver can now see the physical iPhone with both apps installed. The deployed `t3-serve` and `orca-serve` tailnet health probes still fail, so the phone-to-Linux network check remains open.
