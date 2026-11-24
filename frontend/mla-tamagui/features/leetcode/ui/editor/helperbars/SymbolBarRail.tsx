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
import { Keyboard, Pressable, StyleSheet } from 'react-native';

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
const COMPACT_KEY_HEIGHT = 41;
const COMPACT_CELL_WIDTH = 13;
const COMPACT_CELL_HEIGHT = 17;
const CANVAS_KEY = '#08070a';
const CANVAS_LABEL = '#f4f4f5';

type Props = {
  editorRef: React.RefObject<EditorInputHandle | null>;
  source: string;
  selection: Selection;
  language: Language;
  onDismissKeyboard: () => void;
  compact?: boolean;
  backgroundColor?: string;
};

function SymbolGroupBlock({
  group,
  onExpand,
  onType,
  canvas,
}: {
  group: SymbolGroup;
  onExpand?: () => void;
  canvas: boolean;
  onType: (group: SymbolGroup, index: number, char: string) => void;
}) {
  const compact = onExpand !== undefined;
  const testID = `SymbolBarGroup-${group.id}`;
  const content = (
    <YStack
      testID={compact ? undefined : testID}
      rounded="$3"
      height={compact ? COMPACT_KEY_HEIGHT : undefined}
      justify={compact ? "center" : undefined}
      bg="$color3"
      overflow="hidden"
      pointerEvents={compact ? "none" : undefined}
      style={canvas ? styles.canvasKey : undefined}
    >
      {groupCells(group.chars).map((cells, row) => (
        <XStack key={row} justify={compact ? "center" : undefined}>
          {cells.map((char, column) => {
            const index = row * 2 + column;
            return (
              <XStack
                key={index}
                testID={`SymbolBarKey-${group.id}-${index}`}
                {...(!compact ? { role: 'button', 'aria-label': `Type ${char}`, onPress: () => onType(group, index, char) } : {})}
                width={compact ? COMPACT_CELL_WIDTH : CELL_WIDTH}
                height={compact ? COMPACT_CELL_HEIGHT : CELL_HEIGHT}
                items="center"
                justify="center"
                pressStyle={compact ? undefined : { bg: '$color5' }}
                cursor={compact ? undefined : "pointer"}
              >
                <Text variant="code" fontSize={compact ? 12 : undefined} style={canvas ? styles.canvasGlyph : undefined}>{char}</Text>
              </XStack>
            );
          })}
        </XStack>
      ))}
    </YStack>
  );
  if (!onExpand) return content;
  return (
    <Pressable testID={testID} accessibilityRole="button" accessibilityLabel={`Show symbols ${group.chars}`} onPress={onExpand} style={styles.compactGroup}>
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
  backgroundColor,
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
  const canvas = compact && backgroundColor !== undefined;
  const railProps = compact ? {
    density: 'compact' as const,
    style: canvas ? { backgroundColor, borderTopWidth: 0 } : undefined,
    contentContainerStyle: styles.compactRail,
  } : {};
  const keyProps = compact ? {
    density: 'compact' as const,
    stretch: true,
    style: canvas ? styles.canvasButton : styles.compactButton,
    labelStyle: canvas ? styles.canvasLabel : styles.compactLabel,
    iconColor: canvas ? CANVAS_LABEL : undefined,
  } : {};
  const expanded = compact ? shown.find((group) => group.id === expandedId) : undefined;
  const typeSymbol = (group: SymbolGroup, index: number, char: string) => {
    Logger.info('symbol_bar.key_press', { group: group.id, index, char });
    editorRef.current?.typeText(char);
    editorRef.current?.focus();
    setExpandedId(null);
  };

  if (expanded) {
    return (
      <KeyboardToolbarRail testID="EditorExpandedSymbols" {...railProps} contentContainerStyle={styles.expandedRail}>
        {Array.from(expanded.chars).map((char, index) => (
          <KeyboardToolbarButton
            key={index}
            testID={`SymbolBarKey-${expanded.id}-${index}`}
            accessibilityLabel={`Type ${char}`}
            label={char}
            {...keyProps}
            onPress={() => typeSymbol(expanded, index, char)}
          />
        ))}
        <KeyboardToolbarButton
          testID="SymbolBarCloseGroup"
          accessibilityLabel="Close symbol group"
          icon={X}
          systemSymbol="xmark"
          {...keyProps}
          onPress={() => setExpandedId(null)}
        />
      </KeyboardToolbarRail>
    );
  }

  return (
    <KeyboardToolbarRail testID="EditorSymbolBar" {...railProps}>
      <KeyboardToolbarButton
        testID="SymbolBarTab"
        accessibilityLabel={tab.name}
        label={compact ? undefined : "Tab"}
        {...keyProps}
        icon={tab.icon}
        systemSymbol={compact ? "arrow.right.to.line" : undefined}
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
          canvas={canvas}
        />
      ))}
      <KeyboardToolbarButton
        testID="SymbolBarDismiss"
        accessibilityLabel="Hide keyboard"
        label={compact ? undefined : "Hide"}
        {...keyProps}
        style={canvas ? { ...styles.canvasButton, backgroundColor: 'transparent' } : keyProps.style}
        icon={compact ? KeyboardIcon : ChevronDown}
        iconSize={compact ? 18 : undefined}
        systemSymbol={compact ? "keyboard.chevron.compact.down" : undefined}
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


const styles = StyleSheet.create({
  compactRail: { gap: 5, paddingLeft: 8, paddingRight: 0, paddingTop: 3, paddingBottom: 7 },
  expandedRail: { gap: 6, paddingHorizontal: 4, paddingTop: 3, paddingBottom: 7 },
  compactGroup: { flex: 1, minWidth: 34 },
  compactButton: { minWidth: 34, paddingHorizontal: 0, height: COMPACT_KEY_HEIGHT, borderRadius: 5 },
  canvasKey: { backgroundColor: CANVAS_KEY, borderRadius: 5 },
  canvasGlyph: { fontFamily: 'Menlo', fontWeight: '600', color: CANVAS_LABEL },
  compactLabel: { fontSize: 17, fontWeight: '600' },
  canvasLabel: { fontSize: 17, fontWeight: '600', color: CANVAS_LABEL },
  canvasButton: { minWidth: 34, paddingHorizontal: 0, height: COMPACT_KEY_HEIGHT, borderRadius: 5, backgroundColor: CANVAS_KEY },
});
