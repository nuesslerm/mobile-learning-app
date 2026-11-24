import { useReducedMotion } from '@lib/tamagui-design-system';
import { useTheme } from '@lib/tamagui-design-system/primitives';
import React from 'react';
import { View } from 'react-native';
import Animated, { cancelAnimation, useAnimatedStyle, useSharedValue, withRepeat, withTiming } from 'react-native-reanimated';

const WIDTH = 96;
const HEIGHT = 60;
const FROZEN_AT = 0.6;
const GRID_LINES = [0.25, 0.5, 0.75];
const GLOW = [0.3, 0.2, 0.12, 0.06];

/** A small window with a grid, two pulsing dots and a scan line sweeping down it; with reduced motion the line stops at 60% and the dots stay lit. */
export function ScanGlyph() {
  const theme = useTheme();
  const reduced = useReducedMotion();
  const amber = theme.warning?.val ?? '#f5a623';
  const frame = theme.borderColor?.val ?? '#888';
  const sweep = useSharedValue(0);
  const pulse = useSharedValue(1);

  React.useEffect(() => {
    cancelAnimation(sweep);
    cancelAnimation(pulse);
    if (reduced) return;
    sweep.value = withRepeat(withTiming(1, { duration: 1400 }), -1, false);
    pulse.value = withRepeat(withTiming(0.3, { duration: 700 }), -1, true);
  }, [reduced, sweep, pulse]);

  const scanStyle = useAnimatedStyle(() => ({ transform: [{ translateY: (reduced ? FROZEN_AT : sweep.value) * HEIGHT }] }));
  const dotStyle = useAnimatedStyle(() => ({ opacity: reduced ? 1 : pulse.value }));

  return (
    <View testID="ConsoleScanGlyph" style={{ width: WIDTH, height: HEIGHT + 14, alignItems: 'center' }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 4, width: WIDTH, height: 14 }}>
        {[0, 1, 2].map((dot) => (
          <View key={dot} style={{ width: 5, height: 5, borderRadius: 3, backgroundColor: frame }} />
        ))}
        <View style={{ flex: 1, height: 3, borderRadius: 2, backgroundColor: frame, opacity: 0.6 }} />
      </View>
      <View testID="ConsoleScanWindow" style={{ width: WIDTH, height: HEIGHT, borderWidth: 1, borderColor: frame, borderRadius: 6, overflow: 'hidden' }}>
        {GRID_LINES.map((at) => (
          <React.Fragment key={at}>
            <View style={{ position: 'absolute', left: 0, right: 0, top: at * HEIGHT, height: 1, backgroundColor: amber, opacity: 0.35 }} />
            <View style={{ position: 'absolute', top: 0, bottom: 0, left: at * WIDTH, width: 1, backgroundColor: amber, opacity: 0.35 }} />
          </React.Fragment>
        ))}
        <View style={{ position: 'absolute', right: 8, bottom: 8, flexDirection: 'row', gap: 6 }}>
          {[0, 1].map((dot) => (
            <Animated.View key={dot} testID={`ConsoleScanDot-${dot}`} style={[{ width: 6, height: 6, borderRadius: 3, backgroundColor: amber }, dotStyle]} />
          ))}
        </View>
        <Animated.View testID="ConsoleScan" style={[{ position: 'absolute', left: 0, right: 0, top: -2, height: 2 }, scanStyle]}>
          <View style={{ height: 2, backgroundColor: amber }} />
          {GLOW.map((opacity, index) => (
            <View key={index} style={{ position: 'absolute', left: 0, right: 0, top: -(index + 1) * 3, height: 3, backgroundColor: amber, opacity }} />
          ))}
        </Animated.View>
      </View>
    </View>
  );
}
