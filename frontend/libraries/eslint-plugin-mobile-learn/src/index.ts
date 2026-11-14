import rules from './rules/index.js';

// meta.name lets oxlint load it as a JS plugin under the same prefix ESLint uses
const plugin = { meta: { name: 'mobile-learn' }, rules };

export default plugin;
