import type { RpcHandlers } from '@lib/webview-rpc';
import type { PyodideInterface } from 'pyodide';

import type { JudgeParams } from './judgeProtocol';

export type PyodideFileName = 'asm' | 'lock' | 'wasm' | 'stdlib';
type FileUrls = Record<PyodideFileName, string>;
type Chunk = { name: PyodideFileName; index: number; total: number; data: string };

/** What the page's handlers need from the world around them, so the same handlers run in a WebView, in the browser harness and in Node tests. */
export type PageDeps = {
  readFile(url: string): Promise<ArrayBuffer>;
  /** Boots the interpreter from the files the page holds; the spike's `run` reads what Python printed through `onOutput`. */
  boot(files: Partial<Record<PyodideFileName, Uint8Array>>, onOutput: (line: string) => void): Promise<{ python: PyodideInterface; importMs: number; loadMs: number }>;
  /** The judge harness (data/pyodide/harness/judge.py) as source text. */
  harnessSource: string;
  emit(topic: string, data?: unknown): void;
  now(): number;
  heapUsed(): number | undefined;
};

const bytesOf = (base64: string): Uint8Array => Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));

function joined(parts: Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((sum, part) => sum + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

const megabytes = (bytes: number | undefined): number | null => (bytes === undefined ? null : Math.round((bytes / 1048576) * 10) / 10);

type JudgeRun = (payloadJson: string, onCase: (row: string) => void) => string;

export function createPageHandlers(deps: PageDeps): RpcHandlers {
  const files: Partial<Record<PyodideFileName, Uint8Array>> = {};
  const pieces = new Map<PyodideFileName, Uint8Array[]>();
  const output: string[] = [];
  let python: PyodideInterface | undefined;
  let judge: JudgeRun | undefined;

  return {
    ping: () => Date.now(),

    async pull(params) {
      const urls = params as FileUrls;
      const started = deps.now();
      await Promise.all(
        (Object.keys(urls) as PyodideFileName[]).map(async (name) => {
          files[name] = new Uint8Array(await deps.readFile(urls[name]));
        })
      );
      return { ms: Math.round(deps.now() - started) };
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
      const booted = await deps.boot(files, (line) => output.push(line));
      python = booted.python;
      // a private namespace, so nothing the harness defines lands in Pyodide's globals
      const namespace = python.globals.get('dict')();
      python.runPython(deps.harnessSource, { globals: namespace });
      judge = namespace.get('run') as JudgeRun;
      return { importMs: booted.importMs, loadMs: booted.loadMs, version: python.version };
    },

    judge(params) {
      if (!judge) throw new Error('the interpreter has not booted');
      const { runId, payload } = params as JudgeParams;
      return judge(JSON.stringify(payload), (row) => deps.emit('case', { runId, row }));
    },

    async run(params) {
      if (!python) throw new Error('the interpreter has not booted');
      output.length = 0;
      const started = deps.now();
      const outcome = await python.runPythonAsync((params as { code: string }).code).then(
        (result: unknown) => ({ value: result === undefined ? null : String(result), error: null as string | null }),
        (thrown: unknown) => ({ value: null as string | null, error: thrown instanceof Error ? thrown.message : String(thrown) })
      );
      return { stdout: output.join('\n'), ...outcome, ms: Math.round(deps.now() - started) };
    },

    memory() {
      const wasmHeap = (python as unknown as { _module?: { HEAPU8?: { length: number } } } | undefined)?._module?.HEAPU8?.length;
      return { jsHeapMb: megabytes(deps.heapUsed()), wasmHeapMb: megabytes(wasmHeap) };
    },
  };
}
