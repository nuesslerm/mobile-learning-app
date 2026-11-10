import path from 'node:path';
import { fileURLToPath } from 'node:url';

import babel from 'vite-plugin-babel';
import { defineConfig } from 'vitest/config';

const featuresDir = path.dirname(fileURLToPath(import.meta.url));
const shared = (file: string): string => path.join(featuresDir, file);

const stubAliases = [
  {
    find: /^react-native$/,
    replacement: shared('./vitest.react-native.stub.cjs'),
  },
  {
    find: /^lucide-react-native$/,
    replacement: shared('./vitest.lucide.stub.cjs'),
  },
];

// Unit tests for a `@feature/*` package, run from its directory; flow tests have their own config.
export function createFeatureVitestConfig(): ReturnType<typeof defineConfig> {
  return defineConfig({
    plugins: [babel({ babelConfig: { configFile: './babel.config.cjs' } })],
    resolve: { alias: stubAliases },
    test: {
      include: ['src/**/*.test.{ts,tsx}'],
      exclude: ['**/*.flow.test.*', '**/node_modules/**'],
      environment: 'node',
      setupFiles: [shared('./vitest.setup.ts')],
      alias: stubAliases,
      server: {
        deps: {
          inline: [
            /[\\/]node_modules[\\/](?:\.pnpm[\\/])?(?:@testing-library[+/]react-native|react-native|lucide-react-native)(?:[@\\/]|$)/,
          ],
        },
      },
    },
  });
}
