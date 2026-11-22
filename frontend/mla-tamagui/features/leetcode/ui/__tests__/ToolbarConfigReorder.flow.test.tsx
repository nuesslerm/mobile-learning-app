import { act, fireEvent, screen, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_TOOLBAR_ITEMS, toolbarItemKey } from '../../domain';
import { useToolbarConfigStore } from '../../state/toolbarConfigStore';

const ROW_HEIGHT = 64;

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => useToolbarConfigStore.setState({ items: DEFAULT_TOOLBAR_ITEMS }));

const order = () => useToolbarConfigStore.getState().items.map(toolbarItemKey);
const shownNames = () => screen.getAllByTestId('ToolbarConfigRowName').map((name) => name.textContent);

// the pan the way a finger makes it: down on the handle, moves, up. The stub's pan starts on its first move, so the 250 ms long press is not part of this.
async function dragHandle(index: number, distance: number) {
  const handle = screen.getAllByTestId('ReorderableListHandle')[index];
  await act(async () => {
    fireEvent.pointerDown(handle, { clientX: 20, clientY: 0 });
    fireEvent.pointerMove(handle, { clientX: 20, clientY: distance / 2 });
    fireEvent.pointerMove(handle, { clientX: 20, clientY: distance });
    fireEvent.pointerUp(handle, { clientX: 20, clientY: distance });
  });
}

describe('the keyboard bar screen reorders its keys by dragging a handle', () => {
  it('Given the default key list, When the first row\'s handle is dragged one row down, Then the first two keys swap and the rows read in the new order', async () => {
    // Given the default list on screen
    await app.render('/leetcode/toolbar');
    const before = order();
    expect(shownNames().slice(0, 3)).toEqual(['Indent', 'Dedent', 'Parentheses']);

    // When the first handle is dragged one row height down
    await dragHandle(0, ROW_HEIGHT);

    // Then the store holds Dedent before Indent and the rest unchanged, and the screen shows it
    await waitFor(() => expect(order()).toEqual([before[1], before[0], ...before.slice(2)]));
    await waitFor(() => expect(shownNames().slice(0, 3)).toEqual(['Dedent', 'Indent', 'Parentheses']));
  });

  it('Given the default key list, When the third row\'s handle is dragged two rows up, Then it becomes the first key', async () => {
    // Given the default list on screen
    await app.render('/leetcode/toolbar');
    const before = order();

    // When the third handle is dragged two row heights up
    await dragHandle(2, -2 * ROW_HEIGHT);

    // Then that key leads the list and the others follow in their old order
    await waitFor(() => expect(order()).toEqual([before[2], before[0], before[1], ...before.slice(3)]));
  });

  it('Given a drag shorter than half a row, When the handle is released, Then the order is unchanged', async () => {
    // Given the default list on screen
    await app.render('/leetcode/toolbar');
    const before = order();

    // When the first handle is dragged a quarter of a row
    await dragHandle(0, ROW_HEIGHT / 4);

    // Then no key moved
    expect(order()).toEqual(before);
  });
});
