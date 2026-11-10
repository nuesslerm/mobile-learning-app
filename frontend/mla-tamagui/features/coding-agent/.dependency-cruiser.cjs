const { featureDepcruiseConfig } = require('../../../feature-kit/depcruise.feature.cjs');

module.exports = featureDepcruiseConfig([
  { layer: 'domain', dirs: ['domain', 'contracts', 'fixtures'], imports: [] },
  { layer: 'data', dirs: ['data'], imports: ['domain'] },
  { layer: 'state', dirs: ['state'], imports: ['domain', 'data'] },
  { layer: 'integration', dirs: ['integration'], imports: ['domain', 'data', 'state'] },
  { layer: 'ui', dirs: ['ui'], imports: ['domain', 'data', 'state', 'integration'] },
]);
