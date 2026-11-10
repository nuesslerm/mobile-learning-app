/// <reference types="vite/client" />
import type { ComponentType } from 'react';

import { createFlowApp } from '@lib/flow-test-harness';

export const appWithRoot = createFlowApp({
  routes: import.meta.glob<{ default: ComponentType }>('../../app/**/*.tsx'),
  routesRoot: '../../app/',
  includeRootLayout: true,
});