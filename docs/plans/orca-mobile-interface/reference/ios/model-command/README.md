# Opening Model from the chat composer

On the rca iPhone 18 Pro Max, installed Orca and Expo Go opened the saved photo conversation under native-fixture. Picking the command inserted it in the draft. Submitting the draft opened the same sheet as the Model pill. Done closed the sheet and the composer was empty.

| Step | Installed Orca | Expo Go |
| --- | --- | --- |
| Open Commands | [Menu](orca-commands.png) | [Menu](expo-commands.png) |
| Pick /model | [Draft](orca-model-draft.png) · [Recording](orca-pick.mp4) | [Draft](expo-model-draft.png) |
| Submit | [Sheet](orca-model-sheet.png) · [Recording](orca-submit.mp4) | [Sheet](expo-model-sheet.png) · [Recording](expo-submit.mp4) |

The screenshots are 1320 × 2868. The sheet begins at y=1674 in Orca and y=1683 in Expo. Across the full width from the higher edge to the bottom, 67,185 of 1,576,080 pixels differ. The official session showed Default and Full access. The T3 fixture held gpt-5 and Ask. An authenticated read of the bounded thread before and after the Expo and browser journeys returned the same model and the same two message IDs, roles and text.

The [Orca runbook](orca-model.lua) starts in that chat with an empty focused composer, its keyboard visible and the host command catalog loaded. The [Expo runbook](expo-model.lua) starts on the paired Feed with Orca parity as its first desktop and native-fixture in the recorded list position. Both use the iOS harness on this simulator. The [browser runbook](web-model.lua) starts in the opened thread and uses accessible names. [Dark browser capture](web-model-sheet.png) · [Browser recording](web.mp4).

The Commands lists still differ. The reference offers /effort, /clear, /compact and /resume as well as /model. The RCA fixture supplied the model command in this run.
