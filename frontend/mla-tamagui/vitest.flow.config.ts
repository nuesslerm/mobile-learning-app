// Screen-level flow tests (`{src,tests}/**/*.flow.test.{ts,tsx}`): whole
// screens rendered through react-native-web in jsdom by @lib/flow-test-harness.
// Run with `pnpm run test:flow`; the plain unit config excludes these files.
import { fileURLToPath } from 'node:url';
import path from 'node:path';

import { flowTestConfig } from '@lib/flow-test-harness/vitest';
import { configDefaults } from 'vitest/config';

import { featurePackages } from './features/packages.mjs';

export default flowTestConfig({
  packageDir: path.dirname(fileURLToPath(import.meta.url)),
  overrides: {
    test: {
      // Also the in-app features' own flow tests; the package features run theirs from their own config.
      include: ['features/*/**/*.flow.test.{ts,tsx}'],
      exclude: [
        ...configDefaults.exclude,
        ...featurePackages().map((dir) => `${dir}/**`),
      ],
    },
  },
  extraAlias: [
    {
      find: '@',
      replacement: path.resolve(path.dirname(fileURLToPath(import.meta.url))),
    },
    {
      // As metro does for web: the app's own iframe WebView, so flow tests run the real CodeMirror page.
      find: /^react-native-webview$/,
      replacement: path.resolve(
        path.dirname(fileURLToPath(import.meta.url)),
        'lib/webview/ReactNativeWebView.web.tsx'
      ),
    },
  ],
});
