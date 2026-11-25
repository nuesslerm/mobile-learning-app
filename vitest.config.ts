import { defineConfig } from 'vitest/config';

// Root vitest config: library tests with no config of their own.
export default defineConfig({
  test: {
    include: [
      'libraries/**/*.test.{ts,tsx}',
      'frontend/libraries/**/*.test.{ts,tsx}',
      'scripts/**/*.test.mjs',
    ],
    environment: 'node',
  },
});
