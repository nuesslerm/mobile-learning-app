const { hexagonal } = require('../../feature-kit/depcruise.presets.cjs');

module.exports = hexagonal({ core: ['core'], ports: ['ports'], adapters: ['adapters'], testing: ['testing'] });
