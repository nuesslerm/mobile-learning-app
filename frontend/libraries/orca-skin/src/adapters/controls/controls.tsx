import type { ButtonProps, NoticeProps, SpinnerProps, SwitchProps, TextFieldProps } from '@lib/skin';
import { ActivityIndicator, Button as NativeButton, Pressable, StyleSheet, Switch as NativeSwitch, Text, TextInput, View } from 'react-native';

import { homePalettes, homeRadii, homeSpacing, useOrcaScheme } from '../appearance';
import { useChatPalette } from '../chatPalette';
import { tabPalettes } from '../tabPalette';

/** The platform's own button, as the Orca app draws its plain actions. */
export function Button(props: ButtonProps) {
  return (
    <NativeButton
      title={props.label}
      onPress={props.onPress}
      disabled={props.disabled}
      testID={props.testID}
      {...(props.tone === 'danger' ? { color: tabPalettes.light.danger } : {})}
    />
  );
}

export function Spinner(props: SpinnerProps) {
  return <ActivityIndicator accessibilityLabel={props.accessibilityLabel} />;
}

export function Switch(props: SwitchProps) {
  return (
    <NativeSwitch
      value={props.value}
      onValueChange={props.onValueChange}
      accessibilityLabel={props.accessibilityLabel}
      disabled={props.disabled}
      testID={props.testID}
    />
  );
}

export function TextField(props: TextFieldProps) {
  const colors = homePalettes[useOrcaScheme()];
  return (
    <TextInput
      value={props.value}
      onChangeText={props.onChangeText}
      placeholder={props.placeholder}
      placeholderTextColor={colors.textMuted}
      accessibilityLabel={props.accessibilityLabel ?? props.placeholder}
      autoFocus={props.autoFocus}
      autoCapitalize="none"
      autoCorrect={false}
      keyboardType={props.keyboard === 'url' ? 'url' : 'default'}
      onSubmitEditing={props.onSubmit}
      testID={props.testID}
      style={[styles.field, { color: colors.textPrimary, backgroundColor: colors.bgRaised, borderColor: colors.borderSubtle }]}
    />
  );
}

export function Notice(props: NoticeProps) {
  const colors = useChatPalette();
  return (
    <View style={[styles.card, { backgroundColor: colors.bgRaised }]}>
      <View accessibilityLiveRegion="polite" accessibilityRole="alert">
        <Text style={[styles.title, { color: colors.textPrimary }]}>{props.title}</Text>
        <Text style={[styles.detail, { color: colors.textSecondary }]}>{props.detail}</Text>
      </View>
      <Pressable accessibilityRole="button" onPress={props.onAction} style={styles.action}>
        <Text style={[styles.label, { color: colors.accentBlue }]}>{props.actionLabel}</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  field: { minHeight: 44, paddingHorizontal: homeSpacing.md, borderRadius: homeRadii.control, borderWidth: StyleSheet.hairlineWidth, fontSize: 15 },
  card: { marginHorizontal: 12, marginVertical: 10, paddingHorizontal: 14, paddingTop: 14, borderRadius: 14 },
  title: { fontSize: 16, fontWeight: '600' },
  detail: { fontSize: 14, lineHeight: 20, marginTop: 4 },
  action: { alignSelf: 'flex-start', minHeight: 44, justifyContent: 'center' },
  label: { fontSize: 15, fontWeight: '600' },
});
