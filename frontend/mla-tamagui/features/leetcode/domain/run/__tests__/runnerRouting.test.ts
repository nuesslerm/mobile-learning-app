import { describe, expect, it } from 'vitest';

import type { RunCodeParams } from '../../ports';
import type { TestCase } from '../execution';
import { needsDevice } from '../runnerRouting';

const params = (testCases: TestCase[]): RunCodeParams => ({ lang: 'python3', code: 'c', methodName: 'm', testCases });
const caseWith = (compareMode: TestCase['compareMode']): TestCase => ({ inputs: { x: 1 }, expected: 1, compareMode });

describe('needsDevice', () => {
  it('Given a case in approx mode, When needsDevice is asked, Then it is true, even beside cases the backend can score', () => {
    expect(needsDevice(params([caseWith('approx')]))).toBe(true);
    expect(needsDevice(params([caseWith('exact'), caseWith('approx')]))).toBe(true);
  });

  it('Given cases in exact, sorted and setOfSorted mode or in none, When needsDevice is asked, Then it is false', () => {
    expect(needsDevice(params([caseWith('exact'), caseWith('sorted'), caseWith('setOfSorted'), caseWith(undefined)]))).toBe(false);
    expect(needsDevice(params([]))).toBe(false);
  });
});
