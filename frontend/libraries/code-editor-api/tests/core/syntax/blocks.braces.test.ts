import { describe, expect, it } from 'vitest';

import { enclosingBlock, statementAt } from '../../../src';
import { at, endOf, error, node, reply, span } from './helpers';

const text = 'function f() {\n  if (x) {\n    g();\n  }\n  h();\n}\n';
const call = span('ExpressionStatement', text, 'g();');
const inner = node('Block', at(text, '{', 1), endOf(text, '}'), [call]);
const branch = node('IfStatement', at(text, 'if'), inner.to, [inner]);
const outer = node('Block', at(text, '{'), endOf(text, '}', 1), [branch, span('ExpressionStatement', text, 'h();')]);
const fn = node('FunctionDeclaration', 0, outer.to, [outer]);
const value = reply('javascript', node('Script', 0, text.length, [fn]), text.length);

describe('enclosingBlock on a braced language', () => {
  it('given a caret before the inner brace, right after it, and before its closing brace, then the block is outer, inner, inner', () => {
    expect(enclosingBlock(value, text, inner.from)?.block).toBe(outer);
    expect(enclosingBlock(value, text, inner.from + 1)?.block).toBe(inner);
    expect(enclosingBlock(value, text, inner.to - 1)?.block).toBe(inner);
    expect(enclosingBlock(value, text, inner.to)?.block).toBe(outer);
  });

  it('given the caret in the inner block, then the owner is the if, the content is between the braces and the block is closed', () => {
    expect(enclosingBlock(value, text, at(text, 'g()') + 1)).toMatchObject({ owner: branch, block: inner, content: { from: inner.from + 1, to: inner.to - 1 }, closed: true, damaged: false, family: 'braces' });
  });

  it('given a caret outside every block, then the block is the document and nothing owns it', () => {
    expect(enclosingBlock(value, text, 3)).toMatchObject({ owner: null, block: { type: 'Script' }, closed: true });
  });

  it('given an unclosed block, then the caret at the end of the text is inside it, it is not closed, and it is damaged', () => {
    const open = 'function f() {\n  g();\n';
    const block = node('Block', at(open, '{'), open.length, [span('ExpressionStatement', open, 'g();'), error(open.length)]);
    const tree = reply('javascript', node('Script', 0, open.length, [node('FunctionDeclaration', 0, open.length, [block])]), open.length);

    expect(enclosingBlock(tree, open, open.length)).toMatchObject({ block, closed: false, damaged: true, content: { from: block.from + 1, to: block.to } });
  });

  it('given a braceless while body, then the enclosing block is the outer one', () => {
    const source = 'function f() {\n  while (x) y();\n}\n';
    const body = node('Block', at(source, '{'), endOf(source, '}'), [node('WhileStatement', at(source, 'while'), endOf(source, 'y();'), [span('ExpressionStatement', source, 'y();')])]);
    const tree = reply('javascript', node('Script', 0, source.length, [node('FunctionDeclaration', 0, body.to, [body])]), source.length);

    expect(enclosingBlock(tree, source, at(source, 'y()') + 1)?.block).toBe(body);
  });

  it('given a class body and a method, then the method block is owned by the method', () => {
    const source = 'class A {\n  m() {\n    return 1;\n  }\n}\n';
    const method = node('Block', at(source, '{', 1), endOf(source, '}'), [span('ReturnStatement', source, 'return 1;')]);
    const member = node('MethodDeclaration', at(source, 'm()'), method.to, [method]);
    const body = node('ClassBody', at(source, '{'), endOf(source, '}', 1), [member]);
    const tree = reply('javascript', node('Script', 0, source.length, [node('ClassDeclaration', 0, body.to, [body])]), source.length);

    expect(enclosingBlock(tree, source, at(source, 'return') + 2)).toMatchObject({ block: method, owner: member });
    expect(enclosingBlock(tree, source, at(source, 'm()'))?.block).toBe(body);
  });

  it('given a c++ function, then its compound statement is the block and the function the owner', () => {
    const source = 'int main() {\n  return 0;\n}\n';
    const block = node('CompoundStatement', at(source, '{'), endOf(source, '}'), [span('ReturnStatement', source, 'return 0;')]);
    const definition = node('FunctionDefinition', 0, block.to, [block]);
    const tree = reply('cpp', node('Program', 0, source.length, [definition]), source.length);

    expect(enclosingBlock(tree, source, at(source, 'return') + 1)).toMatchObject({ block, owner: definition });
    expect(statementAt(tree, at(source, 'return') + 1)?.type).toBe('ReturnStatement');
  });

  it('given the caret between two statements on one line, then the statement that starts at the caret wins', () => {
    const source = 'a();b();';
    const first = span('ExpressionStatement', source, 'a();');
    const second = span('ExpressionStatement', source, 'b();');
    const tree = reply('javascript', node('Script', 0, source.length, [first, second]), source.length);

    expect(statementAt(tree, 4)).toBe(second);
    expect(statementAt(tree, 3)).toBe(first);
  });
});
