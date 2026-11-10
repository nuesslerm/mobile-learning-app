import { Logger } from '@lib/logging';
import type { GestureResponderEvent } from 'react-native';

import { XStack } from '../primitives';

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
  testID?: string;
};

/**
 * Single-select segmented control -- one XStack rail, one pressable segment
 * per option. The selected segment uses the theme accent so it reads as
 * "the chosen one" against the neutral rail; the unselected segments stay
 * transparent until pressed.
 */
export function SegmentedControl({
  options,
  value,
  onChange,
  pressLog,
  testID,
}: SegmentedControlProps) {
  return (
    <XStack testID={testID} bg="$color3" rounded="$4" p="$1" gap="$1">
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
            bg={selected ? '$color9' : 'transparent'}
            pressStyle={selected ? undefined : { opacity: 0.7 }}
            cursor="pointer"
            onPress={handlePress}
          >
            <Text color={selected ? '$color1' : '$color'}>{option.label}</Text>
          </XStack>
        );
      })}
    </XStack>
  );
}
