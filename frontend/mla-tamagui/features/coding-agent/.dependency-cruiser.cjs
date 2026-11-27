const { hexagonalFeature } = require('../../../feature-kit/depcruise.presets.cjs');

const base = hexagonalFeature({ root: 'src', appPrefix: '../../' });

module.exports = {
  ...base,
  forbidden: [
    ...base.forbidden,
    {
      name: 'only-di-picks-a-skin',
      comment: 'screens read components and colors through @lib/skin; di/ alone names a design system',
      severity: 'error',
      from: { path: '^src/', pathNot: ['^src/di/'] },
      to: { path: '(^|/)(orca-skin|tamagui-skin)/' },
    },
  ],
};
