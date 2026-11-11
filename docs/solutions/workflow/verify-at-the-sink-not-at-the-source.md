---
module: workflow
date: 2026-08-13
problem_type: workflow_issue
component: development_workflow
severity: high
applies_when:
  - "Verifying that a telemetry, logging, or delivery pipeline works end to end"
  - "A task's success criterion is 'the data arrives' rather than 'the code compiles'"
  - "Working in a sandbox where one hop of the chain is genuinely restricted"
  - "Running a long autonomous goal loop with no human checking each step"
root_cause: missing_workflow_step
resolution_type: workflow_improvement
tags:
  - evidence
  - observability
  - verification
  - autonomous-agents
  - betterstack
  - goal-loop
---

# Verify at the sink, not at the source

## Problem

A single session chased "all logs reach BetterStack" through eight distinct
failure modes across the backend, the mobile app, the build chain, and the deploy
path. Every one of them was *first* declared fixed on the strength of evidence
from the wrong end of the chain — configuration read as intent, a transport
reported as wired, a commit pushed, a probe returning success. In each case the
sink was still empty.

The pattern is not carelessness about any one fact. It is that source-side
evidence is abundant, cheap, and almost always genuinely true — and completely
uncorrelated with whether the thing works.

## Symptoms

The same mistake, in eight costumes:

| Source-side evidence, all of it true | What the sink said |
|---|---|
| Every service imports the right logger; every compose file passes the right vars | 14 rows in 24h, all heartbeats, from 3 of 6 services |
| Heartbeats are arriving, so the transport works | Correct, and irrelevant — heartbeats and app logs took different paths |
| `withMetadata({ err })` is the documented convention and is used everywhere | Errors serialized as `{}`, because `Error` fields are non-enumerable |
| The fatal handler logs before exiting | Buffered queue discarded by `process.exit` |
| `process.env.EXPO_PUBLIC_BETTERSTACK_TOKEN` reads as present in the page | Zero requests ever attempted — the config was never the problem |
| `curl` reaches the ingest host and returns `202` | The app never made a request to fail |
| `/debug/ping` reports `sent: true` | Nothing in the collection from that service |
| The fix is pushed and the service redeployed | The redeploy ran the previous script |

## What Didn't Work

- **Treating "configured" as "working".** A configured transport, a passing
  type-check, a committed fix, and a green probe are all fully compatible with
  zero rows.
- **Generalising from the working path.** Heartbeats arriving proved the transport
  was reachable and said nothing about whether application code reached the
  transport. The working path and the broken path never overlapped.
- **Accepting a documented environment limitation as the end of the inquiry.**
  Headless Chromium in this sandbox genuinely cannot TLS-handshake an external
  HTTPS host, and that fact was true, documented, and *not the cause* of the
  mobile silence. Naming a real constraint is not the same as having explained
  the symptom.
- **Reaching for a synthetic probe.** A hand-crafted line pushed into the
  pipeline confirms the last hop and nothing before it. It was explicitly rejected
  as evidence: the requirement was real traffic the application itself chose to
  emit.
- **Stopping at the first plausible fix.** Every layer here had more than one
  defect. Fixing one and re-declaring success was wrong four separate times; each
  fix only exposed the next.

## Solution

**1. "Queried rows or it didn't happen."** The only accepted evidence that a
pipeline works is a query against the destination returning the specific line the
action just caused — named, timestamped, and attributable to that action. Nothing
upstream substitutes.

**2. Real traffic, not probes.** The verification drives the actual product — a
chat turn, a page navigation, a service handling a request — and asserts on the
lines the application chose to emit on its own. For the mobile app this became an
e2e suite that navigates and interacts, then asserts the app's own event names
arrived at the sink.

**3. Route the restricted hop; do not re-raise it.** When one hop of the chain is
genuinely blocked in this environment, use the interception the repo already
provides for exactly that purpose rather than reporting the limitation again. The
browser suite routes the ingest request through the test runner's own network
stack at the CDP layer: the payload crosses byte-for-byte, the content stays
entirely the app's. A documented constraint with an existing, chosen workaround is
not a finding.

**4. Distinguish "no attempt" from "failed attempt" before theorising.** Zero
requests and a failing request have disjoint cause sets. Conflating them sent the
mobile investigation at the sandbox's TLS restriction for hours.

**5. Enumerate the chain, then check every link.** Emit → serialize → buffer →
flush → transport → credential → sink. Each link fails silently and produces the
same non-signal. A fix to one link is not progress toward "it works"; it is
progress toward *knowing which link is next*.

**6. An external watchdog for premature satisfaction.** A second session was
spawned to check in periodically, ask what the actual state was, and restart the
goal loop whenever this one settled for an incomplete result. That is a structural
answer to a structural problem: the agent that just fixed something is the worst
judge of whether the job is done, because the fix is the most salient thing in its
context and the empty sink is not.

## Why This Works

Every link in a delivery chain reports success locally and produces an identical
symptom when it fails: absence. Absence has no source, so there is no error to
read and no stack to walk — which is precisely why the investigation drifts to the
evidence that *is* available, all of which sits at the source.

Anchoring on the sink inverts that. It gives a single unambiguous signal that
cannot be satisfied by intent, and it forces the chain to be walked link by link
instead of reasoned about as a whole.

The watchdog works for the same reason a second reviewer works: "I fixed it" and
"it works" are different claims, and the party holding the first is structurally
unsuited to evaluate the second.

## Prevention

**Define done as an observation, never as a change.** A task whose goal is
delivery is complete when a query at the destination returns the expected row —
not when the code that should produce it is merged, deployed, or configured.

**Never report a delivery pipeline as working from a configuration read.**
Configuration is a statement of intent. It is the cheapest evidence available and
carries the least information about the outcome.

**Treat "only the health signal arrives" as a failure.** A heartbeat proves
reachability and nothing about the application path. If a service is serving
traffic and its only rows are heartbeats, its operational logs are going
somewhere else.

**A real environment limitation still needs a causal chain to the symptom.** Cite
it once, then show how it produces *this* observation — or keep looking. In this
repo, mentioning the sandbox TLS restriction is not a substitute for using the
workaround that already exists for it.

**Expect more than one defect per silent chain.** Four independent defects
produced identical silence on the backend; two more did on mobile. After a fix,
re-verify at the sink rather than closing — the next defect is only observable
once the previous one is gone.

**When running unattended, build in something that disbelieves you.** A periodic
external check-in that asks for the *sink's* state, not the agent's summary, is
cheap and catches the specific failure of stopping at a plausible partial result.

The concrete failures behind this are documented at
`docs/archive/README.md`,
`docs/solutions/runtime-errors/mobile-logs-never-left-the-app.md`, and
`docs/archive/README.md`.
Query recipes for the sink itself: `docs/solutions/workflow/betterstack-query-recipes.md`.