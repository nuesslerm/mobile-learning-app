import { describe, expect, it } from 'vitest';

import { classify, outline, type Language, type SyntaxNode } from '../../../src';
import { depthMap, frozenLines, linesOf, shiftPrefix } from '../../../src/core/semantic/indent';
import { locateRun } from '../../../src/core/semantic/model';
import { realTree } from '../../support/lezerPlain';
import { at, endOf } from '../syntax/helpers';

const blocksIn = (language: Language, nodes: readonly SyntaxNode[]): SyntaxNode[] => {
  const found: SyntaxNode[] = [];
  const visit = (one: SyntaxNode) => {
    if (classify(language, one) === 'block') found.push(one);
    one.children?.forEach(visit);
  };
  for (const one of nodes) visit(one);
  return found;
};

/** The depth of every line of the statement `from` through `to`, in order, as the planner will ask for it. */
function depthsOf(language: Language, text: string, from: string, to = from): number[] {
  const located = locateRun({ text, root: realTree(language, text), language }, { from: at(text, from), to: endOf(text, to) });
  if (!located.ok) throw new Error(located.error.message);
  const { run, extent } = located.value;
  const depths = depthMap(language, text, blocksIn(language, run), extent);
  return linesOf(text, extent).flatMap((line) => (depths.has(line.from) ? [depths.get(line.from) as number] : []));
}

describe('the lines of a range', () => {
  it('given whole lines that end in a newline, when split, then each line is its text and where it starts', () => {
    const text = 'a\n  b\n\nc\n';

    expect(linesOf(text, { from: 2, to: 9 })).toEqual([
      { from: 2, text: '  b' },
      { from: 6, text: '' },
      { from: 7, text: 'c' },
    ]);
  });

  it('given a range that ends the text with no newline, when split, then the last line is complete', () => {
    expect(linesOf('a\nb', { from: 2, to: 3 })).toEqual([{ from: 2, text: 'b' }]);
  });

  it('given an empty range, when split, then there are no lines', () => {
    expect(linesOf('a\n', { from: 2, to: 2 })).toEqual([]);
  });
});

describe('the depth of the lines of a run', () => {
  it('given a javascript if with an else, when its depth map is computed, then the lines inside a block are one deep and the lines that open or close one have their owner depth', () => {
    const text = 'function f() {\n  if (a) {\n    b();\n  } else {\n    c();\n  }\n  d();\n}\n';

    expect(depthsOf('javascript', text, 'if (a) {', '  }\n')).toEqual([0, 1, 0, 1, 0]);
  });

  it('given a javascript block with a nested block, when its depth map is computed, then each line has its nesting inside the run', () => {
    const text = 'function f() {\n  while (a) {\n    for (const x of xs) {\n      g(x);\n    }\n  }\n}\n';

    expect(depthsOf('javascript', text, 'while (a) {', '    }\n  }')).toEqual([0, 1, 2, 1, 0]);
  });

  it('given a brace alone on its line, when its depth map is computed, then it has its owner depth', () => {
    const text = 'int f() {\n  if (a)\n  {\n    b();\n  }\n}\n';

    expect(depthsOf('cpp', text, 'if (a)', '  }')).toEqual([0, 0, 1, 0]);
  });

  it('given a python if with an elif and an else, when its depth map is computed, then the clause headers have their owner depth', () => {
    const text = 'def f(x):\n    if x:\n        a = 1\n    elif y:\n        b = 2\n    else:\n        c = 3\n    return 0\n';

    expect(depthsOf('python', text, 'if x:', 'c = 3')).toEqual([0, 1, 0, 1, 0, 1]);
  });

  it('given a python block with a nested block and a blank line, when its depth map is computed, then the blank line has no depth', () => {
    const text = 'while a:\n    for x in xs:\n        g(x)\n\n    h()\n';

    expect(depthsOf('python', text, 'while a:', 'h()')).toEqual([0, 1, 2, 1]);
  });
});

describe('swapping an indentation prefix', () => {
  const lines = (...texts: string[]) => texts.map((text, i) => ({ from: i * 100, text }));
  const none: ReadonlySet<number> = new Set();

  it('given a four-space prefix swapped for an eight-space one, when applied, then every line keeps its indent relative to the prefix', () => {
    const shifted = shiftPrefix(lines('    if x:', '        y()', '    z()'), '    ', '        ', none);

    expect(shifted).toEqual(['        if x:', '            y()', '        z()']);
  });

  it('given a prefix swapped for a shorter one, when applied, then the lines move back', () => {
    expect(shiftPrefix(lines('        a', '            b'), '        ', '    ', none)).toEqual(['    a', '        b']);
  });

  it('given tab-indented lines, when the prefix is swapped, then the prefix string is kept as it is and nothing is converted', () => {
    expect(shiftPrefix(lines('\ta', '\t\tb'), '\t', '\t\t', none)).toEqual(['\t\ta', '\t\t\tb']);
    expect(shiftPrefix(lines('\ta', '\t\tb'), '\t', '    ', none)).toEqual(['    a', '    \tb']);
  });

  it('given a blank line and a continuation line with less indent than the prefix, when applied, then both are left as they are', () => {
    const shifted = shiftPrefix(lines('    total = (a +', '  b)', '', '   ', '    return total'), '    ', '        ', none);

    expect(shifted).toEqual(['        total = (a +', '  b)', '', '   ', '        return total']);
  });

  it('given a frozen line, when applied, then it is left as it is', () => {
    expect(shiftPrefix(lines('    a = """x', '    y', '    z"""'), '    ', '', new Set([100]))).toEqual(['a = """x', '    y', 'z"""']);
  });

  it('given an empty prefix at the top level, when swapped for four spaces, then every code line gets it', () => {
    expect(shiftPrefix(lines('a', '', 'b'), '', '    ', none)).toEqual(['    a', '', '    b']);
  });
});

describe('the lines frozen by a multi-line literal on real Lezer trees', () => {
  const CASES: [Language, string][] = [
    ['python', 'x = """a\nb\n  c"""\ny = 1\n'],
    ['javascript', 'x = `a\nb\n  c`;\ny = 1;\n'],
    ['cpp', 'auto x = R"(a\nb\n  c)";\nint y = 1;\n'],
  ];

  it.each(CASES)('given a %s literal over three lines, when the frozen lines are computed, then the lines that begin inside it are frozen and no other', (language, text) => {
    const frozen = frozenLines(text, realTree(language, text), language);

    expect([...frozen].sort((a, b) => a - b)).toEqual([at(text, 'b\n'), at(text, '  c')]);
  });

  it('given a triple-quoted string in a block, when the block is shifted a level deeper, then the string lines keep their text', () => {
    const text = 'def f():\n    s = """one\n  two\n   three"""\n    return s\n';
    const root = realTree('python', text);
    const extent = { from: at(text, '    s = '), to: text.length };

    const shifted = shiftPrefix(linesOf(text, extent), '    ', '        ', frozenLines(text, root, 'python'));

    expect(shifted).toEqual(['        s = """one', '  two', '   three"""', '        return s']);
  });

  it('given a document with no literal, when the frozen lines are computed, then there are none, and given no tree, none either', () => {
    const text = 'a = 1\nb = 2\n';

    expect(frozenLines(text, realTree('python', text), 'python').size).toBe(0);
    expect(frozenLines(text, null, 'python').size).toBe(0);
  });

  it('given the outline of a tree with a literal, when read, then the literal is not in it', () => {
    const text = 'x = """a\nb"""\n';

    expect(JSON.stringify(outline('python', realTree('python', text)))).not.toContain('String');
  });
});
