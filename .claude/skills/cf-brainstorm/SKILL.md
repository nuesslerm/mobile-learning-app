---
name: cf-brainstorm
description: Turn a vague idea into a requirements document by asking one question at a time, with repository research delegated to the cheap researcher agent. Use for "let's brainstorm", "think this through with me", "what should we build", or any new feature before planning. Invoke as /cf-brainstorm <idea>.
allowed-tools: Bash(wf offload ask:*), Bash(git status:*), Bash(git add docs/plans/*), Bash(git commit:*), Bash(ls:*), Write
---

You are the thinking partner; the cheap model is the researcher. Slug: a kebab-case name for the idea (ask if unclear). Everything lands in `docs/plans/<slug>/`.

Mode: if the arguments contain the token `mode:non-interactive`, or the caller says the run is hands-off / unattended, ask nothing: take the most conservative default at every choice below, record it under Decisions with the alternatives, and put anything a human should confirm under Open questions. Strip `mode:` tokens from the idea before using it. Otherwise ask as written.

1. Restate the idea in two sentences and name what is unclear. Do not read the codebase yourself yet.
2. Consult prior learnings first, ONE call: `wf offload ask --agent researcher --summary=brief --caller learnings "Follow .agents/skills/cf-compound/references/learnings-researcher.md. Activity: <the idea in one line>. Concepts: <named ideas>. Domains: <area>."`. Save the printed message as `docs/plans/<slug>/learnings.md`. Fold its Recommendations into your questions and into Decisions (a learning you decide against is a Decision with the alternative named). Then, if the idea needs facts from the repo, run ONE research call: `wf offload ask --agent researcher --summary=brief --caller research "<the concrete questions, one per line>. Answer with file paths and line references."` and save the message as `docs/plans/<slug>/research.md`.
3. Ask the user one question at a time, multiple choice where possible, until purpose, users, scope boundaries, constraints and success criteria are pinned. Offer 2-3 approaches with trade-offs and recommend one.
4. Write `docs/plans/<slug>/requirements.md`: Purpose, Users, Scope (in / out), Constraints, Success criteria (checkable), Decisions (with the alternatives rejected), Open questions. Under 120 lines.
5. Commit: `git add docs/plans/<slug>/ && git commit -m "cf(<slug>): requirements"`.
6. Do not plan or implement here. End with two trailer lines: `FILES_READ: <paths you read yourself, or none>` and `ACTIONS: <one short entry per step>`.
   - Invoked directly: stop, and say the next step is `/cf-plan <slug>`.
   - **Invoked as a step of `/cf-lfg`** (`mode:non-interactive`, or a hands-off caller): do NOT stop and do NOT announce a next step; continue straight into the orchestrator's next step. Only a failed GATE stops an orchestrated run.
