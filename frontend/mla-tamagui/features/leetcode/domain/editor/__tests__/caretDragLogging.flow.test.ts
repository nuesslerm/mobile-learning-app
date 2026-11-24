import { createDeferredEngine, createEditorClient } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import { createCaretDrag, moveCaretBy } from '../caretDrag';
import { DEFAULT_TUNING } from '../cursorDrag';

const { CHAR_PX } = DEFAULT_TUNING;
const TEXT = 'alpha beta.gamma(delta, epsilon) zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau';

describe('a caret drag, as logged', () => {
  it('given a drag of thirty moves, when it ends, then one editor.caret_drag line says how many moves, how many calls and how it ended', async () => {
    await forgetLoggedLines();
    const drag = createCaretDrag(createEditorClient(createFakeEditor({ text: TEXT }).engine));

    for (let step = 1; step <= 30; step += 1) drag.move({ dx: step * CHAR_PX, dy: 0 });
    await drag.end();

    const lines = await loggedLines('editor.caret_drag');
    expect(lines).toHaveLength(1);
    expect(lines[0]?.context).toMatchObject({ moves: 30, calls: 2, outcome: 'moved' });
  });

  it('given a tap, when it ends, then the line says no call was made and the outcome is tap', async () => {
    await forgetLoggedLines();
    const drag = createCaretDrag(createEditorClient(createFakeEditor({ text: TEXT }).engine));

    await drag.end();

    const lines = await loggedLines('editor.caret_drag');
    expect(lines).toHaveLength(1);
    expect(lines[0]?.context).toMatchObject({ moves: 0, calls: 0, outcome: 'tap' });
  });

  it('given a drag with a keystroke in the middle, when it ends, then the line names stale as the outcome and two calls', async () => {
    await forgetLoggedLines();
    const fake = createFakeEditor({ text: TEXT });
    const drag = createCaretDrag(createEditorClient(fake.engine));
    const firstSelect = new Promise<void>((resolve) => {
      const stop = fake.engine.listen(['selection'], () => {
        stop();
        resolve();
      });
    });

    drag.move({ dx: 4 * CHAR_PX, dy: 0 });
    await firstSelect;
    fake.userTypes('k');
    drag.move({ dx: 9 * CHAR_PX, dy: 0 });
    await drag.end();

    const lines = await loggedLines('editor.caret_drag');
    expect(lines).toHaveLength(1);
    expect(lines[0]?.context).toMatchObject({ moves: 2, calls: 2, outcome: 'stale' });
  });

  it('given a caret moved by one motion, when it lands or is refused, then one editor.caret_move line says the motion and the outcome', async () => {
    await forgetLoggedLines();

    await moveCaretBy(createEditorClient(createFakeEditor({ text: TEXT }).engine), { unit: 'word', count: 1 });
    await moveCaretBy(createEditorClient(createDeferredEngine(() => undefined)), { unit: 'char', count: -1 });

    const lines = await loggedLines('editor.caret_move');
    expect(lines.map((line) => line.context)).toMatchObject([
      { unit: 'word', count: 1, outcome: 'moved' },
      { unit: 'char', count: -1, outcome: 'not-ready' },
    ]);
  });
});
