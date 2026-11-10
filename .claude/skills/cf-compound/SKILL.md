---
name: cf-compound
description: Document one solved, verified, non-trivial problem as a durable docs/solutions learning under the compound-engineering contract (schema-checked frontmatter, corpus-first category, update over duplicate, CONCEPTS.md proposals), drafted by the cheap researcher and approved by Claude. Use after a change ships or to capture a learning. Invoke as /cf-compound <slug> [mode:non-interactive].
allowed-tools: Bash(wf offload ask:*), Bash(ls docs/solutions:*), Bash(git add docs/solutions/*), Bash(git add CONCEPTS.md), Bash(git commit:*), Read, Write, Edit
---

Preconditions (judge them from the session, do not ask): the problem is solved, verified working, and non-trivial. One learning per run. When nothing qualifies, write nothing and say why.

Mode: with `mode:non-interactive` (or a hands-off caller) ask nothing and end on exactly one terminal line, `Documentation complete` or `Documentation skipped: <reason>`. Otherwise you may ask one question when the update-vs-create choice is unclear.

The contract lives in `references/`: `schema.json` (frontmatter fields and enums), `resolution-template.md` (bug and knowledge track bodies), `concepts-criteria.md` (CONCEPTS.md rules).

1. If `docs/plans/<slug>/learnings.md` names an existing doc covering the same problem, root cause and solution, update that doc; otherwise create a new one.
2. Draft, ONE call: `wf offload ask --agent researcher --summary=brief --caller compound "Draft a docs/solutions learning for plan <slug>: <the learning in one line>. Read docs/plans/<slug>/ and the change (git log --grep 'cf(<slug>)'). Follow .agents/skills/cf-compound/references/resolution-template.md; frontmatter must satisfy .agents/skills/cf-compound/references/schema.json; pick the category from ls docs/solutions. Reply with the full document, then up to 3 CONCEPTS.md proposals (ADD|REFINE|FOLD) per concepts-criteria.md."`.
3. Check the frontmatter against `schema.json` yourself (delimiters, required fields, enums, bug-track fields) and fix it. Write `docs/solutions/<category>/<title-slug>.md` (or the updated doc with `last_updated` stamped). Fix wrong facts: read the defining line before asserting behaviour. Commit as `cf(<slug>): compound <path>`.
4. Apply or drop each CONCEPTS.md proposal by hand (one-sentence definitions, no file paths, no version claims); commit `CONCEPTS.md` as `cf(<slug>): concepts` when it changed.
5. Report: the file (created | updated), track, category, the CONCEPTS.md outcome (`scanned, no qualifying terms` | `N added, N refined, N folded`), the commits. End with `FILES_READ: <paths you read yourself>` and `ACTIONS: <one entry per step>`, then the terminal line.
