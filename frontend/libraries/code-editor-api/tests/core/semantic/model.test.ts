import { describe, expect, it } from 'vitest';

import { classify, outline, type Language, type SyntaxNode } from '../../../src';
import { locateRun, type Located, type Run } from '../../../src/core/semantic/model';
import { parseFor } from '../../../src/testing';
import { realTree } from '../../support/lezerPlain';
import { at, endOf, error, node, span } from '../syntax/helpers';

const PARSERS = [
  ['the fake parser', parseFor],
  ['Lezer', (text: string, language: Language) => realTree(language, text)],
] as const;

const ok = (located: Located): Run => {
  if (!located.ok) throw new Error(`expected a run, got ${located.error.reason}: ${located.error.message}`);
  return located.value;
};
const reason = (located: Located) => (located.ok ? 'ok' : located.error.reason);
const typesOf = (run: Run) => run.run.map((one) => one.type);
const lineOf = (text: string, from: string) => text.lastIndexOf('\n', at(text, from) - 1) + 1;

const IF_BODY = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n';

describe.each(PARSERS)('locating a run of python statements on %s', (_name, parse) => {
  const locate = (text: string, from: string, to = from, options?: { caret?: boolean }) =>
    locateRun({ text, root: parse(text, 'python'), language: 'python' }, { from: at(text, from), to: endOf(text, to) }, options);

  it('given a def with an if holding two statements, when the second statement is located, then the run is that statement in the if body with its whole line as the extent', () => {
    const located = ok(locate(IF_BODY, 'b = 2'));

    expect(typesOf(located)).toEqual(['AssignStatement']);
    expect(located.parent.type).toBe('Body');
    expect(located.owner?.type).toBe('IfStatement');
    expect(located.extent).toEqual({ from: lineOf(IF_BODY, 'b = 2'), to: endOf(IF_BODY, 'b = 2\n') });
    expect(located).toMatchObject({ indentPrefix: '        ', unterminated: false });
  });

  it('given two sibling statements, when their joint range is located, then the run holds both and the extent covers both lines', () => {
    const located = ok(locate(IF_BODY, 'a = 1', 'b = 2'));

    expect(typesOf(located)).toEqual(['AssignStatement', 'AssignStatement']);
    expect(located.extent).toEqual({ from: lineOf(IF_BODY, 'a = 1'), to: endOf(IF_BODY, 'b = 2\n') });
  });

  it('given a compound statement, when its range is located, then the run is the compound in the def body and the extent holds its lines', () => {
    const located = ok(locate(IF_BODY, 'if x:', 'b = 2'));

    expect(typesOf(located)).toEqual(['IfStatement']);
    expect(located.owner?.type).toBe('FunctionDefinition');
    expect(located.extent).toEqual({ from: lineOf(IF_BODY, 'if x:'), to: endOf(IF_BODY, 'b = 2\n') });
  });

  it('given a whole-lines selection with its indentation and its newline, when located, then it finds the same run as the node own range', () => {
    const whole = locateRun({ text: IF_BODY, root: parse(IF_BODY, 'python'), language: 'python' }, { from: lineOf(IF_BODY, 'a = 1'), to: endOf(IF_BODY, 'b = 2\n') });

    expect(whole).toEqual(locate(IF_BODY, 'a = 1', 'b = 2'));
  });

  it('given a range that starts or ends inside a statement, when located, then it is not a node: no-statement', () => {
    expect(reason(locate(IF_BODY, '= 1', 'b = 2'))).toBe('no-statement');
    expect(reason(locate(IF_BODY, 'a = 1', 'b ='))).toBe('no-statement');
  });

  it('given an explicit empty range, when located, then it is no-statement, and given the selection caret in a statement, then it is the innermost statement there', () => {
    const inside = at(IF_BODY, 'b = 2') + 2;
    const run = (options?: { caret?: boolean }) => locateRun({ text: IF_BODY, root: parse(IF_BODY, 'python'), language: 'python' }, { from: inside, to: inside }, options);

    expect(reason(run())).toBe('no-statement');
    expect(typesOf(ok(run({ caret: true })))).toEqual(['AssignStatement']);
    expect(ok(run({ caret: true })).extent.from).toBe(lineOf(IF_BODY, 'b = 2'));
  });

  it('given the selection caret on a blank line, when located, then it is no-statement', () => {
    const text = 'a = 1\n\nb = 2\n';
    const blank = at(text, '\n\n') + 1;

    expect(reason(locateRun({ text, root: parse(text, 'python'), language: 'python' }, { from: blank, to: blank }, { caret: true }))).toBe('no-statement');
  });

  it('given comment lines directly above a statement at its indent, when it is located, then the extent starts at the first of them; a blank line or another indent ends them', () => {
    const joined = 'def f(x):\n    a = 1\n    # explain\n    # more\n    b = 2\n';
    const blank = 'def f(x):\n    a = 1\n    # explain\n\n    b = 2\n';
    const shallow = 'def f(x):\n    a = 1\n# top\n    b = 2\n';

    expect(ok(locate(joined, 'b = 2')).extent.from).toBe(lineOf(joined, '# explain'));
    expect(ok(locate(blank, 'b = 2')).extent.from).toBe(lineOf(blank, 'b = 2'));
    expect(ok(locate(shallow, 'b = 2')).extent.from).toBe(lineOf(shallow, 'b = 2'));
  });

  it('given a range that starts at a comment, when it is located, then the comment is its own start and nothing above joins', () => {
    const text = 'a = 1\n# one\n# two\nb = 2\n';

    expect(ok(locate(text, '# two', 'b = 2')).extent.from).toBe(lineOf(text, '# two'));
  });

  it('given a document with no final newline, when the last statement is located, then the extent ends at the end of the text and says unterminated', () => {
    const text = 'def f(x):\n    return x';
    const located = ok(locate(text, 'return x'));

    expect(located.extent).toEqual({ from: lineOf(text, 'return x'), to: text.length });
    expect(located.unterminated).toBe(true);
  });

  it('given a comment after the statement on its line, when the first statement of the tree is located, then it is allowed and the extent is the whole line', () => {
    const text = 'a = 1  # note\nb = 2\n';
    const first = outline('python', parse(text, 'python')).children?.[0] as SyntaxNode;

    expect(ok(locateRun({ text, root: parse(text, 'python'), language: 'python' }, first)).extent).toEqual({ from: 0, to: endOf(text, '# note\n') });
  });

  it('given a statement with a comment line and another statement after it, when the comment is the run end, then the run ends at the comment', () => {
    const text = 'a = 1\n# note\nb = 2\n';

    expect(ok(locate(text, 'a = 1', '# note')).run).toHaveLength(2);
  });

  it('given a range of comments only, when located, then it is no-statement', () => {
    const text = 'a = 1\n# one\n# two\nb = 2\n';

    expect(reason(locate(text, '# one', '# two'))).toBe('no-statement');
  });
});

describe('locating a run on hand-built trees', () => {
  const python = 'a = 1; b = 2\n';

  it('given two statements on one line, when either is located, then it is not a node: shares-line', () => {
    const root = node('Script', 0, python.length, [span('AssignStatement', python, 'a = 1'), span('AssignStatement', python, 'b = 2')]);
    const locate = (snippet: string) => locateRun({ text: python, root, language: 'python' }, { from: at(python, snippet), to: endOf(python, snippet) });

    expect(reason(locate('a = 1'))).toBe('shares-line');
    expect(reason(locate('b = 2'))).toBe('shares-line');
  });

  it('given a statement with an error child, when located, then it is not a node: damaged', () => {
    const text = 'for x in\nprint(x)\n';
    const root = node('Script', 0, text.length, [node('ForStatement', 0, 8, [error(8)]), span('ExpressionStatement', text, 'print(x)')]);

    expect(reason(locateRun({ text, root, language: 'python' }, { from: 0, to: 8 }))).toBe('damaged');
    expect(reason(locateRun({ text, root, language: 'python' }, { from: at(text, 'print(x)'), to: endOf(text, 'print(x)') }))).toBe('ok');
  });

  it('given a block with an error of its own, when a statement in it is located, then it is not a node: damaged', () => {
    const text = 'def f():\n    a = 1\n    b = 2\n';
    const body = node('Body', at(text, ':'), text.length, [span('AssignStatement', text, 'a = 1'), error(at(text, '    b')), span('AssignStatement', text, 'b = 2')]);
    const root = node('Script', 0, text.length, [node('FunctionDefinition', 0, text.length, [body])]);

    expect(reason(locateRun({ text, root, language: 'python' }, { from: at(text, 'a = 1'), to: endOf(text, 'a = 1') }))).toBe('damaged');
  });

  it('given a braced block that is never closed, when a statement in it is located, then it is not a node: damaged', () => {
    const text = 'function f() {\n  g();\n';
    const root = parseFor(text, 'javascript');

    expect(reason(locateRun({ text, root, language: 'javascript' }, { from: at(text, 'g();'), to: endOf(text, 'g();') }))).toBe('damaged');
  });
});

describe('locating a run on real Lezer trees', () => {
  const locate = (language: Language, text: string, from: string, to = from) =>
    locateRun({ text, root: realTree(language, text), language }, { from: at(text, from), to: endOf(text, to) });

  it('given a javascript braceless if body and an else-if, when each is located, then it is not a node: not-in-block', () => {
    const text = 'function f(x) {\n  if (x) foo();\n  else if (y) bar();\n}\n';

    expect(reason(locate('javascript', text, 'foo();'))).toBe('not-in-block');
    expect(reason(locate('javascript', text, 'if (y) bar();'))).toBe('not-in-block');
  });

  it('given a cpp braceless while body, when located, then it is not a node: not-in-block', () => {
    const text = 'void f() {\n  while (x) y();\n}\n';

    expect(reason(locate('cpp', text, 'y();'))).toBe('not-in-block');
  });

  it('given a javascript catch clause, when its range is located, then it is not a node: no-statement', () => {
    const text = 'try {\n  a();\n} catch (e) {\n  b();\n}\n';

    expect(reason(locate('javascript', text, 'catch (e) {', '}\n'))).toBe('no-statement');
  });

  it('given a python one-line if, when its body statement is located, then it is not a node: shares-line', () => {
    const text = 'if a: b\nc = 1\n';

    expect(reason(locate('python', text, 'b'))).toBe('shares-line');
  });

  it('given a javascript statement followed by a block comment and another statement, when located, then it is not a node: shares-line', () => {
    const text = 'a(); /* note */ b();\n';

    expect(reason(locate('javascript', text, 'a();'))).toBe('shares-line');
  });

  it('given a javascript statement followed by a block comment and nothing else, when located, then the extent is the whole line', () => {
    const text = 'a(); /* note */\nb();\n';

    expect(ok(locate('javascript', text, 'a();')).extent).toEqual({ from: 0, to: endOf(text, '*/\n') });
  });

  it('given comment lines above a javascript statement, when located, then they join it, a block comment on one line too', () => {
    const text = 'function f() {\n  a();\n  // one\n  /* two */\n  b();\n}\n';

    expect(ok(locate('javascript', text, 'b();')).extent.from).toBe(lineOf(text, '// one'));
  });

  it('given a python line that begins inside a triple-quoted string, when the statement after it is located, then that line is no comment above it', () => {
    const text = 'x = """\n# a\n# b"""\ny = 1\n';

    expect(ok(locate('python', text, 'y = 1')).extent.from).toBe(lineOf(text, 'y = 1'));
  });

  it('given a python decorated function, when located, then the run is the decorated statement', () => {
    const text = '@cache\ndef f():\n    return 1\nx = f()\n';
    const located = ok(locate('python', text, '@cache', 'return 1'));

    expect(typesOf(located)).toEqual(['DecoratedStatement']);
    expect(reason(locate('python', text, 'def f():', 'return 1'))).toBe('not-in-block');
  });

  it('given every language, when a statement in a nested block is located, then the outline classes of the run and its parent are a statement and a block', () => {
    const cases: [Language, string, string][] = [
      ['python', 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n', 'print(x)'],
      ['javascript', 'function f(xs) {\n  for (const x of xs) {\n    g(x);\n  }\n  return 1;\n}\n', 'g(x);'],
      ['cpp', 'int f() {\n  for (int i = 0; i < 3; i++) {\n    g(i);\n  }\n  return 1;\n}\n', 'g(i);'],
    ];

    for (const [language, text, statement] of cases) {
      const located = ok(locate(language, text, statement));

      expect(classify(language, located.run[0] as SyntaxNode)).toBe('simple');
      expect(classify(language, located.parent)).toBe('block');
      expect(classify(language, located.owner as SyntaxNode)).toBe('compound');
    }
  });
});
