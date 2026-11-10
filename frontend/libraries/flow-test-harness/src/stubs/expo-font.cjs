'use strict';
// jsdom lays out no glyphs, so a font never finishes loading there (expo-font's web loader waits on fontfaceobserver).
const loaded = new Set();
module.exports = {
  __esModule: true,
  useFonts: (map) => {
    Object.keys(map || {}).forEach((name) => loaded.add(name));
    return [true, null];
  },
  loadAsync: async (nameOrMap) => {
    Object.keys(typeof nameOrMap === 'string' ? { [nameOrMap]: 1 } : nameOrMap || {}).forEach((name) => loaded.add(name));
  },
  isLoaded: (name) => loaded.has(name),
};
