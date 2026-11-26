# Adding a second pairing on the rca iPhone

Installed Orca v0.0.68 (1) and the [Expo Go update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/3275b225-9261-4550-86e7-b86ffa6ce202) both had saved desktops. From Feed, tapping Pair a desktop opened the [scanner in Orca](official-pair.png) and the [camera permission introduction in Expo](expo-pair.png). Their camera permissions were in different states. The paste path was available in both.

Opening Paste pairing code in Orca or Paste code instead in Expo focused an empty field above the same iOS keyboard. The [Orca sheet](official-paste.png) and [Expo sheet](expo-paste.png) both begin at y=1306 in the 1320 × 2868 screenshots. From that edge to the bottom, 982 of 2,061,840 RGBA pixels differ. All differences fall near the sheet corners and input field. Cancel and Back returned to Feed. No code was submitted and the desktops remained paired. [Orca recording](official-paste.mp4) · [Expo recording](expo-paste.mp4).

The [iOS runbook](open-paste.lua) starts at the paired Feed in light mode. On this simulator, the official two-host Feed used pair=29%,28.4% and button=49%,94%. The single-host Expo Feed used pair=29%,22.9% and button=49%,61.2%. The button coordinate depends on the camera state. The out parameter names the screenshot directory.
