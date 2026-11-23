import { describe, expect, it } from 'vitest';

import type { HeaderEnv } from '../composites/native-header/header-env';
import { scrollUnderProps } from '../composites/native-header/scroll-under-props';

const NATIVE: HeaderEnv = {
  platform: 'ios',
  nativeHeaders: true,
  optedIn: true,
};

describe('scrollUnderProps', () => {
  it('Given the native bar, When the props are read, Then the list adjusts its inset automatically', () => {
    expect(scrollUnderProps(NATIVE)).toEqual({
      contentInsetAdjustmentBehavior: 'automatic',
    });
  });

  it.each<[string, HeaderEnv]>([
    ['android', { ...NATIVE, platform: 'android' }],
    ['web', { ...NATIVE, platform: 'web' }],
    ['the setting off', { ...NATIVE, nativeHeaders: false }],
    ['no opt-in', { ...NATIVE, optedIn: false }],
  ])('Given %s, When the props are read, Then they are empty', (_n, env) => {
    expect(scrollUnderProps(env)).toEqual({});
  });
});
