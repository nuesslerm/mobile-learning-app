// Vitest configuration for screen-level flow tests.
//
// A flow test renders a whole screen the way the web export renders it:
// react-native resolves to react-native-web inside jsdom, Tamagui runs its web
// build, and the handful of native-only packages the design system pulls in
// (reanimated, bottom-sheet, safe-area-context, markdown, lucide, expo-router,
// async-storage, svg) resolve to small stubs that keep the component tree honest
// (real DOM, real testIDs, real text) without a native runtime.
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

import { defineConfig, mergeConfig, type UserConfig } from 'vitest/config';

import { FlowReporter } from './flowReporter';

const here = path.dirname(fileURLToPath(import.meta.url));
const stub = (name: string): string =>
  path.resolve(here, '..', 'stubs', `${name}.cjs`);

const DRAWER_STUB = path.resolve(here, '..', 'stubs', 'expo-router-drawer.mjs');

export const FLOW_TEST_GLOB = '{src,tests}/**/*.flow.test.{ts,tsx}';

export interface FlowTestConfigOptions {
  /** Absolute directory of the package under test (its vitest root). */
  packageDir: string;
  /** Extra `{ find, replacement }` aliases, checked before the harness ones so a package can replace a stub. */
  extraAlias?: { find: RegExp | string; replacement: string }[];
  /** Additional vitest settings merged last. */
  overrides?: UserConfig;
  /** The run report (`report.json`, `index.html`); default dir `<packageDir>/.flow-report`, goldens `<packageDir>/tests/goldens`. */
  report?: { dir?: string; goldensDir?: string };
}

export const flowTestAliases = (): { find: RegExp; replacement: string }[] => [
  { find: /^react-native$/, replacement: 'react-native-web' },
  {
    find: /^react-native-reanimated$/,
    replacement: stub('react-native-reanimated'),
  },
  { find: /^@gorhom\/bottom-sheet$/, replacement: stub('gorhom-bottom-sheet') },
  {
    find: /^react-native-markdown-display$/,
    replacement: stub('react-native-markdown-display'),
  },
  {
    find: /^react-native-safe-area-context$/,
    replacement: stub('react-native-safe-area-context'),
  },
  {
    find: /^react-native-gesture-handler$/,
    replacement: stub('react-native-gesture-handler'),
  },
  { find: /^react-native-screens$/, replacement: stub('react-native-screens') },
  { find: /^victory-native$/, replacement: stub('victory-native') },
  {
    find: /^@shopify\/react-native-skia(\/lib\/module\/web)?$/,
    replacement: stub('react-native-skia'),
  },
  { find: /^expo-router$/, replacement: stub('expo-router') },
  { find: /^expo-router\/drawer$/, replacement: DRAWER_STUB },
  { find: /^expo-audio$/, replacement: stub('expo-audio') },
  { find: /^expo-font$/, replacement: stub('expo-font') },
  {
    find: /^@react-native-async-storage\/async-storage$/,
    replacement: stub('async-storage'),
  },
];

export async function flowTestConfig(
  options: FlowTestConfigOptions
): Promise<UserConfig> {
  const alias = [...(options.extraAlias ?? []), ...flowTestAliases()];
  const plugins: NonNullable<UserConfig['plugins']> = [];

  // Real icons, without the barrel's cost: each `import { X } from 'lucide-react-native'` is rewritten to X's own
  // module, so goldens draw the true SVG. Names with no icon module (types, helpers) fall through to the stub below.
  let lucideIcons: Map<string, string> | undefined;
  const iconFiles = (): Map<string, string> => {
    if (lucideIcons) return lucideIcons;
    const entry = createRequire(
      path.join(options.packageDir, 'package.json')
    ).resolve('lucide-react-native');
    const esmDir = path.resolve(path.dirname(entry), '..', 'esm');
    const barrel = readFileSync(
      path.join(esmDir, 'lucide-react-native.mjs'),
      'utf8'
    );
    lucideIcons = new Map();
    for (const m of barrel.matchAll(
      /^export \{([^}]*)\} from '(\.\/icons\/[^']+)';$/gm
    )) {
      for (const spec of m[1].split(',')) {
        const name = spec
          .trim()
          .split(/\s+as\s+/)
          .pop();
        if (name) lucideIcons.set(name, path.join(esmDir, m[2]));
      }
    }
    return lucideIcons;
  };
  plugins.push({
    name: 'flow-lucide-real-icons',
    enforce: 'pre',
    transform(code, id) {
      if (id.includes('node_modules') || !code.includes('lucide-react-native'))
        return;
      const out = code.replace(
        /import\s*\{([^}]*)\}\s*from\s*['"]lucide-react-native['"];?/g,
        (whole, specs: string) => {
          const icons: string[] = [];
          const rest: string[] = [];
          for (const raw of specs
            .split(',')
            .map((x) => x.trim())
            .filter(Boolean)) {
            if (raw.startsWith('type ')) {
              rest.push(raw);
              continue;
            }
            const [imported, local = imported] = raw.split(/\s+as\s+/);
            const file = iconFiles().get(imported);
            if (file)
              icons.push(`import ${local} from ${JSON.stringify(file)};`);
            else rest.push(raw);
          }
          if (icons.length === 0) return whole;
          return [
            ...icons,
            rest.length
              ? `import { ${rest.join(', ')} } from 'lucide-react-native';`
              : '',
          ].join('\n');
        }
      );
      return out === code ? undefined : { code: out, map: null };
    },
  } as NonNullable<UserConfig['plugins']>[number]);

  // The real lucide barrel costs seconds of transform per suite, and a CJS proxy stub cannot serve named imports.
  // So emit an ESM module with one inert icon per name the real package exports.
  plugins.push({
    name: 'flow-lucide-icons',
    enforce: 'pre',
    resolveId(id) {
      if (id === 'lucide-react-native') return '\0flow-lucide';
    },
    load(id) {
      if (id !== '\0flow-lucide') return;
      const entry = createRequire(
        path.join(options.packageDir, 'package.json')
      ).resolve('lucide-react-native');
      const dts = readFileSync(
        path.resolve(path.dirname(entry), '..', 'lucide-react-native.d.ts'),
        'utf8'
      );
      const names = new Set<string>();
      for (const block of dts.matchAll(/^export \{([^}]*)\};$/gm)) {
        for (const spec of block[1].split(',')) {
          const name = spec
            .trim()
            .split(/\s+as\s+/)
            .pop();
          if (name && /^[A-Z]/.test(name) && !['LucideProvider'].includes(name))
            names.add(name);
        }
      }
      const icons = [...names]
        .map((n) => `export const ${n} = makeIcon('${n}');`)
        .join('\n');
      return {
        code: [
          "import React from 'react';",
          "const makeIcon = (name) => { const Icon = (props) => React.createElement('span', { 'data-testid': props.testID, 'data-icon': name, role: 'img' }); Icon.displayName = `Lucide(${name})`; return Icon; };",
          'export const LucideProvider = (props) => props.children;',
          'export const useLucideContext = () => ({ size: 24, color: "currentColor", strokeWidth: 2, absoluteStrokeWidth: false });',
          'export const createLucideIcon = (name) => makeIcon(name);',
          icons,
        ].join('\n'),
        map: null,
      };
    },
  } as NonNullable<UserConfig['plugins']>[number]);

  // The drawer stub renders the drawer view the package's own expo-router renders, at expo-router's version.
  plugins.push({
    name: 'flow-drawer-layout',
    enforce: 'pre',
    resolveId(id, importer) {
      if (
        id !== 'react-native-drawer-layout' ||
        importer?.split('?')[0] !== DRAWER_STUB
      )
        return;
      const expoRouter = createRequire(
        path.join(options.packageDir, 'package.json')
      ).resolve('expo-router/drawer');
      return this.resolve(id, expoRouter, { skipSelf: true });
    },
  } as NonNullable<UserConfig['plugins']>[number]);

  // Stub @rn-primitives .mjs files to prevent import-analysis parsing JSX.
  plugins.push({
    name: 'stub-rn-primitives',
    enforce: 'pre',
    resolveId(id) {
      if (id.includes('@rn-primitives') && id.endsWith('.mjs')) {
        return `virtual:rn-primitives-stub:${id}`;
      }
    },
    load(id) {
      if (id.startsWith('virtual:rn-primitives-stub:')) {
        return {
          code: 'export const Slot=()=>null;export const Separator=()=>null;export default {};',
          map: null,
        };
      }
    },
  } as NonNullable<UserConfig['plugins']>[number]);

  const reportDir =
    options.report?.dir ?? path.join(options.packageDir, '.flow-report');
  const debugOut =
    process.env.FLOW_DEBUG_OUT ?? path.join(reportDir, 'debug.jsonl');
  const reporter = new FlowReporter({
    root: options.packageDir,
    dir: reportDir,
    goldensDir: options.report?.goldensDir,
    debugOut,
    clearDebug: !process.env.FLOW_DEBUG_OUT,
  });

  const base = defineConfig({
    root: options.packageDir,
    plugins,
    // metro's babel-preset-expo inlines EXPO_OS per platform; flow tests render the web one
    define: {
      'process.env.TAMAGUI_TARGET': '"web"',
      'process.env.EXPO_OS': '"web"',
      __DEV__: 'true',
    },
    resolve: {
      alias,
      // The lockfile carries peer variants of react; a second copy next to react-dom fails every suite on load.
      dedupe: ['react', 'react-dom', 'react-native-web'],
      conditions: ['browser', 'module', 'import', 'default'],
      // An ESM build keeps `react-native` imports on the alias; a CJS `main` would require the Flow-typed original.
      mainFields: ['browser', 'module', 'main'],
      // Metro picks `.web.*` platform files first; without this, libraries such as gluestack load their native branch.
      extensions: [
        '.web.tsx',
        '.web.ts',
        '.web.jsx',
        '.web.js',
        '.tsx',
        '.ts',
        '.jsx',
        '.mjs',
        '.js',
        '.json',
      ],
    },
    optimizeDeps: {
      exclude: ['@rn-primitives/slot'],
    },
    test: {
      include: [FLOW_TEST_GLOB],
      environment: 'jsdom',
      setupFiles: [path.resolve(here, 'setup.ts')],
      reporters: ['default', reporter],
      env: { FLOW_DEBUG_OUT: debugOut },
      provide: { flowAppRoot: options.packageDir },
      // A file's first test compiles its lazily loaded route (12-15 s alone), over 20 s under the parallel pre-commit run.
      testTimeout: 60_000,
      // Every dependency goes through vite so the react-native alias applies to
      // CommonJS requires too (an externalized dep would load the Flow-syntax entry).
      // @rn-primitives ships JSX in .mjs, which import-analysis cannot parse; the
      // plugin above rewrites those, and optimizeDeps.exclude keeps them out of
      // the prebundle. `server.deps` has no `exclude` key -- it accepts only
      // `inline` and `external` -- so asking for one here was a type error, not
      // a setting.
      server: { deps: { inline: true } },
    },
  });
  // mergeConfig's overloads are keyed on the record types it is given; passing
  // defineConfig's return union collapses the second parameter to `never`, so
  // both sides are narrowed to UserConfig for the merge.
  return options.overrides
    ? (mergeConfig(base as UserConfig, options.overrides) as UserConfig)
    : (base as UserConfig);
}
