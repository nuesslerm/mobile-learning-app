# iOS simulator visual checks

Use the iPhone 17 Pro Max on iOS 27.0 in portrait with Expo Go and the
existing Mobile MCP driver (`idb` on PATH). The approval picker baseline
is a native device capture at 1320 × 2868.

1. Use Buoy to navigate to `/settings/appearance` and select Dark.
2. Navigate to `/showcase?item=glass-picker` and tap `ShowcaseOpenApprovals`.
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
