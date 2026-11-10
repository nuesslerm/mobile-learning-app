import { toolbarItemKey } from '../../domain';
import { Logger } from '@lib/logging';
import {
  KeyboardToolbarButton,
  KeyboardToolbarRail,
} from '@lib/tamagui-design-system';
import { Settings2 } from '@lib/tamagui-design-system/icons';
import React, { useMemo } from 'react';

import { useToolbarConfigStore } from '../../state/toolbarConfigStore';
import type { EditorInputHandle } from '../core/EditorInput';
import { BUILTIN_KEYS, type KeyActionContext } from '../core/toolbarKeys';
import type { Selection } from '../core/types';

const ACTION_KEY_IDS = new Set(['Undo', 'Redo', 'RenameSymbol']);

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
  source: string;
  selection: Selection;
  onConfigure: () => void;
  onRenameSymbol?: (symbolName: string) => void;
};

export function SyntaxToolbar({
  editorRef,
  source,
  selection,
  onConfigure,
  onRenameSymbol,
}: Props) {
  const items = useToolbarConfigStore((s) => s.items);
  const ctx: KeyActionContext = useMemo(
    () => ({ editorRef, text: source, selection, onRenameSymbol }),
    [editorRef, onRenameSymbol, selection, source]
  );

  return (
    <KeyboardToolbarRail testID="EditorSyntaxToolbar">
      {items.map((item) => {
        if (item.kind === 'builtin') {
          if (ACTION_KEY_IDS.has(item.id)) return null;
          const def = BUILTIN_KEYS[item.id];
          return (
            <KeyboardToolbarButton
              key={toolbarItemKey(item)}
              testID={`EditorSyntax-${item.id}`}
              accessibilityLabel={def.name}
              label={def.label ?? def.name}
              icon={def.icon}
              onPress={() => {
                Logger.info('toolbar.syntax_press', { id: item.id });
                def.run(ctx);
                editorRef.current?.focus();
              }}
            />
          );
        }

        return (
          <KeyboardToolbarButton
            key={toolbarItemKey(item)}
            testID="EditorSyntax-Custom"
            accessibilityLabel={`Insert ${item.label}`}
            label={item.label}
            onPress={() => {
              Logger.info('toolbar.custom_press', {
                id: item.id,
                insertLength: item.insert.length,
              });
              editorRef.current?.insertAtCursor(item.insert, item.cursorOffset);
              editorRef.current?.focus();
            }}
          />
        );
      })}
      <KeyboardToolbarButton
        testID="EditorSyntax-Configure"
        accessibilityLabel="Configure editor toolbar"
        label="Configure"
        icon={Settings2}
        tone="accent"
        onPress={() => {
          Logger.info('toolbar.configure_press', { itemCount: items.length });
          onConfigure();
        }}
      />
    </KeyboardToolbarRail>
  );
}
