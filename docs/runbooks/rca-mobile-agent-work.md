---
title: rca-mobile-agent-work - exercise all six mobile app and provider combinations
verifies: Each provider can start from dev in a fresh workspace through T3 Code and Orca Mobile, edit and test a small service, and deliver a GitHub PR whose merge commit passes dev CI.
tier: runbook
---

# RCA mobile agent work

Use the checkout of `git@github.com:nuesslerm/mobile-learning-app.git` and native T3 Code and Orca Mobile builds on the iPhone Simulator. Start the local RCA service and executor with `t3-serve` and `orca-serve` connected to that service. In the [Mac pairing site](../../clis/rca/pairing-site/README.md), choose each app and open its generated link in the simulator. Confirm both app pickers show the local Mobile Learning App repository. The [deployed phone pairing guide](phone-test-pairing.md) covers pairing a physical phone to the VM.

Start the local `rca-exec` with `--worktree-base` pointing to a dedicated directory outside its repository root. Confirm the executor can use Git, GitHub CLI, Python, and the three provider CLIs under its own account. Verify GitHub access to this repository before asking an agent to publish a PR.

## Matrix

| App | Provider | First-run branch | Work directory |
| --- | --- | --- | --- |
| T3 Code | Claude | `dev-claude-t3` | `rca-test-work/claude-t3` |
| T3 Code | Codex | `dev-codex-t3` | `rca-test-work/codex-t3` |
| T3 Code | OpenCode | `dev-opencode-t3` | `rca-test-work/opencode-t3` |
| Orca Mobile | Claude | `dev-claude-orca` | `rca-test-work/claude-orca` |
| Orca Mobile | Codex | `dev-codex-orca` | `rca-test-work/codex-orca` |
| Orca Mobile | OpenCode | `dev-opencode-orca` | `rca-test-work/opencode-orca` |

On repeat runs, append a unique run suffix to both the branch and directory. Keep old branches and evidence.

## Run one row

1. Fetch `origin/dev` and record its SHA. In the app on the simulator, select the project, create a fresh workspace from that `dev` revision and select the row’s provider. Verify the new branch and worktree in the app or by inspecting the checkout locally.
2. Send the prompt below through the app. If the provider requests approval, read the proposed command in the app and allow it if it stays within this row. Check that the selected provider replies, that output streams into the app, and that the reported diff matches the files in the fresh workspace.
3. Independently run the provided Python tests and check that the diff against the starting SHA only touches the assigned directory. Verify the agent-created PR targets `dev` and describes the actual changes and commands run.
4. Review the PR, check any existing GitHub checks, and merge via GitHub. Record the PR URL and the merge commit SHA. The current repository workflows run on pushes to `dev`, including PR merges. Use `gh run list --branch dev --commit <sha>` to find runs and `gh run watch <id> --exit-status` to follow them to a conclusion. Record cancelled and superseded runs as such.
5. Keep simulator evidence showing the selected provider and final reply. Keep credentials and unrelated project contents out of evidence. The row passes only after the PR is merged and the checks for its merge commit finish successfully.

In Orca's worktree picker, select `origin/dev` as the source. Under Options, turn off **Existing branch** and enter the matrix branch as the name and new branch name. In T3 Code, choose **New worktree**, enable **Start from remote**, and select `origin/dev`. If T3 assigns its own branch name, verify the worktree was created at the recorded SHA and have the agent rename that test branch to the matrix name before it edits or pushes.

## Commit gate and evidence

The Python tests can run on the Mac. The repository's Lefthook `workflows` job also exercises Linux process handling. If that job fails on macOS, preserve the staged files and run the installed pre-commit hook for the same worktree and index in a native Linux environment. Keep Git signing enabled when committing. Record any operator action needed to finish the row, including approvals, publishing and merging.

For each row, record the app and provider, starting SHA, worktree and branch, local test command and count, simulator screenshot, PR link, merge SHA, and GitHub Actions run links and conclusions. Check the base revision's Actions runs when diagnosing failures shared with the merge.

## Prompt

Replace the branch and directory with the values for the row.

> Work in this fresh `dev`-based workspace on branch `BRANCH` in the Mobile Learning App repository. Inside `DIRECTORY`, build a tiny Python standard-library HTTP server that binds to loopback. Give it a JSON `GET /healthz` endpoint and return 404 for an unknown path. Add tests that make real HTTP requests against an ephemeral port, plus a short README with the exact commands. Run the tests locally. Change files only inside `DIRECTORY`. Commit and push the branch and open a GitHub PR against `dev`. Describe the change and include the actual test command and result. Report the test result and the PR URL in this chat. Leave merging to the operator.

When a row stops early, record which screen or operation failed, the exact error, the time, and the checked-out revision. Treat a standalone API response as diagnostic evidence. The matrix requires a prompt and reply through the native app for each row.
