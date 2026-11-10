import { describe, it, expect } from 'vitest';

import { applyCommands } from '../src/use-cases/executor';
import { parseLocalEdit, soundKey } from '../src/use-cases/localEdits';

const code = 'class Solution:\n    def f(self, s):\n        left, right = 0, 1\n        while left < right:\n            left += 1\n';

describe('parseLocalEdit', () => {
  it('resolves a rename without the planner', () => {
    // Given the exact sentence a speaker uses, as whisper transcribes it
    const alt = parseLocalEdit('Rename the variable left to low.', code);

    // Then a rename is produced locally, and applying it renames every use
    expect(alt).not.toBeNull();
    const { text } = applyCommands(code, alt!.commands);
    expect(text).toContain('low, right = 0, 1');
    expect(text).toContain('while low < right:');
    expect(text).toContain('low += 1');
  });

  it('accepts the quoted form whisper often produces', () => {
    // Given whisper's habit of quoting the names it hears
    const alt = parseLocalEdit('Rename the variable "right" to "high" everywhere.', code);

    // Then it still resolves
    expect(alt?.commands[0]).toMatchObject({ type: 'renameSymbol', newName: 'high' });
  });

  it('declines when nothing in the buffer is or sounds like the name', () => {
    // Given a name that is not in the buffer and sounds like nothing in it
    const alt = parseLocalEdit('Rename the variable middle to low.', code);

    // Then it falls through to the planner instead of silently doing nothing
    expect(alt).toBeNull();
  });

  it('declines anything that is not unmistakably a rename', () => {
    // Given an instruction that merely mentions a variable
    expect(parseLocalEdit('add a while loop over left and right', code)).toBeNull();
    expect(parseLocalEdit('move left to the top of the function', code)).toBeNull();
  });
});

describe('whisper spellings of "rename"', () => {
  it('matches the split forms whisper produces', () => {
    // Given the transcript a live run actually returned
    const alt = parseLocalEdit(
      'We name the variable "right" to "high everywhere". Clear.',
      code
    );

    // Then the fast path still takes it -- a rename it misses is pure latency
    expect(alt?.commands[0]).toMatchObject({ type: 'renameSymbol', newName: 'high' });
  });
});

const twoSum =
  'class Solution:\n    def twoSum(self, nums, target):\n        seen = {}\n        for i, n in enumerate(nums):\n            if target - n in seen:\n                return [seen[target - n], i]\n            seen[n] = i\n';

describe('names whisper spells as another word', () => {
  it('renames the buffer name a homophone stands for', () => {
    // Given `seen`, which every whisper model writes as "scene"
    const alt = parseLocalEdit('Rename the variable scene to lookup.', twoSum);

    // When the rename is applied
    const { text } = applyCommands(twoSum, alt!.commands);

    // Then it renamed `seen`, locally
    expect(alt?.label).toBe('Rename seen to lookup');
    expect(text).toContain('lookup = {}');
    expect(text).toContain('return [lookup[target - n], i]');
    expect(text).not.toMatch(/\bseen\b/);
  });

  it('declines when two names in the buffer sound like the one heard', () => {
    // Given `seen` and `sen`, both a homophone of "scene"
    const code2 = twoSum.replace('seen = {}', 'seen = {}\n        sen = 0');

    // When the homophone is heard
    const alt = parseLocalEdit('Rename the variable scene to lookup.', code2);

    // Then the planner decides, not a guess
    expect(alt).toBeNull();
  });

  it('keys words by how they sound', () => {
    // Given homophones and a word that only shares a letter
    // When they are keyed
    // Then the homophones share a key and the other does not
    expect(soundKey('scene')).toBe(soundKey('seen'));
    expect(soundKey('write')).toBe(soundKey('right'));
    expect(soundKey('sum')).not.toBe(soundKey('seen'));
  });
});

describe('names whisper splits in two', () => {
  it('joins the word that closes the sentence', () => {
    // Given whisper's "look up" for `lookup`
    const alt = parseLocalEdit('Rename the variable left to look up.', code);

    // Then the new name is the compound
    expect(alt?.commands[0]).toMatchObject({ type: 'renameSymbol', newName: 'lookup' });
  });

  it('leaves a closing filler word out of the name', () => {
    // Given the speaker adds "everywhere" or "instead"
    // Then the name stays the one word
    expect(parseLocalEdit('Rename left to low everywhere.', code)?.commands[0]).toMatchObject({ newName: 'low' });
    expect(parseLocalEdit('Rename left to low instead', code)?.commands[0]).toMatchObject({ newName: 'low' });
  });
});

describe('names spoken as several words', () => {
  const area = 'class Solution:\n    def maxArea(self, height):\n        max_area = 0\n        for h in height:\n            max_area = max(max_area, h)\n        return max_area\n';

  it('finds the snake_case name', () => {
    // Given the speaker reads `max_area` off the screen as two words
    const alt = parseLocalEdit('Rename the variable max area to area.', area);

    // When it is applied
    const { text } = applyCommands(area, alt!.commands);

    // Then every use is renamed, locally
    expect(text).toContain('area = max(area, h)');
    expect(text).not.toContain('max_area');
  });

  it('declines a phrase that is no name in the buffer', () => {
    // Given words that join to nothing in the buffer
    // Then the planner decides
    expect(parseLocalEdit('Rename the variable top speed to area.', area)).toBeNull();
  });
});
