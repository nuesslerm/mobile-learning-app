import type { OnboardingChoiceProps } from '@lib/agent-ui-contracts';
import { ActivityIndicator, Pressable, StyleSheet, Text } from 'react-native';

import { homePalettes, useOrcaScheme } from './appearance';

export function OnboardingChoice({
  label,
  accessibilityLabel,
  primary,
  busy,
  disabled,
  onPress,
}: OnboardingChoiceProps) {
  const palette = homePalettes[useOrcaScheme()];
  const color = primary ? palette.bgBase : palette.textSecondary;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.button,
        primary ? { backgroundColor: palette.surfaceBright } : styles.secondary,
        pressed && styles.pressed,
        disabled && styles.disabled,
      ]}
    >
      {busy ? (
        <ActivityIndicator color={color} />
      ) : (
        <Text
          style={[
            styles.label,
            primary ? styles.primaryLabel : styles.secondaryLabel,
            { color },
          ]}
        >
          {label}
        </Text>
      )}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  button: {
    minHeight: 44,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 12,
    borderCurve: 'continuous',
    paddingVertical: 8,
  },
  secondary: { marginTop: 4 },
  pressed: { opacity: 0.72 },
  disabled: { opacity: 0.58 },
  label: { fontSize: 14 },
  primaryLabel: { fontWeight: '600' },
  secondaryLabel: { fontWeight: '500' },
});
