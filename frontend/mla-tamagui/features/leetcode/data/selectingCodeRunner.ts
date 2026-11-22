import type { CodeRunnerPort, RunnerChoice } from '../domain/ports';
import { deviceLanguageOf, isPythonLanguage, type DeviceLanguage } from '../domain/run/runLanguage';
import { needsDevice } from '../domain/run/runnerRouting';

/** The languages whose runner is on the device today; JavaScript joins when its runner is reachable. */
const DEVICE_LANGUAGES: readonly DeviceLanguage[] = ['python3'];

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
      const onDevice = isPythonLanguage(params.lang) && (choice === 'device' || needsDevice(params));
      return (onDevice ? runners.device : runners.backend).run(params);
    },
    warmUp: (lang) => {
      if (current() !== 'device') return;
      const language = lang === undefined ? 'python3' : deviceLanguageOf(lang);
      if (language && DEVICE_LANGUAGES.includes(language)) runners.device.warmUp?.(language);
    },
  };
}
