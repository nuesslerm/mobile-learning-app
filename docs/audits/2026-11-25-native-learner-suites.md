# LeetCode and Tamil native learner coverage

Each named case in `e2e/leetcode-native.lua` and `e2e/tamil-native.lua` uses the same assertions on the iOS simulator and Android emulator. Select one through `wf test run tamil-native --platform ios --device <UDID> --case tiles`. Use the update for the selected revision. The local development device is `tamil`, iPhone 17, iOS 27.0, UDID `418CEC10-C73F-4444-AD12-07AD9CF58008`.

| Previous focused Android case | Where its behavior is exercised |
| --- | --- |
| tamil-flash-cards | flash-english checks the English prompt, reveal, transliteration, audio and advancement. flash-tamil checks reverse recall and reopening. |
| tamil-writing, vowels 1–2 | trace follows two strokes through the native surface and checks the percentage. |
| tamil-writing, vowels 3–4 | The same guide and scoring path as trace. Specific letters remain in the browser suite. |
| tamil-writing, vowels 5–6 | The same guide and scoring path as trace. Specific letters remain in the browser suite. |
| tamil-writing, consonant | The consonant template remains in the browser suite. redraw adds a failed attempt, clear and recovery on native. |
| tamil-writing, write | The free-writing pad and self-check remain in browser coverage. The native self-check is a candidate for case 11. |
| tamil-writing, word-uyir-a | Browser word template and self-check. Candidate 11 covers one representative word on native. |
| tamil-writing, word-uyir-aa | Browser word template and self-check. |
| tamil-writing, word-uyir-i | Browser word template and self-check. |
| stop-probe | Android harness cancellation diagnostic, excluded from the learner counts. |

Library search, collection membership, navigation, Python and JavaScript execution, custom inputs, error recovery, draft persistence, symbol insertion and keyboard editing comprise the initial LeetCode set. Tamil adds task resume, tiles, typing, listening, fill-in feedback and missed-exercise review to handwriting and flash cards. Fixed-clock flash-card tests continue to cover review dates and transactional save behavior.

## Timing

Start with ten core journeys per section. The acceptance threshold is 600 seconds per section on a ready device in each of three uncached hosted runs on each platform. Record queue, build, boot and artifact upload separately. A failed case or an incomplete sample cannot establish a cutoff. The first independently passing local iOS cases on the baseline update took 12.088 seconds for Two Sum search and 23.798 seconds for Tamil word tiles, including per-case isolation and recording.
