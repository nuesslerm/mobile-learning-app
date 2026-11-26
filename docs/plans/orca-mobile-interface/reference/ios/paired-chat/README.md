# Pairing and the photo conversation on iOS

Reference build from Orca commit `980c0024741812bd7ae934c0c22144d9926a784e`. The simulator named `rca` (iPhone 18 Pro Max, iOS 27.0, UDID `D81B0C0F-5E6A-4A35-8E49-851FAFCFB7AB`) produced these 1320 × 2868 captures in light appearance with the software keyboard dismissed. The separately installed reference app has its own bundle ID and Keychain entries.

1. Start the reference with a fresh pairing invite from the local RCA Orca service. Accept the host. The first recording starts after credential exchange, when the screen offers a session-opening preference.
2. Run `wf ios test onboarding.lua --device <rca-udid> --env out=<absolute-capture-directory> --test-output-dir <recording-directory>` from this directory. Select Chat UI and skip notifications. See [session choices](open-sessions.png), [notification choices](notifications.png), and the [recording](onboarding.mp4).
3. Open the connected host, select the native fixture worktree, and reopen the RCA photo conversation whose final response contains `RCA_ORCA_COLD_FOLLOW_FINAL`. Wait until its two swatches load. See the [settled conversation](photo-thread.png).
4. Tap the tool summary to expand it. See [tool details](tool-details.png) and the [transition](tool-details.mp4).
5. Select the first swatch and open the viewer’s actions menu. See the [viewer](image-preview.png), [opening transition](image-preview.mp4), and [actions sheet](image-actions.png).

These captures establish the upstream checkpoints. Expo still needs a matching native capture and pixel comparison for each row, including keyboard-open and reconnect scenarios.

## First integrated Expo comparison

The native Expo release build at commit `1d169f4a` opened the same saved thread through the installed app link on the RCA simulator. The composer used the Orca implementation. The [candidate](comparison/mla-photo-thread.png) differed on **3,036,603 of 3,785,760 pixels (80.21%)** in an exact comparison without masks. The candidate opened at the beginning of the conversation, displayed ordinary text rows for tool activity, and omitted both image previews. The header, background, composer controls, scroll anchoring and transcript layout also differ. This checkpoint is open.

## Native Expo with the T3 transport

On the same RCA simulator the installed Expo release reopened the original RCA photo thread over T3. The bounded snapshot returned four tool calls at the same UTF-16 offset in the assistant response, after its progress update. Expo showed the tool group there, with both images from the actual attachment store. Expanding the group, opening the first image and returning to the conversation worked through the native UI.

| Checkpoint | Orca | Expo |
| --- | --- | --- |
| Conversation | [Reference](photo-thread.png) | [Candidate](comparison/mla-t3-photo-thread.png) |
| Expanded tools | [Reference](tool-details.png) | [Candidate](comparison/mla-t3-tool-details.png) |
| First image | [Reference](image-preview.png) | [Candidate](comparison/mla-t3-image-viewer.png) |

The new conversation comparison differs on **1,465,668 of 3,785,760 pixels (38.715%)**, with zero tolerance and no masks. The worktree navigation, text metrics, time and action rows, composer controls, and safe-area spacing remain open. The image viewer also needs its actions sheet. Reproduce the candidate journey with the [native runbook](comparison/reopen-t3-photo.lua), supplying the accessibility label of the first stored image as `firstImage` and an absolute capture directory as `out`.

The [Expo recording](comparison/reopen-t3-photo.mp4) shows the tool disclosure, first image preview and return to the same thread on the RCA simulator.

## Grouped messages in the native Expo release

The [Expo capture](comparison/mla-t3-grouped-photo-thread.png) uses the Orca prose scale, moves the progress message's copy and timestamp below its tool results and accounts for the iPhone bottom inset. An unmasked comparison with the pinned [Orca capture](photo-thread.png) differs on 1,260,080 of 3,785,760 pixels (33.28%). The [RCA recording](comparison/mla-t3-grouped-photo-thread.mp4) exercises the expandable tools, attachment viewer and return to chat. Session chrome, composer actions and timestamps still differ.
