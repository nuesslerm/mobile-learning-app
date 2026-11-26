# Chat attachment image actions

On the **rca iPhone 18 Pro Max, iOS 27**, the installed Orca app and Expo Go opened the same saved two-image conversation in the **native-fixture** checkout. Opening the blue/orange swatch shows the full image. The Share icon beside Close opens three actions. [Orca menu](orca-actions.png) · [Expo menu](expo-actions.png) · [iOS Expo update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/47512b3d-94f7-4b03-888d-e019190efc0c).

| Action | Device evidence |
| --- | --- |
| Copy image | Expo Go confirmed the clipboard write with [Image copied](expo-copy.png). |
| Save image | Expo Go requested add-only access to Photos and confirmed [Image saved](expo-saved.png). The simulator library grew from 8 to 9 assets. The new PNG was 320 × 180. At mid-height its quarter-width pixels were RGB (16, 69, 220) and (249, 126, 12). |
| Share | The iOS activity sheet presented the blue/orange PNG preview in [Orca](orca-share.png) and [Expo Go](expo-share.png). The recording ends before choosing a recipient. |

The gateway supplied the attachment through its existing signed asset URL. The mobile adapter stages the file in a temporary directory, hands the bytes or file to the selected system service, and removes its local staging copy. The update did not require an additional gateway route.

## Reproduce

On this simulator with the paired fixture open, the [native menu runbook](orca-menu.lua) starts in the settled photo conversation. The [Expo runbook](expo-menu.lua) starts in Feed with the fixture desktop first, opens its checkout and the first image, then captures the menu. Both use this recorded iPhone layout. The [copy/save runbook](copy-save.lua) starts on the open Expo menu. Finish the system permission prompt if it appears. Use a disposable image for a save, which adds an asset to Photos.

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test expo-menu.lua --device "$RCA_UDID" \
  --env "out=$CAPTURE_DIR" --test-output-dir "$RUN_DIR/expo"
```

## Pixel comparison

With the same swatch open and the menu settled, Orca's sheet starts at y=2208 and Expo's at y=2209 on a 1320 × 2868 display. The first implementation started at y=2223. Comparing RGBA values across the full width from Orca's sheet top to the bottom gives 10,221 differing pixels out of 871,200 (1.17%). Across the complete screenshot 190,385 of 3,785,760 pixels differ (5.03%). Both counts use zero tolerance and no masks.

The [native reference recording](orca-menu.mp4) opens the swatch and its menu. The [Expo recording](expo-copy-share.mp4) opens the same swatch, copies it and presents the system share sheet. The [copy and share runbook](copy-share.lua) reproduces that sequence.

The exported web app also opened the signed image, downloaded it, displayed the save confirmation, and returned to the same chat in a plain Chromium session. [Recording](web-save.mp4) · [Runbook](web-actions.lua).
