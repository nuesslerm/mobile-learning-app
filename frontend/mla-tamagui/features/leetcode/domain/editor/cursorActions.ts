import { lineColumn, type Motion } from './cursorMotion';
import type { Selection } from './types';

export type CursorAction = { readonly name: string; readonly label: string };

const TABLE: readonly (CursorAction & { readonly motion: Motion })[] = [
  { name: 'increment', label: 'Move one character right', motion: { unit: 'char', count: 1 } },
  { name: 'decrement', label: 'Move one character left', motion: { unit: 'char', count: -1 } },
  { name: 'wordRight', label: 'Move one word right', motion: { unit: 'word', count: 1 } },
  { name: 'wordLeft', label: 'Move one word left', motion: { unit: 'word', count: -1 } },
  { name: 'lineDown', label: 'Move one line down', motion: { unit: 'line', count: 1 } },
  { name: 'lineUp', label: 'Move one line up', motion: { unit: 'line', count: -1 } },
];

/** What a screen reader is offered on the handle: increment and decrement are the adjustable role's own, the rest are named. */
export const CURSOR_ACTIONS: readonly CursorAction[] = TABLE.map(({ name, label }) => ({ name, label }));

/** The caret motion an accessibility action stands for, or null for an action the handle does not own. */
export function motionOfAction(name: string): Motion | null {
  return TABLE.find((action) => action.name === name)?.motion ?? null;
}

const place = (text: string, pos: number): string => {
  const { line, column } = lineColumn(text, pos);
  return `line ${line}, column ${column}`;
};

/** The handle's value for a screen reader: where the caret is, or where a selection runs from and to, by line and column. */
export function caretValueText(text: string, selection: Selection): string {
  if (selection.start !== selection.end) return `Selected from ${place(text, selection.start)} to ${place(text, selection.end)}`;
  const { line, column } = lineColumn(text, selection.end);
  return `Line ${line}, column ${column}`;
}
