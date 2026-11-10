import { defineConfig } from 'vitest/config';

// The service benchmark: one fresh process per run against the implementation LEETCODE_BACKEND_CMD names (default: the release build).
export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    include: ['tests/bench/**/*.benchmark.test.ts'],
    globalSetup: ['tests/bench/global-setup.ts'],
    testTimeout: 120_000,
    hookTimeout: 30_000,
  },
});
