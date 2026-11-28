/// <reference types="vite/client" />
import type { ComponentType } from 'react';

import { createFlowApp } from '@lib/flow-test-harness';

import { SkinRoot } from '../../app-shell/skin/SkinRoot';

export const app = createFlowApp({
  routes: import.meta.glob<{ default: ComponentType }>('../../app/**/*.tsx'),
  routesRoot: '../../app/',
  // these routes mount without the root layout, which is where the app provides its skin
  wrapper: SkinRoot,
});