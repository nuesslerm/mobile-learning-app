const { hexagonalFeature } = require('../../../feature-kit/depcruise.presets.cjs');

const base = hexagonalFeature({ root: 'src', appPrefix: '../../' });

module.exports = {
  ...base,
  forbidden: [
    ...base.forbidden,
    {
      name: 'only-di-picks-a-component-set',
      comment: 'screens read components and colors through @lib/agent-ui-contracts; di/ alone names a design system',
      severity: 'error',
      from: { path: '^src/', pathNot: ['^src/di/'] },
      to: { path: '(^|/)(orca-design-system|tamagui-agent-components)/' },
    },
  ],
};
