import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { createRef } from 'react';
import { createRpcClient, createWebViewHostTransport, type RpcClient } from '@lib/webview-rpc';
import { renderScreen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import WebWebView, { type WebViewHandle } from '../../lib/webview/ReactNativeWebView.web';

/**
 * The rpc protocol over the app's real WebView twin: the page is an iframe running the real library, bundled from
 * support/rpcPage.ts, and the app side is the real host transport on the WebView's ref and onMessage. What the library's
 * own tests prove on an emulated page is proved here on the page the browser tier runs.
 */
// esbuild refuses to run inside jsdom (its typed-array realm check), so the page is bundled by a plain Node process
const PAGE_BUNDLE = execFileSync(
  process.execPath,
  [
    '-e',
    `const { buildSync } = require('esbuild');
     const built = buildSync({ entryPoints: [${JSON.stringify(join(process.cwd(), 'tests/flow/support/rpcPage.ts'))}], bundle: true, format: 'iife', platform: 'browser', target: 'es2020', write: false });
     process.stdout.write(built.outputFiles[0].text);`,
  ],
  { cwd: process.cwd(), encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }
);

async function mountRpcPage(): Promise<{ client: RpcClient; ticks: number[] }> {
  const ref = createRef<WebViewHandle>();
  const host = createWebViewHostTransport(() => ref.current);
  const client = createRpcClient(host, { timeoutMs: 5000 });
  let ready = false;
  client.subscribe('ready', () => {
    ready = true;
  });
  const ticks: number[] = [];
  client.subscribe('tick', (n) => ticks.push(n as number));
  const html = `<!doctype html><html><head></head><body><script>${PAGE_BUNDLE}</script></body></html>`;
  renderScreen(<WebWebView ref={ref} source={{ html }} onMessage={(event) => host.receive(event.nativeEvent.data)} />);
  await waitFor(() => expect(ready).toBe(true));
  return { client, ticks };
}

describe('the rpc protocol over the WebView twin', () => {
  it('given a page running the rpc server, when the app calls it, then results, remote errors and unknown methods come back typed', async () => {
    // Given the mounted page that has announced itself
    const { client } = await mountRpcPage();

    // When the app calls a handler, one that throws, and one that does not exist
    const sum = await client.call<number>('add', { a: 20, b: 22 });
    const broke = await client.call('boom').catch((e: unknown) => e);
    const missing = await client.call('nope').catch((e: unknown) => e);

    // Then the sum is the answer, the throw is a remote error with its data, and the miss is an unknown method
    expect(sum).toBe(42);
    expect(broke).toMatchObject({ code: 'remote', message: 'it broke', data: { where: 'page' } });
    expect(missing).toMatchObject({ code: 'unknown-method' });
  });

  it('given a page that never answers, when the app waits, then the call times out', async () => {
    // Given the mounted page and a handler that never settles
    const { client } = await mountRpcPage();

    // When the app calls it with a short timeout
    const failure = await client.call('hang', undefined, { timeoutMs: 50 }).catch((e: unknown) => e);

    // Then the call fails as a timeout
    expect(failure).toMatchObject({ code: 'timeout' });
  });

  it('given two calls answered out of order and a burst of events, when they are in flight, then each call gets its own answer and every event arrives in order', async () => {
    // Given the mounted page
    const { client, ticks } = await mountRpcPage();

    // When a slow and a quick call are in flight and the page then emits five events
    const [slow, quick] = await Promise.all([client.call('echo', { ms: 60, tag: 'slow' }), client.call('echo', { ms: 1, tag: 'quick' })]);
    await client.call('startTicks', { count: 5 });

    // Then each call has its own params back, and the five events were heard in order
    expect(slow).toEqual({ ms: 60, tag: 'slow' });
    expect(quick).toEqual({ ms: 1, tag: 'quick' });
    await waitFor(() => expect(ticks).toEqual([1, 2, 3, 4, 5]));
  });
});
