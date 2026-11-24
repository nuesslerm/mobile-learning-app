import { createContext, runInContext } from 'node:vm';
import { describe, expect, it } from 'vitest';

import { CONFIG_KEY, LOADED_KEY, type HarnessConfig } from '../workerProtocol';
import { composeWorkerScript } from '../workerSource';

const CONFIG: HarnessConfig = { methodName: 'twoSum', paramTypes: {}, returnType: null, inPlace: null, harness: null };
const PRELUDE = 'void 0';

function reportedLine(script: string): number {
  try {
    runInContext(script, createContext({}), { filename: 'worker.js' });
    return 0;
  } catch (error) {
    return Number(/^worker\.js:(\d+)/.exec((error as Error).stack ?? '')?.[1]);
  }
}

describe('composeWorkerScript', () => {
  it('Given the learner’s code, When the script is composed, Then line N of the code is line N of the script, with the prelude and the configuration on line 1', () => {
    const code = 'var a = 1;\nvar b = 2;\nvar c = 3;';
    const { script } = composeWorkerScript({ prelude: PRELUDE, config: CONFIG, code });
    const lines = script.split('\n');
    expect(lines[0]).toContain('"methodName":"twoSum"');
    expect(lines[0]?.endsWith('var a = 1;')).toBe(true);
    expect(lines.slice(1, 3)).toEqual(['var b = 2;', 'var c = 3;']);
    expect(lines).toHaveLength(4);
  });

  it('Given a leading comment, When the script is composed, Then the comment is on line 1 after the prelude and later lines keep their numbers', () => {
    const code = '// the first line is a comment\nvar twoSum = 1;\nvar x = ;';
    const { script, firstLineOffset } = composeWorkerScript({ prelude: PRELUDE, config: CONFIG, code });
    expect(script.slice(firstLineOffset).startsWith('// the first line is a comment\n')).toBe(true);
    expect(script.split('\n')[2]).toBe('var x = ;');
  });

  it('Given code without a trailing newline, When the script is composed, Then the closing call is on a line of its own after the last line of code', () => {
    const code = 'var a = 1; // no newline after this';
    const { script } = composeWorkerScript({ prelude: PRELUDE, config: CONFIG, code });
    const lines = script.split('\n');
    expect(lines).toHaveLength(2);
    expect(lines[1]).toBe(`;globalThis[Symbol.for(${JSON.stringify(LOADED_KEY)})]()`);
  });

  it('Given the prelude and the configuration, When the script is composed, Then firstLineOffset is the length of line 1 before the learner’s first character and the head holds no newline', () => {
    const { script, firstLineOffset } = composeWorkerScript({ prelude: PRELUDE, config: CONFIG, code: 'CODE' });
    expect(script.slice(firstLineOffset, firstLineOffset + 4)).toBe('CODE');
    expect(script.slice(0, firstLineOffset)).not.toContain('\n');
    expect(script.slice(0, firstLineOffset)).toContain(PRELUDE);
  });

  it('Given a comment as the last text of the code, When the script runs, Then the closing call still runs', () => {
    const { script } = composeWorkerScript({ prelude: PRELUDE, config: CONFIG, code: 'var kept = 1; // trailing comment' });
    let called = 0;
    const sandbox: Record<symbol, unknown> = { [Symbol.for(LOADED_KEY)]: () => void (called += 1) };
    runInContext(script, createContext(sandbox));
    expect(called).toBe(1);
  });

  it('Given the configuration, When the script runs, Then the prelude finds it under its symbol key', () => {
    const seen: unknown[] = [];
    const prelude = `globalThis.seen(globalThis[Symbol.for(${JSON.stringify(CONFIG_KEY)})])`;
    const { script } = composeWorkerScript({ prelude, config: CONFIG, code: '' });
    const sandbox: Record<PropertyKey, unknown> = { seen: (value: unknown) => void seen.push(value), [Symbol.for(LOADED_KEY)]: () => undefined };
    runInContext(script, createContext(sandbox));
    expect(JSON.parse(JSON.stringify(seen[0]))).toEqual(CONFIG);
  });

  it('Given a syntax error on line 3 of the code, When the script is compiled, Then the engine reports line 3', () => {
    const { script } = composeWorkerScript({ prelude: PRELUDE, config: CONFIG, code: 'var a = 1;\nvar b = 2;\nvar c = ;' });
    expect(reportedLine(script)).toBe(3);
  });
});
