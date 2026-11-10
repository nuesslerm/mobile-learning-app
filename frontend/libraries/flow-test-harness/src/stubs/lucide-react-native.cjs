'use strict';
// A callable module object: `import Icon from '<icon>.mjs'` may yield the whole
// module under CJS interop, so the module itself must be a React component.
const React = require('react');
const makeIcon = (name) => {
  const Icon = (props) => React.createElement('span', { 'data-testid': props.testID, 'data-icon': String(name), role: 'img' });
  Icon.displayName = `Lucide(${String(name)})`;
  return Icon;
};
const fixed = {
  __esModule: true,
  useLucideContext: () => ({ size: 24, color: 'currentColor', strokeWidth: 2, absoluteStrokeWidth: false }),
  LucideProvider: (props) => props.children,
};
const root = makeIcon('icon');
module.exports = new Proxy(root, {
  get: (t, name) => {
    if (name in fixed) return fixed[name];
    if (name === 'default') return t;
    if (typeof name === 'symbol' || name in t) return t[name];
    // Only PascalCase names are icons; React probes statics such as
    // childContextTypes / getDerivedStateFromProps and must see undefined.
    return /^[A-Z]/.test(name) ? makeIcon(name) : undefined;
  },
  has: () => true,
});
