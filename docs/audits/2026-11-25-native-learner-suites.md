# LeetCode and Tamil native learner coverage

Each named case in `e2e/leetcode-native.lua` and `e2e/tamil-native.lua` uses the same assertions on the iOS simulator and Android emulator. Select one through `wf test run tamil-native --platform ios --device <UDID> --case tiles`. Use the update for the selected revision. The local development device is `tamil`, iPhone 17, iOS 27.0, UDID `418CEC10-C73F-4444-AD12-07AD9CF58008`.

| Previous focused Android case | Where its behavior is exercised |
| --- | --- |
| tamil-flash-cards | flash-english checks the English prompt, reveal, transliteration, audio and advancement. flash-tamil checks reverse recall and reopening. |
| tamil-writing, vowels 1–2 | trace follows two strokes through the native surface and checks the percentage. |
| tamil-writing, vowels 3–4 | The same guide and scoring path as trace. Specific letters remain in the browser suite. |
| tamil-writing, vowels 5–6 | The same guide and scoring path as trace. Specific letters remain in the browser suite. |
| tamil-writing, consonant | The consonant template remains in the browser suite. redraw adds a failed attempt, clear and recovery on native. |
| tamil-writing, write | The free-writing pad and self-check remain in browser coverage. Native case 11 completes a representative word and its self-check. |
| tamil-writing, word-uyir-a | Browser word template and self-check. Native case 11 covers one representative word on native. |
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

### First hosted benchmark

The [three-sample run](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45538369231) exercised both hosts on `7b895b05`. All six samples stopped at Expo Go startup before the learner assertions passed. Their section times describe failed attempts. The Android introduction required a tap outside the wait condition. The iOS host showed the developer-menu tutorial on a fresh installation.

| Platform | Sample | Queue | Darwin build | Backend setup | Boot and app install | LeetCode | Tamil | Bundle upload | Job total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| ios | 1 | 2 s | 152 s | 41 s | 429 s | 320 s | 387 s | 2 s | 1544 s |
| ios | 2 | 3 s | 198 s | 65 s | 411 s | 372 s | 385 s | 3 s | 1684 s |
| ios | 3 | 2 s | 188 s | 54 s | 403 s | 343 s | 381 s | 3 s | 1621 s |
| android | 1 | 4 s | — | 39 s | 75 s | 42 s | 42 s | 3 s | 396 s |
| android | 2 | 3 s | — | 36 s | 76 s | 44 s | 44 s | 2 s | 402 s |
| android | 3 | 2 s | — | 40 s | 81 s | 56 s | 57 s | 3 s | 403 s |

Android used the shared Linux `wf-build` job, which took 215 seconds. Both test update publishes and job dependencies precede the device jobs.

The [Android run on 36c163c9](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45542540641) passed nine journeys in each section. LeetCode took 311.6 seconds with an extra slash inserted by tapping an expanded symbol group. Tamil took 306.6 seconds with the last relaunch landing on the Android home screen. The next run selects the symbol in the active group and launches Expo Go with an explicit package.

### Core benchmark on both hosted runners

In the [three-sample core run](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45544327590), Android passed all ten Tamil journeys in 169.8, 165.8 and 169.3 seconds. LeetCode took 213.4, 281.9 and 211.3 seconds. The first Android sample passed all twenty journeys. The other two exposed races opening the language wheel, editing immediately after loading a draft, inserting a symbol, and dismissing the Android keyboard.

The next benchmark proposes fifteen journeys in each section. Cases 11–15 exercise editor settings, completion, blocks, two independent drafts and a deterministic voice edit on LeetCode. On Tamil they exercise word writing, weak-letter review, skipped audio, lesson exercise search and double-tap rating persistence. The voice case replays audio and the voice transport only for its device-lab deep link. Its code runs against the same runner as the other editor journeys.

The iOS [fresh-host recording and accessibility tree](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45540167190) showed the system confirmation “Open in ‘Expo Go’?” before Expo's own developer menu. The hosted job now opens the exact update and records the startup diagnostic before starting the section timers. This diagnostic is outside the learner-case count.

### Expanded suite verification

On the local iPhone 17 running iOS 27.0, Tamil passed all fifteen journeys in 214.9 seconds. The exercise search now enters All Topics, searches for “your number”, opens the matching task and verifies its first exercise. That replacement passed independently in 17.2 seconds. Weak-letter review and reverse flash cards, including playback from the Tamil front, passed independently in 14.7 and 19.2 seconds.

On [the expanded Android run](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45546504135), sample 3 completed LeetCode in 358.2 seconds (5/15) and Tamil in 268.5 seconds (14/15). A [follow-up recording](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45547314155) identified the visible system IME dismiss button when the app toolbar was hidden. The driver now uses that control. The failed samples do not qualify the fifteen-case cutoff.
