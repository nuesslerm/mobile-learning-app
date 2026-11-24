// The Pyodide runtime files Metro ships with the app, by the name the page expects them under. Copied from the pinned
// `pyodide` package by scripts/pyodide-bundle.mjs.
export const PYODIDE_ASSET_MODULES = {
  asm: require('./assets/pyodide.asm.mjs.bin') as number,
  lock: require('./assets/pyodide-lock.json.bin') as number,
  wasm: require('./assets/pyodide.asm.wasm') as number,
  stdlib: require('./assets/python_stdlib.zip') as number,
};
