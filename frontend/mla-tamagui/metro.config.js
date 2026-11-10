const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { withTamagui } = require('@tamagui/metro-plugin');
const { getDefaultConfig } = require('expo/metro-config');

const config = getDefaultConfig(__dirname, { isCSSEnabled: true });

// The transform cache does not key on inlined EXPO_PUBLIC_* values: an export reused modules folded with the
// last build's values (the lab flag, the backend token). Hashed, as some values are credentials.
const publicEnv = Object.keys(process.env)
  .filter((key) => key.startsWith('EXPO_PUBLIC_'))
  .sort()
  .map((key) => `${key}=${process.env[key]}`)
  .join('\n');
config.cacheVersion = `${config.cacheVersion ?? ''}|public-env:${crypto.createHash('sha256').update(publicEnv).digest('hex')}`;

// Metro's default store is os.tmpdir(), which is gone on a container recreate; under node_modules a kept checkout
// starts warm.
const { FileStore } = require('@expo/metro-config/build/binary-file-store');
config.cacheStores = [new FileStore({ root: path.join(__dirname, 'node_modules', '.cache', 'app-build', 'metro') })];

// pnpm links workspace packages as symlinks; Metro must follow them.
config.resolver.unstable_enableSymlinks = true;
config.resolver.unstable_enablePackageExports = true;

// Shrinks Metro's crawl/watch surface for this monorepo -- see
// mobile-learning-app/metro.config.js's matching block for the full
// rationale (why `dist/**` and nested node_modules are deliberately left
// unblocked) and the `fs.inotify` pressure this addresses.
// Anchored deliberately. An unanchored /\/ios\/.*/ matches any path with an
// `ios` segment, and react-native-screens 4.26 (Expo SDK 57's pin) keeps real
// sources in `src/components/gamma/stack/header/ios/`, so the broad form
// blocked a library's own files and the iOS/Android bundle failed outright
// with "Unable to resolve module ./ios/StackHeaderItemSpacer.ios". Web never
// noticed, which is exactly the browser-passes/Expo-Go-fails shape. The block
// still has to cover a package's TOP-LEVEL ios/ and android/ trees, though --
// @shopify/react-native-skia's android/ alone is enough fs.inotify pressure to
// make `expo start` die on startup -- so match those by position instead.
const escapeForRegExp = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const sep = escapeForRegExp(path.sep);
const ownDir = escapeForRegExp(__dirname + path.sep);
config.resolver.blockList = [
  new RegExp(`^${ownDir}ios${sep}.*`),
  new RegExp(`^${ownDir}android${sep}.*`),
  new RegExp(`${sep}node_modules${sep}(@[^${sep}]+${sep})?[^${sep}]+${sep}(ios|android)${sep}.*`),
  /\/\.git\/.*/,
];
config.maxWorkers = process.env.METRO_MAX_WORKERS
  ? Number(process.env.METRO_MAX_WORKERS)
  : Math.min(4, os.cpus().length);

/**
 * Workspace ESM packages import source TypeScript with `.js` extensions
 * (e.g. `./logger/Logger.js` from `frontend/libraries/logging/src/index.ts`). Metro
 * does not map `.js` to `.ts` on its own, so try the TypeScript extensions
 * when the import comes from a workspace source file.
 *
 * Ported from mobile-learning-app's metro.config.js. Omitting it is not a
 * subtle degradation: any import of @lib/logging fails to bundle outright.
 */
function resolveJsToTs(context, moduleName) {
  if (
    !moduleName.endsWith('.js') ||
    !context.originModulePath ||
    context.originModulePath.includes('node_modules')
  ) {
    return undefined;
  }
  const candidatePath = path.resolve(path.dirname(context.originModulePath), moduleName);
  for (const ext of ['.ts', '.tsx', '.web.ts', '.web.tsx']) {
    const withExt = candidatePath.replace(/\.js$/, ext);
    if (fs.existsSync(withExt)) return { type: 'sourceFile', filePath: withExt };
  }
  return undefined;
}

/**
 * Metro's condition resolution picks Tamagui's `.mjs` build over its
 * `.native.js` one on native, which surfaces as a runtime failure a web
 * bundle can never show -- i.e. exactly the difference between "works in the
 * browser" and "works in Expo Go". Web keeps the default conditions.
 */
/**
 * Packages that hold process-wide singletons must resolve to ONE copy. pnpm
 * gives the feature packages in features/ an expo-router whose peer hash
 * differs from the app's, so Metro bundled two copies and a feature's
 * `router.push` hit a store whose navigationRef was never attached
 * ("Cannot read properties of undefined (reading 'isReady')").
 */
const SINGLETON_MODULES = ['expo-router'];
function isSingleton(moduleName) {
  return SINGLETON_MODULES.some((name) => moduleName === name || moduleName.startsWith(name + '/'));
}

// @quidone/react-native-wheel-picker's index also exports a DatePicker, and its date-fns barrel is 304 modules (7% of
// the graph, on every platform). The app only uses WheelPicker, so the picker's own date-fns import resolves to nothing.
const WHEEL_PICKER_DIR = `${path.sep}@quidone${path.sep}react-native-wheel-picker${path.sep}`;

config.resolver.resolveRequest = (context, moduleName, platform) => {
  if (moduleName === 'date-fns' && context.originModulePath.includes(WHEEL_PICKER_DIR)) {
    return { type: 'empty' };
  }
  if (isSingleton(moduleName) && !context.originModulePath.startsWith(__dirname + path.sep)) {
    return context.resolveRequest(
      { ...context, originModulePath: path.join(__dirname, 'package.json') },
      moduleName,
      platform
    );
  }

  // Native react-native-webview renders a red unsupported-platform placeholder
  // on web, so the CodeMirror editor would be an empty box there. Route web to
  // an iframe shim and leave iOS/Android on the native package.
  if (platform === 'web' && moduleName === 'react-native-webview') {
    return {
      type: 'sourceFile',
      filePath: path.join(__dirname, 'lib/webview/ReactNativeWebView.web.tsx'),
    };
  }

  try {
    if (platform !== 'web') {
      return context.resolveRequest(
        { ...context, unstable_conditionNames: ['react-native', 'require', 'default'] },
        moduleName,
        platform
      );
    }
    // the static render resolves the app's transpiled imports as requires: Tamagui loaded as CJS and ESM, two contexts,
    // and a screen built from a package's ESM half found no TamaguiProvider, so the server left it to the client
    const tamagui = moduleName === 'tamagui' || moduleName.startsWith('@tamagui/');
    return context.resolveRequest(tamagui ? { ...context, isESMImport: true } : context, moduleName, platform);
  } catch (error) {
    const tsResolution = resolveJsToTs(context, moduleName);
    if (tsResolution) return tsResolution;
    throw error;
  }
};

// Expo's default evaluates every module in the bundle at start-up (`inlineRequires: false`). With it on, a module
// runs on first use, so the shell no longer builds reanimated, skia, charts and markdown before they are needed.
config.transformer.getTransformOptions = async () => ({
  transform: { experimentalImportSupport: true, inlineRequires: true },
});

module.exports = withTamagui(config, {
  components: ['tamagui', '@lib/tamagui-design-system'],
  config: './tamagui.config.ts',
  outputCSS: './tamagui-web.css',
});
