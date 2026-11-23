import { describe, expect, it } from 'vitest';

import type { ErrorCheckpoint, ErrorJump as EditorJump } from '../../../domain/editor/errorJump';
import type { RunEditor } from '../../../domain/run/runEditor';
import { createErrorJump } from '../useErrorJump';

const CHECKPOINT: ErrorCheckpoint = { text: 'a\nb', revision: 1 as never, instance: 'i' };

function editorWith(answer: () => Promise<EditorJump>): RunEditor & { cleared: number } {
  const editor = {
    cleared: 0,
    checkpoint: async () => CHECKPOINT,
    showErrorLine: answer,
    clearErrorLine: async () => {
      editor.cleared += 1;
    },
  };
  return editor;
}

const SHOWN: EditorJump = { kind: 'shown', line: 2, range: { from: 2, to: 3 } };

describe('createErrorJump', () => {
  it('Given a mark asked for and the result cleared before the editor answered, When the answer lands, Then the mark is taken away again', async () => {
    let release: (value: EditorJump) => void = () => undefined;
    const editor = editorWith(() => new Promise<EditorJump>((resolve) => (release = resolve)));
    const jump = createErrorJump(() => editor);

    const pending = jump.show(CHECKPOINT, { line: 2 });
    jump.clear();
    release(SHOWN);

    expect(await pending).toEqual({ kind: 'stale' });
    expect(editor.cleared).toBe(2);
  });

  it('Given a located jump, When the editor shows it, Then the line is reported', async () => {
    const jump = createErrorJump(() => editorWith(async () => SHOWN));

    expect(await jump.show(CHECKPOINT, { line: 2 })).toEqual({ kind: 'shown', line: 2 });
  });

  it('Given no location, When asked to show, Then there is nothing to show', async () => {
    const jump = createErrorJump(() => editorWith(async () => ({ kind: 'no-location' })));

    expect(await jump.show(CHECKPOINT, undefined)).toEqual({ kind: 'none' });
  });

  it('Given a shown line, When it is shown again, Then the same place is marked', async () => {
    const jump = createErrorJump(() => editorWith(async () => SHOWN));
    await jump.show(CHECKPOINT, { line: 2 });

    expect(await jump.showAgain()).toEqual({ kind: 'shown', line: 2 });
  });
});
