import type { HomeSettingsRowProps } from '@lib/skin';
import { Icon, ListItem, Switch, Text, View } from '@lib/tamagui-design-system';
import { Check } from '@lib/tamagui-design-system/icons';

import { homeIcons } from './homeIcons';

/** One row of a settings group, laid out as the app's own Settings rows are. */
export function HomeSettingsRow(props: HomeSettingsRowProps) {
  const label = props.accessibilityLabel ?? (props.value ? `${props.label}, ${props.value}` : props.label);
  return (
    <ListItem
      variant="grouped"
      showChevron={props.navigates}
      pressLog="off"
      onPress={props.onPress}
      accessibilityLabel={label}
      selected={props.selected}
      disabled={props.disabled}
      testID="skin-settings-row"
    >
      <View flexDirection="row" items="center" gap="$3" flex={1}>
        {props.icon ? <Icon as={homeIcons[props.icon]} size={18} color="$color11" /> : null}
        <Text flex={1} numberOfLines={1}>
          {props.label}
        </Text>
        {props.value ? (
          <Text color="$color11" numberOfLines={1}>
            {props.value}
          </Text>
        ) : null}
        {props.accessory ? <Text variant="strong">{props.accessory}</Text> : null}
        {props.toggle ? (
          <Switch
            testID="skin-settings-switch"
            value={props.toggle.value}
            onValueChange={props.toggle.onValueChange}
            accessibilityLabel={props.toggle.accessibilityLabel ?? props.label}
          />
        ) : null}
        {props.selected ? <Icon as={Check} size={18} /> : null}
      </View>
    </ListItem>
  );
}
