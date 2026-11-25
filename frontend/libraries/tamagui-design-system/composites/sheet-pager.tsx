import { useRef, useState, type ComponentRef, type ReactNode } from 'react';
import { Pressable, ScrollView, StyleSheet, View, useWindowDimensions } from 'react-native';

import { useCompactColors } from './compact-navigation';

type SheetPagerPage = { id: string; label: string; content: ReactNode };

export function SheetPager({ pages, page, onPageChange }: {
  pages: readonly SheetPagerPage[];
  page: number;
  onPageChange: (index: number) => void;
}) {
  const colors = useCompactColors();
  const pager = useRef<ComponentRef<typeof ScrollView>>(null);
  const { width } = useWindowDimensions();
  const [pageWidth, setPageWidth] = useState(width);

  return (
    <View style={styles.pager} onLayout={({ nativeEvent }) => {
      if (nativeEvent.layout.width > 0) setPageWidth(nativeEvent.layout.width);
    }}>
      <ScrollView
        ref={pager}
        horizontal
        pagingEnabled
        showsHorizontalScrollIndicator={false}
        onMomentumScrollEnd={({ nativeEvent }) => {
          onPageChange(Math.max(0, Math.min(pages.length - 1, Math.round(nativeEvent.contentOffset.x / pageWidth))));
        }}
      >
        {pages.map(({ id, content }, index) => (
          <View key={id} style={{ width: pageWidth }} accessibilityElementsHidden={index !== page}
            importantForAccessibility={index === page ? 'auto' : 'no-hide-descendants'}>
            {content}
          </View>
        ))}
      </ScrollView>
      {pages.length > 1 && (
        <View style={styles.dots}>
          {pages.map(({ id, label }, index) => (
            <Pressable key={id} accessibilityRole="button" accessibilityLabel={label}
              accessibilityState={{ selected: index === page }} hitSlop={6} onPress={() => {
                pager.current?.scrollTo({ x: index * pageWidth, animated: true });
                onPageChange(index);
              }}
              style={[styles.dot, { backgroundColor: index === page ? colors.foreground : colors.muted, opacity: index === page ? 1 : 0.45 }]} />
          ))}
        </View>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  pager: { flex: 1, paddingBottom: 22 },
  dots: { flexDirection: 'row', gap: 10, justifyContent: 'center', alignItems: 'center' },
  dot: { width: 6, height: 6, borderRadius: 3 },
});
