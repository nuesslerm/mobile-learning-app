import { useEffect, useRef, useState } from 'react';
import { Animated, Easing, StyleSheet, Text, View, type TextStyle } from 'react-native';

import { useReducedMotion } from '../lib/reducedMotion/useReducedMotion';

const gap = 24;

export function MarqueeLabel({ label, textStyle, pauseMs = 1800 }: { label: string; textStyle?: TextStyle; pauseMs?: number }) {
  const reducedMotion = useReducedMotion();
  const offset = useRef(new Animated.Value(0)).current;
  const [available, setAvailable] = useState(0);
  const [measured, setMeasured] = useState({ label: '', width: 0 });
  const width = measured.label === label ? measured.width : 0;
  const scroll = !reducedMotion && available > 0 && width > available;

  useEffect(() => {
    offset.setValue(0);
    if (!scroll) return;
    const distance = width + gap;
    const animation = Animated.loop(Animated.sequence([
      Animated.delay(pauseMs),
      Animated.timing(offset, { toValue: -distance, duration: Math.round(distance * 25), easing: Easing.linear, useNativeDriver: true, isInteraction: false }),
    ]));
    animation.start();
    return () => { animation.stop(); };
  }, [offset, pauseMs, scroll, width]);

  return (
    <View style={styles.clip} onLayout={({ nativeEvent }) => setAvailable(nativeEvent.layout.width)}>
      <Text numberOfLines={1} accessibilityElementsHidden importantForAccessibility="no"
        style={[textStyle, styles.measure]} onTextLayout={({ nativeEvent }) => {
          const firstLine = nativeEvent.lines[0];
          if (firstLine) setMeasured({ label, width: Math.ceil(firstLine.width) });
        }}>{label}</Text>
      {scroll ? (
        <Animated.View accessibilityElementsHidden importantForAccessibility="no-hide-descendants"
          style={[styles.track, { width: 2 * (width + gap), transform: [{ translateX: offset }] }]}>
          <Text numberOfLines={1} style={[textStyle, { width: width + 1 }]}>{label}</Text>
          <Text numberOfLines={1} style={[textStyle, { width: width + 1 }]}>{label}</Text>
        </Animated.View>
      ) : <Text numberOfLines={1} style={textStyle}>{label}</Text>}
    </View>
  );
}

const styles = StyleSheet.create({
  clip: { overflow: 'hidden' },
  measure: { position: 'absolute', opacity: 0, width: 1800 },
  track: { flexDirection: 'row', gap: gap - 1 },
});
