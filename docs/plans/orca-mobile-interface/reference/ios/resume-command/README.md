# The /resume command in mobile chat

Installed Orca 0.0.68 (1) and Expo Go were paired to the same native-fixture chat on the rca iPhone 18 Pro Max. Selecting /resume kept the slash draft and the Commands menu open. Typing /resume and pressing Send kept the full draft, dismissed the keyboard and showed “Available in the desktop app” above the filtered list. The notice disappeared by the four-second check in both clients. The RCA-backed thread still contained the same two messages and used the same model after both native journeys and the browser replay.

| Action | Installed Orca | Expo Go |
| --- | --- | --- |
| Open Commands | [Menu](orca-commands.png) | [Menu](expo-commands.png) |
| Pick /resume | [Notice](orca-pick.png) · [Recording](orca-pick.mp4) | [Notice](expo-pick.png) · [Recording](expo-pick.mp4) |
| Send /resume | [Notice](orca-send.png) · [Recording](orca-send.mp4) | [Notice](expo-send.png) · [Recording](expo-send.mp4) |

The screenshots measure 1320 × 2868 pixels. After Send, the first red text pixels appeared at y=2075 in Orca and y=2080 in Expo. Across the bottom 956 rows, 58,762 of 1,261,920 pixels differed. The model controls, microphone and available commands differ between the captured clients. The unfiltered Orca menu also showed /effort.

The [Expo navigation runbook](expo-navigation.lua) starts on the paired Feed, opens the desktop and the native-fixture checkout, and waits for the composer. The [selection runbook](orca-pick.lua) starts in the open menu with the keyboard dismissed. Both clients used the same selection and [Send runbook](orca-send.lua). To prepare the Send case, start with the preserved slash draft and run [the typing steps](type-resume.lua) on the iOS English keyboard. Set the runbook's out variable to a writable capture directory. Each recording includes the notice expiring.

On the dark mobile web export, the [browser runbook](web.lua) selected /resume, typed an argument, pressed Send, and asserted that the notice disappeared while the draft remained editable. [Browser screenshot](web-send.png) · [Browser recording](web.mp4) · [iOS EAS Update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/91c0f05e-6f97-4040-846a-3d546c82519b)
