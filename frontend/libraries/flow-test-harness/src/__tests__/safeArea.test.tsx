import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { describe, expect, it } from 'vitest';

import { renderHook } from '../index';
import { resetSafeArea, setSafeAreaDevice } from '../safeArea';

describe('the safe-area stub', () => {
  it('given no device named, when a screen reads the insets, then it gets an iPhone 15 Pro (a notch, a home indicator)', () => {
    const { result } = renderHook(() => useSafeAreaInsets());

    expect(result.current).toEqual({ top: 59, right: 0, bottom: 34, left: 0 });
  });

  it('given a device named, when a screen reads the insets, then it gets that device', () => {
    setSafeAreaDevice('iphone-se');

    const { result } = renderHook(() => useSafeAreaInsets());

    expect(result.current).toEqual({ top: 20, right: 0, bottom: 0, left: 0 });
    resetSafeArea();
  });
});
