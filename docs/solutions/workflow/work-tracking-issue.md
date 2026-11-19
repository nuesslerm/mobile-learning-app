---
title: "Track long-running work in one GitHub issue: its shape, how to add work, update it and complete it"
date: 2026-11-19
category: workflow
module: git-workflow
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "Starting work that spans many items, sessions or commits, or being asked to drive an issue to completion"
  - "Adding an item, changing an item's status, closing a checkpoint or listing a blocker"
tags: [issue, plan, tracking, state, done, blocked]
---

# Track long-running work in one GitHub issue

Issue #83 is the model: one issue holds the goal, the decisions, every item with its status, what is blocked and what is done, so a person (on a phone) or an agent (after a compaction) reads one body and knows where the work stands. The issue body is the plan and the record; the code, the tests and the docs hold the detail.

## The shape of the body

In this order, each a `##` heading:

1. **Goal**: what is true when the work is finished, in a paragraph; then **Where it stands** (one paragraph, rewritten at a checkpoint, not appended to), **Order** (the sections in the order they run, and the checkpoints between them), **Item style**, **Closing** and **Rules** (the standing constraints of this work).
2. **Decisions**: a numbered list, one decision each, with its date and who made it (`owner direction, 18 Nov`). A decision is never silently reversed: add a new numbered one that says it supersedes the old.
3. **Sections**, one per area of work, lettered (`M`, `R`, `O`, `G`), each with a short intro and a link to its plan doc when it has one.
4. **Blocked**, **Done** and **Parked** (below).

An **item** is a `###` heading inside a section, named `<letter><number>. <what>`, then a flat list:

```
- [ ] **Local:** implemented and tested locally (flow test where one can exist)
- [ ] **CI:** proven in its section's checkpoint run on the `dev` head
- **Do:** what to build, with the paths and symbols, as imperatives; the decisions it follows.
- **Proof:** what shows it works, by type (flow, chain, replay, integration, deployed) and what each must show.
- **Needs:** what must exist first (another item, a sign-off, a resource); omit when nothing.
- **State:** where it is now, in facts; `not started` until it starts.
```

The **Local** box is ticked when the work is done and tested in the sandbox; the **CI** box only by a green checkpoint (below). A todo names its proof; an item with no way to prove it is not an item yet.

## Adding work

- New work goes in the section it belongs to, or a new lettered section when it is a new area: a `###` item with both boxes unticked, `State: not started`, and its Do, Proof and Needs written before work starts. Research that shapes the plan is summarised into the item and linked (a plan doc under `docs/plans/`); raw notes stay out of the body.
- A choice the owner made goes in **Decisions** with its date and the words of the choice. A choice that is still the owner's goes in the item as **Open:** with the options and your recommended default; carry on with the default.
- Change the body only. Never add comments to the issue.

## Updating while working

- Edit an item's **State** line when its status changes, in facts (what landed, what is left), and nothing else. No narrative, no per-commit evidence, no run links while the work is open; the commit log and the checkpoint run hold those.
- Tick **Local** when the item is implemented and tested locally.
- A **Rules** or **Decisions** change is its own edit with its date.

## Completing

- A **checkpoint** is one hosted run on the `dev` head that covers the sections it names. Do not start the next section until it is green; a red one is fixed first.
- With the checkpoint green: tick the **CI** boxes of its items, and for each finished item replace its text with one line under **Done**: `- **<item> (<short name>):** <what landed>: <commit>, <commit>, <run>`, where the run is the hosted run's URL as a markdown link labelled `run <id>`. One run link per line, the run that proved it.
- The work is finished when every item is done or in **Blocked**, and the last hosted run on the head is green. The final message is a short summary per section of what landed.

## Blocked and parked

- **Blocked** is a list of unticked items: `- [ ] **<area>: <item>.** **Blocked <date> on <who or what>:** what was tried and the real error`, plus what unblocks it. An item is blocked only after a concrete attempt failed with a real error (look for the credential, install the tool first); not for lack of time.
- **Parked** is work dropped on purpose, one line each with the reason; it is not a to-do list.

## Editing the body from an agent

The body is edited whole through the REST API (the sandbox blocks `gh` GraphQL). Always read, change and write back in one go, so another edit is never overwritten:

```
gh api repos/<owner>/<repo>/issues/<n> --jq .body > body.md      # read the live body
# change only the section you mean to, with a script (python, not a hand copy)
gh api -X PATCH repos/<owner>/<repo>/issues/<n> -F body=@body.md  # write it back
```

Keep every other section byte for byte. Check the live body afterwards for what you meant to add.
