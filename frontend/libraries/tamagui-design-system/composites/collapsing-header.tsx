import type { ReactNode } from 'react';
import { useCallback, useState } from 'react';
import type {
  NativeScrollEvent,
  NativeSyntheticEvent,
  StyleProp,
  ViewStyle,
} from 'react-native';
// React Native's ScrollView, not Tamagui's: Tamagui types
// `contentContainerStyle` as style PROPS (`pt`, `px`), so a plain RN style
// object from a caller does not typecheck against it.
import { ScrollView } from 'react-native';
import Animated, {
  useAnimatedStyle,
  useSharedValue,
} from 'react-native-reanimated';

import { YStack } from '../primitives';

export type CollapsingHeaderProps = {
  /** The tall hero. Scrolls away with the content. */
  expanded: ReactNode;
  /** The short bar, faded in as the hero scrolls off. */
  compact: ReactNode;
  children: ReactNode;
  /** Scroll distance over which the crossfade completes. */
  collapseDistance?: number;
  contentContainerStyle?: StyleProp<ViewStyle>;
  testID: string;
  scrollViewTestID?: string;
};

/**
 * Two-state collapsing header: a tall hero that scrolls away into a compact bar.
 *
 * A deliberately small answer to the one component of app #1 worth keeping the
 * shape of. App #1's is a sticky-geometry engine with a checkpoint marker
 * registry, where the compact bar relabels itself from whichever section is
 * under it. That earns its complexity there; here the bar is fixed, so scroll
 * offset alone drives it.
 *
 * The hero is the scroll view's FIRST CHILD rather than an overlay with
 * matching top padding. The overlay version looked identical and was broken:
 * the first list row sat underneath it and could not be tapped at all, because
 * an absolutely-positioned sibling above it takes the pointer event. Only the
 * compact bar overlays, and only while it is the one on screen.
 *
 * The repo's scroll guards are why this uses a plain
 * ScrollView with a plain JS `onScroll` writing to a shared value, rather than
 * `useAnimatedScrollHandler` or `useScrollViewOffset` (both banned, both
 * enforced), and why only the inner Animated.View carries animated props --
 * never a [static, animated] style array.
 */
export function CollapsingHeader({
  expanded,
  compact,
  children,
  collapseDistance = 120,
  contentContainerStyle,
  testID,
  scrollViewTestID,
}: CollapsingHeaderProps) {
  const progress = useSharedValue(0);
  // Opacity alone would leave the invisible bar swallowing taps meant for the
  // content beneath it. `collapsed` flips once per threshold crossing, and it
  // is the only thing `pointerEvents` can key off since a shared value is not
  // readable from the render pass.
  const [collapsed, setCollapsed] = useState(false);

  const onScroll = useCallback(
    (event: NativeSyntheticEvent<NativeScrollEvent>) => {
      const y = event.nativeEvent.contentOffset.y;
      progress.value = Math.min(1, Math.max(0, y / collapseDistance));
      setCollapsed(y > collapseDistance / 2);
    },
    [collapseDistance, progress]
  );

  const barStyle = useAnimatedStyle(() => ({ opacity: progress.value }));

  return (
    <YStack flex={1} testID={testID}>
      <ScrollView
        testID={scrollViewTestID}
        scrollEventThrottle={16}
        onScroll={onScroll}
        contentContainerStyle={contentContainerStyle}
      >
        {expanded}
        {children}
      </ScrollView>

      <YStack
        position="absolute"
        t={0}
        l={0}
        r={0}
        pointerEvents={collapsed ? 'auto' : 'none'}
      >
        <Animated.View style={barStyle}>{compact}</Animated.View>
      </YStack>
    </YStack>
  );
}
