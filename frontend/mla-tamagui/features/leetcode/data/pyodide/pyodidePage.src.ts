// The page the hidden WebView runs: it answers the app's calls over @lib/webview-rpc (handlers in pageHandlers.ts), receives the
// Pyodide files (pulled by URL, or pushed in base64 chunks), boots the interpreter from memory and runs the judge. Compiled to
// pyodidePage.ts by scripts/pyodide-bundle.mjs.
import { createPageTransport, createRpcServer, type PageWindow } from '@lib/webview-rpc';
import { loadPyodide } from 'pyodide';

import judgeSource from './harness/judge.py';
import { createPageHandlers, type PageDeps } from './pageHandlers';

// Pyodide asks for the wasm under this address; the page answers it from memory.
const INDEX_URL = 'https://pyodide.invalid/';

function readFile(url: string): Promise<ArrayBuffer> {
  if (!url.startsWith('file:')) return fetch(url).then((response) => response.arrayBuffer());
  // fetch refuses file: URLs; XMLHttpRequest reads them when the WebView allows file access
  return new Promise((resolve, reject) => {
    const request = new XMLHttpRequest();
    request.open('GET', url);
    request.responseType = 'arraybuffer';
    request.onload = () => (request.status === 0 || request.status === 200 ? resolve(request.response as ArrayBuffer) : reject(new Error(`${url}: ${request.status}`)));
    request.onerror = () => reject(new Error(`${url}: the page could not read it`));
    request.send();
  });
}

const boot: PageDeps['boot'] = async (files) => {
  const need = (name: 'asm' | 'lock' | 'wasm' | 'stdlib'): Uint8Array => {
    const file = files[name];
    if (!file) throw new Error(`the ${name} file has not arrived`);
    return file;
  };
  const started = performance.now();
  const asm = URL.createObjectURL(new Blob([need('asm') as BlobPart], { type: 'text/javascript' }));
  const stdlib = URL.createObjectURL(new Blob([need('stdlib') as BlobPart]));
  const wasm = need('wasm');
  const realFetch = window.fetch.bind(window);
  window.fetch = (input, init) =>
    String(input).startsWith(INDEX_URL) ? Promise.resolve(new Response(wasm as BodyInit, { headers: { 'Content-Type': 'application/wasm' } })) : realFetch(input, init);
  const module = (await import(asm)) as { default: unknown };
  const imported = performance.now();
  const python = await loadPyodide({
    indexURL: INDEX_URL,
    lockFileContents: new TextDecoder().decode(need('lock')),
    stdLibURL: stdlib,
    createPyodideModule: module.default as never,
    // a solution that calls input() reads end of file instead of waiting for a prompt that cannot come
    stdin: () => null,
    env: { PYTHONHASHSEED: '0' },
    // what a solution prints is the judge's to capture per case; anything else is dropped
    stdout: () => undefined,
    stderr: () => undefined,
  });
  window.fetch = realFetch;
  return { python, importMs: Math.round(imported - started), loadMs: Math.round(performance.now() - imported) };
};

const server = createRpcServer(
  createPageTransport(window as unknown as PageWindow),
  createPageHandlers({
    readFile,
    boot,
    harnessSource: judgeSource,
    emit: (topic, data) => server.emit(topic, data),
    now: () => performance.now(),
    heapUsed: () => (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize,
  })
);
server.emit('ready');
