import { useEffect, useRef, useState } from 'react';
import { Platform } from 'react-native';

import type { ComposerProps, ComposerSuggestion, ComposerSuggestions } from '../ports/ChatComponents';

/** The `@file`, `/command` or `$skill` the caret is in, if any. */
export function suggestionTrigger(text: string, caret: number) {
  const before = text.slice(0, caret);
  const match =
    before.match(/(?:^|\s)(@)(\S*)$/) ??
    before.match(/(?:^|\s)([/$])([^\s/@$]*)$/);
  if (!match) return null;
  return {
    sigil: match[1],
    query: match[2],
    start: caret - match[2].length - 1,
  };
}

export function fileToken(path: string) {
  if (!/[\s@]/.test(path)) return '@' + path;
  if (path.includes('"') && !path.includes("'")) return "@'" + path + "'";
  return '@"' + path.replaceAll('"', '\\"') + '"';
}

type Field = { focus(): void; blur(): void };
type Selection = { start: number; end: number };

/** What a composer does with its text, whatever it looks like: where the caret is, which suggestions it asks for, and how a pick lands in the text. */
export function useComposerEditing<F extends Field>({ value, notice, onChangeText, onPickSuggestion }: Pick<ComposerProps, 'value' | 'notice' | 'onChangeText' | 'onPickSuggestion'>) {
  const field = useRef<F>(null);
  useEffect(() => {
    if (notice && Platform.OS === 'ios') field.current?.blur();
  }, [notice]);
  const refocus = useRef(false);
  const [caret, setCaret] = useState(value.length);
  const [selection, setSelection] = useState<Selection>();
  const [kind, setKind] = useState<ComposerSuggestion['kind'] | null>(null);
  const end = Math.min(caret, value.length);
  const trigger = suggestionTrigger(value, end);
  function insert(text: string, position: number) {
    onChangeText(text);
    setCaret(position);
    setSelection({ start: position, end: position });
  }
  return {
    field,
    selection,
    trigger,
    kind,
    selectionChanged(next: Selection) {
      setCaret(next.end);
      setSelection(undefined);
    },
    textChanged(text: string) {
      if (!text.startsWith('/')) setKind(null);
      onChangeText(text);
    },
    /** The add menu chose a kind: its sigil goes in at the caret. */
    open(chosen: ComposerSuggestion['kind']) {
      setKind(chosen);
      if (chosen === 'file') {
        const before = value.slice(0, end);
        const lead = before && !/\s$/.test(before) ? ' @' : '@';
        insert(before + lead + value.slice(end), end + lead.length);
      } else {
        insert(value.trimStart() ? '/ ' + value.trimStart() : '/', 1);
      }
      if (Platform.OS === 'ios') refocus.current = chosen !== 'command';
      else field.current?.focus();
    },
    pick(row: ComposerSuggestion) {
      if (!trigger || onPickSuggestion?.(row, !value.slice(0, trigger.start).trim())) return;
      const token = row.kind === 'file' ? fileToken(row.token) : row.token;
      const before = value.slice(0, trigger.start) + token + ' ';
      insert(before + value.slice(end).replace(/^ /, ''), before.length);
      setKind(null);
      field.current?.focus();
    },
    /** The add menu left the screen: the field takes the focus back where it gave it up. */
    addDismissed() {
      if (refocus.current) field.current?.focus();
      refocus.current = false;
    },
  };
}

export const SUGGESTION_GROUPS = ['file', 'command', 'skill'] as const;
export const SUGGESTION_HEADINGS = { file: 'Files', command: 'Commands', skill: 'Skills' } as const;
export const ADD_CHOICES = [
  { kind: 'file', label: 'Workspace file' },
  { kind: 'skill', label: 'Skills' },
  { kind: 'command', label: 'Commands' },
] as const;

/** The rows a suggestion list shows for what was typed, by group, with the state of the load behind them. */
export function useComposerSuggestionRows(source: ComposerSuggestions, sigil: string, query: string, kind: ComposerSuggestion['kind'] | null) {
  const [entries, setEntries] = useState<readonly ComposerSuggestion[]>([]);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const files = sigil === '@';
  const requestedQuery = files ? query : '';
  useEffect(() => {
    let active = true;
    setEntries([]);
    setError(false);
    const timer = setTimeout(
      () => {
        const load = files ? source.files(requestedQuery) : source.catalog();
        void load.then(
          (rows) => {
            if (active) setEntries(rows);
          },
          () => {
            if (active) setError(true);
          }
        );
      },
      files && requestedQuery ? 120 : 0
    );
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [source, files, requestedQuery, attempt]);
  const commands = source.commands ?? [];
  const catalog = [
    ...commands,
    ...entries.filter((row) => !commands.some((command) => command.token === row.token && row.kind === 'command')),
  ];
  const matches = files
    ? entries
    : catalog.filter((row) => [row.token, ...(row.aliases ?? [])].some((name) => name.toLowerCase().includes(query.toLowerCase())));
  const narrowed = matches.filter((row) => row.kind === (sigil === '$' ? 'skill' : kind));
  const shown = narrowed.length || sigil === '$' ? narrowed : matches;
  return {
    error,
    empty: !error && !shown.length,
    retry: () => setAttempt((n) => n + 1),
    groups: SUGGESTION_GROUPS.map((group) => ({ group, heading: SUGGESTION_HEADINGS[group], rows: shown.filter((row) => row.kind === group).slice(0, 10) })).filter((entry) => entry.rows.length > 0),
  };
}
