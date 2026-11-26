# Switch Branch in Expo Go

On the `rca` iPhone 18 Pro Max, the [published iOS update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/01f7c4bd-5a26-4b97-8fe6-19e48fc48ecf) opened Source Control for a detached disposable worktree. The sheet listed 31 local branches. Selecting `expo-mobile-1125` changed that worktree to the chosen branch. Changes displayed the confirmed branch during the refresh. Reopening the sheet showed the checkmark and **current** subtitle. Git independently reported `expo-mobile-1125` in that worktree and `dev` in the project checkout. The [recording](switch.mp4) starts on the settled Changes screen. [Initial list](branches.png) · [Changes after selection](changes.png) · [Current branch](current.png) · [Git and API verification](verification.json)

### Reproduce on a disposable checkout

1. Choose an existing local branch that is free in `git worktree list`. Create a disposable worktree with `git worktree add --detach <new-directory> <branch>`. Leave the normal project checkout in place.
2. In the paired Expo app, open the desktop, long-press the disposable checkout, and select **Source Control**. Wait for its starting revision to appear.
3. Run [switch.lua](switch.lua) through `wf ios test` on `rca` in Expo Go. Pass an existing output directory with `--env out=<absolute-directory>` and the intended branch row with `--env branch_point=<x%,y%>`. The recorded fourth row was at `27%,31%`.
4. Check `git -C <new-directory> branch --show-current` and the branch of the original checkout. A switch to a branch occupied elsewhere returns HTTP 409. Local edits that Git would overwrite remain intact.

### Visual comparison

Both captures use light appearance at 1320 × 2868. The [installed Orca reference](../branches.png) and Expo show the same branch list with a detached HEAD. Their underlying Changes pages show different checkouts. The full frame includes those backgrounds and the system clock. The sheet-body comparison covers every pixel across the width from physical row 288 to the bottom, without masking text, separators or rows. [Exact RGBA counts](pixels.json)

| Capture | Differing full-frame pixels | Differing sheet-body pixels |
| --- | ---: | ---: |
| First Expo run | 9.10% | 5.51% |
| Adjusted row spacing | 5.14% | 1.13% |
| Published run | 4.33% | 0.22% |
