# @storybook/react-native-ui Patch

The Storybook addons bottom-sheet (`MobileAddonsPanel`) had no backdrop component. Drags on the panel
content leaked through to the story's scroll view behind it, causing both to scroll simultaneously.
There was also no tap-outside-to-dismiss affordance.

The patch adds a `BottomSheetBackdrop` with `pressBehavior="close"` and `disappearsOnIndex={-1}`,
which blocks touches from reaching the story layer while the panel is open and dismisses the panel on
tap-outside. The panel body is also switched to `BottomSheetScrollView` so internal scroll is handled
by the sheet's own gesture recogniser rather than the underlying story scroll.
