# Add to message on the RCA iPhone

Installed Orca opened the saved photo conversation in the native-fixture checkout on the paired rca iPhone 18 Pro Max. Tap the plus button to open [Add to message](orca-add-to-message.png). The sheet offered Workspace file, Skills and Commands.

- [Workspace file](orca-workspace-file.png) inserted an @ into the empty draft.
- [Skills](orca-skills.png) inserted a slash. On this run the available rows were /model, /effort and /resume under Commands. Orca falls back to the available rows when the selected category is empty.
- [Commands](orca-commands.png) opened a command list that included /model, /effort, /clear and /compact. This capture followed the Skills selection in the same draft. The host had also finished loading the transcript images and model.

[Watch the installed app open Skills](orca-skills.mp4). To repeat the [capture](open-skills.lua), start at the host list with the paired fixture in the same position, set the output directory and run the iOS harness.

```sh
WF_IOS_APP_ID=com.stably.orca.mobile wf ios test open-skills.lua --device "$RCA_UDID" \
  --env "out=$CAPTURE_DIR" --test-output-dir "$RECORDING_DIR"
```

## Expo Go and the exported web app

The [iOS update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/07ee9d33-b060-4f54-a4f2-86f7e38abf58) opened the same saved chat on the same iPhone. The [Expo sheet](expo-add-to-message.png) starts at y=2209 and the [Orca sheet](orca-add-to-message.png) starts at y=2208. Across the full-width region from the higher edge through the bottom, 10,542 of 871,200 RGBA pixels differ (1.21%). The complete frames differ on 1,529,248 of 3,785,760 pixels; the two transcripts were at different positions with different attachment loading states. [Expo recording](expo-menu.mp4).

The fixture checkout contained a temporary file named Composer demo.md. Tapping Workspace file opened the [file list](expo-workspace-file.png). Picking its row inserted the [quoted file reference](expo-file-picked.png). Tapping Skills opened [provider suggestions](expo-skills.png). Selecting /test-report kept the file reference after the inserted skill in the [unsent draft](expo-skill-picked.png). [Skill selection recording](expo-skill.mp4).

To repeat the [iOS file selection](pick-expo-file.lua), open the fixture chat from the paired host, with the keyboard closed and Composer demo.md available at the checkout root. The [web runbook](pick-web-file.lua) uses accessible labels. The [browser recording](web-file.mp4) shows the same file selection through the exported app and the local T3 gateway. The browser captured a light transcript surface with low-contrast text while its composer used dark colors. The iOS capture uses the light palette throughout.

The gateway integration test creates two sibling projects under one isolated executor, each with a distinct README and skill. Both T3 operations returned the entries for the requested subdirectory. The installed reference also offers client commands such as /model and /effort. Matching those commands and Orca’s leading skill chip remains open.

## Browser appearance after cold load

The [earlier draft](web-draft.png) showed a light header and transcript with a dark composer. The static export renders the light palette. The web client now hydrates that palette and then applies the browser preference across the screen.

Both checks opened the saved photo chat directly in paired Chromium at the iPhone viewport. The [runbook](check-web-theme.lua) checked the top header, transcript margin and composer after the transcript appeared. The samples use viewport percentages (48, 2), (97, 47) and (70, 93), respectively.

| Browser preference | Header | Transcript | Composer | Evidence |
| --- | --- | --- | --- | --- |
| Dark | #171717 | #0a0a0a | #1f1f1f | [Screenshot](web-dark.png) · [Recording](web-dark.mp4) |
| Light | #fafafa | #ffffff | #ffffff | [Screenshot](web-light.png) · [Recording](web-light.mp4) |
