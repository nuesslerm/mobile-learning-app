# Tamil marker: which vision model marks the writing (2026-10-05)

`POST /api/tamil/check` puts the printed letter or word beside the learner's ink and asks a vision model
whether the writing reads as it. Device-lab gate G2 found `minimax/MiniMax-M3`, the model it used, marking
a faithful copy of இலை anywhere from 25 to 85, so the choice was measured.

## Samples

Ink traced over each write pad's template on the lab export (`wf browser trace --source template --human`,
iPhone 15 Pro, Noto Sans Tamil), sent exactly as the app sends it, to the marker run from the checkout
(`backend/remote-chat-agent`, with the whole-word reference and trimmed panels of this change):

| Ink | Marked as | Expected |
| --- | --- | --- |
| அ | அ (`uyir-a`) | right |
| அம்மா | அம்மா (`word-uyir-a`) | right |
| ஆடு | ஆடு (`word-uyir-aa`) | right |
| இலை | இலை (`word-uyir-i`) | right |
| அ | இலை | wrong |
| ஆடு | அம்மா | wrong |

## Results

First pass, n = 1 per model:

| Model | Correct | p50 | Note |
| --- | --- | --- | --- |
| `minimax/MiniMax-M3` | 5 / 6 | 13.7 s | passed ஆடு as அம்மா (72) |
| `opencode-go/qwen3.8-max` | 5 / 6 | 109 s | one 150 s timeout; too slow for the app's wait |
| `opencode-go/gpt-6-luna` | 6 / 6 | 15.4 s | |
| `opencode-go/glm-5.3` | 1 / 6 | 6.2 s | 502 on every image |
| `opencode-go/kimi-k3` | 5 / 6 | 105 s | one 150 s timeout |

Finalists, n = 3:

| Model | Correct | Right ones passed | Wrong ones refused | p50 | p90 |
| --- | --- | --- | --- | --- | --- |
| `minimax/MiniMax-M3` | 15 / 18 | 11 / 12 (60–85) | 4 / 6 (passed ஆடு as அம்மா at 72 and 60) | 14.0 s | 37.4 s |
| `opencode-go/gpt-6-luna` | 18 / 18 | 12 / 12 (88–96) | 6 / 6 (18–55) | 9.4 s | 18.3 s |

## Decision

The Tamil marker uses `opencode-go/gpt-6-luna` (`MODEL_TAMIL_MARKER`, overridable with
`OPENCODE_TAMIL_MARKER_MODEL`). It got every sample right with a wide margin either side of the pass mark
(60), and answers faster. The other vision uses keep `MODEL_VISION`; nothing here measured them.

The same change fixed what the model was shown: the printed reference was sized from the word's length and
clipped இலை at both edges, and the ink sat small in the middle of its panel. Both are now trimmed to what is
drawn and fitted inside the same margin, and a word is called a word in the prompt.
