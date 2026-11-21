import { PYODIDE_PAGE_JS } from './pyodidePage';

/** The page the hidden WebView loads: the generated script inside a bare document (a closing script tag in the script is escaped). */
export const PYODIDE_PAGE_HTML = `<!doctype html><html><head><meta charset="utf-8"></head><body><script>${PYODIDE_PAGE_JS.replaceAll('</script', '<\\/script')}</script></body></html>`;
