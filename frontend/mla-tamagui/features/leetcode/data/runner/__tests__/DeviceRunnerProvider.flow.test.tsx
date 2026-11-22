import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { renderScreen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import type { CodeRunnerPort } from '../../../domain/ports';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import { JS_PAGE_HTML } from '../../javascript/jsPageHtml';
import type { AssetIo } from '../../pyodide/pyodideAssets';
import { DeviceRunnerProvider, useOptionalDeviceCodeRunner } from '../DeviceRunnerProvider';

/**
 * The provider's hidden views over the app's real WebView twin: each page is an iframe running the real rpc server (bundled from
 * a support file, because esbuild refuses to run inside jsdom), the app side is the real runner and page host.
 */
function pageHtmlOf(support: string): string {
  const bundle = execFileSync(
    process.execPath,
    [
      '-e',
      `const { buildSync } = require('esbuild');
     const built = buildSync({ entryPoints: [${JSON.stringify(join(process.cwd(), support))}], bundle: true, format: 'iife', platform: 'browser', target: 'es2020', write: false });
     process.stdout.write(built.outputFiles[0].text);`,
    ],
    { cwd: process.cwd(), encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }
  );
  return `<!doctype html><html><head></head><body><script>${bundle}</script></body></html>`;
}

const PAGE_HTML = pageHtmlOf('features/leetcode/data/pyodide/__tests__/support/fakePythonPage.ts');
const JAVASCRIPT_PAGE_HTML = pageHtmlOf('features/leetcode/data/javascript/__tests__/support/fakeJsPage.ts');
const io: AssetIo = { resolve: async (id) => `http://assets.invalid/${id}.bin`, writePage: async () => 'file:///unused', chunks: async function* () {} };
const MODULES = async () => ({ asm: 1, lock: 2, wasm: 3, stdlib: 4 });

const problem = ProblemCatalog.getById('two-sum');
if (!problem) throw new Error('no two-sum');

function mount(javascriptPageHtml: string = JAVASCRIPT_PAGE_HTML) {
  const held: { runner?: CodeRunnerPort } = {};
  function Probe() {
    held.runner = useOptionalDeviceCodeRunner();
    return null;
  }
  const view = renderScreen(
    <DeviceRunnerProvider modules={MODULES} pageHtml={PAGE_HTML} javascriptPageHtml={javascriptPageHtml} io={io}>
      <Probe />
    </DeviceRunnerProvider>
  );
  return { held, view };
}

describe('the device runner provider', () => {
  it('Given the provider mounted and no run made, When the tree renders, Then no page is mounted and the runner is available to the tree', () => {
    const { held, view } = mount();
    expect(held.runner).toBeDefined();
    expect(view.container.querySelector('iframe')).toBeNull();
  });

  it('Given a run is made, When the hidden page loads and answers, Then the response is the page’s verdict, the page stays mounted, and a second run reuses it', async () => {
    // Given the mounted provider
    const { held, view } = mount();

    // When a run is made (the view mounts the page once the runner asks for it: React flushes that between polls, not inside one act), and then another
    const pendingFirst = held.runner?.run(runParamsFor(problem, 'python3', 'code'));
    await waitFor(() => expect(view.container.querySelector('iframe')).not.toBeNull());
    const frame = view.container.querySelector('iframe');
    const first = await pendingFirst;
    const second = await held.runner?.run(runParamsFor(problem, 'python3', 'code'));

    // Then both are Accepted by the one page, which stayed mounted
    expect(first).toMatchObject({ source: 'offline', status_msg: 'Accepted', total_correct: 12, total_testcases: 12 });
    expect(second).toMatchObject({ status_msg: 'Accepted' });
    expect(view.container.querySelector('iframe')).toBe(frame);
    await waitFor(() => expect((frame?.contentWindow as (Window & { __loads?: number }) | null)?.__loads).toBe(1));
  });

  it('Given a JavaScript run is made, When its hidden page loads and answers, Then the JavaScript page is the only one mounted and the Python page was never loaded', async () => {
    // Given the mounted provider
    const { held, view } = mount();

    // When a JavaScript run is made
    const pending = held.runner?.run(runParamsFor(problem, 'javascript', 'code'));
    await waitFor(() => expect(view.container.querySelector('iframe[title="JavaScriptHostWebView"]')).not.toBeNull());
    const response = await pending;

    // Then the JavaScript page answered and no other page was mounted
    expect(response).toMatchObject({ source: 'offline', status_msg: 'Accepted', total_correct: 12, total_testcases: 12 });
    expect(view.container.querySelectorAll('iframe')).toHaveLength(1);
    expect(view.container.querySelector('iframe[title="PyodideHostWebView"]')).toBeNull();
  });

  it('Given the real JavaScript page in a WebView that has no Worker, When a JavaScript run is made, Then the run fails as unavailable at the boot stage and says it cannot start a worker', async () => {
    // Given the provider with the page the app ships (jsdom has no Worker and no blob urls, as a WebView might not)
    const { held, view } = mount(JS_PAGE_HTML);

    // When a JavaScript run is made (the page mounts, answers and is thrown away while the test polls: React flushes that between polls, not inside one act)
    let outcome: unknown;
    void held.runner?.run(runParamsFor(problem, 'javascript', 'code')).then(
      () => {
        outcome = 'answered';
      },
      (error: unknown) => {
        outcome = error;
      }
    );
    await waitFor(() => expect(outcome).toBeDefined());

    // Then the page had loaded and answered, its boot probe found no way to start a worker, and the failed page is gone
    expect(outcome).toMatchObject({ name: 'CodeRunnerError', kind: 'unavailable', stage: 'boot', message: expect.stringContaining('cannot start a worker') });
    await waitFor(() => expect(view.container.querySelector('iframe')).toBeNull());
  });
});
