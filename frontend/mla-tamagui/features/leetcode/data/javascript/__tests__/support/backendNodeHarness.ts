import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createContext, runInContext } from 'node:vm';

import type { TestResult } from '../../../../domain/run/execution';
import type { JudgePayload } from '../../../runner/judgeProtocol';

const SOURCE_PATH = join(__dirname, '../fixtures/backend-node-harness.js');

export const backendHarnessSource = (): string => readFileSync(SOURCE_PATH, 'utf8');

export type BackendRun = { ok?: boolean; results?: TestResult[]; error?: string };

/**
 * Runs the backend's own Node harness (a byte copy of rs/harness/node.js) in a vm context with a fake process: the payload arrives on
 * stdin, the answer is what it writes to stdout. A module-level print lands in that same stdout in the real process, so it is the
 * run the backend cannot parse.
 */
export function runBackendHarness(payload: Pick<JudgePayload, 'code' | 'methodName' | 'testCases'>): Promise<BackendRun> {
  return new Promise((resolve, reject) => {
    const handlers: Record<string, (chunk?: string) => void> = {};
    let polluted = false;
    const fakeProcess = {
      stdin: {
        on: (event: string, handler: (chunk?: string) => void) => {
          handlers[event] = handler;
        },
      },
      stdout: {
        write: (text: string) => resolve(polluted ? { ok: false, error: 'Runner produced no parseable output' } : (JSON.parse(text) as BackendRun)),
      },
      stderr: { write: (text: string) => reject(new Error(text)) },
      exit: (code: number) => reject(new Error(`the harness exited with ${code}`)),
    };
    const sandbox = {
      process: fakeProcess,
      console: {
        log: () => {
          polluted = true;
        },
      },
    };
    runInContext(backendHarnessSource(), createContext(sandbox), { filename: 'backend-node-harness.js' });
    handlers.data?.(JSON.stringify(payload));
    handlers.end?.();
  });
}
