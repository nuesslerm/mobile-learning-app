---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-agent/src/**"
  - "libraries/agent-api/agent-api-agent/src/events.rs"
  - "backend/remote-chat-agent/service-core/src/application/agent_groups/engine_threads.rs"
opened: 2026-11-20
last_verified: 2026-11-21
links:
  - "docs/plans/rca-parity/plan.md"
  - "docs/plans/rca-offer-more/plan.md"
---
# Orca `agentSession.cancelUsageLimitContinue` stays not offered, and Continue after a restart names one cause

Issue #83, O3 (sessions cluster). Checked 20 Nov against the recorded host (`orca-v1.4.259-recorded`, `-crawl`) and the pinned source in
`references/orca`; the restart half was built on 21 Nov.

## What is served

`agentSession.continueInterrupted` is served end to end, and it needed no recording of a cut turn, because the pinned source fixes everything the
phone reads: the stop row (`presentation: "orca-stop"`, `orcaStop: {cause}`), the cut rule (`latestNativeChatOrcaStopCut`), the params, the answer
(`{sessionId, outcome}`, where the phone only tells `refused` from the rest) and the prompt the host sends
(`AGENT_SESSION_RESTART_CONTINUATION_MESSAGE`). What was missing was a cause in rca, not a shape:

- The agent API's `turn.finished` carries an optional `cause` beside `reason`; `restart` is the only one. The service writes it for a turn it
  found running when it started again (the sweep that already ended such a turn `interrupted`). A user's stop writes none.
- The mapper folds an interrupted turn with that cause into the turn plus one stop row of its own, and answers Continue by the host's rule: the
  cut is checked again at the call, and a cut another message or an earlier Continue already answered sends nothing (`superseded`).
- The host advertises `agent-session.continue-interrupted.v1` and `.phone.v1`, which the phone needs before it offers Continue.

Evidence: the contract test of the frame, the service's restart test (the cause on the wire after a real crash and restart, none after a user's
stop), the mapper's flow tests, and a chain test that kills the real rca-service mid-turn, restarts it over its state, reads the cut and continues
it through the real executor (`tests/integration/interrupted.rs`). The crawl and recorded replays of the row's five validation shapes are clean, but
they run no turn, so the row is classed `unrecordable` and served by the chain test: no recording shows a valid call over a real cut. The order of the
checks (`sessionId`, then `turnItemId`, then extra keys, as `sendRecorded` does) is from the pinned schema and unchecked against a recording.

## What is narrower than the host

- The cause is always `crash`. The host tells an update, a quit and a crash apart; rca finds only a turn that was running when it started again,
  which died with its process whichever way the process went. A graceful stop that writes `quit` or `update` before the service exits is not built.
- The host writes an attribution note after it sends ("Orca asked this agent to continue after a restart. Your own prompt was not re-sent.").
  The journal here is folded from rca's events and has no row of the mapper's own, so the continue prompt shows as a user message.
- The reply is `pending` once rca accepted the turn; the host's `continued`, `unknown` and `refused` after a provider verdict are not told apart
  (a refusal rca gives at the start is `refused` with its code as the reason).
- The phone's wording comes from the stop row's `crash` cause, and the row's own text is a generic sentence naming no agent.
- The restart offer for lost background work (`agentSession.restartContinue`, a different row) is not part of this.

## Still not offered: `agentSession.cancelUsageLimitContinue`

It cancels the timer the host keeps for a turn that hit a provider usage limit, with `resetsAt` read from the CLI's own message. Nothing here can be
built without a recording from a limited account:

- the recordings hold only the refusal `structured_agent_session_unsupported` (valid params) and validation errors (bad params);
- the CLIs' limit text is not pinned or recorded, and the real-CLI jobs hold no limited Claude or Codex login to record one (the owner holds it);
- the build behind it is a cause `usage_limit` with a `resets_at` on `turn.finished`, a stored resume time per thread, a clock that sends the
  continue turn, and a cancel that clears it, plus `agent-session.usage-limit-auto-continue.v1` and the `send-usage-limit-continuation` shape.

## What would close it

A recording of a host turn stopped by a usage limit, from a limited Claude or Codex login only the owner holds; then the pieces above.
