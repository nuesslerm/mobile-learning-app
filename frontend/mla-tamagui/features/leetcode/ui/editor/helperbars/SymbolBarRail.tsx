import type { Language } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';
import {
  KeyboardToolbarButton,
  KeyboardToolbarRail,
  Text,
} from '@lib/tamagui-design-system';
import { ChevronDown, Keyboard as KeyboardIcon, X } from '@lib/tamagui-design-system/icons';
import { XStack, YStack } from '@lib/tamagui-design-system/primitives';
import React, { useMemo, useState } from 'react';
import { Keyboard, Pressable } from 'react-native';

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
const COMPACT_CELL_SIZE = 18;

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
  source: string;
  selection: Selection;
  language: Language;
  onDismissKeyboard: () => void;
  compact?: boolean;
};

function SymbolGroupBlock({
  group,
  onExpand,
  onType,
}: {
  group: SymbolGroup;
  onExpand?: () => void;
  onType: (group: SymbolGroup, index: number, char: string) => void;
}) {
  const compact = onExpand !== undefined;
  const testID = `SymbolBarGroup-${group.id}`;
  const content = (
    <YStack
      testID={compact ? undefined : testID}
      rounded="$3"
      bg="$color3"
      overflow="hidden"
      pointerEvents={compact ? "none" : undefined}
    >
      {groupCells(group.chars).map((cells, row) => (
        <XStack key={row}>
          {cells.map((char, column) => {
            const index = row * 2 + column;
            return (
              <XStack
                key={index}
                testID={`SymbolBarKey-${group.id}-${index}`}
                {...(!compact ? { role: 'button', 'aria-label': `Type ${char}`, onPress: () => onType(group, index, char) } : {})}
                width={compact ? COMPACT_CELL_SIZE : CELL_WIDTH}
                height={compact ? COMPACT_CELL_SIZE : CELL_HEIGHT}
                items="center"
                justify="center"
                pressStyle={compact ? undefined : { bg: '$color5' }}
                cursor={compact ? undefined : "pointer"}
              >
                <Text variant="code" fontSize={compact ? 11 : undefined}>{char}</Text>
              </XStack>
            );
          })}
        </XStack>
      ))}
    </YStack>
  );
  if (!onExpand) return content;
  return (
    <Pressable testID={testID} accessibilityRole="button" accessibilityLabel={`Show symbols ${group.chars}`} onPress={onExpand} style={{ flexGrow: 1 }}>
      {content}
    </Pressable>
  );
}

/** The recording's symbol bar: Tab, the groups in the person's order as 2x2 blocks, and a key that puts the keyboard away. Every character is typed, so brackets close themselves, a closer is stepped over and a selection is wrapped. */
export function SymbolBarRail({
  editorRef,
  source,
  selection,
  language,
  onDismissKeyboard,
  compact = false,
}: Props) {
  const groups = useSymbolBarStore((s) => s.groups);
  const tabSize = useEditorPreferencesStore((s) => s.tabSize);
  const shown = useMemo(() => visibleGroups(groups), [groups]);
  const ctx: KeyActionContext = useMemo(
    () => ({ editorRef, text: source, selection, language, indentUnit: ' '.repeat(tabSize) }),
    [editorRef, selection, source, language, tabSize]
  );
  const tab = BUILTIN_KEYS.Tab;
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const expanded = compact ? shown.find((group) => group.id === expandedId) : undefined;
  const typeSymbol = (group: SymbolGroup, index: number, char: string) => {
    Logger.info('symbol_bar.key_press', { group: group.id, index, char });
    editorRef.current?.typeText(char);
    editorRef.current?.focus();
    setExpandedId(null);
  };

  if (expanded) {
    return (
      <KeyboardToolbarRail testID="EditorExpandedSymbols" density="compact">
        {Array.from(expanded.chars).map((char, index) => (
          <KeyboardToolbarButton
            key={index}
            testID={`SymbolBarKey-${expanded.id}-${index}`}
            accessibilityLabel={`Type ${char}`}
            label={char}
            density="compact"
            stretch
            onPress={() => typeSymbol(expanded, index, char)}
          />
        ))}
        <KeyboardToolbarButton
          testID="SymbolBarCloseGroup"
          accessibilityLabel="Close symbol group"
          icon={X}
          density="compact"
          onPress={() => setExpandedId(null)}
        />
      </KeyboardToolbarRail>
    );
  }

  return (
    <KeyboardToolbarRail testID="EditorSymbolBar" density={compact ? "compact" : "default"}>
      <KeyboardToolbarButton
        testID="SymbolBarTab"
        accessibilityLabel={tab.name}
        label={compact ? undefined : "Tab"}
        density={compact ? "compact" : "default"}
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
          onExpand={compact ? () => setExpandedId(group.id) : undefined}
          onType={typeSymbol}
        />
      ))}
      <KeyboardToolbarButton
        testID="SymbolBarDismiss"
        accessibilityLabel="Hide keyboard"
        label={compact ? undefined : "Hide"}
        density={compact ? "compact" : "default"}
        icon={compact ? KeyboardIcon : ChevronDown}
        tone={compact ? "default" : "accent"}
        onPress={() => {
          Logger.info('symbol_bar.dismiss_press', {});
          Keyboard.dismiss();
          onDismissKeyboard();
        }}
      />
    </KeyboardToolbarRail>
  );
}
