import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import babel from 'vite-plugin-babel';
import { configDefaults, defineConfig } from 'vitest/config';

import { featurePackages } from './features/packages.mjs';

const APP_ROOT = path.dirname(fileURLToPath(import.meta.url));

const reactNativeStubDir = fs.mkdtempSync(
  path.join(os.tmpdir(), 'vitest-rn-stub-')
);
const reactNativeStubFile = path.join(reactNativeStubDir, 'index.js');
fs.writeFileSync(
  reactNativeStubFile,
  `// Auto-generated stub: see vitest.config.ts for the rationale.
'use strict';
const StyleSheet = {
  create: (s) => s,
  flatten: (s) => s,
  hairlineWidth: 0.5,
};
module.exports = {
  Platform: { OS: 'ios', select: (obj) => obj.ios ?? obj.default },
  StyleSheet,
  View: 'View',
  ScrollView: 'ScrollView',
  Text: 'Text',
  TextInput: 'TextInput',
  Pressable: 'Pressable',
  TouchableOpacity: 'TouchableOpacity',
  Image: 'Image',
  ActivityIndicator: 'ActivityIndicator',
  FlatList: 'FlatList',
  SectionList: 'SectionList',
  VirtualizedList: 'VirtualizedList',
  Modal: 'Modal',
  RefreshControl: 'RefreshControl',
  Switch: 'Switch',
  KeyboardAvoidingView: 'KeyboardAvoidingView',
  SafeAreaView: 'SafeAreaView',
  StatusBar: 'StatusBar',
  Animated: {
    View: 'Animated.View',
    Text: 'Animated.Text',
    ScrollView: 'Animated.ScrollView',
    Image: 'Animated.Image',
    createAnimatedComponent: (c) => c,
  },
  Easing: { inOut: () => () => 0, ease: () => () => 0, linear: () => () => 0 },
  Dimensions: { get: () => ({ width: 320, height: 640, scale: 2, fontScale: 1 }) },
  PixelRatio: { get: () => 2, getFontScale: () => 1, getPixelSizeForLayoutSize: (n) => n * 2 },
  NativeModules: {},
  requireNativeComponent: (n) => n,
  findNodeHandle: () => null,
  unstable_batchedUpdates: (fn) => fn(),
  LogBox: { ignoreLogs: () => {} },
  Appearance: { getColorScheme: () => 'light', addChangeListener: () => ({ remove: () => {} }) },
  AppState: { currentState: 'active', addEventListener: () => ({ remove: () => {} }) },
  Keyboard: { addListener: () => ({ remove: () => {} }), dismiss: () => {} },
  Linking: { openURL: () => Promise.resolve(), addEventListener: () => ({ remove: () => {} }) },
  Share: { share: () => Promise.resolve() },
  Alert: { alert: () => {} },
  ToastAndroid: { show: () => {}, SHORT: 0, LONG: 1, BOTTOM: 0, CENTER: 1, TOP: 2 },
  I18nManager: { isRTL: false, allowRTL: () => {}, forceRTL: () => {} },
  AccessibilityInfo: { isReduceMotionEnabled: () => Promise.resolve(false), isScreenReaderEnabled: () => Promise.resolve(false), addEventListener: () => ({ remove: () => {} }) },
};
`,
  'utf8'
);

const lucideStubDir = fs.mkdtempSync(
  path.join(os.tmpdir(), 'vitest-lucide-stub-')
);
const lucideStubFile = path.join(lucideStubDir, 'index.js');
fs.writeFileSync(
  lucideStubFile,
  `// Auto-generated stub: see vitest.config.ts for the rationale.
'use strict';
const lucideHandler = {
  get: (_target, name) => {
    if (name === '__esModule') return true;
    if (name === 'default') return 'LucideIcon';
    if (name === 'useLucideContext') return () => ({ size: 24, color: 'currentColor', strokeWidth: 2, absoluteStrokeWidth: false });
    if (name === 'LucideProvider') return (props) => (props && props.children);
    return 'Lucide_' + String(name);
  },
};
module.exports = new Proxy({}, lucideHandler);
`,
  'utf8'
);

export default defineConfig({
  resolve: {
    dedupe: ['@testing-library/react-native', 'react-test-renderer', 'react'],
    alias: [
      { find: '@', replacement: APP_ROOT },
      { find: /^react-native$/, replacement: reactNativeStubFile },
      { find: /^lucide-react-native$/, replacement: lucideStubFile },
    ],
  },
  plugins: [
    babel({
      babelConfig: { configFile: './babel.config.js' },
    }),
  ],
  test: {
    // feature packages nested in features/ run their own tests
    exclude: [
      ...configDefaults.exclude,
      ...featurePackages().map((dir) => `${dir}/**`),
    ],
    include: [
      'features/**/*.test.{ts,tsx}',
      'hooks/**/*.test.{ts,tsx}',
      'components/**/*.test.{ts,tsx}',
      'app-shell/**/*.test.{ts,tsx}',
      'config/**/*.test.{ts,tsx}',
      'lib/**/*.test.{ts,tsx}',
      'tests/unit/**/*.test.{ts,tsx}',
      'scripts/**/*.test.{ts,tsx,mjs}',
    ],
    environment: 'node',
    setupFiles: ['./vitest.setup.ts'],
    alias: [
      { find: /^react-native$/, replacement: reactNativeStubFile },
      { find: /^lucide-react-native$/, replacement: lucideStubFile },
    ],
    server: {
      deps: {
        inline: [
          /[\\/]node_modules[\\/](?:\.pnpm[\\/])?(?:@testing-library[+/]react-native|react-native|lucide-react-native)(?:[@\\/]|$)/,
        ],
      },
    },
  },
});
