import { Text, View } from '@lib/tamagui-design-system';
import { XStack } from '@lib/tamagui-design-system/primitives';
import React from 'react';
import { GestureDetector } from 'react-native-gesture-handler';

import { CHIP_WIDTH, chipOrigin, PALETTE_HEIGHT } from '../../domain/blocks/blockLayout';
import type { DragSource } from '../../domain/blocks/dropPlan';
import { PYTHON_PALETTE, type PaletteItem } from '../../domain/blocks/palette';
import { useBlockDrag, type DragShell } from './useBlockDrag';

/** One chip: a drag from it picks up a new statement, which a drop inserts at the slot under the finger. */
function Chip({ item, index, shell }: { item: PaletteItem; index: number; shell: DragShell }) {
  const source = React.useMemo<DragSource>(() => ({ kind: 'palette', item: item.id }), [item.id]);
  const origin = React.useMemo(() => chipOrigin(index), [index]);
  const pan = useBlockDrag(source, origin, shell);
  return (
    <GestureDetector gesture={pan}>
      <View width={CHIP_WIDTH} height={PALETTE_HEIGHT} items="center" justify="center" aria-label={`Add ${item.label}`} testID={`BlockPaletteItem-${item.id}`}>
        <View px="$2.5" py="$1" rounded="$4" borderWidth={1} borderColor="$borderColor" bg="$color3">
          <Text variant="code">{item.label}</Text>
        </View>
      </View>
    </GestureDetector>
  );
}

/** The statements a person can add, in a row of a fixed height right above the rows. */
export function BlockPalette({ shell }: { shell: DragShell }) {
  return (
    <XStack height={PALETTE_HEIGHT} items="center" borderBottomWidth={1} borderColor="$borderColor" testID="BlockPalette">
      {PYTHON_PALETTE.map((item, index) => (
        <Chip key={item.id} item={item} index={index} shell={shell} />
      ))}
    </XStack>
  );
}
