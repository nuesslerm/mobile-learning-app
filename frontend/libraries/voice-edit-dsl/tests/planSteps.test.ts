import { describe, expect, it } from 'vitest';

import { applyCommands } from '../src/use-cases/executor';
import {
  buildStepRealizeRequest,
  linesProduced,
  planAnchor,
  splitBlock,
  type PlanStep,
} from '../src/use-cases/planSteps';

const STUB =
  'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        ';
const SOLVED =
  'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        return True';

describe('splitBlock', () => {
  it('splits a multi-sentence block into one trimmed step per sentence, dropping empties', () => {
    // Given a spoken block with sentences and line breaks
    // When split into steps
    // Then each sentence becomes a trimmed step, and stray separators are dropped
    expect(splitBlock('first. second?\nthird! \nfourth')).toEqual([
      'first',
      'second',
      'third',
      'fourth',
    ]);
  });

  it('a terminator-less block is a single step', () => {
    // Given a block with no sentence terminator
    // When split into steps
    // Then it stays one whole step
    expect(splitBlock('keep only alphanumeric characters')).toEqual([
      'keep only alphanumeric characters',
    ]);
  });
});

describe('planAnchor', () => {
  it('replaces the whitespace-only stub line of a python body when nothing is realised yet', () => {
    // Given the starter code whose body is a whitespace-only stub line
    // When the first step is anchored
    // Then it lands on that stub line as a replaceStub
    expect(planAnchor(STUB, [], 0)).toEqual({ kind: 'replaceStub', line: 3 });
  });

  it('anchors after the nearest realised earlier step, at its endLine', () => {
    // Given a step realised on lines 3-4
    const steps: PlanStep[] = [
      { id: '1', text: 'a', status: 'realized', lines: { startLine: 3, endLine: 4 } },
    ];
    // When the next step is anchored
    // Then it inserts after line 4, ignoring the stub line
    expect(planAnchor(SOLVED, steps, 1)).toEqual({
      kind: 'insertAfterLine',
      line: 4,
    });
  });

  it('a realised step without lines does not anchor; the next realised one does', () => {
    // Given the nearest earlier realised step has no line range and an older one does
    const steps: PlanStep[] = [
      { id: '1', text: 'a', status: 'realized', lines: { startLine: 3, endLine: 3 } },
      { id: '2', text: 'b', status: 'realized', lines: null },
    ];
    // When the third step is anchored
    // Then it falls back to the older realised step's endLine
    expect(planAnchor(SOLVED, steps, 2)).toEqual({
      kind: 'insertAfterLine',
      line: 3,
    });
  });

  it('with no realised step and a non-stub buffer, inserts after the last non-blank line', () => {
    // Given code with a real body and no trailing stub
    // When a step is anchored before anything is realised
    // Then it inserts after the last non-blank line
    expect(planAnchor(SOLVED, [], 0)).toEqual({
      kind: 'insertAfterLine',
      line: 3,
    });
  });
});

describe('buildStepRealizeRequest', () => {
  it('names the anchor line and the step text for an insertAfterLine anchor', () => {
    // Given an insertAfterLine anchor on line 3
    const anchor = planAnchor(SOLVED, [], 0);
    const step: PlanStep = {
      id: '1',
      text: 'return true if the string is a palindrome',
      status: 'pending',
      lines: null,
    };
    // When the realise request is built
    const request = buildStepRealizeRequest(SOLVED, 'python3', step, anchor);
    // Then the anchor is exactly line 3 and the transcript says to insert after it, unchanged
    expect(anchor).toEqual({ kind: 'insertAfterLine', line: 3 });
    expect(request.startLine).toBe(3);
    expect(request.endLine).toBe(3);
    expect(request.transcript).toContain('line 3');
    expect(request.transcript).toContain(step.text);
    expect(request.transcript).toContain('Do not change line 3');
  });

  it('tells the planner to replace the stub line with the step code', () => {
    // Given a replaceStub anchor on the stub line
    const step: PlanStep = {
      id: '1',
      text: 'keep only alphanumeric characters',
      status: 'pending',
      lines: null,
    };
    // When the realise request is built
    const request = buildStepRealizeRequest(
      STUB,
      'python3',
      step,
      planAnchor(STUB, [], 0)
    );
    // Then the transcript names the stub line and the step text
    expect(request.startLine).toBe(3);
    expect(request.transcript).toContain('line 3');
    expect(request.transcript).toContain(step.text);
  });
});

describe('linesProduced', () => {
  it('counts the lines an insertAfterLine edit added, starting the line after the anchor', () => {
    // Given a buffer and an edit that inserts two lines after line 2
    const before = 'a\nb\nc';
    const after = applyCommands(before, [
      { type: 'insertAfterLine', line: 2, text: 'd\ne' },
    ]).text;
    // When the produced span is measured
    // Then it covers exactly the added lines
    expect(linesProduced(before, after, { kind: 'insertAfterLine', line: 2 })).toEqual({
      startLine: 3,
      endLine: 4,
    });
  });

  it('counts the lines a replaceStub edit produced from the stub line on', () => {
    // Given the stub buffer and an edit that turns the stub into two code lines
    const after = applyCommands(STUB, [
      { type: 'replaceLines', startLine: 3, endLine: 3, newText: '        l = 0\n        r = 1' },
    ]).text;
    // When the produced span is measured
    // Then it covers the new code lines starting at the stub
    expect(linesProduced(STUB, after, { kind: 'replaceStub', line: 3 })).toEqual({
      startLine: 3,
      endLine: 4,
    });
  });

  it('is null when the edit changed nothing', () => {
    // Given an anchor and no change between before and after
    // When the produced span is measured
    // Then there are no produced lines
    expect(
      linesProduced('a\nb', 'a\nb', { kind: 'insertAfterLine', line: 1 })
    ).toBeNull();
  });
});
describe('planAnchor with an unrealised gap', () => {
  it('Given step 2 pending and step 1 realised, When step 3 is anchored, Then it falls back to the method end rather than step 1', () => {
    // Given step 1 realised over lines 3-3 and step 2 still pending
    const steps: PlanStep[] = [
      {
        id: 'a',
        text: 'first',
        status: 'realized',
        lines: { startLine: 3, endLine: 3 },
        origin: 'text',
      },
      { id: 'b', text: 'second', status: 'pending', lines: null, origin: 'text' },
      { id: 'c', text: 'third', status: 'pending', lines: null, origin: 'text' },
    ];

    // When the third step is anchored against a four-line buffer
    const code =
      'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        left = 0\n        return True';
    const anchor = planAnchor(code, steps, 2);

    // Then it lands at the end of the method, not where step 2 is going to go
    expect(anchor).toEqual({ kind: 'insertAfterLine', line: 4 });
  });
});
