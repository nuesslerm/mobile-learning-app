// The shell and the in-app features, cruised in place by `pnpm run lint`. Presets: frontend/feature-kit/depcruise.presets.cjs.
const { hexagonalFeature, appShell } = require('../feature-kit/depcruise.presets.cjs');

// A widget counts as ui (leetcode's code-editor); a sub-feature repeats the ring names inside itself (leetcode's plan).
const FEATURES = {
  leetcode: { widgets: ['code-editor'], subFeatures: ['plan'] },
  'tamil-writing': {},
  dashboards: {},
};
const shell = appShell();

/** @type {import('dependency-cruiser').IConfiguration} */
module.exports = {
  forbidden: [
    ...Object.entries(FEATURES).flatMap(([name, extra]) =>
      hexagonalFeature({ root: `features/${name}`, ...extra }).forbidden.filter((rule) => rule.name !== 'no-circular'),
    ),
    ...shell.forbidden,
  ],
  options: shell.options,
};
