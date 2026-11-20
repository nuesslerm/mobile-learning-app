---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/data/pyodide/harness/judge.py"
  - "frontend/mla-tamagui/features/leetcode/data/pyodide/pyodideRunner.ts"
opened: 2026-11-20
last_verified: 2026-11-20
links: []
---
# A bare `except:` in a learner's hot loop defeats the on-device time limit

## What is broken

The judge stops a runaway solution with a `sys.monitoring` callback that raises `TimeoutError` (a `BaseException`) once the per-case deadline has passed. A solution of the shape `while True: try: work() except: pass` catches that exception on every iteration and goes on for ever. Raising again on every later tick does not help: each raise is swallowed the same way. The runner's hard limit (`runMs` plus the bridge slack) then tries to tear the page down, which needs the page's main thread, and the thread is the one that is stuck. On a phone only the hidden WebView freezes and the app goes on (the runner's next run fails with a timeout stage); in the browser export the page and the app share a thread and freeze together.

The same family: `dispose()` of the runner rejects the queued and the running job but does not cancel an `execute()` that is already awaiting the page, so a run in flight at that moment can still reload the page after the dispose.

## Why blocked

Not blocked. A Pyodide interpreter on the main thread has no other way to be interrupted from the outside.

## What unblocks it

Run the interpreter in a Web Worker with Pyodide's interrupt buffer (`setInterruptBuffer`, needs a `SharedArrayBuffer`, so cross-origin isolation in the WebView and in the web export) or a worker that the page can terminate (issue #86, B2.8, risk R2: only if the phone shows it). For `dispose()`, check the engine's generation after every await in `execute` and bail out without reloading.
