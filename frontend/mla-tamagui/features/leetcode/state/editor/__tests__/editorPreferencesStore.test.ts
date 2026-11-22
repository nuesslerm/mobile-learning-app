import { __resetLoggerConfig, configureLogger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_EDITOR_PREFERENCES } from '../../../domain/editor-settings/editorPreferences';
import { useEditorPreferencesStore } from '../editorPreferencesStore';

const storage = vi.hoisted(() => ({ failRead: false }));

// the real package's web shim reaches for `window`, which this node suite does not have: an in-memory storage stands in for it
vi.mock('@react-native-async-storage/async-storage', () => {
  const store = new Map<string, string>();
  return {
    default: {
      getItem: async (key: string) => {
        if (storage.failRead) throw new Error('storage unavailable');
        return store.get(key) ?? null;
      },
      setItem: async (key: string, value: string) => void store.set(key, value),
      removeItem: async (key: string) => void store.delete(key),
    },
  };
});

type Line = { level: string; event: string; fields: Record<string, unknown> };

function captureLogs() {
  const lines: Line[] = [];
  const chain = (fields: Record<string, unknown>): Record<string, unknown> => {
    const record = (level: string) => (event: string) => void lines.push({ level, event, fields });
    return {
      withContext: () => chain(fields),
      withError: (error: unknown) => chain({ ...fields, error: error instanceof Error ? error.message : error }),
      withMetadata: (more: Record<string, unknown>) => chain({ ...fields, ...more }),
      debug: record('debug'),
      info: record('info'),
      warn: record('warn'),
      error: record('error'),
    };
  };
  configureLogger({ root: (() => chain({})) as unknown as Parameters<typeof configureLogger>[0]['root'], scrub: false });
  return lines;
}

const KEY = 'leetcode-editor-prefs';
const stored = (state: object) => AsyncStorage.setItem(KEY, JSON.stringify({ state, version: 1 }));

beforeEach(async () => {
  storage.failRead = false;
  await AsyncStorage.removeItem(KEY);
  useEditorPreferencesStore.setState(DEFAULT_EDITOR_PREFERENCES);
});
afterEach(() => __resetLoggerConfig());

describe('useEditorPreferencesStore', () => {
  it('Given no stored value, When the store rehydrates, Then the preferences are today\'s editor', async () => {
    await useEditorPreferencesStore.persist.rehydrate();

    const { fontSize, tabSize, wrap, whitespace } = useEditorPreferencesStore.getState();
    expect({ fontSize, tabSize, wrap, whitespace }).toEqual({ fontSize: 14, tabSize: 4, wrap: true, whitespace: false });
  });

  it('Given a person changed a field while nothing was stored, When the store rehydrates, Then the change is kept', async () => {
    useEditorPreferencesStore.setState({ fontSize: 18 });

    await useEditorPreferencesStore.persist.rehydrate();

    expect(useEditorPreferencesStore.getState().fontSize).toBe(18);
  });

  it('Given stored 16, 2, wrap off and whitespace on, When the store rehydrates, Then they hold', async () => {
    await stored({ fontSize: 16, tabSize: 2, wrap: false, whitespace: true });

    await useEditorPreferencesStore.persist.rehydrate();

    const { fontSize, tabSize, wrap, whitespace } = useEditorPreferencesStore.getState();
    expect({ fontSize, tabSize, wrap, whitespace }).toEqual({ fontSize: 16, tabSize: 2, wrap: false, whitespace: true });
  });

  it('Given garbage in one stored field, When the store rehydrates, Then that field alone falls back to its default', async () => {
    await stored({ fontSize: 17, tabSize: 8, wrap: false, whitespace: true });

    await useEditorPreferencesStore.persist.rehydrate();

    const { fontSize, tabSize, wrap, whitespace } = useEditorPreferencesStore.getState();
    expect({ fontSize, tabSize, wrap, whitespace }).toEqual({ fontSize: 14, tabSize: 8, wrap: false, whitespace: true });
  });

  it('Given each setter, When it is called, Then the field changes, one static line names it and the four fields are what is stored', async () => {
    const lines = captureLogs();

    useEditorPreferencesStore.getState().setFontSize(20);
    useEditorPreferencesStore.getState().setTabSize(8);
    useEditorPreferencesStore.getState().setWrap(false);
    useEditorPreferencesStore.getState().setWhitespace(true);

    const { fontSize, tabSize, wrap, whitespace } = useEditorPreferencesStore.getState();
    expect({ fontSize, tabSize, wrap, whitespace }).toEqual({ fontSize: 20, tabSize: 8, wrap: false, whitespace: true });
    expect(lines).toMatchObject([
      { level: 'info', event: 'leetcode.editor_prefs.set', fields: { field: 'fontSize', value: 20 } },
      { level: 'info', event: 'leetcode.editor_prefs.set', fields: { field: 'tabSize', value: 8 } },
      { level: 'info', event: 'leetcode.editor_prefs.set', fields: { field: 'wrap', value: false } },
      { level: 'info', event: 'leetcode.editor_prefs.set', fields: { field: 'whitespace', value: true } },
    ]);
    expect(JSON.parse((await AsyncStorage.getItem(KEY)) ?? 'null')).toEqual({
      state: { fontSize: 20, tabSize: 8, wrap: false, whitespace: true },
      version: 1,
    });
  });

  it('Given a read that fails, When the store rehydrates, Then hydration still ends, the defaults hold and the failure is logged', async () => {
    const lines = captureLogs();
    storage.failRead = true;

    await expect(useEditorPreferencesStore.persist.rehydrate()).resolves.toBeUndefined();

    expect(useEditorPreferencesStore.getState().fontSize).toBe(14);
    expect(lines.filter((line) => line.event === 'leetcode.editor_prefs.rehydrate_failed')).toMatchObject([
      { level: 'error', fields: { error: 'storage unavailable' } },
    ]);
  });
});
