import { defineConfig } from 'vitest/config';
import babel from 'vite-plugin-babel';

// eslint-disable-next-line import/no-default-export
export default defineConfig({
  plugins: [
    babel({
      babelConfig: { configFile: './babel.config.cjs' },
    }),
  ],
  test: {
    include: ['src/**/*.test.{ts,tsx}'],
    environment: 'node',
  },
});
