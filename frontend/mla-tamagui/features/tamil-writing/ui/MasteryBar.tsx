import { View } from '@lib/tamagui-design-system';
import { XStack } from '@lib/tamagui-design-system/primitives';

export type MasteryBarProps = {
  /** 0..1. Values outside the range are clamped rather than overflowing. */
  ratio: number;
  testID: string;
};

const BAR_HEIGHT = 6;

/**
 * A progress bar the design system does not ship, kept local to this feature.
 *
 * The fill is sized by flex rather than a percentage width: Tamagui drops a
 * `width="0%"` outright, so an empty bar rendered as a full one -- and this
 * app's `$color4` is the accent blue, so the track alone looked like 100%.
 */
export function MasteryBar({ ratio, testID }: MasteryBarProps): React.ReactElement {
  const clamped = Math.min(Math.max(Number.isFinite(ratio) ? ratio : 0, 0), 1);
  return (
    <XStack
      testID={testID}
      height={BAR_HEIGHT}
      rounded={9999}
      overflow="hidden"
      bg="$color3"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(clamped * 100)}
    >
      <View testID={`${testID}Fill`} flex={clamped} bg="$green9" />
      <View testID={`${testID}Track`} flex={1 - clamped} />
    </XStack>
  );
}
