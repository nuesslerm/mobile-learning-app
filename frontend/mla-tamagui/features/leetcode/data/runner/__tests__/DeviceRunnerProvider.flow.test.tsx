import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { renderScreen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import type { CodeRunnerPort } from '../../../domain/ports';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import type { AssetIo } from '../../pyodide/pyodideAssets';
import { DeviceRunnerProvider, useOptionalDeviceCodeRunner } from '../DeviceRunnerProvider';

/**
 * The provider's hidden view over the app's real WebView twin: the page is an iframe running the real rpc server (bundled from
 * support/fakePythonPage.ts, because esbuild refuses to run inside jsdom), the app side is the real runner and page host.
 */
const PAGE_BUNDLE = execFileSync(
  process.execPath,
  [
    '-e',
    `const { buildSync } = require('esbuild');
     const built = buildSync({ entryPoints: [${JSON.stringify(join(process.cwd(), 'features/leetcode/data/pyodide/__tests__/support/fakePythonPage.ts'))}], bundle: true, format: 'iife', platform: 'browser', target: 'es2020', write: false });
     process.stdout.write(built.outputFiles[0].text);`,
  ],
  { cwd: process.cwd(), encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }
);
const PAGE_HTML = `<!doctype html><html><head></head><body><script>${PAGE_BUNDLE}</script></body></html>`;
const io: AssetIo = { resolve: async (id) => `http://assets.invalid/${id}.bin`, writePage: async () => 'file:///unused', chunks: async function* () {} };
const MODULES = async () => ({ asm: 1, lock: 2, wasm: 3, stdlib: 4 });

const problem = ProblemCatalog.getById('two-sum');
if (!problem) throw new Error('no two-sum');

function mount() {
  const held: { runner?: CodeRunnerPort } = {};
  function Probe() {
    held.runner = useOptionalDeviceCodeRunner();
    return null;
  }
  const view = renderScreen(
    <DeviceRunnerProvider modules={MODULES} pageHtml={PAGE_HTML} io={io}>
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
});
