# Workspace status grouping

On the paired `rca` iPhone 18 Pro Max, installed Orca 0.0.68 and Expo Go opened their host lists with Project grouping. Switching to Status showed **In progress 22** on both. Tapping the heading hid the checkouts and kept the count visible. Tapping it again expanded the list. Both clients returned to Project afterward. A fresh authenticated read of the Expo gateway matched the original workspace preferences.

| iPhone state | Orca | Expo Go |
| --- | --- | --- |
| Expanded | [Screenshot](orca-expanded.png) | [Screenshot](expo-expanded.png) |
| Collapsed | [Screenshot](orca-collapsed.png) | [Screenshot](expo-collapsed.png) |
| Select Status, collapse, expand | [Recording](orca.mp4) | [Recording](expo.mp4) |

The measured header rectangle spans x=35–484 and y=574–644 on the 1320 × 2868 captures. Of 31,950 pixels, the expanded header differed on 2,747, compared with 4,748 before alignment. The collapsed header differed on 2,743, down from 4,758. The two host titles differ. The top-right host actions, bottom-right add button and agent activity under the main checkout are visible differences outside the header. [Measured counts](pixels.json)

## Reproduce

Start on a paired host list in Project mode with the keyboard dismissed and all groups expanded. Use a disposable fixture with all visible checkouts in the default status. Set `WF_IOS_APP_ID` to the selected client, `RCA_UDID` to the test simulator and `CAPTURE_DIR` to an existing absolute directory.

```sh
wf ios test ios.lua --device "$RCA_UDID" --env "out=$CAPTURE_DIR" --env "client=reference" --test-output-dir "$CAPTURE_DIR/journey"
wf ios test restore.lua --device "$RCA_UDID" --env "out=$CAPTURE_DIR" --env "client=reference" --test-output-dir "$CAPTURE_DIR/restore"
```

The [browser runbook](web.lua) starts at the paired Feed. Supply the fixture's `count`, `project` and an absolute `out` directory. Search for native-fixture reduced the group to **In progress 1**. The run then restored Project. [Light search](web-light-search.png) · [Dark search](web-dark-search.png) · [Browser recording](web.mp4)

The production list uses the paired host's ordered status definitions, skips empty sections, and uses the same fallback as the status picker for a removed or missing status. The rendered route test exercises custom ordering, pinned checkouts, disclosure and filtered counts. Disclosure currently lasts for this visit. Saving it across visits and clients remains open.
