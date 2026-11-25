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

### Proposed fifteen on revision 75b7dace

On the named local simulator the full LeetCode set passed in 233.8 seconds and Tamil in 215.5 seconds. All 30 journeys have a read-back recording. The [phone update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/a6d9148f-62a7-46b9-a374-c0bf0594ccd4) separately passed the bare-link launch and answer-reveal check.

All fifteen Tamil journeys passed in 272.7, 274.0 and 276.0 seconds on three uncached hosted Android emulators. The [same benchmark](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45549199373) recorded LeetCode at 598.8, 491.2 and 493.8 seconds, with 5, 6 and 6 passing. The Android diagnostic tree included the IME dismiss button in the navigation window, which the active application selector could not inspect. The driver now reads that window's bounds and taps its button. One hosted iOS preflight stalled on a system accessibility read during the Expo Go handoff.

The [Android window-driver run](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45552306039) passed 13/15 LeetCode journeys in 411.7 seconds and all 15 Tamil journeys in 288.0 seconds. Its recordings exposed competition between the custom case's autofocus and the target field, and a Tab press before the completion opened. The custom-input case now runs Climbing Stairs with its single numeric parameter. The completion case waits for the Android suggestion and checks the accepted document before executing. Those cases passed on the named iPhone in 13.7 and 14.7 seconds respectively. A persistent, device-specific companion served three independent local cases using the hosted driver path.

Hosted iOS sample 3 on [75b7dace](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45549199373) passed four LeetCode journeys in 771.4 seconds and two Tamil journeys in 1,633.2 seconds. The simulator recording showed Expo Go’s floating tools button over the editor settings. Keyboard hit tests on unoccupied screen points returned an error. After XCTest typed காதல், the library was visible in subsequent recordings while accessibility returned only a zero-sized application. This matches the disabled application accessibility behavior reported for [fresh simulators](https://github.com/getsentry/XcodeBuildMCP/issues/290). The hosted fixture enables application accessibility before launch, preserves those flags across XCTest input, and hides Expo Go’s developer overlay. Individual keyboard probes now continue to the next screen point after an empty hit.

Both local sections passed with the prepared simulator identity, LeetCode in 201.4 seconds and Tamil in 188.3 seconds. Hosted sample 3 spent 107.4 seconds on the first Unicode input, including a cold Xcode helper build. The workflow now builds that helper before the section timer. With the prebuilt helper on tamil, typed translation passed in 16.8 seconds, followed by JavaScript in 11.7 seconds, Python in 10.7 seconds and listening in 9.1 seconds.

### Android measurements on cc903188

Two Android runners passed all thirty journeys. The third passed all fifteen Tamil journeys and thirteen LeetCode journeys. Its recording captured the Android “Expo Go isn't responding” dialog during a cold reload, and a scroll that went past the Blocks setting. The fixtures now reset the learner’s scoped files, databases and preferences between cases while retaining the warmed EAS update. The list scrolls in smaller increments while looking for settings. [CI bundles](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45554502158) · [Expo Go SDK 58 storage layout](https://github.com/expo/expo/blob/sdk-58/apps/expo-go/android/expoview/src/main/java/host/exp/exponent/utils/ScopedContext.kt)

| Sample | Queue | Backend | Boot/install | LeetCode | Tamil | Upload | Device job |
| --- | ---: | ---: | ---: | --- | --- | ---: | ---: |
| 1 | 3 s | 35 s | 81 s | 15/15 in 372.3 s | 15/15 in 298.8 s | 3 s | 956 s |
| 2 | 3 s | 31 s | 69 s | 13/15 in 440.7 s | 15/15 in 262.0 s | 3 s | 985 s |
| 3 | 2 s | 32 s | 78 s | 15/15 in 356.7 s | 15/15 in 283.7 s | 4 s | 900 s |
Shared Linux wf build job: 136 s.

### Warm update and platform controls

[Qualification on ac0cc9d2](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45557351214) used the same update as the named iPhone. Locally all 15 LeetCode journeys passed in 199.9 s and all 15 Tamil journeys in 182.6 s, with 30 recordings and the original learner data restored. The three Android samples passed 15, 13 and 15 LeetCode journeys in 240.0, 352.3 and 253.5 s and all 15 Tamil journeys in 188.9, 213.2 and 191.9 s. The second emulator's recordings show Expo's floating Tools menu covering the editor and its system task switcher opening over the partially clipped Blocks setting. Hosted iOS startup recordings show the first-open Expo consent alert in SpringBoard with an empty accessibility tree.

### Hosted startup on 20373c22

[Three fresh samples](https://github.com/nuesslerm/mobile-learning-app/actions/runs/45560162269) produced these timings. The local iPhone passed all 30 journeys on the corresponding update, LeetCode in 202.3 s and Tamil in 186.7 s. The hosted iPhones opened the Library after scheme approval. Their startup recordings show the Library while the idb accessibility tree is empty. Four Android starts stalled before the first learner action; a fifth case touched a partly clipped mastery control.

| Platform | Sample | Queue (s) | Darwin CLI build (s) | Keyboard helper build (s) | Backend (s) | Boot/install (s) | LeetCode | Tamil | Upload (s) | Job (s) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | ---: |
| android | 1 | 2 | — | — | 30 | 72 | 15/15 in 265.7 s | 14/15 in 284.6 s | 3 | 810 |
| android | 2 | 1 | — | — | 36 | 87 | 14/15 in 388.9 s | 14/15 in 240.3 s | 3 | 934 |
| android | 3 | 2 | — | — | 31 | 73 | 13/15 in 442.1 s | 15/15 in 190.9 s | 3 | 916 |
| ios | 1 | 3 | 205 | 32 | 74 | 366 | — | — | 4 | 999 |
| ios | 2 | 3 | 223 | 26 | 79 | 457 | — | — | 3 | 1148 |
| ios | 3 | 3 | 176 | 27 | 61 | 404 | — | — | 5 | 1036 |

The shared Linux wf build took 116 s.
Android test update publishing job took 176 seconds.
iOS test update publishing job took 171 seconds.
