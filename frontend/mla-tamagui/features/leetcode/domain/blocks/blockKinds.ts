export type BlockKind = 'if' | 'for' | 'while' | 'def' | 'class' | 'try' | 'with' | 'return' | 'statement' | 'clause' | 'text';

/** Lezer's Python node names that get a label of their own; every other statement is a plain `statement` (a decorated function and a match are leaves). `blockKinds.test.ts` checks each name against the library's tables. */
export const PYTHON_BLOCK_KINDS: ReadonlyMap<string, BlockKind> = new Map<string, BlockKind>([
  ['IfStatement', 'if'],
  ['ForStatement', 'for'],
  ['WhileStatement', 'while'],
  ['FunctionDefinition', 'def'],
  ['ClassDefinition', 'class'],
  ['TryStatement', 'try'],
  ['WithStatement', 'with'],
  ['ReturnStatement', 'return'],
]);

export const kindOfNode = (type: string): BlockKind => PYTHON_BLOCK_KINDS.get(type) ?? 'statement';
