---
title: Background processes: keep the PID, never pkill -f
date: 2026-09-02
category: workflow
tags: [bash, processes, pgrep, pkill]
problem_type: knowledge
module: docs
---

Moved verbatim out of the root `AGENTS.md` on 2026-09-02 (context diet, known-gaps #51). The root keeps a distilled version and points here.

## Background processes: keep the PID, never `pkill -f`

**`pgrep -f` / `pkill -f` are unusable from an agent Bash call in this harness,
and they fail in the direction that destroys work.** Every command is wrapped in
`/bin/bash -c '<the literal command text>'`, so the pattern you are searching for
is *itself* present in the argv of the shell running the search. Demonstrated
directly — `pgrep -af "UNIQUE_MARKER_9f3a"` exits 0 and prints a match, for a
marker no process ever had:

```
24560 /bin/bash -c … eval 'pgrep -af "UNIQUE_MARKER_9f3a" | head -3; …'
```

Two consequences, both observed in one session:

- **Every liveness check is a false positive.** `pgrep -c -f foo` never returns
  0, so "is it still running?" always answers yes — including after the process
  died. A count of 2 or 3 is usually two greps and a wrapper, not two workers.
- **`pkill -f foo` kills the shell that ran it**, and anything chained after it
  in the same invocation. `pkill -f x && relaunch` terminates before `relaunch`
  runs, which reads as "the restart silently failed."

**Instead: keep the handle you were given.** Write the PID at launch and act on
that number — a PID cannot match itself by accident.

```bash
setsid nohup <cmd> > out.log 2>&1 < /dev/null & echo $! > .agents/tmp/<name>.pid
kill "$(cat .agents/tmp/<name>.pid)"          # stop it
kill -0 "$(cat .agents/tmp/<name>.pid)" 2>/dev/null && echo alive || echo dead
```

For liveness, prefer a signal the process actually produces — a fresh line in
its log, a health endpoint — over any form of process-table matching. If you
genuinely must match on a name, use `pgrep -x <exact-binary>` (matches the
executable, not the command line) and never `pkill` on the result without
first printing what it selected.
