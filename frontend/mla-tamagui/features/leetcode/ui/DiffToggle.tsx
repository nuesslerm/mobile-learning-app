import { Logger } from '@lib/logging';
import { Text } from '@lib/tamagui-design-system';
import { useCallback } from 'react';
import { Circle, XStack } from '@lib/tamagui-design-system/primitives';

export type DiffToggleProps = {
  enabled: boolean;
  onToggle: (enabled: boolean) => void;
  disabled?: boolean;
  testID?: string;
};

/**
 * Pill that toggles the editor's inline diff overlay, showing its state with
 * both colour and a dot so it reads at a glance.
 *
 * App #1's version hand-rolls a Duolingo "chunky button": a Pressable with a
 * shadow offset that changes on press, five `rgbaColors.*(1)` lookups and
 * inline width/height/borderRadius numbers. This is the same control as a
 * themed XStack -- the state colours come from the theme rather than from a
 * store read.
 */
export function DiffToggle({
  enabled,
  onToggle,
  disabled = false,
  testID = 'DiffToggle',
}: DiffToggleProps) {
  const handlePress = useCallback(() => {
    if (disabled) return;
    const next = !enabled;
    Logger.info('diff.toggle', { testID, enabled: next });
    onToggle(next);
  }, [disabled, enabled, onToggle, testID]);

  return (
    <XStack
      testID={testID}
      role="switch"
      aria-label="Show diff"
      aria-checked={enabled}
      items="center"
      gap="$2"
      px="$3"
      height={36}
      rounded={9999}
      borderWidth={1}
      borderColor={enabled ? '$blue9' : '$borderColor'}
      bg={enabled ? '$blue9' : '$color2'}
      opacity={disabled ? 0.5 : 1}
      pressStyle={{ opacity: 0.7 }}
      cursor={disabled ? 'default' : 'pointer'}
      onPress={handlePress}
    >
      <Circle
        size={8}
        bg={enabled ? '$color1' : '$color9'}
        testID={`${testID}-Indicator`}
      />
      <Text
        variant="small"
        color={enabled ? '$color1' : '$color'}
        testID={`${testID}-Label`}
      >
        Diff
      </Text>
    </XStack>
  );
}
