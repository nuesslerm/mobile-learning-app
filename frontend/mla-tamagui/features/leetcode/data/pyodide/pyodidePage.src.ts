// The page the hidden WebView runs: it answers the app's calls over @lib/webview-rpc, receives the Pyodide files (pulled by
// URL, or pushed in base64 chunks), boots the interpreter from memory and runs code. Compiled to pyodidePage.ts by
// scripts/pyodide-bundle.mjs.
import { createPageTransport, createRpcServer, type PageWindow } from '@lib/webview-rpc';
import { loadPyodide, type PyodideInterface } from 'pyodide';

type FileName = 'asm' | 'lock' | 'wasm' | 'stdlib';
type FileUrls = Record<FileName, string>;
type Chunk = { name: FileName; index: number; total: number; data: string };

// Pyodide asks for the wasm under this address; the page answers it from memory.
const INDEX_URL = 'https://pyodide.invalid/';

const files: Partial<Record<FileName, Uint8Array>> = {};
const pieces = new Map<FileName, Uint8Array[]>();
const stdout: string[] = [];
let python: PyodideInterface | undefined;

function bytesOf(base64: string): Uint8Array {
  const raw = atob(base64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i += 1) out[i] = raw.charCodeAt(i);
  return out;
}

function joined(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

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

function need(name: FileName): Uint8Array {
  const file = files[name];
  if (!file) throw new Error(`the ${name} file has not arrived`);
  return file;
}

function megabytes(bytes: number | undefined): number | null {
  return bytes === undefined ? null : Math.round((bytes / 1048576) * 10) / 10;
}

const server = createRpcServer(createPageTransport(window as unknown as PageWindow), {
  ping: () => Date.now(),

  async pull(params) {
    const urls = params as FileUrls;
    const started = performance.now();
    const names = Object.keys(urls) as FileName[];
    await Promise.all(
      names.map(async (name) => {
        files[name] = new Uint8Array(await readFile(urls[name]));
      })
    );
    return { ms: Math.round(performance.now() - started) };
  },

  chunk(params) {
    const { name, index, total, data } = params as Chunk;
    const parts = index === 0 ? [] : (pieces.get(name) ?? []);
    parts.push(bytesOf(data));
    pieces.set(name, parts);
    if (index === total - 1) {
      files[name] = joined(parts);
      pieces.delete(name);
    }
    return null;
  },

  async boot() {
    const started = performance.now();
    const asm = URL.createObjectURL(new Blob([need('asm') as BlobPart], { type: 'text/javascript' }));
    const stdlib = URL.createObjectURL(new Blob([need('stdlib') as BlobPart]));
    const wasm = need('wasm');
    const realFetch = window.fetch.bind(window);
    window.fetch = (input, init) =>
      String(input).startsWith(INDEX_URL)
        ? Promise.resolve(new Response(wasm as BodyInit, { headers: { 'Content-Type': 'application/wasm' } }))
        : realFetch(input, init);
    const module = (await import(asm)) as { default: unknown };
    const imported = performance.now();
    python = await loadPyodide({
      indexURL: INDEX_URL,
      lockFileContents: new TextDecoder().decode(need('lock')),
      stdLibURL: stdlib,
      createPyodideModule: module.default as never,
      stdout: (line) => stdout.push(line),
      stderr: (line) => stdout.push(line),
    });
    window.fetch = realFetch;
    return { importMs: Math.round(imported - started), loadMs: Math.round(performance.now() - imported), version: python.version };
  },

  async run(params) {
    if (!python) throw new Error('the interpreter has not booted');
    stdout.length = 0;
    const started = performance.now();
    const outcome = await python.runPythonAsync((params as { code: string }).code).then(
      (result: unknown) => ({ value: result === undefined ? null : String(result), error: null as string | null }),
      (thrown: unknown) => ({ value: null as string | null, error: thrown instanceof Error ? thrown.message : String(thrown) })
    );
    return { stdout: stdout.join('\n'), ...outcome, ms: Math.round(performance.now() - started) };
  },

  memory() {
    const heap = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize;
    const wasmHeap = (python as unknown as { _module?: { HEAPU8?: { length: number } } } | undefined)?._module?.HEAPU8?.length;
    return { jsHeapMb: megabytes(heap), wasmHeapMb: megabytes(wasmHeap) };
  },
});

server.emit('ready');
