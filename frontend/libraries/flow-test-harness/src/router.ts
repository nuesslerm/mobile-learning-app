// The expo-router stub records navigation on `globalThis.__flowRouter`; this
// module is the typed handle a test uses to seed params and read what the
// screen navigated to.
export type RouterCall = ['push' | 'replace' | 'navigate', string] | ['back'];

interface FlowRouterState {
  calls: RouterCall[];
  params: Record<string, string | undefined>;
  pathname?: string;
  listeners?: Set<() => void>;
}

const g = globalThis as { __flowRouter?: FlowRouterState };

function state(): FlowRouterState {
  g.__flowRouter ??= { calls: [], params: {} };
  return g.__flowRouter;
}

export interface RouterHarness {
  /** Seed what `useLocalSearchParams()` returns; a mounted screen re-renders with them, as when expo-router follows a link
   * to the same route with other params (the screen is updated, not remounted). Wrap the call in `act`. */
  setParams(params: Record<string, string | undefined>): void;
  setPathname(pathname: string): void;
  /** Every router call so far, oldest first. */
  calls(): RouterCall[];
  /** Hrefs passed to `router.push`, oldest first. */
  pushes(): string[];
  lastPush(): string | undefined;
  reset(): void;
}

export function routerHarness(): RouterHarness {
  return {
    setParams: (params) => {
      state().params = params;
      state().listeners?.forEach((notify) => notify());
    },
    setPathname: (pathname) => {
      state().pathname = pathname;
    },
    calls: () => [...state().calls],
    pushes: () => state().calls.filter((c) => c[0] === 'push').map((c) => c[1] as string),
    lastPush: () => state().calls.filter((c) => c[0] === 'push').map((c) => c[1] as string).at(-1),
    reset: resetRouterHarness,
  };
}

export function resetRouterHarness(): void {
  g.__flowRouter = { calls: [], params: {} };
}
