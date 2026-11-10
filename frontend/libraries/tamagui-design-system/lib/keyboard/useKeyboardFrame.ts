import { useEffect } from 'react';

import { useKeyboardStore } from '../keyboardStore';

import { startKeyboardSource } from './keyboardSource';
import type { KeyboardFrame } from './types';

// Ref-counted so the source is installed exactly once no matter how many
// components read the frame, and torn down when the last one unmounts.
let refCount = 0;
let stop: (() => void) | null = null;

/**
 * The keyboard's current geometry, on every platform.
 *
 * The hook INSTALLS ITS OWN SOURCE. That is the whole point: the previous
 * design had a `KeyboardObserver` component the app had to mount, and only the
 * native path depended on it -- so forgetting it left web working perfectly and
 * native with `visible: false` forever, which is why the editor's keyboard
 * toolbar never appeared on a real device while every web check passed. There
 * is now nothing to mount and nothing to forget.
 */
export function useKeyboardFrame(): KeyboardFrame {
  useEffect(() => {
    if (refCount === 0) stop = startKeyboardSource();
    refCount += 1;
    return () => {
      refCount -= 1;
      if (refCount === 0) {
        stop?.();
        stop = null;
      }
    };
  }, []);

  const visible = useKeyboardStore((s) => s.visible);
  const height = useKeyboardStore((s) => s.height);
  const duration = useKeyboardStore((s) => s.duration);
  const easing = useKeyboardStore((s) => s.easing);

  return { visible, height, duration, easing };
}
