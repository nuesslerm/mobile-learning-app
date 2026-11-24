import { describe, expect, it } from 'vitest';

import type { CodeRunnerPort, RunCodeParams, RunnerChoice } from '../../domain/ports';
import { passingRun } from '../../testing/fakeCodeRunner';
import { createSelectingCodeRunner } from '../selectingCodeRunner';
import { createUnavailableCodeRunner } from '../unavailableCodeRunner';

const params = (lang: string): RunCodeParams => ({ lang, code: 'c', methodName: 'm', testCases: [] });

function setup(choice: RunnerChoice) {
  const log: string[] = [];
  const warmedWith: (string | undefined)[] = [];
  const runner = (name: string): CodeRunnerPort => ({
    run: async () => {
      log.push(name);
      return passingRun();
    },
    warmUp: (lang) => {
      log.push(`${name} warm`);
      warmedWith.push(lang);
    },
  });
  const selecting = createSelectingCodeRunner({ choose: async () => choice, current: () => choice, runners: { backend: runner('backend'), device: runner('device') } });
  return { log, warmedWith, selecting };
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

  it('Given either setting, When a JavaScript run is made, Then the device runs it and the backend is never called', async () => {
    const device = setup('device');
    await device.selecting.run(params('javascript'));
    const backend = setup('backend');
    await backend.selecting.run(params('javascript'));
    expect([device.log, backend.log]).toEqual([['device'], ['device']]);
  });

  it('Given either setting, When a C++ run is made, Then the backend runs it', async () => {
    const device = setup('device');
    await device.selecting.run(params('cpp'));
    const backend = setup('backend');
    await backend.selecting.run(params('cpp'));
    expect([device.log, backend.log]).toEqual([['backend'], ['backend']]);
  });

  it('Given the backend setting, When a Python run with an approx case is made, Then the device runner answers and the backend is not called', async () => {
    const { log, selecting } = setup('backend');
    await selecting.run({ ...params('python3'), testCases: [{ inputs: { x: 1 }, expected: 1.5, compareMode: 'approx' }] });
    await selecting.run({ ...params('python3'), testCases: [{ inputs: { x: 1 }, expected: 1, compareMode: 'exact' }] });
    expect(log).toEqual(['device', 'backend']);
  });

  it('Given the backend setting, When a Python run with a harness is made, Then the device runner answers and the backend is not called', async () => {
    const { log, selecting } = setup('backend');
    await selecting.run({ ...params('python3'), harness: { kind: 'design', className: 'Trie' } });
    await selecting.run({ ...params('python3'), harness: null });
    expect(log).toEqual(['device', 'backend']);
  });

  it('Given each setting, When warmUp is called, Then only the device setting warms the device runner', () => {
    const device = setup('device');
    device.selecting.warmUp?.();
    const backend = setup('backend');
    backend.selecting.warmUp?.();
    expect([device.log, backend.log]).toEqual([['device warm'], []]);
  });

  it("Given the device setting, When warmUp('python3') is called, Then the device runner is warmed with 'python3'", () => {
    const { warmedWith, selecting } = setup('device');
    selecting.warmUp?.('python3');
    selecting.warmUp?.();
    expect(warmedWith).toEqual(['python3', 'python3']);
  });

  it("Given the backend setting, When warmUp('python3') is called, Then nothing is warmed", () => {
    const { log, selecting } = setup('backend');
    selecting.warmUp?.('python3');
    expect(log).toEqual([]);
  });

  it("Given the device setting, When warmUp('javascript') and warmUp('cpp') are called, Then only JavaScript is warmed", () => {
    const { log, warmedWith, selecting } = setup('device');
    selecting.warmUp?.('javascript');
    selecting.warmUp?.('cpp');
    expect([log, warmedWith]).toEqual([['device warm'], ['javascript']]);
  });

  it("Given the backend setting, When warmUp('javascript') is called, Then the device runner is warmed because JavaScript runs there whatever the setting says", () => {
    const { warmedWith, selecting } = setup('backend');
    selecting.warmUp?.('javascript');
    expect(warmedWith).toEqual(['javascript']);
  });
});

describe('createUnavailableCodeRunner', () => {
  it('Given no device runner is mounted, When it is run, Then it rejects as unavailable at once', async () => {
    await expect(createUnavailableCodeRunner().run(params('python3'))).rejects.toMatchObject({ name: 'CodeRunnerError', kind: 'unavailable' });
  });
});
