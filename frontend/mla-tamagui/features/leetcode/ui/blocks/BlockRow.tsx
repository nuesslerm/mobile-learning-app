import { Icon, Text, View } from '@lib/tamagui-design-system';
import { GripVertical } from '@lib/tamagui-design-system/icons';
import { XStack } from '@lib/tamagui-design-system/primitives';
import React from 'react';
import { GestureDetector } from 'react-native-gesture-handler';

import { GUTTER, INDENT_STEP, ROW_HEIGHT, rowTop } from '../../domain/blocks/blockLayout';
import type { BlockRow as BlockRowModel } from '../../domain/blocks/blockModel';
import type { DragSource } from '../../domain/blocks/dropPlan';
import { useBlockDrag, type DragShell } from './useBlockDrag';

/** What a screen reader says for a row: the line and its level, or that it is plain text. */
export const rowDescription = (row: BlockRowModel): string => (row.kind === 'text' ? `${row.label}, plain text` : `${row.label}, level ${row.depth + 1}`);

/** The grip of a row that can move: a drag from it picks the whole statement up. */
function Grip({ row, shell }: { row: BlockRowModel; shell: DragShell }) {
  const source = React.useMemo<DragSource>(() => ({ kind: 'block', id: row.id }), [row.id]);
  const origin = React.useMemo(() => ({ x: 0, y: rowTop(row.index) }), [row.index]);
  const pan = useBlockDrag(source, origin, shell);
  return (
    <GestureDetector gesture={pan}>
      <View width={GUTTER} height={ROW_HEIGHT} items="center" justify="center" aria-label={`Move ${row.label}`} testID={`BlockHandle-${row.index}`}>
        <Icon as={GripVertical} size={18} color="$color11" />
      </View>
    </GestureDetector>
  );
}

type Props = { row: BlockRowModel; shell: DragShell };

/** One statement of the code as a row of a fixed height at its row's place: the gutter holds the grip, the line is indented by its depth. */
export const BlockRow = React.memo(function BlockRow({ row, shell }: Props) {
  const quiet = row.kind === 'text' || row.kind === 'clause';
  return (
    <XStack position="absolute" l={0} r={0} t={rowTop(row.index)} height={ROW_HEIGHT} items="center" borderBottomWidth={1} borderColor="$borderColor" aria-label={rowDescription(row)} testID={`BlockRow-${row.index}`}>
      {row.movable ? <Grip row={row} shell={shell} /> : <View width={GUTTER} />}
      <View flex={1} ml={row.depth * INDENT_STEP} pr="$2">
        <Text variant="code" numberOfLines={1} color={quiet ? '$color11' : '$color'}>
          {row.label}
        </Text>
      </View>
    </XStack>
  );
});
