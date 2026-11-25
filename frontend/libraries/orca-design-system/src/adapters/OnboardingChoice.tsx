import type { OnboardingChoiceProps } from '@lib/agent-ui-contracts';
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  useColorScheme,
} from 'react-native';

const palettes = {
  light: { base: '#ffffff', secondary: '#696969', bright: '#171717' },
  dark: { base: '#0a0a0a', secondary: '#acacac', bright: '#e5e5e5' },
};

export function OnboardingChoice({
  label,
  accessibilityLabel,
  primary,
  busy,
  disabled,
  onPress,
}: OnboardingChoiceProps) {
  const palette = palettes[useColorScheme() === 'dark' ? 'dark' : 'light'];
  const color = primary ? palette.base : palette.secondary;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.button,
        primary ? { backgroundColor: palette.bright } : styles.secondary,
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
    paddingVertical: 8,
  },
  secondary: { marginTop: 4 },
  pressed: { opacity: 0.72 },
  disabled: { opacity: 0.58 },
  label: { fontSize: 14 },
  primaryLabel: { fontWeight: '600' },
  secondaryLabel: { fontWeight: '500' },
});
