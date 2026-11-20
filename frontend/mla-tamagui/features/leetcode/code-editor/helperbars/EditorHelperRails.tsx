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
  onDismissKeyboard: () => void;
  voiceButton?: React.ReactNode;
};

export function EditorHelperRails({
  editorRef,
  source,
  selection,
  onConfigure,
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
      />
      <ActionToolbar
        editorRef={editorRef}
        source={source}
        selection={selection}
        onDone={onDismissKeyboard}
      >
        {voiceButton}
      </ActionToolbar>
    </>
  );
}
