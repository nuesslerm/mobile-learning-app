# Chat workspace title

On the paired photo chat, the installed Orca app on the `rca` iPhone 18 Pro Max displays the workspace name and Git branch. Tapping the title opens a card showing the full workspace name and absolute checkout directory. Tapping outside dismisses it and keeps the conversation open.

| Same simulator and checkout | Installed Orca | Expo Go |
| --- | --- | --- |
| Chat header | [Orca header](orca-chat.png) | [Expo header](expo-chat.png) |
| Open workspace details | [Orca card](orca-details.png) | [Expo card](expo-details.png) |
| Open and dismiss | [Orca recording](orca.mp4) | [Expo recording](expo.mp4) |

The measured title and branch region differs on **1,079 of 67,200 pixels**. The measured interior of the details card differs on **6,803 of 136,704 pixels**. Comparing Expo before and after dismissal gives **0 of 3,548,160 changed pixels** below the status bar. [Measured rectangles](pixels.json) · [Restored chat](expo-returned.png)

The selected chat's authenticated checkout lookup supplies the display name, current branch and directory. On reconnect or when chat regains focus, Expo refreshes the checkout. A detached checkout uses the connection label. The live probe resolved identical metadata with and without an explicit checkout ID. [Observed gateway response fields](gateway.json)

On the exported app, the same route passed in both light and dark appearance. An unsent draft and the photo response were present again after dismissal. [Browser runbook](web.lua) · [Light](web-light.png) · [Dark](web-dark.png) · [Recording](web.mp4)

Open the saved photo chat in the [iOS update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/c28dec44-cb1d-4db8-a438-ff7ba092a4de) and run the [iOS steps](title.lua) with an existing absolute capture directory.

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test title.lua --device "$RCA_UDID" --env "out=$CAPTURE_DIR" --test-output-dir "$RUN_DIR"
```

Explicit iOS line heights reduced the differences in the card interior from 12,071 to 6,803 pixels. Both directory lines in the final capture sit one physical pixel below their reference positions.

In one local request per route, the selected checkout serialized to 238 bytes and the catalog of 22 checkouts to 4,761 bytes. The checkout lookup took 1,349 ms and the catalog took 1,053 ms in those samples. Chat renders while its metadata loads. [Local response sizes and timings](response-size.json)
