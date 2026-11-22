import { Text, View } from '@lib/tamagui-design-system';

import { depthX, gapY } from '../../domain/blocks/blockLayout';
import type { DropPlan } from '../../domain/blocks/dropPlan';

/** The line where a drop would land, at the row boundary and the depth of the slot, and in words where that is: 'Move after if x:'. */
export function BlockDropIndicator({ plan }: { plan: DropPlan }) {
  const { gap, depth } = plan.slot;
  return (
    <View position="absolute" l={0} r={0} t={gapY(gap) - 2} height={4} pointerEvents="none" aria-label={plan.text} testID="BlockDropIndicator">
      <View position="absolute" l={depthX(depth)} r={0} t={0} height={4} rounded="$1" bg="$blue9" />
      <View position="absolute" r="$2" t={gap === 0 ? 8 : -26} px="$2" py="$1" rounded="$2" bg="$blue9">
        <Text variant="small" color="$color1" numberOfLines={1} testID="BlockDropIndicatorText">
          {plan.text}
        </Text>
      </View>
    </View>
  );
}
