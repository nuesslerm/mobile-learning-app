# `wf browser` on the device-lab runbooks: before the migration and at the head

Date: 2026-10-09. Owner request: the before and after of the browser CLI, run on the real runbooks with the device
frame, with screenshots and recordings. Page with the recordings: https://claude.ai/artifact/K8p7f5ZDn3wmLGFUMfPYgu
(private to its owner until shared).

## Method

- Before: `wf` built from `9a08c6a2` (the trunk commit before the standard-crates work), release profile with
  `lto=false` and 16 codegen units. After: the release build of the head, `ba089e948`. Binary sizes are not comparable.
- App: `EXPO_PUBLIC_DEVICE_LAB=1 pnpm --filter mla-tamagui run export:web`, served by `e2e/lib/staticServer.mjs` on
  port 8800 under `/mla-tamagui`. Device: `iphone-15-pro`, device frame, `open --video`.
- Backends: the deployed `remote-chat-agent` on the VM (its `/health` reported `ba089e948`) through
  `wf browser tailnet-forward` for speech to text, the planner and the Tamil marker. `leetcode-backend` for Run Local
  is the local deploy-profile binary on port 8789 with the export's default token, not the production service.
- Input: only the CLI (`tap`, `swipe`, `type --at-caret --human`, `press`, `trace`, `stroke`, `audio-inject`).
  Spoken turns are piper (`en_US-lessac-medium`) clips. One run per runbook per build, driven by a Node script that
  shells out to `wf browser`; the script is scratch and not committed.

## Results

| Runbook | Before | After |
| --- | --- | --- |
| G3 `two-sum` typed into CodeMirror (99 keys, 12 `type` calls, median key gap per call 234 ms before, 245 ms after) | Run Local 12 / 12, `video-check` 14 of 14 | Run Local 12 / 12, `video-check` 14 of 14 (the first complete run said 13 of 14, no dot for the tap at 6,009 ms; the rerun passed) |
| G1 `two-sum` by six spoken turns | identical transcripts; first step only on turn 1; 12 / 12 after turn 5 and 6; `video-check` 30 of 30 | the same; `video-check` 30 of 30 |
| G2 Tamil, seven letters traced on the lesson pad | 99, 99, 97, 100, 99, 99, 99 % on the line; `video-check` 8 of 8 | the same scores; `video-check` 8 of 8 |
| G2 அ written large, live marker | NOT YET · 55, then LOOKS RIGHT · 88 | NOT YET · 40, then LOOKS RIGHT · 92 |

## Differences found

- The pre-migration `wf` answers the device-frame page with `{"ok":false,"error":"Not found"}` when it is started
  outside a checkout; the head build renders it from any directory. Every old-build run started inside the checkout.
- A refused command line printed one line from `wf` before; it now prints clap's error and a usage line that leaves out
  the subcommand (`docs/gaps/wf-browser-usage-omits-subcommand.md`).
- `trace --source template` does not follow the template on the write pad with either binary
  (`docs/gaps/device-lab-tamil-template-trace.md`). The large letter was drawn with `stroke`.

## Not run

Part C of the Tamil runbook (three words), problems 1 and 3 of the voice runbook, the production `leetcode-backend`,
repeated runs for a spread. The Tamil marker is a model, so its score varies for the same strokes.
