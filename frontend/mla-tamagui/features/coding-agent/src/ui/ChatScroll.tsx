import { useRef, type ReactNode } from 'react';
import { ScrollView, type ScrollViewInstance } from 'react-native';

export function ChatScroll({ children }: { children: ReactNode }) {
  const scroll = useRef<ScrollViewInstance>(null);
  const follow = useRef(true);
  const anchor = () => {
    if (follow.current) scroll.current?.scrollToEnd({ animated: false });
  };
  return (
    <ScrollView
      ref={scroll}
      style={{ flex: 1, minHeight: 0 }}
      contentContainerStyle={{paddingVertical: 8, flexGrow: 1}}
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
