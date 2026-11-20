import type { CodeRunnerPort, RunnerChoice } from '../domain/ports';

const isDeviceLanguage = (lang: string) => lang === 'python3' || lang === 'python';

/**
 * The runner Run Local calls. The setting is read per call (the container is built once), and only Python can run on the device:
 * every other language goes to the backend whatever the setting says.
 */
export function createSelectingCodeRunner(config: {
  choose: () => Promise<RunnerChoice>;
  current: () => RunnerChoice;
  runners: Record<RunnerChoice, CodeRunnerPort>;
}): CodeRunnerPort {
  const { choose, current, runners } = config;
  return {
    run: async (params) => {
      const choice = await choose();
      return (choice === 'device' && isDeviceLanguage(params.lang) ? runners.device : runners.backend).run(params);
    },
    warmUp: () => {
      if (current() === 'device') runners.device.warmUp?.();
    },
  };
}
