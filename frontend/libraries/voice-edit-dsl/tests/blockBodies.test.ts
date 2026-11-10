import { describe, expect, it } from 'vitest';

import { applyCommands } from '../src/use-cases/executor';
import { completeBlockBodies } from '../src/use-cases/blockBodies';
import type { EditAlternative } from '../src/contracts/schema';

const code = 'class Solution:\n    def twoSum(self, nums, target):\n        visited = {}\n';

const alt = (commands: EditAlternative['commands']): EditAlternative => ({ id: 'a', label: 'x', summary: 'y', confidence: 0.9, commands });

describe('completeBlockBodies', () => {
  it('Given an inserted loop header with nothing under it, When completed, Then a pass body keeps the code parsing', () => {
    // Given the planner stopped at the header
    const before = alt([{ type: 'insertAfterLine', line: 3, text: '        for i, n in enumerate(nums):' }]);

    // When it is completed and applied
    const { text } = applyCommands(code, completeBlockBodies(before, code, 'python').commands);

    // Then the header has an indented pass under it
    expect(text).toContain('        for i, n in enumerate(nums):\n            pass');
  });

  it('Given a header that brings its own body, When completed, Then nothing is added', () => {
    const own = alt([{ type: 'insertAfterLine', line: 3, text: '        for i, n in enumerate(nums):\n            seen = i' }]);
    expect(completeBlockBodies(own, code, 'python')).toEqual(own);
  });

  it('Given a header placed above a deeper line, When completed, Then that line is its body and nothing is added', () => {
    const withBody = 'def f():\n    x = 1\n';
    const before = alt([{ type: 'insertAfterLine', line: 1, text: 'if x:' }]);
    expect(completeBlockBodies(before, 'def f():\n    if x:\n        y = 1\n', 'python')).toEqual(alt([{ type: 'insertAfterLine', line: 1, text: 'if x:' }]));
    expect(withBody).toBeTruthy();
  });

  it('Given another language, When completed, Then it is left alone', () => {
    const before = alt([{ type: 'insertAfterLine', line: 1, text: 'for (const n of nums) {' }]);
    expect(completeBlockBodies(before, 'function f(nums) {\n', 'typescript')).toEqual(before);
  });
});
