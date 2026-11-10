import path from 'node:path';

import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const DIR = path.resolve(import.meta.dirname ?? '.');

export default defineConfig({
  root: DIR,
  base: '/_mcp/device-frame/',
  plugins: [react()],
  resolve: {
    alias: {
      '@cli/harness-contracts': path.resolve(DIR, 'contracts/src'),
      '@cli/harness-data': path.resolve(DIR, 'data/src'),
      '@cli/harness-use-cases': path.resolve(DIR, 'use-cases/src'),
      '@cli/harness-di': path.resolve(DIR, 'di/src'),
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    modulePreload: false,
    rollupOptions: {
      input: path.resolve(DIR, 'index.html'),
    },
  },
  server: {
    port: 5175,
    strictPort: true,
  },
});
