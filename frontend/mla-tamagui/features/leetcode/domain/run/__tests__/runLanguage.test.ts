import { describe, expect, it } from 'vitest';

import { deviceLanguageOf, isJavaScriptLanguage, isPythonLanguage } from '../runLanguage';

describe('deviceLanguageOf', () => {
  it.each([
    ['python3', 'python3'],
    ['python', 'python3'],
    ['javascript', 'javascript'],
    ['typescript', null],
    ['cpp', null],
    ['', null],
  ])('Given the editor language %j, When deviceLanguageOf asks, Then it is %j', (lang, expected) => {
    expect(deviceLanguageOf(lang)).toBe(expected);
  });
});

describe('the language predicates', () => {
  it('Given each language, When asked, Then only python3 and python are Python and only javascript is JavaScript', () => {
    const languages = ['python3', 'python', 'javascript', 'typescript', 'cpp'];
    expect(languages.filter(isPythonLanguage)).toEqual(['python3', 'python']);
    expect(languages.filter(isJavaScriptLanguage)).toEqual(['javascript']);
  });
});
