# Session navigation on the RCA iPhone

The pinned native Orca client shows five saved tabs for the selected worktree. The [tabs sheet](orca-tabs.png) and [more-actions menu](orca-more-actions.png) were captured from the existing photo chat. The [settled reference conversation](../paired-chat/photo-thread.png) shows the original header and its controls.

The installed native Expo release opened the same photo conversation through the authenticated T3 gateway. The [chat](expo-chat.png) now shows the connection state and the switcher. From the [picker](expo-tabs.png), the RCA runbook opened the other projected thread, verified its distinctive saved response, returned to the photo thread and verified its attachment. [Watch the recording](switch-tabs.mp4).

The unmasked chat comparison found **494,512 of 3,785,760 pixels different (13.06%)**. The current endpoint supplies two openable T3 chats in the active project's RCA executor workspace. Orca's five saved worktree tabs, their titles, the checkout label, new-tab and source-control actions, composer controls and timestamps remain open.

The RCA extension is `GET /api/orchestration/threads/{threadId}/session-context`. It requires an orchestration read bearer and protocol version 2. It resolves tabs at connection and on opening the switcher. The ordinary T3 thread subscription continues to carry messages.

To replay [switch-tabs.lua](switch-tabs.lua), open the established Expo photo conversation. Pass the two tab names, the sibling's saved reply and the attachment name from the same fixture. The script checks conversation content after both selections.

```sh
wf ios test switch-tabs.lua --device "$RCA_UDID" \
  --env out="$CAPTURE_DIR" --env workspace=mac-mobile-learning \
  --env 'currentTitle=Native fixture photo thread' --env 'otherTitle=Orca parity capture' \
  --env 'countLabel=2 tabs' --env otherReply=RCA_CHAT_T3_OK --env photo="$FIRST_ATTACHMENT" \
  --test-output-dir "$RECORDING_DIR"
```
