import { clipTo, cutToRange, outline } from '../core/syntax';
import type { TreeSnapshot } from '../ports/host';
import type { Language, Pos, SyntaxNode } from '../ports/protocol';

/**
 * The two tiny parsers behind the fake editor's `tree`: they use Lezer's names for the structural nodes, so `classify` reads their trees.
 * As the real grammar has it, a compound statement ends where its last body ends, newline included. Out of scope on purpose: decorators, match, lambdas, multi-line headers, one-line `if x: y` bodies; the rest is a simple statement.
 */

const node = (type: string, from: Pos, to: Pos, children: readonly SyntaxNode[] = []): SyntaxNode => ({ type, from, to, ...(children.length > 0 ? { children } : {}) });
const damage = (at: Pos): SyntaxNode => ({ type: '⚠', from: at, to: at, error: true });

type Line = { readonly start: Pos; readonly end: Pos; readonly indent: number; readonly text: string; readonly textStart: Pos; readonly textEnd: Pos };

const PYTHON_HEADERS: Readonly<Record<string, string>> = {
  def: 'FunctionDefinition', class: 'ClassDefinition', if: 'IfStatement', while: 'WhileStatement', for: 'ForStatement', try: 'TryStatement', with: 'WithStatement',
};
const PYTHON_CONTINUATIONS = new Set(['elif', 'else', 'except', 'finally']);
const PYTHON_SIMPLE: Readonly<Record<string, string>> = {
  return: 'ReturnStatement', pass: 'PassStatement', break: 'BreakStatement', continue: 'ContinueStatement', raise: 'RaiseStatement', import: 'ImportStatement', from: 'ImportStatement',
  assert: 'AssertStatement', del: 'DeleteStatement', global: 'ScopeStatement', nonlocal: 'ScopeStatement', yield: 'YieldStatement',
};

const linesOf = (text: string): Line[] => {
  const lines: Line[] = [];
  let start = 0;
  for (const raw of text.split('\n')) {
    const end = start + raw.length;
    const body = raw.trimEnd();
    const indent = raw.length - raw.trimStart().length;
    lines.push({ start, end, indent, text: body.trimStart(), textStart: start + indent, textEnd: start + body.length });
    start = end + 1;
  }
  return lines;
};

const keywordOf = (line: string): string => /^[A-Za-z_]+/.exec(line)?.[0] ?? '';

function simpleType(line: string): string {
  const named = PYTHON_SIMPLE[keywordOf(line)];
  if (named) return named;
  if (/^[^=()'"]*[-+*/%&|^]=/.test(line)) return 'UpdateStatement';
  if (/^[^=()'"<>!]*=(?!=)/.test(line)) return 'AssignStatement';
  return 'ExpressionStatement';
}

/** A Python subset by indentation: a header ends in a colon and owns the deeper lines under it; a header with no colon is a statement with an error child. */
export function parseIndented(text: string): SyntaxNode {
  const lines = linesOf(text);
  const afterLine = (line: Line): Pos => Math.min(line.end + 1, text.length);

  const statements = (from: number, blockIndent: number): { nodes: SyntaxNode[]; next: number; lastLine: Line | null } => {
    const nodes: SyntaxNode[] = [];
    let i = from;
    let lastLine: Line | null = null;
    while (i < lines.length) {
      const line = lines[i] as Line;
      if (line.text === '') { i += 1; continue; }
      if (line.indent < blockIndent) break;
      lastLine = line;
      if (line.text.startsWith('#')) { nodes.push(node('Comment', line.textStart, line.textEnd)); i += 1; continue; }
      const keyword = keywordOf(line.text);
      const type = PYTHON_HEADERS[keyword];
      const continues = PYTHON_CONTINUATIONS.has(keyword);
      if (!type && !continues) { nodes.push(node(simpleType(line.text), line.textStart, line.textEnd)); i += 1; continue; }
      if (!line.text.endsWith(':')) {
        nodes.push(node(type ?? 'ExpressionStatement', line.textStart, line.textEnd, [damage(line.textEnd)]));
        i += 1;
        continue;
      }
      const colon = line.textEnd - 1;
      let j = i + 1;
      while (j < lines.length && (lines[j] as Line).text === '') j += 1;
      const deeper = j < lines.length && (lines[j] as Line).indent > line.indent;
      const inner = deeper ? statements(j, (lines[j] as Line).indent) : { nodes: [], next: i + 1, lastLine: null };
      const last = inner.lastLine ?? line;
      const body = node('Body', colon, afterLine(last), inner.nodes.length > 0 ? inner.nodes : [damage(afterLine(last))]);
      const previous = nodes.at(-1);
      if (continues && previous && previous.type !== 'Comment') {
        nodes[nodes.length - 1] = node(previous.type, previous.from, body.to, [...(previous.children ?? []), body]);
      } else {
        nodes.push(node(type ?? 'IfStatement', line.textStart, body.to, [body]));
      }
      lastLine = last;
      i = inner.next;
    }
    return { nodes, next: i, lastLine };
  };

  return node('Script', 0, text.length, statements(0, 0).nodes);
}

type BracedWords = { readonly compound: Readonly<Record<string, string>>; readonly body: string; readonly classBody: string; readonly root: string; readonly callable: string };
const BRACED: Readonly<Record<'javascript' | 'cpp', BracedWords>> = {
  javascript: { compound: { if: 'IfStatement', for: 'ForStatement', while: 'WhileStatement', do: 'DoStatement', switch: 'SwitchStatement', try: 'TryStatement', function: 'FunctionDeclaration', class: 'ClassDeclaration' }, body: 'Block', classBody: 'ClassBody', root: 'Script', callable: 'MethodDeclaration' },
  cpp: { compound: { if: 'IfStatement', for: 'ForStatement', while: 'WhileStatement', do: 'DoStatement', switch: 'SwitchStatement', try: 'TryStatement', class: 'ClassSpecifier', struct: 'StructSpecifier', namespace: 'NamespaceDefinition' }, body: 'CompoundStatement', classBody: 'FieldDeclarationList', root: 'Program', callable: 'FunctionDefinition' },
};
const CPP_TYPES = new Set(['int', 'long', 'short', 'char', 'bool', 'float', 'double', 'auto', 'unsigned', 'signed', 'void', 'size_t', 'string', 'vector', 'const']);
const CONTINUATIONS = new Set(['else', 'catch', 'finally']);

/** Blocks by brace pairs, one simple statement per semicolon; strings and comments are skipped; an unclosed brace gives an error child at the end. */
export function parseBraced(text: string, language: 'javascript' | 'cpp'): SyntaxNode {
  const words = BRACED[language];
  const length = text.length;
  let at = 0;

  const skipTrivia = (): void => {
    for (;;) {
      while (at < length && /\s/.test(text[at] as string)) at += 1;
      if (text.startsWith('//', at)) while (at < length && text[at] !== '\n') at += 1;
      else if (text.startsWith('/*', at)) { const close = text.indexOf('*/', at + 2); at = close < 0 ? length : close + 2; }
      else return;
    }
  };
  const skipString = (): void => {
    const quote = text[at] as string;
    at += 1;
    while (at < length && text[at] !== quote) at += text[at] === '\\' ? 2 : 1;
    at = Math.min(at + 1, length);
  };

  const simpleOf = (from: Pos, to: Pos): SyntaxNode => {
    const source = text.slice(from, to).trim();
    const word = keywordOf(source);
    const named: Record<string, string> = { return: 'ReturnStatement', break: 'BreakStatement', continue: 'ContinueStatement', throw: 'ThrowStatement', goto: 'GotoStatement' };
    const type = named[word] ?? (language === 'javascript' ? (['const', 'let', 'var'].includes(word) ? 'VariableDeclaration' : 'ExpressionStatement') : CPP_TYPES.has(word) ? 'Declaration' : 'ExpressionStatement');
    return node(type, from, to);
  };

  const ownerOf = (header: string, hasParens: boolean, inClass: boolean): string => {
    const word = keywordOf(header);
    const known = words.compound[word];
    if (known) return known;
    if (header.includes('=>')) return 'ArrowFunction';
    if (hasParens && inClass) return words.callable;
    return hasParens ? (language === 'javascript' ? 'FunctionDeclaration' : 'FunctionDefinition') : 'ExpressionStatement';
  };

  /** statements up to the closing brace of the current block (or the end of the text) */
  const statements = (inClass: boolean): SyntaxNode[] => {
    const nodes: SyntaxNode[] = [];
    for (;;) {
      skipTrivia();
      if (at >= length || text[at] === '}') return nodes;
      const from = at;
      let paren = 0;
      let ended = false;
      while (at < length && !ended) {
        const char = text[at] as string;
        if (char === '"' || char === "'" || char === '`') skipString();
        else if (text.startsWith('//', at) || text.startsWith('/*', at)) skipTrivia();
        else if (char === '(') { paren += 1; at += 1; }
        else if (char === ')') { paren -= 1; at += 1; }
        else if (paren <= 0 && (char === ';' || char === '{' || char === '}')) ended = true;
        else at += 1;
      }
      let headerEnd = at;
      while (headerEnd > from && /\s/.test(text[headerEnd - 1] as string)) headerEnd -= 1;
      const header = text.slice(from, headerEnd);
      if (at < length && text[at] === ';') {
        at += 1;
        const word = keywordOf(header);
        const control = words.compound[word];
        if (control && (word === 'if' || word === 'for' || word === 'while')) {
          const open = header.indexOf('(');
          let depth = 0;
          let close = open;
          for (let k = open; k < header.length && open >= 0; k += 1) {
            if (header[k] === '(') depth += 1;
            if (header[k] === ')') { depth -= 1; if (depth === 0) { close = k; break; } }
          }
          const rest = open >= 0 ? header.slice(close + 1) : '';
          nodes.push(rest.trim() ? node(control, from, at, [simpleOf(from + close + 1 + (rest.length - rest.trimStart().length), at)]) : simpleOf(from, at));
        } else nodes.push(simpleOf(from, at));
        continue;
      }
      if (at < length && text[at] === '{') {
        const word = keywordOf(header);
        const classLike = word === 'class' || word === 'struct';
        const open = at;
        at += 1;
        const inner = statements(classLike);
        const closed = at < length && text[at] === '}';
        if (closed) at += 1;
        const blockType = classLike ? words.classBody : words.body;
        const block = node(blockType, open, at, closed ? inner : [...inner, damage(at)]);
        if (CONTINUATIONS.has(word) && nodes.length > 0) {
          const previous = nodes[nodes.length - 1] as SyntaxNode;
          const part = word === 'else' ? block : node(word === 'catch' ? 'CatchClause' : 'FinallyClause', from, at, [block]);
          nodes[nodes.length - 1] = node(previous.type, previous.from, at, [...(previous.children ?? []), part]);
        } else {
          nodes.push(node(ownerOf(header, header.includes('('), inClass), from, at, [block]));
        }
        continue;
      }
      if (at >= length && header.trim()) nodes.push(simpleOf(from, headerEnd));
      if (at < length && text[at] === '}') return nodes;
    }
  };

  const top: SyntaxNode[] = [];
  while (at < length) {
    top.push(...statements(false));
    if (at < length && text[at] === '}') at += 1;
  }
  return node(words.root, 0, length, top);
}

export type TreeParse = (text: string, language: Language) => SyntaxNode;

export const parseFor: TreeParse = (text, language) => (language === 'python' ? parseIndented(text) : parseBraced(text, language));

/** A snapshot over a finished parse that the engine can still pace: `slowBy` calls to `work` are needed before the tree covers the text. */
export function snapshotOf(text: string, language: Language, parse: TreeParse = parseFor, slowBy = 1): TreeSnapshot {
  const root = parse(text, language);
  const step = Math.max(1, Math.ceil(text.length / Math.max(1, slowBy)));
  let reached = 0;
  return {
    language,
    length: text.length,
    work(upto) {
      if (reached < upto) reached = Math.min(text.length, reached + step);
      return reached >= upto;
    },
    parsedTo: () => reached,
    read(range, detail) {
      if (reached === 0 && text.length > 0) return null;
      const shown = reached >= text.length ? root : clipTo(root, reached);
      const sized = detail === 'outline' ? outline(language, shown) : shown;
      return cutToRange(sized, range);
    },
  };
}
