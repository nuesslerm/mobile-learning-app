const { PRESENTATION_LAYERS, featureDepcruiseConfig, withDomainDirs } = require('../../../feature-kit/depcruise.feature.cjs');

module.exports = featureDepcruiseConfig(withDomainDirs(PRESENTATION_LAYERS, ['domain', 'contracts']));
