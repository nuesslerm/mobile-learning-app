import { IconButton, Text } from '@lib/tamagui-design-system';
import { Pencil, Plus } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import React from 'react';
import { Pressable, ScrollView, type LayoutChangeEvent, type ScrollViewInstance } from 'react-native';

import { chipScrollTarget } from '../../domain/console/chipScroll';
import type { ConsoleCase } from '../../domain/console/testcases';

type Props = {
  cases: readonly ConsoleCase[];
  selectedId: string | null;
  atCap: boolean;
  neutral?: boolean;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onEdit: () => void;
};

/** One chip per case in a plain horizontal scroller (the sheet's one registered scrollable is its body), with + and the pencil pinned at the right. */
export function CaseChips({ cases, selectedId, atCap, neutral = false, onSelect, onAdd, onEdit }: Props) {
  const scrollRef = React.useRef<ScrollViewInstance>(null);
  const viewport = React.useRef(0);
  const scrollX = React.useRef(0);
  const positions = React.useRef(new Map<string, { x: number; width: number }>());

  React.useEffect(() => {
    const chip = selectedId ? positions.current.get(selectedId) : undefined;
    if (!chip) return;
    const target = chipScrollTarget(chip, viewport.current, scrollX.current);
    if (target !== scrollX.current) scrollRef.current?.scrollTo({ x: target, animated: true });
  }, [selectedId, cases.length]);

  return (
    <XStack items="center" gap="$2" testID="ConsoleChips">
      <ScrollView
        ref={scrollRef}
        horizontal
        showsHorizontalScrollIndicator={false}
        style={{ flex: 1 }}
        contentContainerStyle={{ gap: 8, paddingVertical: 2 }}
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
            <Pressable
              key={entry.id}
              role="tab"
              aria-selected={selected}
              accessibilityLabel={entry.label}
              testID={`ConsoleChip-${index + 1}`}
              onLayout={(event: LayoutChangeEvent) => {
                const { x, width } = event.nativeEvent.layout;
                positions.current.set(entry.id, { x, width });
              }}
              onPress={() => onSelect(entry.id)}
            >
              <XStack px="$3" py="$2" rounded="$10" bg={selected ? (neutral ? '$color6' : '$color9') : '$color4'}>
                <Text variant="small" color={selected && !neutral ? '$color1' : '$color'}>
                  {entry.label}
                </Text>
              </XStack>
            </Pressable>
          );
        })}
      </ScrollView>
      <IconButton testID="ConsoleChipAdd" icon={Plus} iconSize={neutral ? 14 : undefined} variant="secondary" size="$3" pressLog="off" disabled={atCap} accessibilityLabel="Add a case" onPress={onAdd} />
      <IconButton testID="ConsoleChipEdit" icon={Pencil} iconSize={neutral ? 14 : undefined} variant="secondary" size="$3" pressLog="off" accessibilityLabel="Edit this case" onPress={onEdit} />
    </XStack>
  );
}
