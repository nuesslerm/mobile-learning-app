import {
  DEFAULT_TOOLBAR_ITEMS,
  toolbarConfigSchema,
  toolbarItemKey,
  type BuiltinKeyId,
  type ToolbarItem,
} from '../domain';
import { Logger } from '@lib/logging';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
/**
 * Persisted configuration for the coding keyboard bar: which keys are shown and
 * in what order. Mirrors the persist/AsyncStorage pattern of `authStore.ts`.
 *
 * The persisted `items` are validated against the domain schema on rehydrate, so
 * corrupt or out-of-date storage falls back to the default toolbar instead of
 * breaking the bar.
 */
type AddCustomInput = {
  label: string;
  insert: string;
  cursorOffset?: number;
};
type ToolbarConfigState = {
  items: ToolbarItem[];
  setItems: (items: ToolbarItem[]) => void;
  move: (from: number, to: number) => void;
  removeKey: (key: string) => void;
  addBuiltin: (id: BuiltinKeyId) => void;
  addCustom: (input: AddCustomInput) => void;
  resetToDefault: () => void;
};
function reorder(
  items: ToolbarItem[],
  from: number,
  to: number
): ToolbarItem[] {
  if (
    from === to ||
    from < 0 ||
    to < 0 ||
    from >= items.length ||
    to >= items.length
  ) {
    return items;
  }
  const next = items.slice();
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}
function createCustomId(): string {
  return `c${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}
const persistedSchema = z.object({
  items: toolbarConfigSchema,
});
export const useToolbarConfigStore = create<ToolbarConfigState>()(
  persist(
    (set) => ({
      items: DEFAULT_TOOLBAR_ITEMS,
      setItems: (items) => {
        Logger.info('toolbar.set_items', {
          count: items.length,
        });
        set({
          items,
        });
      },
      move: (from, to) => {
        Logger.info('toolbar.move', {
          from,
          to,
        });
        set((state) => ({
          items: reorder(state.items, from, to),
        }));
      },
      removeKey: (key) => {
        Logger.info('toolbar.remove_key', {
          key,
        });
        set((state) => ({
          items: state.items.filter((item) => toolbarItemKey(item) !== key),
        }));
      },
      addBuiltin: (id) => {
        Logger.info('toolbar.add_builtin', {
          id,
        });
        set((state) =>
          state.items.some((item) => item.kind === 'builtin' && item.id === id)
            ? state
            : {
                items: [
                  ...state.items,
                  {
                    kind: 'builtin',
                    id,
                  },
                ],
              }
        );
      },
      addCustom: ({ label, insert, cursorOffset }) => {
        Logger.info('toolbar.add_custom', {
          labelLength: label.length,
          insertLength: insert.length,
          cursorOffset,
        });
        set((state) => ({
          items: [
            ...state.items,
            {
              kind: 'custom',
              id: createCustomId(),
              label,
              insert,
              ...(cursorOffset == null
                ? {}
                : {
                    cursorOffset,
                  }),
            },
          ],
        }));
      },
      resetToDefault: () => {
        Logger.info('toolbar.reset_to_default', {
          count: DEFAULT_TOOLBAR_ITEMS.length,
        });
        set({
          items: DEFAULT_TOOLBAR_ITEMS,
        });
      },
    }),
    {
      name: 'leetcode-toolbar-config',
      version: 1,
      storage: createJSONStorage(() => AsyncStorage),
      partialize: (state) => ({
        items: state.items,
      }),
      // Validate persisted items; fall back to defaults on any mismatch.
      merge: (persisted, current) => {
        const parsed = persistedSchema.safeParse(persisted);
        if (!parsed.success && persisted != null) {
          Logger.warn('leetcode.toolbar.rehydrate_rejected', {
            issues: parsed.error.issues.length,
          });
        }
        return {
          ...current,
          items:
            parsed.success && parsed.data.items.length > 0
              ? parsed.data.items
              : current.items,
        };
      },
      onRehydrateStorage: () => (_state, err) => {
        if (err)
          Logger.withError(err).error('leetcode.toolbar.rehydrate_failed');
      },
    }
  )
);
