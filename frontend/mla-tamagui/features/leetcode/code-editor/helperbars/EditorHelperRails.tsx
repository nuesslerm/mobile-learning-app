import React from 'react';

import type { EditorInputHandle } from '../core/EditorInput';
import type { Selection } from '../core/types';

import { ActionToolbar } from './ActionToolbar';
import { SyntaxToolbar } from './SyntaxToolbar';

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
  source: string;
  selection: Selection;
  onConfigure: () => void;
  onRenameSymbol?: (symbolName: string) => void;
  onDismissKeyboard: () => void;
  voiceButton?: React.ReactNode;
};

export function EditorHelperRails({
  editorRef,
  source,
  selection,
  onConfigure,
  onRenameSymbol,
  onDismissKeyboard,
  voiceButton,
}: Props) {
  return (
    <>
      <SyntaxToolbar
        editorRef={editorRef}
        source={source}
        selection={selection}
        onConfigure={onConfigure}
        onRenameSymbol={onRenameSymbol}
      />
      <ActionToolbar
        editorRef={editorRef}
        source={source}
        selection={selection}
        onRenameSymbol={onRenameSymbol}
        onDone={onDismissKeyboard}
      >
        {voiceButton}
      </ActionToolbar>
    </>
  );
}
