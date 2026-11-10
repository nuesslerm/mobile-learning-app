import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { transformSync } from '@babel/core';
import { expectWarning, screen } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { appWithRoot as app } from './appWithRoot';
import { fetchDouble, nativeGlobalHandler } from './betterStackDoubles';

const HOST = 'ingest.betterstack.flow.test';
const TOKEN = 'flow-test-ingest-token';

afterEach(() => {
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
});

describe('given a build that carries the BetterStack ingest pair', () => {
  it('when the app boots and an error goes uncaught, then app.boot and app.crash are posted with the bearer token before the fatal handler runs', async () => {
    // Given the pair in the build's env, an ingest that holds its answers, and React Native's global handler
    vi.stubEnv('EXPO_PUBLIC_BETTERSTACK_TOKEN', TOKEN);
    vi.stubEnv('EXPO_PUBLIC_BETTERSTACK_HOST', HOST);
    const ingest = fetchDouble(HOST, { hold: true });
    vi.stubGlobal('fetch', ingest.fetch);
    const fatalSeen: unknown[] = [];
    const uncaught = nativeGlobalHandler((error) => fatalSeen.push(error));

    // When the app boots through the root layout and a fatal error reaches the global handler
    await app.render('/settings');
    expect(screen.getByTestId('SettingsIndexScreen')).toBeTruthy();
    expectWarning(/app\.crash/);
    const boom = new Error('boom');
    uncaught(boom, true);

    // Then the crash POST left at once, and React Native's fatal handler waits until the ingest answers
    expect(ingest.posts.length).toBeGreaterThan(0);
    await Promise.resolve();
    expect(fatalSeen).toEqual([]);
    ingest.releaseAll();
    await vi.waitFor(() => expect(fatalSeen).toEqual([boom]));

    // And every POST went to the ingest host with the bearer token, together carrying app.boot and app.crash
    for (const post of ingest.posts) {
      expect(post).toMatchObject({
        url: `https://${HOST}`,
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${TOKEN}` },
      });
    }
    const items = ingest.posts.flatMap((p) => p.items);
    expect(items.find((i) => i.message === 'app.boot')).toMatchObject({ level: 'info', source: 'AppShell', app: 'mla-tamagui', platform: 'web' });
    // BetterStack's `message` is the event name, so the crash's own message field arrives as context_message
    expect(items.find((i) => i.message === 'app.crash')).toMatchObject({
      level: 'error',
      kind: 'error',
      fatal: true,
      context_message: 'boom',
      error: expect.objectContaining({ message: 'boom' }),
      app: 'mla-tamagui',
    });
  });
});

describe("given the app's logger as Expo builds it for a device", () => {
  it('when the build carries the pair, then both values are inlined into the bundle', () => {
    // Given the pair in the build's env and Expo's own babel preset in a production device build
    vi.stubEnv('EXPO_PUBLIC_BETTERSTACK_TOKEN', TOKEN);
    vi.stubEnv('EXPO_PUBLIC_BETTERSTACK_HOST', HOST);
    const filename = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../lib/logger.ts');
    const caller = { name: 'metro', bundler: 'metro', platform: 'android', isDev: false, isServer: false };

    // When lib/logger.ts is transformed the way Metro does for an EAS Update
    const out = transformSync(readFileSync(filename, 'utf8'), {
      filename,
      babelrc: false,
      configFile: false,
      presets: ['babel-preset-expo'],
      caller,
    });

    // Then both values are literals in the output: a dynamic read would be left as an empty lookup on a device
    expect(out?.code).toContain(JSON.stringify(TOKEN));
    expect(out?.code).toContain(JSON.stringify(HOST));
    expect(out?.code).not.toContain('EXPO_PUBLIC_BETTERSTACK');
  });
});
