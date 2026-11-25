# Pairing and the photo conversation on iOS

Reference build from Orca commit `980c0024741812bd7ae934c0c22144d9926a784e`. The simulator named `rca` produced these 1320 × 2868 captures in light appearance with the software keyboard dismissed. The separately installed reference app has its own bundle ID and Keychain entries.

1. Start the reference with a fresh pairing invite from the local RCA Orca service. Accept the host. The first recording starts after credential exchange, when the screen offers a session-opening preference.
2. Run `wf ios test onboarding.lua --device <rca-udid> --env out=<absolute-capture-directory> --test-output-dir <recording-directory>` from this directory. Select Chat UI and skip notifications. See [session choices](open-sessions.png), [notification choices](notifications.png), and the [recording](onboarding.mp4).
3. Open the connected host, select the native fixture worktree, and reopen the RCA photo conversation whose final response contains `RCA_ORCA_COLD_FOLLOW_FINAL`. Wait until its two swatches load. See the [settled conversation](photo-thread.png).
4. Tap the tool summary to expand it. See [tool details](tool-details.png) and the [transition](tool-details.mp4).
5. Select the first swatch and open the viewer’s actions menu. See the [viewer](image-preview.png), [opening transition](image-preview.mp4), and [actions sheet](image-actions.png).

These captures establish the upstream checkpoints. Expo still needs a matching native capture and pixel comparison for each row, including keyboard-open and reconnect scenarios.
