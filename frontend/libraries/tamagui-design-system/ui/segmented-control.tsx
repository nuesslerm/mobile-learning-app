import { Logger } from '@lib/logging';
import { useEffect, useState } from 'react';
import type { GestureResponderEvent, LayoutChangeEvent } from 'react-native';
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
  testID?: string;
};

const TRACK_PADDING = 4;
const SEGMENT_GAP = 4;
const THUMB_MS = 180;

/** Where the thumb sits for the chosen segment: its width and how far it is moved, in a track of `trackWidth` with `count` segments. */
export function thumbGeometry(index: number, count: number, trackWidth: number): { width: number; offset: number } {
  if (count <= 0 || trackWidth <= 0) return { width: 0, offset: 0 };
  const width = Math.max(0, (trackWidth - 2 * TRACK_PADDING - SEGMENT_GAP * (count - 1)) / count);
  return { width, offset: Math.max(0, index) * (width + SEGMENT_GAP) };
}

function Thumb({ index, count, trackWidth }: { index: number; count: number; trackWidth: number }) {
  const theme = useTheme();
  const reduced = useReducedMotion();
  const { width, offset } = thumbGeometry(index, count, trackWidth);
  const x = useSharedValue(offset);
  useEffect(() => {
    x.value = reduced ? offset : withTiming(offset, { duration: THUMB_MS });
  }, [x, offset, reduced]);
  const style = useAnimatedStyle(() => ({ transform: [{ translateX: x.value }] }));
  return (
    <Animated.View
      testID="SegmentedThumb"
      pointerEvents="none"
      style={[{ position: 'absolute', top: TRACK_PADDING, bottom: TRACK_PADDING, left: TRACK_PADDING, width, borderRadius: 10, backgroundColor: theme.color9?.val ?? '#444' }, style]}
    />
  );
}

/**
 * Single-select segmented control -- one XStack rail, one pressable segment
 * per option. The selected segment uses the theme accent so it reads as
 * "the chosen one" against the neutral rail; the unselected segments stay
 * transparent until pressed.
 */
export function SegmentedControl({ options, value, onChange, pressLog, variant = 'filled', testID }: SegmentedControlProps) {
  const thumb = variant === 'thumb';
  const [trackWidth, setTrackWidth] = useState(0);
  const selectedIndex = options.findIndex((option) => option.value === value);
  return (
    <XStack
      testID={testID}
      bg="$color3"
      rounded="$4"
      p="$1"
      gap="$1"
      {...(thumb ? { role: 'tablist' as const, onLayout: (event: LayoutChangeEvent) => setTrackWidth(event.nativeEvent.layout.width) } : {})}
    >
      {thumb ? <Thumb index={selectedIndex} count={options.length} trackWidth={trackWidth} /> : null}
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
            py="$2"
            rounded="$3"
            bg={selected && !thumb ? '$color9' : 'transparent'}
            pressStyle={selected ? undefined : { opacity: 0.7 }}
            cursor="pointer"
            onPress={handlePress}
            {...(thumb ? { role: 'tab' as const, 'aria-selected': selected } : {})}
          >
            <Text color={selected ? '$color1' : '$color'}>{option.label}</Text>
          </XStack>
        );
      })}
    </XStack>
  );
}
