import { describe, expect, it } from 'vitest';

import { enclosingBlock, statementAt } from '../../../src';
import { at, endOf, error, node, reply, span } from './helpers';

// def f(xs):
//     for x in xs:
//         if x > 1:
//             print(x)
//     return 1
const text = 'def f(xs):\n    for x in xs:\n        if x > 1:\n            print(x)\n    return 1\n';
const print = span('ExpressionStatement', text, 'print(x)');
const afterPrint = endOf(text, 'print(x)') + 1;
const ifBody = node('Body', at(text, ':', 2), afterPrint, [print]);
const ifStatement = node('IfStatement', at(text, 'if x'), endOf(text, 'print(x)'), [ifBody]);
const forBody = node('Body', at(text, ':', 1), afterPrint, [ifStatement]);
const forStatement = node('ForStatement', at(text, 'for x'), endOf(text, 'print(x)'), [forBody]);
const returnStatement = span('ReturnStatement', text, 'return 1');
const defBody = node('Body', at(text, ':'), text.length, [forStatement, returnStatement]);
const definition = node('FunctionDefinition', 0, endOf(text, 'return 1'), [defBody]);
const value = reply('python', node('Script', 0, text.length, [definition]), text.length);

describe('enclosingBlock on python', () => {
  it('given a for inside a def, when the caret is in the for body, then the block is the for body owned by the ForStatement', () => {
    const found = enclosingBlock(value, text, at(text, 'if x'));

    expect(found).toMatchObject({ block: forBody, owner: forStatement, indent: 8, closed: true, damaged: false, family: 'indent' });
  });

  it('given the caret on the for line, then the block is the def body, and right after the colon it is the for body', () => {
    expect(enclosingBlock(value, text, at(text, 'for x') + 3)?.block).toBe(defBody);
    expect(enclosingBlock(value, text, at(text, ':', 1))?.block).toBe(defBody);
    expect(enclosingBlock(value, text, at(text, ':', 1) + 1)?.block).toBe(forBody);
  });

  it('given the caret in the innermost statement, then the block is the if body and its content starts after the colon', () => {
    const found = enclosingBlock(value, text, at(text, 'print') + 2);

    expect(found?.block).toBe(ifBody);
    expect(found?.content).toEqual({ from: ifBody.from + 1, to: ifBody.to });
  });

  it('given the caret at the end of the last statement and no final newline, then it is still inside the block', () => {
    const bare = 'def f():\n    return 1';
    const body = node('Body', at(bare, ':'), bare.length, [span('ReturnStatement', bare, 'return 1')]);
    const tree = reply('python', node('Script', 0, bare.length, [node('FunctionDefinition', 0, bare.length, [body])]), bare.length);

    expect(enclosingBlock(tree, bare, bare.length)?.block).toBe(body);
  });

  it('given the caret on a line with a dedented statement after the body, then the block is the outer one', () => {
    const flat = 'if x:\n    a\nb\n';
    const body = node('Body', at(flat, ':'), at(flat, 'b'), [span('ExpressionStatement', flat, 'a')]);
    const tree = reply('python', node('Script', 0, flat.length, [node('IfStatement', 0, endOf(flat, 'a'), [body]), span('ExpressionStatement', flat, 'b')]), flat.length);

    expect(enclosingBlock(tree, flat, at(flat, 'b'))?.block.type).toBe('Script');
  });

  it('given a whitespace-only indented line after the last statement, when the column is 4 then it is the def body, when 8 then the if body, when 0 then the document', () => {
    const base = 'def f(xs):\n    if xs:\n        print(1)\n';
    const inner = span('ExpressionStatement', base, 'print(1)');
    const innerBody = node('Body', at(base, ':', 1), base.length, [inner]);
    const outerBody = node('Body', at(base, ':'), base.length, [node('IfStatement', at(base, 'if'), endOf(base, 'print(1)'), [innerBody])]);
    const make = (pad: string) => {
      const source = base + pad;
      return { source, tree: reply('python', node('Script', 0, source.length, [node('FunctionDefinition', 0, endOf(base, 'print(1)'), [outerBody])]), source.length, source.length) };
    };

    expect(enclosingBlock(make('        ').tree, make('        ').source, base.length + 8)?.block).toBe(innerBody);
    expect(enclosingBlock(make('    ').tree, make('    ').source, base.length + 4)?.block).toBe(outerBody);
    expect(enclosingBlock(make('').tree, make('').source, base.length)?.block.type).toBe('Script');
  });

  it('given decorators and async, then a decorator line is the DecoratedStatement, the def line the FunctionDefinition, and the owner of its body the FunctionDefinition', () => {
    const source = '@cache\nasync def f():\n    return 1\n';
    const body = node('Body', at(source, ':'), source.length, [span('ReturnStatement', source, 'return 1')]);
    const fn = node('FunctionDefinition', at(source, 'async'), endOf(source, 'return 1'), [body]);
    const decorated = node('DecoratedStatement', 0, fn.to, [span('Decorator', source, '@cache'), fn]);
    const tree = reply('python', node('Script', 0, source.length, [decorated]), source.length);

    expect(statementAt(tree, 2)).toBe(decorated);
    expect(statementAt(tree, at(source, 'async') + 2)).toBe(fn);
    expect(enclosingBlock(tree, source, at(source, 'return') + 1)?.owner).toBe(fn);
  });

  it('given a tree read only to part of the document, when the caret is past it, then there is no answer', () => {
    const partial = reply('python', node('Script', 0, 10), text.length, 10);

    expect(enclosingBlock(partial, text, 11)).toBeNull();
    expect(statementAt(partial, 11)).toBeNull();
  });
});

describe('the half-typed line', () => {
  const source = 'def f(xs):\n    for x in\n    return 1\n';
  const header = node('ForStatement', at(source, 'for'), endOf(source, 'for x in'), [error(endOf(source, 'for x in'))]);
  const ret = span('ReturnStatement', source, 'return 1');
  const body = node('Body', at(source, ':'), source.length, [header, ret]);
  const tree = reply('python', node('Script', 0, source.length, [node('FunctionDefinition', 0, endOf(source, 'return 1'), [body])]), source.length);

  it('given the half-typed `for x in`, then the ForStatement is the statement there and the def body around it is undamaged', () => {
    expect(statementAt(tree, endOf(source, 'for x in'))).toBe(header);
    expect(enclosingBlock(tree, source, endOf(source, 'for x in'))).toMatchObject({ block: body, damaged: false });
  });

  it('given the caret in `return 1` after the broken line, then the def body is still found', () => {
    expect(enclosingBlock(tree, source, at(source, 'return') + 3)?.block).toBe(body);
  });

  it('given a block with an error of its own, then it is damaged', () => {
    const marked = node('Body', body.from, body.to, [ret, error(body.to)]);
    const damaged = reply('python', node('Script', 0, source.length, [node('FunctionDefinition', 0, ret.to, [marked])]), source.length);

    expect(enclosingBlock(damaged, source, at(source, 'return') + 3)?.damaged).toBe(true);
  });
});
