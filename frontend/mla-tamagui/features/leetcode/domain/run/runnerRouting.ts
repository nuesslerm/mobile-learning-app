import type { RunCodeParams } from '../ports';
import { isJavaScriptLanguage } from './runLanguage';

/** The backend types compareMode as a closed enum, so a case with any other mode fails the whole request, not just its row. */
const BACKEND_COMPARE_MODES: readonly string[] = ['exact', 'sorted', 'setOfSorted'];

/** True for a run the backend cannot score (JavaScript, whose starters its Node harness cannot call, and a harness problem, whose objects it cannot build): only the device runner can, whatever the Run Local setting says. */
export function needsDevice(params: RunCodeParams): boolean {
  return isJavaScriptLanguage(params.lang) || Boolean(params.harness) || params.testCases.some((testCase) => testCase.compareMode !== undefined && !BACKEND_COMPARE_MODES.includes(testCase.compareMode));
}
