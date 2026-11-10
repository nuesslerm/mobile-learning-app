# R2c replay: wf-t3 and orca-serve against the recorded crawl cassettes

Date: 7 Nov 2026. Cassettes: `clis/t3/cassettes/t3-crawl` (1,248 interactions, 8 tagged nondeterministic and not compared) and `clis/orca-serve/cassettes/orca-crawl` (3,682, none nondeterministic), both recorded on the VM against the pinned real servers (`recorded_on: vm`, pin hash checked by the replay tests). The candidates are the adapters as they are on this branch: no adapter behaviour was changed in this milestone, so every difference below is an adapter bug (or a not-yet-served method) listed with its interaction id in `known-gaps.json` beside the set. A difference that is new, or a listed one that no longer differs as listed, fails the integration test.

## Replay setup (what is compared against what)

- Orca: `wf mimic replay` through `orca-serve` and the real rca chain; the reads (no `state:write` tag) share one chain and the state-changing cases another, in cassette order, as the VM recording shared one server.
- T3: the reads share one `wf-t3` chain and the state-changing cases another; each interaction opens with the recorded session prelude (pairing, socket, project and thread, made once per chain, its steps copied), then its own socket. `state:fresh` cases get a chain each.
- Write cases are recorded once, on the state their chunk accumulated; the committed crawl cassettes were recorded before the fix that stopped the stability sample moving a write case to a pristine server, so a state-dependent write row can differ on replay for that reason, and this table does not separate those rows (re-record on the VM to remove it).

## Totals by severity

| | Interactions | Compared | Clean | Allowlisted (machine-only paths) | Drift | Breaking | Gap groups |
|---|---|---|---|---|---|---|---|
| Orca | 3682 | 3682 | 168 | 0 | 2854 | 660 | 19 |
| T3 | 1248 | 1240 | 0 | 381 | 182 | 677 | 98 |

Orca has no allowlist for the crawl, so every interaction without a gap is clean; T3 carries the recorded machine-only allowlist (host name, paths, versions), so an interaction without a gap is allowlisted, never clean.

## Rows

| | Rows with recorded cases | Rows with at least one differing case | Rows whose cases all agree |
|---|---|---|---|
| Orca | 799 | 768 | 31 |
| T3 | 301 | 271 | 30 |

## Orca: largest gap groups (of 19; all ids in `orca-crawl/known-gaps.json`)

| Cases | Severity | Class (worst difference, positions elided) | Example id |
|---|---|---|---|
| 2681 | drift | changed at steps._:s2c:*.[*].payload.error.code | `orca-accounts.addClaudeFromConfigDir-boundary-full` |
| 405 | breaking | missing at steps._:s2c:*.[*].payload.result | `orca-accounts.list-invalid-drop-refreshUsage` |
| 190 | breaking | missing at steps._:s2c:*.[*].payload.error.data | `orca-agentSession.agents-invalid-no-payload` |
| 108 | drift | changed at steps._:s2c:*.[*].payload.error.message | `orca-agent.launchReplay-invalid-no-payload` |
| 63 | drift | changed at steps._:s2c:*.[*].payload.error.code | `orca-agentSession.readVisual-boundary-full` |
| 19 | breaking | missing at steps._:s2c:*.[*].payload.error | `orca-agentSession.release-boundary-limit-sessionId` |
| 17 | breaking | missing at steps._:s2c:*.[*].payload.result | `orca-accounts.antigravityChatSignInStart-empty-empty-object` |
| 6 | breaking | missing at steps._:s2c:*.[*].payload.error.data | `orca-agentSessionAttachment.read-boundary-limit-path` |
| 4 | breaking | kind at steps._:s2c:*.[*].payload.result.authoritativeWindowId | `orca-status.get-boundary-full` |
| 3 | breaking | missing at steps._:s2c:*.[*].payload.client_error | `orca-accounts.removeClaude-valid-min` |
| 2 | breaking | missing at steps._:s2c:*.[*].payload.error.data | `orca-agentSession.createSupport-boundary-full` |
| 2 | drift | changed at steps._:s2c:*.[*].payload.result.[*] | `orca-preflight.detectAgents-invalid-no-payload` |

## T3: largest gap groups (of 98; all ids in `t3-crawl/known-gaps.json`)

| Cases | Severity | Class (worst difference, positions elided) | Example id |
|---|---|---|---|
| 231 | breaking | missing at steps.ws:s2c:*.[*].payload.exit.cause.[*].defect | `t3-command-delegated-task.request-boundary-full` |
| 102 | breaking | missing at steps.ws:s2c:*.[*].payload.exit.cause.[*].error.cause | `t3-command-checkpoint.rollback-boundary-full` |
| 100 | drift | changed at steps.ws:s2c:*.[*].payload.exit.cause.[*].defect | `t3-command-delegated-task.request-invalid-no-payload` |
| 49 | breaking | missing at steps.ws:s2c:*.[*].payload.exit.value | `t3-rpc-agentSessions.scan-invalid-no-payload` |
| 35 | breaking | kind at steps.http:ret:_.[*].payload.body | `t3-http-auth.browserSession-invalid-drop-credential` |
| 27 | breaking | missing at steps.ws:s2c:*.[*].payload.exit.cause | `t3-rpc-projectClone.start-boundary-full` |
| 18 | breaking | kind at steps.http:ret:_.[*].payload.body | `t3-http-auth.browserSession-invalid-no-payload` |
| 16 | drift | changed at steps.ws:s2c:*.[*].payload.exit.cause.[*].error._tag | `t3-rpc-pullRequests.activity-invalid-wrong-type-projectId` |
| 13 | drift | extra at steps.ws:s2c:*.[*] | `t3-rpc-cloud.getRelayClientStatus-invalid-no-payload` |
| 13 | breaking | missing at steps.ws:s2c:*.[*].payload.values | `t3-rpc-cloud.installRelayClient-invalid-no-payload` |
| 12 | breaking | missing at steps.ws:s2c:*.[*].payload.exit.cause.[*].error.reason | `t3-rpc-pullRequests.activity-boundary-limit-number` |
| 10 | breaking | missing at steps.ws:s2c:*.[*].payload.exit.cause.[*].error.reason | `t3-rpc-server.deleteAcpRegistrySession-valid-min` |

## Replay cost

Measured with `time -p cargo test -p <crate> --test integration the_answers_the_real` (debug build, the whole test including its recorded slices; CPU is user plus sys of the test process and the servers it starts).

| | Wall | CPU |
|---|---|---|
| orca-serve (orca-recorded and orca-crawl) | 76.6 s | 68.3 s |
| wf-t3 (t3-recorded-1..3 and t3-crawl) | 753.5 s | 59.4 s |

T3's wall time is the sockets' settle windows (400 ms per case, sequential), not CPU. The scenario sets are recorded but not replayed here.

## Orca gap fixes of the same day (adapter bugs behind the largest fixable groups)

Cause groups of the breaking rows (phone rows only: `orca-methods.json` marks 293 of 824 rpc rows as methods the phone calls, and `orca-serve` serves only those, so the 456 breaking rows of desktop-only rows such as `orchestration.*` mutations, `browser.*` and `layout.*` are a scope decision, not adapter bugs; a first fix for `orchestration.*` was reverted for that reason). Of the 278 breaking rows of phone rows the largest groups were GitHub Projects (59, needs an rca capability), Linear and Jira (50), settings and ui defaults (18) and the unsubscribe rows (14). Fixed:

| Group | Cause | Fix |
|---|---|---|
| Linear and Jira (`map-settings/src/trackers.rs`) | `method_not_found` for 16 phone rows | a host with no tracker account: disconnected status, empty lists, a lookup finds nothing, writes say Linear is not connected, the schema's missing-id messages; `linear.connect` (asks Linear) and the `linear.listIssues` full-filter row stay gaps |
| `settings.get`, `settings.update`, `ui.get`, `ui.set` (`map-state`, `defaults.json`) | answered only what was set; Orca answers its defaults under it | the pinned host's defaults merged under the kept object |
| `accounts`, `notifications`, `terminal` `unsubscribe` and the `agentSession` and `nativeChat` unsubscribe validation (`map-settings/src/streams.rs`, `map-agent`) | `method_not_found`, and no id validation | end an id this host never opened, refuse a missing or mistyped id in the schema's words and a session id that is not `<agent>_<token>` |

| Set | Breaking before | Breaking after | Drift before | Drift after | Gap groups before | after |
|---|---|---|---|---|---|---|
| `orca-recorded` (278 compared) | 76 | 53 | 142 | 141 | 19 | 17 |
| `orca-crawl` (3,682 compared) | 658 | 613 | 2,669 | 2,618 | 26 | 24 |

Together the three groups removed 68 breaking interactions (23 in `orca-recorded`, 45 in `orca-crawl`) and 10 drift ones. Counts are interactions listed in `known-gaps.json` by severity. Left in the largest unfixable-here groups: unserved desktop-only rows (`method_not_found`, drift for invalid payloads, breaking for valid ones), GitHub Projects and GitLab items (rca holds no issue or project data), `agentSession.createSupport` and attachment rows (rca capability), `status.get` capabilities and degradations, the `settings.update` of `machineName`. The chain evidence for `coverage` was regenerated from a run that includes the `mimic_` tests (without them the `mobileWeb.bundle.chunk` row loses its evidence).
