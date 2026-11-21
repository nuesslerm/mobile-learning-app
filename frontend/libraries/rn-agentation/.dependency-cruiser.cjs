const { hexagonal } = require('../../feature-kit/depcruise.presets.cjs');

module.exports = hexagonal({ core: ['domain', 'geometry', 'markdown', 'api'], ports: [], adapters: ['capture', 'document', 'state', 'hooks', 'headless'], testing: [] });
