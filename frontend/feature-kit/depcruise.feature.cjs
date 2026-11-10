// Today's layer preset for the feature packages (domain ← data ← state ← ui), not the target; the barrel, tests and files
// outside a layer are free. Target rings: docs/solutions/architecture/hexagonal-architecture-by-package-type.md (type 8).
const PRESENTATION_LAYERS = [
  { layer: 'domain', dirs: ['domain'], imports: [] },
  { layer: 'data', dirs: ['data'], imports: ['domain'] },
  { layer: 'state', dirs: ['state'], imports: ['domain', 'data'] },
  { layer: 'ui', dirs: ['ui'], imports: ['domain', 'data', 'state'] },
];

/** The layers with the domain layer spread over other directories too (e.g. `contracts`). */
// dependency-cruiser reads TypeScript through the `typescript` package's API, which it supports below 7; handed a 7.0
// `typescript` it cruises 0 modules and passes, so it refuses to run instead.
const { version: typescriptVersion } = require('typescript');
if (Number(typescriptVersion.split('.')[0]) >= 7) throw new Error(`dependency-cruiser needs the typescript API below 7, got ${typescriptVersion}`);

function withDomainDirs(layers, dirs) {
  return layers.map((spec) => (spec.layer === 'domain' ? { ...spec, dirs } : spec));
}

const pathOf = (dirs) => `^src/(${dirs.join('|')})/`;

/** @returns {import('dependency-cruiser').IConfiguration} */
function featureDepcruiseConfig(layers = PRESENTATION_LAYERS) {
  return {
    forbidden: layers.map(({ layer, dirs, imports }) => {
      const banned = layers.filter((other) => other.layer !== layer && !imports.includes(other.layer)).flatMap((other) => other.dirs);
      return {
        name: `${layer}-imports-${imports.length ? imports.join('-') : 'nothing'}`,
        comment: `${layer} may import ${imports.length ? imports.join(', ') : 'no other layer'}`,
        severity: 'error',
        from: { path: pathOf(dirs), pathNot: '(/__tests__/|\\.test\\.tsx?$)' },
        to: { path: banned.length ? pathOf(banned) : '^$' },
      };
    }),
    options: { doNotFollow: { path: 'node_modules' }, tsPreCompilationDeps: true, tsConfig: { fileName: 'tsconfig.json' } },
  };
}

module.exports = { PRESENTATION_LAYERS, withDomainDirs, featureDepcruiseConfig };
