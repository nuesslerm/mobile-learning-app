import { router } from 'expo-router';
import { describe, expect, it } from 'vitest';

import { routerHarness, storageHarness } from '../index';

describe('harnesses', () => {
  it('given router pushes and a storage seed, when read back and reset, then both handles reflect the stub state', () => {
    const r = routerHarness();
    const s = storageHarness();
    router.push('/a');
    router.push('/b');
    s.seed({ token: 'abc' });
    expect(r.pushes()).toEqual(['/a', '/b']);
    expect(s.get('token')).toBe('abc');
    r.reset();
    s.reset();
    expect(r.calls()).toEqual([]);
  });
});
