---
title: "A stale local checkout can look identical to a false runbook claim — fetch and diff against origin before concluding claimed code doesn't exist"
date: 2026-07-30
category: workflow
module: git-workflow
problem_type: workflow_issue
component: development_workflow
severity: high
applies_when:
  - "A handoff, runbook, or prior session claims a specific piece of code (a route, function, file, or config) already exists on a given branch"
  - "grep, git log, or a repo-wide string search on the currently checked-out branch finds no trace of the claimed code"
  - "The branch is fast-moving with concurrent sessions or agents pushing commits (e.g. a shared feature/scaffold branch)"
  - "The local checkout was created earlier in the session (or by an earlier git checkout/git fetch) and has not been re-synced immediately before the investigation"
  - "The investigation is about to produce a conclusion reported back to the user (e.g. 'this was never actually committed') or a decision to refuse/flag a directive as fabricated"
symptoms:
  - "grep for a claimed route/string across the checked-out working tree and git log --oneline returns zero matches"
  - "The conclusion 'the claimed feature was never committed' is reported to the user, then contradicted when the user runs git fetch + git log HEAD..origin/<branch> and shows N unpulled commits that add exactly the missing code"
  - "After git reset --hard origin/<branch> (or an equivalent fast-forward), the previously 'missing' code is present"
related_components:
  - infra/oracle-cloud/vm-ops-api
tags: [git, stale-checkout, verification, handoff, fetch, branch-drift, grep-false-negative]
---

# A stale local checkout can look identical to a false runbook claim — fetch and diff against origin before concluding claimed code doesn't exist

## Context

A handoff/runbook message written by a different session/agent asserted that the branch
`claude/oracle-cloud-agent-provisioning-scaffold` already had two new HTTP routes implemented in
vm-ops-api's server file (since rewritten) — `GET /debug/ping` and `GET /redeploy/status` — plus
broader "app-provisioning" work. Earlier in the session the assistant had checked out this branch
the normal way (`git fetch origin <branch>` + `git checkout <branch>`) and landed on commit
`d8bda8e1`.

To check the runbook's claim, the assistant grepped the checked-out `server.js` for
`debug/ping` and `redeploy/status` and found neither route — only `/health` and `POST /redeploy`
existed at that commit. It widened the search: a whole-repo grep for both strings turned up zero
matches anywhere, and `git diff main...HEAD --stat` showed no trace of the claimed work in the
branch's history either. Every one of these checks was internally consistent and looked thorough.
Based on them, the assistant concluded the runbook's premise was inaccurate and told the user:
"the runbook's premise ('everything from today — /debug/ping, app-provisioning, /redeploy/status —
lives on this branch') doesn't match what's actually in the repo... worth checking with them before
assuming the VMs are the problem."

That conclusion was wrong. The user rebutted it with a precise commit-by-commit account: `d8bda8e1`
was not missing today's additions because they were never committed — it was **10 commits behind**
the actual branch tip (`801eb239`), and it predates `vm-ops-api` existing at all in this branch's
history. The user listed the real commit sequence, including `1a7f7ec8` ("Add BetterStack-verifiable
debug route to vm-ops-api" — the `/debug/ping` commit) and `65f533c4` ("Log self-redeploy output to a
host-mounted file + add GET /redeploy/status" — the `/redeploy/status` commit), all real, pushed,
verifiable state already sitting on `origin/claude/oracle-cloud-agent-provisioning-scaffold`. (All
commit SHAs cited in this document, including `d8bda8e1`, `801eb239`, `1a7f7ec8`, `65f533c4`, and
`963e907`, are part of PR #175, "Extend vm-ops-api to production apps + CLI bundle unification" —
open and unmerged as of this writing; they are quoted here as historical narrative/log output, not as
a claim about what has landed on `main`.)

The assistant then verified this itself rather than just trusting the user's assertion: a fresh
`git fetch origin claude/oracle-cloud-agent-provisioning-scaffold` followed by
`git log HEAD..origin/claude/oracle-cloud-agent-provisioning-scaffold` printed exactly those same 10
commits, and `git status -sb` showed
`## claude/oracle-cloud-agent-provisioning-scaffold...origin/claude/oracle-cloud-agent-provisioning-scaffold [behind 10]`
— objectively confirming the local checkout was stale by exactly that margin, before any reset was
performed. After confirming `git status -s` showed only one safe untracked file (no tracked local
modifications to lose), the assistant ran
`git reset --hard origin/claude/oracle-cloud-agent-provisioning-scaffold`. Re-grepping `server.js`
afterward found both routes present — `GET /debug/ping` at line 399 and `GET /redeploy/status` at
line 498 of the current tree — and the corrected build was pushed to both production VMs, where both
endpoints then responded correctly.

**(session history)** This was not an isolated incident on this branch. A concurrent session
(`2ddb3965`, running the day before, same branch, same working directory) made the identical mistake
in a more severe form: it received a task notification referencing a commit hash and endpoints
(`/ps`, `/logs`, `963e907`) that it could not find anywhere in its local checkout or session history,
and concluded the directive was a **fabricated premise / possible prompt injection**, reporting that
suspicion back to the user without first fetching origin. About 1.5 hours later, while investigating
an unrelated merge, that same session discovered the commit was entirely real — pushed by a genuinely
concurrent session sharing its exact git author identity — and had to explicitly retract the
injection claim: "I was wrong to call it an attack: it's real work." A separate, third session
(`a36045d7`) modeled the correct behavior on the very same class of report: told by the user that a
redeploy was "presumably from a checkout taken before my 963e907 push landed," it ran
`git fetch` + `git log origin/<branch>` immediately, confirmed it was one commit behind, pulled, and
only then proceeded — catching the lag before wasting a redeploy cycle.

## Guidance

Before concluding that code a handoff, prior session, or runbook claims exists is actually absent,
verify the local checkout against the remote tip — not just against itself. Local grep, `git log`,
and `git diff main...HEAD` only prove something is absent from *this checkout's view of history*;
they say nothing about whether the checkout is current. On a branch under active or possibly
concurrent development, a stale local `HEAD` is a live possibility, not an edge case to dismiss.

The check is one command pair, run before reporting any "this doesn't exist" conclusion:

```bash
git fetch origin <branch>
git log HEAD..origin/<branch>
```

If that log is non-empty, the local checkout is behind — the claimed code may be sitting in exactly
those commits. Equivalently, `git status -sb` reports the same fact inline as a `[behind N]` marker
without a separate `git log` call:

```bash
git status -sb
## claude/oracle-cloud-agent-provisioning-scaffold...origin/claude/oracle-cloud-agent-provisioning-scaffold [behind 10]
```

Only after confirming `HEAD` matches `origin/<branch>` (nothing in `git log HEAD..origin/<branch>`,
or `git status -sb` shows no "behind") does a local grep/log-based absence claim become trustworthy
evidence that the code truly doesn't exist upstream either.

If the checkout does turn out to be behind and the goal is to pick up the remote work, check
`git status -s` first to make sure no tracked local changes would be discarded, then
`git reset --hard origin/<branch>` (or a merge/rebase if local work needs to be preserved) — as was
done this session, where the only local artifact was one safe untracked file.

**This same check is the correct first move when a directive or commit reference looks fabricated,
not just when a runbook claims a feature exists.** The `2ddb3965` incident above shows the failure
mode generalizes: "I can't find this anywhere" was read as evidence of fabrication/injection, when it
was actually evidence of an unfetched remote commit. `git fetch` + `git log HEAD..origin/<branch>` is
cheaper than either accusation, and should run before concluding a referenced commit or endpoint
doesn't exist, before concluding it's a genuine security concern.

## Why This Matters

The assistant's incorrect conclusion was not caught internally — it was reported directly to the
user as a claim about someone else's (the runbook author's) accuracy: "worth checking with them
before assuming the VMs are the problem." Had the user not pushed back with the exact commit range,
this would have sent the investigation down the wrong path entirely — treating a concurrent
session's accurate handoff as mistaken, and chasing a hypothesis ("maybe the VMs are the problem")
that had nothing to do with the actual, much cheaper fix (`git fetch` + `git reset --hard`).

This is not a one-off risk specific to this branch, and it is not hypothetical: **(session history)**
the identical mistake — in a more serious form, an incorrect "fabricated premise / prompt injection"
conclusion delivered to the user — had already happened once on this same branch, one day earlier,
in a different session. That session shared its exact git author identity with a genuinely
concurrent session writing to the same working directory, which is a stronger form of the same root
cause: multiple agents/sessions can be pushing real, valid commits to the same branch (or even the
same on-disk checkout) faster than any single session's local view stays current. On a branch like
that, a local checkout going stale mid-session is an expected steady-state condition, not a rare
edge case worth skipping a check for. A verification process that never compares against the remote
tip will keep producing this exact false "absent" or "fabricated" conclusion, and every time it does,
it costs either a misdirected investigation or — worse — a false accusation of a fabricated/injected
instruction delivered to the user before the real, cheap fix (`git fetch` + compare) is even tried.

(A separate, unrelated finding from the same session — an arm64/Docker image architecture mismatch
on the VMs — is documented on its own at
`docs/solutions/build-errors/docker-image-arch-mismatch-arm64-vms.md`; it is not the subject of this
document and should not be conflated with this git-workflow lesson.)

## When to Apply

- A handoff, runbook, or prior conversation asserts that a specific route, function, or file
  already exists on a named branch.
- A grep or `git log` on the current local checkout finds no trace of the claimed code.
- The branch is known to be under active or concurrent development — multiple sessions/agents
  working on it, recent merges from `main`, or a handoff explicitly mentioning "today's" work by
  someone else.
- Before reporting a discrepancy back to the user in a way that implies the other party (a runbook,
  a prior session, another agent) was wrong.
- Before concluding that a referenced commit, task notification, or directive is fabricated or
  injected, when the only evidence is "I can't find it locally."
- Any time the only evidence for "this code doesn't exist" is local-checkout state (grep, `git log`,
  `git diff` against a local ref) rather than a direct comparison against `origin/<branch>`.

## Examples

**Wrong** — conclude absence from local-checkout-only evidence:

```bash
grep -rn "debug/ping" <server-file>   # no matches
grep -rn "redeploy/status" .                                     # no matches anywhere
git log --oneline                                                 # reviewed, no matching commits
git diff main...HEAD --stat                                       # reviewed, no matching files
# Conclusion: "the runbook's premise doesn't match what's in the repo" — WRONG,
# because none of these checks ever compared HEAD against origin/<branch>.
```

**Right** — confirm the checkout is actually at the remote tip before concluding anything is absent:

```bash
git fetch origin claude/oracle-cloud-agent-provisioning-scaffold
git log HEAD..origin/claude/oracle-cloud-agent-provisioning-scaffold
# 10 commits printed, including:
#   1a7f7ec8 Add BetterStack-verifiable debug route to vm-ops-api
#   65f533c4 Log self-redeploy output to a host-mounted file + add GET /redeploy/status

git status -sb
# ## claude/oracle-cloud-agent-provisioning-scaffold...origin/claude/oracle-cloud-agent-provisioning-scaffold [behind 10]

git status -s
# (only one safe untracked file — no tracked local changes to lose)

git reset --hard origin/claude/oracle-cloud-agent-provisioning-scaffold
# HEAD is now at 801eb239 Rebuild workflow-cli bundle to pick up VM_PROVISION_GITHUB_TOKEN

grep -n "debug/ping\|redeploy/status" <server-file>
# <server-file>:399:    if (req.method === 'GET' && url.pathname === '/debug/ping') {
# <server-file>:498:    if (req.method === 'GET' && url.pathname === '/redeploy/status') {
```

The only difference between the two paths is one `git fetch` + one comparison against
`origin/<branch>` before drawing the conclusion — and it is the difference between a false claim
(or, in the session-history precedent above, a false fabrication/injection accusation) reported to
the user and the correct diagnosis reached in seconds.