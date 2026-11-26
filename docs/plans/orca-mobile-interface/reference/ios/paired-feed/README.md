# Paired Feed and Tasks on the rca iPhone

On the iPhone 18 Pro Max named rca, the Expo Go coding-agent route opens a paired desktop card with its connection indicator, worktree count and OS. Selecting the desktop opens its workspaces. Back returns to the same Feed. [iOS update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/d85216e6-6100-41c7-a5cf-123831f993e8) · [Feed](expo-feed.png) · [Desktop](expo-host.png) · [Return](expo-return.png) · [Recording](expo-desktop.mp4) · [Runbook](expo-desktop.lua).

The installed Orca v0.0.68 (1) has two paired desktops. Its Tasks row opens “Open Tasks On.” Selecting “Mac - Mobile Learning” shows a loading state and then “Update Orca on this host.” The reference runbook waits for the latter and returns to Feed. Multi-desktop selection and Tasks are open in Expo. [Orca Feed](orca-feed.png) · [Desktop picker](orca-tasks-picker.png) · [Tasks compatibility notice](orca-tasks-result.png) · [Recording](installed-tasks.mp4) · [Runbook](installed-tasks.lua).

## Pixel comparisons

Both iOS captures are 1320 × 2868 pixels in light appearance. The reference and Expo desktop names differ. The worktree subtitle is identical in wording. The desktop heading matched all 112,200 pixels. The machine tile region differed at 315 of 28,891 pixels, the subtitle region at 11 of 40,000 and the chevron region at 106 of 22,836. The pairing row differed at 212 of 174,240 after aligning it by one 174-pixel desktop row. The Expo Feed before and after opening a desktop matched all 3,548,160 pixels below y=180. The comparison file gives the exclusive right and bottom bounds in physical pixels. [Region measurements](pixel-comparison.json).

## RCA mobile API

The RCA extension GET `/api/mobile/home` requires `orchestration:read`. It reads the default workspace's Git worktree list once and returns the host name, OS and worktree count. Opening a desktop requests the detailed catalog, including saved labels and pins. A 404 from an older gateway falls back to its workspace catalog. Authentication, authorization and server errors propagate to the Feed.

On the paired gateway, the compact response for 22 worktrees was 64 bytes and the detailed catalog was 4,431 bytes. A credential issued before the local gateway restart worked at the same address afterward. The flow suite checks that opening Feed skips the detailed catalog, that opening the desktop reads it, and that a 403 never triggers the compatibility fallback.

The exported browser runbook reads the same count, opens the desktop, verifies the native-fixture checkout, then returns to Feed. [Browser runbook](web.lua) · [Browser screenshot](web-feed.png) · [Browser recording](web.mp4).
