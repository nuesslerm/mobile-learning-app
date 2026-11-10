import { expectWarning, screen } from '@lib/flow-test-harness';
import { liveTail } from '@lib/logging/liveTail';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { appWithRoot as app } from './appWithRoot';
import { fetchDouble, nativeGlobalHandler } from './betterStackDoubles';

const HOST = 'ingest.betterstack.flow.test';
const TOKEN = 'flow-test-ingest-token';

afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

describe('given a build without the BetterStack ingest pair', () => {
  it('when the app boots and an error goes uncaught, then both lines are logged and nothing is posted', async () => {
    // Given the pair unset, a fetch double, and React Native's global handler
    vi.stubEnv('EXPO_PUBLIC_BETTERSTACK_TOKEN', '');
    vi.stubEnv('EXPO_PUBLIC_BETTERSTACK_HOST', '');
    const ingest = fetchDouble(HOST);
    vi.stubGlobal('fetch', ingest.fetch);
    const fatalSeen: unknown[] = [];
    const uncaught = nativeGlobalHandler((error) => fatalSeen.push(error));

    // When the app boots through the root layout and a fatal error reaches the global handler
    await app.render('/settings');
    expect(screen.getByTestId('SettingsIndexScreen')).toBeTruthy();
    expectWarning(/app\.crash/);
    const boom = new Error('nobody listening');
    uncaught(boom, true);
    await vi.waitFor(() => expect(fatalSeen).toEqual([boom]));

    // Then both lines reached the live tail, and no request left the app at all
    const texts = liveTail.getEntries().map((e) => e.text);
    expect(texts).toEqual(expect.arrayContaining(['app.boot', 'app.crash']));
    expect(ingest.requests).toEqual([]);
  });
});

describe('given a build that carries only half of the pair', () => {
  it.each([
    ['the token', TOKEN, ''],
    ['the host', '', HOST],
  ])('when the logger boots with only %s and logs, then nothing is posted', async (_half, token, host) => {
    // Given half of the pair and a fetch double (a second React cannot render in this file, so the boot is the
    // logger module the root layout imports first, loaded fresh)
    vi.stubEnv('EXPO_PUBLIC_BETTERSTACK_TOKEN', token);
    vi.stubEnv('EXPO_PUBLIC_BETTERSTACK_HOST', host);
    const ingest = fetchDouble(HOST);
    vi.stubGlobal('fetch', ingest.fetch);
    vi.resetModules();
    await import('@/lib/logger');
    const { Logger } = await import('@lib/logging');
    const { flushAllBufferedSinks } = await import('@lib/logging/primitives');

    // When a line is logged and every sink is drained
    Logger.info('probe.half_pair', { source: 'flow-test' });
    flushAllBufferedSinks();

    // Then no request left the app
    expect(ingest.requests).toEqual([]);
  });
});
