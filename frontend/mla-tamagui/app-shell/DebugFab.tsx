import {
  IconButton,
  KEYBOARD_TOOLBAR_Z,
  useOpenSheetStore,
} from '@lib/tamagui-design-system';
import { Bug } from '@lib/tamagui-design-system/icons';
import { useState } from 'react';
import { useWindowDimensions } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  runOnJS,
  useAnimatedStyle,
  useSharedValue,
} from 'react-native-reanimated';
import { YStack } from '@lib/tamagui-design-system/primitives';

import { DebugLogSheet } from './DebugLogSheet';

// Anchor offsets from screen-shell.tsx.web-style left/bottom insets below
// (`l="$4" b="$6"`), duplicated here as raw pixels because the pan handler's
// clamp math runs in a worklet and can't read Tamagui tokens. Close enough
// -- this only needs to keep the FAB from being dragged fully off-screen,
// not to pixel-match the token values.
//
// LEFT-anchored, not right: the composer's Send button lives in the bottom-
// right corner of the viewport on /coding-agent/<threadId> (it's the trailing
// slot of ComposerBase, which sits flush at the bottom of the thread column).
// Docking the FAB on the same corner overlaps it; `elementFromPoint` at the
// Send button's centre then returns the FAB, a real tap opens the log sheet,
// and the `thread.turn.start` dispatch never fires. The left anchor is clear
// of the Send button (and of the chip row above the input, which is the only
// other thing that lives in the composer's bottom half).
const ANCHOR_LEFT_PX = 16;
const ANCHOR_BOTTOM_PX = 24;
const FAB_SIZE_PX = 56;
// How far past the anchor point the FAB is allowed to travel in the
// direction that would push it further into its own corner -- a little
// slack rather than zero, so it doesn't feel like it's hit a hard wall the
// instant you start dragging back toward "home".
const SLACK_PX = 8;

/**
 * Opens the log sheet on a tap; draggable on a pan so it can be moved off
 * whatever it's currently covering.
 *
 * Position is session-only (a plain shared value, not persisted) --
 * deliberately: the ask this solves is "let me drag it out of the way while
 * testing a specific screen right now", not "remember where I like it
 * forever". A reload resets it to the default corner, which is the same
 * "yields to what's actually on screen" behavior as the sheet-open case
 * below, just triggered by a reload instead of automatically.
 *
 * `Gesture.Race(tap, pan)`: a plain tap (no meaningful movement) opens the
 * sheet; a drag of any real distance moves the FAB instead. Racing them
 * (rather than composing pan+onEnd-distance-check by hand) is the standard
 * gesture-handler pattern for exactly this "either a tap or a drag, never
 * both" case.
 */
export function DebugFab() {
  const [open, setOpen] = useState(false);
  // The sheet pulls in bottom-sheet and reanimated, so it is not built until the first tap.
  const [sheetMounted, setSheetMounted] = useState(false);
  const openSheet = () => {
    setSheetMounted(true);
    setOpen(true);
  };
  const sheetOpen = useOpenSheetStore((state) => state.count > 0);
  const { width, height } = useWindowDimensions();

  const translateX = useSharedValue(0);
  const translateY = useSharedValue(0);
  const startX = useSharedValue(0);
  const startY = useSharedValue(0);

  // Left-anchored: the natural position is at `ANCHOR_LEFT_PX`. Drag right
  // up to where the FAB's right edge meets the viewport edge; drag left only
  // `SLACK_PX` past the anchor, the same nudge the bottom-anchored version
  // gives when you push it back toward "home".
  const minX = -SLACK_PX;
  const maxX = width - ANCHOR_LEFT_PX - FAB_SIZE_PX;
  const minY = -(height - ANCHOR_BOTTOM_PX - FAB_SIZE_PX);
  const maxY = SLACK_PX;

  const pan = Gesture.Pan()
    .onStart(() => {
      startX.value = translateX.value;
      startY.value = translateY.value;
    })
    .onUpdate((event) => {
      translateX.value = Math.min(
        maxX,
        Math.max(minX, startX.value + event.translationX)
      );
      translateY.value = Math.min(
        maxY,
        Math.max(minY, startY.value + event.translationY)
      );
    });

  const tap = Gesture.Tap().onEnd(() => {
    // .onEnd runs as a UI-thread worklet; setOpen is a plain JS function
    // (a React state setter), so it must cross back via runOnJS -- calling
    // it directly silently does nothing (found live: the tap gesture fired,
    // pan never activated, and setOpen still never ran).
    runOnJS(openSheet)();
  });

  const gesture = Gesture.Race(pan, tap);

  const animatedStyle = useAnimatedStyle(() => ({
    transform: [
      { translateX: translateX.value },
      { translateY: translateY.value },
    ],
  }));

  const sheet = sheetMounted ? (
    <DebugLogSheet open={open} onOpenChange={setOpen} />
  ) : null;

  if (sheetOpen && !open) return sheet;

  return (
    <>
      {/* Deliberately BELOW the keyboard toolbar: when the rail is up it runs
          full-width across the bottom, and the FAB would otherwise cover its
          right-hand keys (Rename, Done). Dragging still works over it -- the
          FAB itself renders above the rail's own controls only where it sits. */}
      <GestureDetector gesture={gesture}>
        <Animated.View
          style={[
            {
              position: 'absolute',
              left: ANCHOR_LEFT_PX,
              bottom: ANCHOR_BOTTOM_PX,
              zIndex: KEYBOARD_TOOLBAR_Z - 1,
            },
            animatedStyle,
          ]}
          testID="DebugFabWrap"
        >
          <YStack pointerEvents="none">
            <IconButton
              icon={Bug}
              variant="secondary"
              size="$5"
              testID="DebugFab"
            />
          </YStack>
        </Animated.View>
      </GestureDetector>
      {sheet}
    </>
  );
}
