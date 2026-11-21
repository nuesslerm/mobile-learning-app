import type { Language, SyntaxNode } from '../../ports/protocol';

export type NodeClass = 'root' | 'block' | 'compound' | 'simple' | 'clause' | 'comment' | 'error' | 'other';
export type SyntaxFamily = 'indent' | 'braces';
type Named = Exclude<NodeClass, 'error' | 'other'>;

const CONTROL = ['IfStatement', 'SwitchStatement', 'DoStatement', 'WhileStatement', 'ForStatement', 'TryStatement'];

/** Lezer's node names, by what they are for editing; `syntaxTables.test.ts` checks every name against the real grammar. A name is never matched by pattern: `Definition` ends a statement in Python and an identifier in JavaScript. */
export const SYNTAX_TABLES: Readonly<Record<Language, Readonly<Record<Named, readonly string[]>>>> = {
  python: {
    root: ['Script'],
    block: ['Body', 'MatchBody'],
    compound: ['IfStatement', 'WhileStatement', 'ForStatement', 'TryStatement', 'WithStatement', 'MatchStatement', 'FunctionDefinition', 'ClassDefinition', 'DecoratedStatement'],
    clause: ['MatchClause'],
    simple: ['AssignStatement', 'UpdateStatement', 'ExpressionStatement', 'DeleteStatement', 'PassStatement', 'BreakStatement', 'ContinueStatement', 'ReturnStatement', 'YieldStatement', 'PrintStatement', 'RaiseStatement', 'ImportStatement', 'ScopeStatement', 'AssertStatement', 'TypeDefinition'],
    comment: ['Comment'],
  },
  javascript: {
    root: ['Script'],
    block: ['Block', 'ClassBody', 'SwitchBody', 'StaticBlock'],
    compound: [...CONTROL, 'FunctionDeclaration', 'FunctionExpression', 'ClassDeclaration', 'ClassExpression', 'MethodDeclaration', 'ArrowFunction', 'WithStatement', 'LabeledStatement'],
    clause: ['CatchClause', 'FinallyClause', 'CaseLabel', 'DefaultLabel'],
    simple: ['ExpressionStatement', 'VariableDeclaration', 'ReturnStatement', 'BreakStatement', 'ContinueStatement', 'ThrowStatement', 'DebuggerStatement', 'ImportDeclaration'],
    comment: ['LineComment', 'BlockComment'],
  },
  cpp: {
    root: ['Program'],
    block: ['CompoundStatement', 'FieldDeclarationList'],
    compound: [...CONTROL, 'ForRangeLoop', 'FunctionDefinition', 'LambdaExpression', 'ClassSpecifier', 'StructSpecifier', 'UnionSpecifier', 'NamespaceDefinition', 'LabeledStatement'],
    clause: ['CatchClause', 'CaseStatement'],
    simple: ['ExpressionStatement', 'Declaration', 'ReturnStatement', 'BreakStatement', 'ContinueStatement', 'GotoStatement', 'ThrowStatement'],
    comment: ['LineComment', 'BlockComment'],
  },
};

const ERROR_TYPE = '⚠';

const INDEX: Readonly<Record<Language, ReadonlyMap<string, Named>>> = {
  python: indexOf('python'),
  javascript: indexOf('javascript'),
  cpp: indexOf('cpp'),
};

function indexOf(language: Language): ReadonlyMap<string, Named> {
  const index = new Map<string, Named>();
  for (const [name, types] of Object.entries(SYNTAX_TABLES[language]) as [Named, readonly string[]][]) for (const type of types) index.set(type, name);
  return index;
}

export function classify(language: Language, node: Pick<SyntaxNode, 'type' | 'error'>): NodeClass {
  if (node.error || node.type === ERROR_TYPE) return 'error';
  return INDEX[language].get(node.type) ?? 'other';
}

export const familyOf = (language: Language): SyntaxFamily => (language === 'python' ? 'indent' : 'braces');

/** The outline keeps the nodes that mean something to editing; an expression or a list is not one. */
export const isOutline = (language: Language, node: Pick<SyntaxNode, 'type' | 'error'>): boolean => classify(language, node) !== 'other';

/** The tree with every node `isOutline` rejects removed: its children attach to the nearest node that is kept. */
export function outline(language: Language, root: SyntaxNode): SyntaxNode {
  const below = (node: SyntaxNode): SyntaxNode[] => (node.children ?? []).flatMap((child) => (isOutline(language, child) ? [keep(child)] : below(child)));
  const keep = (node: SyntaxNode): SyntaxNode => {
    const children = below(node);
    return { type: node.type, from: node.from, to: node.to, ...(node.error ? { error: true as const } : {}), ...(children.length > 0 ? { children } : {}) };
  };
  return keep(root);
}
