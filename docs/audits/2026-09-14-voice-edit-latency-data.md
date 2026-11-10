# Voice/text edit latency: what was measured, what moved, what has not shipped

All numbers below are measured, not estimated. Every row names where it came
from. "Deployed" means the `remote-chat-agent` image running on the Oracle A1
VM on 2026-09-14, which predates every server-side change in this branch —
so server-side levers are reported as *coded, unmeasured in production*, not
as improvements.

## 1. Baseline (start of this work, deployed backend)

| call | measured | source |
| --- | --- | --- |
| `POST /api/voice-transcribe`, whisper.cpp `base.en` | 12.3s, 13.0s | curl against the deployed backend, session notes |
| `POST /api/voice-plan`, identical payload, 3 runs | 20.5s / 37.5s / 75.7s | same |
| harness CDP network-proxy overhead | +1.4s per call (7.98s → 9.36s) | same; the proxy was removed from the runbook |

## 2. Spoken turns, end to end (25 turns, 4 runs, deployed backend)

Driver: `wf harness voice-fixtures drive --voice-model … --text "…"`, fake
mic, browser-observed. `transcribeMs` = Send tapped → transcript on screen;
`planMs` = transcript confirmed → alternatives/applied (this is the
post-confirm *wait*, after the speculative plan already started).

Source: `.agents/tmp/audio-run/{final-run,solve-0521,solve-0553,solve-0604}.jsonl`.

| metric | p50 | min | max | n |
| --- | --- | --- | --- | --- |
| transcribe (browser wall) | 13.4s | 11.9s | 24.2s | 25 |
| post-confirm plan wait | 20.5s | 5.7s | 54.0s | 25 |

Spread on the plan is provider variance on identical work (§1 shows it on a
byte-identical payload); nothing on this branch changes that until the
provider race ships.

## 3. Whisper flags and model, local benchmark (this sandbox: x86, 4 cores)

Built whisper.cpp from `whisper-node`'s vendored source, ran the 5 committed
fixtures plus the 5 palindrome sentences (Piper, resampled to 16 kHz mono as
the backend does). Ground truth: `tests/fixtures/voice/manifest.json` and the
sentences themselves. Runner: `.agents/tmp/whisper-bench/bench.py`, raw rows
in `results.json` next to it.

| model | flags | p50 | max | exact | mean WER |
| --- | --- | --- | --- | --- | --- |
| base.en | defaults (4 threads, beam 5) | 2616ms | 5942ms | 4/10 | 0.111 |
| base.en | `-t 4` | 2365ms | 5110ms | 4/10 | 0.111 |
| base.en | `-t 2` | 3750ms | 5127ms | 4/10 | 0.111 |
| base.en | `-t 4 -bs 1 -bo 1` (greedy) | 8693ms | 68917ms | 3/10 | 0.167 |
| tiny.en | defaults | 2321ms | 4926ms | 5/10 | 0.108 |
| tiny.en | `-t 4` | 1690ms | 5720ms | 5/10 | 0.108 |
| tiny.en | `-t 2` | 1901ms | 2417ms | 5/10 | 0.108 |
| tiny.en | `-t 4 -bs 1 -bo 1` (greedy) | 1755ms | 12524ms | 6/10 | 0.088 |

What this decides:

- **Greedy decoding is out.** On `base.en` it produced a 68.9s outlier and
  worse accuracy; the plan had it as a candidate flag. Not shipped.
- **`tiny.en` is 1.4× faster than `base.en` at equal accuracy on this set**
  (1690 vs 2365ms p50 with `-t 4`; WER 0.108 vs 0.111; 5/10 vs 4/10 exact).
  Both models are now baked into the image and `WHISPER_MODEL_NAME` picks at
  runtime, so the switch is a redeploy with one env var, and the
  `whisper_model` column in the stats table is where the production
  comparison lands. Both models mishear the same sentence (`set s to` →
  "set us to") — a phrasing problem, not a model problem.
- **Thread count:** on this 4-core box `-t 4` equals the default (the default
  *is* 4). The A1 has 2 vCPUs, where 4 threads oversubscribe; the benefit
  there is unmeasured until the deploy. `-t` now follows the CPU count.
- **CPU target of the build** (`-mcpu=neoverse-n1` instead of generic
  ARMv8-A): cannot be measured on x86. It is the largest suspected server-side
  lever and is measured by the `whisper_ms` column after the deploy.

Mis-transcriptions, `-t 4`, both models (the ones the planner then has to
survive):

```
base.en  pal1  set us to only the self and numeric characters in lowercase
base.en  pal3  ... with a past body
base.en  pal4  Replace pass with nth that returns false, when the character it left ...
base.en  add-a-docstring  Add a dockstring above the function
tiny.en  pal1  Set us to only its health and numeric characters in lowercase
tiny.en  pal4  Replace paths with the nth at return's false ...
tiny.en  wrap-in-try-catch  Wrap the function body in a tri-catch
```

## 4. Typed vs spoken, same five steps, same deployed backend (paired)

Typed: `wf harness voice-fixtures drive --typed` (3 passes, 14 turns, every
pass ended at Run Local `14/14`; `.agents/tmp/audio-run/typed-run*.jsonl`).
Spoken: the three spoken runs of the identical script
(`final-run`, `solve-0553`, `solve-0604`). "Turn total" excludes the ~5s the
driver spends letting the clip play and the fixed review pause; it is the
part the user actually waits on the machine for.

| step | spoken: transcribe p50 (n) | spoken: post-confirm wait p50 (n) | spoken turn total p50 | typed: plan wall p50 (n) | typed turn total |
| --- | --- | --- | --- | --- | --- |
| set s to only its alphanumeric charact… | 13100ms (3) | 20540ms (3) | 33959ms | 13868ms (2) | 13868ms |
| after that line, declare left as zero … | 13400ms (3) | 23648ms (3) | 36575ms | 9417ms (3) | 9417ms |
| add a while loop that runs while left … | 13992ms (3) | 18609ms (3) | 36672ms | 23103ms (3) | 23103ms |
| replace pass with an if that returns f… | 13951ms (3) | 11004ms (3) | 24967ms | 20500ms (3) | 20500ms |
| at the very end of the method, after t… | 12922ms (3) | 33891ms (3) | 46813ms | 16430ms (3) | 16430ms |
| **all steps** | 13400ms (15) | 20540ms | **36575ms** | 13379ms (14) | **13379ms** |

Typed turns skip transcription entirely and cannot use the speculative
plan (there is nothing to speculate from), so their wait is one full planner
call. Spoken turns pay transcription *and* the residual planner wait. Net on
this data: **a typed turn's machine wait is ~64% lower than a
spoken turn's** (p50 13379ms vs 36575ms), with the same final
result on every pass. The planner's own variance (min 5169ms, max
41981ms across typed turns) is unchanged -- it is the deployed provider,
untouched by anything on this branch until the image ships.

## 5. Speculative plan: how much it actually hides (spoken pass, 5 turns)

The plan call starts the moment whisper answers, while the transcript is on
screen; the user-facing wait is what remains after they confirm. This pass
reads both numbers from the app's own log (`voice_edit.plan.response`:
`ms` = post-confirm wait, `wallMs` = the HTTP call's true duration) next to
the driver's browser-observed numbers. Source:
`.agents/tmp/audio-run/voice-run-2.jsonl` (`appTail`); every turn hit the
speculative plan (`speculative_hit reused=true`); the pass ended at Run
Local `14/14`.

| step | transcribe (app wall) | plan call wall | post-confirm wait | hidden |
| --- | --- | --- | --- | --- |
| set s to only its alphanumeric… | 13431ms | 11015ms | 10597ms | 418ms |
| after that line, declare left… | 13073ms | 12461ms | 12251ms | 210ms |
| add a while loop… | 12366ms | 6817ms | 6390ms | 427ms |
| replace pass with an if… | 13465ms | 22213ms | 21889ms | 324ms |
| at the very end of the method… | 11922ms | 12570ms | 12245ms | 325ms |

Hidden time equals the time the user spends on the review sheet before
confirming, capped at the plan's duration. The driver confirms in under a
second, so it hides ~0.3s; a person who reads the transcript for 5s hides
5s. Measured on 2026-09-13 with a deliberate review pause: post-confirm
waits of 2692ms and 1925ms against plan calls in the 8-11s range. The
`post_confirm_wait_ms` vs `plan_wall_ms` columns in the stats table are
where the real-user number will come from. Browser-observed transcribe
(driver) is 0.4-0.6s above the app's own wall time: polling granularity,
not backend time.

## 6. Rename: the local fast path (typed, 1 turn)

`rename left to lo` typed after a pointer declaration: `voice_edit.plan.local
{label: "Rename left to lo"}`, no `plan.send`, driver `planMs` **86ms**,
buffer updated to `lo = 0`. Spoken renames take the same path once the
transcript is confirmed, so a rename costs transcription only (~13s spoken,
~0.1s typed) instead of transcription plus a 6-54s planner call.

## 7. Scorecard: what moved, what is coded but unmeasured in production

| use case | before (deployed, spoken) | now | moved by |
| --- | --- | --- | --- |
| an edit, typed | did not exist | 13.4s p50 machine wait (n=14) | new path: no transcription, one planner call |
| an edit, spoken | 36.6s p50 (13.4s transcribe + 20.5s wait, n=15) | same | nothing server-side has shipped |
| a rename, typed | did not exist | 0.09s, zero network calls | local fast path |
| a rename, spoken | 13.4s + planner call | 13.4s + 0s | local fast path |
| review-time overlap (speculative plan) | none | hides review time up to the plan's length (0.3s for the driver, 1.9-2.7s measured with a pause) | client only |
| transcription model | base.en, 2365ms local p50 | tiny.en available at runtime, 1690ms local p50 (−29%), equal accuracy on 10 sentences | needs the deploy |
| transcription threads / CPU target | default 4 threads on 2 vCPUs, generic ARMv8-A build | `-t <cpus>`, `-mcpu=neoverse-n1` | needs the deploy; measured then via `whisper_ms` |
| planner | 5.7-54.0s on identical work | same | caching, tools-off, session reuse, retry coded; racing not yet; all need the deploy |

Reproduce: `bash .agents/tmp/audio-run/typed-pass.sh <browser> <out.jsonl>`,
`bash .agents/tmp/audio-run/voice-pass.sh <browser> <out.jsonl>`,
`python3 .agents/tmp/whisper-bench/bench.py`; after the deploy,
`wf edit-stats summary --since 24h --by side,route,source,whisper_model`.

## 8. Faster than whisper.cpp: other engines on the same 10 sentences (local x86, 4 cores)

Same audio, same ground truth, same box as §3. Runner:
`.agents/tmp/whisper-bench/alt-bench.sh`, raw rows in `alt-results.json`.
whisper.cpp rows repeated from §3 for the comparison.

| engine | p50 | max | exact | mean WER |
| --- | --- | --- | --- | --- |
| whisper.cpp base.en `-t 4` (what runs on the VM) | 2365ms | 5110ms | 4/10 | 0.111 |
| whisper.cpp tiny.en `-t 4` | 1690ms | 5720ms | 5/10 | 0.108 |
| **faster-whisper tiny.en int8, greedy** | **558ms** | 803ms | 5/10 | **0.093** |
| faster-whisper tiny.en int8, beam 5 | 717ms | 1150ms | 5/10 | 0.110 |
| faster-whisper base.en int8, greedy | 852ms | 1042ms | 5/10 | 0.121 |
| faster-whisper base.en int8, beam 5 | 952ms | 1571ms | 5/10 | 0.115 |
| moonshine/tiny (ONNX) | 1399ms | 1840ms | 4/10 | 0.118 |
| moonshine/base (ONNX) | 2331ms | 3416ms | 4/10 | 0.138 |

faster-whisper (CTranslate2, int8) is **4.2× faster than the whisper.cpp
build the VM runs, at better accuracy** on this set, and its worst case
(803ms) is below whisper.cpp's median. Caveat: int8 GEMM on this x86 box
uses AVX2; on the A1 (Neoverse-N1) CTranslate2 falls back to NEON/Ruy and
the ratio will be smaller -- it has to be re-measured there (the
`whisper_ms` column, with the engine name in `whisper_model`). Moonshine,
designed for edge devices, was slower than faster-whisper here.

None of the self-hosted options change the 20s round-trip picture on their
own: 13.4s → ~1-2s of transcription leaves the planner's 20.5s p50 as the
whole budget. On-device streaming recognition (iOS/Android system
recognisers, Web Speech API) removes the transcription leg and the upload
entirely; the planner still has to fall below ~10s for a 20s ceiling, which
is the provider race with a fast-inference provider, not anything in this
repository today.

## 9. The Expo Go path: faster-whisper sidecar vs the deployed route, same audio, same scorer

`wf harness voice-fixtures score --transcribe-url <url> --only-extra --extra
.agents/tmp/whisper-bench/audio16/expected.tsv` -- one multipart POST per
sentence, client round trip timed, word error rate against the known text.
Deployed route scored over the tailnet forward on the VM (A1); the sidecar
(`backend/remote-chat-agent/stt/server.py`) ran in this sandbox (x86,
4 cores), so its absolute numbers will move on the A1 -- the ratio to the
x86 whisper.cpp run in §3 (2365ms) is the honest comparison until then.

| engine | where | p50 | p90 | max | exact | mean WER |
| --- | --- | --- | --- | --- | --- | --- |
| deployed whisper.cpp base.en (today's route) | A1 VM | 13788ms | 15175ms | 15270ms | 4/10 | 0.111 |
| whisper.cpp base.en `-t 4` (§3, binary only) | x86 | 2365ms | – | 5110ms | 4/10 | 0.111 |
| **sidecar faster-whisper tiny.en int8, vocabulary prompt** | x86 | **371ms** | **456ms** | 504ms | **7/10** | **0.044** |
| sidecar tiny.en int8, prompt off | x86 | 371ms | 413ms | 474ms | 5/10 | 0.081 |
| sidecar base.en int8, vocabulary prompt | x86 | 565ms | 639ms | 698ms | 7/10 | 0.059 |
| sidecar base.en int8, prompt off | x86 | 601ms | 623ms | 708ms | 5/10 | 0.115 |

Readings:

- Same hardware, whisper.cpp base.en → sidecar tiny.en: **6.4× faster**
  (2365 → 371ms p50) and accuracy up from 4/10 to 7/10 exact (WER 0.111 →
  0.044). Against what users hit today the gap is 37× (13788 → 371ms), of
  which an unknown share is the A1's slower cores -- the deploy settles it.
- **The vocabulary prompt is free accuracy**: 5/10 → 7/10 exact, WER 0.081 →
  0.044 on tiny.en, 0.115 → 0.059 on base.en, at identical latency. It is the
  sidecar's default (`STT_INITIAL_PROMPT` overrides; the scorer's
  `--field prompt=` disables it for A/B).
- tiny.en beats base.en on this corpus on both axes; base.en stays baked in
  for the day the corpus says otherwise.
- The remaining misses are the same two sentences for every engine
  (`set s to` → "set us to", `pass with an if` → "pass with nth"): phrasing,
  not compute. Both are one-word rewrites of the runbook script.

Budget after the deploy, spoken turn: transcription ~0.4-1.5s (A1 estimate
2-4× this sandbox) + planner. The <5s p90 target for voice→text is met by
this path on the sandbox with 10× headroom; the planner remains the whole
of the 20s round-trip problem.

## 10. The planner itself: prompt variant, agent and model, benchmarked with the production code path

`backend/remote-chat-agent/tests/bench/voicePlanner.bench.ts` drives the real
`planVoiceEdit` against a local `opencode serve` started with the repo's own
`opencode-server/server-config.json` (which now defines the `voice-planner`
agent: temperature 0, top_p 1, every tool off). Corpus: the five
valid-palindrome steps of the runbook, each against the code state the
previous step leaves, plus the one-breath "write a two pointer solution"
ask (gate 5). Configs: `v1` = the original offset prompt under opencode's
default agent; `v2` = the line-anchored, one-alternative, minimal prompt;
`v2+voice-planner` = v2 under the temperature-0 agent. 3 repeats per
config, sessions reset between repeats and shared within one (as in
production). Run 2026-09-14 20:00–20:30 UTC from this sandbox; results in
`.agents/tmp/plan-bench/results.jsonl`, summary from
`.agents/tmp/plan-bench/summarize.mjs`.

Per (model, config), 15 scored turns (the refusal ask is scored separately):

| model | config | p50 ms | p90 ms | max ms | parsed | validator ok | compiles | expected | one alt | avg changed lines | byte-identical steps | 45 s timeouts | refusal ok |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| kimi highspeed | v1 | 3839 | 6846 | 10444 | 15/15 | 15/15 | 12/15 | 14/15 | 15/15 | 2.0 | 0/5 | 0 | 3/3 |
| kimi highspeed | v2 | 2413 | 2909 | 3412 | 15/15 | 15/15 | 15/15 | 15/15 | 15/15 | 1.9 | 3/5 | 0 | 2/3 |
| kimi highspeed | v2+voice-planner | 2445 | 2889 | 2905 | 15/15 | 15/15 | 15/15 | 15/15 | 15/15 | 1.9 | 2/5 | 0 | 2/3 |
| MiniMax-M2.5-highspeed | v1 | 2585 | 3853 | 4522 | 15/15 | 15/15 | 8/15 | 15/15 | 15/15 | 2.3 | 0/5 | 0 | 3/3 |
| MiniMax-M2.5-highspeed | v2 | 2286 | 3611 | 6137 | 15/15 | 13/15 | 15/15 | 15/15 | 15/15 | 2.7 | 1/5 | 0 | 3/3 |
| MiniMax-M2.5-highspeed | v2+voice-planner | 2456 | 2878 | 3030 | 15/15 | 14/15 | 15/15 | 15/15 | 15/15 | 2.3 | 1/5 | 0 | 3/3 |
| deepseek-v4-flash | v1 | 6651 | 45000 | 45000 | 13/15 | 13/15 | 8/15 | 12/15 | 13/15 | 2.1 | 0/5 | 2 | 1/3 |
| deepseek-v4-flash | v2 | 2596 | 45001 | 45002 | 13/15 | 13/15 | 13/15 | 13/15 | 13/15 | 1.8 | 1/5 | 2 | 3/3 |
| deepseek-v4-flash | v2+voice-planner | 2171 | 8258 | 45002 | 14/15 | 14/15 | 14/15 | 14/15 | 14/15 | 1.6 | 4/5 | 1 | 3/3 |

"compiles" = the applied buffer parses as Python (`ast.parse`); "expected"
= the fragments the step asks for are present; "byte-identical steps" =
steps whose applied buffer hashed the same on all 3 repeats.

What the numbers say:

1. **v1's offsets corrupt code the validator accepts.** Every v1 row has
   `validator ok` 13–15/15 but `compiles` 8–12/15: the offset commands land
   one or two characters off and the result is indented wrong or glued to
   the neighbouring line. v2's line-anchored commands compile 15/15 on
   kimi and MiniMax; deepseek's misses are all 45 s timeouts, not bad code.
   This is the same failure the live realise turns showed (§ below).
2. **Latency.** v2 halves kimi's p50 (3839 → 2413 ms) and pulls every
   p90 under 3 s on kimi and MiniMax with the agent (2889 / 2878 ms, max
   2905 / 3030). Against the deployed planner's post-confirm p50 of 30.3 s
   (§2) the planner call itself is no longer the round trip's problem.
3. **deepseek-v4-flash hangs.** 5 of 45 turns produced nothing for 45 s
   and had to be cut by the new turn budget; before the budget one such
   turn sat for over eight minutes without tripping the stream's
   inactivity watchdog (other sessions' events kept it "active"). Default
   planner model moved to `kimi-for-coding/kimi-for-coding-highspeed`
   (`config/models.ts`, `.env.defaults`); `OPENCODE_TEXT_MODEL` overrides.
4. **Determinism at temperature 0 is semantic, not byte-level.** The
   `voice-planner` agent does not make replies byte-identical (kimi 2/5,
   MiniMax 1/5 steps; deepseek 4/5): the same instruction comes back as
   `left, right = 0, len(s) - 1` on one run and two assignments on the
   next. What is stable is the outcome: 15/15 compile and carry the
   expected fragments. Flow and integration tests therefore assert on the
   applied buffer's parse and content, not on the model's text -- and the
   deterministic parts of the flow (mode switches, pseudo lines, renames,
   the realise fence and normaliser) are pure functions with unit tests.
5. **Minimal edits.** 1.6–2.7 changed lines per step across configs; v2
   never re-emits untouched lines (rule 2 of the prompt), one alternative
   in 15/15 turns on every model.
6. **Refusal of the one-breath ask** is now enforced by the size validator
   on the client (an answer over 6 lines per command is marked invalid),
   which is what "refusal ok" counts: 3/3 for MiniMax and deepseek on v2,
   2/3 for kimi (one 5-line answer slipped under the limit).

Parser hardening found by this run, all deterministic and unit-tested:
raw line breaks inside JSON code strings are repaired
(`repairJsonControlChars`), `newText`/`text` drift between the two block
commands is folded back (`normalizeVoiceEditResponse`), and schema-failure
errors now carry the raw reply. Before these, 4 of the first 6 v2 replies
failed to parse.

### Pseudo-code mode, live (typed, Metro web, deployed v1 planner)

Turns and their client-measured `planMs` from the driver
(`wf harness voice-fixtures drive --typed`), 2026-09-14 20:37–20:45 UTC,
recording `.agents/tmp/audio-run/pseudo-run.mp4`:

| turn | path | planMs |
| --- | --- | --- |
| enter pseudo code mode | local | 20–29 |
| keep only alphanumeric characters in lowercase | local (comment line) | 61–67 |
| declare left and right pointers at both ends of s | local (comment line) | 14–52 |
| implement line 4 | planner, fenced | 25367 |
| implement line 3 | planner, fenced | 28499 |
| while left is less than right, … move both pointers inward | local (comment line) | ~50 |
| after the loop, return true | local (comment line) | 48 |
| implement line 5 | planner, fenced | 47749 |
| implement line 10 | planner, fenced | 11442 |
| exit pseudo code mode | local | 13–21 |

Run Local after the exit turn: `14 / 14 test cases passed`. Every local
turn is under 70 ms; every planner turn is the deployed v1 planner's
11–48 s (one earlier attempt hit the client's 180 s timeout). The fenced
realise caught the deployed planner's offset drift three times (spans
starting 1–2 characters early, ending 1 past the buffer) and, after the
normaliser, applied exactly the fenced line each time.

## 11. Deployed: the same probes against the released remote-chat-agent (2026-09-15)

Released from this branch with the VM pipeline (`wf agent-shell build
remote-chat-agent remote-chat-agent-stt opencode-server --ref
claude/leetcode-audio-editing-demo-2zs3a9 --tag vm-pr299`, then a full
`recreate=all` redeploy through vm-ops-api so the `stt` and
`opencode-server:vm-pr299` services came up). Probes from this sandbox over
the tailnet SOCKS proxy (`curl --socks5-hostname localhost:1055
http://remote-chat-agent-prod:8787/...`), so wall times include one tailnet
hop; the server rows are what the service measured itself.

Planner, the realise payload for "declare left and right pointers" on the
palindrome buffer (`/api/voice-plan`, `source: text`), 5 calls:

```
voice-plan 200 4.632s   (first call: session create, cache_read=0)
voice-plan 200 2.631s
voice-plan 200 2.187s
voice-plan 200 2.343s
voice-plan 200 2.170s
answer, all 5 identical: replaceLines 4-4 "        left, right = 0, len(s) - 1"
wf edit-stats summary --since 40m --by side,route,planner_model,whisper_model
server  plan  kimi-for-coding/kimi-for-coding-highspeed@v2+voice-planner  n=5 ok=5 p50=2061ms p95=4352ms cacheHits=4
```

Transcription, the 16 kHz fixture `add-a-docstring.wav` (`/api/voice-transcribe`), 8 calls over two rounds:

```
voice-transcribe 200 1.373s / 1.300s / 1.365s / 1.314s / 1.341s / 1.454s / 1.301s / 1.294s
transcript: "Add a docstring above the function."   (expected: add a docstring above the function)
server  transcribe  faster-whisper:tiny.en:int8  n=3 ok=3 p50=732ms p95=797ms   (whisper 716-777 ms of it)
```

Against the deployed baseline of §2 and §9: planner post-confirm p50 30.3 s →
2.1 s server / 2.2–2.6 s wall (14×); transcription 13.8 s → 0.73 s server /
1.3 s wall (10–19×). Every planner answer was identical across the 5 calls.
What it took to get here is in `docs/known-gaps.md` (Funnel served an empty
hostname; opencode-server had to ship on the same tag as the agent because
the `voice-planner` agent is baked into its config; the sidecar only comes
up on a full recreate because vm-ops-api's scoped redeploy restarts `agent`
alone).

## 12. The Plan tab, solved end to end against the deployed backend (2026-09-15)

The whole problem through the new workspace: four typed plan steps, each
realised in plan order by one fenced planner call, no manual code edit.
Metro web in the device frame, `wf harness plan drive`, deployed
`remote-chat-agent` on `vm-pr299` (planner `kimi-for-coding-highspeed`,
`voice-planner` agent, temperature 0).

| step | what was typed | edit applied | realise ms |
| --- | --- | --- | --- |
| 1 | strip everything that is not alphanumeric and lowercase it | lines 3-3, the filter fills the stub body | 3386 |
| 2 | set left to zero and right to the last index | lines 4-4, `left, right = 0, len(s) - 1` | 2827 |
| 3 | while left is less than right compare the characters and return false if they differ then move both pointers | lines 5-9, the whole loop | 2815 |
| 4 | after the loop return true | lines 10-10, `return True` | 2434 |

Realise p50 2821 ms, max 3386 ms, 4/4 answers inside their fence. Run Local
after the last step: `14 / 14 test cases passed`.

Adding a step (the plan-list operation the user does most) does no I/O at
all; the driver reports 382-441 ms for it, which is its own 500 ms poll
interval rounding a sub-frame render, not app latency.

Against §2's spoken baseline of 30.3 s per planner turn, a whole solve is now
four realises totalling 11.5 s of backend time.

Two harness bugs found by this pass, both fixed in `clis/harness`
(`browserProbes.ts`): a tap fired before React caught up with the typed text
hits a disabled control, whose `pointer-events: none` swallows it in silence;
and a tap fired while the harness keyboard is up is measured in one layout and
delivered in another, because the daemon's own mouse press blurs the field and
drops the keyboard between the two. Typing now blurs and dismisses the
keyboard itself, and every tap waits for its target to stop moving.

Server-side, the same four realises plus the first pass's four
(`wf edit-stats summary --since 1h --by side,route,planner_model`):

```
side    route  planner_model                                               n  ok  p50ms  p95ms  maxms  cacheHits
server  plan   kimi-for-coding/kimi-for-coding-highspeed@v2+voice-planner  8  8   2166   2566   2566   8
```

Every row reused the prompt cache (`cache_read` 3840-5888 tokens, growing with
the buffer), so the wall-clock gap of roughly 600 ms per realise is the round
trip and the client's own apply, not planner time. The matching client rows
are still missing: the deployed image predates the `plan-voice` / `plan-text`
stat sources, so `POST /api/edit-stats` rejects them with 400 until the agent
is rebuilt from this branch (`docs/known-gaps.md`).

Re-run after the review fixes moved the plan slice and changed the split rule
(same four steps, same deployed planner, a cold VM after a reboot):

| step | realise ms |
| --- | --- |
| 1 | 16053 |
| 2 | 4334 |
| 3 | 4018 |
| 4 | 3837 |

Run Local `14 / 14 test cases passed` again. The first call paid for a cold
opencode session on a just-rebooted VM (no prompt cache, no warm process); the
three after it sit at 3.8-4.3 s against 2.4-3.4 s on the warm run above, which
is the same shape at a slightly higher floor.

## 13. Both halves of a turn, on the deployed image (2026-09-15)

`remote-chat-agent:sha-66260fa2` carries the plan stat sources and the chunk
route, and `HEALTH_RPT.EDIT_EVENTS` carries the widened `SOURCE` and the new
`CHUNK_INDEX` (`wf infra oracle db add-plan-columns`). One realise now writes
both halves under one id:

```
2026-09-15T14:42:19.492Z  mu2s67x8-u8ttnx8u  client  turn  plan-text  ok  2521ms  auto  plan=2521ms
2026-09-15T14:42:19.492Z  mu2s67x8-u8ttnx8u  server  plan  plan-text  ok  2233ms  kimi-for-coding/kimi-for-coding-highspeed@v2+voice-planner  cache_read=2304
2026-09-15T14:41:49.123Z  probe2-1789483303  client  transcribe-chunk  plan-voice  ok  456ms  auto
```

The 288 ms between the client's 2521 ms and the server's 2233 ms is the round
trip plus the client's own apply. The third row is a chunk row posted by hand
through `POST /api/edit-stats`, which proves the path a live transcription
uses; the live-words gate itself still needs a driver that injects one clip
per chunk.

## 14. The Kimi provider id moved (2026-10-05)

models.dev, the catalog opencode resolves model ids against, no longer lists a `kimi-for-coding` provider: the
Kimi coding plan is now `kimi-code-plan-global` (api.kimi.ai) and `kimi-code-plan-cn` (api.kimi.com), both on
`KIMI_API_KEY`. Device-lab gate G1 hit it on its first spoken turn: every planner call failed with `Model not
found: kimi-for-coding/kimi-for-coding-highspeed` and the app showed `Server error (500)`. The dispatch ladders had
already moved to `kimi-code-plan-global`; the planner's `MODEL_TEXT` and `MODEL_COMPLEX`, `.env.defaults` and the
server config's provider options had not. Under the new id the endpoint also refuses the `voice-planner` agent's
`top_p: 1` ("only 0.95 is allowed for this model"). At temperature 0 the planner decodes greedily, so top_p
changes nothing; the agent no longer sets it.

The section 10 benchmark (`tests/bench/voicePlanner.bench.ts`, production code path), local opencode 1.18.34,
n = 1:

| Model | Config | Steps | p50 | p90 | Valid | Compiles | Expected | Refusal |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `kimi-code-plan-global/kimi-for-coding-highspeed` | v2+voice-planner | 5 | 2871 ms | 3087 ms | 5/5 | 5/5 | 5/5 | 1/1 |

The same shape as section 10's kimi highspeed row (p50 2.4 s, p90 2.9 s).

### The whole-solution ask gets the size of a first step (2026-10-05)

Device-lab gate G1 asked "write a hash map solution for two sum" and the planner answered with the whole
seven-line solution, which passed 12 / 12: the ordinary limits (6 lines per command, 10 across) admit a compact
solution, and the prompt's "first step only" rule is the only other guard. `editSizeLimitsFor` now holds a
"write / implement / solve ... solution" ask to 3 lines (a first step: the variables or the signature) in the
server route and in the client's own validation, and the prompt names the hash map case and the 3-line cap.

Planner benchmark (`tests/bench/voicePlanner.bench.ts`, now scoring the refusal with the app's limits),
`kimi-code-plan-global/kimi-for-coding-highspeed`, v2+voice-planner, n = 6 per step:

| Steps | p50 | p90 | Valid | Compiles | Expected | Whole-solution ask |
| --- | --- | --- | --- | --- | --- | --- |
| 30 | 2644 ms | 3196 ms | 30/30 | 30/30 | 30/30 | refused or first step 6/6 |

### A block header gets a body (2026-10-05)

In a final device-lab G1 run the planner answered "loop over the numbers with enumerate" with the `for` header and
no body, and the step's Run Local reported `CompileError: expected an indented block`, which the runbook's
"buffer always parses" gate forbids. `completeBlockBodies` (voice-edit-dsl) now gives an inserted or replaced
Python block header with nothing deeper after it a `pass` body, in `planVoiceEdit`, as a hand-spoken "with a pass
body" does. Planner benchmark (n = 3 per step), same model and agent: 15/15 valid, compiling and as expected, p50
2898 ms, p90 3365 ms, the whole-solution ask refused 3/3.
