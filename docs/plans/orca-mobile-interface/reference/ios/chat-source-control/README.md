# Source Control from a paired chat

On the `rca` iPhone 18 Pro Max simulator, open the paired photo conversation on `main`, tap the branch button in the chat header, then go back. The installed Orca app and Expo Go both returned to the same conversation. The [Expo update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/a32e7e9e-9804-4776-8ced-36b68a76137b) opens the Coding agent destination.

| Step | Installed Orca | Expo Go |
| --- | --- | --- |
| Open photo conversation | [Chat](orca-chat.png) | [Chat](expo-chat.png) |
| Tap branch button | [Changes and Pull request](orca-source.png) | [Changes and history](expo-source.png) |
| Tap Back | [Returned chat](orca-returned.png) | [Returned chat](expo-returned.png) |

The 1320 × 2868 captures put the branch glyph at the same coordinates. The button crop differs on 300 of 17,145 pixels. The Expo conversation before and after Back matches on all 3,548,160 pixels below row 180. The visible tab count, connection subtitle, timestamps, and composer controls still differ between clients. Orca displayed a loading state on Changes during the reference capture. [Pixel measurements](pixels.json)

The live gateway resolved the saved thread to the same main checkout with and without an explicit checkout ID. The request returned HTTP 404 for an unrelated checkout and HTTP 401 without a bearer. [Probe results](gateway.json)

The iOS recordings show the actual button and return gesture. [Orca](orca.mp4) · [Expo round trip](expo-roundtrip.mp4) · [Recorded Expo steps](expo-roundtrip.lua)

With either app foregrounded on the paired photo chat, run the same route using its bundle ID and an existing screenshot directory.

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test chat-source.lua --device "$RCA_UDID" --env "out=$PWD/captures" --test-output-dir "$PWD/result"
```

In a second reference run the installed app reported branch `dev` once the checkout loaded. Selecting Pull request displayed the loading indicator and kept the commit composer and Stage all control visible. The capture ended before a PR summary or empty state appeared. [Selected tab](orca-pull-request.png) · [Recording](orca-pull-request.mp4)

On the exported mobile web app, the runbook inserted an unsent `/model ` draft, opened Source Control, then returned. Both light and dark runs found the draft and the same final assistant response. [Browser runbook](web.lua) · [Light return](web-light-returned.png) · [Dark return](web-dark-returned.png) · [Dark recording](web.mp4)
