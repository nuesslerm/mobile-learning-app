# Expo checkout Source Control

On the paired `rca` iPhone, long-pressing `native-fixture` and choosing Source Control opened its committed branch changes. Tapping **104 commits** opened history. Tapping “Pack offline Tamil lesson audio” opened its first-parent comparison. Back returned to the commit list. The detached checkout displayed `f5b0c44` in Expo. [Expo Go update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/a428d93d-3cf8-4fd7-a938-516b77776c58) · [Changes](changes.png) · [Commits](commits.png) · [Recording](commit-drill.mp4)

The file list on the commit screen matched all eight paths and per-file counts from Git. The changes totalled 171 added lines and 137 removed. The branch comparison with `origin/main` listed 1,497 files. Gzip reduced that HTTP response from 195,165 bytes to 23,886 bytes on the paired gateway. [Installed Orca commit](orca-commit.png) · [Expo commit](commit.png) · [Measurements](measurements.json)

After scrolling six times in Expo Go, the visible short SHAs matched positions 146 through 160 of `git log`. The list requests 50 commits per page. [Scrolled list](paged.png) · [Pagination recording](paginate.mp4)

## Visual comparison

The installed and replacement commit lists both rendered 14 timeline markers between physical rows 350 and 2,750 of the 1320 × 2868 screenshots. Both used a 168-pixel row pitch. Expo's dots started one physical pixel higher and its metadata text occupied less horizontal space. An exact RGBA comparison found 190,008 differing pixels out of the 3,168,000 in this region. The product-wide parity checklist remains open. [Installed list](../detached-commits.png) · [Expo list](commits.png)

## Runbooks

Start the iPhone on this checkout’s settled Changes screen. Run [commit-drill.lua](commit-drill.lua) with `wf ios test` and an absolute `out` directory, then [paginate.lua](paginate.lua) from the returned list. The [browser runbook](browser.lua) starts on the paired Feed, searches for the checkout, opens a commit and returns to the checkout list. [Browser recording](browser.mp4)

Working-tree status and edits, the Pull request tab, file-diff navigation and menu actions still need implementation in Expo.
