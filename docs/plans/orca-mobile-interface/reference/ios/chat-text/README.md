# Chat text settings

The **rca iPhone 18 Pro Max on iOS 27** ran installed Orca 0.0.68 and Expo Go against the paired photo thread. Open **More actions → Text size → More text settings**. Moving the slider and choosing a font or spacing changes the open chat. The regular Settings → Chat UI entry edits the same device preference. [Expo Go update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/65315bb0-90e3-4d61-b9ac-e978bbdef958)

| Capture | Installed Orca | Expo |
| --- | --- | --- |
| Quick sheet | [Chat text](orca-sheet.png) | [Chat text](expo-sheet.png) |
| Full settings | [Default preview](orca-settings.png) | [Default preview](expo-settings.png) |
| Font picker | [Options](orca-fonts.png) | [Options](expo-fonts.png) |
| Interaction | [Settings recording](orca-settings.mp4) | [Change, return and reset](expo.mp4) |

## Native verification

All screenshots have 1,320 × 2,868 physical pixels. In the installed Orca settings page, Compact changed 434,632 of the 2,984,520 pixels in rows 349 through 2609. Restoring Default changed zero pixels compared with the initial capture. [Compact preview](orca-compact.png)

Expo restored a previously saved 21 pt Serif, Compact chat after relaunch. Then the runbook reset to defaults, selected those settings again and returned to the same thread. The transcript region in rows 475 through 2479 changed in 923,503 of 2,646,600 pixels. After resetting again, every pixel in that region matched the initial default. The quick sheet at 21 pt after returning from Settings matched its cold-launch capture across all 1,095,600 pixels in rows 1930 through 2759. [Initial chat](expo-chat-default.png) · [Edited chat](expo-chat-edited.png) · [Restored chat](expo-chat-restored.png) · [Cold launch](expo-cold-sheet.png) · [Live sheet](expo-sheet-edited.png) · [Measurements](measurements.json)

The default Expo settings content differs from installed Orca in 608,501 of 2,984,520 pixels. The code preview needs Orca's syntax colors, line numbers, and copy control. Its smaller height also moves the text controls upward. The default-view row currently displays On. Switching to the raw terminal and pinch gestures still need device verification and the terminal destination still needs implementation.

## Reproduce

Use the named rca simulator, light appearance and the paired fixture thread. Each Lua file accepts an absolute screenshot directory as the `out` environment variable in `wf ios test`. Use a separate test-output directory per recording.

- In installed Orca, start on the paired host and run [open settings](orca-settings.lua), [open the font picker](orca-font.lua), then [change and restore spacing](orca-spacing.lua).
- In Expo Go, configure 21 pt, Serif and Compact once. Relaunch at the linked update's paired Feed, run [open the chat sheet](expo-start.lua), [open settings and reveal Reset](expo-cold-settings.lua), then [verify the round trip](expo-round-trip.lua). The last run returns to the chat with default settings.
- In the exported app on dark mobile web, start on the paired thread and run [the header and settings journey](web.lua). Open the settings URL, run [save Monospace](web-save.lua), reload the same route and run [verify the stored font and reset it](web-cold.lua). [Settings](web-settings.png) · [Cold reload](web-cold.png) · [Recording](web.mp4) · [Cold-reload recording](web-cold.mp4)
