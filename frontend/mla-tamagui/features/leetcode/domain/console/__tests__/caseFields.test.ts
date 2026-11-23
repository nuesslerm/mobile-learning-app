import { describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../../problemCatalog';
import { parameterNames } from '../../run/customCase';
import { captionsOf, fieldTextsOf, jsonText, parseCaseFields } from '../caseFields';

const problem = (id: string) => {
  const found = ProblemCatalog.getById(id);
  if (!found) throw new Error(`no problem ${id}`);
  return found;
};

describe('captionsOf', () => {
  it('given merge-two-sorted-lists, when captions are read, then both lists carry ListNode', () => {
    expect(captionsOf(problem('merge-two-sorted-lists')).map((c) => c.caption)).toEqual(['list1 . ListNode', 'list2 . ListNode']);
  });

  it('given valid-parentheses, when captions are read, then there is just s', () => {
    expect(captionsOf(problem('valid-parentheses'))).toEqual([{ name: 's', caption: 's' }]);
  });
});

describe('jsonText', () => {
  it('given a string, false and null, when shown, then strings keep their quotes', () => {
    expect([jsonText('([)]'), jsonText(false), jsonText(null), jsonText([1, 'a'])]).toEqual(['"([)]"', 'false', 'null', '[1,"a"]']);
  });
});

describe('parseCaseFields', () => {
  it('given a single-quoted field, when parsed, then the problem names s and the message says to use double quotes', () => {
    const parsed = parseCaseFields({ s: "'('" }, ['s']);
    expect(parsed).toMatchObject({ ok: false, name: 's' });
    expect(!parsed.ok && parsed.message).toContain('double quotes');
  });

  it('given valid text for every parameter, when parsed, then expected is null and inputs follow parameter order', () => {
    const parsed = parseCaseFields({ target: '9', nums: '[2,7]' }, ['nums', 'target']);
    expect(parsed).toEqual({ ok: true, testCase: { inputs: { nums: [2, 7], target: 9 }, expected: null } });
    expect(parsed.ok && Object.keys(parsed.testCase.inputs)).toEqual(['nums', 'target']);
  });

  it('given an empty field, when parsed, then it is refused naming the parameter', () => {
    expect(parseCaseFields({ s: ' ' }, ['s'])).toEqual({ ok: false, name: 's', message: 's is empty' });
  });

  it('given every bundled case, when its fields are written and parsed, then the same arguments come back in the same order', () => {
    const mismatches: string[] = [];
    for (const p of ProblemCatalog.getAll()) {
      const names = parameterNames(p);
      for (const [i, testCase] of (p.testCases ?? []).entries()) {
        const a = parseCaseFields(fieldTextsOf(testCase, names), names);
        if (!(a.ok && JSON.stringify(Object.entries(a.testCase.inputs)) === JSON.stringify(Object.entries(testCase.inputs)))) mismatches.push(`${p.id}#${i}`);
      }
    }
    expect(mismatches).toEqual([]);
  });
});
