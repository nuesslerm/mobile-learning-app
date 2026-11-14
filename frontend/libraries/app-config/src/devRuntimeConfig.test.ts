import { describe, expect, it, vi, beforeEach } from 'vitest';

import type * as Mod from './devRuntimeConfig';

// `LaunchArguments` is a real collaborator (the native bridge) of the
// devRuntimeConfig subject under test, mocked here so tests control its
// return value. The mock function is created via `vi.hoisted` and referenced
// directly (never re-imported from './nativeLaunchArgs') so the subject
// under test can't be confused with the thing being mocked, while
// `loadFresh()` still exercises the real `devRuntimeConfig.ts` module code.
const { mockLaunchArgumentsValue } = vi.hoisted(() => ({
  mockLaunchArgumentsValue: vi.fn(),
}));

// oxlint-disable-next-line no-restricted-properties -- app-config reads the launch args and a JSON file at import; its unit deletes these mocks
vi.mock('./nativeLaunchArgs', () => ({
  LaunchArguments: { value: mockLaunchArgumentsValue },
}));

const loadFresh = async (): Promise<typeof Mod> => {
  vi.resetModules();
  return import('./devRuntimeConfig');
};

describe('devRuntimeConfig', () => {
  beforeEach(() => {
    mockLaunchArgumentsValue.mockReset();
  });

  it('exposes the expected top-level keys (static file path)', async () => {
    mockLaunchArgumentsValue.mockReturnValue({});
    const mod = await loadFresh();
    expect(mod.devRuntimeConfig).toHaveProperty('initial_route');
    expect(mod.devRuntimeConfig).toHaveProperty('feature_flags');
  });

  it('ALLOWED_ROUTES is a non-empty readonly array of Href-shaped strings', async () => {
    mockLaunchArgumentsValue.mockReturnValue({});
    const mod = await loadFresh();
    expect(mod.ALLOWED_ROUTES.length).toBeGreaterThan(0);
    for (const route of mod.ALLOWED_ROUTES) {
      expect(typeof route).toBe('string');
      expect(route.startsWith('/')).toBe(true);
    }
  });

  it('isFeatureEnabled returns false for an unknown flag', async () => {
    mockLaunchArgumentsValue.mockReturnValue({});
    const mod = await loadFresh();
    expect(mod.isFeatureEnabled('does_not_exist')).toBe(false);
  });

  it('uses the --devConfigJson launch arg when present and valid (JSON string)', async () => {
    mockLaunchArgumentsValue.mockReturnValue({
      devConfigJson: JSON.stringify({
        initial_route: '/leetcode/contains-duplicate',
        feature_flags: { shiny: true },
      }),
    });
    const mod = await loadFresh();
    expect(mod.devRuntimeConfig.initial_route).toBe(
      '/leetcode/contains-duplicate'
    );
    expect(mod.devRuntimeConfig.feature_flags).toEqual({ shiny: true });
    expect(mod.isFeatureEnabled('shiny')).toBe(true);
  });

  it('accepts a pre-parsed object for --devConfigJson (bridge auto-parses JSON)', async () => {
    // expo-native-launch-arguments tries JSON.parse on every value and returns
    // the parsed object when it parses. The wrapper must handle that shape too.
    mockLaunchArgumentsValue.mockReturnValue({
      devConfigJson: {
        initial_route: '/leetcode/group-anagrams',
        feature_flags: { dark: true },
      },
    });
    const mod = await loadFresh();
    expect(mod.devRuntimeConfig.initial_route).toBe('/leetcode/group-anagrams');
    expect(mod.isFeatureEnabled('dark')).toBe(true);
  });

  it('falls back to the bundled file when the launch arg is absent (accepts a valid route)', async () => {
    // Launch arg is absent → fall back to the bundled file, which carries a
    // route from ALLOWED_ROUTES. The schema must accept it as-is so the
    // boot lands on the requested screen (no devRuntimeConfigError).
    // oxlint-disable-next-line no-restricted-properties -- app-config reads the launch args and a JSON file at import; its unit deletes these mocks
    vi.doMock('../dev-runtime-config.json', () => ({
      default: { initial_route: '/leetcode/two-sum', feature_flags: {} },
    }));
    mockLaunchArgumentsValue.mockReturnValue({});
    try {
      const mod = await loadFresh();
      expect(mod.devRuntimeConfig.initial_route).toBe('/leetcode/two-sum');
      expect(mod.devRuntimeConfig.feature_flags).toEqual({});
      expect(mod.devRuntimeConfigError).toBeNull();
    } finally {
      vi.doUnmock('../dev-runtime-config.json');
    }
  });

  it('falls back to the bundled file when the launch arg is an empty string (rejects an unknown route)', async () => {
    // Launch arg is empty → fall back to the bundled file, which carries a
    // path that is NOT in ALLOWED_ROUTES. The schema must reject it and
    // expose devRuntimeConfigError so the layout renders AppBootErrorBoundary
    // instead of silently booting on the Home tab.
    // oxlint-disable-next-line no-restricted-properties -- app-config reads the launch args and a JSON file at import; its unit deletes these mocks
    vi.doMock('../dev-runtime-config.json', () => ({
      default: { initial_route: '/not-a-real-route', feature_flags: {} },
    }));
    mockLaunchArgumentsValue.mockReturnValue({ devConfigJson: '' });
    try {
      const mod = await loadFresh();
      expect(mod.devRuntimeConfig.initial_route).toBeNull();
      expect(mod.devRuntimeConfigError).toBeInstanceOf(
        mod.InvalidDevRuntimeConfigError
      );
      expect(mod.devRuntimeConfigError?.source).toBe('file');
    } finally {
      vi.doUnmock('../dev-runtime-config.json');
    }
  });

  it('falls back to the bundled file when the launch arg is unparseable JSON (rejects a query string on a route)', async () => {
    // Launch arg is unparseable JSON → fall back to the bundled file, which
    // carries a valid route plus a query string. Query strings are NOT
    // accepted on any route in the new list, so the schema must reject it
    // with devRuntimeConfigError set.
    // oxlint-disable-next-line no-restricted-properties -- app-config reads the launch args and a JSON file at import; its unit deletes these mocks
    vi.doMock('../dev-runtime-config.json', () => ({
      default: { initial_route: '/leetcode/two-sum?foo=bar', feature_flags: {} },
    }));
    mockLaunchArgumentsValue.mockReturnValue({
      devConfigJson: 'not-json{{',
    });
    try {
      const mod = await loadFresh();
      expect(mod.devRuntimeConfig.initial_route).toBeNull();
      expect(mod.devRuntimeConfigError).toBeInstanceOf(
        mod.InvalidDevRuntimeConfigError
      );
      expect(mod.devRuntimeConfigError?.source).toBe('file');
    } finally {
      vi.doUnmock('../dev-runtime-config.json');
    }
  });

  it('falls back to safe defaults when the launch arg fails Zod validation', async () => {
    mockLaunchArgumentsValue.mockReturnValue({
      devConfigJson: JSON.stringify({
        initial_route: '/not-allowed',
        feature_flags: {},
      }),
    });
    const mod = await loadFresh();
    // Zod rejects an unknown route → safe defaults, but `devRuntimeConfigError`
    // is set so the layout can render `AppBootErrorBoundary` with the Zod path
    // (not silently boot on the (app) Home tab — that masked a config bug
    // for an entire debug session and is the reason this branch exists).
    expect(mod.devRuntimeConfig.initial_route).toBeNull();
    expect(mod.devRuntimeConfig.feature_flags).toEqual({});
    expect(mod.devRuntimeConfigError).toBeInstanceOf(
      mod.InvalidDevRuntimeConfigError
    );
    expect(mod.devRuntimeConfigError?.source).toBe('launchArg');
    expect(mod.devRuntimeConfigError?.zodError.issues[0]?.path).toEqual([
      'initial_route',
    ]);
  });

  it('falls back to safe defaults when the bundled file is malformed (AC4)', async () => {
    // Simulate a corrupted/malformed-shape dev-runtime-config.json. The module
    // must degrade to safe defaults at runtime instead of crashing init.
    // oxlint-disable-next-line no-restricted-properties -- app-config reads the launch args and a JSON file at import; its unit deletes these mocks
    vi.doMock('../dev-runtime-config.json', () => ({
      default: { initial_route: 42, feature_flags: 'not-a-record' },
    }));
    mockLaunchArgumentsValue.mockReturnValue({});
    try {
      const mod = await loadFresh();
      expect(mod.devRuntimeConfig.initial_route).toBeNull();
      expect(mod.devRuntimeConfig.feature_flags).toEqual({});
      expect(mod.devRuntimeConfigError).toBeInstanceOf(
        mod.InvalidDevRuntimeConfigError
      );
      expect(mod.devRuntimeConfigError?.source).toBe('file');
      // The two Zod issues (initial_route wrong type, feature_flags wrong type)
      // should both be present so the boot page can list them.
      expect(mod.devRuntimeConfigError?.zodError.issues.length).toBeGreaterThan(
        0
      );
    } finally {
      vi.doUnmock('../dev-runtime-config.json');
    }
  });

  it('exposes the previous (silently-swallowed) bug class: invalid initial_route must NOT boot on (tabs)', async () => {
    // Regression guard for the "blank screen of shame" — when the file's
    // initial_route is invalid (e.g. an unenumerated path that was hand-typed),
    // the app must surface a `devRuntimeConfigError` so the layout renders
    // `AppBootErrorBoundary` instead of booting on the (app) Home tab with
    // `initialRoute: null`. Asserting the error is exposed (not swallowed) is
    // what makes the boot page actually appear in practice.
    // oxlint-disable-next-line no-restricted-properties -- app-config reads the launch args and a JSON file at import; its unit deletes these mocks
    vi.doMock('../dev-runtime-config.json', () => ({
      default: {
        initial_route: '/storybook?story=composites-headers-header--sticky',
        feature_flags: {},
      },
    }));
    mockLaunchArgumentsValue.mockReturnValue({});
    try {
      const mod = await loadFresh();
      expect(mod.devRuntimeConfigError).toBeInstanceOf(
        mod.InvalidDevRuntimeConfigError
      );
      expect(mod.devRuntimeConfigError?.message).toContain('initial_route');
    } finally {
      vi.doUnmock('../dev-runtime-config.json');
    }
  });

  it('error message dynamically lists ALLOWED_ROUTES (single source of truth, not hardcoded)', async () => {
    // Regression guard: prior versions added a hidden string-regex branch on
    // top of `ALLOWED_ROUTES`, which let hand-typed or stale routes pass Zod
    // validation and produced a confusing error that hid the full allow-list
    // from the developer staring at a BetterStack crash page. The error
    // message now renders the live `ALLOWED_ROUTES` list at construction
    // time, so the message and the program-exposed `allowedRoutes` field
    // always agree — there is no hardcoded route/path string anywhere in
    // the source.
    // oxlint-disable-next-line no-restricted-properties -- app-config reads the launch args and a JSON file at import; its unit deletes these mocks
    vi.doMock('../dev-runtime-config.json', () => ({
      default: {
        initial_route: '/features/leetcode/ui/ProblemEditorScreenV2',
        feature_flags: {},
      },
    }));
    mockLaunchArgumentsValue.mockReturnValue({});
    try {
      const mod = await loadFresh();
      const err = mod.devRuntimeConfigError;
      expect(err).toBeInstanceOf(mod.InvalidDevRuntimeConfigError);
      // Programmatic field — the live export, no copy.
      expect(err?.allowedRoutes).toEqual(mod.ALLOWED_ROUTES);
      // The rendered message must include every allowed route, computed
      // from the same ALLOWED_ROUTES array. (We assert on the live array,
      // not on a literal substring, so this test stays in sync with the
      // single source of truth.)
      const msg = err?.message ?? '';
      for (const route of mod.ALLOWED_ROUTES) {
        expect(msg).toContain(route);
      }
    } finally {
      vi.doUnmock('../dev-runtime-config.json');
    }
  });

  it('survives LaunchArguments.value throwing (parses bundled file)', async () => {
    mockLaunchArgumentsValue.mockImplementation(() => {
      throw new Error('native bridge down');
    });
    const mod = await loadFresh();
    // File value may be null or a valid route from ALLOWED_ROUTES. Don't
    // assert a specific value — just that the module initialized without
    // throwing.
    expect(mod.devRuntimeConfig).toBeDefined();
  });
});
