const { hexagonal } = require('../../feature-kit/depcruise.presets.cjs');

module.exports = hexagonal({ core: ['primitives', 'logger', 'pipeline', 'plugins', 'liveTail', 'persistence'], ports: [], adapters: ['install'], testing: [], coreNpm: ['loglayer', '@loglayer/transport'] });
