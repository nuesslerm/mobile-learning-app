import { createDeferredEngine, createEditorClient, unwrap, type EditorEngine, type RequestName } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import type { EditAlternative } from '@lib/voice-edit-dsl';
import { describe, expect, it } from 'vitest';

import { applyPlannedEdit, planVoiceEdit, readBasis, type EditBasis, type PlannedEdit } from '../voiceEditPlan';

const alt = (commands: EditAlternative['commands']): Pick<EditAlternative, 'commands'> => ({ commands });

function editor(text: string, instance = 'fake-0') {
  const fake = createFakeEditor({ text, instance });
  const client = createEditorClient(fake.engine);
  return { fake, client };
}

async function planned(text: string, commands: EditAlternative['commands']) {
  const { fake, client } = editor(text);
  const basis = await readBasis(client);
  if (!basis) throw new Error('the fake editor gave no basis');
  const result = planVoiceEdit(basis, alt(commands));
  if (!result.ok) throw new Error(`refused: ${result.reason}`);
  return { fake, client, basis, edit: result.edit };
}

const textSteps = (edit: PlannedEdit) => edit.steps.filter((step) => step.op === 'replace');

describe('readBasis', () => {
  it('given an editor holding text, when the basis is read, then it names the text, its revision and the editor instance', async () => {
    const { fake, client } = editor('abc', 'page-7');
    fake.userTypes('x');

    expect(await readBasis(client)).toEqual({ text: 'xabc', revision: 1, instance: 'page-7' });
  });

  it('given an engine with no editor behind it, when the basis is read, then there is none', async () => {
    const client = createEditorClient(createDeferredEngine(() => undefined));

    expect(await readBasis(client)).toBeNull();
  });
});

describe('planVoiceEdit and applyPlannedEdit', () => {
  it.each([
    ['a replaceRange', 'function add(a, b) { return a + b; }', [{ type: 'replaceRange', start: 9, end: 12, newText: 'sum' }], 'function sum(a, b) { return a + b; }', { from: 9, to: 12 }],
    ['an insertAt', 'console.log();', [{ type: 'insertAt', position: 12, text: '"hello"' }], 'console.log("hello");', { from: 12, to: 19 }],
    ['a full replacement', 'old', [{ type: 'replaceRange', start: 0, end: 3, newText: 'new' }], 'new', { from: 0, to: 3 }],
  ] as const)('given %s, when planned and applied at the basis revision, then the text is the edited text and landed is the changed span', async (_name, text, commands, expected, span) => {
    const { fake, client, edit } = await planned(text, [...commands]);

    const outcome = await applyPlannedEdit(client, edit);

    expect(outcome).toEqual({ ok: true, modifiedText: expected, landed: span, revision: 1 });
    expect(fake.text()).toBe(expected);
    expect(edit.modifiedText).toBe(expected);
  });

  it('given a replaceRange alternative, when planned, then the steps are one replace of the differing middle and one select', async () => {
    const { edit } = await planned('function add(a, b) { return a + b; }', [{ type: 'replaceRange', start: 9, end: 12, newText: 'sum' }]);

    expect(edit.steps).toEqual([
      { op: 'replace', range: { from: 9, to: 12 }, text: 'sum' },
      { op: 'select', anchor: { step: 0, edge: 'start', offset: 3 } },
    ]);
  });

  it('given an insertAt alternative, when it is applied at the basis revision, then landed covers exactly the inserted text', async () => {
    const { fake, client, edit } = await planned('x = 1\ny = 2', [{ type: 'insertAt', position: 5, text: '; z = 3' }]);

    const outcome = await applyPlannedEdit(client, edit);

    expect(fake.text()).toBe('x = 1; z = 3\ny = 2');
    expect(outcome.ok && outcome.landed).toEqual({ from: 5, to: 12 });
  });

  it('given an alternative with no commands, when planned and applied, then there are no steps and nothing changes', async () => {
    const { fake, client, edit } = await planned('const x = 1;', []);
    const before = fake.peek();

    const outcome = await applyPlannedEdit(client, edit);

    expect(edit.steps).toEqual([]);
    expect(outcome).toEqual({ ok: true, modifiedText: 'const x = 1;', landed: null, revision: 0 });
    expect(fake.peek()).toEqual(before);
  });

  it('given a replacement with the text it replaces, when planned and applied, then there is no text step and the text and revision stay', async () => {
    const { fake, client, edit } = await planned('const x = 1;', [{ type: 'replaceRange', start: 6, end: 7, newText: 'x' }]);

    const outcome = await applyPlannedEdit(client, edit);

    expect(textSteps(edit)).toEqual([]);
    expect(outcome.ok && outcome.landed).toBeNull();
    expect(fake.text()).toBe('const x = 1;');
    expect(fake.peek().revision).toBe(0);
  });

  it('given two overlapping commands, when planned, then it is the refusal invalid-plan and nothing throws', async () => {
    const { client } = editor('abcdefghij');
    const basis = (await readBasis(client)) as EditBasis;

    const result = planVoiceEdit(basis, alt([
      { type: 'replaceRange', start: 2, end: 6, newText: 'X' },
      { type: 'replaceRange', start: 4, end: 8, newText: 'Y' },
    ]));

    expect(result).toMatchObject({ ok: false, reason: 'invalid-plan', code: 'invalid-plan' });
    expect(result.ok ? '' : result.message).toContain('overlaps');
  });

  it("given a line-anchored alternative on the starter's whitespace stub line, when planned and applied, then the block sits one level into the method and nothing doubles its indentation", async () => {
    const starter = 'class Solution:\n    def twoSum(self, nums, target):\n        ';
    const { fake, client, edit } = await planned(starter, [{ type: 'insertAfterLine', line: 3, text: '        seen = {}\n        return []' }]);

    await applyPlannedEdit(client, edit);

    expect(fake.text()).toBe(`${starter}seen = {}\n        return []`);
  });

  it('given the editor still at the basis revision, when the plan is applied, then it lands as one undo step and the caret is where the last command put it', async () => {
    const { fake, client, edit } = await planned('a = 1\nb = 2\nc = 3', [
      { type: 'replaceRange', start: 0, end: 1, newText: 'first' },
      { type: 'insertAt', position: 11, text: ' + 9' },
    ]);

    await applyPlannedEdit(client, edit);

    expect(fake.text()).toBe('first = 1\nb = 2 + 9\nc = 3');
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 19, head: 19 });
    expect(fake.peek().undoDepth).toBe(1);
    unwrap(await client.undo());
    expect(fake.text()).toBe('a = 1\nb = 2\nc = 3');
  });

  it('given a keystroke typed after the basis was read, when the plan is applied, then it is refused as stale, the typed text is in the buffer and the undo depth did not grow', async () => {
    const { fake, client, edit } = await planned('abc def', [{ type: 'replaceRange', start: 4, end: 7, newText: 'xyz' }]);
    fake.userTypes('!');
    const undoDepth = fake.peek().undoDepth;

    const outcome = await applyPlannedEdit(client, edit);

    expect(outcome).toMatchObject({ ok: false, reason: 'stale', code: 'stale' });
    expect(fake.text()).toBe('!abc def');
    expect(fake.peek().undoDepth).toBe(undoDepth);
  });

  it('given a basis from another editor instance, when applied, then it is refused as stale through the instance mismatch and the buffer is untouched', async () => {
    const { edit } = await planned('abc def', [{ type: 'replaceRange', start: 4, end: 7, newText: 'xyz' }]);
    const other = editor('abc def', 'fake-1');

    const outcome = await applyPlannedEdit(other.client, edit);

    expect(outcome).toMatchObject({ ok: false, reason: 'stale', code: 'instance-mismatch' });
    expect(other.fake.text()).toBe('abc def');
    expect(other.fake.peek().revision).toBe(0);
  });

  it('given a rename with three occurrences, when planned, then it is one hunk from the first to the last occurrence', async () => {
    const text = 'def f(count):\n    a = count\n    return a + count\n';
    const { edit } = await planned(text, [{ type: 'renameSymbol', position: text.indexOf('count'), newName: 'n' }]);

    const [hunk] = textSteps(edit);

    expect(textSteps(edit)).toHaveLength(1);
    expect(hunk).toEqual({
      op: 'replace',
      range: { from: text.indexOf('count'), to: text.lastIndexOf('count') + 'count'.length },
      text: 'n):\n    a = n\n    return a + n',
    });
    expect(edit.modifiedText).toBe('def f(n):\n    a = n\n    return a + n\n');
  });

  it('given a deferred engine with no editor behind it, when a plan is applied, then the outcome is not-ready and the call is made once', async () => {
    const deferred = createDeferredEngine(() => undefined);
    const calls: RequestName[] = [];
    const engine: EditorEngine = {
      request: (name, params) => {
        calls.push(name);
        return deferred.request(name, params);
      },
      listen: (types, onEvent) => deferred.listen(types, onEvent),
    };
    const basis: EditBasis = { text: 'abc', revision: 0, instance: 'fake-0' };
    const result = planVoiceEdit(basis, alt([{ type: 'insertAt', position: 3, text: 'd' }]));
    if (!result.ok) throw new Error('planning failed');

    const outcome = await applyPlannedEdit(createEditorClient(engine), result.edit);

    expect(outcome).toMatchObject({ ok: false, reason: 'not-ready', code: 'not-ready' });
    expect(calls).toEqual(['apply']);
  });
});
