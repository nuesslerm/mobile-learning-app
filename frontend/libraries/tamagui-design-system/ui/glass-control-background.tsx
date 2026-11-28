import { GlassView, isGlassEffectAPIAvailable, isLiquidGlassAvailable } from 'expo-glass-effect';
import { StyleSheet, useColorScheme } from 'react-native';

import { usePresetThemeStore } from '../theme/state/presetThemeStore';

export const DARK_GLASS_CONTROL_TINT = 'rgba(0,0,0,0.45)';

export function supportsGlassControl(): boolean {
  return isLiquidGlassAvailable() && isGlassEffectAPIAvailable();
}

const CAPSULE = 9999;

/**
 * Native glass behind a control or a floating surface. A control is a capsule; a surface with a corner radius of its own passes it, because
 * the glass is not clipped by its parent: a capsule behind a card of several lines drew a lens across it.
 */
export function GlassControlBackground({ interactive = false, tintColor, radius = CAPSULE }: { interactive?: boolean; tintColor?: string; radius?: number }) {
  const system = useColorScheme();
  const mode = usePresetThemeStore((state) => state.themeMode);
  const scheme = mode === 'system' ? system ?? 'light' : mode;
  return (
    <GlassView
      isInteractive={interactive}
      pointerEvents="none"
      glassEffectStyle="regular"
      colorScheme={scheme}
      tintColor={tintColor ?? (scheme === 'dark' ? DARK_GLASS_CONTROL_TINT : undefined)}
      style={[StyleSheet.absoluteFill, { borderRadius: radius }]}
    />
  );
}
