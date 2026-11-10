---
name: cf-dispatch
description: Delegate well-specified coding work (non-trivial edits in 1-3 files, mechanical multi-file changes, procedural research, anything with large output) to a cheap opencode model and read back a compact summary. Use when a task is mechanical or its output would bloat your context; not for a single Read, a one-line fix, design, review, or security work.
license: MIT
allowed-tools: Bash(wf offload ask:*)
---

# cf-dispatch

Claude orchestrates; a cheap model executes. Three numbers to keep in mind:

- A Claude subagent spawn costs ~100K cache-write tokens before it does any work. Never delegate through the `Agent` tool.
- A `delegate` call or a Bash `wf offload ask` rides your already-warm cache; you pay only for the summary it returns (a few hundred tokens).
- Dispatch follows the ladder in `libraries/rca-core/plugins/ladder/data/model-ladders.json`: rungs per shape (`implementer:<tier>`, `judge:text`, `judge:vision`, `document`, `runbook`; `rca run --shape` picks one), rotate on any non-fatal failure, escalate on a second identical failure, and skip cold pools — Opus/Fable (`automatic: false`) are tried only when pinned. Every run logs real tokens and cost; `wf offload ask --stats` shows the totals.

## Default: whole workflows, not micro-steps

Prefer handing over an entire unit of work and reading one result:

| Surface | When |
| --- | --- |
| `delegate` MCP tool (`task`, `files`, `mode`) | Any single unit of mechanical work; structured, no prompt file. `delegate_continue(runId, prompt)` refines it in the same opencode session. |
| `/cheap <task>` | Same thing from a slash command. |
| `wf offload ask --auto --shape implementer:unit --files a.ts,b.ts --summary=brief "…"` | Same from Bash (as `opencode run`, edits and commands are allowed either way and `--auto` approves what opencode would ask; for a run that must not change files use `--agent read-only`; `--shape implementer:unit` rotates away from a model that wrote nothing; `--files` sends the files it reads or changes, since the rca service has no checkout, and `-f` also attaches a file to the message; `-c`/`-s <id>` continue a session, `--agent`, `--variant` and `--title` are opencode's) (`wf offload ask --prompt-file <file>` takes a prompt file; `-` reads stdin). `--summary` (full) adds tool lines + diff stat. |
| `wf offload ask --batch <glob> -t template.md` | The same change across many files ({{file}} / {{content}} placeholders); one table back, then `git diff --stat`. |
| `wf offload ask --runbook <name>` | Execute a runbook on the cheap model (`--runbook-cli claude` escalates). |

## Routing table

| Task shape | Do |
| --- | --- |
| 1 file, ≤10 lines, location known; one Read/Grep; a question the current context already answers | **Direct.** A dispatch takes 40–400 s. |
| 1 file with non-trivial logic or unknown location; 2–3 files, one goal; a checklist-style investigation; output >1.5 KB | **Delegate.** |
| >3 files or multi-step logic | **Decompose**: one delegate per unit, check `git diff --stat` between units. |
| Architecture/design, planning, code review, security-sensitive changes, live debugging | **Stays on Claude.** |

## Writing the prompt

```
Stack: TypeScript, vitest. Key files: clis/foo/src/x.ts, clis/foo/tests/x.test.ts
TASK: Rename `frobnicate` to `normalize` in both files and update the test names.
CONSTRAINTS: no other edits; keep the export list unchanged.
VERIFY: grep -n "normalize" clis/foo/src/x.ts shows the new name; grep -c frobnicate clis/foo returns 0.
```

One task per prompt. Name the exact files. Ask for a grep-based VERIFY, not a re-read. English.

## Reading the result

`--summary=brief` (what `/cheap` uses; always write it with `=`, a bare `--summary` swallows the next word) prints the model's final message and `[served by … | tokens=… | cost=$… | 42s | run <id> | transcript <path>]`, nothing else. Relay it verbatim; do not verify it. `--summary` (full) adds the `[read]/[edit]/[shell]` lines and `git diff --stat` for when you are orchestrating several units. Only open the transcript or the files when the message hedges, contradicts the task, or reports an error. `--agent implementer|researcher|reviewer` runs under one of the project's opencode agents (pinned model, permissions, step budget).

Red flags:

- `WROTE_NOTHING`: exit 0, no file changed. Fix the prompt (name the file, say "make the edit now, do not describe it") and retry. Max 3 attempts, then do it yourself.
- `[failed after: …]`: every provider capped or a hard error. Read the tail, fix or do it yourself.
- No `[edit]` after a long wait on an edit task: it is exploring. Tighten `Key files`.

## Never

- Delegate through the `Agent` tool.
- Write a prompt file for a short prompt (use `--prompt` or the MCP tool).
- Re-read every changed file as a matter of course.
- Delegate a judgment call you cannot name.

## Where the code lives

`libraries/rca-core` (the ladder, run planner, parser, stats) and `clis/offload/src/dispatch*.rs` (`wf offload ask` and its `--batch`, `--stats`, `--savings`, `--runbook` modes). Run log: `.agents/tmp/opencode-dispatch-runs.jsonl`.
