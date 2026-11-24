// The shell and the in-app features, cruised in place by `pnpm run lint`. Presets: frontend/feature-kit/depcruise.presets.cjs.
const { hexagonalFeature, presentationFeature, appShell } = require('../feature-kit/depcruise.presets.cjs');
const { FEATURES } = require('./features/feature-map.cjs');

const PRESETS = { 'hexagonal-feature': hexagonalFeature, 'presentation-feature': presentationFeature };

const shell = appShell();

/** @type {import('dependency-cruiser').IConfiguration} */
module.exports = {
  forbidden: [
    ...Object.entries(FEATURES).flatMap(([name, { type, ...extra }]) =>
      PRESETS[type]({ root: `features/${name}`, ...extra }).forbidden.filter((rule) => rule.name !== 'no-circular'),
    ),
    ...shell.forbidden,
  ],
  options: shell.options,
};
