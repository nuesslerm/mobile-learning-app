# iOS simulator visual checks

Use the iPhone 17 Pro Max on iOS 27.0 in portrait with Expo Go and the
existing Mobile MCP driver (`idb` on PATH). The approval picker baseline
is a native device capture at 1320 × 2868.

1. Use Buoy to navigate to `/settings/appearance` and select Dark.
2. Navigate to `/showcase?item=glass-picker&visualCapture=1` and tap `ShowcaseOpenApprovals`.
3. Check that **Approve for me** is selected and that the sheet has settled.
4. Capture and compare the current screen.

```sh
wf test ios-visual approval-picker-dark --device <simulator-udid> \
  --expect ShowcaseApproval-automatic --ignore 1140,200,1320,355
```

The command checks that the row exists in the simulator's accessibility tree
before capturing. It returns JSON with the image paths and changed-pixel
count. Pixelmatch compares at native resolution with the same tolerance as
the web and Android golden checks. The top 7% is ignored for the iOS status
bar. The approval picker command also masks the Expo Go gear at the top
right, which appears on some navigations. A mismatch exits with status 1 and writes actual, expected and diff
images. Update a reviewed baseline explicitly with `--update-goldens`.

Add `visualCapture=1` to the route query when using a development build.
It hides the Buoy launcher during capture while keeping Buoy connected for
navigation. Removing the query parameter restores the launcher. The approval
golden was captured this way, so it needs no launcher mask.

The [LeetCode reference recording frames](../../../../frontend/reference-material/leetcode/README.md)
are design targets. Native golden checks track changes from an accepted
application state.

## Compare against a design reference

Use the same screen state as the reference. The screenshot guard checks the
visible accessibility tree before saving the capture.

```sh
wf test ios-visual keyboard-symbols --device <simulator-udid> \
  --expect SymbolBarGroup-g3 \
  --reference frontend/reference-material/leetcode/16-keyboard-symbol-groups.png \
  --ignore 0,0,1284,174 --out .context/ios-reference
```

The run keeps the original simulator PNG and uses macOS `sips` to convert both
images to sRGB at the reference dimensions. The images must have a portrait
aspect ratio within 1% of each other. Ignore rectangles are measured in
reference pixels. The full viewport is compared unless you explicitly
pass a rectangle. The example excludes the status bar.

The output includes paths to the normalized capture and reference. A
pixelmatch difference over the usual 0.2% tolerance exits with status 1 and
writes a diff. This is a visual comparison with a tolerance. Each reference
comparison writes its own run folder, and the supplied image is read-only.

Add `--exact` to the reference comparison to require identical RGBA values
after normalization. Any unmasked pixel that differs produces a mismatch,
including antialiased edges and one-level channel changes. The result then
includes `"comparison": "exact"`. Omit all `--ignore` rectangles to compare
the full screen. With different viewport sizes, exact mode describes the
normalized images.

## Replay the editor selection round trip

Frames 23–25 of the LeetCode recording show the menu at the caret, Console
opening with Case 3 selected, and the keyboard returning to the caret.

1. Open Merge Two Sorted Lists with the JavaScript starter. Use 14 px text,
   visible whitespace, and the on-screen German–English keyboard. In Symbol
   Bar settings, move Group 2 below Group 3.
2. Open Console, select Case 3, and close the sheet. Place the caret after
   the last semicolon. Tap the caret again to open the native edit menu.
3. With the menu still open, tap Console. Wait for the sheet to settle.
   Verify that Case 3 is selected and the fields read `[]` and `[0]`.
4. Tap the sheet handle. Verify that the keyboard, caret, and reordered symbol
   bar return. Record the round trip to retain evidence of the transition.

```sh
wf test ios-visual selection-roundtrip-case-three --device <simulator-udid> \
  --expect ConsoleSheetHandle \
  --reference frontend/reference-material/leetcode/24-console-over-focused-editor.png \
  --ignore 0,0,1284,174 --out .context/ios-reference
```

Run the command in step 3. The accessibility guard establishes that the console is open.
Use the mobile accessibility tree to confirm that Case 3 is selected. iOS supplies the
edit menu and keyboard, so keep their OS version, language and viewport in
the evidence when comparing different devices.
