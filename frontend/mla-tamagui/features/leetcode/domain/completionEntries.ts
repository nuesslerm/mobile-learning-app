import type { Blind75Problem } from './models';

/** A name the editor may offer while the problem is open; the same shape the editor's completion config takes, declared here so the domain stays free of the editor library. */
export type SignatureEntry = { readonly label: string; readonly type?: string; readonly detail?: string };
export type StarterLanguage = 'python' | 'javascript' | 'cpp';

const STARTER_KEY: Readonly<Record<StarterLanguage, string>> = { python: 'python3', javascript: 'javascript', cpp: 'cpp' };
const WORD = /^[A-Za-z_]\w*$/;
const PYTHON_CONSTANTS = new Set(['None', 'True', 'False']);
const MAX_ENTRIES = 200;

const OPENERS = '([{';
const CLOSERS = ')]}';

/** The text between the parenthesis that opens at `open` and its match, or null when it never closes. */
function parenthesised(text: string, open: number): string | null {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    const char = text[i] as string;
    if (char === '(') depth += 1;
    if (char === ')') {
      depth -= 1;
      if (depth === 0) return text.slice(open + 1, i);
    }
  }
  return null;
}

/** Splits at the commas that are not inside brackets (or, with `angles`, inside template arguments). */
function splitTopLevel(list: string, angles: boolean): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < list.length; i += 1) {
    const char = list[i] as string;
    if (OPENERS.includes(char) || (angles && char === '<')) depth += 1;
    else if (CLOSERS.includes(char) || (angles && char === '>')) depth -= 1;
    else if (char === ',' && depth === 0) {
      parts.push(list.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(list.slice(start));
  return parts.map((part) => part.trim()).filter((part) => part !== '');
}

type Parameter = { readonly name: string; readonly annotation?: string };

const callable = (label: string, parameters: readonly Parameter[]): SignatureEntry => ({
  label,
  type: 'function',
  detail: `(${parameters.map((parameter) => parameter.name).filter((name) => name !== 'self').join(', ')})`,
});

function pythonEntries(starter: string, method: string): SignatureEntry[] {
  // the method's own line: the first `def` of the starter can be the commented `__init__` of a linked-list problem
  const at = new RegExp(`^[ \\t]*def ${method}\\(`, 'm').exec(starter);
  if (!at) return [];
  const open = at.index + at[0].length - 1;
  const list = parenthesised(starter, open);
  if (list === null) return [];
  const header = starter.slice(open, starter.indexOf('\n', open) < 0 ? undefined : starter.indexOf('\n', open));
  const parameters: Parameter[] = splitTopLevel(list, false).flatMap((part) => {
    const name = /^\*{0,2}([A-Za-z_]\w*)/.exec(part)?.[1];
    if (!name) return [];
    const annotation = /^[^:=]*:\s*([^=]+?)\s*(?:=.*)?$/.exec(part)?.[1];
    return [{ name, ...(annotation ? { annotation } : {}) }];
  });
  const types = [...new Set(header.match(/\b[A-Z]\w*/g) ?? [])].filter((name) => !PYTHON_CONSTANTS.has(name));
  return [
    callable(method, parameters),
    ...parameters.map((parameter): SignatureEntry => ({ label: parameter.name, type: 'variable', ...(parameter.annotation ? { detail: parameter.annotation } : {}) })),
    ...types.map((label): SignatureEntry => ({ label, type: 'class' })),
  ];
}

function bracedEntries(starter: string, method: string, language: 'javascript' | 'cpp'): SignatureEntry[] {
  const at = new RegExp(`\\b${method}\\s*(?:=\\s*(?:function\\s*)?)?\\(`).exec(starter);
  if (!at) return [];
  const list = parenthesised(starter, at.index + at[0].length - 1);
  if (list === null) return [];
  const parameters: Parameter[] = splitTopLevel(list, language === 'cpp').flatMap((part) => {
    const bare = part.replace(/\s*=.*$/, '').trim();
    const name = /([A-Za-z_]\w*)\s*(?:\[\])*$/.exec(bare)?.[1];
    if (!name) return [];
    const annotation = language === 'cpp' ? bare.slice(0, bare.length - name.length).replace(/[\s&*]+$/, '').trim() : '';
    return [{ name, ...(annotation ? { annotation } : {}) }];
  });
  return [callable(method, parameters), ...parameters.map((parameter): SignatureEntry => ({ label: parameter.name, type: 'variable', ...(parameter.annotation ? { detail: parameter.annotation } : {}) }))];
}

/** The names a problem's starter introduces, to offer them while the person types: the method, its parameters and the types its signature uses. Empty when the problem has no starter in the language. */
export function completionEntriesFor(problem: Pick<Blind75Problem, 'starterCode' | 'methodName'>, language: StarterLanguage): SignatureEntry[] {
  const starter = problem.starterCode[STARTER_KEY[language]];
  const method = problem.methodName;
  if (!starter || !method || !WORD.test(method)) return [];
  const found = language === 'python' ? pythonEntries(starter, method) : bracedEntries(starter, method, language);
  const unique = new Map<string, SignatureEntry>();
  for (const entry of found) if (WORD.test(entry.label) && entry.label.length <= 64 && !unique.has(entry.label)) unique.set(entry.label, entry);
  return [...unique.values()].slice(0, MAX_ENTRIES);
}
