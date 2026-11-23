import type { CodeRunnerPort, RunnerChoice } from '../domain/ports';
import { deviceLanguageOf, isJavaScriptLanguage, isPythonLanguage } from '../domain/run/runLanguage';
import { needsDevice } from '../domain/run/runnerRouting';

/**
 * The runner Run Local calls. The setting is read per call (the container is built once). JavaScript always runs on the device,
 * because the backend's Node harness cannot call the catalog's starters; Python follows the setting unless the backend cannot
 * score the run; every other language goes to the backend.
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
      const onDevice = isJavaScriptLanguage(params.lang) || (isPythonLanguage(params.lang) && (choice === 'device' || needsDevice(params)));
      return (onDevice ? runners.device : runners.backend).run(params);
    },
    warmUp: (lang) => {
      const language = lang === undefined ? 'python3' : deviceLanguageOf(lang);
      if (!language) return;
      if (language === 'javascript' || current() === 'device') runners.device.warmUp?.(language);
    },
  };
}
