# Model and permissions on the RCA iPhone

Open the saved photo conversation and tap its model pill. Orca lists Default, MiniMax-M3 and MiniMax-M3.1-Flash. Its Default row explains that opencode chooses the model. Permissions opens a second page. Compare the [Orca capture](orca-model.png), [Expo Go capture](expo-model.png), [Expo permissions page](expo-permissions.png) and [Expo recording](open-model.mp4).

The Expo run used the same iPhone 18 Pro Max at 1320 × 2868 and a disposable copy of the paired T3 projection. The test selected MiniMax-M3, then Default, then Full access in the mobile UI. An independent authenticated shell request confirmed that the saved thread used the default model and full access. Reopening the published Expo update restored both choices. The original projected thread retained its gpt-5 and approval-required settings.

The top edge of the Model sheet is at y=1674 in Orca and y=1683 in Expo. Comparing every RGBA pixel from the higher sheet edge through the bottom finds 67,553 differing pixels out of 1,576,080 (4.29%). The full-frame difference is 1,110,964 of 3,785,760 pixels (29.35%). The two apps display different worktree labels, chat titles, timestamps and tab counts behind the sheet. Previously the Expo sheet began at y=1710.

Replay the [device test](open-model.lua) against a paired photo chat and the opencode fixture catalog. The model pill's accessible label reflects the current thread settings. The script opens both pages and returns to the chat, preserving the settings.

```sh
WF_IOS_APP_ID=host.exp.Exponent wf ios test open-model.lua --device "$RCA_UDID" \
  --env 'modelButton=Model, Model, full access' --env out="$CAPTURE_DIR" \
  --test-output-dir "$RECORDING_DIR"
```
