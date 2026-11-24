import type { CodeRunnerPort } from '../../domain/ports';
import { deviceLanguageOf, type DeviceLanguage } from '../../domain/run/runLanguage';
import { runResponseOf } from '../../domain/run/verdict';

/**
 * The one on-device CodeRunnerPort: a run, and a warm-up, goes to the runner of its language. A language with no runner
 * answers as the page runner does for a language it does not handle, so the caller never has to know which engines exist.
 * A warm-up with no language is Python's, as it was before there was another engine.
 */
export function createDeviceRunnerRouter(runners: Partial<Record<DeviceLanguage, CodeRunnerPort>>): CodeRunnerPort {
  return {
    run(params) {
      const language = deviceLanguageOf(params.lang);
      const runner = language ? runners[language] : undefined;
      if (!runner) return Promise.resolve(runResponseOf({ ok: false, error: `Unsupported language: ${params.lang}`, error_kind: 'compile' }, 'offline'));
      return runner.run(params);
    },
    warmUp(lang) {
      const language = lang === undefined ? 'python3' : deviceLanguageOf(lang);
      if (language) runners[language]?.warmUp?.(language);
    },
  };
}
