import { XStack, YStack } from '../primitives';

import { Icon, type IconProps } from './icon';
import { Text } from './text';

export type StatTileProps = {
  icon: IconProps['as'];
  value: string;
  label: string;
};

/**
 * Dashboard stat: tinted icon chip, big value, uppercase label on a card
 * surface.
 *
 * The chip tint that app #1 got from `bg-primary/10` is a `$primary` layer
 * at 0.1 opacity under the icon, since the theme keys resolve to opaque
 * `rgb()` strings and every colour still comes from a token.
 */
export function StatTile({ icon, value, label }: StatTileProps) {
  return (
    <YStack
      testID="StatTile-Root"
      flex={1}
      minH={112}
      rounded="$xl"
      borderWidth={1}
      borderColor="$border"
      bg="$card"
      p="$4"
    >
      <XStack
        width="$10"
        height="$10"
        rounded="$pill"
        items="center"
        justify="center"
      >
        <YStack
          position="absolute"
          t={0}
          b={0}
          l={0}
          r={0}
          rounded="$pill"
          bg="$primary"
          opacity={0.1}
        />
        <Icon as={icon} size={18} color="$primary" />
      </XStack>
      <Text variant="strong" size="$3xl" mt="$4">
        {value}
      </Text>
      <Text
        color="$muted-foreground"
        size="$xs"
        textTransform="uppercase"
        letterSpacing={0.5}
        mt="$1"
      >
        {label}
      </Text>
    </YStack>
  );
}
