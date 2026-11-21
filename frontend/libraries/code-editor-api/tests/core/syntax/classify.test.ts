import { describe, expect, it } from 'vitest';

import { LANGUAGE_NAMES, SYNTAX_TABLES, classify, familyOf, isOutline, outline } from '../../../src';
import { error, node } from './helpers';

describe('the class tables', () => {
  it.each(LANGUAGE_NAMES)('given the %s table, when its names are read, then each starts with a capital and none is in two classes', (language) => {
    const names = Object.values(SYNTAX_TABLES[language]).flat();

    expect(names.every((name) => /^[A-Z]/.test(name))).toBe(true);
    expect(new Set(names).size).toBe(names.length);
  });

  it('given a name that is a statement in one language and an identifier in another, then each language answers for itself', () => {
    expect(classify('python', { type: 'FunctionDefinition' })).toBe('compound');
    expect(classify('javascript', { type: 'FunctionDefinition' })).toBe('other');
    expect(classify('javascript', { type: 'VariableDefinition' })).toBe('other');
    expect(classify('cpp', { type: 'CompoundStatement' })).toBe('block');
  });

  it('given an error node, by flag or by its Lezer name, then it is an error in every language', () => {
    expect(classify('python', { type: 'Whatever', error: true })).toBe('error');
    expect(classify('cpp', { type: '⚠' })).toBe('error');
  });

  it('given the languages, then python is indentation and the others are braces', () => {
    expect(LANGUAGE_NAMES.map(familyOf)).toEqual(['indent', 'braces', 'braces']);
  });
});

describe('outline', () => {
  it('given statements with expressions inside, then the expressions are gone and the statements keep their order', () => {
    const tree = node('Script', 0, 40, [
      node('AssignStatement', 0, 5, [node('VariableName', 0, 1), node('Number', 4, 5)]),
      node('FunctionDefinition', 6, 40, [node('ParamList', 12, 15, [node('VariableName', 13, 14)]), node('Body', 15, 40, [node('ReturnStatement', 20, 28, [node('Number', 27, 28)]), error(40)])]),
    ]);

    expect(outline('python', tree)).toEqual(
      node('Script', 0, 40, [node('AssignStatement', 0, 5), node('FunctionDefinition', 6, 40, [node('Body', 15, 40, [node('ReturnStatement', 20, 28), error(40)])])]),
    );
  });

  it('given a function nested in an expression, then it attaches to the nearest statement that is kept', () => {
    const tree = node('Script', 0, 30, [node('ExpressionStatement', 0, 30, [node('CallExpression', 0, 29, [node('ArrowFunction', 8, 28, [node('Block', 14, 28)])])])]);

    expect(outline('javascript', tree).children?.[0]?.children?.map((n) => n.type)).toEqual(['ArrowFunction']);
  });

  it('given a type, then isOutline says whether the outline keeps it', () => {
    expect(isOutline('python', { type: 'Body' })).toBe(true);
    expect(isOutline('python', { type: 'ParamList' })).toBe(false);
  });
});
