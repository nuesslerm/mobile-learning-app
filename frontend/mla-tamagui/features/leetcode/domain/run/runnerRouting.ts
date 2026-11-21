import type { RunCodeParams } from '../ports';

/** The backend types compareMode as a closed enum, so a case with any other mode fails the whole request, not just its row. */
const BACKEND_COMPARE_MODES: readonly string[] = ['exact', 'sorted', 'setOfSorted'];

/** True for a run the backend cannot score: only the device runner can, whatever the Run Local setting says. */
export function needsDevice(params: RunCodeParams): boolean {
  return params.testCases.some((testCase) => testCase.compareMode !== undefined && !BACKEND_COMPARE_MODES.includes(testCase.compareMode));
}
