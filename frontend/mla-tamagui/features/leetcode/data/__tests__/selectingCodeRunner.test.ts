import { describe, expect, it } from 'vitest';

import type { CodeRunnerPort, RunCodeParams, RunnerChoice } from '../../domain/ports';
import { passingRun } from '../../testing/fakeCodeRunner';
import { createSelectingCodeRunner } from '../selectingCodeRunner';
import { createUnavailableCodeRunner } from '../unavailableCodeRunner';

const params = (lang: string): RunCodeParams => ({ lang, code: 'c', methodName: 'm', testCases: [] });

function setup(choice: RunnerChoice) {
  const log: string[] = [];
  const runner = (name: string): CodeRunnerPort => ({
    run: async () => {
      log.push(name);
      return passingRun();
    },
    warmUp: () => void log.push(`${name} warm`),
  });
  const selecting = createSelectingCodeRunner({ choose: async () => choice, current: () => choice, runners: { backend: runner('backend'), device: runner('device') } });
  return { log, selecting };
}

describe('createSelectingCodeRunner', () => {
  it('Given the backend setting, When a Python run is made, Then the backend runs it', async () => {
    const { log, selecting } = setup('backend');
    await selecting.run(params('python3'));
    expect(log).toEqual(['backend']);
  });

  it('Given the device setting, When a Python run is made, Then the device runs it and the backend is not called', async () => {
    const { log, selecting } = setup('device');
    await selecting.run(params('python3'));
    await selecting.run(params('python'));
    expect(log).toEqual(['device', 'device']);
  });

  it('Given the device setting, When a JavaScript run is made, Then the backend runs it because only Python runs on the device', async () => {
    const { log, selecting } = setup('device');
    await selecting.run(params('javascript'));
    expect(log).toEqual(['backend']);
  });

  it('Given the backend setting, When a Python run with an approx case is made, Then the device runner answers and the backend is not called', async () => {
    const { log, selecting } = setup('backend');
    await selecting.run({ ...params('python3'), testCases: [{ inputs: { x: 1 }, expected: 1.5, compareMode: 'approx' }] });
    await selecting.run({ ...params('python3'), testCases: [{ inputs: { x: 1 }, expected: 1, compareMode: 'exact' }] });
    expect(log).toEqual(['device', 'backend']);
  });

  it('Given each setting, When warmUp is called, Then only the device setting warms the device runner', () => {
    const device = setup('device');
    device.selecting.warmUp?.();
    const backend = setup('backend');
    backend.selecting.warmUp?.();
    expect([device.log, backend.log]).toEqual([['device warm'], []]);
  });
});

describe('createUnavailableCodeRunner', () => {
  it('Given no device runner is mounted, When it is run, Then it rejects as unavailable at once', async () => {
    await expect(createUnavailableCodeRunner().run(params('python3'))).rejects.toMatchObject({ name: 'CodeRunnerError', kind: 'unavailable' });
  });
});
