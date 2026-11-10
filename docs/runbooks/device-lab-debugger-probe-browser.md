---
title: device-lab debugger probe — a breakpoint in the app's code in the browser
verifies: A Reset of the Two Sum editor in the browser stops at two lines of ProblemEditorScreen.tsx through `--logpoint`; the hits carry the stack at original source lines, the top frame's locals and the captures, and the run's report shows them.
tier: agent-driven
maxAgeDays: 30
e2e: e2e/debugger-probe.lua
---

# device-lab debugger probe in the browser

The test is unfocused, so CI never runs it. Build the web export of the tree (`wf test prepare --web`: it carries source maps, so a
hit resolves to its original line), focus the test (`focus: true` in the header of `e2e/debugger-probe.lua`) and run it with the
two points as test flags:

```bash
wf test prepare --web
wf test run debugger-probe --out <dir> \
  -- --logpoint features/leetcode/ui/ProblemEditorScreen.tsx:504 --logpoint features/leetcode/ui/ProblemEditorScreen.tsx:513
```

`--logpoint` attaches to the page's V8 through the browser's debugger, maps `file:line` through the export's source maps and
re-arms every point after a reload. Each hit is a line of `rec/debug.jsonl` and a row of the run's `rec/report.html`.

Pass: the editor is back at its starter code, zero JS warnings, and `debug.jsonl` holds a hit at line 504 (the request) and one at
line 513 (the confirm handler), each with frames whose top is `ProblemEditorScreen.tsx`.

The script types `# probe edit` on the editor's last line first, lowers the keyboard with Done, screenshots the Reset
dialog, confirms, and asserts the line is gone (screenshots `edited`, `before-reset`, `reset-dialog`, `after-reset`).

Unverified: this browser form has not been run since the Android arm was dropped (22 Oct); the first run, with the export and the command above, records whether the two hits land at 504 and 513.

There is no Android twin: Expo Go's Hermes was only reachable through the dev Metro's inspector, which is retired.
