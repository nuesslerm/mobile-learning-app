// Shared Babel preset for TypeScript/Node consumers of @lib/logging.
// Applies @babel/preset-typescript so .ts files can be transformed without a
// separate tsc emit step, and injects the @lib/logging/babel plugin so
// every Logger.<method>(...) call site gets {file, line, fn, fnPath, testID}
// context injected at compile time.

module.exports = function babelPresetLogging() {
  return {
    presets: [require.resolve('@babel/preset-typescript')],
    plugins: [require.resolve('./dist/babel/injectLoggerSource.js')],
  };
};
