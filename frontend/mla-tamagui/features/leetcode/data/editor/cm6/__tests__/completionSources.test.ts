import { CompletionContext, type Completion, type CompletionSource } from '@codemirror/autocomplete';
import { cpp } from '@codemirror/lang-cpp';
import { python } from '@codemirror/lang-python';
import { ensureSyntaxTree, syntaxTreeAvailable } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import type { CompletionEntry, Language } from '@lib/code-editor-api';
import { describe, expect, it, vi } from 'vitest';

import { createCompletionSource } from '../completionSources';

type Result = { from: number; options: Completion[] } | null;
const NO_LIMIT = Number.POSITIVE_INFINITY;
const stateOf = (doc: string, language?: Language) => EditorState.create({ doc, extensions: [language === 'cpp' ? cpp() : python()] });
const ask = (source: CompletionSource, state: EditorState, explicit = false) => source(new CompletionContext(state, state.doc.length, explicit)) as Result;

// the parse runs to its end with no clock, so the state is as a machine with all the time in the world leaves it
const run = (source: CompletionSource, doc: string, options: { explicit?: boolean; language?: Language } = {}) => {
  const state = stateOf(doc, options.language);
  ensureSyntaxTree(state, state.doc.length, NO_LIMIT);
  return ask(source, state, options.explicit);
};

// every reading of the clock is a second later than the last, so any parse with a time limit stops after its first step
function withClockPastEveryDeadline<T>(body: () => T): T {
  let now = Date.now();
  const spy = vi.spyOn(Date, 'now').mockImplementation(() => (now += 1_000));
  try {
    return body();
  } finally {
    spy.mockRestore();
  }
}
const labels = (result: { options: Completion[] } | null, prefix = ''): string[] => (result?.options ?? []).filter((option) => option.label.startsWith(prefix)).map((option) => option.label);
const boostOf = (result: { options: Completion[] } | null, label: string): number => result?.options.find((option) => option.label === label)?.boost ?? 0;

const entries: CompletionEntry[] = [{ label: 'twoSum', type: 'function', detail: '(nums, target)' }, { label: 'nums', type: 'variable', detail: 'List[int]' }, { label: 'target', type: 'variable', detail: 'int' }];
const source = createCompletionSource({ language: 'python', entries });
const starter = 'class Solution:\n    def twoSum(self, nums: List[int], target: int) -> List[int]:\n        ';

describe('createCompletionSource', () => {
  it('given a method body and the prefix ret, when the source runs, then return is offered first among the matches', () => {
    const result = run(source, `${starter}ret`);

    expect(labels(result, 'ret')).toEqual(['return']);
    expect(result?.from).toBe(`${starter}`.length);
  });

  it('given the two-sum starter, when nu is typed, then nums is offered once and the entry detail wins', () => {
    const result = run(source, `${starter}nu`);

    expect(labels(result, 'nu')).toEqual(['nums']);
    expect(result?.options.find((option) => option.label === 'nums')).toMatchObject({ detail: 'List[int]', type: 'variable' });
  });

  it('given a prefix inside a string or a comment, when the source runs, then there is nothing to offer', () => {
    expect(run(source, `${starter}x = "ret`)).toBeNull();
    expect(run(source, `${starter}# ret`)).toBeNull();
  });

  it('given a grammar that has not reached the caret when its time is up, when the source runs, then nothing is offered, and once the parse is done the same state gets its list', () => {
    const doc = `${starter}ret`;
    const state = withClockPastEveryDeadline(() => stateOf(doc));
    expect(syntaxTreeAvailable(state)).toBe(false);

    const unparsed = withClockPastEveryDeadline(() => ask(source, state));
    ensureSyntaxTree(state, state.doc.length, NO_LIMIT);
    const parsed = ask(source, state);

    expect(unparsed).toBeNull();
    expect(labels(parsed, 'ret')).toEqual(['return']);
  });

  it('given a state whose own tree stopped short of a string, when the parse is done and the source runs, then nothing is offered inside the string', () => {
    const doc = `${starter}x = "ret`;
    const state = withClockPastEveryDeadline(() => stateOf(doc));
    expect(syntaxTreeAvailable(state)).toBe(false);
    ensureSyntaxTree(state, state.doc.length, NO_LIMIT);

    expect(ask(source, state)).toBeNull();
  });

  it('given self.ca and a buffer with self.cache, when the source runs, then cache is offered and ca is not', () => {
    const result = run(source, `${starter}self.cache = {}\n        self.ca`);

    expect(labels(result)).toEqual(['cache']);
  });

  it('given one character typed, when the source runs implicitly, then null; when explicitly, then options', () => {
    expect(run(source, `${starter}r`)).toBeNull();
    expect(labels(run(source, `${starter}r`, { explicit: true }), 'r')).toContain('return');
  });

  it('given the prefix ra, when the sources run, then range ranks above raise', () => {
    const result = run(source, `${starter}ra`);

    expect(labels(result, 'ra')).toEqual(expect.arrayContaining(['range', 'raise']));
    expect(boostOf(result, 'range')).toBeGreaterThan(boostOf(result, 'raise'));
  });

  it('given the prefix _, then dunders appear; given se, they do not', () => {
    expect(labels(run(source, `${starter}__`), '__').length).toBeGreaterThan(0);
    expect(labels(run(source, `${starter}se`)).filter((label) => label.startsWith('__'))).toEqual([]);
  });

  it('given a builtin that is a snippet in the language package, when the source runs, then only plain names are offered', () => {
    const result = run(source, `${starter}for`);

    expect(result?.options.every((option) => typeof option.apply !== 'function')).toBe(true);
    expect(labels(result, 'for')).toContain('for');
  });

  it('given the signature line deleted and entries given, when nu is typed, then nums is still offered', () => {
    expect(labels(run(source, 'nu'), 'nu')).toEqual(['nums']);
  });

  it('given a name defined in the buffer, when its prefix is typed, then it ranks below the problem entries and above plain names', () => {
    const result = run(source, `${starter}total_count = 1\n        tot`);

    expect(labels(result, 'tot')).toContain('total_count');
    expect(boostOf(result, 'total_count')).toBeGreaterThan(boostOf(result, 'raise'));
  });

  it('given a cpp language, when the source runs, then it offers the language keywords and does not throw', () => {
    const cppSource = createCompletionSource({ language: 'cpp', entries: [] });

    expect(labels(run(cppSource, 'int main() {\n  ret', { language: 'cpp' }), 'ret')).toEqual(['return']);
    expect(run(cppSource, 'int main() {\n  x = "ret', { language: 'cpp' })).toBeNull();
  });
});
