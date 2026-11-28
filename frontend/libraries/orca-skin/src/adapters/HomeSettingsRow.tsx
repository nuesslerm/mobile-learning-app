// Ported from `references/orca/mobile/src/settings/mobile-settings-menu.tsx` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.),
// over `references/orca/mobile/src/components/list/ListGroup.tsx`.
import type { HomeSettingsRowProps } from '@lib/skin';
import { Pressable, Switch, Text, View } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';
import { CheckIcon, ChevronRightIcon, homeIcons } from './homeIcons';
import { useHomeTabStyles } from './homeTabStyles';

/** One row of the settings menu the home's last tab carries. */
export function HomeSettingsRow(props: HomeSettingsRowProps) {
  const styles = useHomeTabStyles();
  const colors = homePalettes[useOrcaScheme()];
  const Icon = props.icon ? homeIcons[props.icon] : null;
  const label =
    props.accessibilityLabel ??
    (props.value ? `${props.label}, ${props.value}` : props.label);
  const content = (
    <>
      {Icon ? (
        <View style={styles.settingsLeading}>
          <Icon size={18} color={colors.textSecondary} />
        </View>
      ) : null}
      <Text style={styles.settingsLabel} numberOfLines={1}>
        {props.label}
      </Text>
      {props.value ? (
        <Text style={styles.settingsValue} numberOfLines={1}>
          {props.value}
        </Text>
      ) : null}
      {props.accessory ? (
        <Text style={styles.settingsAccessory} numberOfLines={1}>
          {props.accessory}
        </Text>
      ) : null}
      {props.toggle ? (
        <Switch
          value={props.toggle.value}
          onValueChange={props.toggle.onValueChange}
          accessibilityLabel={props.toggle.accessibilityLabel ?? props.label}
        />
      ) : null}
      {props.selected ? <CheckIcon size={17} color={colors.textPrimary} /> : null}
      {props.navigates ? <ChevronRightIcon size={15} strokeWidth={2.2} color={colors.textMuted} /> : null}
    </>
  );
  if (!props.onPress) {
    return <View style={[styles.settingsRow, props.disabled && styles.disabled]}>{content}</View>;
  }
  return (
    <Pressable
      style={[styles.settingsRow, props.disabled && styles.disabled]}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled: Boolean(props.disabled) }}
      disabled={props.disabled}
      onPress={props.onPress}
    >
      {content}
    </Pressable>
  );
}
