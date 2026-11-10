// Renders a route file's screen inside its _layout.tsx chain the way
// expo-router mounts app/: seeds the router stub's params/pathname and builds a
// chain where layout i's Stack/Slot renders layout i+1, then the screen.
import { type RenderResult } from '@testing-library/react';
import type { ComponentType, ReactElement, ReactNode } from 'react';

import { prerenderScreen, renderScreen, type RenderScreenOptions } from './render';
import type { RouterCall } from './router';

export interface RenderRouteOptions extends RenderScreenOptions {
  layouts: ComponentType[];
  screen: ComponentType;
  params?: Record<string, string>;
  pathname?: string;
  wrapper?: ComponentType<{ children: ReactNode }>;
}

interface FlowRouterState {
  calls: RouterCall[];
  params: Record<string, string | undefined>;
  pathname?: string;
  screen?: ComponentType;
}

const g = globalThis as { __flowRouter?: FlowRouterState };

function state(): FlowRouterState {
  g.__flowRouter ??= { calls: [], params: {} };
  return g.__flowRouter;
}

function buildRoute(opts: RenderRouteOptions): { root: ReactElement; rest: Omit<RenderRouteOptions, 'layouts' | 'screen' | 'params' | 'pathname' | 'wrapper'> } {
  const { layouts, screen, params, pathname, wrapper: Wrapper, ...rest } = opts;
  const s = state();
  s.params = params ?? {};
  s.pathname = pathname;

  const chain = (index: number): ComponentType =>
    function RouteChain() {
      s.screen = index + 1 < layouts.length ? chain(index + 1) : screen;
      const Layout = layouts[index];
      return <Layout />;
    };

  const Screen = screen;
  let root: ReactElement;
  if (layouts.length === 0) {
    s.screen = Screen;
    root = <Screen />;
  } else {
    s.screen = layouts.length > 1 ? chain(1) : Screen;
    const Outer = layouts[0];
    root = <Outer />;
  }

  return { root: Wrapper ? <Wrapper>{root}</Wrapper> : root, rest };
}

export function renderRoute(opts: RenderRouteOptions): RenderResult {
  const { root, rest } = buildRoute(opts);
  return renderScreen(root, rest);
}

/** What the server renders when it is not the page the browser then shows: the template page of a dynamic route. */
export type ServerPage = { params: Record<string, string>; pathname: string };

export function prerenderRoute(opts: RenderRouteOptions, server?: ServerPage): Promise<RenderResult> {
  const { rest } = buildRoute(opts);
  const serverUi = server ? () => buildRoute({ ...opts, ...server }).root : undefined;
  return prerenderScreen(() => buildRoute(opts).root, rest, serverUi);
}