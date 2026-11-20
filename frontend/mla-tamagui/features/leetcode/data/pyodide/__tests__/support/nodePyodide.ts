import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { loadPyodide, type PyodideInterface } from 'pyodide';

import { rawRunSchema, type RawRun } from '../../../../domain/verdict';
import type { JudgePayload } from '../../judgeProtocol';

const HERE = dirname(new URL(import.meta.url).pathname);
const JUDGE_SOURCE = readFileSync(join(HERE, '../../harness/judge.py'), 'utf8');
const BACKEND_SOURCE = readFileSync(join(HERE, '../fixtures/backend-python-harness.py'), 'utf8');

export type NodeJudge = {
  py: PyodideInterface;
  /** the on-device harness */
  run(payload: JudgePayload, onCase?: (row: string) => void): RawRun;
  /** the leetcode-backend's own script, run verbatim in the same interpreter (SIGALRM, which Pyodide lacks, is a no-op) with what its process runner sends */
  runBackend(payload: JudgePayload): RawRun;
};

/** One Pyodide for a test file: `indexURL` is needed, or Pyodide derives its directory from the source-mapped stack and finds nothing. */
export async function bootNodeJudge(): Promise<NodeJudge> {
  const indexURL = dirname(createRequire(import.meta.url).resolve('pyodide/package.json'));
  const py = await loadPyodide({ indexURL, env: { PYTHONHASHSEED: '0' }, stdin: () => null, stdout: () => undefined, stderr: () => undefined });
  const ns = py.globals.get('dict')();
  py.runPython(JUDGE_SOURCE, { globals: ns });
  const judge = ns.get('run');
  py.runPython(`
import io, sys, types
BACKEND = ${JSON.stringify(BACKEND_SOURCE)}
def run_backend(payload_json):
    shim = types.ModuleType('signal')
    shim.SIGALRM = 14
    shim.signal = lambda *a: None
    shim.alarm = lambda *a: 0
    saved = (sys.stdin, sys.stdout, sys.modules.get('signal'))
    sys.modules['signal'] = shim
    sys.stdin = io.StringIO(payload_json)
    sys.stdout = captured = io.StringIO()
    try:
        exec(compile(BACKEND, 'backend_python.py', 'exec'), {'__name__': '__main__'})
    except SystemExit:
        pass
    finally:
        sys.stdin, sys.stdout = saved[0], saved[1]
        if saved[2] is None:
            del sys.modules['signal']
        else:
            sys.modules['signal'] = saved[2]
    return captured.getvalue()
`);
  const backend = py.globals.get('run_backend');
  return {
    py,
    run: (payload, onCase) => rawRunSchema.parse(JSON.parse(onCase ? judge(JSON.stringify(payload), onCase) : judge(JSON.stringify(payload)))),
    runBackend: (payload) => {
      const sent = { mode: 'run', code: payload.code, methodName: payload.methodName, testCases: payload.testCases, timeoutMs: payload.timeoutMs ?? 5000 };
      const text = String(backend(JSON.stringify(sent))).trim();
      try {
        return rawRunSchema.parse(JSON.parse(text));
      } catch {
        // what the backend's process runner answers when stdout is not one JSON document
        return { ok: false, results: [], error: 'Runner produced no parseable output' };
      }
    },
  };
}
