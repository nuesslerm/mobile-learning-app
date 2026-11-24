import type { Language } from '@lib/code-editor-api';
import React from 'react';

import type { EditorInputHandle } from '../EditorInput';
import type { Selection } from '../../../domain/editor/types';
import { useSymbolBarStore } from '../../../state/editor/symbolBarStore';

import { ActionToolbar } from './ActionToolbar';
import { SymbolBarRail } from './SymbolBarRail';
import { SyntaxToolbar } from './SyntaxToolbar';

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
  source: string;
  selection: Selection;
  language: Language;
  onConfigure: () => void;
  onDismissKeyboard: () => void;
  voiceButton?: React.ReactNode;
  /** A row of its own above the symbol toolbar (the console's action bar). */
  topRow?: React.ReactNode;
  compactSymbols?: boolean;
};

export function EditorHelperRails({
  editorRef,
  source,
  selection,
  language,
  onConfigure,
  onDismissKeyboard,
  voiceButton,
  topRow,
  compactSymbols = false,
}: Props) {
  const shown = useSymbolBarStore((s) => s.shown);
  const railStyle = useSymbolBarStore((s) => s.railStyle);

  return (
    <>
      {topRow}
      {!shown ? null : railStyle === 'symbols' ? (
        <SymbolBarRail
          editorRef={editorRef}
          source={source}
          selection={selection}
          language={language}
          onDismissKeyboard={onDismissKeyboard}
          compact={compactSymbols}
        />
      ) : (
        <SyntaxToolbar
          editorRef={editorRef}
          source={source}
          selection={selection}
          language={language}
          onConfigure={onConfigure}
        />
      )}
      {compactSymbols && shown && railStyle === 'symbols' ? null : <ActionToolbar
        editorRef={editorRef}
        source={source}
        selection={selection}
        onDone={onDismissKeyboard}
      >
        {voiceButton}
      </ActionToolbar>}
    </>
  );
}
