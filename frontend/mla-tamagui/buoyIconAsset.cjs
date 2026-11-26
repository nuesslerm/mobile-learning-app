const path = require('node:path');

module.exports = function buoyIconAsset(asset) {
  const icons = path.join('@buoy-gg', 'floating-tools-core', 'lib', 'module', 'icons', 'assets');
  if (!asset.fileSystemLocation.endsWith(path.sep + icons) || asset.scales.length < 2) return asset;

  // Metro sorts scales ascending. One high-resolution copy per icon keeps EAS below its asset limit.
  return { ...asset, files: asset.files.slice(-1), scales: asset.scales.slice(-1) };
};
