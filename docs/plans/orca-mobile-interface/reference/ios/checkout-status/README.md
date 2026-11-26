# Move a checkout to a status

On the paired `rca` iPhone 18 Pro Max, open the desktop, long-press **native-fixture**, and tap **Move to Status**. In installed Orca 0.0.68 the sheet listed Todo, In progress, In review and Done. The initial checkout had no saved status and Orca checked In progress. Selecting In review dismissed the sheet. A fresh paired T3 client then read `in-review` on that checkout. [Installed menu](orca-menu.png) · [Orca sheet](orca-review.png) · [Orca recording](orca-assign.mp4) · [Gateway response](gateway-after-orca.json)

On the [published Expo update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/5351f025-7674-47b1-b4f7-c27eba4b3c0e), opening the same checkout showed In review checked. Selecting In progress saved the change, and reopening the picker in installed Orca showed the checkmark beside In progress. The original unset status and other checkout metadata were verified after restoration. Earlier browser diagnostics also exercised Pin on native-fixture and the disposable workspace-options checkout. Both were returned to their previous unpinned state. [Expo sheet](expo-review.png) · [Expo recording](expo-assign.mp4) · [Orca after the Expo edit](orca-after-expo.png) · [Round trip](round-trip.json)

The first Expo capture used 45 pt rows. The adjusted rows measure 48 pt, with the same separator positions as Orca. Inside the recorded sheet region, 124,981 pixels differed before adjustment and 9,565 of 962,208 differed afterward. The host list behind the sheet is excluded. [Pixel measurements](pixels.json)

## Reproduce

Use a disposable checkout visible to both paired clients. Open its status picker in each client before running the matching assignment. Set `CAPTURE_DIR` to an existing absolute directory. Both runbooks change the checkout’s saved status. [Orca assignment](orca-assign.lua) · [Expo assignment](expo-assign.lua)

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test expo-assign.lua --device "$RCA_UDID" --env "out=$CAPTURE_DIR" --test-output-dir "$RUN_DIR"
```

A separate route flow exercised custom server labels, a failed lookup and retry, and reopening the selector after saving. The gateway test checked scope, stale or unknown status IDs, and preservation of existing settings.

The exported mobile web app completed the same status menu journey in light and dark. A recorded long-press revealed that the closing touch could activate a newly opened sheet action. The sheet now accepts pointer clicks that began on that control and keyboard activation. The browser runbook filters the list to the disposable fixture, waits for the sheet to settle, selects In review and then restores In progress. [Browser runbook](web.lua) · [Light](web-light.png) · [Dark](web-dark.png) · [Recording](web.mp4)

The T3 extension reads the desktop status definitions once when the picker opens. The default four choices took 28 ms locally and occupied 146 bytes of compact JSON in one observed request. Saving revalidates the status ID against the current definitions. [Measured response](status-read.json)
