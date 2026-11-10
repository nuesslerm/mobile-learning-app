---
title: device-lab G2 — Tamil letters and words drawn on the canvas, marked right
verifies: On the device-lab render (the lab web export at iPhone 15 Pro size, live backends), an agent writes Tamil letters and words on the canvas with human touch strokes, and they pass the app's similarity checks — the local on-the-line score for traced letters and the live marker's LOOKS RIGHT for letters and words written over the template
tier: agent-driven
packages: frontend/mla-tamagui, frontend/libraries/tamagui-design-system, frontend/libraries/gesture-geometry, backend/remote-chat-agent, clis/wf-harness
maxAgeDays: 30
---

# device-lab G2: Tamil letters and words, marked right

Gate G2 of the device-lab spec ([`spec.md`](../plans/device-lab/spec.md) §7). An agent writes Tamil on the
canvas the way a person does with a finger, and the app marks it right.

An agent runs it step by step through `wf browser`, on a commit that is already on `origin/dev`, and writes the run to `docs/runbooks/.results/device-lab-tamil-writing.json` (`runbook`, `sha`, `ranAt`, `verdict`, `evidence`).
It needs the phase 3 driver work: human touch strokes (I13), `wf browser draw-strokes-on`, the
input log and `wf browser check-taps`.

## Rules: the human input model

- **Act only through input.** Strokes go through `wf browser draw-strokes-on`; taps through `wf browser tap-on`;
  scrolling through `wf browser swipe` or `wf browser scroll-until-visible`. `wf browser get-element` and
  `wf browser hierarchy` read the page; nothing clicks, focuses, sets a value or dispatches an event.
- **Write from what the screen shows.** On the lesson pad, follow the guide line the pad draws
  (`wf browser draw-strokes-on --id TamilPracticeCanvasSurface --source guide`). On the write pad, write over the faint
  template (`--source template`, which reads the template's centre lines off the pad's pixels).
  Never take strokes from glyph tables, course data or an injected SVG.
- **Human strokes.** Every stroke is a touch stroke at hand speed, with tremor (`draw-strokes-on`'s default;
  `--human=false` draws straight and evenly).
- **Look before each decision.** Take a screenshot before each letter, after each stroke and after each check.

## Setup

```bash
wf browser tailnet-forward --host remote-chat-agent-prod --port 8787 --local-port 8787   # the live marker
# While remote-chat-agent-prod is off the tailnet (docs/gaps/device-lab-live-agent-unreachable.md), run the
# same marker from this checkout instead, with the vault's provider keys, as production does:
#   (cd .agents/tmp/marker-ws && MINIMAX_API_KEY=$(wf secret get MINIMAX_API_KEY --backend bitwarden) \
#     KIMI_API_KEY=$(wf secret get KIMI_API_KEY --backend bitwarden) \
#     OPENCODE_API_KEY=$(wf secret get OPENCODE_GO_API_KEY --backend bitwarden) \
#     OPENCODE_CONFIG="$PWD/../../../backend/remote-chat-agent/opencode-server/server-config.json" \
#     opencode serve --hostname 127.0.0.1 --port 4096)
#   (cd backend/remote-chat-agent && OPENCODE_SERVER_URL=http://127.0.0.1:4096 \
#     pnpm exec tsx --env-file=.env.defaults src/index.ts)   # listens on 8787
# Both run in the background; the export's agent URL is http://127.0.0.1:8787 either way.
EXPO_PUBLIC_DEVICE_LAB=1 pnpm --filter mla-tamagui run export:web
# Serve under the export's baseUrl (/mla-tamagui). Served at the root, a dynamic route such as
# /tamil/uyir-1 renders "No lesson with id [lessonId]" (dry run, 2026-10-05).
mkdir -p .agents/tmp/device-lab-serve && ln -sfn "$PWD/frontend/mla-tamagui/dist" .agents/tmp/device-lab-serve/mla-tamagui
setsid nohup node frontend/mla-tamagui/e2e/lib/staticServer.mjs .agents/tmp/device-lab-serve 8800 \
  > .agents/tmp/device-lab-serve.log 2>&1 & echo $! > .agents/tmp/device-lab-serve.pid
mkdir -p .agents/tmp/device-lab-g2
wf browser start-device --device tamil --screen-size iphone-15-pro
wf browser start-recording .agents/tmp/device-lab-g2/g2.mp4
```

## Steps

### A. Letters on the lesson pad (local similarity check)

| # | Do | Expect |
| --- | --- | --- |
| 1 | `wf browser open-link http://127.0.0.1:8800/mla-tamagui/tamil/uyir-1`; `wf browser assert-visible --id TamilPracticeStrokeStep --timeout 30000`; screenshot (`wf browser take-screenshot .agents/tmp/device-lab-g2/<step>.png`) | The stroke coach on அ: `TamilPracticeStrokeStep` reads "Stroke 1 of N" |
| 2 | For each stroke: `wf browser draw-strokes-on --id TamilPracticeCanvasSurface --source guide`, then a screenshot | The step advances to the next stroke. A refusal (the step stays, with feedback) gets one more attempt |
| 3 | After the last stroke, read `TamilPracticeStrokeScore` (`wf browser get-element --id TamilPracticeStrokeScore`) | "NN% ON THE LINE" with NN ≥ 80; note it with the glyph |
| 4 | `wf browser tap-on --id TamilPracticeNext`, the › above the card. "Got it" sits below the fold, and only a swipe reaches it (I13): `wf browser scroll-until-visible --element-text "Got it"` | The next letter, back on stroke 1 |
| 5 | Steps 2–4 for every letter of `uyir-1`: அ ஆ இ ஈ உ ஊ | Six letters scored |
| 6 | Navigate to `/mla-tamagui/tamil/uyirmei-k`; steps 2–3 for its first letter, க | Seven letters scored |

### B. A letter written large (live marker)

| # | Do | Expect |
| --- | --- | --- |
| 7 | Navigate to `/mla-tamagui/tamil/write/uyir-a`; screenshot | The large pad with the faint அ template |
| 8 | `wf browser draw-strokes-on --id TamilWriteCanvasSurface --source template`; screenshot | Ink over the template |
| 9 | `wf browser tap-on --id TamilWriteCheck`; `wf browser assert-visible --id TamilWriteVerdictBadge --timeout 30000`; screenshot | "LOOKS RIGHT · <score>". On "NOT YET", tap `TamilWriteClear` and write it once more |

### C. Words (live marker)

| # | Do | Expect |
| --- | --- | --- |
| 10 | Steps 7–9 on `/mla-tamagui/tamil/write/word-uyir-a` | அம்மா: "LOOKS RIGHT · <score>" within two attempts |
| 11 | Steps 7–9 on `/mla-tamagui/tamil/write/word-uyir-aa` | ஆடு: "LOOKS RIGHT · <score>" within two attempts |
| 12 | Steps 7–9 on `/mla-tamagui/tamil/write/word-uyir-i` | இலை: "LOOKS RIGHT · <score>" within two attempts |

### D. Recording

| # | Do | Expect |
| --- | --- | --- |
| 13 | `wf browser stop-recording`, then `wf browser stop-device --device tamil`; the `.mp4`, its input log and the screenshots are in `.agents/tmp/device-lab-g2/` | The files are there |
| 14 | `wf browser check-taps .agents/tmp/device-lab-g2/g2.mp4` | A dot at every logged tap |
| 15 | Publish `.agents/tmp/device-lab-g2` (videos, screenshots, input logs) as one page | Its link; put it in the result record and the last step's notes |

## Gates

1. **Traced letters.** Every stroke is accepted within two attempts, and all seven letters score at least
   80 % ON THE LINE. That is the floor `frontend/mla-tamagui/e2e/verify-tamil.mjs` holds exact walks to, so
   a human stroke that falls below it is a finding about the stroke model or the validator, not a pass.
2. **Freehand letter.** அ written large is marked "LOOKS RIGHT".
3. **Words.** அம்மா, ஆடு and இலை are each marked "LOOKS RIGHT" within two attempts.
4. **Input only.** The input log holds only touch strokes, taps and swipes; `check-taps` passes.

The gate fails on any miss. Fix the app, the mock or the driver, then run it again; never relax a gate.

## Recording the run

`--record` writes `docs/runbooks/.results/device-lab-tamil-writing.json`. Then tick G2 in
[`progress.md`](../plans/device-lab/progress.md) with the record's sha, every score and verdict, and the
artifacts link.
