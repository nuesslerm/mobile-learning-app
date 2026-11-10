---
description: Delegate a task to a cheap opencode model and relay only its summary
argument-hint: <task description, self-contained>
allowed-tools: Bash(wf offload ask:*)
disable-model-invocation: true
---

Run exactly one command, with the user's text as the prompt (quote it safely):

```bash
wf offload ask --auto --shape implementer:unit --summary=brief --caller cheap --prompt "$ARGUMENTS"
```

The run is the rca service's and it has no checkout: when the task edits or reads named files, add `--files a.ts,b.ts` (what it changes comes back into the repository). Then relay the printed summary verbatim: the model's final message and the bracketed meta line with the `transcript` path. Do not verify it, do not re-read the files it changed, do not open the transcript unless the message hedges or reports an error. If it says `WROTE_NOTHING`, say so and offer to retry with a sharper prompt.
