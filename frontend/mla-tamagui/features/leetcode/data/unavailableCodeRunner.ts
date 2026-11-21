import { CodeRunnerError, type CodeRunnerPort } from '../domain/ports';

/** The device runner of a tree that has none mounted: choosing it fails at once and says why. */
export function createUnavailableCodeRunner(): CodeRunnerPort {
  return {
    run: () => Promise.reject(new CodeRunnerError('unavailable', 'On-device runs are not available in this app tree')),
  };
}
