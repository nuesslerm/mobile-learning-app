import { CompletionContext, type Completion, type CompletionSource } from '@codemirror/autocomplete';
import { cpp } from '@codemirror/lang-cpp';
import { python } from '@codemirror/lang-python';
import { ensureSyntaxTree } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import type { CompletionEntry, Language } from '@lib/code-editor-api';
import { describe, expect, it } from 'vitest';

import { createCompletionSource } from '../completionSources';

const run = (source: CompletionSource, doc: string, options: { explicit?: boolean; language?: Language } = {}) => {
  const state = EditorState.create({ doc, extensions: [options.language === 'cpp' ? cpp() : python()] });
  ensureSyntaxTree(state, state.doc.length, 5_000);
  return source(new CompletionContext(state, doc.length, options.explicit ?? false)) as { from: number; options: Completion[] } | null;
};
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
