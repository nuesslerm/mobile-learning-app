// The page side of the provider's flow test: the real rpc server and page transport answering like the Pyodide page, with no Python.
import { createPageTransport, createRpcServer, type PageWindow } from '@lib/webview-rpc';

type Case = { inputs: Record<string, unknown>; expected: unknown };
const loads = ((window as unknown as { __loads?: number }).__loads = ((window as unknown as { __loads?: number }).__loads ?? 0) + 1);

const server = createRpcServer(createPageTransport(window as unknown as PageWindow), {
  ping: () => Date.now(),
  pull: () => ({ ms: 1 }),
  chunk: () => null,
  boot: () => ({ importMs: 1, loadMs: 1, version: `fake-${loads}` }),
  judge: (params) => {
    const { runId, payload } = params as { runId: number; payload: { testCases: Case[] } };
    const results = payload.testCases.map((c) => ({ pass: true, input: c.inputs, actual: c.expected, expected: c.expected, stdout: '', stderr: '', error: null, elapsed_ms: 1 }));
    results.forEach((result, index) => server.emit('case', { runId, row: JSON.stringify({ index, result }) }));
    return JSON.stringify({ ok: true, results });
  },
});
server.emit('ready');
