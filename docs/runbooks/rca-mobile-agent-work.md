---
title: rca-mobile-agent-work - exercise all six mobile app and provider combinations
verifies: Each provider can start from dev through T3 Code and Orca Mobile, deliver a tested PR, and handle interactive agent work in the app.
tier: runbook
---

# RCA mobile agent work

Use the checkout of `git@github.com:nuesslerm/mobile-learning-app.git` and native T3 Code and Orca Mobile builds on the iPhone Simulator. Start the local RCA service and executor with `t3-serve` and `orca-serve` connected to that service. In the [Mac pairing site](../../clis/rca/pairing-site/README.md), choose each app and open its generated link in the simulator. Confirm both app pickers show the local Mobile Learning App repository. The [deployed phone pairing guide](phone-test-pairing.md) covers pairing a physical phone to the VM.

Start the local `rca-exec` with `--worktree-base` pointing to a dedicated directory outside its repository root. Set `RCA_STATE_DIR` to a persistent local directory before starting the service so its thread events survive a restart. Keep the service running through the matrix because executor workspace registrations are in memory. Confirm the executor can use Git, GitHub CLI, Python, and the three provider CLIs under its own account. Verify GitHub access to this repository before asking an agent to publish a PR.

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
When testing Codex, compare the model offered in the app with the connected CLI's `model/list` before starting a run. Record the model along with the provider.

## Run one row

1. Fetch `origin/dev` and record its SHA. In the app on the simulator, select the project, create a fresh workspace from that `dev` revision and select the row’s provider. Verify the new branch and worktree in the app or by inspecting the checkout locally.
2. Send the prompt below through the app. If the provider requests approval, read the proposed command in the app and allow it if it stays within this row. Check that the selected provider replies, that output streams into the app, and that the reported diff matches the files in the fresh workspace.
3. Independently run the provided Python tests and check that the diff against the starting SHA only touches the assigned directory. Verify the agent-created PR targets `dev` and describes the actual changes and commands run.
4. Review the PR, check any existing GitHub checks, and merge via GitHub. Record the PR URL and the merge commit SHA. The current repository workflows run on pushes to `dev`, including PR merges. Use `gh run list --branch dev --commit <sha>` to find runs and `gh run watch <id> --exit-status` to follow them to a conclusion. Wait for the core checks before merging the next row. Record cancellations and which stages CI ran for each merge.
5. Keep simulator evidence showing the selected provider and final reply. Keep credentials and unrelated project contents out of evidence. The row passes only after the PR is merged and the checks for its merge commit finish successfully.

In Orca's worktree picker, select `origin/dev` as the source. Under Options, turn off **Existing branch** and enter the matrix branch as the name and new branch name. In T3 Code, choose **New worktree**, enable **Start from remote**, and select `origin/dev`. If T3 assigns its own branch name, verify the worktree was created at the recorded SHA and have the agent rename that test branch to the matrix name before it edits or pushes.

If Orca restores a sent message to the composer, check the RCA thread history for the turn before resending. Record a terminal fallback or an operator-created structured chat as a launch failure or workaround for that row.

## Commit gate and evidence

The Python tests can run on the Mac. The repository's Lefthook `workflows` job also exercises Linux process handling. If that job fails on macOS, preserve the staged files and run the installed pre-commit hook for the same worktree and index in a native Linux environment. Mount the worktree and Git common directory at their original paths. Use a separate Cargo target directory for each worktree because workflow tests embed their checkout path at compile time. For an agent to finish the commit, configure a hook that delegates to Linux with `git config --worktree core.hooksPath <directory>` in the test worktree. The hook should propagate the Linux gate's exit status. Keep signing on the Mac. Record any operator action needed to finish the row, including approvals, publishing and merging.

For each row, record the app and provider, starting SHA, worktree and branch, local test command and count, simulator screenshot, PR link, merge SHA, and GitHub Actions run links and conclusions. Check the base revision's Actions runs when diagnosing failures shared with the merge.

## Prompt

Replace the branch and directory with the values for the row.

> Work in this fresh `dev`-based workspace on branch `BRANCH` in the Mobile Learning App repository. Inside `DIRECTORY`, build a tiny Python standard-library HTTP server that binds to loopback. Give it a JSON `GET /healthz` endpoint and return 404 for an unknown path. Add tests that make real HTTP requests against an ephemeral port, plus a short README with the exact commands. Run the tests locally. Change files only inside `DIRECTORY`. Commit and push the branch and open a GitHub PR against `dev`. Describe the change and include the actual test command and result. Report the test result and the PR URL in this chat. Leave merging to the operator.

When a row stops early, record which screen or operation failed, the exact error, the time, and the checked-out revision. Treat a standalone API response as diagnostic evidence. The matrix requires a prompt and reply through the native app for each row.

## Run the interaction checks

Reuse the row's fresh workspace after its Python tests. Keep these prompts read-only. Use a unique marker per app, provider, and operation so replies and tool output can be matched to their turns.

1. **Model switching.** In the same thread, send a short prompt with the first model. Choose a different model in the app, send a second prompt, and check the provider's turn metadata or CLI log for the effective model. Record a single-model catalog or a rejected provider credential as a blocker for this check.
2. **Media.** Import two synthetic PNGs into simulator Photos, each with two distinct color blocks. Attach both in the app and ask the agent to report the left and right colors of each in attachment order. Compare the reply to the images. Record how the app renders the sent attachments and screenshots of the reply.
3. **Skills.** Type `/` in the workspace composer. Select an installed project skill and ask the agent to apply it to a read-only check, such as verifying a reported Python test command. Capture the menu, the agent's skill load or tool request, and the result of the check. Catalog discovery and skill execution are separate checks.
4. **Subagent and background task.** Ask the CLI to start a native subagent that counts lines in the Python server created for the row and returns the count. Ask the parent to start a background task that sleeps for 60 seconds and prints your background marker. Have the parent collect the result. Verify the subagent invocation, the task output, and the exit status or the provider's completion event. Record whether the app displayed child activity.
5. **Steer and queue.** Once the provider is active in step 4, send a message to add a steer marker to the active turn's report. Use the app's steer control or promote a queued message to steer. Then queue a separate message with a different marker and leave it waiting. Record `turn.steered`, the first report, and `queue.started` after the original `turn.finished`. Verify that the queued prompt runs once and that its reply contains only its own marker. To exercise queued model selection, choose the second model before queuing and check the effective model when the queued turn starts.

Keep a result for every operation in every row. Record the thread and turn IDs, the selected model, and the screenshot or event sequence that supports the result. Label a device action separately from an adapter RPC and a CLI tool trace. If a turn returns slowly or a send times out, look up the turn and queue by ID before retrying.

## Parity regression cases

Use the case IDs and Given/When/Then steps in [the parity case manifest](rca-mobile-parity.json). Compare with [observed results](rca-mobile-parity-results.md) before assigning a gap to the client, adapter, or provider. Run each relevant case on both apps and use the three providers wherever the case exercises provider behavior.

For future harness integration, drive the app through accessibility labels or stable test IDs where the installed build exposes them. Pair each UI action with a read-only adapter or RCA assertion. Wait for the case's specific event, such as a pending request or a queued item ID, before the next action. Use scripted providers and release gates for ordering, disconnection, prompts, and pagination. Run a separate live-provider check for vision output, effective model, skill loading, child execution, and background output. The current `wf test run` targets Android and browser. The iOS visual check captures and compares a screen and needs an iOS driver to exercise these flows.

Keep a per-run JSON result keyed by case ID, app, provider, and evidence tier (`simulator`, `adapter`, or `physical`). Record the app build, server revision, host OS, start and finish times, verdict (`pass`, `fail`, `partial`, `blocked`, or `not-run`), assertion outcomes, and evidence paths. Add thread, turn, queue and request IDs where relevant. Keep recordings and sanitized event traces beside the result and link the report from the results ledger. Exclude pairing data, authorization headers, and unrelated workspaces from the bundle.

## Retire test worktrees

At the end of a run, or before starting another run, use `git worktree list` to find checkouts created for finished test rows and temporary verification. Keep any checkout still needed to reproduce a failure or open in an active test. Save the evidence outside the checkout and verify that its commits are reachable from a retained branch or merged PR.

Check that no turns, queued messages, setup jobs, or terminal commands still need the workspace. Close any idle test-only terminals for the worktree you are retiring. Run `git status --porcelain --untracked-files=all` in the checkout and inspect ignored files for local evidence. Move any evidence or untracked caches you need to keep out of the checkout. Leave worktrees with uncommitted work for follow-up.

Remove an idle, clean worktree through RCA's `worktree.remove` if it is still registered. For a checkout left behind after an RCA restart or created directly in Git, use `git worktree remove -- "<path>"`. Verify the checkout is gone from Git and any RCA registration is gone from the host. Keep the test branch and PR links so the checkout can be recreated later.
