import type { ReactElement } from 'react';

import type { HeaderMenuSpec } from './header-menu-spec';

export type GlassMenuGroupRenderer = (
  menus: readonly HeaderMenuSpec[],
  scheme: 'light' | 'dark',
  hideBackground: boolean
) => ReactElement | undefined;

export const renderGlassMenuGroup: GlassMenuGroupRenderer = () => undefined;
