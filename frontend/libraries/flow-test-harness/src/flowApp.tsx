// Builds an app object that renders a path against a routes map (lazy loaders
// or eager modules) the way expo-router would: group stripping, param capture,
// layout chain, overrides. Only matched keys are loaded.
import { DependencyOverridesProvider } from '@lib/app-di';
import {
  act,
  renderHook as renderHookBase,
  type RenderHookResult,
} from '@testing-library/react';
import type { ComponentType, ReactNode } from 'react';

import { createSession, type FlowSession } from './interactions/session';
import { FlowProviders } from './render';
import { prerenderRoute, renderRoute, type ServerPage } from './route';

type FlowRenderOptions = {
  overrides?: Record<string, object>;
  theme?: 'light' | 'dark';
  search?: Record<string, string>;
  /**
   * Server-render first and hydrate after the page has loaded, as the static web export does. Default true. 'export'
   * renders the page the export holds for the path (its `generateStaticParams` page, else the template; no query).
   */
  prerender?: boolean | 'export';
};

type FlowRenderHookOptions = {
  overrides?: Record<string, object>;
};

export type FlowApp = {
  render(path: string, opts?: FlowRenderOptions): Promise<FlowSession>;
  renderHook<R>(
    hook: () => R,
    opts?: FlowRenderHookOptions
  ): RenderHookResult<R, unknown>;
};

type StaticParams = Record<string, string | string[]>;
type RouteModule = {
  default: ComponentType;
  generateStaticParams?: (props: { params?: StaticParams }) => StaticParams[];
};
type RouteLoader = () => Promise<RouteModule>;
type RouteValue = RouteLoader | RouteModule;

export type CreateFlowAppConfig = {
  routes: Record<string, RouteValue>;
  routesRoot: string;
  basePath?: string;
  includeRootLayout?: boolean;
};

type ParsedRoute = {
  isLayout: boolean;
  directory: string[];
  pattern: string[];
  module: RouteValue;
};

const isGroup = (segment: string): boolean =>
  segment.startsWith('(') && segment.endsWith(')');
const isDynamic = (segment: string): boolean =>
  segment.startsWith('[') && segment.endsWith(']');

function parseRoute(
  key: string,
  routesRoot: string,
  module: RouteValue
): ParsedRoute {
  const relative = key.slice(routesRoot.length);
  const withoutExt = relative.endsWith('.tsx')
    ? relative.slice(0, -4)
    : relative;
  const segments = withoutExt.split('/').filter(Boolean);
  const directory = segments.slice(0, -1);
  const last = segments[segments.length - 1];
  const effectiveDirectory = directory.filter((segment) => !isGroup(segment));
  const pattern =
    last === '_layout' || last === 'index'
      ? effectiveDirectory
      : [...effectiveDirectory, last];
  return { isLayout: last === '_layout', directory, pattern, module };
}

function matchPattern(
  pattern: string[],
  path: string[]
): Record<string, string> | null {
  if (pattern.length !== path.length) return null;
  const params: Record<string, string> = {};
  for (let i = 0; i < pattern.length; i++) {
    const segment = pattern[i];
    if (isDynamic(segment)) {
      params[segment.slice(1, -1)] = path[i];
    } else if (segment !== path[i]) {
      return null;
    }
  }
  return params;
}

// Sort comparator: static segments beat dynamic ones, left to right.
function staticFirst(a: string[], b: string[]): number {
  for (let i = 0; i < Math.min(a.length, b.length); i++) {
    const diff = (isDynamic(a[i]) ? 0 : 1) - (isDynamic(b[i]) ? 0 : 1);
    if (diff !== 0) return -diff;
  }
  return b.length - a.length;
}

function isPrefix(prefix: string[], segments: string[]): boolean {
  return (
    prefix.length <= segments.length &&
    prefix.every((segment, i) => segment === segments[i])
  );
}

type Resolved = {
  screen: ParsedRoute;
  layouts: ParsedRoute[];
  params: Record<string, string>;
};

function resolveRoute(
  routes: ParsedRoute[],
  path: string[],
  includeRootLayout: boolean
): Resolved | null {
  const candidates = routes
    .filter((route) => !route.isLayout)
    .map((route) => ({ route, params: matchPattern(route.pattern, path) }))
    .filter(
      (
        candidate
      ): candidate is { route: ParsedRoute; params: Record<string, string> } =>
        candidate.params !== null
    );
  if (candidates.length === 0) return null;
  candidates.sort((a, b) => staticFirst(a.route.pattern, b.route.pattern));
  const best = candidates[0];
  const layouts = routes
    .filter(
      (route) =>
        route.isLayout && isPrefix(route.directory, best.route.directory)
    )
    .sort((a, b) => a.directory.length - b.directory.length)
    .filter((route) => includeRootLayout || route.directory.length > 0);
  return { screen: best.route, layouts, params: best.params };
}

async function loadModule(value: RouteValue): Promise<RouteModule> {
  return typeof value === 'function' ? value() : value;
}

export function createFlowApp(cfg: CreateFlowAppConfig): FlowApp {
  const routes = Object.entries(cfg.routes).map(([key, module]) =>
    parseRoute(key, cfg.routesRoot, module)
  );
  const includeRootLayout = cfg.includeRootLayout ?? false;

  const splitPath = (path: string): string[] => {
    const rest =
      cfg.basePath && path.startsWith(cfg.basePath)
        ? path.slice(cfg.basePath.length)
        : path;
    return rest.split('/').filter(Boolean);
  };

  const normalizePathname = (path: string): string => {
    const trimmed = path.replace(/^\/+/, '').replace(/\/+$/, '');
    return trimmed ? `/${trimmed}` : '/';
  };

  // The page the export holds for a path: the route's own when `generateStaticParams` lists its params, else the template.
  const exportedPage = (resolved: Resolved, mod: RouteModule, path: string): ServerPage => {
    const { pattern } = resolved.screen;
    const names = pattern.filter(isDynamic).map((segment) => segment.slice(1, -1));
    const listed = mod.generateStaticParams?.({ params: {} }) ?? [];
    if (names.length === 0 || listed.some((entry) => names.every((name) => entry[name] === resolved.params[name]))) {
      return { params: resolved.params, pathname: normalizePathname(path) };
    }
    const base = cfg.basePath && path.startsWith(cfg.basePath) ? cfg.basePath : '';
    const segments = splitPath(path).map((segment, i) => (isDynamic(pattern[i] ?? '') ? pattern[i] : segment));
    return {
      params: Object.fromEntries(names.map((name) => [name, `[${name}]`])),
      pathname: normalizePathname(`${base}/${segments.join('/')}`),
    };
  };

  return {
    async render(path, opts: FlowRenderOptions = {}) {
      const resolved = resolveRoute(routes, splitPath(path), includeRootLayout);
      if (!resolved) throw new Error(`createFlowApp: no route for ${path}`);
      const [screen, layouts] = await Promise.all([
        loadModule(resolved.screen.module),
        Promise.all(resolved.layouts.map((route) => loadModule(route.module))),
      ]);
      const Wrapper = ({ children }: { children: ReactNode }) => (
        <DependencyOverridesProvider overrides={opts.overrides ?? {}}>
          {children}
        </DependencyOverridesProvider>
      );
      const routeOpts = {
        layouts: layouts.map((route) => route.default),
        screen: screen.default,
        params: { ...resolved.params, ...opts.search },
        pathname: normalizePathname(path),
        theme: opts.theme,
        wrapper: Wrapper,
      };
      const prerender = opts.prerender ?? true;
      if (prerender) {
        const server = prerender === 'export' ? exportedPage(resolved, screen, path) : undefined;
        return act(async () => createSession(await prerenderRoute(routeOpts, server)));
      }
      return act(() => createSession(renderRoute(routeOpts)));
    },
    renderHook<R>(hook: () => R, opts: FlowRenderHookOptions = {}) {
      return renderHookBase(hook, {
        wrapper: ({ children }) => (
          <FlowProviders>
            <DependencyOverridesProvider overrides={opts.overrides ?? {}}>
              {children}
            </DependencyOverridesProvider>
          </FlowProviders>
        ),
      });
    },
  };
}
