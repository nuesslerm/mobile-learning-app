import { Logger } from '@lib/logging';
import {
  KeyboardToolbarButton,
  KeyboardToolbarRail,
} from '@lib/tamagui-design-system';
import * as Clipboard from 'expo-clipboard';
import {
  Check,
  ClipboardPaste,
  Copy,
  Pencil,
  RotateCcw,
  RotateCw,
} from '@lib/tamagui-design-system/icons';
import React, { useCallback } from 'react';
import { Keyboard } from 'react-native';

import type { EditorInputHandle } from '../core/EditorInput';
import { getWordAtSelection } from '../core/editing';
import type { Selection } from '../core/types';

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
  source: string;
  selection: Selection;
  onRenameSymbol?: (symbolName: string) => void;
  onDone: () => void;
  children?: React.ReactNode;
};

export function ActionToolbar({
  editorRef,
  source,
  selection,
  onRenameSymbol,
  onDone,
  children,
}: Props) {
  const selectedText =
    selection.start === selection.end
      ? ''
      : source.slice(
          Math.min(selection.start, selection.end),
          Math.max(selection.start, selection.end)
        );
  const renameTarget = getWordAtSelection(source, selection);
  const hasSelection = selectedText.length > 0;

  const refocusEditor = useCallback(() => {
    Logger.info('toolbar.refocus', { action: 'refocus' });
    editorRef.current?.focus();
  }, [editorRef]);

  const handleCopy = useCallback(async () => {
    Logger.info('toolbar.copy_press', { selectedLength: selectedText.length });
    if (!selectedText) {
      refocusEditor();
      return;
    }
    try {
      await Clipboard.setStringAsync(selectedText);
      Logger.info('toolbar.copy_done', { selectedLength: selectedText.length });
    } catch (err) {
      Logger.withError(err).warn('toolbar.copy_error');
    }
    refocusEditor();
  }, [refocusEditor, selectedText]);

  const handlePaste = useCallback(async () => {
    Logger.info('toolbar.paste_press', { sourceLength: source.length });
    try {
      const text = await Clipboard.getStringAsync();
      if (text) {
        editorRef.current?.insertAtCursor(text);
      }
      Logger.info('toolbar.paste_done', { pastedLength: text.length });
    } catch (err) {
      Logger.withError(err).warn('toolbar.paste_error');
    }
    refocusEditor();
  }, [editorRef, refocusEditor, source]);

  const handleRename = useCallback(() => {
    Logger.info('toolbar.rename_press', { hasTarget: renameTarget != null });
    if (renameTarget) {
      onRenameSymbol?.(renameTarget);
    }
  }, [onRenameSymbol, renameTarget]);

  return (
    <KeyboardToolbarRail testID="EditorActionToolbar">
      <KeyboardToolbarButton
        testID="EditorAction-Undo"
        accessibilityLabel="Undo"
        label="Undo"
        icon={RotateCcw}
        onPress={() => {
          Logger.info('toolbar.undo_press', { action: 'undo' });
          editorRef.current?.undo();
          refocusEditor();
        }}
      />
      <KeyboardToolbarButton
        testID="EditorAction-Redo"
        accessibilityLabel="Redo"
        label="Redo"
        icon={RotateCw}
        onPress={() => {
          Logger.info('toolbar.redo_press', { action: 'redo' });
          editorRef.current?.redo();
          refocusEditor();
        }}
      />
      <KeyboardToolbarButton
        testID="EditorAction-Copy"
        accessibilityLabel="Copy selected code"
        label="Copy"
        icon={Copy}
        disabled={!hasSelection}
        onPress={() => {
          Logger.info('toolbar.copy_tap', { hasSelection });
          void handleCopy();
        }}
      />
      <KeyboardToolbarButton
        testID="EditorAction-Paste"
        accessibilityLabel="Paste code"
        label="Paste"
        icon={ClipboardPaste}
        tone="success"
        onPress={() => {
          Logger.info('toolbar.paste_tap', { sourceLength: source.length });
          void handlePaste();
        }}
      />
      <KeyboardToolbarButton
        testID="EditorAction-Rename"
        accessibilityLabel="Rename symbol"
        label="Rename"
        icon={Pencil}
        tone="accent"
        disabled={!renameTarget}
        onPress={handleRename}
      />
      <KeyboardToolbarButton
        testID="EditorAction-Done"
        accessibilityLabel="Done editing"
        label="Done"
        icon={Check}
        tone="accent"
        onPress={() => {
          Logger.info('toolbar.done_press', { sourceLength: source.length });
          Keyboard.dismiss();
          onDone();
        }}
      />
      {children}
    </KeyboardToolbarRail>
  );
}
