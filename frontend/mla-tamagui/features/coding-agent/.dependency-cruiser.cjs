const { hexagonalFeature } = require('../../../feature-kit/depcruise.presets.cjs');

const base = hexagonalFeature({ root: 'src', appPrefix: '../../' });

module.exports = {
  ...base,
  forbidden: [
    ...base.forbidden,
    {
      name: 'only-the-root-names-a-skin',
      comment: 'the app root provides the skin; here only the test harness names one',
      severity: 'error',
      from: { path: '^src/', pathNot: ['^src/testing/SkinHarness\\.tsx$'] },
      to: { path: '(^|/)(orca-skin|tamagui-skin)/' },
    },
  ],
};
