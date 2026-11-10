---
name: cf-review
description: Review the change against its requirements with the cheap read-only reviewer agent first, then adjudicate its findings. Use after /cf-work or when asked to review a change before shipping. Invoke as /cf-review <slug> [--base <ref>].
allowed-tools: Bash(wf offload ask:*), Bash(git diff:*), Bash(git log:*), Bash(git add:*), Bash(git commit:*), Read, Write
---

1. Run `wf offload ask --agent reviewer --summary=brief --caller review "Review the change for plan <slug> (git diff <base, default the commit before the first 'Unit: <slug>/' trailer>..HEAD) against docs/plans/<slug>/requirements.md. Treat docs/plans/<slug>/learnings.md, when present, as known pitfalls: repeating one is a blocker. One finding per line: <blocker|bug|risk|nit> <file:line> <finding>."` and save the message as `docs/plans/<slug>/review.md`.
2. For each `blocker` or `bug`: confirm it against the diff (read the named file:line only), then either append a fix unit to `units.jsonl` and run `/cf-work <slug>`, or fix it by hand when it is one file and a few lines. Dismiss a finding explicitly with a one-line reason. `risk` and `nit` items: list them, fix only if cheap.
3. Commit `review.md` and any hand fix as `cf(<slug>): review`. Report: findings accepted / dismissed / fixed, the commits, and the final `git diff --stat`. End with `FILES_READ: <paths you read yourself>` and `ACTIONS: <one entry per step>`.
   - Invoked directly: the next step is `/cf-compound <slug>`.
   - **Invoked as a step of `/cf-lfg`**: do NOT stop and do NOT announce a next step; continue into the orchestrator's next step. Only an unresolved `blocker` stops an orchestrated run.
