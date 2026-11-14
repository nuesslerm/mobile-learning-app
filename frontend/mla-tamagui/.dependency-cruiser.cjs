/**
 * dependency-cruiser — layer boundary enforcement for features/leetcode.
 *
 * Direction: ui → di/state → data → domain. `ui` reaches
 * `data` only through `di` or `state` (ports injected via
 * LeetCodeContainerProvider); `domain/` is the pure core (types, schemas) with no
 *  intra-feature imports and holds the ports it speaks (`domain/ports.ts`). The rules per package type and the target these rules move to:
 * docs/solutions/architecture/hexagonal-architecture-by-package-type.md.
 */
// dependency-cruiser reads TypeScript through the `typescript` package's API, which it supports below 7; handed a 7.0
// `typescript` it cruises 0 modules and passes, so it refuses to run instead.
const { version: typescriptVersion } = require('typescript');
if (Number(typescriptVersion.split('.')[0]) >= 7) throw new Error(`dependency-cruiser needs the typescript API below 7, got ${typescriptVersion}`);


/** @type {import('dependency-cruiser').IConfiguration} */
module.exports = {
  forbidden: [
    {
      name: 'no-circular',
      severity: 'warn',
      from: {},
      to: { circular: true },
    },
    {
      name: 'domain-is-pure',
      comment: 'domain/ must not depend on any other leetcode layer.',
      severity: 'error',
      from: { path: '^features/leetcode/domain/' },
      to: { path: '^features/leetcode/(code-editor|data|di|plan|state|ui)/' },
    },
    {
      name: 'data-never-imports-higher-layers',
      comment: 'data/ must not import ui/, di/ or state/ — adapters live below the composition root.',
      severity: 'error',
      from: { path: '^features/leetcode/data/' },
      to: { path: '^features/leetcode/(ui|di|state)/' },
    },
    {
      name: 'ui-never-imports-data',
      comment:
        'ui/ reaches data through di/ or state/, never directly. // debt: ui/ProblemEditorScreen.tsx imports data/problemCatalog and data/voiceEditIdentity; ui/ProblemListScreen.tsx imports data/problemCatalog — tracked in docs/gaps/leetcode-ui-direct-io-debt.md.',
      severity: 'error',
      from: { path: '^features/leetcode/ui/' },
      to: {
        path: '^features/leetcode/data/',
        pathNot: [
          '^features/leetcode/data/problemCatalog\\.ts$',
          '^features/leetcode/data/voiceEditIdentity\\.ts$',
        ],
      },
    },
  ],
  options: {
    doNotFollow: { path: 'node_modules' },
    tsConfig: { fileName: 'tsconfig.json' },
    tsPreCompilationDeps: true,
    enhancedResolveOptions: {
      extensions: ['.ts', '.js', '.json'],
    },
  },
};