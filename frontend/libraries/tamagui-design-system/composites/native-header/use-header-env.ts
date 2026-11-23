import {
  isGlassEffectAPIAvailable,
  isLiquidGlassAvailable,
} from 'expo-glass-effect';
import { Platform } from 'react-native';

import { useGlassHeaderStore } from '../../lib/glassHeaderStore';

import { nativeHeaderActive, type HeaderEnv } from './header-env';

/** What decides between the native glass bar and the Tamagui header: the platform, both Liquid Glass checks (the package asks for the second before a GlassView), the setting and the screen's own opt-in. */
export function useHeaderEnv(optedIn: boolean): HeaderEnv {
  const glassHeaders = useGlassHeaderStore((state) => state.enabled);
  return {
    platform: Platform.OS,
    liquidGlass: isLiquidGlassAvailable() && isGlassEffectAPIAvailable(),
    glassHeaders,
    optedIn,
  };
}

/** Whether the native glass bar is the one in play, whichever screen asks: the root follows it for the navigator's theme and the OS chrome. */
export function useNativeHeaderPath(): boolean {
  return nativeHeaderActive(useHeaderEnv(true));
}
