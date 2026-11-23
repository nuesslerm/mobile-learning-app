import { act, fireEvent, renderScreen, waitFor, within } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { resetGroups } from '../../../domain/editor-settings/symbolGroups';
import { DEFAULT_RAIL_STYLE, DEFAULT_SYMBOL_BAR_SHOWN, useSymbolBarStore } from '../../../state/editor/symbolBarStore';
import { SymbolBarSheet } from '../SymbolBarSheet';

// The sheet is a component that reads and writes only the symbol bar store and has no composition root, so it renders through renderScreen
// (sheetBranches is the precedent); the route-level test that mounts it on the editor screen comes with the slice that mounts it.

const ROW_HEIGHT = 56;

// reset before the sheet mounts: a store write while a component is mounted is an update outside act
beforeEach(() => useSymbolBarStore.setState({ shown: DEFAULT_SYMBOL_BAR_SHOWN, groups: resetGroups(), railStyle: DEFAULT_RAIL_STYLE }));

async function openSheet(onOpenChange: (open: boolean) => void = () => {}) {
  const view = renderScreen(<SymbolBarSheet open onOpenChange={onOpenChange} />);
  await waitFor(() => expect(view.getByTestId('SymbolBarSheetHeader')).toBeTruthy());
  return view;
}

type View = Awaited<ReturnType<typeof openSheet>>;

const rowIds = (view: View) =>
  Array.from(view.container.querySelectorAll('[data-testid^="SymbolBarRow-"]')).map((row) => row.getAttribute('data-testid')?.replace('SymbolBarRow-', ''));
const labels = (view: View) => rowIds(view).map((id) => view.getByTestId(`SymbolBarRowLabel-${id}`).textContent);
const chars = (view: View) => rowIds(view).map((id) => (view.getByTestId(`SymbolBarChars-${id}`) as HTMLInputElement).value);
const switchOf = (view: View) => within(view.getByTestId('SymbolBarSwitch')).getByRole('switch') as HTMLInputElement;
const isDisabled = (element: HTMLElement) => element.getAttribute('aria-disabled') === 'true' || (element as HTMLButtonElement).disabled === true;

async function drag(view: View, id: string, distance: number) {
  const handle = within(view.getByTestId(`SymbolBarRow-${id}`)).getByTestId('ReorderableListHandle');
  await act(async () => {
    fireEvent.pointerDown(handle, { clientX: 10, clientY: 0 });
    fireEvent.pointerMove(handle, { clientX: 10, clientY: distance });
    fireEvent.pointerUp(handle, { clientX: 10, clientY: distance });
  });
}

const DEFAULT_CHARS = ['#$^%', '&|?!', '\\@~_', '+-*/', '\'"`=', '{}[]', '()<>', '.:,;'];

describe('the Symbol Bar sheet', () => {
  it('Given the defaults, When the sheet opens, Then Reset is disabled, the switch is on, eight rows read Group 1 to 8 with their characters and the captions are the recording\'s', async () => {
    const view = await openSheet();

    expect(isDisabled(view.getByTestId('SymbolBarReset'))).toBe(true);
    expect(switchOf(view).checked).toBe(true);
    expect(labels(view)).toEqual(['Group 1', 'Group 2', 'Group 3', 'Group 4', 'Group 5', 'Group 6', 'Group 7', 'Group 8']);
    expect(chars(view)).toEqual(DEFAULT_CHARS);
    expect(view.getByText('Symbol Bar')).toBeTruthy();
    expect(view.getByText('Show Symbol Bar')).toBeTruthy();
    expect(view.getByTestId('SymbolBarSwitchCaption').textContent).toBe('Shown above the keyboard while editing code.');
    expect(view.getByTestId('SymbolBarGroupsCaption').textContent).toBe('Each group holds up to 4 characters. Leave a group empty to hide it. Drag the handle to reorder.');
  });

  it('Given the defaults, When group 2 is dragged below group 3, Then the rows keep their places\' names, the characters follow the new order and Reset is enabled', async () => {
    const view = await openSheet();

    await drag(view, 'g2', ROW_HEIGHT);

    await waitFor(() => expect(chars(view).slice(0, 3)).toEqual(['#$^%', '\\@~_', '&|?!']));
    expect(labels(view)).toEqual(['Group 1', 'Group 2', 'Group 3', 'Group 4', 'Group 5', 'Group 6', 'Group 7', 'Group 8']);
    expect(rowIds(view).slice(0, 3)).toEqual(['g1', 'g3', 'g2']);
    expect(useSymbolBarStore.getState().groups.map((group) => group.id).slice(0, 3)).toEqual(['g1', 'g3', 'g2']);
    expect(isDisabled(view.getByTestId('SymbolBarReset'))).toBe(false);
  });

  it('Given a group dragged two rows up, When it lands, Then the store holds that order', async () => {
    const view = await openSheet();

    await drag(view, 'g4', -ROW_HEIGHT * 2);

    await waitFor(() => expect(useSymbolBarStore.getState().groups.map((group) => group.id)).toEqual(['g1', 'g4', 'g2', 'g3', 'g5', 'g6', 'g7', 'g8']));
  });

  it('Given a reordered list, When Reset is pressed, Then the defaults are back and Reset is disabled again', async () => {
    const view = await openSheet();
    await drag(view, 'g2', ROW_HEIGHT);
    await waitFor(() => expect(isDisabled(view.getByTestId('SymbolBarReset'))).toBe(false));

    fireEvent.click(view.getByTestId('SymbolBarReset'));

    await waitFor(() => expect(chars(view)).toEqual(DEFAULT_CHARS));
    expect(isDisabled(view.getByTestId('SymbolBarReset'))).toBe(true);
  });

  it('Given the switch is on, When it is turned off, Then the store says the bar is hidden and the switch reads off', async () => {
    const view = await openSheet();

    fireEvent.click(switchOf(view));

    await waitFor(() => expect(useSymbolBarStore.getState().shown).toBe(false));
    expect(switchOf(view).checked).toBe(false);
  });

  it('Given a row, When its characters are edited to abcdef, Then four of them are kept and shown, and Reset is enabled', async () => {
    const view = await openSheet();

    fireEvent.change(view.getByTestId('SymbolBarChars-g1'), { target: { value: 'abcdef' } });

    await waitFor(() => expect(useSymbolBarStore.getState().groups[0].chars).toBe('abcd'));
    await waitFor(() => expect((view.getByTestId('SymbolBarChars-g1') as HTMLInputElement).value).toBe('abcd'));
    expect(isDisabled(view.getByTestId('SymbolBarReset'))).toBe(false);
  });

  it('Given a row, When its characters are emptied, Then the group stays in the sheet and the store holds it empty, which hides it from the rail', async () => {
    const view = await openSheet();

    fireEvent.change(view.getByTestId('SymbolBarChars-g4'), { target: { value: '' } });

    await waitFor(() => expect(useSymbolBarStore.getState().groups[3].chars).toBe(''));
    expect(rowIds(view)).toHaveLength(8);
    expect((view.getByTestId('SymbolBarChars-g4') as HTMLInputElement).value).toBe('');
  });

  it('Given a space typed into a row, When it is applied, Then it is dropped', async () => {
    const view = await openSheet();

    fireEvent.change(view.getByTestId('SymbolBarChars-g2'), { target: { value: '& |' } });

    await waitFor(() => expect(useSymbolBarStore.getState().groups[1].chars).toBe('&|'));
  });

  it('Given the classic rail, When Symbols is pressed and then Classic, Then the store follows each choice and the pressed one reads checked', async () => {
    const view = await openSheet();
    expect(view.getByTestId('SymbolBarRailStyle-classic').getAttribute('aria-checked')).toBe('true');

    fireEvent.click(view.getByTestId('SymbolBarRailStyle-symbols'));
    await waitFor(() => expect(useSymbolBarStore.getState().railStyle).toBe('symbols'));
    expect(view.getByTestId('SymbolBarRailStyle-symbols').getAttribute('aria-checked')).toBe('true');
    fireEvent.click(view.getByTestId('SymbolBarRailStyle-classic'));

    await waitFor(() => expect(useSymbolBarStore.getState().railStyle).toBe('classic'));
  });

  it('Given the sheet is open, When Done is pressed, Then the sheet asks to close once and nothing in the store changed', async () => {
    const onOpenChange = vi.fn();
    const view = await openSheet(onOpenChange);

    fireEvent.click(view.getByTestId('SymbolBarDone'));

    expect(onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
    expect(useSymbolBarStore.getState().groups).toEqual(resetGroups());
  });

  it('Given the sheet is closed, When it renders, Then nothing of it is on screen', () => {
    const view = renderScreen(<SymbolBarSheet open={false} onOpenChange={() => {}} />);

    expect(view.queryByTestId('SymbolBarSheetHeader')).toBeNull();
  });
});
