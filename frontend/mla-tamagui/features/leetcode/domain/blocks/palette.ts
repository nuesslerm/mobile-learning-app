export type PaletteItemId = 'if' | 'for' | 'while' | 'def' | 'return';
export type PaletteItem = { readonly id: PaletteItemId; readonly label: string; readonly snippet: string };

/** What a chip inserts: one statement, with a `pass` body where it has one and plain-text placeholders the person edits in the editor. The engine gives the lines the indent of the slot. */
export const PYTHON_PALETTE: readonly PaletteItem[] = [
  { id: 'if', label: 'if', snippet: 'if condition:\n    pass' },
  { id: 'for', label: 'for', snippet: 'for item in items:\n    pass' },
  { id: 'while', label: 'while', snippet: 'while condition:\n    pass' },
  { id: 'def', label: 'def', snippet: 'def name():\n    pass' },
  { id: 'return', label: 'return', snippet: 'return' },
];

export const paletteItem = (id: PaletteItemId): PaletteItem | undefined => PYTHON_PALETTE.find((item) => item.id === id);
