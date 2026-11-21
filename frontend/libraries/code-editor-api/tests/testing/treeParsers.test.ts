import { describe, expect, it } from 'vitest';

import type { SyntaxNode } from '../../src';
import { parseBraced, parseIndented, snapshotOf } from '../../src/testing';

const shape = (node: SyntaxNode): string => (node.children ? `${node.type}[${node.children.map(shape).join(',')}]` : node.type);
const text = (source: string, node: SyntaxNode): string => source.slice(node.from, node.to);

describe('parseIndented', () => {
  const source = 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n';

  it('given a def with a for inside, then bodies nest by indentation, a body starts at its colon and ends after the last line, and the script spans the document', () => {
    const tree = parseIndented(source);

    expect(shape(tree)).toBe('Script[FunctionDefinition[Body[ForStatement[Body[ExpressionStatement]],ReturnStatement]]]');
    expect(tree).toMatchObject({ from: 0, to: source.length });
    const body = tree.children?.[0]?.children?.[0] as SyntaxNode;
    expect(text(source, body)).toBe(':\n    for x in xs:\n        print(x)\n    return 1\n');
    const loop = body.children?.[0] as SyntaxNode;
    expect(text(source, loop)).toBe('for x in xs:\n        print(x)\n');
    expect(text(source, loop.children?.[0] as SyntaxNode)).toBe(':\n        print(x)\n');
  });

  it('given elif and else, then they are further bodies of the same if statement', () => {
    const chain = 'if a:\n    b\nelif c:\n    d\nelse:\n    e\n';
    const tree = parseIndented(chain);

    expect(shape(tree)).toBe('Script[IfStatement[Body[ExpressionStatement],Body[ExpressionStatement],Body[ExpressionStatement]]]');
    expect(text(chain, tree.children?.[0] as SyntaxNode)).toBe('if a:\n    b\nelif c:\n    d\nelse:\n    e\n');
  });

  it('given try, except and finally, then they are one try statement', () => {
    expect(shape(parseIndented('try:\n    a\nexcept E:\n    b\nfinally:\n    c\n'))).toBe('Script[TryStatement[Body[ExpressionStatement],Body[ExpressionStatement],Body[ExpressionStatement]]]');
  });

  it('given a header with no colon, then it is a statement with an error child and the next line is a sibling', () => {
    const broken = 'for x in\nprint(x)\n';
    const tree = parseIndented(broken);

    expect(shape(tree)).toBe('Script[ForStatement[⚠],ExpressionStatement]');
    expect(tree.children?.[0]?.children?.[0]).toMatchObject({ error: true, from: 8, to: 8 });
  });

  it('given a header with a colon and nothing under it, then the body carries an error', () => {
    expect(shape(parseIndented('for x in xs:\n'))).toBe('Script[ForStatement[Body[⚠]]]');
  });

  it('given statements of every simple kind, then each is named the way the grammar names it', () => {
    const kinds = 'x = 1\nx += 2\nprint(x)\nreturn x\npass\nimport os\nx == 3\n';

    expect(shape(parseIndented(kinds))).toBe('Script[AssignStatement,UpdateStatement,ExpressionStatement,ReturnStatement,PassStatement,ImportStatement,ExpressionStatement]');
  });

  it('given a comment-only, a blank-only and an empty file, then the script holds the comment or nothing', () => {
    expect(shape(parseIndented('# hi\n'))).toBe('Script[Comment]');
    expect(parseIndented('\n  \n\n')).toEqual({ type: 'Script', from: 0, to: 5 });
    expect(parseIndented('')).toEqual({ type: 'Script', from: 0, to: 0 });
  });

  it('given blank lines inside a body, then they do not end it', () => {
    expect(shape(parseIndented('def f():\n    a\n\n    b\n'))).toBe('Script[FunctionDefinition[Body[ExpressionStatement,ExpressionStatement]]]');
  });
});

describe('parseBraced', () => {
  it('given a function with a for inside, then blocks follow the braces and the owner is named by its keyword', () => {
    const source = 'function f(xs) {\n  for (const x of xs) {\n    g(x);\n  }\n  return 1;\n}\n';
    const tree = parseBraced(source, 'javascript');

    expect(shape(tree)).toBe('Script[FunctionDeclaration[Block[ForStatement[Block[ExpressionStatement]],ReturnStatement]]]');
    const block = tree.children?.[0]?.children?.[0] as SyntaxNode;
    expect(text(source, block).startsWith('{')).toBe(true);
    expect(text(source, block).endsWith('}')).toBe(true);
  });

  it('given else, catch and finally, then they extend the statement before them', () => {
    expect(shape(parseBraced('if (a) { b(); } else { c(); }', 'javascript'))).toBe('Script[IfStatement[Block[ExpressionStatement],Block[ExpressionStatement]]]');
    expect(shape(parseBraced('try { a(); } catch (e) { b(); } finally { c(); }', 'javascript'))).toBe('Script[TryStatement[Block[ExpressionStatement],CatchClause[Block[ExpressionStatement]],FinallyClause[Block[ExpressionStatement]]]]');
  });

  it('given braces inside strings and comments, then they do not open or close a block', () => {
    expect(shape(parseBraced('f("}"); // {\n/* } */ g(\'{\');', 'javascript'))).toBe('Script[ExpressionStatement,ExpressionStatement]');
  });

  it('given an unclosed brace, then the block runs to the end of the text with an error child there', () => {
    const open = 'function f() {\n  g();\n';
    const tree = parseBraced(open, 'javascript');

    expect(shape(tree)).toBe('Script[FunctionDeclaration[Block[ExpressionStatement,⚠]]]');
    expect(tree.children?.[0]?.children?.[0]).toMatchObject({ to: open.length });
  });

  it('given a braceless while, then the statement after the header is its child and no block is made', () => {
    expect(shape(parseBraced('while (x) y();', 'javascript'))).toBe('Script[WhileStatement[ExpressionStatement]]');
  });

  it('given a c++ function with a declaration, then the compound statement holds it and the return', () => {
    expect(shape(parseBraced('int main() {\n  int x = 0;\n  return x;\n}\n', 'cpp'))).toBe('Program[FunctionDefinition[CompoundStatement[Declaration,ReturnStatement]]]');
  });

  it('given a class and a method in javascript, then the class body holds a method declaration', () => {
    expect(shape(parseBraced('class A {\n  m() {\n    return 1;\n  }\n}\n', 'javascript'))).toBe('Script[ClassDeclaration[ClassBody[MethodDeclaration[Block[ReturnStatement]]]]]');
  });

  it('given a struct in c++, then its body is a field declaration list', () => {
    expect(shape(parseBraced('struct S {\n  int a;\n};', 'cpp'))).toBe('Program[StructSpecifier[FieldDeclarationList[Declaration]],ExpressionStatement]');
  });

  it('given an empty text, then the root is empty', () => {
    expect(parseBraced('', 'cpp')).toEqual({ type: 'Program', from: 0, to: 0 });
  });
});

describe('snapshotOf', () => {
  const source = 'def f():\n    return 1\n';

  it('given a slow parse, when work is called, then the tree covers the text only after the asked number of calls and the reads are clipped until then', () => {
    const snapshot = snapshotOf(source, 'python', undefined, 4);
    const reads: number[] = [];
    let done = false;
    let calls = 0;
    while (!done) {
      done = snapshot.work(source.length, 8);
      calls += 1;
      reads.push(snapshot.parsedTo());
    }

    expect(calls).toBe(4);
    expect(reads.every((reached, i) => i === 0 || reached >= (reads[i - 1] as number))).toBe(true);
    expect(snapshot.read({ from: 0, to: source.length }, 'full')).toMatchObject({ type: 'Script', to: source.length });
  });

  it('given nothing parsed yet, then read is null; given half, then the root ends where the parse stopped', () => {
    const snapshot = snapshotOf(source, 'python', undefined, 2);

    expect(snapshot.read({ from: 0, to: source.length }, 'full')).toBeNull();
    snapshot.work(source.length, 8);
    expect(snapshot.read({ from: 0, to: source.length }, 'full')?.to).toBe(snapshot.parsedTo());
  });

  it('given the outline detail, then expression nodes are gone, and a range keeps only what touches it', () => {
    const braced = 'function f() { g(1); }\nfunction h() { k(2); }\n';
    const snapshot = snapshotOf(braced, 'javascript');
    snapshot.work(braced.length, 8);
    const second = braced.indexOf('function h');
    const inside = braced.indexOf('k(2)') + 1;

    const cut = snapshot.read({ from: inside, to: inside }, 'outline') as SyntaxNode;

    expect(shape(cut)).toBe('Script[FunctionDeclaration[Block[ExpressionStatement]]]');
    expect(cut.children?.[0]?.from).toBe(second);
  });
});
