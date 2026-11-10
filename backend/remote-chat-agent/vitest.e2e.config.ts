import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    include: ['tests/e2e/**/*.e2e.test.ts'],
    // Fails the run when no real opencode is reachable. See
    // tests/e2e/globalSetup.ts — an e2e suite whose dependency is mocked cannot
    // fail on the integration defects it exists to catch.
    globalSetup: ['./tests/e2e/globalSetup.ts'],
    testTimeout: 60000,
    hookTimeout: 30000,
    fileParallelism: false,
    pool: 'forks',
    poolOptions: {
      forks: {
        singleFork: true,
      },
    },
  },
});
