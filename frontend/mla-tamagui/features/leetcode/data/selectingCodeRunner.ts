import type { CodeRunnerPort, RunnerChoice } from '../domain/ports';
import { needsDevice } from '../domain/run/runnerRouting';

const isDeviceLanguage = (lang: string) => lang === 'python3' || lang === 'python';

/**
 * The runner Run Local calls. The setting is read per call (the container is built once), and only Python can run on the device:
 * every other language goes to the backend whatever the setting says. A Python run the backend cannot score goes to the device
 * whatever the setting says.
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
      const onDevice = isDeviceLanguage(params.lang) && (choice === 'device' || needsDevice(params));
      return (onDevice ? runners.device : runners.backend).run(params);
    },
    warmUp: () => {
      if (current() === 'device') runners.device.warmUp?.();
    },
  };
}
