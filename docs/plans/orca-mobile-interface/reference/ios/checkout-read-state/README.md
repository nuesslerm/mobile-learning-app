# Checkout read state

The installed Orca 0.0.68 app and Expo Go shared one checkout on the `rca` iPhone 18 Pro Max, iOS 27. In installed Orca, holding `native-fixture` opened **Mark Unread**. Selecting it added an amber bell beneath the status dot and changed the menu to **Mark Read**. The paired Expo app displayed the same state. Mark Read in Expo cleared the bell and restored **Mark Unread**. [Expo Go update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/116b2bbd-bd12-457c-91f0-1f2a4115731b)

| State | Installed Orca | Expo Go |
| --- | --- | --- |
| Read | [Checkout list](orca-read.png) · [Mark Unread](orca-mark-unread.png) | [Checkout list](expo-read.png) · [Mark Unread](expo-mark-unread.png) |
| Unread | [Checkout list](orca-unread.png) · [Mark Read](orca-mark-read.png) | [Checkout list](expo-unread.png) · [Mark Read](expo-mark-read.png) |
| Interaction | [Set unread](orca-unread.mp4) | [Mark read and reopen the menu](expo-read.mp4) |

The list contained 22 checkouts. An authenticated catalog read returned `unread: true` for both `native-fixture` and `main` after the native write. After the Expo write, the fixture returned `false` and `main` still returned `true`. The gateway restarted between the two clients and kept its pairing. Both bell glyphs contained 410 amber pixels with the same 24 × 28 physical-pixel bounds. The fixture bell appeared 80 physical pixels higher in Expo. Expo's menu currently has three actions. The reference menu shown here has seven.

## Run the comparison

With the connected list settled on the fixture, pass its visible row position to [native-hold.lua](native-hold.lua) using `wf ios test --env rowPoint=… --env out=…`. Check the captured menu title and action. Pass the action's visible position to [native-action.lua](native-action.lua) with `--env actionPoint=…` and a fresh output directory. On the recorded screen, the Orca row was at `25%,81.8%` and Mark Unread at `24%,77%`. The Expo row was at `25%,79.3%` and Mark Read at `23%,87.7%`.

Checking a managed checkout aggregates its manual flag and listed, unread agent panes. Mark Read acknowledges that checkout's pending panes. Unmanaged checkouts retain their own manual flag. The Rust flow tests exercise another checkout's unread finish and an unregistered worktree. The on-device recording exercises the manual flag.

## Mobile browser

On a paired dark iPhone 15 Pro browser viewport, open the host and scroll the `native-fixture` row into view with the keyboard dismissed. Run [browser.lua](browser.lua) with the screenshot directory in `out`, the screen height in `height` (852 here) and the original marker in `initialUnread`. Use a fixture whose final state should be read. The runbook waits for the menu to finish moving, marks the checkout unread, reopens the menu to mark it read and confirms that Mark Unread is available again. An independent gateway read confirmed the fixture was read and the main checkout was still unread. [Menu](web-mark-read.png) · [Recording](web-read.mp4)
