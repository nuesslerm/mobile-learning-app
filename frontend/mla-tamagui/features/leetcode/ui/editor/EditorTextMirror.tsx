import React, { useSyncExternalStore } from 'react';
import { Text } from 'react-native';

export type EditorTextSource = {
  get: () => string;
  /** Sets the text; `notify: false` is for the render pass, before the mirror's first read. */
  set: (text: string, notify?: boolean) => void;
  subscribe: (listener: () => void) => () => void;
};

export function createEditorTextSource(initial: string): EditorTextSource {
  let text = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => text,
    set(next, notify = true) {
      if (next === text) return;
      text = next;
      if (notify) for (const listener of listeners) listener();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/**
 * The document as the page last reported it, on a native node a device test reads at once: the web view's own accessibility
 * text lagged, went stale, and in three hosted runs never appeared. Hidden from screen readers, which read the page.
 */
export function EditorTextMirror({ source }: { source: EditorTextSource }) {
  const text = useSyncExternalStore(source.subscribe, source.get, source.get);
  return (
    <Text
      testID="CodeEditorText"
      accessible={false}
      importantForAccessibility="no"
      pointerEvents="none"
      style={{ position: 'absolute', top: 0, left: 0, width: 1, height: 1, overflow: 'hidden', fontSize: 1, color: 'transparent' }}
    >
      {text}
    </Text>
  );
}
