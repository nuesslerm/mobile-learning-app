# Connected desktop to saved chat

On the `rca` iPhone 18 Pro Max, Expo Go restored its pairing with the local T3 gateway. From [Feed](expo-feed.png), opening **Orca parity** showed the [22 Git checkouts](expo-host.png) in the configured repository. Selecting the detached **native-fixture** checkout opened its saved [photo conversation](expo-chat.png). The header kept the checkout name and displayed the saved tab title, **New thread**. Back returned to the checkout list and then to Feed. [Recording](open-checkout.mp4).

The [Orca host frame](orca-host.png) is the first frame of the earlier [native recording](../workspace-options/options.mp4), when the host had 19 checkouts. The host title and count reflect the saved host configuration and live Git inventory. The reference has activity below the main branch, a host menu and a create-workspace button. Expo's list still needs those controls and the reference toolbar styling. The video compression and different live inventory make a pixel score for these two host captures unsuitable for judging layout.

Comparing the [native photo conversation](../paired-chat/photo-thread.png) to the new Expo capture, 475,656 of 3,785,760 pixels differ (12.56%, full screen, no masks or tolerance). The native tab count is five. Four saved entries resolve to live RCA chats. The composer and the message actions also differ.

## Gateway and reproduction

`GET /api/mobile/workspaces` requires a bearer with orchestration read access. The gateway lists the configured repository's Git checkouts and resolves workspace registrations over one upstream connection. Opening a row supplies its returned ID as a URL-encoded `checkout` query parameter. The gateway resolves the saved active tab against live RCA threads and non-archived T3 projections. It returns the available projected chats when the saved selection cannot open.

Open the published update on the paired simulator at the ordinary coding-agent destination. Run the [journey](open-checkout.lua) with the host label, checkout, saved tab title and a distinctive reply from the chosen chat.

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test open-checkout.lua --device "$RCA_UDID" \
  --env out="$CAPTURE_DIR" --env 'host=Orca parity' --env checkout=native-fixture \
  --env 'tabTitle=New thread' --env 'reply=.*RCA_ORCA_COLD_FOLLOW_FINAL.*' \
  --test-output-dir "$RECORDING_DIR"
```
