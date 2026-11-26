# Rename and pin a checkout

Expo Go and the installed Orca app paired independently with the same RCA desktop on the `rca` iPhone 18 Pro Max running iOS 27. Both listed 22 Git checkouts. The simulator used the iOS [Expo update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/734675cb-7f0b-408d-ac56-8fd4539b9c30).

Long-pressing **native-fixture** opens the [Orca checkout menu](orca-actions.png) or the [Expo menu](expo-actions.png). Compare the focused name in [Orca](orca-rename.png) and [Expo](expo-rename.png). Both select the existing name and show Cancel and Save above the software keyboard. Expo implements Rename and Pin/Unpin. The reference also shows Source Control, Sleep, Mark Unread, Move to Status and Delete. The host toolbar and row activity differ.

In the [Expo recording](edit-checkout.mp4), the operator replaced the selected name, tapped Save and pinned the renamed row. A fresh authenticated catalog read returned the edited name and pin. The [Expo pinned section](expo-pinned.png) and [Orca pinned section](orca-pinned.png) show the shared record. Unpin in the [Orca recording](verify-orca.mp4) changed the gateway response to `pinned=false`. A separate [restore capture](expo-restored.png) shows the automatic checkout name after clearing the field and saving in Expo.

## Repeat on the fixture

The Expo runbook starts on the settled host list with the disposable **native-fixture** checkout unpinned and its automatic name visible. The recorded coordinates are for this fixture, iPhone and English keyboard. The test edits shared desktop metadata. The native verification starts on the reopened host list with the edited checkout pinned. Confirm the edited row is visible under Pinned before opening its menu. The runbook requires an accessible Unpin button so a reconnecting host cannot accidentally pin another row. The native recording was taken with a visually verified Unpin button.

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test checkout-actions.lua --device "$RCA_UDID" \
  --env "out=$CAPTURE_DIR" --env 'rowPoint=28%,79.3%' --env 'renamedRowPoint=27%,83.8%' \
  --test-output-dir "$RUN_DIR/expo"
WF_IOS_APP_ID=com.stably.orca.mobile wf ios test verify-orca.lua --device "$RCA_UDID" \
  --env "out=$CAPTURE_DIR" --test-output-dir "$RUN_DIR/orca"
```

After verification, clear the disposable checkout’s display name in Expo and tap Save. Confirm its automatic name and pin state with a new authenticated catalog request.

## RCA gateway

`GET /api/mobile/workspaces` reads one RCA metadata snapshot for the checkout list. `POST /api/mobile/workspaces` takes the checkout ID from that catalog and a display name, a pin, or both. It requires orchestration operate access and preserves the other stored metadata. An empty display name restores the automatic name.

The exported [web recording](web-rename.mp4) opens the menu with a touch held on main, opens [Rename Workspace](web-rename.png), checks the branch-name hint and cancels. The browser sends a trailing click when the finger lifts. The backdrop now requires a press that began on it before that click can dismiss the sheet.
