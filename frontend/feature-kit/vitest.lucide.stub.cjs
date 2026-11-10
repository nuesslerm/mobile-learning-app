// Stand-in for `lucide-react-native` in the feature unit tests: every icon is a named string.
'use strict';
module.exports = new Proxy(
  {},
  {
    get: (_target, name) => {
      if (name === '__esModule') return true;
      if (name === 'default') return 'LucideIcon';
      if (name === 'useLucideContext')
        return () => ({
          size: 24,
          color: 'currentColor',
          strokeWidth: 2,
          absoluteStrokeWidth: false,
        });
      if (name === 'LucideProvider') return (props) => props && props.children;
      return `Lucide_${String(name)}`;
    },
  }
);
