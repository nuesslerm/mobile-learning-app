---
title: device-lab G1 — voice edit, recorded with tap dots
verifies: On the device-lab render (the lab web export at iPhone 15 Pro size, live backends), an agent solves three leetcode problems by spoken edits alone, tapping like a person, and every tap shows as a dot in the recording
tier: agent-driven
packages: frontend/mla-tamagui, frontend/libraries/voice-edit-dsl, backend/remote-chat-agent, clis/wf-harness
maxAgeDays: 30
---

# device-lab G1: voice edit, recorded with tap dots

Gate G1 of the device-lab spec ([`spec.md`](../plans/device-lab/spec.md) §7). It runs the spoken turns of
[`e2e/voice-edit.lua`](../../e2e/voice-edit.lua) (first written as [`voice-edit-iterative-solve.md`](../archive/README.md)) on the device-lab render, the way a person
would run it, and records every problem on video.

An agent runs it step by step through `wf browser`, on a commit that is already on `origin/dev`, and writes the run to `docs/runbooks/.results/device-lab-voice-edit.json` (`runbook`, `sha`, `ranAt`, `verdict`, `evidence`). A commit not yet on `origin/dev` would leave the recorded sha orphaned by a later rebase.
It needs the phase 3 driver work: touch
taps (I13), the input log and `wf browser check-taps`.

## Rules: the human input model

- **Act only through input.** Taps go through `wf browser tap-on`. Speech goes through `wf browser set-microphone-input`:
  a spoken clip set as the microphone's next recording, which starts playing when the app starts recording.
  `wf browser get-element` and `wf browser hierarchy` read the page; nothing clicks, focuses, sets a value or
  dispatches an event.
- **Touch taps with dots.** Every tap is a touch tap that draws its dot.
- **Look before each decision.** Take a screenshot (`wf browser take-screenshot .agents/tmp/device-lab-g1/<step>.png`)
  or read `wf browser hierarchy` before choosing the next step, as a person reads the screen.
- **Voice only.** Nothing is typed into the editor or into the transcript.

## Setup

```bash
wf browser tailnet-forward --host remote-chat-agent-prod --port 8787 --local-port 8787
wf browser tailnet-forward --host leetcode-backend-prod --port 8789 --local-port 8789   # use the name `wf vm net device list` shows; it was leetcode-backend-prod-2 on 2026-10-05
# While remote-chat-agent-prod is off the tailnet (docs/gaps/device-lab-live-agent-unreachable.md), run the
# agent from this checkout instead, as production runs it: opencode serve with the vault's provider keys (the
# setup in device-lab-tamil-writing.md), the faster-whisper sidecar production runs (tiny.en), and the agent
# pointed at both. All three run in the background; the export's agent URL is http://127.0.0.1:8787 either way.
#   python3 -m venv .agents/tmp/stt-venv && .agents/tmp/stt-venv/bin/pip install -r backend/remote-chat-agent/stt/requirements.txt
#   STT_MODEL=tiny.en PORT=8790 .agents/tmp/stt-venv/bin/python backend/remote-chat-agent/stt/server.py
#   (cd backend/remote-chat-agent && STT_URL=http://127.0.0.1:8790/transcribe OPENCODE_SERVER_URL=http://127.0.0.1:4096 \
#     pnpm exec tsx --env-file=.env.defaults src/index.ts)
# The lab export with live backends (S1). Run Local needs the credential, which is inlined
# into dist/: never publish or commit dist/.
EXPO_PUBLIC_DEVICE_LAB=1 EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN="$(wf secret get VM_GATEWAY_AUTH_SEED)" \
  pnpm --filter mla-tamagui run export:web
# Serve under the export's baseUrl (/mla-tamagui). Served at the root, a dynamic route such as
# /tamil/uyir-1 renders "No lesson with id [lessonId]" (dry run, 2026-10-05).
mkdir -p .agents/tmp/device-lab-serve && ln -sfn "$PWD/frontend/mla-tamagui/dist" .agents/tmp/device-lab-serve/mla-tamagui
setsid nohup node frontend/mla-tamagui/e2e/lib/staticServer.mjs .agents/tmp/device-lab-serve 8800 \
  > .agents/tmp/device-lab-serve.log 2>&1 & echo $! > .agents/tmp/device-lab-serve.pid
# The voice: piper, offline.
python3 -m venv .agents/tmp/tts-venv && .agents/tmp/tts-venv/bin/pip install piper-tts
.agents/tmp/tts-venv/bin/python -m piper.download_voices en_US-lessac-medium --data-dir .agents/tmp/voice-models
mkdir -p .agents/tmp/device-lab-g1
```

As in the source runbook, put no proxy in front of the two backends: the device frame's live network reaches them.

## One spoken turn

Every row of the source runbook's problem tables is one turn. Every control is tapped by its id; a selector finds it in
the app frame, beside the editor's own frame.

1. **Say it.** Synthesise the row's `say` text and queue it:
   `printf '%s' "<say>" | .agents/tmp/tts-venv/bin/piper -m .agents/tmp/voice-models/en_US-lessac-medium.onnx -f <wav>`,
   then `wf browser set-microphone-input <wav>`.
2. **Record.** `wf browser tap-on --id ProblemEditorVoiceStartButton`; wait until the recorder took the clip
   (`wf browser extended-wait-until --condition "device:get_device_state('microphone').consumed" --timeout 5000`) and
   the clip's length has played; `wf browser tap-on --id ProblemEditorVoiceToolbar-Send`.
3. **Read the transcript** in `ProblemEditorVoiceReviewSheet-TranscriptInput` and take a screenshot. A mis-hearing
   that keeps the meaning ("paleontrome" for palindrome) is sent as it is; the planner reads intent. One that
   changes it (a name, a number, an operator) is said again: queue the clip once more, slower
   (`--length-scale 1.15`), and tap `ProblemEditorVoiceReviewSheet-ReRecord`, which starts a new recording.
4. **Send** with `ProblemEditorVoiceReviewSheet-Send`. The edit applies on its own, or the alternative picker
   opens: read each row's label, summary and invalid reason, and tap the row that does what was said
   (`ProblemEditorAlternativePicker-Row-<n>`), or `ProblemEditorAlternativePicker-Close` when every row is
   invalid (the whole-solution ask).
5. **Put the keyboard away.** The editor takes focus when the page opens and after every applied edit, and the
   keyboard then covers the mic and Run Local. A native build does the same: the editor's WebView sets
   `keyboardDisplayRequiresUserAction={false}`. Bring Done into view
   (`wf browser scroll-until-visible --element-id EditorAction-Done --from-id EditorActionToolbar --direction RIGHT`)
   and `wf browser tap-on --id EditorAction-Done`.
6. **Run Local.** `wf browser tap-on --id ProblemEditorRunLocalButton` and read
   `ProblemEditorRunOutcomeTotals` (or `-RuntimeError`, `-CompileError`) with `wf browser get-element`; take a screenshot.

Timing comes from the app itself: each turn posts an `edit_stats.client_turn` row to the agent, whose log shows
`transcribeWallMs` (the gates' transcribeMs), `postConfirmWaitMs` (planMs: Send on the transcript to the applied
edit or the picker) and `localFastPath` (the rename), and the planner's own `voice_plan.timing` line
(`plannerMs`, `cacheRead`). An in-page waiter that only listens (a passive `touchend` listener and polling) may
time the same spans from the touch, as a cross-check.

## Steps

Each problem gets its own browser, so each has its own video.

| # | Do | Expect |
| --- | --- | --- |
| 1 | `wf browser start-device --device voice --screen-size iphone-15-pro`, `wf browser start-recording .agents/tmp/device-lab-g1/<problem>.mp4`, then `wf browser open-link http://127.0.0.1:8800/mla-tamagui/leetcode/valid-palindrome`; screenshot | The problem in the screen frame: status bar, home indicator, the Python stub, the keyboard up |
| 2 | Put the keyboard away (turn step 5) | The mic and Run Local in view |
| 3 | One turn for every row of problem 1 in `e2e/voice-edit.lua` | That turn's expected code; record the turn's transcript, outcome, timings and the Run Local count in the step notes |
| 4 | `wf browser stop-recording`, then `wf browser stop-device --device voice` | Names the `.mp4`; its input log lies beside it in `.agents/tmp/device-lab-g1/` |
| 5 | `wf browser check-taps .agents/tmp/device-lab-g1/<problem>.mp4` | A dot at every logged tap |
| 6 | Steps 1–5 for problem 2 (`/mla-tamagui/leetcode/two-sum`) and problem 3 (`/mla-tamagui/leetcode/container-with-most-water`) | As above |
| 7 | Publish `.agents/tmp/device-lab-g1` (videos, screenshots, input logs) as one page | Its link; put it in the result record and the last step's notes |

If a problem's editor is not on its stub when it opens, reset it first: `wf browser tap-on --id ProblemEditorResetButton`,
then `wf browser tap-on --id ResetCodeDialog-Confirm`.

## Gates

1. **The source runbook's gates 1–8** hold for all three problems: voice only, monotonic progress, buffer
   always parses, one repair at most, whole-solution asks refused, renames local, latency, caching on. A
   rename is checked by its outcome under the name the transcript carries (whisper writes "lo" as "low").
2. **All-pass.** Run Local reports every test passing at the end of each problem (`14 / 14` for
   `valid-palindrome`).
3. **Taps are visible.** `check-taps` passes for every video.
4. **Input only.** The input log holds only taps, swipes and the microphone.

The gate fails on any miss. Fix the app, the mock or the driver, then run it again; never relax a gate.

## Recording the run

Write `docs/runbooks/.results/device-lab-voice-edit.json` by hand from the step notes: `runbook`, `sha` (the
`origin/dev` commit the run used), `ranAt`, `verdict`, a `criteria` map with one entry per gate, `notes`, and
`evidence` (the published page, per-problem Run Local counts, the turn timings). Then tick G1 in
[`progress.md`](../plans/device-lab/progress.md) with the record's sha and the artifacts link.
