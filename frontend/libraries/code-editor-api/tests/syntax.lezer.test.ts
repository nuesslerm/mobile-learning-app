import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { describe, expect, it } from 'vitest';

import { LANGUAGE_NAMES, MULTILINE_LITERALS, SYNTAX_TABLES, classify, enclosingBlock, outline, statementAt, type Language, type SyntaxNode, type TreeValue } from '../src';
import { parseFor } from '../src/testing';
import { GRAMMARS, realTree, stateOf } from './support/lezerPlain';

const asValue = (language: Language, doc: string, root: SyntaxNode): TreeValue => ({ instance: 'lezer', revision: 0, language, length: doc.length, parsedTo: doc.length, complete: true, detail: 'outline', root: outline(language, root) });

// [class, from, to] of every outline node in document order; comments are left out because the fake's brace parser skips them
function tuples(language: Language, node: SyntaxNode): string[] {
  const own = classify(language, node) === 'comment' ? [] : [`${classify(language, node)} ${node.from}-${node.to}`];
  return [...own, ...(node.children ?? []).flatMap((child) => tuples(language, child))];
}

describe('the classification tables against the installed grammars', () => {
  it.each(LANGUAGE_NAMES)('given the %s table, when checked against the grammar, then every name is a real node type', (language) => {
    const real = new Set(GRAMMARS[language].parser.nodeSet.types.map((type) => type.name));

    for (const [kind, names] of Object.entries(SYNTAX_TABLES[language])) {
      for (const name of names) expect(real.has(name), `${language} ${kind}: ${name}`).toBe(true);
    }
  });
});

describe('the multi-line literal table against the installed grammars', () => {
  it.each(LANGUAGE_NAMES)('given the %s literal names, when checked against the grammar, then every one is a real node type that the outline leaves out', (language) => {
    const real = new Set(GRAMMARS[language].parser.nodeSet.types.map((type) => type.name));

    for (const name of MULTILINE_LITERALS[language]) {
      expect(real.has(name), `${language}: ${name}`).toBe(true);
      expect(classify(language, { type: name })).toBe('other');
    }
  });
});

// What the fake's parsers promise, proved against the real grammar. Constructs outside the fake's subset are left out on purpose:
// decorators, match, lambdas, multi-line headers, one-line `if x: y`, do-while, braceless else, arrow functions in expressions, switch, class access specifiers.
const CORPUS: Record<Language, string[]> = {
  python: [
    'x = 1\n',
    'x = 1\ny = 2\nz = x + y\n',
    'def f(xs):\n    return 1\n',
    'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n',
    'def f():\n    return 1',
    'if a:\n    b\nelif c:\n    d\nelse:\n    e\n',
    'while x:\n    x -= 1\n',
    'class A:\n    def m(self):\n        pass\n',
    'try:\n    a\nexcept E:\n    b\nfinally:\n    c\n',
    'with open(p) as f:\n    data = f.read()\n',
    'for i in range(3):\n    for j in range(3):\n        print(i, j)\n',
    'def f():\n    if x:\n        return 1\n    return 2\n\nprint(f())\n',
    'def f():\n    a\n\n    b\n',
    'import os\nfrom a import b\n',
    'def f(nums, target):\n    seen = {}\n    for i, n in enumerate(nums):\n        if target - n in seen:\n            return [seen[target - n], i]\n        seen[n] = i\n',
  ],
  javascript: [
    'x = 1;\ny = 2;\n',
    'function f(xs) {\n  for (const x of xs) {\n    g(x);\n  }\n  return 1;\n}\n',
    'if (a) { b(); } else { c(); }',
    'try { a(); } catch (e) { b(); } finally { c(); }',
    'while (x) { y(); }',
    'const x = 1;\nlet y = 2;\n',
    'class A {\n  m() {\n    return 1;\n  }\n}\n',
    'for (let i = 0; i < 3; i++) {\n  a(i);\n}\n',
    'function f() {\n  if (x) {\n    return 1;\n  }\n  return 2;\n}\n',
    'function twoSum(nums, target) {\n  const seen = new Map();\n  for (let i = 0; i < nums.length; i++) {\n    const need = target - nums[i];\n    if (seen.has(need)) {\n      return [seen.get(need), i];\n    }\n    seen.set(nums[i], i);\n  }\n}\n',
  ],
  cpp: [
    'int main() {\n  int x = 0;\n  return x;\n}\n',
    'void f() {\n  for (int i = 0; i < 3; i++) {\n    g(i);\n  }\n}\n',
    'int add(int a, int b) {\n  return a + b;\n}\n',
    'void f() {\n  if (a) { b(); } else { c(); }\n}\n',
    'void f() {\n  while (x) {\n    y();\n  }\n}\n',
    'void f() {\n  try { a(); } catch (const E& e) { b(); }\n}\n',
  ],
};

describe('the fake editor parsers against the real grammars', () => {
  for (const language of LANGUAGE_NAMES) {
    it.each(CORPUS[language].map((snippet) => [JSON.stringify(snippet).slice(0, 70), snippet] as const))(
      `given ${language} %s, when the fake parser and Lezer both parse it, then their structural nodes have the same class and range`,
      (_name, snippet) => {
        const fake = outline(language, parseFor(snippet, language));
        const real = outline(language, realTree(language, snippet));

        expect(tuples(language, fake)).toEqual(tuples(language, real));
      },
    );
  }
});

describe('the enclosing block on real trees', () => {
  const python = 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n';

  it('given a for inside a def, when the caret is in the for body, then the block is the for body owned by the for, and on the for line the def body', () => {
    const value = asValue('python', python, realTree('python', python));

    const inner = enclosingBlock(value, python, python.indexOf('print') + 2);
    const outer = enclosingBlock(value, python, python.indexOf('for x') + 3);

    expect(python.startsWith('for', inner?.owner?.from)).toBe(true);
    expect(inner).toMatchObject({ indent: 8, damaged: false });
    expect(python.startsWith('def', outer?.owner?.from)).toBe(true);
  });

  it('given the half-typed for line, then the tree has an error there and the line after is still in the def body', () => {
    const broken = 'def f(xs):\n    for x in\n    return 1\n';
    const value = asValue('python', broken, realTree('python', broken));

    expect(JSON.stringify(value.root)).toContain('"error":true');
    expect(broken.startsWith('def', enclosingBlock(value, broken, broken.indexOf('return') + 3)?.owner?.from)).toBe(true);
  });

  it('given a whitespace-only indented line after the last statement, when the column is 4 then the def body, when 8 then the for body', () => {
    const base = 'def f(xs):\n    for x in xs:\n        print(x)\n';
    const at = (pad: string) => {
      const doc = base + pad;
      return enclosingBlock(asValue('python', doc, realTree('python', doc)), doc, doc.length);
    };

    expect(base.startsWith('for', at('        ')?.owner?.from)).toBe(true);
    expect(at('    ')?.owner?.from).toBe(0);
    expect(at('')?.block.type).toBe('Script');
  });

  it('given a decorated async def, when the caret is in its body, then the owner is the function definition', () => {
    const doc = '@cache\nasync def f():\n    return 1\n';
    const value = asValue('python', doc, realTree('python', doc));

    expect(statementAt(value, 2)?.type).toBe('DecoratedStatement');
    expect(enclosingBlock(value, doc, doc.indexOf('return') + 2)?.owner?.type).toBe('FunctionDefinition');
  });

  it('given javascript with an unclosed block, then the block is not closed and damaged, and a braceless while is not a block', () => {
    const open = 'function f() {\n  g();\n';
    const value = asValue('javascript', open, realTree('javascript', open));

    expect(enclosingBlock(value, open, open.length)).toMatchObject({ closed: false, damaged: true });
    const braceless = 'function f() {\n  while (x) y();\n}\n';
    expect(enclosingBlock(asValue('javascript', braceless, realTree('javascript', braceless)), braceless, braceless.indexOf('y()') + 1)?.owner?.type).toBe('FunctionDeclaration');
  });

  it('given a half-typed javascript for header, then the tree has an error and the function around it is still found', () => {
    const doc = 'function f(xs) {\n  for (const x of\n  return 1;\n}\n';
    const value = asValue('javascript', doc, realTree('javascript', doc));

    expect(JSON.stringify(value.root)).toContain('"error":true');
    expect(enclosingBlock(value, doc, doc.indexOf('return') + 3)?.block.type).toBeDefined();
  });
});

describe('the parse budget', () => {
  it('given a 6000-character document, when only its initial tree exists, then it covers a prefix and ensureSyntaxTree completes it', () => {
    const doc = Array.from({ length: 300 }, (_, index) => `x${index} = ${index}`).join('\n');
    const state = stateOf('python', doc);

    const initial = syntaxTree(state).length;
    const done = ensureSyntaxTree(state, doc.length, Number.POSITIVE_INFINITY);

    expect(doc.length).toBeGreaterThan(3_001);
    expect(initial).toBeLessThan(doc.length);
    expect(done?.length).toBe(doc.length);
  });
});
