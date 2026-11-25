import { Logger } from '@lib/logging';
import { useEffect, useState } from 'react';
import type { GestureResponderEvent, LayoutChangeEvent, ViewStyle } from 'react-native';
import Animated, { useAnimatedStyle, useSharedValue, withTiming } from 'react-native-reanimated';

import { useReducedMotion } from '../lib/reducedMotion/useReducedMotion';
import { useTheme, XStack } from '../primitives';

import { Text } from './text';

export type SegmentedControlOption = {
  value: string;
  label: string;
};

export type SegmentedControlProps = {
  options: SegmentedControlOption[];
  value: string;
  onChange: (value: string) => void;
  /** Suppress the automatic change log. */
  pressLog?: 'off';
  /** 'filled' (default) paints the chosen segment; 'thumb' slides one thumb under the chosen label and gives the control tab roles. */
  variant?: 'filled' | 'thumb';
  tone?: 'accent' | 'neutral';
  density?: 'default' | 'compact';
  trackStyle?: ViewStyle;
  thumbStyle?: ViewStyle;
  testID?: string;
};

const TRACK_PADDING = 4;
const SEGMENT_GAP = 4;
const THUMB_MS = 180;

/** Where the thumb sits for the chosen segment: its width and how far it is moved, in a track of `trackWidth` with `count` segments. */
export function thumbGeometry(index: number, count: number, trackWidth: number, padding = TRACK_PADDING): { width: number; offset: number } {
  if (count <= 0 || trackWidth <= 0) return { width: 0, offset: 0 };
  const width = Math.max(0, (trackWidth - 2 * padding - SEGMENT_GAP * (count - 1)) / count);
  return { width, offset: Math.max(0, index) * (width + SEGMENT_GAP) };
}

function Thumb({ index, count, trackWidth, padding, tone, thumbStyle }: { index: number; count: number; trackWidth: number; padding: number; thumbStyle?: ViewStyle; tone: 'accent' | 'neutral' }) {
  const theme = useTheme();
  const reduced = useReducedMotion();
  const { width, offset } = thumbGeometry(index, count, trackWidth, padding);
  const x = useSharedValue(offset);
  useEffect(() => {
    x.value = reduced ? offset : withTiming(offset, { duration: THUMB_MS });
  }, [x, offset, reduced]);
  const style = useAnimatedStyle(() => ({ transform: [{ translateX: x.value }] }));
  return (
    <Animated.View
      testID="SegmentedThumb"
      pointerEvents="none"
      style={[{ position: 'absolute', top: padding, bottom: padding, left: padding, width, borderRadius: 10, backgroundColor: (tone === 'neutral' ? theme.color7?.val : theme.color9?.val) ?? '#444' }, thumbStyle, style]}
    />
  );
}

/**
 * Single-select segmented control -- one XStack rail, one pressable segment
 * per option. The selected segment uses the theme accent so it reads as
 * "the chosen one" against the neutral rail; the unselected segments stay
 * transparent until pressed.
 */
export function SegmentedControl({ options, value, onChange, pressLog, variant = 'filled', tone = 'accent', density = 'default', trackStyle, thumbStyle, testID }: SegmentedControlProps) {
  const thumb = variant === 'thumb';
  const neutral = tone === 'neutral';
  const compact = density === 'compact';
  const padding = compact ? 2 : TRACK_PADDING;
  const [trackWidth, setTrackWidth] = useState(0);
  const selectedIndex = options.findIndex((option) => option.value === value);
  return (
    <XStack
      testID={testID}
      style={trackStyle}
      bg={neutral ? "$color4" : "$color3"}
      rounded={compact ? "$10" : "$4"}
      p={compact ? padding : "$1"}
      gap="$1"
      {...(thumb ? { role: 'tablist' as const, onLayout: (event: LayoutChangeEvent) => setTrackWidth(event.nativeEvent.layout.width) } : {})}
    >
      {thumb ? <Thumb index={selectedIndex} count={options.length} trackWidth={trackWidth} padding={padding} tone={tone} thumbStyle={thumbStyle} /> : null}
      {options.map((option) => {
        const selected = option.value === value;
        const handlePress = (_event: GestureResponderEvent) => {
          if (option.value === value) return;
          if (pressLog !== 'off') {
            Logger.info('ds.segmentedControl.change', {
              testID,
              value: option.value,
            });
          }
          onChange(option.value);
        };
        return (
          <XStack
            key={option.value}
            testID={testID ? `${testID}-${option.value}` : undefined}
            flex={1}
            items="center"
            justify="center"
            py={compact ? "$1" : "$2"}
            rounded="$3"
            bg={selected && !thumb ? (neutral ? '$color7' : '$color9') : 'transparent'}
            pressStyle={selected ? undefined : { opacity: 0.7 }}
            cursor="pointer"
            onPress={handlePress}
            {...(thumb ? { role: 'tab' as const, 'aria-selected': selected } : {})}
          >
            <Text variant={compact ? "small" : undefined} color={selected && !neutral ? '$color1' : '$color'}>{option.label}</Text>
          </XStack>
        );
      })}
    </XStack>
  );
}
