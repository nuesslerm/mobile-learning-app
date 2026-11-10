import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['tests/bench/**/*.benchmark.test.ts'],
    globalSetup: ['tests/blackbox/global-setup.ts'],
    testTimeout: 30 * 60 * 1000,
    hookTimeout: 120_000,
    pool: 'forks',
    fileParallelism: false,
  },
});
