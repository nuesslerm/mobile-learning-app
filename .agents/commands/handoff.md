---
description: Generate a structured session handoff summary and save it to .agents/tmp/handoff.md. Use when context is getting long (>70% full) or before ending a session that another agent or future session will resume.
allowed-tools: Read, Write, Bash
---

Generate a structured handoff summary for this session using anchored iterative summarization.
Save it to `.agents/tmp/handoff.md` and report the path.

The summary must use this exact structure — every section is mandatory:

```markdown
# Session Handoff
date: $ARGUMENTS  <!-- if no date given, use today -->

## Intent
<!-- What the user is trying to accomplish. One paragraph. -->

## Decisions made
<!-- Bullet list of architectural/design decisions and WHY they were made. -->
- Decision: [what] — Reason: [why]

## Files modified
<!-- Every file touched. Be specific: function names, not just file names. -->
- path/to/file.ts — [what changed and why]

## Files read (not modified)
<!-- Key reference files read. Omit trivial/obvious reads. -->
- path/to/file.ts — [why it was read]

## Current state
<!-- Where things stand right now. Tests passing? Build broken? In-progress? -->

## Blockers / open questions
<!-- Anything unresolved that the next agent needs to know. -->

## Next steps
<!-- Ordered list of what should happen next. -->
1. [concrete action]
```

Rules:
- Never summarize code as prose — preserve exact file paths, function names, error messages, and error codes verbatim
- If a file was modified, name the function(s) changed, not just the file
- Protect early-session constraints: any rules the user stated at the start must appear in Decisions
- Keep it under 400 lines — this file is read at session start, not browsed
