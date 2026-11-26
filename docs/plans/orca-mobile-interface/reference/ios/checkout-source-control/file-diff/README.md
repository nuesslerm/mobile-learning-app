# File diffs on a linked checkout

Installed Orca 0.0.68 on the paired `rca` iPhone 18 Pro Max opened `native-fixture` at `f5b0c44`. From **Source Control → Changes**, the first branch file was `.github/workflows/e2e-tests.yml`. The header showed **1 of 1497 · 0 viewed**. The bottom action read **Viewed · next file**. [Branch file screenshot](branch-file.png)

From **Changes → 104 commits → Pack offline Tamil lesson audio**, the first file was `clis/anki-content/import_tamil.py`. Its header showed **1 of 8 in this commit**, the path row showed **+17 −4**, and the bottom action read **Next file**. The diff displayed numbered lines, highlighted additions and deletions, and controls to expand unchanged lines. [Commit file screenshot](commit-file.png) · [iPhone recording](commit-file.mp4)

For the commit comparison, native RPC returned the file's 8,255 parent bytes and 8,726 committed bytes. Both strings matched the corresponding Git objects exactly. The branch comparison returned an empty original and the same 8,726 committed bytes, consistent with the file's absence at the merge base. [Verification values](git-verification.json)

## Reproduce

Open the paired desktop, long-press `native-fixture`, and choose **Source Control**. Wait for the 1,497-file branch list to load, then tap its first file. Starting from that file view, run [open-commit-file.lua](open-commit-file.lua) with `wf ios test` and `--env out=<absolute-output-directory>`. The runbook returns to Changes, opens the commit list, selects its first commit, then opens its first file.
