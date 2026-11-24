import { GlassView, isGlassEffectAPIAvailable, isLiquidGlassAvailable } from 'expo-glass-effect';
import { StyleSheet, useColorScheme } from 'react-native';

import { usePresetThemeStore } from '../theme/state/presetThemeStore';

export const DARK_GLASS_CONTROL_TINT = 'rgba(0,0,0,0.45)';

export function supportsGlassControl(): boolean {
  return isLiquidGlassAvailable() && isGlassEffectAPIAvailable();
}

export function GlassControlBackground() {
  const system = useColorScheme();
  const mode = usePresetThemeStore((state) => state.themeMode);
  const scheme = mode === 'system' ? system ?? 'light' : mode;
  return (
    <GlassView
      pointerEvents="none"
      glassEffectStyle="regular"
      colorScheme={scheme}
      tintColor={scheme === 'dark' ? DARK_GLASS_CONTROL_TINT : undefined}
      style={styles.surface}
    />
  );
}

const styles = StyleSheet.create({
  surface: { ...StyleSheet.absoluteFill, borderRadius: 9999 },
});
