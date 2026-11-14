// The shell and the in-app features, cruised in place by `pnpm run lint`. Presets: frontend/feature-kit/depcruise.presets.cjs.
const { hexagonalFeature, appShell } = require('../feature-kit/depcruise.presets.cjs');

const FEATURES = ['leetcode', 'tamil-writing', 'dashboards'];
const shell = appShell();

/** @type {import('dependency-cruiser').IConfiguration} */
module.exports = {
  forbidden: [
    ...FEATURES.flatMap((name) => hexagonalFeature({ root: `features/${name}` }).forbidden.filter((rule) => rule.name !== 'no-circular')),
    ...shell.forbidden,
  ],
  options: shell.options,
};
