import { Logger } from '@lib/logging';
import { Icon, View } from '@lib/tamagui-design-system';
import { GripVertical } from '@lib/tamagui-design-system/icons';
import type { LucideIcon } from 'lucide-react-native';
import * as React from 'react';
import { useEffect, type ReactNode } from 'react';
import { useWindowDimensions, type ViewStyle } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  runOnJS,
  useAnimatedReaction,
  useAnimatedStyle,
  useFrameCallback,
  useSharedValue,
  withSpring,
  type SharedValue,
} from 'react-native-reanimated';

/**
 * Generic fixed-row-height drag-to-reorder list with edge auto-scroll.
 *
 * Built from gesture-handler + Reanimated (the same primitives as
 * features/leetcode/ui/WordBank/DraggableTile) — deliberately NO `measure()`
 * and NO FlatList, so it's immune to the NativeWind cssInterop / Fabric crash
 * that broke the FlatList- and measure()-based drag libraries in Expo Go.
 *
 * Rows are absolutely positioned by `index * rowHeight`; the active row follows
 * the finger and the others spring to make room. While dragging, if the finger
 * is within `EDGE_*` px of the top/bottom of the screen, a per-row frame callback
 * scrolls via the caller's imperative `scrollToY` (a plain ScrollView ref —
 * NOT a Reanimated animated ref, which NativeWind's cssInterop wrapper fails to
 * initialise here) and the row keeps tracking the finger across the scroll. Edge
 * detection uses the finger's absolute screen Y, so it can't silently no-op.
 */
type Positions = Record<string, number>;

export type ReorderActivation = 'long-press' | 'immediate';

type ReorderableListProps<T> = {
  data: T[];
  keyExtractor: (item: T) => string;
  /** Render the row; place `handle` wherever the drag grip should live. */
  renderItem: (item: T, handle: ReactNode) => ReactNode;
  onReorder: (next: T[]) => void;
  rowHeight: number;
  /** Live scroll offset (from the ScrollView's JS scroll callback). Leave both scroll props out for a list that has no scroll view around it: it then does no edge auto-scroll. */
  scrollOffset?: SharedValue<number>;
  /** Imperatively scroll the surrounding ScrollView (plain ref `.scrollTo`). */
  scrollToY?: (y: number) => void;
  /** 'long-press' (the default) picks a row up after 250 ms on the handle; 'immediate' at the first movement, for a list that does not scroll under the finger. */
  activation?: ReorderActivation;
  /** The glyph on the handle (the grip by default). */
  handleIcon?: LucideIcon;
};

// App #1 pulls this from its theme's motion config. One spring config used
// twice in one file is not a design token; the two call sites are right here.
const springGentle = { damping: 25, stiffness: 150, mass: 1 } as const;

const HANDLE_STYLE: ViewStyle = {
  width: 44,
  height: '100%',
  alignItems: 'center',
  justifyContent: 'center',
};

// How close (px) the finger must be to the top/bottom of the screen to start
// auto-scrolling, and how many px/frame to scroll. `EDGE_TOP` is larger to clear
// the header.
const EDGE_TOP = 150;
const EDGE_BOTTOM = 140;
const SCROLL_SPEED = 14;

function clamp(value: number, min: number, max: number): number {
  'worklet';
  return Math.max(min, Math.min(max, value));
}

/** Shift the index map so `draggedId` moves to `newIndex`, others closing the gap. */
function reorderPositions(
  positions: Positions,
  draggedId: string,
  newIndex: number
): Positions {
  'worklet';
  const oldIndex = positions[draggedId];
  if (newIndex === oldIndex) return positions;
  // Built with Object.keys + an indexed loop (no spread / for-in — neither is
  // supported inside a Reanimated worklet).
  const keys = Object.keys(positions);
  const next: Positions = {};
  for (let i = 0; i < keys.length; i++) {
    const key = keys[i];
    const p = positions[key];
    if (key === draggedId) {
      next[key] = newIndex;
    } else if (oldIndex < newIndex) {
      next[key] = p > oldIndex && p <= newIndex ? p - 1 : p;
    } else {
      next[key] = p >= newIndex && p < oldIndex ? p + 1 : p;
    }
  }
  return next;
}

function buildPositions<T>(
  data: T[],
  keyExtractor: (item: T) => string
): Positions {
  return Object.fromEntries(data.map((item, i) => [keyExtractor(item), i]));
}

function Row<T>({
  item,
  id,
  positions,
  rowHeight,
  count,
  windowHeight,
  scrollOffset,
  scrollToY,
  activation,
  handleIcon,
  onCommit,
  renderItem,
}: {
  item: T;
  id: string;
  positions: SharedValue<Positions>;
  rowHeight: number;
  count: SharedValue<number>;
  windowHeight: number;
  scrollOffset: SharedValue<number>;
  scrollToY: ((y: number) => void) | undefined;
  activation: ReorderActivation;
  handleIcon: LucideIcon;
  onCommit: () => void;
  renderItem: (item: T, handle: ReactNode) => ReactNode;
}) {
  const isActive = useSharedValue(false);
  const top = useSharedValue(positions.value[id] * rowHeight);
  const startTop = useSharedValue(0);
  const startScroll = useSharedValue(0);
  const translation = useSharedValue(0);
  const pointerY = useSharedValue(0);

  // When the order changes (by this or another row's drag), settle to the slot.
  useAnimatedReaction(
    () => positions.value[id],
    (pos) => {
      if (!isActive.value) {
        top.value = withSpring(pos * rowHeight, springGentle);
      }
    },
    [rowHeight]
  );

  // Recompute this row's content position + hovered index. Called from the
  // gesture (finger moved) AND the frame callback (so the row keeps following
  // the finger while auto-scrolling with the finger held still).
  const updatePosition = () => {
    'worklet';
    top.value =
      startTop.value +
      translation.value +
      (scrollOffset.value - startScroll.value);
    const newIndex = clamp(
      Math.round(top.value / rowHeight),
      0,
      count.value - 1
    );
    if (newIndex !== positions.value[id]) {
      positions.value = reorderPositions(positions.value, id, newIndex);
    }
  };

  // Edge auto-scroll. Runs every frame but is a no-op unless this row is active.
  useFrameCallback(() => {
    if (!isActive.value || scrollToY === undefined) return;
    if (pointerY.value < EDGE_TOP && scrollOffset.value > 0) {
      runOnJS(scrollToY)(scrollOffset.value - SCROLL_SPEED);
      updatePosition();
    } else if (pointerY.value > windowHeight - EDGE_BOTTOM) {
      runOnJS(scrollToY)(scrollOffset.value + SCROLL_SPEED);
      updatePosition();
    }
  });

  const basePan = Gesture.Pan();
  const pan = (activation === 'long-press' ? basePan.activateAfterLongPress(250) : basePan)
    .onStart((e) => {
      isActive.value = true;
      startTop.value = positions.value[id] * rowHeight;
      startScroll.value = scrollOffset.value;
      translation.value = 0;
      pointerY.value = e.absoluteY;
    })
    .onChange((e) => {
      translation.value = e.translationY;
      pointerY.value = e.absoluteY;
      updatePosition();
    })
    .onEnd(() => {
      top.value = withSpring(positions.value[id] * rowHeight, springGentle);
    })
    .onFinalize(() => {
      if (!isActive.value) return;
      isActive.value = false;
      runOnJS(onCommit)();
    });

  const style = useAnimatedStyle(
    () => ({
      position: 'absolute',
      left: 0,
      right: 0,
      top: top.value,
      height: rowHeight,
      zIndex: isActive.value ? 10 : 0,
      elevation: isActive.value ? 8 : 0,
    }),
    [rowHeight]
  );

  const handle = (
    <GestureDetector gesture={pan}>
      <View style={HANDLE_STYLE} testID="ReorderableListHandle">
        <Icon as={handleIcon} size={22} color="$color11" />
      </View>
    </GestureDetector>
  );

  return (
    <Animated.View style={style} testID="ReorderableList-AnimatedView1">
      {renderItem(item, handle)}
    </Animated.View>
  );
}

export function ReorderableList<T>({
  data,
  keyExtractor,
  renderItem,
  onReorder,
  rowHeight,
  scrollOffset,
  scrollToY,
  activation = 'long-press',
  handleIcon = GripVertical,
}: ReorderableListProps<T>) {
  const { height: windowHeight } = useWindowDimensions();
  const ownScrollOffset = useSharedValue(0);
  const positions = useSharedValue<Positions>(
    buildPositions(data, keyExtractor)
  );
  const count = useSharedValue(data.length);
  // Re-sync the index map + count when the data set changes (add/remove/reset).
  // Prop-reaction: shared values can't be written during render (Reanimated
  // strict mode); sync them from an effect keyed on a stable data signature.
  const signature = React.useMemo(
    () => data.map(keyExtractor).join('|'),
    [data, keyExtractor]
  );
  useEffect(() => {
    Logger.info('reorder.data_resynced', { count: data.length, signature });
    positions.value = buildPositions(data, keyExtractor);
    count.value = data.length;
  }, [signature, data, keyExtractor, positions, count]);

  const commit = () => {
    const pos = positions.value;
    const next = [...data].sort(
      (a, b) => pos[keyExtractor(a)] - pos[keyExtractor(b)]
    );
    onReorder(next);
  };

  return (
    <View height={data.length * rowHeight} testID="ReorderableList">
      {data.map((item) => {
        const id = keyExtractor(item);
        return (
          <Row
            key={id}
            item={item}
            id={id}
            positions={positions}
            rowHeight={rowHeight}
            count={count}
            windowHeight={windowHeight}
            scrollOffset={scrollOffset ?? ownScrollOffset}
            scrollToY={scrollToY}
            activation={activation}
            handleIcon={handleIcon}
            onCommit={commit}
            renderItem={renderItem}
          />
        );
      })}
    </View>
  );
}
