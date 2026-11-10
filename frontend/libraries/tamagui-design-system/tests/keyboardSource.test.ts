// Both keyboard sources, held to ONE contract.
//
// This is the test that would have caught the bug it was written for. The web
// and native paths used to be separate `useKeyboardFrame` implementations that
// shared only a return TYPE: web read the visual viewport directly, native read
// a store that some component had to be mounted to fill. Nobody mounted it, so
// on a real device `visible` was false forever and the editor's keyboard
// toolbar never appeared -- while every web check passed, because web needed no
// wiring at all.
//
// Both now push into the same store through a `startKeyboardSource()` with the
// same signature, so the cases below can drive either one and assert the same
// observable outcome. What a web check can prove about native is exactly the
// surface these tests hold in common.
import { beforeEach, describe, expect, it, vi } from 'vitest';

type Listener = (event: unknown) => void;

const listeners = new Map<string, Listener[]>();

vi.mock('react-native', () => ({
  Platform: { OS: 'ios' },
  Keyboard: {
    addListener: (name: string, fn: Listener) => {
      listeners.set(name, [...(listeners.get(name) ?? []), fn]);
      return {
        remove: () =>
          listeners.set(name, (listeners.get(name) ?? []).filter((l) => l !== fn)),
      };
    },
  },
}));

vi.mock('@lib/logging', () => ({
  Logger: {
    info: vi.fn(),
    warn: vi.fn(),
    withError: () => ({ warn: vi.fn() }),
  },
}));

function emit(name: string, event: unknown) {
  for (const fn of listeners.get(name) ?? []) fn(event);
}

async function freshStore() {
  vi.resetModules();
  listeners.clear();
  const { useKeyboardStore } = await import('../lib/keyboardStore');
  return useKeyboardStore;
}

describe('given the native keyboard source', () => {
  beforeEach(() => {
    listeners.clear();
  });

  it('when the keyboard shows, then the store carries its height and transition', async () => {
    const store = await freshStore();
    const { startKeyboardSource } = await import('../lib/keyboard/keyboardSource');
    const stop = startKeyboardSource();

    emit('keyboardWillShow', {
      endCoordinates: { height: 336 },
      duration: 250,
      easing: 'keyboard',
    });

    expect(store.getState().visible).toBe(true);
    expect(store.getState().height).toBe(336);
    expect(store.getState().duration).toBe(250);
    stop();
  });

  it('when the keyboard hides, then the store reports it hidden', async () => {
    const store = await freshStore();
    const { startKeyboardSource } = await import('../lib/keyboard/keyboardSource');
    const stop = startKeyboardSource();

    emit('keyboardWillShow', { endCoordinates: { height: 336 }, duration: 250, easing: null });
    emit('keyboardWillHide', { endCoordinates: { height: 0 }, duration: 250, easing: null });

    expect(store.getState().visible).toBe(false);
    expect(store.getState().height).toBe(0);
    stop();
  });

  it('when the source is stopped, then a later event no longer reaches the store', async () => {
    // The teardown matters: the hook ref-counts installs, so a leaked listener
    // would keep writing after the last consumer unmounted.
    const store = await freshStore();
    const { startKeyboardSource } = await import('../lib/keyboard/keyboardSource');
    startKeyboardSource()();

    emit('keyboardWillShow', { endCoordinates: { height: 336 }, duration: 250, easing: null });

    expect(store.getState().visible).toBe(false);
  });

  it('when subscribing, then it listens for the events the platform actually emits', async () => {
    // iOS gets Will*, which carries the animation duration so UI moves WITH the
    // keyboard. Android never fires Will* at all, so it must use Did*, and
    // getting this backwards is silent: no events, no toolbar, no error.
    await freshStore();
    const { startKeyboardSource } = await import('../lib/keyboard/keyboardSource');
    const stop = startKeyboardSource();

    expect([...listeners.keys()].sort()).toEqual(['keyboardWillHide', 'keyboardWillShow']);
    stop();
  });
});

describe('given the web keyboard source', () => {
  // A hand-built window rather than jsdom: this suite runs in the 'node'
  // environment, and the source only needs the few surfaces asserted here.
  // Building them explicitly also documents exactly what it depends on.
  function fakeWindow({ innerHeight = 852, visualHeight = 852, offsetTop = 0 } = {}) {
    const handlers = new Map<string, Set<(e?: unknown) => void>>();
    const on = (name: string, fn: (e?: unknown) => void) => {
      if (!handlers.has(name)) handlers.set(name, new Set());
      handlers.get(name)?.add(fn);
    };
    const off = (name: string, fn: (e?: unknown) => void) => handlers.get(name)?.delete(fn);
    const win = {
      innerHeight,
      visualViewport: { height: visualHeight, offsetTop, addEventListener: on, removeEventListener: off },
      addEventListener: on,
      removeEventListener: off,
      // Synchronous, so a test can assert right after firing an event.
      requestAnimationFrame: (fn: () => void) => {
        fn();
        return 1;
      },
      cancelAnimationFrame: () => {},
      __MCP_KEYBOARD_VISIBLE__: undefined as boolean | undefined,
      __MCP_KEYBOARD_MODE__: undefined as 'overlay' | 'resize' | undefined,
      __MCP_KEYBOARD_HEIGHT__: undefined as number | undefined,
    };
    const doc = { activeElement: null as unknown, addEventListener: on, removeEventListener: off };
    return { win, doc, fire: (name: string) => handlers.get(name)?.forEach((fn) => fn()) };
  }

  async function startWeb(setup: ReturnType<typeof fakeWindow>) {
    const store = await freshStore();
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- installing a minimal fake window for this source's few DOM dependencies
    (globalThis as any).window = setup.win;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- ditto for document
    (globalThis as any).document = setup.doc;
    const { startKeyboardSource } = await import('../lib/keyboard/keyboardSource.web');
    return { store, stop: startKeyboardSource() };
  }

  it('when the visual viewport shrinks under a focused input, then the store reports that overlap', async () => {
    // Android's real behaviour, and the harness's `resize` mode: the viewport
    // itself shrinks, so its own geometry is the answer.
    const setup = fakeWindow({ innerHeight: 852, visualHeight: 582 });
    setup.doc.activeElement = { tagName: 'TEXTAREA', getAttribute: () => null };
    const { store, stop } = await startWeb(setup);

    setup.fire('resize');

    expect(store.getState().visible).toBe(true);
    expect(store.getState().height).toBe(270);
    stop();
  });

  it('when the harness reports an overlay keyboard, then the store takes its inset', async () => {
    // iOS's real behaviour: the viewport keeps its full height, so nothing in
    // the DOM says the lower band is covered -- only the harness does.
    const setup = fakeWindow({ innerHeight: 852, visualHeight: 852 });
    setup.win.__MCP_KEYBOARD_VISIBLE__ = true;
    setup.win.__MCP_KEYBOARD_MODE__ = 'overlay';
    setup.win.__MCP_KEYBOARD_HEIGHT__ = 270;
    const { store, stop } = await startWeb(setup);

    setup.fire('mcp:keyboardframe');

    expect(store.getState().visible).toBe(true);
    expect(store.getState().height).toBe(270);
    stop();
  });

  it('when the harness reports a resize keyboard, then no inset is added on top', async () => {
    // The viewport already carries the overlap here; adding the harness height
    // as well would double-shift the layout.
    const setup = fakeWindow({ innerHeight: 645, visualHeight: 645 });
    setup.win.__MCP_KEYBOARD_VISIBLE__ = true;
    setup.win.__MCP_KEYBOARD_MODE__ = 'resize';
    setup.win.__MCP_KEYBOARD_HEIGHT__ = 270;
    const { store, stop } = await startWeb(setup);

    setup.fire('mcp:keyboardframe');

    expect(store.getState().visible).toBe(true);
    expect(store.getState().height).toBe(0);
    stop();
  });

  it('when nothing is focused and the viewport is whole, then the store reports hidden', async () => {
    const setup = fakeWindow();
    const { store, stop } = await startWeb(setup);

    setup.fire('resize');

    expect(store.getState().visible).toBe(false);
    expect(store.getState().height).toBe(0);
    stop();
  });

  it('when the source is stopped, then a later event no longer reaches the store', async () => {
    // Start in a hidden world so the source's own initial publish leaves the
    // store hidden -- otherwise this asserts against that publish rather than
    // against the teardown, and passes for the wrong reason.
    const setup = fakeWindow({ innerHeight: 852, visualHeight: 852 });
    const { store, stop } = await startWeb(setup);
    expect(store.getState().visible).toBe(false);
    stop();

    // Now make the world look exactly like a raised keyboard.
    setup.doc.activeElement = { tagName: 'TEXTAREA', getAttribute: () => null };
    setup.win.visualViewport.height = 582;
    setup.fire('resize');

    expect(store.getState().visible).toBe(false);
    expect(store.getState().height).toBe(0);
  });
});
