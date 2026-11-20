import { PYODIDE_ASSET_MODULES } from '../data/pyodide/assets';
import { PYODIDE_PAGE_JS } from '../data/pyodide/pyodidePage';

const PAGE_HTML = `<!doctype html><html><head><meta charset="utf-8"></head><body><script>${PYODIDE_PAGE_JS.replaceAll('</script', '<\\/script')}</script></body></html>`;

/** What the throwaway Pyodide spike screen runs on: the page and the asset modules. */
export const pyodideSpike = { pageHtml: PAGE_HTML, assets: PYODIDE_ASSET_MODULES };
