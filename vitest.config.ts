import { defineConfig } from 'vitest/config';
import babel from 'vite-plugin-babel';

// Root vitest config: library tests with no config of their own.
export default defineConfig({
  plugins: [
    babel({
      babelConfig: { configFile: './babel.config.cjs' },
    }),
  ],
  test: {
    include: [
      'libraries/**/*.test.{ts,tsx}',
      'frontend/libraries/**/*.test.{ts,tsx}',
      'scripts/**/*.test.mjs',
    ],
    environment: 'node',
  },
});
