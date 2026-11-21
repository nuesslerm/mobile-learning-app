import type { Completion, CompletionContext, CompletionResult, CompletionSource } from '@codemirror/autocomplete';
import { globalCompletion, localCompletionSource } from '@codemirror/lang-python';
import { syntaxTree } from '@codemirror/language';
import type { CompletionEntry, Language } from '@lib/code-editor-api';

export type CompletionSourceOptions = { readonly language: Language; readonly entries: readonly CompletionEntry[] };

const KEYWORDS: Readonly<Record<Language, readonly string[]>> = {
  python: ['and', 'as', 'assert', 'async', 'await', 'break', 'class', 'continue', 'def', 'del', 'elif', 'else', 'except', 'finally', 'for', 'from', 'global', 'if', 'import', 'in', 'is', 'lambda', 'nonlocal', 'not', 'or', 'pass', 'raise', 'return', 'try', 'while', 'with', 'yield'],
  javascript: ['async', 'await', 'break', 'case', 'catch', 'class', 'const', 'continue', 'debugger', 'default', 'delete', 'do', 'else', 'export', 'extends', 'false', 'finally', 'for', 'function', 'if', 'import', 'in', 'instanceof', 'let', 'new', 'null', 'of', 'return', 'super', 'switch', 'this', 'throw', 'true', 'try', 'typeof', 'undefined', 'var', 'void', 'while', 'with', 'yield'],
  cpp: ['auto', 'bool', 'break', 'case', 'catch', 'char', 'class', 'const', 'constexpr', 'continue', 'default', 'delete', 'do', 'double', 'else', 'enum', 'explicit', 'extern', 'false', 'float', 'for', 'friend', 'goto', 'if', 'inline', 'int', 'long', 'namespace', 'new', 'nullptr', 'operator', 'private', 'protected', 'public', 'return', 'short', 'signed', 'sizeof', 'static', 'struct', 'switch', 'template', 'this', 'throw', 'true', 'try', 'typedef', 'typename', 'union', 'unsigned', 'using', 'virtual', 'void', 'volatile', 'while'],
};

// the grammar's names for text that is not code: nothing is offered inside them
const NOT_CODE: Readonly<Record<Language, ReadonlySet<string>>> = {
  python: new Set(['String', 'FormatString', 'Comment']),
  javascript: new Set(['String', 'TemplateString', 'Comment', 'LineComment', 'BlockComment', 'RegExp']),
  cpp: new Set(['String', 'RawString', 'CharLiteral', 'LineComment', 'BlockComment']),
};

// what a person reaches for first while solving a problem, ranked above the rest of the alphabet
const BOOSTED = new Set(['range', 'len', 'enumerate', 'sorted', 'print', 'min', 'max', 'sum', 'return', 'True', 'False', 'None']);
const HIDDEN_BUILTINS = new Set(['exit', 'quit', 'license', 'help', 'copyright', 'credits']);

const BOOST = { entry: 40, local: 20, curated: 10, plain: 0 } as const;
const clamp = (boost: number): number => Math.max(-99, Math.min(99, boost));

function builtinsOf(context: CompletionContext, prefix: string): Completion[] {
  const all = (globalCompletion(context) as CompletionResult | null)?.options ?? [];
  const underscored = prefix.startsWith('_');
  return all
    .filter((option) => typeof option.apply !== 'function') // the snippets: they need Tab between fields, which a phone does not have
    .filter((option) => !HIDDEN_BUILTINS.has(option.label) && (underscored || !/^__/.test(option.label)));
}

function memberNames(text: string, typing: string): Completion[] {
  const seen = new Set<string>();
  for (const match of text.matchAll(/\.([A-Za-z_]\w+)/g)) {
    const name = match[1] as string;
    if (name !== typing) seen.add(name);
  }
  return [...seen].map((label) => ({ label, type: 'property' }));
}

/** One merged source for the editor's completion: the problem's own names first, then the names in scope, the language's keywords and, in Python, its builtins. Pure over the state it is given. */
export function createCompletionSource({ language, entries }: CompletionSourceOptions): CompletionSource {
  const priority: Completion[] = entries.map((entry) => ({
    label: entry.label,
    type: entry.type ?? 'variable',
    ...(entry.detail === undefined ? {} : { detail: entry.detail }),
    boost: clamp(BOOST.entry + (entry.boost ?? 0)),
  }));
  const keywords: Completion[] = KEYWORDS[language].map((label) => ({ label, type: 'keyword', boost: BOOSTED.has(label) ? BOOST.curated : BOOST.plain }));

  return (context) => {
    const word = context.matchBefore(/[A-Za-z_]\w*/);
    if (!word && !context.explicit) return null;
    const typing = word?.text ?? '';
    if (typing.length < 2 && !context.explicit) return null;
    const inside = syntaxTree(context.state).resolveInner(context.pos, -1).name;
    if (NOT_CODE[language].has(inside)) return null;
    const from = word?.from ?? context.pos;

    const afterDot = from > 0 && context.state.sliceDoc(from - 1, from) === '.';
    const found: Completion[] = afterDot
      ? memberNames(context.state.doc.toString(), typing)
      : [
          ...priority,
          ...(language === 'python' ? (localCompletionSource(context)?.options ?? []).map((option) => ({ ...option, boost: BOOST.local })) : []),
          ...keywords,
          ...(language === 'python' ? builtinsOf(context, typing).map((option) => ({ ...option, boost: BOOSTED.has(option.label) ? BOOST.curated : BOOST.plain })) : []),
        ];

    const unique = new Map<string, Completion>();
    for (const option of found) if (!unique.has(option.label)) unique.set(option.label, option);
    return unique.size === 0 ? null : { from, options: [...unique.values()], validFor: /^\w*$/ };
  };
}
