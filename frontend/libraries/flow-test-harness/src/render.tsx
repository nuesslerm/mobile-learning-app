// Renders a screen inside the same provider stack the app root uses
// (TamaguiProvider + PortalProvider), on top of @testing-library/react.
import {
  PortalProvider,
  TamaguiProvider,
} from '@lib/tamagui-design-system/primitives';
import { tamaguiConfig } from '@lib/tamagui-design-system/theme';
import {
  render,
  type RenderOptions,
  type RenderResult,
} from '@testing-library/react';
import type { ReactElement, ReactNode } from 'react';
import { renderToString } from 'react-dom/server';

export type RenderScreenOptions = {
  theme?: 'light' | 'dark';
} & Omit<RenderOptions, 'wrapper'>;

// FLOW_THEME lets a golden run render the app's dark theme; the colour scheme alone only flips media queries.
const defaultTheme = (): 'light' | 'dark' =>
  process.env.FLOW_THEME === 'dark' ? 'dark' : 'light';

export function FlowProviders({
  children,
  theme = defaultTheme(),
}: {
  children: ReactNode;
  theme?: 'light' | 'dark';
}) {
  return (
    <TamaguiProvider config={tamaguiConfig} defaultTheme={theme}>
      <PortalProvider shouldAddRootHost>{children}</PortalProvider>
    </TamaguiProvider>
  );
}

export function renderScreen(
  ui: ReactElement,
  options: RenderScreenOptions = {}
): RenderResult {
  const { theme, ...rest } = options;
  return render(ui, {
    ...rest,
    wrapper: ({ children }) => (
      <FlowProviders theme={theme}>{children}</FlowProviders>
    ),
  });
}

// The web export ships server-rendered HTML that the browser loads (iframes included) before hydration attaches a single listener.
export async function prerenderScreen(
  ui: ReactElement | (() => ReactElement),
  options: RenderScreenOptions = {},
  serverUi: ReactElement | (() => ReactElement) = ui
): Promise<RenderResult> {
  // A factory is rebuilt for hydration: route chains record router state while they render.
  const build = typeof ui === 'function' ? ui : () => ui;
  const buildServer = typeof serverUi === 'function' ? serverUi : () => serverUi;
  const { theme, ...rest } = options;
  const container = document.body.appendChild(document.createElement('div'));
  container.innerHTML = renderToString(
    <FlowProviders theme={theme}>{buildServer()}</FlowProviders>
  );
  // Like a browser: prerendered frames finish loading (and post what they post) before the app's JS hydrates.
  const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
  for (let i = 0; i < 200; i += 1) {
    await tick();
    const frames = Array.from(container.querySelectorAll('iframe'));
    if (frames.every((f) => f.contentDocument?.readyState === 'complete'))
      break;
  }
  await tick();
  await tick();
  return render(build(), {
    // Queries span the body, as an unprerendered render's do: portaled UI (keyboard toolbars, sheets) mounts there.
    baseElement: document.body,
    ...rest,
    container,
    hydrate: true,
    wrapper: ({ children }) => (
      <FlowProviders theme={theme}>{children}</FlowProviders>
    ),
  });
}
