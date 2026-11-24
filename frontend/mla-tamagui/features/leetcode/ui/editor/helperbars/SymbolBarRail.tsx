import type { Language } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';
import {
  KeyboardToolbarButton,
  KeyboardToolbarRail,
  Text,
} from '@lib/tamagui-design-system';
import { ChevronDown } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import React, { useMemo } from 'react';
import { Keyboard } from 'react-native';

import {
  groupCells,
  visibleGroups,
  type SymbolGroup,
} from '../../../domain/editor-settings/symbolGroups';
import type { Selection } from '../../../domain/editor/types';
import { useEditorPreferencesStore } from '../../../state/editor/editorPreferencesStore';
import { useSymbolBarStore } from '../../../state/editor/symbolBarStore';
import type { EditorInputHandle } from '../EditorInput';
import { BUILTIN_KEYS, type KeyActionContext } from '../toolbarKeys';

const CELL_WIDTH = 30;
const CELL_HEIGHT = 24;

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
  source: string;
  selection: Selection;
  language: Language;
  onDismissKeyboard: () => void;
};

function SymbolGroupBlock({
  group,
  onType,
}: {
  group: SymbolGroup;
  onType: (group: SymbolGroup, index: number, char: string) => void;
}) {
  return (
    <YStack
      testID={`SymbolBarGroup-${group.id}`}
      rounded="$3"
      bg="$color3"
      overflow="hidden"
    >
      {groupCells(group.chars).map((cells, row) => (
        <XStack key={row}>
          {cells.map((char, column) => {
            const index = row * 2 + column;
            return (
              <XStack
                key={index}
                testID={`SymbolBarKey-${group.id}-${index}`}
                role="button"
                aria-label={`Type ${char}`}
                width={CELL_WIDTH}
                height={CELL_HEIGHT}
                items="center"
                justify="center"
                pressStyle={{ bg: '$color5' }}
                cursor="pointer"
                onPress={() => onType(group, index, char)}
              >
                <Text variant="code">{char}</Text>
              </XStack>
            );
          })}
        </XStack>
      ))}
    </YStack>
  );
}

/** The recording's symbol bar: Tab, the groups in the person's order as 2x2 blocks, and a key that puts the keyboard away. Every character is typed, so brackets close themselves, a closer is stepped over and a selection is wrapped. */
export function SymbolBarRail({
  editorRef,
  source,
  selection,
  language,
  onDismissKeyboard,
}: Props) {
  const groups = useSymbolBarStore((s) => s.groups);
  const tabSize = useEditorPreferencesStore((s) => s.tabSize);
  const shown = useMemo(() => visibleGroups(groups), [groups]);
  const ctx: KeyActionContext = useMemo(
    () => ({ editorRef, text: source, selection, language, indentUnit: ' '.repeat(tabSize) }),
    [editorRef, selection, source, language, tabSize]
  );
  const tab = BUILTIN_KEYS.Tab;

  return (
    <KeyboardToolbarRail testID="EditorSymbolBar">
      <KeyboardToolbarButton
        testID="SymbolBarTab"
        accessibilityLabel={tab.name}
        label="Tab"
        icon={tab.icon}
        onPress={() => {
          Logger.info('symbol_bar.tab_press', {});
          tab.run(ctx);
          editorRef.current?.focus();
        }}
      />
      {shown.map((group) => (
        <SymbolGroupBlock
          key={group.id}
          group={group}
          onType={(pressed, index, char) => {
            Logger.info('symbol_bar.key_press', { group: pressed.id, index, char });
            editorRef.current?.typeText(char);
            editorRef.current?.focus();
          }}
        />
      ))}
      <KeyboardToolbarButton
        testID="SymbolBarDismiss"
        accessibilityLabel="Hide keyboard"
        label="Hide"
        icon={ChevronDown}
        tone="accent"
        onPress={() => {
          Logger.info('symbol_bar.dismiss_press', {});
          Keyboard.dismiss();
          onDismissKeyboard();
        }}
      />
    </KeyboardToolbarRail>
  );
}
