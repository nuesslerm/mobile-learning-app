# Checkout Source Control in Orca 0.0.68

On the `rca` iPhone 18 Pro Max, long-pressing `native-fixture` on the connected desktop opened the checkout menu. Its first action, **Source Control**, opened the **Changes** tab with `dev` beneath the screen title. The tab initially displayed a loading indicator. Once settled, it showed the branch selector, push status, commit count, comparison with `main` and changed files with per-file line counts. [Checkout menu](menu.png) · [Loading](changes-loading.png) · [Changes](changes.png) · [Open recording](open.mp4)

Tapping the commit count opened a scrollable commit history. Each row showed a subject, short SHA, author and relative time. Its first row was the checkout read-state change at `582da1a`. The **Pull request** tab opened PR #103 with its open status, base and head, checks, review threads, approvals, merge conflicts, changed-file count, viewed count and description. These values came from the paired repository at capture time. [Commits](commits.png) · [Pull request](pull-request.png) · [Tab recording](tabs.mp4)

## Checkout identity

These captures document the installed client’s layout. At capture time the RCA adapter supplied the parent repository’s `dev` history, changes and PR #103. The selected `native-fixture` worktree was detached at `f5b0c443`. Use Git in the selected checkout to verify data parity independently of these images.

After correcting the adapter, the paired T3 HTTP endpoint and native Orca RPC each returned the same first 100 commits as `git log` in `native-fixture`. Native branch comparison reported the selected HEAD. The T3 endpoint returned 50 commits per page and rejected a continuation after a HEAD change with HTTP 409. [Paired gateway results](checkout-history.json)

On the same iPhone, the **Commits** screen then opened with “Pack offline Tamil lesson audio” (`f5b0c44`) at the top. The screen still displayed `dev` beneath the title for this detached checkout. The first row is verified against Git. The branch caption remains an open discrepancy. [Updated Commits screenshot](detached-commits.png) · [Recording](detached-commits.mp4)

## Reproduce

Use the paired Mac · Mobile Learning desktop in installed Orca, with default text size and the `native-fixture` checkout visible at the recorded position. Run [open.lua](open.lua) with `wf ios test --env out=<absolute-directory> --test-output-dir <recording-directory>` on the `rca` simulator. Once the changes list has loaded, run [tabs.lua](tabs.lua) with a separate recording directory. The second run ends on the commits screen. Wait for the first commit subject before taking its screenshot. Return to Source Control and reopen Pull request to capture the populated review and checks rows.

To verify the corrected data, start on the selected checkout’s settled **Changes** screen and run [detached-commits.lua](detached-commits.lua). Compare the first displayed SHA with `git rev-parse --short HEAD` run in the same checkout.
