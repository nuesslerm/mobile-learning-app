// Python sources imported as text: esbuild's text loader embeds them in the page bundle (scripts/pyodide-bundle.mjs).
declare module '*.py' {
  const source: string;
  export default source;
}
