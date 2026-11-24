import { Platform } from 'react-native';

import { useNativeHeaderStore } from '../../lib/nativeHeaderStore';

import { nativeHeaderActive, type HeaderEnv } from './header-env';

/** What decides between the system header and the Tamagui header: the platform, the setting and the screen's own opt-in. */
export function useHeaderEnv(optedIn: boolean): HeaderEnv {
  const nativeHeaders = useNativeHeaderStore((state) => state.enabled);
  return { platform: Platform.OS, nativeHeaders, optedIn };
}

/** Whether the system header is the one in play, whichever screen asks: the root follows it for the navigator's theme and the OS chrome. */
export function useNativeHeaderPath(): boolean {
  return nativeHeaderActive(useHeaderEnv(true));
}
