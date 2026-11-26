# Switch Branch on a linked checkout

In installed Orca 0.0.68, the branch button on **Source Control → Changes** opened an empty **Switch Branch** sheet for `native-fixture`. The paired RCA adapter returned `runtime_error` from `git.localBranches`. [Before](empty.png)

On the same iPhone and checkout, the repaired adapter returned 31 local branches. Their names matched `git for-each-ref refs/heads`. The current branch was null, matching this checkout's detached HEAD at `f5b0c44`. The picker displayed the branches and scrolled through the list. The header of the underlying Changes page still displayed `dev`. The runbook opened the picker and left HEAD at `f5b0c44`. [Branch sheet](branches.png) · [Recording](open.mp4) · [RPC and Git check](git-verification.json)

Start on the connected checkout's settled Changes page, with its branch row visible below the tabs. Run [open.lua](open.lua) with `wf ios test` and `--env out=<absolute-output-directory>` on `rca`. [Expo Go now switches the selected worktree](expo/README.md).
