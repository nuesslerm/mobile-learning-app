import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

beforeEach(() => {
  vi.resetModules();
  vi.stubEnv('EXPO_PUBLIC_APP_ENV', 'development');
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- mirror the Metro/React Native global defined in src/global.d.ts
  (globalThis as any).__DEV__ = true;
});

afterEach(() => {
  vi.unstubAllEnvs();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (globalThis as any).__DEV__;
});

describe('appConfig.vmOpsApiBaseUrl', () => {
  it('Given EXPO_PUBLIC_VM_OPS_API_BASE_URL is set When appConfig is imported Then the env value is used', async () => {
    vi.stubEnv('EXPO_PUBLIC_VM_OPS_API_BASE_URL', 'https://override.example.test');
    const { appConfig } = await import('./appConfig');
    expect(appConfig.vmOpsApiBaseUrl).toBe('https://override.example.test');
  });

  it('Given EXPO_PUBLIC_VM_OPS_API_BASE_URL is unset When appConfig is imported in development Then the development default is used', async () => {
    vi.stubEnv('EXPO_PUBLIC_VM_OPS_API_BASE_URL', '');
    const { appConfig } = await import('./appConfig');
    expect(appConfig.vmOpsApiBaseUrl).toBe('http://vm-ops-api-main:8080');
  });
});