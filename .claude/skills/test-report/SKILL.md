---
name: test-report
description: Build one report Artifact for every published test run that has no report yet (run bundles in the verification-artifacts bucket, newer than lastPublishedAt in docs/runbooks/.results/test-report-state.json). Use when asked for a test report, "report the latest runs", or when the test-report clock fires (every 6 h).
license: MIT
metadata:
  origin: custom
  category: testing
  claude_code: supported
---

# test-report

Every CI test run publishes a bundle (`wf test publish`): report, screenshots, recordings, timing and its metrics row, listed
newest first in the bucket's `verification/test-runs/index.json`. This skill turns the runs that have no report yet into one
report page. The test-report clock (every 6 h) fires a tick into the `<reports-session>` and the session runs it; anyone can ask for it.

## 1. Which runs

1. Read `lastPublishedAt` from `docs/runbooks/.results/test-report-state.json` on `origin/dev`
   (`git fetch origin dev && git show origin/dev:docs/runbooks/.results/test-report-state.json`).
2. `wf test results --query "since=<lastPublishedAt>" --json` lists the runs published after it, newest first, from the
   test_runs table and the bundle index joined on runId (`runs[]`: runId, test, unit, platform, sha, verdict, durationS,
   publishedAt, screen, warnings, recordings, readBack, phases, byVerb, timing, report, bundle; `wf test --help`). Without
   `wf`, fetch the index directly:
   `https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/test-runs/index.json`.
3. No new runs: say so in one line and stop. Commit and push nothing, not even a state file.

## 2. What each run holds

For every new run fetch its `bundle.json`: report {html, json}, screenshots[], recordings[], debug, timing, timingSummary,
screen, warnings, readBack, metrics, files[]. Fetch each report.json for steps, warnings, goldens, stubs and debug hits.
Download every screenshot and recording into the scratchpad and look at the screenshots.

## 3. The page

One Artifact (HTML, title "Test runs <date>"):

- Media never hot-links the bucket. Publish once with capabilities `{"assets": {}}`, upload every screenshot and recording
  with `asset: true` (`file_paths`, up to 25 per call), reference each by the `/_blob/<id>` url the upload returns, and keep
  the bucket URL as a link beside it.
- The same design language as the harness report pages (`clis/harness-core/src/run_report.rs`): colour tokens on `:root` with a dark
  set under `prefers-color-scheme: dark` (pass green, fail/error red, muted grey, one accent); a verdict band at the top
  (passed/failed/errored counts, the date, the sha range); chips for harness and verdict; a row of tiles (runs, wall time,
  steps, screenshots, recordings, debug hits, warnings); a summary table; then one card per run, failed and errored first.
- Each card: verdict band; facts (script, what it does, commit, screen, started, finished, ran on, message); the screenshots
  one per row at phone width with a numbered caption saying what each shows; every recording as
  `<video controls playsinline preload="metadata" poster="<a screenshot>">` with a `<source type="video/mp4">` and a link; a
  numbered step table with tabular-nums timings (the # is the action id the on-page markers carry); the debugger hits as
  `<details open>` (file:line, function, app frames first, library frames greyed, the locals that matter); the timing table
  with per-verb share bars; network stubs with their hits; warnings ("none" when empty); a read-back mark from `readBack`.
- A "Over time" section above the cards: `wf test results --query "since=14d" --json`, one small inline-SVG line chart per
  test and unit (x: publishedAt, y: durationS, with the 60 s budget as a dashed line, failed runs as red points), and per
  test a stacked bar per run of its phases (setup, script, teardown, report, publish) and its top three verbs by seconds
  from `byVerb`, so a slower start-up or a slower `find` shows as a trend, not one number.
- `viewport-fit=cover` and `env(safe-area-inset-*)` padding, no mid-word wraps (nowrap on ids, kinds and times), nothing
  wider than the viewport at 393 px; ASCII separators only.
- Before the final publish, render it through the browser harness at the phone preset and look at it: copy the HTML and the
  media (at the same `/_blob/<id>` paths) into a scratchpad dir, serve it (`python3 -m http.server`), then
  `wf browser test <lua> --frame document --no-record --env base=<url> --env pages=index,s900,s1900 --env out=<dir>`
  with a script that opens each page, waits for it and takes a screenshot (`device:take_screenshot`) (`sNNN.html` copies scroll to NNN px on load). Fix what
  looks wrong, then publish.

## 4. The state

Write `{"lastPublishedAt": <newest publishedAt>, "lastReport": <page url>, "runs": n}` to
`docs/runbooks/.results/test-report-state.json` and one ledger line (page URL, run count, pass/fail/error counts) to
`docs/plans/device-test-process/progress-2026-10-14.md`, in a scratch worktree of `origin/dev` so a session's own work is
never touched:

```bash
git fetch origin dev && git worktree add -f "$SCRATCH/report-wt" origin/dev && cd "$SCRATCH/report-wt"
# edit the two files, then:
git add -A && env -u CLAUDE_CODE_REMOTE git commit -m "Test report: <n> runs (<p> passed, <f> failed, <e> errored) [skip ci]"
git push origin HEAD:dev   # on a rejected push: git fetch origin dev && git rebase origin/dev, then push again
cd - && git worktree remove "$SCRATCH/report-wt"
```

`[skip ci]` keeps the report commit from starting Test and Deploy, whose cancel-in-progress jobs it would otherwise cancel. A
red lefthook hook is yours to fix, never to bypass. Never put a model id in a commit.

## 5. Reply

Three lines: the page URL, the counts, the newest run's id.
