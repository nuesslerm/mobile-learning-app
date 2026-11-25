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

The existing iOS light and dark screenshot comparisons continue on their reference device. They measure visual regressions separately from the timed learner journeys.

| Existing iOS visual checks | Corresponding learner behavior |
| --- | --- |
| Tamil translation prompt and correct answer | tiles checks the submitted answer. typed exercises the keyboard. |
| Tamil listening prompt | listen starts the recording at both speeds and submits an answer. |
| Tamil fill prompt and incorrect answer | fill checks the expected missing word. |
| Tamil handwriting guide and accepted trace | trace checks the score. redraw clears a failed attempt and recovers. |
| Two Sum description and editor | search opens the matching description. one-editor checks the navigation stack. |
| Two Sum keyboard and result console | python and javascript edit and execute a solution. keyboard continues at the intended position after a run. |

Library search, collection membership, navigation, Python and JavaScript execution, custom inputs, error recovery, draft persistence, symbol insertion and keyboard editing comprise the initial LeetCode set. Tamil adds task resume, tiles, typing, listening, fill-in feedback and missed-exercise review to handwriting and flash cards. Fixed-clock flash-card tests continue to cover review dates and transactional save behavior.

## Timing

Start with ten core journeys per section. The acceptance threshold is 600 seconds per section on a ready device in each of three uncached hosted runs on each platform. Record queue, build, boot and artifact upload separately. A failed case or an incomplete sample cannot establish a cutoff.

On the booted local `tamil` simulator, the ten LeetCode cases passed in 142.3 seconds and the ten Tamil cases passed in 135.2 seconds. Both runs used the iOS test update published by CI for revision `7b895b05ddeb`. The elapsed times include the storage reset, assertions and recording.

| LeetCode journey | Seconds | Tamil journey | Seconds |
| --- | ---: | --- | ---: |
| search | 13.7 | resume | 9.7 |
| neetcode | 15.2 | tiles | 8.6 |
| one-editor | 8.1 | typed | 21.8 |
| python | 11.2 | listen | 9.6 |
| javascript | 13.7 | fill | 9.1 |
| custom-input | 14.7 | review | 14.7 |
| repair-error | 14.7 | trace | 17.3 |
| draft | 21.8 | redraw | 16.2 |
| symbol | 13.7 | flash-english | 10.2 |
| keyboard | 15.2 | flash-tamil | 17.7 |
