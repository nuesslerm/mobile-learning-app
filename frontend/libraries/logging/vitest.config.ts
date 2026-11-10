import { defineConfig } from 'vitest/config';

// Vitest requires a default export for its config file.
// eslint-disable-next-line import/no-default-export
export default defineConfig({
  test: {
    include: ['tests/**/*.test.{ts,tsx}'],
    // Default to node; React tests opt in to jsdom via
    // `// @vitest-environment jsdom` at the top of their file.
    environment: 'node',
  },
});
