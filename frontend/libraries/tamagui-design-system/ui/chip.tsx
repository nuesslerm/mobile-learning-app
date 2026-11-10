import { Logger } from '@lib/logging';
import type { LucideIcon } from 'lucide-react-native';
import type { GestureResponderEvent } from 'react-native';

import { XStack } from '../primitives';

import { Icon } from './icon';
import { Text } from './text';

export type ChipSize = 'sm' | 'md';

export type ChipProps = {
  /** Lucide icon rendered before the label. */
  icon?: LucideIcon;
  label: string;
  selected?: boolean;
  size?: ChipSize;
  onPress?: (event: GestureResponderEvent) => void;
  /** Suppress the automatic press log. Use on rows that would log themselves. */
  pressLog?: 'off';
  testID?: string;
};

const SIZE_PROPS: Record<ChipSize, { px: '$2.5' | '$3'; py: '$1' | '$1.5' }> = {
  sm: { px: '$2.5', py: '$1' },
  md: { px: '$3', py: '$1.5' },
};

const LABEL_COLOR: Record<'on' | 'off', '$color1' | '$color'> = {
  on: '$color1',
  off: '$color',
};

/**
 * Pressable pill -- a compact filter/selector chip.
 *
 * Selection flips the surface to the theme accent and the label to the
 * contrasting foreground, the same one-two the rest of the DS uses for
 * "this row is the chosen one".
 */
export function Chip({
  icon,
  label,
  selected = false,
  size = 'md',
  onPress,
  pressLog,
  testID,
}: ChipProps) {
  const handlePress = (event: GestureResponderEvent) => {
    if (pressLog !== 'off') Logger.info('ds.chip.press', { testID, selected });
    onPress?.(event);
  };

  const tone = selected ? 'on' : 'off';

  return (
    <XStack
      testID={testID}
      items="center"
      gap="$1.5"
      rounded={9999}
      borderWidth={1}
      borderColor="$borderColor"
      bg={selected ? '$color9' : '$color3'}
      pressStyle={{ opacity: 0.7 }}
      cursor="pointer"
      {...SIZE_PROPS[size]}
      onPress={handlePress}
    >
      {icon ? (
        <Icon
          as={icon}
          size={size === 'sm' ? 14 : 16}
          color={LABEL_COLOR[tone]}
        />
      ) : null}
      <Text size={size === 'sm' ? '$1' : '$2'} color={LABEL_COLOR[tone]}>
        {label}
      </Text>
    </XStack>
  );
}
