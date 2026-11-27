import { afterEach, describe, expect, it, vi } from 'vitest';

async function buoyOn(platform: string, env: Record<string, string>) {
  vi.resetModules();
  vi.doMock('react-native', () => ({ Platform: { OS: platform } }));
  for (const [name, value] of Object.entries(env)) vi.stubEnv(name, value);
  return (await import('../buoyEnabled')).buoyEnabled;
}

describe('Buoy in a preview', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.doUnmock('react-native');
  });

  it('Given a staging preview on a phone, when the app starts, then Buoy is on', async () => {
    expect(await buoyOn('ios', { EXPO_PUBLIC_APP_ENV: 'staging', EXPO_PUBLIC_DEVICE_LAB: '' })).toBe(true);
  });

  it('Given a device-lab build, when the app starts, then Buoy stays out so a test can read the screen', async () => {
    expect(await buoyOn('ios', { EXPO_PUBLIC_APP_ENV: 'staging', EXPO_PUBLIC_DEVICE_LAB: '1' })).toBe(false);
  });

  it('Given the web, when the app starts, then Buoy stays out', async () => {
    expect(await buoyOn('web', { EXPO_PUBLIC_APP_ENV: 'staging', EXPO_PUBLIC_DEVICE_LAB: '' })).toBe(false);
  });
});
