import type { HomeSettingsRowProps } from '@lib/skin';
import { Icon, Switch, Text } from '@lib/tamagui-design-system';
import { Check, ChevronRight } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';

import { homeIcons } from './homeIcons';

/** One row of a settings group. */
export function HomeSettingsRow(props: HomeSettingsRowProps) {
  const label =
    props.accessibilityLabel ??
    (props.value ? `${props.label}, ${props.value}` : props.label);
  return (
    <XStack
      items="center"
      gap="$3"
      px="$4"
      py="$2"
      minH={48}
      opacity={props.disabled ? 0.4 : 1}
      {...(props.onPress
        ? {
            role: 'button',
            'aria-label': label,
            'aria-disabled': Boolean(props.disabled),
            disabled: props.disabled,
            onPress: props.onPress,
            pressStyle: { bg: '$color3' },
            cursor: 'pointer',
          }
        : {})}
    >
      {props.icon ? (
        <XStack width={24} justify="center">
          <Icon as={homeIcons[props.icon]} size={18} color="$color11" />
        </XStack>
      ) : null}
      <Text flex={1} numberOfLines={1}>
        {props.label}
      </Text>
      {props.value ? (
        <Text color="$color11" shrink={1} maxW={180} numberOfLines={1}>
          {props.value}
        </Text>
      ) : null}
      {props.accessory ? (
        <Text fontWeight="600" numberOfLines={1}>
          {props.accessory}
        </Text>
      ) : null}
      {props.toggle ? (
        <Switch
          testID="agent-settings-switch"
          value={props.toggle.value}
          onValueChange={props.toggle.onValueChange}
          accessibilityLabel={props.toggle.accessibilityLabel ?? props.label}
        />
      ) : null}
      {props.selected ? <Icon as={Check} size={17} /> : null}
      {props.navigates ? (
        <Icon as={ChevronRight} size={18} color="$color10" />
      ) : null}
    </XStack>
  );
}
