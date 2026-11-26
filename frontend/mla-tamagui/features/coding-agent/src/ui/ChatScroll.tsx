import { useChatText } from '@lib/orca-design-system';
import { useRef, type ReactNode } from 'react';
import {
  ScrollView,
  type GestureResponderEvent,
  type ScrollViewInstance,
} from 'react-native';

export function ChatScroll({ children }: { children: ReactNode }) {
  const { value, update } = useChatText();
  const pinch = useRef<{ distance: number; size: number; last: number } | null>(
    null
  );
  function trackTouches(event: GestureResponderEvent) {
    const touches = event.nativeEvent.touches;
    if (touches.length !== 2) {
      pinch.current = null;
      return;
    }
    const [a, b] = touches;
    const distance = Math.hypot(a.pageX - b.pageX, a.pageY - b.pageY);
    if (!distance) return;
    if (!pinch.current) {
      pinch.current = { distance, size: value.size, last: value.size };
      return;
    }
    const size = Math.min(
      24,
      Math.max(
        11,
        Math.round((pinch.current.size * distance) / pinch.current.distance)
      )
    );
    if (size !== pinch.current.last) {
      pinch.current.last = size;
      update({ size });
    }
  }
  const scroll = useRef<ScrollViewInstance>(null);
  const follow = useRef(true);
  const anchor = () => {
    if (follow.current) scroll.current?.scrollToEnd({ animated: false });
  };
  return (
    <ScrollView
      ref={scroll}
      onTouchStart={trackTouches}
      onTouchMove={trackTouches}
      onTouchEnd={trackTouches}
      onTouchCancel={() => {
        pinch.current = null;
      }}
      style={{ flex: 1, minHeight: 0 }}
      contentContainerStyle={{ paddingVertical: 8, flexGrow: 1 }}
      keyboardShouldPersistTaps="handled"
      onLayout={anchor}
      onContentSizeChange={anchor}
      scrollEventThrottle={16}
      onScroll={({
        nativeEvent: { contentOffset, contentSize, layoutMeasurement },
      }) => {
        follow.current =
          contentSize.height - contentOffset.y - layoutMeasurement.height <= 32;
      }}
    >
      {children}
    </ScrollView>
  );
}
