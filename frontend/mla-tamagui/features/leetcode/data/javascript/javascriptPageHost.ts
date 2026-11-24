import type { PageHost } from '../runner/pageHost';
import { createWebViewPageHost, type PageView } from '../runner/webViewPageHost';
import { JS_PAGE_HTML } from './jsPageHtml';

/** The address the page is given as its own: a worker started from a blob needs the page to have an origin, and the page needs no file. */
export const JS_PAGE_BASE_URL = 'https://localhost/';

/** The JavaScript runner's page host: the page travels as html, there are no runtime files to resolve, and nothing can fail before the page is shown. */
export function createJavaScriptPageHost(html: string = JS_PAGE_HTML): { host: PageHost; view: PageView } {
  return createWebViewPageHost({ resolve: async () => ({ source: { html, baseUrl: JS_PAGE_BASE_URL }, files: {} }) });
}
