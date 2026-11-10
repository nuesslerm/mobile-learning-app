import { useFonts } from 'expo-font';
import { useEffect } from 'react';

import { Logger } from '@lib/logging';

import notoSansTamil from '../../../assets/fonts/NotoSansTamil-Regular.ttf';

/**
 * The font remote-chat-agent draws its reference in when it marks a word. A
 * template printed in the platform's own Tamil font teaches a different shape
 * (ை in the browser's fallback reads as ெ there), and the marker rejects it.
 */
const TAMIL_FONT_FAMILY = 'NotoSansTamil';

/** True once the bundled font is ready to draw with. */
export function useTamilFont(): boolean {
  const [loaded, error] = useFonts({ [TAMIL_FONT_FAMILY]: notoSansTamil });
  useEffect(() => {
    if (error)
      Logger.error('tamil.font.load_failed', { message: error.message });
  }, [error]);
  return loaded;
}
