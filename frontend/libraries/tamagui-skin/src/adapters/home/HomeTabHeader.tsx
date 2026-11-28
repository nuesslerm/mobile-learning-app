import type { HomeTabHeaderProps } from '@lib/skin';
import { Icon, Text } from '@lib/tamagui-design-system';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';

import { homeIcons } from './homeIcons';

const FLANK_WIDTH = 96;
const ACTION_SIZE = 34;

/** A tab's title row in the shape of the app's screen header: a centred title, the actions in one pill. */
export function HomeTabHeader(props: HomeTabHeaderProps) {
  const actions = props.actions ?? [];
  return (
    <XStack
      testID="skin-home-header"
      items="center"
      px="$3"
      pt="$2"
      pb="$2.5"
      bg="$background"
      borderBottomWidth={1}
      borderColor="$borderColor"
    >
      <XStack width={FLANK_WIDTH} />
      <YStack flex={1} items="center">
        <Text variant="large" role="heading" numberOfLines={1}>
          {props.title}
        </Text>
      </YStack>
      <XStack width={FLANK_WIDTH} minH={ACTION_SIZE + 4} items="center" justify="flex-end">
        {actions.length > 0 ? (
          <XStack rounded={9999} bg="$color3" p="$0.5" gap="$0.5">
            {actions.map((action) => (
              <XStack
                key={action.id}
                role="button" accessible
                aria-label={action.accessibilityLabel}
                aria-expanded={Boolean(action.active)}
                aria-disabled={Boolean(action.disabled)}
                width={ACTION_SIZE}
                height={ACTION_SIZE}
                items="center"
                justify="center"
                rounded={9999}
                bg={action.active ? '$color5' : 'transparent'}
                opacity={action.disabled ? 0.4 : 1}
                pressStyle={{ bg: '$color5' }}
                cursor="pointer"
                disabled={action.disabled}
                onPress={action.onPress}
              >
                <Icon as={homeIcons[action.icon]} size={19} />
              </XStack>
            ))}
          </XStack>
        ) : null}
      </XStack>
    </XStack>
  );
}
