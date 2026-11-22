import { describe, expect, it } from 'vitest';

import type { CodeRunnerPort, RunCodeParams } from '../../../domain/ports';
import { passingRun } from '../../../testing/fakeCodeRunner';
import { createDeviceRunnerRouter } from '../deviceRunnerRouter';

const params = (lang: string): RunCodeParams => ({ lang, code: 'c', methodName: 'm', testCases: [] });

function recording(name: string, log: string[]): CodeRunnerPort {
  return {
    run: async (p) => {
      log.push(`${name} run ${p.lang}`);
      return passingRun();
    },
    warmUp: (lang) => void log.push(`${name} warm ${lang}`),
  };
}

describe('the device runner router', () => {
  it('Given a runner registered for python3 only, When a python3 or python run is made, Then that runner answers', async () => {
    const log: string[] = [];
    const router = createDeviceRunnerRouter({ python3: recording('py', log) });
    await router.run(params('python3'));
    await router.run(params('python'));
    expect(log).toEqual(['py run python3', 'py run python']);
  });

  it('Given a runner registered for python3 only, When a javascript run is made, Then the response is the offline compile error Unsupported language: javascript and no runner is called', async () => {
    const log: string[] = [];
    const router = createDeviceRunnerRouter({ python3: recording('py', log) });
    const response = await router.run(params('javascript'));
    expect(response).toMatchObject({ source: 'offline', status_msg: 'Error', compile_error: 'Unsupported language: javascript' });
    expect(log).toEqual([]);
  });

  it('Given a language no engine has, When a run is made, Then it is the same compile error', async () => {
    const router = createDeviceRunnerRouter({ python3: recording('py', []), javascript: recording('js', []) });
    expect(await router.run(params('cpp'))).toMatchObject({ compile_error: 'Unsupported language: cpp' });
  });

  it('Given both runners, When a run of each language is made, Then each language’s runner answers', async () => {
    const log: string[] = [];
    const router = createDeviceRunnerRouter({ python3: recording('py', log), javascript: recording('js', log) });
    await router.run(params('javascript'));
    await router.run(params('python3'));
    expect(log).toEqual(['js run javascript', 'py run python3']);
  });

  it('Given both runners, When warmUp(javascript) is called, Then only the JavaScript runner is warmed', () => {
    const log: string[] = [];
    const router = createDeviceRunnerRouter({ python3: recording('py', log), javascript: recording('js', log) });
    router.warmUp?.('javascript');
    expect(log).toEqual(['js warm javascript']);
  });

  it('Given both runners, When warmUp has no language, Then the Python runner is warmed as it always was', () => {
    const log: string[] = [];
    const router = createDeviceRunnerRouter({ python3: recording('py', log), javascript: recording('js', log) });
    router.warmUp?.();
    router.warmUp?.('python');
    expect(log).toEqual(['py warm python3', 'py warm python3']);
  });

  it('Given a language with no runner or none registered, When warmUp is called, Then nothing is warmed and nothing throws', () => {
    const log: string[] = [];
    const router = createDeviceRunnerRouter({ python3: recording('py', log) });
    expect(() => router.warmUp?.('javascript')).not.toThrow();
    expect(() => router.warmUp?.('cpp')).not.toThrow();
    expect(() => createDeviceRunnerRouter({}).warmUp?.()).not.toThrow();
    expect(log).toEqual([]);
  });
});
