import { describe, expect, it } from 'vitest';

import { applyChanges, classify, errorRanges, outline, planBatch, type Change, type Language, type Range, type SyntaxNode } from '../../../src';
import { newErrors, prune, shapeOf, verifyStructure, type Expectation, type Verified } from '../../../src/core/semantic/verify';
import { realTree } from '../../support/lezerPlain';
import { at, endOf, error, node, span } from '../syntax/helpers';

const doc = (text: string) => ({ length: text.length, slice: (from: number, to: number) => text.slice(from, to) });
const lineAt = (text: string, snippet: string): Range => ({ from: text.lastIndexOf('\n', at(text, snippet) - 1) + 1, to: endOf(text, `${snippet}\n`) });

/** Where the first block of the document starts: the function body in these snippets. */
function firstBlockFrom(language: Language, text: string): number {
  const find = (one: SyntaxNode): SyntaxNode | null => (classify(language, one) === 'block' ? one : (one.children ?? []).reduce<SyntaxNode | null>((found, child) => found ?? find(child), null));
  const block = find(outline(language, realTree(language, text)));
  if (!block) throw new Error('the snippet has no block');
  return block.from;
}

/** Plans one splice, applies it to the text and verifies it against both real trees. */
function verify(language: Language, text: string, edits: Change[], expectation: Expectation | null): { verified: Verified; after: string; written: Range | null; changes: readonly Change[] } {
  const planned = planBatch(doc(text), [{ op: 'splice', edits, landed: null }]);
  if (!planned.ok) throw new Error(planned.error.message);
  const { changes, written } = planned.plan;
  const after = applyChanges(text, changes);
  const verified = verifyStructure({ language, base: realTree(language, text), result: realTree(language, after), changes, written, expectations: [expectation] });
  return { verified, after, written: written[0] ?? null, changes };
}

const refusal = (verified: Verified) => (verified.ok ? null : verified.error);

const PY = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n';

describe('verifying the structure of an edit on real trees', () => {
  it('given a statement moved out of an if, when verified against the def body as its parent, then the shape outside the edited ranges is the same and it is ok', () => {
    const old = lineAt(PY, 'b = 2');
    const edits = [{ ...old, insert: '' }, { from: at(PY, '    return b'), to: at(PY, '    return b'), insert: '    b = 2\n' }];

    const { verified } = verify('python', PY, edits, { removed: [old], writes: { parent: firstBlockFrom('python', PY), shapes: [{ is: 'AssignStatement', children: [] }] } });

    expect(verified).toEqual({ ok: true });
  });

  it('given the python return moved one level deeper so that it parses and joins the for body, when verified against the def body as its parent, then it is would-not-parse structure naming the return', () => {
    const text = 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n';
    const old = lineAt(text, '    return 1'.trimStart());

    const { verified, after } = verify('python', text, [{ ...old, insert: '        return 1\n' }], { removed: [old], writes: { parent: firstBlockFrom('python', text) } });

    expect(refusal(verified)).toMatchObject({ code: 'would-not-parse', reason: 'structure' });
    const [named] = refusal(verified)?.errors ?? [];
    expect(after.slice(named?.from, named?.to)).toBe('return 1');
  });

  it('given a result with the same shape, when the written text is a statement in the block it was meant for, then it is ok for braces too', () => {
    const text = 'function f() {\n  a();\n  b();\n}\n';
    const old = lineAt(text, '  a();'.trimStart());
    const edits = [{ ...old, insert: '' }, { from: endOf(text, 'b();\n'), to: endOf(text, 'b();\n'), insert: '  a();\n' }];

    const { verified } = verify('javascript', text, edits, { removed: [old], writes: { parent: firstBlockFrom('javascript', text), shapes: [{ is: 'simple', children: [] }] } });

    expect(verified).toEqual({ ok: true });
  });

  it('given a new error node, when the errors are compared, then it is reported with its range in the result text', () => {
    const text = 'def f():\n    return 1\n';
    const edit = { from: at(text, '1\n'), to: at(text, '1\n') + 1, insert: '(' };
    const changes = [edit];
    const after = applyChanges(text, changes);

    const fresh = newErrors(realTree('python', text), realTree('python', after), changes);

    expect(fresh.length).toBeGreaterThan(0);
    for (const range of fresh) expect(range.to).toBeLessThanOrEqual(after.length);
  });

  it('given an error that was already there outside the edit, when the errors are compared, then it is not new', () => {
    const text = 'def f(:\n    pass\n\ndef g():\n    return 1\n';
    const edit = { from: at(text, '1\n'), to: at(text, '1\n') + 1, insert: '2' };
    const after = applyChanges(text, [edit]);

    expect(errorRanges(realTree('python', text)).length).toBeGreaterThan(0);
    expect(newErrors(realTree('python', text), realTree('python', after), [edit])).toEqual([]);
  });

  it('given an error that moves with the text above it, when the errors are compared, then it is still not new', () => {
    const text = 'a = 1\ndef f(:\n    pass\n';
    const edit = { from: 0, to: 0, insert: 'b = 2\nc = 3\n' };
    const after = applyChanges(text, [edit]);

    expect(newErrors(realTree('python', text), realTree('python', after), [edit])).toEqual([]);
  });

  it('given an inserted else: that parses as a clause of the previous if, when verified with the def body as the expected parent, then it is structure', () => {
    const text = 'def f(x):\n    if x:\n        a = 1\n    return a\n';
    const insert = '    else:\n        a = 2\n';
    const place = at(text, '    return a');

    const { verified } = verify('python', text, [{ from: place, to: place, insert }], { writes: { parent: firstBlockFrom('python', text) } });

    expect(refusal(verified)).toMatchObject({ code: 'would-not-parse', reason: 'structure' });
  });

  it('given an inserted else block in javascript, when verified with the function body as the expected parent, then it is structure', () => {
    const text = 'function f(x) {\n  if (x) {\n    a = 1;\n  }\n  return a;\n}\n';
    const place = at(text, '  return a;');

    const { verified } = verify('javascript', text, [{ from: place, to: place, insert: '  else {\n    a = 2;\n  }\n' }], { writes: { parent: firstBlockFrom('javascript', text) } });

    expect(refusal(verified)).toMatchObject({ code: 'would-not-parse', reason: 'structure' });
  });

  it('given a javascript wrap header `if (x) foo();` that parses with the lines in a bare block, when verified against the wrapper shape, then it is structure naming the written range', () => {
    const text = 'function f() {\n  a();\n  b();\n}\n';
    const old = { from: at(text, '  a();'), to: endOf(text, 'b();\n') };
    const wrapper: Expectation = {
      removed: [old],
      writes: { parent: firstBlockFrom('javascript', text), shapes: [{ is: 'compound', children: [{ is: 'block', children: [{ is: 'simple', children: [] }, { is: 'simple', children: [] }] }] }] },
    };

    const bad = verify('javascript', text, [{ ...old, insert: '  if (x) foo(); {\n    a();\n    b();\n  }\n' }], wrapper);
    const good = verify('javascript', text, [{ ...old, insert: '  for (const x of xs) {\n    a();\n    b();\n  }\n' }], wrapper);

    expect(refusal(bad.verified)).toMatchObject({ code: 'would-not-parse', reason: 'structure' });
    const [named] = refusal(bad.verified)?.errors ?? [];
    expect(named?.from).toBeGreaterThanOrEqual(bad.written?.from ?? Infinity);
    expect(named?.to).toBeLessThanOrEqual(bad.written?.to ?? 0);
    expect(good.verified).toEqual({ ok: true });
  });

  it('given a python wrap whose body is one statement short of the shape it promised, when verified, then it is structure naming the wrapper', () => {
    const text = 'def f():\n    a = 1\n    b = 2\n';
    const old = { from: at(text, '    a = 1'), to: text.length };
    const promise: Expectation = {
      removed: [old],
      writes: { parent: firstBlockFrom('python', text), shapes: [{ is: 'compound', children: [{ is: 'block', children: [{ is: 'simple', children: [] }, { is: 'simple', children: [] }] }] }] },
    };

    const { verified, after } = verify('python', text, [{ ...old, insert: '    for i in range(2):\n        a = 1\n    b = 2\n' }], promise);

    expect(refusal(verified)).toMatchObject({ reason: 'structure' });
    const [named] = refusal(verified)?.errors ?? [];
    expect(after.slice(named?.from, named?.to)).toContain('for i in range(2):');
  });

  it('given comments added or removed, when verified, then the shapes are unaffected', () => {
    const text = 'a();\nb();\n';
    const added = verify('javascript', text, [{ from: 0, to: 0, insert: '// note\n' }], { writes: { parent: 'root', shapes: [] } });
    const commented = '// note\na();\nb();\n';
    const old = lineAt(commented, '// note');
    const removed = verify('javascript', commented, [{ ...old, insert: '' }], { removed: [old] });

    expect(added.verified).toEqual({ ok: true });
    expect(removed.verified).toEqual({ ok: true });
  });

  it('given a result that loses a statement outside the declared ranges, when verified, then it is structure naming the block that lost it', () => {
    const text = 'function f() {\n  a();\n  b();\n}\n';
    const old = lineAt(text, 'b();');

    const { verified, after } = verify('javascript', text, [{ ...old, insert: '' }], { removed: [] });

    expect(refusal(verified)).toMatchObject({ code: 'would-not-parse', reason: 'structure' });
    const [named] = refusal(verified)?.errors ?? [];
    expect(after.slice(named?.from, named?.to)).toBe('{\n  a();\n}');
  });

  it('given text written that no step vouched for, when verified, then it is structure naming the written range', () => {
    const text = 'a();\n';

    const { verified, written } = verify('javascript', text, [{ from: 0, to: 0, insert: 'b();\n' }], null);

    expect(refusal(verified)).toMatchObject({ reason: 'structure', errors: [written] });
  });

  it('given a bare block written in the right place where the step promised no shape, when verified, then it is structure naming the block', () => {
    const text = 'a();\n';

    const { verified, after } = verify('javascript', text, [{ from: 0, to: 0, insert: '{\n  b();\n}\n' }], { writes: { parent: 'root' } });

    expect(refusal(verified)).toMatchObject({ reason: 'structure' });
    const [named] = refusal(verified)?.errors ?? [];
    expect(after.slice(named?.from, named?.to)).toBe('{\n  b();\n}');
  });

  it('given the written text is two statements, when the step promised one, then it is structure', () => {
    const text = 'a();\n';

    const { verified } = verify('javascript', text, [{ from: 0, to: 0, insert: 'b();\nc();\n' }], { writes: { parent: 'root', shapes: [{ is: 'simple' }] } });

    expect(refusal(verified)).toMatchObject({ reason: 'structure' });
  });
});

describe('the shape of an outline', () => {
  const text = '# top\ndef f():\n    a = 1\n    # inside\n    b = 2\n';
  const body = node('Body', at(text, ':'), text.length, [span('AssignStatement', text, 'a = 1'), span('Comment', text, '# inside'), span('AssignStatement', text, 'b = 2'), error(text.length)]);
  const tree = node('Script', 0, text.length, [span('Comment', text, '# top'), node('FunctionDefinition', at(text, 'def'), text.length, [body])]);

  it('given an outline with comments and error nodes, when its shape is taken, then the types and the nesting are left', () => {
    expect(shapeOf('python', tree)).toEqual({ is: 'Script', children: [{ is: 'FunctionDefinition', children: [{ is: 'Body', children: [{ is: 'AssignStatement', children: [] }, { is: 'AssignStatement', children: [] }] }] }] });
  });

  it('given a range that wholly covers a statement, when the outline is pruned, then that node is gone and a node it only partly covers stays', () => {
    const pruned = prune(tree, [{ from: at(text, 'a = 1'), to: endOf(text, 'a = 1\n') }, { from: at(text, 'def'), to: at(text, 'def') + 3 }]);

    const kept = pruned.children?.find((one) => one.type === 'FunctionDefinition');
    expect(kept?.children?.[0]?.children?.map((one) => one.type)).toEqual(['Comment', 'AssignStatement', '⚠']);
  });

  it('given a range around everything, when the outline is pruned, then the root stays', () => {
    expect(prune(tree, [{ from: 0, to: text.length }])).toMatchObject({ type: 'Script', from: 0, to: text.length });
  });
});
