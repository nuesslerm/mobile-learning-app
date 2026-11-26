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
