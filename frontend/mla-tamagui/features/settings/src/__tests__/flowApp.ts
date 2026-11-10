/// <reference types="vite/client" />
import { createFlowApp } from '@lib/flow-test-harness';
import type { ComponentType } from 'react';

export const app = createFlowApp({
  routes: import.meta.glob<{ default: ComponentType }>('../routes/**/*.tsx'),
  routesRoot: '../routes/',
  basePath: '/settings',
});