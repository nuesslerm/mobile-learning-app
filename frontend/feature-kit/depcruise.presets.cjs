// The dependency-cruiser presets per package type: docs/solutions/architecture/hexagonal-architecture-by-package-type.md.
// Each returns an IConfiguration whose rules are all `error`, `no-circular` included. A package cruises its own `src`
// (`root: 'src'`); the app cruises an in-app feature in place (`root: 'features/<name>'`).
const { version: typescriptVersion } = require('typescript');
if (Number(typescriptVersion.split('.')[0]) >= 7) throw new Error(`dependency-cruiser needs the typescript API below 7, got ${typescriptVersion}`);

const TESTS = ['(/__tests__/|\\.test\\.tsx?$)'];
const NODE_BUILTINS = { dependencyTypes: ['core'] };
const APP_DI = '(^|/)(frontend/libraries/)?app-di/';
const NON_SOURCE_NPM = ['npm', 'npm-dev', 'npm-peer', 'npm-optional', 'npm-bundled'];
const EXTERNAL = [...NON_SOURCE_NPM, 'npm-no-pkg', 'npm-unknown', 'unknown'];

const options = () => ({
  doNotFollow: { path: 'node_modules' },
  tsPreCompilationDeps: true,
  tsConfig: { fileName: 'tsconfig.json' },
  enhancedResolveOptions: { extensions: ['.ts', '.tsx', '.js', '.json'] },
});

// A resolved package path ends in `node_modules/<pkg>/…`; one the cruise could not resolve is the bare specifier.
const pkgPath = (alternatives) => `(^|/)node_modules/(${alternatives})(/|$)|^(${alternatives})(/|$)`;
const rule = (name, comment, from, to) => ({ name, comment, severity: 'error', from, to });
const noCircular = () => rule('no-circular', 'a cycle is an error', {}, { circular: true });
const dirs = (names) => `(${names.join('|')})`;

/** One rule per ring: `ring` may import only the rings in `may` (and itself). */
function ringRules(root, rings, testsFree = true) {
  const names = Object.keys(rings);
  return names.map((ring) => {
    const may = rings[ring];
    const banned = names.filter((other) => other !== ring && !may.includes(other));
    return rule(
      `${ring}-imports-${may.length ? may.join('-') : 'nothing'}`,
      `${ring} may import ${may.length ? may.join(', ') : 'no other ring'}`,
      { path: `^${root}/${dirs([ring])}/`, ...(testsFree ? { pathNot: TESTS } : {}) },
      { path: banned.length ? `^${root}/${dirs(banned)}/` : '^$' },
    );
  });
}

/** @param {{ root: string, appPrefix?: string }} o root: 'src' or 'features/<name>'; appPrefix: where `app/`, `app-shell/`, `lib/` sit relative to the cruise cwd */
function hexagonalFeature({ root = 'src', appPrefix = '' } = {}) {
  const inApp = root.startsWith('features/');
  const own = inApp ? root.split('/')[1] : '';
  const rings = {
    domain: [],
    data: ['domain'],
    state: ['domain'],
    ui: ['domain', 'state'],
    di: ['domain', 'data', 'state'],
    routes: ['di', 'ui'],
  };
  const production = { pathNot: TESTS };
  const notTesting = ['domain', 'data', 'state', 'ui', 'di', 'routes'];
  return {
    forbidden: [
      rule(
        'module-in-a-ring',
        'every module sits in domain, data, state, ui, di, routes or testing, or is the barrel',
        { path: `^${root}/`, pathNot: [`^${root}/${dirs([...notTesting, 'testing'])}/`, `^${root}/index\\.tsx?$`, '\\.(json|md)$', ...TESTS] },
        {},
      ),
      ...ringRules(root, rings),
      rule('production-does-not-import-testing', 'no production ring imports testing/', { path: `^${root}/${dirs(notTesting)}/`, ...production }, { path: `^${root}/testing/` }),
      rule('only-di-reads-app-di', 'di/ is the only reader of @lib/app-di', { path: `^${root}/${dirs(['domain', 'data', 'state', 'ui', 'routes'])}/`, ...production }, { path: APP_DI }),
      rule(
        'domain-pulls-no-framework',
        'domain/ imports no React, React Native, Expo or tamagui',
        { path: `^${root}/domain/`, ...production },
        { path: pkgPath('react|react-native|react-dom|expo[^/]*|@expo|@react-native|tamagui|@tamagui') },
      ),
      rule('domain-pulls-no-node', 'domain/ imports no node built-in', { path: `^${root}/domain/`, ...production }, NODE_BUILTINS),
      rule(
        'feature-is-a-leaf',
        'a feature imports no other feature',
        { path: `^${root}/` },
        inApp ? { path: `^features/(?!${own}/)[^/]+/` } : { path: '^\\.\\./[^/]+/src/' },
      ),
      rule(
        'feature-does-not-import-the-shell',
        'nothing under a feature imports app/, app-shell/ or lib/ of the shell',
        { path: `^${root}/` },
        { path: `^${appPrefix.replace(/\./g, '\\.')}(app|app-shell|lib)/` },
      ),
      noCircular(),
    ],
    options: options(),
  };
}

/** `ui/`, `routes/` and an optional `state/`: no domain, data, di or ports. */
function presentationFeature({ root = 'src', appPrefix = '' } = {}) {
  const base = hexagonalFeature({ root, appPrefix });
  const keep = new Set(['no-circular', 'feature-is-a-leaf', 'feature-does-not-import-the-shell', 'only-di-reads-app-di']);
  const extra = [
    rule('presentation-has-no-ring-beyond-ui', 'a presentation feature has ui/, routes/ and state/ only', { path: `^${root}/` }, { path: `^${root}/${dirs(['domain', 'data', 'di'])}/` }),
    ...ringRules(root, { ui: ['state'], state: [], routes: ['ui'] }),
  ];
  return { forbidden: [...base.forbidden.filter((r) => keep.has(r.name)), ...extra], options: options() };
}

/** A flat `src/` of schemas: zod is the only dependency. */
function contractsPackage({ root = 'src' } = {}) {
  return {
    forbidden: [
      rule('contracts-import-only-zod', 'zod is the only dependency', { path: `^${root}/`, pathNot: TESTS }, { dependencyTypes: EXTERNAL, pathNot: pkgPath('zod') }),
      rule('contracts-no-node', 'no node built-ins', { path: `^${root}/`, pathNot: TESTS }, NODE_BUILTINS),
      noCircular(),
    ],
    options: options(),
  };
}

/** The shell: `app/`, `app-shell/`, `lib/` reach features only through a barrel, `routes/*` or `testing`. */
function appShell({ shell = ['app', 'app-shell'], lib = 'lib' } = {}) {
  const shellDirs = dirs(shell);
  return {
    forbidden: [
      rule('shell-uses-feature-barrels-only', 'a feature is reached through its barrel, routes/* or testing', { path: `^${dirs([...shell, lib])}/` }, { path: '^features/[^/]+/(domain|data|state|ui|di)/' }),
      rule('features-do-not-import-the-shell', 'nothing under features/ imports the shell', { path: '^features/' }, { path: `^${dirs([...shell, lib])}/` }),
      rule('lib-only-for-the-shell', 'lib/ is imported only by app/ and app-shell/', { path: `^(?!${shellDirs}/|${lib}/)`, pathNot: ['^node_modules/', '^features/', '^tests/', ...TESTS] }, { path: `^${lib}/` }),
      noCircular(),
    ],
    options: options(),
  };
}

/** Flat `src/`: relative modules and an allow-list of pure dependencies only. */
function pure({ allow = ['zod'] } = {}) {
  return {
    forbidden: [
      rule('pure-no-node', 'no node built-ins', { path: '^src/', pathNot: TESTS }, NODE_BUILTINS),
      rule('pure-npm-allow-list', `only ${allow.join(', ')}`, { path: '^src/', pathNot: TESTS }, { dependencyTypes: EXTERNAL, pathNot: pkgPath(allow.map((a) => a.replace(/[/@]/g, '\\$&')).join('|')) }),
      rule('pure-no-workspace-package', 'no workspace package', { path: '^src/', pathNot: TESTS }, { path: '^\\.\\./', pathNot: '(^|/)node_modules/' }),
      noCircular(),
    ],
    options: options(),
  };
}

/** Core, ports, adapters, testing: a library's folder names mapped to roles. */
function hexagonal({ core = ['core'], ports = ['ports'], adapters = ['adapters'], testing = ['testing'], coreNpm = [] } = {}) {
  const group = (names) => `^src/${dirs(names)}/`;
  const prod = { pathNot: TESTS };
  return {
    forbidden: [
      rule('core-imports-core-and-ports', 'core imports core and ports only', { path: group(core), ...prod }, { path: `${group(adapters)}|${group(testing)}` }),
      rule('ports-import-core', 'ports import core only', { path: group(ports), ...prod }, { path: `${group(adapters)}|${group(testing)}` }),
      rule('adapters-import-core-and-ports', 'adapters import core and ports; never testing', { path: group(adapters), ...prod }, { path: group(testing) }),
      rule('core-no-node', 'core imports no node built-in', { path: group(core), ...prod }, NODE_BUILTINS),
      rule(
        'core-no-device-modules',
        'only adapters import Expo, React Native or AsyncStorage',
        { path: `${group(core)}|${group(ports)}`, ...prod },
        { path: pkgPath('expo[^/]*|@expo|react-native|@react-native|@react-native-async-storage'), pathNot: coreNpm.length ? pkgPath(coreNpm.join('|')) : '^$' },
      ),
      rule('library-does-not-import-the-app', 'no library imports frontend/mla-tamagui', { path: '^src/' }, { path: '(^|/)mla-tamagui/' }),
      rule('library-imports-only-logging', 'a hexagonal library may import @lib/logging only', { path: '^src/' }, { path: '(^|/)frontend/libraries/(?!logging/)|^\\.\\./(?!logging/)[^/]+/src/' }),
      noCircular(),
    ],
    options: options(),
  };
}

/** theme -> primitives, icons -> ui -> composites; lib is a leaf. */
function component() {
  const rings = { theme: ['lib'], primitives: ['theme', 'lib'], icons: ['lib'], ui: ['theme', 'primitives', 'icons', 'lib'], composites: ['theme', 'primitives', 'icons', 'ui', 'lib'], lib: [] };
  return {
    forbidden: [
      ...Object.entries(rings).map(([ring, may]) => {
        const banned = Object.keys(rings).filter((o) => o !== ring && !may.includes(o));
        return rule(`${ring}-imports-${may.length ? may.join('-') : 'nothing'}`, `${ring} may import ${may.join(', ') || 'no other layer'}`, { path: `^src/${ring}(/|\\.tsx?$)`, pathNot: TESTS }, { path: `^src/${dirs(banned)}(/|\\.tsx?$)` });
      }),
      rule('library-does-not-import-the-app', 'no library imports frontend/mla-tamagui', { path: '^src/' }, { path: '(^|/)mla-tamagui/' }),
      noCircular(),
    ],
    options: options(),
  };
}

/** A test harness is feature-agnostic: it imports no app or feature code. */
function harness() {
  return {
    forbidden: [
      rule('harness-imports-no-app-or-feature', 'never imports app or feature code', { path: '^src/' }, { path: '(^|/)mla-tamagui/|(^|/)features/' }),
      noCircular(),
    ],
    options: options(),
  };
}

module.exports = { hexagonalFeature, presentationFeature, contractsPackage, appShell, pure, hexagonal, component, harness };
