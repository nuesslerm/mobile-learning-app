import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const reloadAsync = vi.fn();
const devReload = vi.fn();

vi.mock('expo-updates', () => ({ reloadAsync: (...args: unknown[]) => reloadAsync(...args) }));
vi.mock('react-native', () => ({ DevSettings: { reload: (...args: unknown[]) => devReload(...args) } }));

type DevScope = typeof globalThis & { __DEV__?: boolean };

async function restartWith(dev: boolean) {
  (globalThis as DevScope).__DEV__ = dev;
  const { restart } = await import('../devRestart');
  await restart();
}

describe('restart', () => {
  beforeEach(() => {
    reloadAsync.mockReset();
    devReload.mockReset();
    vi.resetModules();
  });
  afterEach(() => {
    delete (globalThis as DevScope).__DEV__;
  });

  it('Given a development build, When the app restarts, Then DevSettings reloads the bundle and expo-updates is left alone', async () => {
    // Given a development build whose DevSettings can reload
    // When the app restarts
    await restartWith(true);

    // Then DevSettings reloaded and no update was applied
    expect(devReload).toHaveBeenCalledTimes(1);
    expect(reloadAsync).not.toHaveBeenCalled();
  });

  it('Given a release build on a phone, When the app restarts, Then expo-updates reloads it (the native branch the browser never runs)', async () => {
    // Given a release build: no development reload
    // When the app restarts
    await restartWith(false);

    // Then it asked expo-updates to reload, and DevSettings stayed out of it
    expect(reloadAsync).toHaveBeenCalledTimes(1);
    expect(devReload).not.toHaveBeenCalled();
  });

  it('Given expo-updates refusing to reload, When the app restarts, Then it logs and does not throw', async () => {
    // Given expo-updates rejecting the reload
    reloadAsync.mockRejectedValueOnce(new Error('updates disabled'));

    // When the app restarts
    // Then the rejection is swallowed, as the caller does not await a result
    await expect(restartWith(false)).resolves.toBeUndefined();
  });
});
