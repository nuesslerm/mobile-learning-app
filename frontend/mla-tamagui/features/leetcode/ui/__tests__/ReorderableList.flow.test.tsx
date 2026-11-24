import { act, fireEvent, renderScreen, waitFor, within } from '@lib/flow-test-harness';
import { Text } from '@lib/tamagui-design-system';
import { Menu } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import { useState } from 'react';
import { describe, expect, it } from 'vitest';

import { ReorderableList, type ReorderActivation } from '../ReorderableList';

const ROW_HEIGHT = 48;

function Scene({ activation, withIcon }: { activation?: ReorderActivation; withIcon?: boolean }) {
  const [items, setItems] = useState(['one', 'two', 'three']);
  return (
    <ReorderableList
      data={items}
      keyExtractor={(item) => item}
      rowHeight={ROW_HEIGHT}
      activation={activation}
      handleIcon={withIcon ? Menu : undefined}
      onReorder={setItems}
      renderItem={(item, handle) => (
        <XStack testID={`Row-${item}`} items="center" height={ROW_HEIGHT}>
          <Text flex={1}>{item}</Text>
          {handle}
        </XStack>
      )}
    />
  );
}

const names = (container: HTMLElement) =>
  Array.from(container.querySelectorAll('[data-testid^="Row-"]')).map((row) => row.getAttribute('data-testid')?.replace('Row-', ''));

async function dragHandleOf(container: HTMLElement, name: string, distance: number) {
  const handle = within(within(container).getByTestId(`Row-${name}`)).getByTestId('ReorderableListHandle');
  await act(async () => {
    fireEvent.pointerDown(handle, { clientX: 10, clientY: 0 });
    fireEvent.pointerMove(handle, { clientX: 10, clientY: distance });
    fireEvent.pointerUp(handle, { clientX: 10, clientY: distance });
  });
}

describe('ReorderableList with its opt-in props', () => {
  it('Given immediate activation, another handle glyph and no scroll props, When a handle is dragged one row height, Then the order changes by one', async () => {
    // Given a three-row list with the opt-ins and no scroll view around it
    const view = renderScreen(<Scene activation="immediate" withIcon />);
    expect(names(view.container)).toEqual(['one', 'two', 'three']);

    // When the first row is dragged one row down
    await dragHandleOf(view.container, 'one', ROW_HEIGHT);

    // Then it sits below the second and the others keep their places
    await waitFor(() => expect(names(view.container)).toEqual(['two', 'one', 'three']));
  });

  it('Given the opt-ins, When the last row is dragged one row up, Then the last two swap', async () => {
    // Given the same list
    const view = renderScreen(<Scene activation="immediate" withIcon />);

    // When the last row is dragged one row up
    await dragHandleOf(view.container, 'three', -ROW_HEIGHT);

    // Then it sits above the second
    await waitFor(() => expect(names(view.container)).toEqual(['one', 'three', 'two']));
  });

  it('Given no props beyond the required ones, When a handle is dragged, Then the list still reorders and each handle holds the grip', async () => {
    // Given a list with the defaults and a list with another glyph
    const plain = renderScreen(<Scene />);
    // the real icons render: the grip is six dots (circles), the three-line glyph has none
    const dotsOf = (container: HTMLElement) => within(container).getAllByTestId('ReorderableListHandle')[0].querySelectorAll('circle').length;
    expect(dotsOf(plain.container)).toBe(6);

    // When the first row is dragged down
    await dragHandleOf(plain.container, 'one', ROW_HEIGHT);

    // Then it reorders as it did before the opt-ins existed
    await waitFor(() => expect(names(plain.container)).toEqual(['two', 'one', 'three']));
    plain.unmount();
    const custom = renderScreen(<Scene withIcon />);
    expect(dotsOf(custom.container)).toBe(0);
    expect(within(custom.container).getAllByTestId('ReorderableListHandle')[0].querySelector('svg')).toBeTruthy();
  });
});
