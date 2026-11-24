import { IconButton, Text } from '@lib/tamagui-design-system';
import { Check, Pencil, Plus } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import React from 'react';
import { Pressable, ScrollView, StyleSheet, View, type LayoutChangeEvent, type ScrollViewInstance } from 'react-native';

import { chipScrollTarget } from '../../domain/console/chipScroll';
import type { ConsoleCase } from '../../domain/console/testcases';

type Props = {
  cases: readonly ConsoleCase[];
  selectedId: string | null;
  atCap: boolean;
  neutral?: boolean;
  darkGlass?: boolean;
  editing?: boolean;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onEdit: () => void;
  onRemove?: (id: string) => void;
};

const FLOATING_TRAILING_GUTTER = 16;
const EDIT_TRAILING_GUTTER = 28;
const DARK_GLASS_ACTION = { bg: '#252427' } as const;
const FLOATING_CHIP_TEXT = { fontSize: 14.5, lineHeight: 18, fontWeight: '600', y: 1 } as const;

/** One chip per case in a plain horizontal scroller (the sheet's one registered scrollable is its body), with + and the pencil pinned at the right. */
export function CaseChips({ cases, selectedId, atCap, neutral = false, darkGlass = false, editing = false, onSelect, onAdd, onEdit, onRemove }: Props) {
  const scrollRef = React.useRef<ScrollViewInstance>(null);
  const viewport = React.useRef(0);
  const scrollX = React.useRef(0);
  const positions = React.useRef(new Map<string, { x: number; width: number }>());
  const activeColor = darkGlass ? '#262428' : neutral ? '$color6' : '$color9';
  const inactiveColor = darkGlass ? '#1a1b1d' : '$color4';
  const trailingGutter = editing ? EDIT_TRAILING_GUTTER : neutral ? FLOATING_TRAILING_GUTTER : undefined;

  const reveal = React.useCallback((chip: { x: number; width: number }) => {
    const target = chipScrollTarget(chip, viewport.current, scrollX.current, trailingGutter);
    if (target !== scrollX.current) scrollRef.current?.scrollTo({ x: target, animated: true });
  }, [trailingGutter]);

  const revealSelected = React.useCallback(() => {
    const chip = selectedId ? positions.current.get(selectedId) : undefined;
    if (chip) reveal(chip);
  }, [selectedId, reveal]);

  React.useEffect(revealSelected, [cases.length, revealSelected]);

  return (
    <XStack items="center" gap={neutral ? '$1' : '$2'} testID="ConsoleChips">
      <ScrollView
        ref={scrollRef}
        horizontal
        showsHorizontalScrollIndicator={false}
        style={{ flex: 1 }}
        contentContainerStyle={{ gap: neutral ? 10 : 8, paddingTop: editing ? 6 : 2, paddingRight: trailingGutter ?? 0, paddingBottom: 2 }}
        onLayout={(event: LayoutChangeEvent) => {
          viewport.current = event.nativeEvent.layout.width;
          revealSelected();
        }}
        onContentSizeChange={revealSelected}
        onScroll={(event) => {
          scrollX.current = event.nativeEvent.contentOffset.x;
        }}
        scrollEventThrottle={16}
        role="tablist"
        testID="ConsoleChipScroll"
      >
        {cases.map((entry, index) => {
          const selected = entry.id === selectedId;
          return (
            <View
              key={entry.id}
              onLayout={(event: LayoutChangeEvent) => {
                const { x, width } = event.nativeEvent.layout;
                positions.current.set(entry.id, { x, width });
                if (selected) reveal({ x, width });
              }}
            >
              <Pressable
                role="tab"
                aria-selected={selected}
                accessibilityLabel={entry.label}
                testID={`ConsoleChip-${index + 1}`}
                onPress={() => onSelect(entry.id)}
              >
                <XStack
                  px={neutral ? 13 : '$3'} py={neutral ? 8 : '$2'} rounded="$10"
                  bg={selected ? activeColor : inactiveColor}
                >
                  <Text variant="small" color={selected && !neutral ? '$color1' : '$color'} opacity={darkGlass && !selected ? 0.5 : undefined} {...(neutral ? FLOATING_CHIP_TEXT : {})}>
                    {entry.label}
                  </Text>
                </XStack>
              </Pressable>
              {onRemove ? (
                <Pressable
                  testID={`ConsoleChipRemove-${index + 1}`}
                  accessibilityRole="button"
                  accessibilityLabel={`Remove ${entry.label}`}
                  hitSlop={4}
                  onPress={() => onRemove(entry.id)}
                  style={styles.remove}
                >
                  <View pointerEvents="none" style={styles.minus} />
                </Pressable>
              ) : null}
            </View>
          );
        })}
      </ScrollView>
      <IconButton testID="ConsoleChipAdd" icon={Plus} iconSize={neutral ? 15 : undefined} iconSystemSymbol={neutral ? 'plus' : undefined} variant="secondary" {...(darkGlass ? DARK_GLASS_ACTION : {})} y={neutral ? 1 : undefined} size="$3" pressLog="off" disabled={atCap} accessibilityLabel="Add a case" onPress={onAdd} />
      <IconButton testID="ConsoleChipEdit" icon={editing ? Check : Pencil} iconSize={neutral ? 15 : undefined} iconSystemSymbol={neutral ? (editing ? 'checkmark' : 'pencil') : undefined} variant="secondary" {...(darkGlass ? DARK_GLASS_ACTION : {})} y={neutral ? 1 : undefined} size="$3" pressLog="off" accessibilityLabel={editing ? "Done editing test cases" : "Edit this case"} onPress={onEdit} />
    </XStack>
  );
}

const styles = StyleSheet.create({
  remove: {
    position: 'absolute',
    top: -5,
    right: -9,
    width: 22,
    height: 20,
    borderRadius: 10,
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: '#2b2b2f',
  },
  minus: { width: 13, height: 2, borderRadius: 1, backgroundColor: '#f5f5f7' },
});
