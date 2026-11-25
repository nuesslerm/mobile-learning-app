import { expectWarning, screen } from '@lib/flow-test-harness';
import { loggedLines } from './loggedLines';
import { describe, expect, it, vi } from 'vitest';

import { installCrashHandler } from '../../app-shell/crashHandler';
import { appWithRoot as app } from './appWithRoot';

type GlobalHandler = (error: unknown, isFatal?: boolean) => void;

describe('given the app shell with a native-style global error handler already set', () => {
  it('when an error goes uncaught and a rejection goes unhandled, then each is logged as app.crash and the earlier handler still runs', async () => {
    // Given React Native's global handler, as InitializeCore leaves it, before the app loads
    const seenByEarlier: unknown[] = [];
    let handler: GlobalHandler = (error) => seenByEarlier.push(error);
    (globalThis as { ErrorUtils?: unknown }).ErrorUtils = {
      getGlobalHandler: () => handler,
      setGlobalHandler: (next: GlobalHandler) => {
        handler = next;
      },
    };
    await app.render('/settings');
    expect(screen.getByTestId('SettingsIndexScreen')).toBeTruthy();
    const before = (await loggedLines('app.crash')).length;
    expectWarning(/app\.crash/);

    // When an error reaches the global handler, and the page sees an unhandled rejection
    const boom = new Error('boom');
    handler(boom, true);
    window.dispatchEvent(Object.assign(new Event('unhandledrejection'), { reason: new Error('nobody waited') }));

    // Then the earlier handler saw the error once the crash drain settled, and both are in the log as app.crash with their kind
    await vi.waitFor(() => expect(seenByEarlier).toEqual([boom]));
    const logged = (await loggedLines('app.crash')).slice(before);
    expect(logged.map((e) => e.context?.kind)).toEqual(['error', 'rejection']);
    expect(logged[0]?.context?.fatal).toBe(true);
    expect(logged.map((e) => e.context?.message)).toEqual(['boom', 'nobody waited']);

    // When a value with no toString is thrown
    const bare = Object.create(null) as object;
    handler(bare, false);
    // Then it is logged with a plain message and the earlier handler still sees it
    expect(seenByEarlier).toEqual([boom, bare]);
    expect((await loggedLines('app.crash')).slice(before)[2]?.context?.message).toBe('[object Object]');
  });

  it('when the web loader imports a split route that is not loaded yet, then the earlier handler gets the miss at once and no crash is logged', async () => {
    // Given a global handler as the web runtime leaves it, with the crash handler installed over it
    const seenByEarlier: unknown[] = [];
    let handler: GlobalHandler = (error) => seenByEarlier.push(error);
    installCrashHandler({
      ErrorUtils: {
        getGlobalHandler: () => handler,
        setGlobalHandler: (next: GlobalHandler) => {
          handler = next;
        },
      },
    });
    const before = (await loggedLines('app.crash')).length;

    // When Metro reports a module whose chunk has not arrived, as a fatal error
    const miss = new Error('Requiring unknown module "3885".');
    handler(miss, true);

    // Then the earlier handler has it before the call returns, so the loader's catch can fetch the chunk, and nothing is logged
    expect(seenByEarlier).toEqual([miss]);
    expect((await loggedLines('app.crash')).length).toBe(before);
  });
});
