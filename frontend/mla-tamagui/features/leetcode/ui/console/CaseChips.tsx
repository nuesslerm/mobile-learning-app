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
  editing?: boolean;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onEdit: () => void;
  onRemove?: (id: string) => void;
};

/** One chip per case in a plain horizontal scroller (the sheet's one registered scrollable is its body), with + and the pencil pinned at the right. */
export function CaseChips({ cases, selectedId, atCap, neutral = false, editing = false, onSelect, onAdd, onEdit, onRemove }: Props) {
  const scrollRef = React.useRef<ScrollViewInstance>(null);
  const viewport = React.useRef(0);
  const scrollX = React.useRef(0);
  const positions = React.useRef(new Map<string, { x: number; width: number }>());

  const reveal = React.useCallback((chip: { x: number; width: number }) => {
    const target = chipScrollTarget(chip, viewport.current, scrollX.current);
    if (target !== scrollX.current) scrollRef.current?.scrollTo({ x: target, animated: true });
  }, []);

  React.useEffect(() => {
    const chip = selectedId ? positions.current.get(selectedId) : undefined;
    if (chip) reveal(chip);
  }, [selectedId, cases.length, reveal]);

  return (
    <XStack items="center" gap="$2" testID="ConsoleChips">
      <ScrollView
        ref={scrollRef}
        horizontal
        showsHorizontalScrollIndicator={false}
        style={{ flex: 1 }}
        contentContainerStyle={{ gap: neutral ? 12 : 8, paddingLeft: neutral ? 4 : 0, paddingTop: editing ? 6 : 2, paddingRight: editing ? 12 : 0, paddingBottom: 2 }}
        onLayout={(event: LayoutChangeEvent) => {
          viewport.current = event.nativeEvent.layout.width;
        }}
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
                <XStack px="$3" py="$2" rounded="$10" bg={selected ? (neutral ? '$color6' : '$color9') : '$color4'}>
                  <Text variant="small" color={selected && !neutral ? '$color1' : '$color'}>
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
      <IconButton testID="ConsoleChipAdd" icon={Plus} iconSize={neutral ? 14 : undefined} variant="secondary" size="$3" pressLog="off" disabled={atCap} accessibilityLabel="Add a case" onPress={onAdd} />
      <IconButton testID="ConsoleChipEdit" icon={editing ? Check : Pencil} iconSize={neutral ? 14 : undefined} variant="secondary" size="$3" pressLog="off" accessibilityLabel={editing ? "Done editing test cases" : "Edit this case"} onPress={onEdit} />
    </XStack>
  );
}

const styles = StyleSheet.create({
  remove: {
    position: 'absolute',
    top: -5,
    right: -12,
    width: 22,
    height: 20,
    borderRadius: 10,
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: '#303034',
  },
  minus: { width: 12, height: 2, borderRadius: 1, backgroundColor: '#f5f5f7' },
});
