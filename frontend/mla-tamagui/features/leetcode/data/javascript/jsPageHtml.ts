import { JS_PAGE_JS } from './jsPage';

/** The page the hidden WebView loads: the generated script inside a bare document (the build refuses a script that holds a closing script tag). */
export const JS_PAGE_HTML = `<!doctype html><html><head><meta charset="utf-8"></head><body><script>${JS_PAGE_JS}</script></body></html>`;
