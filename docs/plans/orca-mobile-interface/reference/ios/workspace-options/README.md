# Workspace options on the RCA simulator

In the pinned Orca mobile app, open the connected host and tap the lower-left sliders on its workspace list. The sheet identifies these settings as synced across devices. The fixture uses grouping by project, recent sort, manual project order, detailed cards and all eight properties. The five hide filters are off.

| Upstream checkpoint | RCA iPhone capture | Transition into sheet |
| --- | --- | --- |
| Workspace options, grouping and filters | [Options](options.png) | [Open options](options.mp4) |
| Sort by | [Sort choices](sort.png) | [Open sort](sort.mp4) |
| Project order | [Project order](project-order.png) | [Open project order](project-order.mp4) |
| Card layout | [Card layouts](card-layout.png) | [Open layouts](card-layout.mp4) |
| Show properties and agent activity layout | [Property choices](properties.png) | [Open properties](properties.mp4) |

Run `wf ios test options.lua --device <rca-udid> --env out=<absolute-capture-directory> --test-output-dir <recording-directory>` from this directory with the connected host's workspace list open. The [journey](options.lua) uses the same iPhone 18 Pro Max viewport and light theme as the [chat reference](../paired-chat/README.md). Source implementations live in [workspace-options](../../../../../../references/orca/mobile/src/host-screen/workspace-options) and [view-settings](../../../../../../references/orca/mobile/src/host-screen/view-settings).

These are reference captures. Matching candidate frames, setting changes on a second client and persistence after reconnect remain open.

## RCA persistence through T3

Two independently paired clients exercised the workspace preference extension against the same RCA host. The first changed the card layout from Detailed to Compact. After restarting the isolated gateway at the same address, the second read Compact and restored Detailed. The calls used authenticated T3 RPC requests to `mobile.viewPreferences.get` and `mobile.viewPreferences.set`. These methods extend the upstream T3 contract. Matched native UI captures remain open.

## Expo Go on the paired RCA iPhone

Open Workspace options using the sliders under the paired workspace list. The phone reads the saved preferences with one authenticated GET to `/api/mobile/workspace-preferences`. POST accepts the same versioned partial update as `mobile.viewPreferences.set`. Both transports share the RCA state record. A read-only bearer can read the settings. A write needs the orchestration operate scope.

| Checkpoint | Expo capture | Differing pixels in sheet region |
| --- | --- | ---: |
| Workspace options | [Root sheet](expo-options.png) | 126,747 (4.52%) |
| Sort by | [Sort choices](expo-sort.png) | 38,689 (1.38%) |
| Project order | [Project order choices](expo-project-order.png) | 22,788 (0.81%) |
| Card layout | [Layout choices](expo-card-layout.png) | 15,636 (0.56%) |
| Show properties | [Properties and activity layout](expo-properties.png) | 55,728 (1.99%) |

Each comparison counts all RGBA differences across the full width from the higher sheet edge to the bottom (2,801,040 pixels). On these 1320 × 2868 captures all five Orca sheets begin at y=747 and Expo at y=746. The host screen behind the sheet still differs. Expo currently shows projected chats under T3 projects. The native reference shows worktrees, their branch and activity, and the host toolbar.

The [device runbook](expo-options.lua) opens the four pages and changes Detailed to Compact. After opening the published update again, run it with `restoreOnly=true` to check Compact and restore Detailed. The test takes the original and alternate layouts as inputs. From Feed, open the paired desktop before starting the runbook. Use project grouping with the filters off. [Layout change recording](expo-change-layout.mp4) · [Restore recording](expo-restore-layout.mp4).

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test expo-options.lua --device "$RCA_UDID" \
  --env out="$CAPTURE_DIR" --env originalLayout=Detailed --env otherLayout=Compact \
  --env restoreOnly=false --test-output-dir "$RECORDING_DIR/change"
```
