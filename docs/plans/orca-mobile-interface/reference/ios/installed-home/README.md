# Installed Orca home navigation

The About screen on the rca iPhone 18 Pro Max shows Orca v0.0.68 (1). The upstream mobile app configuration at the pinned revision declares version 0.0.68, build 1 and bundle ID com.stably.orca.mobile. The desktop conformance fixtures record the host revision they were captured against. [About screenshot](orca-about.png) · [Mobile app configuration](https://github.com/stablyai/orca/blob/2c0cbbc279b61afe80a954fbf43de4f34ef2c32c/mobile/app.json) · [Mobile source inventory](../../../source-inventory.md).

The installed app opened Feed with two paired desktops. The tab bar reads Feed, Bots, Agents, Usage and Automations. Bots uses the robot glyph, and Agents uses a terminal. Settings opens from the gear on Feed and has a centered title and a circular Back button. Back returns to Feed. [Feed](orca-feed.png) · [Settings](orca-settings.png) · [Settings recording](orca-settings.mp4).

Neither paired desktop advertised bot support. The installed app showed “Update Orca on your computers” under Bots. With one desktop the source uses its name, and with no pairing it offers a pairing hint. [Bots screenshot](orca-bots.png) · [Bots recording](orca-bots.mp4) · [Host-dependent states](https://github.com/stablyai/orca/blob/2c0cbbc279b61afe80a954fbf43de4f34ef2c32c/mobile/src/home/tabs/MobileHomeBotsTab.tsx).

The [installed-app runbook](installed-home.lua) starts on the paired Feed, opens Bots, returns to Feed, opens Settings and reads About. The [Expo runbook](expo-home.lua) follows the same path through Settings and returns to Feed. Use the iOS harness on rca with the keyboard closed.

## Expo Go and browser verification

On the [iOS update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/3275b225-9261-4550-86e7-b86ffa6ce202), the paired RCA desktop was named Orca parity. The Bots notice used that name. Opening Settings from Feed and tapping Back returned to the same Feed. [Feed](expo-feed.png) · [Bots](expo-bots.png) · [Settings](expo-settings.png) · [Returned Feed](expo-feed-return.png) · [Simulator recording](expo.mp4).

All four navigation captures are 1320 × 2868 RGBA images. The comparisons use the full width and physical pixels. On both Feed and Bots, the bar starts at y=2615 in both apps. All 333,960 pixels from that edge through the bottom match on each tab. In Settings, all 188,760 pixels from y=190 through y=332 match the installed app. Across the entire settings screen below y=190, 14,728 of 3,534,960 pixels differ, including the installed version, update and check-time rows. Expo's Feed before opening Bots and after returning from Settings has zero differing pixels from y=180 through the bottom. The simulator runbook checks the active glyphs and the settings title so a tap into a different destination fails the run.

The [exported-browser runbook](web.lua) opened the one-host Bots notice, opened Settings, checked the menu and returned to the original host list. [Browser Bots](web-bots.png) · [Browser Settings](web-settings.png) · [Browser recording](web.mp4).

The paired Feed body still differs. Installed Orca shows computer cards, tasks, an update offer for that installed version and a floating add button. The replacement currently lists its paired desktop and checkouts. Bot listing and creation on hosts that advertise those capabilities are open.
